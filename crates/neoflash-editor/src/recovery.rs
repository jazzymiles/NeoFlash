//! Autosave timer and crash-recovery storage (`Preferences ▸ General ▸ Autosave`).
//!
//! Recovery snapshots live exclusively under `<app_support>/Recovery/<stable_id>.nflash` paired
//! with a `<stable_id>.json` metadata sidecar, never next to the user's authored file.
use crate::app::Editor;
use eframe::egui;
use neoflash_core::Project;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static UNTITLED_SEQ: AtomicU64 = AtomicU64::new(1);

const EXTERNAL_FILE_POLL_INTERVAL_SECS: f64 = 1.0;
const MAX_DISK_SCAN_ENTRIES: usize = 1_000_000;
const MAX_DISK_SCAN_BYTES: u64 = 64 * 1024 * 1024 * 1024;

/// Kind of external on-disk change detected for an open document tab (`T-200`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalFileChange {
    /// File was modified in place on disk (same identity, different content).
    Modified,
    /// File was atomically replaced on disk (new inode/device or recreated with different content).
    Replaced,
    /// File or directory was deleted or moved away from its tracked path on disk.
    Deleted,
}

impl ExternalFileChange {
    pub fn label(self) -> &'static str {
        match self {
            Self::Modified => "modified on disk",
            Self::Replaced => "replaced on disk",
            Self::Deleted => "deleted or moved on disk",
        }
    }

    pub fn dialog_title(self) -> &'static str {
        match self {
            Self::Modified => "File modified on disk",
            Self::Replaced => "File replaced on disk",
            Self::Deleted => "File deleted on disk",
        }
    }

    pub fn summary(self, document_name: &str, dirty: bool) -> String {
        match (self, dirty) {
            (Self::Modified, true) => format!(
                "“{document_name}” was modified on disk by another program while you have unsaved changes in NeoFlash. Reload from disk, keep editing your current version, or Save As a new file."
            ),
            (Self::Modified, false) => format!(
                "“{document_name}” was modified on disk by another program. Reload the updated file from disk, keep the version currently open in NeoFlash, or Save As a new file."
            ),
            (Self::Replaced, true) => format!(
                "“{document_name}” was replaced on disk while you have unsaved changes in NeoFlash. Reload the replacement from disk, keep editing your current version, or Save As a new file."
            ),
            (Self::Replaced, false) => format!(
                "“{document_name}” was replaced on disk by another program. Reload the replacement from disk, keep the version currently open in NeoFlash, or Save As a new file."
            ),
            (Self::Deleted, _) => format!(
                "“{document_name}” was deleted or moved on disk. Keep editing in memory to save it again, or Save As a new file."
            ),
        }
    }
}

/// On-disk identity and content fingerprint for an open document path (`T-200`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiskFileStamp {
    pub path: PathBuf,
    pub is_dir: bool,
    pub len: u64,
    pub mtime_secs: u64,
    pub mtime_nanos: u32,
    #[cfg(unix)]
    pub dev: u64,
    #[cfg(unix)]
    pub ino: u64,
    pub content_hash: u64,
    /// False when a package scan hit a safety bound or an entry could not be read. Incomplete
    /// stamps are treated as changed rather than silently accepting an uninspected tree.
    pub scan_complete: bool,
}

/// Cheap on-disk identity/metadata fingerprint used by automatic polling.
///
/// Native `.nflash` files are probed with a single metadata call. XFL packages additionally walk
/// their tree, but never read file contents. A full [`DiskFileStamp`] is captured only when this
/// probe changes (or when a user action such as Save forces a definitive check).
#[derive(Clone, Debug, PartialEq, Eq)]
struct DiskMetadataStamp {
    path: PathBuf,
    is_dir: bool,
    len: u64,
    mtime_secs: u64,
    mtime_nanos: u32,
    #[cfg(unix)]
    dev: u64,
    #[cfg(unix)]
    ino: u64,
    tree_hash: u64,
    scan_complete: bool,
}

impl DiskFileStamp {
    /// Capture a full identity and content stamp for `path` (file or XFL directory).
    pub fn capture(path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        if metadata.is_dir() {
            Self::capture_directory(path, &metadata)
        } else {
            Self::capture_file(path, &metadata)
        }
    }

    fn capture_file(path: &Path, metadata: &std::fs::Metadata) -> Option<Self> {
        let mut scan = DiskTreeScan::new(metadata_mtime(metadata));
        let anchor_name = path.file_name().map(PathBuf::from).unwrap_or_default();
        scan_regular_file(path, &anchor_name, metadata, &mut scan);

        // An uncompressed XFL may be opened through either its `.xfl` marker or directly through
        // `DOMDocument.xml`. Fold the complete LIBRARY tree into both forms so background edits to
        // symbols are not missed while the anchor file itself remains unchanged.
        let is_xfl_anchor = is_xfl_anchor(path);
        if is_xfl_anchor && let Some(parent) = path.parent() {
            let dom = parent.join("DOMDocument.xml");
            if dom != path {
                scan_path_entry(&dom, Path::new("DOMDocument.xml"), &mut scan);
            }
            let lib_dir = parent.join("LIBRARY");
            if lib_dir.is_dir() {
                scan_directory_tree(&lib_dir, Path::new("LIBRARY"), &mut scan);
            }
        }
        let (mtime_secs, mtime_nanos) = scan.max_mtime;
        #[cfg(unix)]
        let (dev, ino) = {
            use std::os::unix::fs::MetadataExt as _;
            (metadata.dev(), metadata.ino())
        };
        Some(Self {
            path: canonical_or_normalized_path(path),
            is_dir: false,
            len: scan.total_len,
            mtime_secs,
            mtime_nanos,
            #[cfg(unix)]
            dev,
            #[cfg(unix)]
            ino,
            content_hash: scan.hash,
            scan_complete: scan.complete,
        })
    }

    fn capture_directory(path: &Path, metadata: &std::fs::Metadata) -> Option<Self> {
        let mut scan = DiskTreeScan::new(metadata_mtime(metadata));
        scan_directory_tree(path, Path::new(""), &mut scan);
        #[cfg(unix)]
        let (dev, ino) = {
            use std::os::unix::fs::MetadataExt as _;
            (metadata.dev(), metadata.ino())
        };
        Some(Self {
            path: canonical_or_normalized_path(path),
            is_dir: true,
            len: scan.total_len,
            mtime_secs: scan.max_mtime.0,
            mtime_nanos: scan.max_mtime.1,
            #[cfg(unix)]
            dev,
            #[cfg(unix)]
            ino,
            content_hash: scan.hash,
            scan_complete: scan.complete,
        })
    }

    /// Compare `self` (baseline) with `current` on-disk stamp and classify any external change.
    pub fn classify_change(&self, current: &Self) -> Option<ExternalFileChange> {
        if self.is_dir != current.is_dir {
            return Some(ExternalFileChange::Replaced);
        }
        #[cfg(unix)]
        let identity_changed = !self.is_dir && (self.dev != current.dev || self.ino != current.ino);
        #[cfg(not(unix))]
        let identity_changed = false;

        if !self.scan_complete || !current.scan_complete {
            return Some(if identity_changed {
                ExternalFileChange::Replaced
            } else {
                ExternalFileChange::Modified
            });
        }

        if self.content_hash != current.content_hash || self.len != current.len {
            if identity_changed {
                Some(ExternalFileChange::Replaced)
            } else {
                Some(ExternalFileChange::Modified)
            }
        } else {
            None
        }
    }
}

impl DiskMetadataStamp {
    fn capture(path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        let mut scan = MetadataTreeScan::new(metadata_mtime(&metadata));
        if metadata.is_dir() {
            scan_metadata_directory_tree(path, Path::new(""), &mut scan);
        } else {
            let anchor_name = path.file_name().map(PathBuf::from).unwrap_or_default();
            scan.record_file(&anchor_name, &metadata);
            if is_xfl_anchor(path)
                && let Some(parent) = path.parent()
            {
                let dom = parent.join("DOMDocument.xml");
                if dom != path {
                    scan_metadata_path_entry(&dom, Path::new("DOMDocument.xml"), &mut scan);
                }
                let library = parent.join("LIBRARY");
                if library.is_dir() {
                    scan_metadata_directory_tree(&library, Path::new("LIBRARY"), &mut scan);
                }
            }
        }
        #[cfg(unix)]
        let (dev, ino) = {
            use std::os::unix::fs::MetadataExt as _;
            (metadata.dev(), metadata.ino())
        };
        Some(Self {
            path: canonical_or_normalized_path(path),
            is_dir: metadata.is_dir(),
            len: scan.total_len,
            mtime_secs: scan.max_mtime.0,
            mtime_nanos: scan.max_mtime.1,
            #[cfg(unix)]
            dev,
            #[cfg(unix)]
            ino,
            tree_hash: scan.hash,
            scan_complete: scan.complete,
        })
    }
}

/// Metadata sidecar written alongside `<stable_id>.nflash`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RecoverySidecar {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) original_path: Option<PathBuf>,
    pub(crate) document_name: String,
    pub(crate) timestamp_unix_secs: u64,
}

/// Parsed crash-recovery snapshot discovered on launch.
#[derive(Clone, Debug)]
pub(crate) struct RecoveryCandidate {
    pub(crate) id: String,
    pub(crate) original_path: Option<PathBuf>,
    pub(crate) document_name: String,
    pub(crate) timestamp_unix_secs: u64,
    pub(crate) project: Project,
}

/// Per-editor autosave timer and crash-recovery directory state.
#[derive(Clone, Debug)]
pub(crate) struct RecoveryManager {
    /// Target recovery directory (`None` in headless tests unless explicitly injected).
    dir: Option<PathBuf>,
    /// Stable filename stem for the currently open document.
    active_id: String,
    /// All recovery stems owned by the current document lifecycle (cleaned up on save/discard/quit).
    tracked_ids: Vec<String>,
    /// Monotonic timestamp (seconds) when the document became dirty or was last autosaved.
    dirty_anchor_secs: Option<f64>,
    /// Crash-recovery snapshots awaiting a user `Restore` / `Discard` decision.
    prompts: VecDeque<RecoveryCandidate>,
    /// Last known on-disk modification timestamp (seconds) for the active document path.
    last_disk_mtime_secs: Option<u64>,
    /// Monotonic editor time of the last automatic external-file fingerprint poll. Explicit Save
    /// guards and user-requested checks bypass this throttle.
    last_disk_poll_secs: Option<f64>,
    /// Baseline on-disk fingerprint captured when the document was opened, imported, saved, or acknowledged.
    disk_stamp: Option<DiskFileStamp>,
    /// Metadata-only companion to `disk_stamp`, used to avoid reading unchanged documents on every
    /// automatic UI poll.
    disk_metadata_stamp: Option<DiskMetadataStamp>,
    /// Most recent conflicting on-disk fingerprint observed for the current conflict.
    conflict_stamp: Option<DiskFileStamp>,
    /// Metadata-only fingerprint corresponding to `conflict_stamp`.
    conflict_metadata_stamp: Option<DiskMetadataStamp>,
    /// Whether the tracked path existed on disk when baselined (distinguishes deleted files from new paths).
    had_disk_baseline: bool,
    /// Whether the user already chose `Keep editing` after the file was deleted on disk.
    acknowledged_deleted: bool,
    /// Current unacknowledged external change for this document session.
    external_change: Option<ExternalFileChange>,
    /// Whether an external modification to the active file on disk has been detected.
    external_change_detected: bool,
    /// Whether the modal external-change confirmation dialog is open for this session.
    external_prompt_open: bool,
}

impl Default for RecoveryManager {
    fn default() -> Self {
        let active_id = next_untitled_id();
        Self {
            dir: None,
            tracked_ids: vec![active_id.clone()],
            active_id,
            dirty_anchor_secs: None,
            prompts: VecDeque::new(),
            last_disk_mtime_secs: None,
            last_disk_poll_secs: None,
            disk_stamp: None,
            disk_metadata_stamp: None,
            conflict_stamp: None,
            conflict_metadata_stamp: None,
            had_disk_baseline: false,
            acknowledged_deleted: false,
            external_change: None,
            external_change_detected: false,
            external_prompt_open: false,
        }
    }
}

impl RecoveryManager {
    fn tick(
        &mut self,
        project: &Project,
        path: Option<&Path>,
        dirty: bool,
        interval: f64,
        now: f64,
        wall: u64,
    ) -> Result<Option<Duration>, String> {
        if !dirty {
            self.reset_timer();
            return Ok(None);
        }
        let anchor = self.dirty_anchor_secs.get_or_insert(now);
        if now < *anchor {
            *anchor = now;
        }
        let elapsed = now - *anchor;
        if elapsed >= interval {
            self.dirty_anchor_secs = Some(now);
            let name = path
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| project.name.clone());
            self.write_active(project, path, &name, wall)?;
            Ok(Some(Duration::from_secs_f64(interval)))
        } else {
            Ok(Some(Duration::from_secs_f64(
                (interval - elapsed).max(0.25),
            )))
        }
    }

    /// Platform app-support recovery folder (`<eframe::storage_dir("NeoFlash")>/Recovery`).
    pub(crate) fn default_recovery_dir() -> Option<PathBuf> {
        eframe::storage_dir("NeoFlash").map(|dir| dir.join("Recovery"))
    }

    pub(crate) fn set_dir(&mut self, dir: Option<PathBuf>) {
        self.dir = dir;
    }

    pub(crate) fn take_dir(&mut self) -> Option<PathBuf> {
        self.dir.take()
    }

    #[cfg(test)]
    pub(crate) fn active_id(&self) -> &str {
        &self.active_id
    }

    /// A fresh document owns its recovery IDs/timer; only the directory is shared.
    pub(crate) fn for_document(&self, path: Option<&Path>) -> Self {
        let mut recovery = Self {
            dir: self.dir.clone(),
            ..Self::default()
        };
        if path.is_some() {
            recovery.retarget(path);
        }
        recovery
    }

    pub(crate) fn move_prompts_to(&mut self, other: &mut Self) {
        other.prompts.append(&mut self.prompts);
    }

    /// Deterministic filename-safe ID for a saved document path (64-bit FNV-1a).
    pub(crate) fn stable_id_for_path(path: &Path) -> String {
        let resolved = canonical_or_normalized_path(path);
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for &byte in resolved.as_os_str().as_encoded_bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        format!("doc-{hash:016x}")
    }

    /// Update the active recovery ID for `path` without deleting existing files.
    pub(crate) fn retarget(&mut self, path: Option<&Path>) {
        let id = path
            .map(Self::stable_id_for_path)
            .unwrap_or_else(next_untitled_id);
        if !self.tracked_ids.iter().any(|existing| existing == &id) {
            self.tracked_ids.push(id.clone());
        }
        self.active_id = id;
        self.dirty_anchor_secs = None;
        self.sync_disk_baseline(path);
    }

    /// Capture and store the current on-disk baseline for `path`, clearing any external conflict.
    pub(crate) fn sync_disk_baseline(&mut self, path: Option<&Path>) {
        self.disk_stamp = path.and_then(DiskFileStamp::capture);
        self.disk_metadata_stamp = path.and_then(DiskMetadataStamp::capture);
        self.conflict_stamp = None;
        self.conflict_metadata_stamp = None;
        self.had_disk_baseline = self.disk_stamp.is_some();
        self.acknowledged_deleted = false;
        self.last_disk_mtime_secs = self.disk_stamp.as_ref().map(|s| s.mtime_secs);
        self.external_change = None;
        self.external_change_detected = false;
        self.external_prompt_open = false;
    }

    fn automatic_disk_poll_due(&mut self, now_secs: f64) -> bool {
        if !now_secs.is_finite() {
            self.last_disk_poll_secs = None;
            return true;
        }
        let due = self.last_disk_poll_secs.is_none_or(|last| {
            now_secs < last || now_secs - last >= EXTERNAL_FILE_POLL_INTERVAL_SECS
        });
        if due {
            self.last_disk_poll_secs = Some(now_secs);
        }
        due
    }

    /// Acknowledge the current external change (`Keep editing` / `Ignore`) and re-baseline to disk.
    pub(crate) fn acknowledge_external_change(&mut self, path: Option<&Path>) {
        self.disk_stamp = path.and_then(DiskFileStamp::capture);
        self.disk_metadata_stamp = path.and_then(DiskMetadataStamp::capture);
        self.conflict_stamp = None;
        self.conflict_metadata_stamp = None;
        self.had_disk_baseline = path.is_some();
        self.acknowledged_deleted = path.is_some() && self.disk_stamp.is_none();
        self.last_disk_mtime_secs = self.disk_stamp.as_ref().map(|s| s.mtime_secs);
        self.external_change = None;
        self.external_change_detected = false;
        self.external_prompt_open = false;
    }

    /// Inspect `path` on disk against the stored baseline.
    /// Returns `Some(change)` when a *new* external change event occurs (so callers can notify once),
    /// while `self.external_change()` remains `Some(change)` until resolved or acknowledged.
    pub(crate) fn check_disk_change(&mut self, path: Option<&Path>) -> Option<ExternalFileChange> {
        self.check_disk_change_impl(path, true)
    }

    fn check_disk_change_automatic(&mut self, path: Option<&Path>) -> Option<ExternalFileChange> {
        self.check_disk_change_impl(path, false)
    }

    fn check_disk_change_impl(
        &mut self,
        path: Option<&Path>,
        force_full_scan: bool,
    ) -> Option<ExternalFileChange> {
        let path = path?;
        let current_metadata = DiskMetadataStamp::capture(path);
        if !force_full_scan && let Some(current_metadata) = current_metadata.as_ref() {
            let reference = if self.external_change.is_some() {
                self.conflict_metadata_stamp.as_ref()
            } else {
                self.disk_metadata_stamp.as_ref()
            };
            let full_reference_complete = self.external_change.is_some()
                || self
                    .disk_stamp
                    .as_ref()
                    .is_some_and(|stamp| stamp.scan_complete);
            if current_metadata.scan_complete
                && reference.is_some_and(|reference| {
                    reference.scan_complete && reference == current_metadata
                })
                && full_reference_complete
            {
                self.last_disk_mtime_secs = Some(current_metadata.mtime_secs);
                return None;
            }
        }

        match DiskFileStamp::capture(path) {
            Some(current) => {
                self.last_disk_mtime_secs = Some(current.mtime_secs);
                let Some(baseline) = &self.disk_stamp else {
                    if self.acknowledged_deleted {
                        // A previously deleted file was recreated on disk.
                        self.acknowledged_deleted = false;
                        self.had_disk_baseline = true;
                        let change = ExternalFileChange::Replaced;
                        let is_new = self.external_change != Some(change)
                            || self.conflict_stamp.as_ref() != Some(&current);
                        self.conflict_stamp = Some(current);
                        self.conflict_metadata_stamp = current_metadata;
                        self.external_change = Some(change);
                        self.external_change_detected = true;
                        return is_new.then_some(change);
                    }
                    self.disk_stamp = Some(current);
                    self.disk_metadata_stamp = current_metadata;
                    self.had_disk_baseline = true;
                    return None;
                };
                match baseline.classify_change(&current) {
                    Some(change) => {
                        let is_new = self.external_change != Some(change)
                            || self.conflict_stamp.as_ref() != Some(&current);
                        self.conflict_stamp = Some(current);
                        self.conflict_metadata_stamp = current_metadata;
                        self.external_change = Some(change);
                        self.external_change_detected = true;
                        is_new.then_some(change)
                    }
                    None => {
                        self.disk_stamp = Some(current);
                        self.disk_metadata_stamp = current_metadata;
                        self.conflict_stamp = None;
                        self.conflict_metadata_stamp = None;
                        self.external_change = None;
                        self.external_change_detected = false;
                        self.external_prompt_open = false;
                        None
                    }
                }
            }
            None => {
                if !self.had_disk_baseline || self.acknowledged_deleted {
                    return None;
                }
                let change = ExternalFileChange::Deleted;
                let is_new = self.external_change != Some(change) || self.conflict_stamp.is_some();
                self.conflict_stamp = None;
                self.conflict_metadata_stamp = None;
                self.external_change = Some(change);
                self.external_change_detected = true;
                is_new.then_some(change)
            }
        }
    }

    pub(crate) fn external_change(&self) -> Option<ExternalFileChange> {
        self.external_change
    }

    pub(crate) fn has_external_prompt(&self) -> bool {
        self.external_prompt_open && self.external_change.is_some()
    }

    pub(crate) fn open_external_prompt(&mut self) {
        if self.external_change.is_some() {
            self.external_prompt_open = true;
        }
    }

    pub(crate) fn dismiss_external_prompt(&mut self) {
        self.external_prompt_open = false;
    }

    #[allow(dead_code)]
    pub(crate) fn disk_stamp(&self) -> Option<&DiskFileStamp> {
        self.disk_stamp.as_ref()
    }

    /// Adopt a restored recovery candidate so its file stays protected until saved or discarded.
    pub(crate) fn adopt_restored(&mut self, candidate_id: &str, path: Option<&Path>) {
        self.tracked_ids.clear();
        self.tracked_ids.push(candidate_id.to_owned());
        let active = path
            .map(Self::stable_id_for_path)
            .unwrap_or_else(|| candidate_id.to_owned());
        if !self.tracked_ids.iter().any(|existing| existing == &active) {
            self.tracked_ids.push(active.clone());
        }
        self.active_id = active;
        self.dirty_anchor_secs = None;
        self.sync_disk_baseline(path);
    }

    pub(crate) fn reset_timer(&mut self) {
        self.dirty_anchor_secs = None;
    }

    /// Remove all recovery files owned by the current document session.
    pub(crate) fn clear_active(&mut self) -> Vec<String> {
        self.clear_active_except(&self.pending_ids())
    }

    pub(crate) fn pending_ids(&self) -> Vec<String> {
        self.prompts
            .iter()
            .map(|candidate| candidate.id.clone())
            .collect()
    }

    /// Undecided launch candidates remain protected even when a clean original closes.
    pub(crate) fn clear_active_except(&mut self, protected: &[String]) -> Vec<String> {
        self.dirty_anchor_secs = None;
        let mut errors = Vec::new();
        if let Some(dir) = &self.dir {
            for id in &self.tracked_ids {
                if !protected.contains(id) {
                    errors.extend(remove_id_files(dir, id));
                }
            }
        }
        self.tracked_ids.clear();
        self.tracked_ids.push(self.active_id.clone());
        errors
    }

    /// Remove recovery files for the previous document state and switch to `path`.
    pub(crate) fn clear_and_retarget(&mut self, path: Option<&Path>) -> Vec<String> {
        let errors = self.clear_active();
        self.tracked_ids.clear();
        self.retarget(path);
        errors
    }

    /// Remove a specific recovery candidate's files from disk.
    pub(crate) fn delete_candidate_files(&self, id: &str) -> Vec<String> {
        match &self.dir {
            Some(dir) => remove_id_files(dir, id),
            None => Vec::new(),
        }
    }

    pub(crate) fn has_prompt(&self) -> bool {
        !self.prompts.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn current_prompt(&self) -> Option<&RecoveryCandidate> {
        self.prompts.front()
    }

    pub(crate) fn current_prompt_summary(&self, now_unix_secs: u64) -> Option<String> {
        let cand = self.prompts.front()?;
        let when = format_recovery_time(cand.timestamp_unix_secs, now_unix_secs);
        Some(format!(
            "An autosaved copy of “{}” from {} was found. Restore it or discard the recovery file.",
            cand.document_name, when
        ))
    }

    pub(crate) fn pop_prompt(&mut self) -> Option<RecoveryCandidate> {
        self.prompts.pop_front()
    }

    /// Atomically write `<active_id>.json` and `<active_id>.nflash` in the recovery directory.
    pub(crate) fn write_active(
        &mut self,
        project: &Project,
        original_path: Option<&Path>,
        document_name: &str,
        timestamp_unix_secs: u64,
    ) -> Result<(), String> {
        let Some(dir) = &self.dir else {
            return Ok(());
        };
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let project_json = project.to_json().map_err(|e| e.to_string())?;
        let sidecar = RecoverySidecar {
            original_path: original_path.map(Path::to_path_buf),
            document_name: document_name.to_owned(),
            timestamp_unix_secs,
        };
        let sidecar_json = serde_json::to_string_pretty(&sidecar).map_err(|e| e.to_string())?;
        let id = &self.active_id;
        atomic_write(
            &dir.join(format!("{id}.json.tmp")),
            &dir.join(format!("{id}.json")),
            sidecar_json.as_bytes(),
        )?;
        atomic_write(
            &dir.join(format!("{id}.nflash.tmp")),
            &dir.join(format!("{id}.nflash")),
            project_json.as_bytes(),
        )?;
        if !self
            .tracked_ids
            .iter()
            .any(|existing| existing == &self.active_id)
        {
            self.tracked_ids.push(self.active_id.clone());
        }
        Ok(())
    }

    /// Scan the recovery folder on startup without blocking on corrupt or unreadable files.
    pub(crate) fn scan_on_launch(&mut self) -> Vec<String> {
        self.prompts.clear();
        let Some(dir) = &self.dir else {
            return Vec::new();
        };
        if !dir.exists() {
            return Vec::new();
        };
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) => return vec![format!("Recovery scan failed: {e}")],
        };
        let mut warnings = Vec::new();
        let mut candidates = Vec::new();
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) => {
                    warnings.push(format!("Recovery directory entry unreadable: {e}"));
                    continue;
                }
            };
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("nflash") {
                continue;
            }
            let Some(id) = path
                .file_stem()
                .and_then(|s| s.to_str())
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
            else {
                continue;
            };
            let project = match std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|raw| Project::from_json(&raw).map_err(|e| e.to_string()))
            {
                Ok(project) => project,
                Err(e) => {
                    warnings.push(format!("Recovery file {} skipped: {e}", path.display()));
                    continue;
                }
            };
            let sidecar_path = dir.join(format!("{id}.json"));
            let (original_path, document_name, timestamp_unix_secs) = if sidecar_path.exists() {
                match std::fs::read_to_string(&sidecar_path)
                    .map_err(|e| e.to_string())
                    .and_then(|raw| {
                        serde_json::from_str::<RecoverySidecar>(&raw).map_err(|e| e.to_string())
                    }) {
                    Ok(meta) => {
                        let name = if meta.document_name.trim().is_empty() {
                            fallback_doc_name(&project, &id)
                        } else {
                            meta.document_name
                        };
                        (meta.original_path, name, meta.timestamp_unix_secs)
                    }
                    Err(e) => {
                        warnings.push(format!(
                            "Recovery metadata {} unreadable ({e}); using project defaults.",
                            sidecar_path.display()
                        ));
                        (
                            None,
                            fallback_doc_name(&project, &id),
                            file_mtime_unix_secs(&path),
                        )
                    }
                }
            } else {
                (
                    None,
                    fallback_doc_name(&project, &id),
                    file_mtime_unix_secs(&path),
                )
            };
            candidates.push(RecoveryCandidate {
                id,
                original_path,
                document_name,
                timestamp_unix_secs,
                project,
            });
        }
        candidates.sort_by(|a, b| {
            b.timestamp_unix_secs
                .cmp(&a.timestamp_unix_secs)
                .then_with(|| a.id.cmp(&b.id))
        });
        self.prompts = candidates.into();
        warnings
    }
}

impl Editor {
    /// Autosave active and inactive dirty documents using independent timers and IDs.
    pub(crate) fn tick_autosave(&mut self, now_secs: f64, wall_unix_secs: u64) -> Option<Duration> {
        let _ = self.poll_external_file_changes(now_secs);
        if self.recovery.has_prompt() && (!self.dirty() || !self.sessions.is_empty()) {
            return None;
        }
        let enabled = self.workspace.general.autosave;
        let interval = f64::from(self.workspace.general.autosave_minutes.clamp(1, 120)) * 60.;
        let dirty = self.dirty();
        let mut results = vec![self.recovery.tick(
            &self.project,
            self.path.as_deref(),
            enabled && dirty,
            interval,
            now_secs,
            wall_unix_secs,
        )];
        for session in self.sessions.iter_mut().flatten() {
            let dirty = session.dirty();
            results.push(session.recovery.tick(
                &session.project,
                session.path.as_deref(),
                enabled && dirty,
                interval,
                now_secs,
                wall_unix_secs,
            ));
        }
        let mut wait = None;
        for result in results {
            match result {
                Ok(Some(duration)) => {
                    wait = Some(wait.map_or(duration, |old: Duration| old.min(duration)))
                }
                Ok(None) => {}
                Err(error) => {
                    self.message(format!("Autosave failed: {error}"));
                    let retry = Duration::from_secs_f64(interval);
                    wait = Some(wait.map_or(retry, |old| old.min(retry)));
                }
            }
        }
        wait
    }

    pub(crate) fn check_recovery_on_launch(&mut self) {
        for warning in self.recovery.scan_on_launch() {
            self.message(warning);
        }
    }

    pub(crate) fn clear_active_recovery(&mut self) {
        for err in self.recovery.clear_active() {
            self.message(format!("Recovery cleanup failed: {err}"));
        }
    }

    pub(crate) fn clear_and_retarget_recovery(&mut self) {
        let path = self.tracked_disk_path().map(Path::to_path_buf);
        for err in self.recovery.clear_and_retarget(path.as_deref()) {
            self.message(format!("Recovery cleanup failed: {err}"));
        }
    }

    /// Restore each candidate into an accessible tab without replacing unsaved content.
    pub(crate) fn restore_recovery_prompt(&mut self, ctx: &egui::Context) {
        self.restore_recovery_document(ctx);
    }

    #[allow(dead_code)]
    pub(crate) fn restore_recovery_in_session(&mut self, ctx: &egui::Context) {
        self.restore_recovery_document(ctx);
    }

    fn restore_recovery_document(&mut self, ctx: &egui::Context) {
        if self.confirm.is_some() || self.close_origin.is_some() || self.timeline_edit.dragging() {
            return;
        }
        let Some(cand) = self.recovery.prompts.front().cloned() else {
            return;
        };
        let mut path = cand.original_path.clone();
        // An already open original keeps its identity, even when clean. Its recovery becomes
        // an independent tab so the existing history, selection and view are never replaced.
        let conflict = path
            .as_deref()
            .and_then(|p| self.session_for_path(p))
            .is_some();
        let mut independent_recovery = None;
        if conflict {
            path = None;
            // Copy first under a new identity. The original open document owns the old ID;
            // either document may autosave, Save As or close without deleting the other's copy.
            let mut recovery = self.recovery.for_document(None);
            if let Err(error) = recovery.write_active(
                &cand.project,
                None,
                &cand.document_name,
                cand.timestamp_unix_secs,
            ) {
                self.message(format!("Recovery restore failed: {error}"));
                return;
            }
            independent_recovery = Some(recovery);
        }
        self.recovery.pop_prompt();
        if self.has_document() {
            self.open_new_session_tab(ctx, cand.project, path);
            self.saved_project = None;
        } else {
            // Do not delete the candidate when replacing a clean original with the same ID.
            let mut fresh = self.recovery.for_document(None);
            self.recovery.move_prompts_to(&mut fresh);
            self.recovery = fresh;
            self.install_document(ctx, cand.project, path, false);
        }
        if let Some(mut recovery) = independent_recovery {
            self.recovery.move_prompts_to(&mut recovery);
            self.recovery = recovery;
        } else {
            self.recovery.adopt_restored(&cand.id, self.path.as_deref());
        }
        if let Some(path) = &self.path
            && path.exists()
        {
            self.workspace.general.push_recent(path);
        }
        self.message(format!("Recovered “{}”.", cand.document_name));
    }

    /// Discard the front crash-recovery candidate and delete its recovery files from disk.
    pub(crate) fn discard_recovery_prompt(&mut self) {
        let Some(cand) = self.recovery.pop_prompt() else {
            return;
        };
        let errors = self.recovery.delete_candidate_files(&cand.id);
        if errors.is_empty() {
            self.message(format!(
                "Discarded recovery file for “{}”.",
                cand.document_name
            ));
        } else {
            for err in errors {
                self.message(format!("Recovery cleanup failed: {err}"));
            }
        }
    }

    /// Tracked disk path for the active document (canonical `.nflash` path or imported source path).
    pub(crate) fn tracked_disk_path(&self) -> Option<&Path> {
        self.path.as_deref().or(self.import_source.as_deref())
    }

    pub(crate) fn document_display_name(&self) -> String {
        self.tracked_disk_path()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.project.name.clone())
    }

    /// Record the current on-disk fingerprint of the active document (called after open/import/save).
    pub(crate) fn sync_disk_mtime(&mut self) {
        let path = self.tracked_disk_path().map(Path::to_path_buf);
        self.recovery.sync_disk_baseline(path.as_deref());
    }

    /// Return the current unacknowledged external change for the active document tab, if any.
    pub fn external_file_change(&self) -> Option<ExternalFileChange> {
        self.session_external_file_change(self.active_session)
    }

    /// Return the current unacknowledged external change for tab `index` (active or background).
    pub fn session_external_file_change(&self, index: usize) -> Option<ExternalFileChange> {
        if !self.has_document() || index >= self.sessions.len() {
            return None;
        }
        if index == self.active_session {
            self.recovery.external_change()
        } else {
            self.sessions[index]
                .as_ref()
                .and_then(|session| session.recovery.external_change())
        }
    }

    pub(crate) fn has_external_change_prompt(&self) -> bool {
        self.has_document() && self.recovery.has_external_prompt()
    }

    pub(crate) fn external_change_prompt_summary(&self) -> Option<(&'static str, String, bool)> {
        if !self.has_external_change_prompt() {
            return None;
        }
        let change = self.external_file_change()?;
        let name = self.document_display_name();
        let can_reload = change != ExternalFileChange::Deleted
            && self.tracked_disk_path().is_some_and(Path::exists);
        Some((
            change.dialog_title(),
            change.summary(&name, self.dirty()),
            can_reload,
        ))
    }

    pub(crate) fn dismiss_external_change_prompt(&mut self) {
        self.recovery.dismiss_external_prompt();
    }

    /// Poll all open document tabs (active + background `DocumentSession`s) for external on-disk
    /// modifications, atomic replacements, or deletions (`T-200`).
    /// Returns all `(tab_index, ExternalFileChange)` pairs currently having an unacknowledged conflict.
    pub fn check_external_file_changes(&mut self) -> Vec<(usize, ExternalFileChange)> {
        self.check_external_file_changes_impl(None)
    }

    fn poll_external_file_changes(&mut self, now_secs: f64) -> Vec<(usize, ExternalFileChange)> {
        self.check_external_file_changes_impl(Some(now_secs))
    }

    fn check_external_file_changes_impl(
        &mut self,
        automatic_now_secs: Option<f64>,
    ) -> Vec<(usize, ExternalFileChange)> {
        if !self.has_document() {
            return Vec::new();
        }
        let mut active_conflicts = Vec::new();
        let mut newly_detected = Vec::new();

        for index in 0..self.sessions.len() {
            if index == self.active_session {
                let poll_due =
                    automatic_now_secs.is_none_or(|now| self.recovery.automatic_disk_poll_due(now));
                let path = self.tracked_disk_path().map(Path::to_path_buf);
                let new_change = poll_due.then(|| match automatic_now_secs {
                    Some(_) => self.recovery.check_disk_change_automatic(path.as_deref()),
                    None => self.recovery.check_disk_change(path.as_deref()),
                });
                if let Some(Some(new_change)) = new_change {
                    self.recovery.open_external_prompt();
                    newly_detected.push((index, self.document_display_name(), new_change));
                }
                if let Some(change) = self.recovery.external_change() {
                    active_conflicts.push((index, change));
                }
            } else if let Some(session) = self.sessions[index].as_mut() {
                let poll_due = automatic_now_secs
                    .is_none_or(|now| session.recovery.automatic_disk_poll_due(now));
                let path = session.tracked_disk_path().map(Path::to_path_buf);
                let name = session.document_display_name();
                let new_change = poll_due.then(|| match automatic_now_secs {
                    Some(_) => session
                        .recovery
                        .check_disk_change_automatic(path.as_deref()),
                    None => session.recovery.check_disk_change(path.as_deref()),
                });
                if let Some(Some(new_change)) = new_change {
                    session.recovery.open_external_prompt();
                    newly_detected.push((index, name, new_change));
                }
                if let Some(change) = session.recovery.external_change() {
                    active_conflicts.push((index, change));
                }
            }
        }

        for (index, name, change) in newly_detected {
            let tab_note = if index == self.active_session {
                String::new()
            } else {
                format!(" (tab {})", index + 1)
            };
            self.notify_with_output(
                neoflash_ui::components::ToastType::Warning,
                format!(
                    "External change: “{name}”{tab_note} was {}. Reload from disk, Keep editing, or Save As.",
                    change.label()
                ),
            );
        }
        active_conflicts
    }

    /// Check whether the active document file on disk has an unacknowledged external change.
    pub fn check_external_modification(&mut self) -> bool {
        let _ = self.check_external_file_changes();
        self.recovery.external_change().is_some()
    }

    /// Guard `Save` (`⌘S`) on a tab with an unacknowledged external change so the editor never
    /// blindly overwrites external changes on disk without an explicit user decision (`T-200`).
    pub(crate) fn guard_save_on_external_conflict(&mut self, target_path: Option<&Path>) -> bool {
        if !self.has_document() {
            return false;
        }
        let Some(tracked) = self.tracked_disk_path().map(Path::to_path_buf) else {
            return true;
        };
        if let Some(target) = target_path
            && !crate::document_sessions::same_path(&tracked, target)
        {
            // Saving to a different path (`Save As…`) does not overwrite the conflicting file.
            return true;
        }
        let _ = self.check_external_modification();
        if let Some(change) = self.recovery.external_change() {
            self.recovery.open_external_prompt();
            let name = self.document_display_name();
            self.notify_with_output(
                neoflash_ui::components::ToastType::Warning,
                format!(
                    "Save paused: “{name}” was {}. Choose Reload, Keep editing, or Save As first.",
                    change.label()
                ),
            );
            return false;
        }
        true
    }

    /// Keep the in-memory document buffer and ignore/acknowledge the current external disk change (`T-200`).
    /// Re-baselines the disk stamp so the prompt does not nag repeatedly, while marking the buffer dirty
    /// so a subsequent `Save` (`⌘S`) writes the kept edits cleanly.
    pub fn keep_editing_external_change(&mut self) -> bool {
        if !self.has_document() {
            return false;
        }
        let path = self.tracked_disk_path().map(Path::to_path_buf);
        self.recovery.acknowledge_external_change(path.as_deref());
        self.saved_project = None;
        self.message("Kept current edits in memory; external disk change ignored.");
        true
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn ignore_external_modification(&mut self) -> bool {
        self.keep_editing_external_change()
    }

    /// Acknowledge an external change on tab `index` (`Keep editing` / `Ignore`) without losing edits.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn keep_editing_session_external_change(&mut self, index: usize) -> bool {
        if !self.has_document() || index >= self.sessions.len() {
            return false;
        }
        if index == self.active_session {
            return self.keep_editing_external_change();
        }
        let Some(session) = self.sessions[index].as_mut() else {
            return false;
        };
        session.keep_editing_external_change();
        self.message("Kept current edits in memory; external disk change ignored.");
        true
    }

    /// Resolve an external conflict by saving the current in-memory document to `path` (`Save As…`).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn save_as_external_change(&mut self, path: PathBuf) -> bool {
        if !self.has_document() {
            return false;
        }
        let tracked = self.tracked_disk_path().map(Path::to_path_buf);
        if tracked
            .as_deref()
            .is_some_and(|existing| crate::document_sessions::same_path(existing, &path))
        {
            self.recovery
                .acknowledge_external_change(tracked.as_deref());
            self.saved_project = None;
        }
        self.save_to_path(path)
    }

    pub(crate) fn load_project_from_disk_path(
        path: &Path,
    ) -> Result<(Project, Vec<String>), String> {
        if !path.exists() {
            return Err(format!(
                "file no longer exists on disk ({})",
                path.display()
            ));
        }
        let mut diagnostics = Vec::new();
        let project = if Self::is_legacy_flash_path(path) {
            let report = neoflash_import::inspect(path).map_err(|error| error.to_string())?;
            diagnostics = report
                .diagnostics
                .iter()
                .map(|diagnostic| {
                    format!(
                        "{} [{}]: {}",
                        diagnostic.location, diagnostic.code, diagnostic.message
                    )
                })
                .collect();
            if !report.can_import {
                return Err(format!(
                    "source cannot be imported safely ({})",
                    path.display()
                ));
            }
            neoflash_import::import_project(path).map_err(|error| error.to_string())?
        } else {
            let raw = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
            Project::from_json(&raw).map_err(|error| error.to_string())?
        };
        project.validate().map_err(|error| error.to_string())?;
        Ok((project, diagnostics))
    }

    /// Reload the open document from disk after an external modification (when clean or confirmed).
    /// Validates and parses the disk file before swapping any editor state so a corrupt or deleted
    /// file preserves the exact in-memory `project`, `history`, `selection`, `saved_project` and `recovery`.
    pub fn reload_external_modification(&mut self, ctx: &egui::Context) -> bool {
        if !self.has_document()
            || self.confirm.is_some()
            || self.close_origin.is_some()
            || self.timeline_edit.dragging()
        {
            return false;
        }
        let Some(disk_path) = self.tracked_disk_path().map(Path::to_path_buf) else {
            return false;
        };
        let canonical_path = self.path.clone();
        let preserved_import_source = self.import_source.clone();
        // Explicit Reload reads disk; ordinary Open of this path only activates the existing tab.
        // Validate first so a read/parse failure cannot consume selection, drafts, history or recovery.
        match Self::load_project_from_disk_path(&disk_path) {
            Ok((project, diagnostics)) => {
                self.install_document(ctx, project, canonical_path, true);
                self.import_source = preserved_import_source;
                self.sync_disk_mtime();
                for diagnostic in diagnostics {
                    self.message(diagnostic);
                }
                self.message("Project reloaded from disk.");
                true
            }
            Err(error) => {
                self.notify_with_output(
                    neoflash_ui::components::ToastType::Error,
                    format!("Reload failed: {error}"),
                );
                false
            }
        }
    }

    /// Reload a specific session tab (`index`) from disk after validating the on-disk file first.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn reload_session_external_modification(
        &mut self,
        ctx: &egui::Context,
        index: usize,
    ) -> bool {
        if !self.has_document()
            || index >= self.sessions.len()
            || self.confirm.is_some()
            || self.close_origin.is_some()
            || self.timeline_edit.dragging()
        {
            return false;
        }
        if index == self.active_session {
            return self.reload_external_modification(ctx);
        }
        let Some((disk_path, canonical_path, preserved_import_source)) =
            self.sessions[index].as_ref().and_then(|session| {
                Some((
                    session.tracked_disk_path()?.to_path_buf(),
                    session.path.clone(),
                    session.import_source.clone(),
                ))
            })
        else {
            return false;
        };
        match Self::load_project_from_disk_path(&disk_path) {
            Ok((project, diagnostics)) => {
                let cleanup_errors = self.sessions[index]
                    .as_mut()
                    .map(|session| session.recovery.clear_active())
                    .unwrap_or_default();
                for err in cleanup_errors {
                    self.message(format!("Recovery cleanup failed: {err}"));
                }
                if let Some(slot) = self.sessions.get_mut(index).and_then(Option::as_mut) {
                    let mut recovery = slot.recovery.for_document(canonical_path.as_deref());
                    recovery.sync_disk_baseline(Some(&disk_path));
                    let mut replacement = crate::document_sessions::DocumentSession::new_for_reload(
                        project,
                        canonical_path,
                        preserved_import_source,
                        self.workspace.undo_levels as usize,
                        recovery,
                        slot.onion_before,
                        slot.onion_after,
                        slot.stage_view.prefs.clone(),
                    );
                    std::mem::swap(slot, &mut replacement);
                }
                for diagnostic in diagnostics {
                    self.message(diagnostic);
                }
                self.message("Project reloaded from disk.");
                true
            }
            Err(error) => {
                self.notify_with_output(
                    neoflash_ui::components::ToastType::Error,
                    format!("Reload failed: {error}"),
                );
                false
            }
        }
    }
}

pub(crate) fn canonical_or_normalized_path(path: &Path) -> PathBuf {
    if let Ok(canon) = std::fs::canonicalize(path) {
        return canon;
    }
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    if let (Some(parent), Some(name)) = (abs.parent(), abs.file_name())
        && let Ok(canon_parent) = std::fs::canonicalize(parent)
    {
        return canon_parent.join(name);
    }
    abs
}

fn fnv1a64_continue(mut hash: u64, bytes: &[u8]) -> u64 {
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

fn metadata_mtime(metadata: &std::fs::Metadata) -> (u64, u32) {
    metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or((0, 0), |d| (d.as_secs(), d.subsec_nanos()))
}

fn is_xfl_anchor(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("xfl"))
        || path
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("DOMDocument.xml"))
}

#[derive(Clone, Copy)]
struct DiskScanLimits {
    entries: usize,
    bytes: u64,
}

const DISK_SCAN_LIMITS: DiskScanLimits = DiskScanLimits {
    entries: MAX_DISK_SCAN_ENTRIES,
    bytes: MAX_DISK_SCAN_BYTES,
};

/// Metadata-only package scan. Entry digests are combined commutatively, so directory iteration
/// order cannot create false change notifications and no directory listing has to be collected or
/// sorted in memory.
struct MetadataTreeScan {
    hash: u64,
    total_len: u64,
    max_mtime: (u64, u32),
    entries: usize,
    complete: bool,
    limit_hit: bool,
}

impl MetadataTreeScan {
    fn new(max_mtime: (u64, u32)) -> Self {
        Self {
            hash: 0,
            total_len: 0,
            max_mtime,
            entries: 0,
            complete: true,
            limit_hit: false,
        }
    }

    fn record(&mut self, kind: u8, relative: &Path, metadata: &std::fs::Metadata) -> bool {
        if self.limit_hit {
            return false;
        }
        if self.entries >= MAX_DISK_SCAN_ENTRIES {
            self.complete = false;
            self.limit_hit = true;
            return false;
        }
        self.entries += 1;
        self.max_mtime = self.max_mtime.max(metadata_mtime(metadata));

        let mut entry_hash = 0xcbf2_9ce4_8422_2325;
        entry_hash = fnv1a64_continue(entry_hash, &[kind]);
        let relative_bytes = relative.as_os_str().as_encoded_bytes();
        entry_hash = fnv1a64_continue(entry_hash, &(relative_bytes.len() as u64).to_le_bytes());
        entry_hash = fnv1a64_continue(entry_hash, relative_bytes);
        entry_hash = fnv1a64_continue(entry_hash, &metadata.len().to_le_bytes());
        let (secs, nanos) = metadata_mtime(metadata);
        entry_hash = fnv1a64_continue(entry_hash, &secs.to_le_bytes());
        entry_hash = fnv1a64_continue(entry_hash, &nanos.to_le_bytes());
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt as _;
            entry_hash = fnv1a64_continue(entry_hash, &metadata.dev().to_le_bytes());
            entry_hash = fnv1a64_continue(entry_hash, &metadata.ino().to_le_bytes());
        }
        self.hash ^= entry_hash.rotate_left((entry_hash & 63) as u32);
        true
    }

    fn record_file(&mut self, relative: &Path, metadata: &std::fs::Metadata) {
        if self.record(b'F', relative, metadata) {
            self.total_len = self.total_len.saturating_add(metadata.len());
        }
    }

    fn record_error(&mut self) {
        self.complete = false;
    }
}

fn scan_metadata_path_entry(path: &Path, relative: &Path, scan: &mut MetadataTreeScan) {
    if scan.limit_hit {
        return;
    }
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(_) => {
            scan.record_error();
            return;
        }
    };
    if metadata.is_file() {
        scan.record_file(relative, &metadata);
    } else if metadata.is_dir() {
        scan_metadata_directory_tree(path, relative, scan);
    } else if metadata.file_type().is_symlink() {
        if scan.record(b'L', relative, &metadata) && std::fs::read_link(path).is_err() {
            scan.record_error();
        }
    } else {
        let _ = scan.record(b'O', relative, &metadata);
    }
}

fn scan_metadata_directory_tree(root: &Path, prefix: &Path, scan: &mut MetadataTreeScan) {
    if !prefix.as_os_str().is_empty() {
        let metadata = match std::fs::symlink_metadata(root) {
            Ok(metadata) => metadata,
            Err(_) => {
                scan.record_error();
                return;
            }
        };
        if !scan.record(b'D', prefix, &metadata) {
            return;
        }
    }

    let mut pending = vec![(root.to_path_buf(), prefix.to_path_buf())];
    while let Some((dir, relative_dir)) = pending.pop() {
        if scan.limit_hit {
            return;
        }
        let read_dir = match std::fs::read_dir(&dir) {
            Ok(read_dir) => read_dir,
            Err(_) => {
                scan.record_error();
                continue;
            }
        };
        for entry in read_dir {
            if scan.limit_hit {
                return;
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    scan.record_error();
                    continue;
                }
            };
            let path = entry.path();
            let relative = relative_dir.join(entry.file_name());
            let metadata = match std::fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(_) => {
                    scan.record_error();
                    continue;
                }
            };
            if metadata.is_dir() {
                if !scan.record(b'D', &relative, &metadata) {
                    return;
                }
                pending.push((path, relative));
            } else if metadata.is_file() {
                scan.record_file(&relative, &metadata);
            } else if metadata.file_type().is_symlink() {
                if scan.record(b'L', &relative, &metadata) && std::fs::read_link(&path).is_err() {
                    scan.record_error();
                }
            } else {
                let _ = scan.record(b'O', &relative, &metadata);
            }
        }
    }
}

struct DiskTreeScan {
    hash: u64,
    total_len: u64,
    max_mtime: (u64, u32),
    entries: usize,
    scanned_bytes: u64,
    complete: bool,
    limit_hit: bool,
    limits: DiskScanLimits,
}

impl DiskTreeScan {
    fn new(max_mtime: (u64, u32)) -> Self {
        Self::with_limits(max_mtime, DISK_SCAN_LIMITS)
    }

    fn with_limits(max_mtime: (u64, u32), limits: DiskScanLimits) -> Self {
        Self {
            hash: 0xcbf2_9ce4_8422_2325,
            total_len: 0,
            max_mtime,
            entries: 0,
            scanned_bytes: 0,
            complete: true,
            limit_hit: false,
            limits,
        }
    }

    fn hash_framed(&mut self, bytes: &[u8]) {
        self.hash = fnv1a64_continue(self.hash, &(bytes.len() as u64).to_le_bytes());
        self.hash = fnv1a64_continue(self.hash, bytes);
    }

    fn record_entry(&mut self, kind: u8, relative: &Path, payload_len: u64) -> bool {
        if self.limit_hit {
            return false;
        }
        if self.entries >= self.limits.entries {
            self.record_limit(b"entry-limit");
            return false;
        }
        self.entries += 1;
        self.hash = fnv1a64_continue(self.hash, &[kind]);
        self.hash_framed(relative.as_os_str().as_encoded_bytes());
        self.hash = fnv1a64_continue(self.hash, &payload_len.to_le_bytes());
        true
    }

    fn record_error(&mut self, relative: &Path, error: &std::io::Error) {
        self.complete = false;
        self.hash = fnv1a64_continue(self.hash, b"E");
        self.hash_framed(relative.as_os_str().as_encoded_bytes());
        self.hash_framed(format!("{:?}", error.kind()).as_bytes());
    }

    fn record_unstable(&mut self, relative: &Path) {
        self.complete = false;
        self.hash = fnv1a64_continue(self.hash, b"U");
        self.hash_framed(relative.as_os_str().as_encoded_bytes());
    }

    fn record_limit(&mut self, reason: &[u8]) {
        if !self.limit_hit {
            self.complete = false;
            self.limit_hit = true;
            self.hash = fnv1a64_continue(self.hash, b"L");
            self.hash_framed(reason);
        }
    }

    fn note_mtime(&mut self, metadata: &std::fs::Metadata) {
        self.max_mtime = self.max_mtime.max(metadata_mtime(metadata));
    }
}

fn scan_regular_file(
    path: &Path,
    relative: &Path,
    metadata: &std::fs::Metadata,
    scan: &mut DiskTreeScan,
) {
    if !scan.record_entry(b'F', relative, metadata.len()) {
        return;
    }
    scan.note_mtime(metadata);
    scan.total_len = scan.total_len.saturating_add(metadata.len());
    if scan
        .scanned_bytes
        .checked_add(metadata.len())
        .is_none_or(|total| total > scan.limits.bytes)
    {
        scan.record_limit(b"byte-limit");
        return;
    }

    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) => {
            scan.record_error(relative, &error);
            return;
        }
    };
    let mut buffer = [0u8; 64 * 1024];
    let mut read_len = 0u64;
    loop {
        match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                read_len = read_len.saturating_add(count as u64);
                scan.hash = fnv1a64_continue(scan.hash, &buffer[..count]);
            }
            Err(error) => {
                scan.record_error(relative, &error);
                return;
            }
        }
    }
    scan.scanned_bytes = scan.scanned_bytes.saturating_add(read_len);
    let stable = file.metadata().is_ok_and(|after| {
        after.len() == metadata.len() && metadata_mtime(&after) == metadata_mtime(metadata)
    });
    if read_len != metadata.len() || !stable {
        scan.record_unstable(relative);
    }
}

fn scan_path_entry(path: &Path, relative: &Path, scan: &mut DiskTreeScan) {
    if scan.limit_hit {
        return;
    }
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            scan.record_error(relative, &error);
            return;
        }
    };
    if metadata.is_file() {
        scan_regular_file(path, relative, &metadata, scan);
    } else if metadata.is_dir() {
        scan_directory_tree(path, relative, scan);
    } else if metadata.file_type().is_symlink() {
        if !scan.record_entry(b'L', relative, metadata.len()) {
            return;
        }
        scan.note_mtime(&metadata);
        match std::fs::read_link(path) {
            Ok(target) => scan.hash_framed(target.as_os_str().as_encoded_bytes()),
            Err(error) => scan.record_error(relative, &error),
        }
    } else {
        let _ = scan.record_entry(b'O', relative, metadata.len());
        scan.note_mtime(&metadata);
    }
}

fn scan_directory_tree(root: &Path, prefix: &Path, scan: &mut DiskTreeScan) {
    if !prefix.as_os_str().is_empty() {
        let metadata = match std::fs::symlink_metadata(root) {
            Ok(metadata) => metadata,
            Err(error) => {
                scan.record_error(prefix, &error);
                return;
            }
        };
        if !scan.record_entry(b'D', prefix, 0) {
            return;
        }
        scan.note_mtime(&metadata);
    }

    let mut pending = vec![(root.to_path_buf(), prefix.to_path_buf())];
    while let Some((dir, relative_dir)) = pending.pop() {
        if scan.limit_hit {
            return;
        }
        let read_dir = match std::fs::read_dir(&dir) {
            Ok(read_dir) => read_dir,
            Err(error) => {
                scan.record_error(&relative_dir, &error);
                continue;
            }
        };
        let mut entries = Vec::new();
        for entry in read_dir {
            // Bound the temporary listing while it is being built. Previously a directory with
            // millions of children was collected in full before `record_entry` could apply the
            // package-entry limit.
            if entries.len() >= scan.limits.entries.saturating_sub(scan.entries) {
                scan.record_limit(b"entry-limit");
                break;
            }
            match entry {
                Ok(entry) => entries.push(entry),
                Err(error) => scan.record_error(&relative_dir, &error),
            }
        }
        if scan.limit_hit {
            return;
        }
        entries.sort_by_key(|entry| entry.file_name());
        let mut child_dirs = Vec::new();
        for entry in entries {
            if scan.limit_hit {
                return;
            }
            let path = entry.path();
            let relative = relative_dir.join(entry.file_name());
            let metadata = match std::fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) => {
                    scan.record_error(&relative, &error);
                    continue;
                }
            };
            if metadata.is_dir() {
                if !scan.record_entry(b'D', &relative, 0) {
                    return;
                }
                scan.note_mtime(&metadata);
                child_dirs.push((path, relative));
            } else if metadata.is_file() {
                scan_regular_file(&path, &relative, &metadata, scan);
            } else if metadata.file_type().is_symlink() {
                if !scan.record_entry(b'L', &relative, metadata.len()) {
                    return;
                }
                scan.note_mtime(&metadata);
                match std::fs::read_link(&path) {
                    Ok(target) => scan.hash_framed(target.as_os_str().as_encoded_bytes()),
                    Err(error) => scan.record_error(&relative, &error),
                }
            } else {
                let _ = scan.record_entry(b'O', &relative, metadata.len());
                scan.note_mtime(&metadata);
            }
        }
        for child in child_dirs.into_iter().rev() {
            pending.push(child);
        }
    }
}

pub(crate) fn unix_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn next_untitled_id() -> String {
    let pid = std::process::id();
    let seq = UNTITLED_SEQ.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("untitled-{pid}-{nanos:x}-{seq}")
}

fn fallback_doc_name(project: &Project, id: &str) -> String {
    let trimmed = project.name.trim();
    if trimmed.is_empty() {
        id.to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn file_mtime_unix_secs(path: &Path) -> u64 {
    std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
}

fn atomic_write(tmp: &Path, dst: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = std::fs::File::create(tmp).map_err(|e| e.to_string())?;
    file.write_all(bytes).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    std::fs::rename(tmp, dst).map_err(|e| e.to_string())
}

fn remove_id_files(dir: &Path, id: &str) -> Vec<String> {
    let mut errors = Vec::new();
    for suffix in ["nflash", "json", "nflash.tmp", "json.tmp"] {
        let target = dir.join(format!("{id}.{suffix}"));
        if let Err(err) = std::fs::remove_file(&target)
            && err.kind() != std::io::ErrorKind::NotFound
        {
            errors.push(format!("{}: {err}", target.display()));
        }
    }
    errors
}

pub(crate) fn format_recovery_time(timestamp_unix_secs: u64, now_unix_secs: u64) -> String {
    if timestamp_unix_secs == 0 {
        return "a previous session".into();
    }
    let diff = now_unix_secs.saturating_sub(timestamp_unix_secs);
    if diff < 60 {
        "just now".into()
    } else if diff < 3600 {
        let mins = diff / 60;
        if mins == 1 {
            "1 minute ago".into()
        } else {
            format!("{mins} minutes ago")
        }
    } else if diff < 86_400 {
        let hours = diff / 3600;
        if hours == 1 {
            "1 hour ago".into()
        } else {
            format!("{hours} hours ago")
        }
    } else if diff < 7 * 86_400 {
        let days = diff / 86_400;
        if days == 1 {
            "1 day ago".into()
        } else {
            format!("{days} days ago")
        }
    } else {
        let days = (timestamp_unix_secs / 86_400) as i64;
        let rem = timestamp_unix_secs % 86_400;
        let hour = rem / 3600;
        let minute = (rem % 3600) / 60;
        let (year, month, day) = civil_from_days(days);
        format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02} UTC")
    }
}

/// Howard Hinnant's proleptic Gregorian calendar conversion from Unix epoch days.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = i64::from(yoe) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{Editor, Pending};
    use crate::workspace::{GeneralPrefs, OnLaunch};
    use eframe::App as _;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_DIR_SEQ: AtomicU64 = AtomicU64::new(1);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(prefix: &str) -> Self {
            let seq = TEST_DIR_SEQ.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "neoflash-recovery-test-{prefix}-{}-{seq}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("temp test dir");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn recent_files_push_dedupes_orders_caps_at_ten_and_drops_missing() {
        let dir = TempDir::new("recent");
        let mut prefs = GeneralPrefs::default();
        let mut files = Vec::new();
        for idx in 1..=12 {
            let file = dir.path().join(format!("doc_{idx:02}.nflash"));
            std::fs::write(&file, Project::blank().to_json().unwrap()).unwrap();
            prefs.push_recent(&file);
            files.push(file);
        }
        assert_eq!(prefs.recent_files.len(), 10);
        let listed = prefs.existing_recent_files();
        assert_eq!(listed.len(), 10);
        // Most recent (`doc_12`) is first; oldest two (`doc_01`, `doc_02`) rolled off.
        assert_eq!(listed[0].file_name().unwrap(), "doc_12.nflash");
        assert_eq!(listed[9].file_name().unwrap(), "doc_03.nflash");

        // Re-pushing `doc_05` via a non-canonical `./` path moves it to index 0 without duplicating.
        let dot_five = dir.path().join(".").join("doc_05.nflash");
        prefs.push_recent(&dot_five);
        assert_eq!(prefs.recent_files.len(), 10);
        let listed = prefs.existing_recent_files();
        assert_eq!(listed[0].file_name().unwrap(), "doc_05.nflash");
        assert_eq!(
            listed
                .iter()
                .filter(|p| p.file_name().and_then(|s| s.to_str()) == Some("doc_05.nflash"))
                .count(),
            1
        );

        // Deleting `doc_11` on disk drops it from `existing_recent_files()` while preserving order.
        std::fs::remove_file(&files[10]).unwrap();
        let listed = prefs.existing_recent_files();
        assert_eq!(listed.len(), 9);
        assert!(
            !listed
                .iter()
                .any(|p| p.file_name().and_then(|s| s.to_str()) == Some("doc_11.nflash"))
        );
    }

    #[test]
    fn on_launch_opens_new_example_or_last_document_with_fallback() {
        let dir = TempDir::new("on-launch");
        let saved_path = dir.path().join("LastSaved.nflash");
        let mut saved_proj = Project::blank();
        saved_proj.name = "LastSavedProject".into();
        saved_proj.width = 777;
        std::fs::write(&saved_path, saved_proj.to_json().unwrap()).unwrap();

        let ctx = egui::Context::default();

        // 1. NewDocument opens a clean blank project (and headless `from_project` has no disk recovery dir).
        let mut editor = Editor::from_project(&ctx, Project::demo());
        assert!(editor.recovery.dir.is_none());
        editor.workspace.general.on_launch = OnLaunch::NewDocument;
        editor.apply_on_launch(&ctx);
        assert_eq!(editor.project.name, "Untitled");
        assert!(
            editor.symbol().timeline.layers[0].keyframes[0]
                .elements
                .is_empty()
        );
        assert!(!editor.dirty());
        assert!(editor.path.is_none());

        // 2. LastDocument skips missing entries and opens the first existing recent file.
        let mut editor = Editor::from_project(&ctx, Project::blank());
        editor.workspace.general.on_launch = OnLaunch::LastDocument;
        editor.workspace.general.recent_files = vec![
            dir.path()
                .join("deleted_first.nflash")
                .to_string_lossy()
                .into_owned(),
            saved_path.to_string_lossy().into_owned(),
        ];
        editor.apply_on_launch(&ctx);
        assert_eq!(editor.project.name, "LastSavedProject");
        assert_eq!(editor.project.width, 777);
        assert_eq!(
            std::fs::canonicalize(editor.path.as_ref().unwrap()).unwrap(),
            std::fs::canonicalize(&saved_path).unwrap()
        );
        assert!(!editor.dirty());

        // Opening the same canonical path never reloads or consumes edits. Explicit Reload does.
        saved_proj.name = "External change".into();
        std::fs::write(&saved_path, saved_proj.to_json().unwrap()).unwrap();
        editor.transact(|editor| editor.project.name = "Local edits".into());
        assert!(editor.try_open_path(saved_path.clone(), &ctx));
        assert_eq!(editor.project.name, "Local edits");
        assert!(editor.history.can_undo());
        assert!(editor.reload_external_modification(&ctx));
        assert_eq!(editor.project.name, "External change");
        assert_eq!(editor.session_count(), 1);
        assert!(!editor.history.can_undo());
        assert!(!editor.dirty());
        editor.transact(|editor| editor.project.name = "Protected from failed reload".into());
        let before = editor.project.clone();
        std::fs::write(&saved_path, "{invalid").unwrap();
        assert!(!editor.reload_external_modification(&ctx));
        assert_eq!(editor.project, before);
        assert!(editor.history.can_undo());
        std::fs::write(&saved_path, saved_proj.to_json().unwrap()).unwrap();

        // 3. LastDocument with no existing recent files falls back to Example with a message.
        let demo = Project::demo();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        editor.workspace.general.on_launch = OnLaunch::LastDocument;
        editor.workspace.general.recent_files = vec![
            dir.path()
                .join("gone.nflash")
                .to_string_lossy()
                .into_owned(),
        ];
        editor.apply_on_launch(&ctx);
        assert_eq!(editor.project.name, demo.name);
        assert!(
            !editor.symbol().timeline.layers[0].keyframes[0]
                .elements
                .is_empty()
        );
        assert!(!editor.dirty());
        assert!(
            editor.status.contains("example document"),
            "status was: {}",
            editor.status
        );

        // 4. LastDocument with a corrupt recent file falls back to Example with a diagnostic.
        let corrupt = dir.path().join("corrupt.nflash");
        std::fs::write(&corrupt, "{not valid json").unwrap();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        editor.workspace.general.on_launch = OnLaunch::LastDocument;
        editor.workspace.general.push_recent(&corrupt);
        editor.apply_on_launch(&ctx);
        assert_eq!(editor.project.name, demo.name);
        assert!(editor.status.contains("Open failed"));
        assert!(editor.status.contains("Opened example document."));
    }

    #[test]
    fn autosave_timer_writes_atomically_to_recovery_dir_only_when_enabled_and_dirty() {
        let user_dir = TempDir::new("user-docs");
        let rec_dir = TempDir::new("app-support-recovery");
        let user_file = user_dir.path().join("Hero.nflash");

        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        editor.recovery.set_dir(Some(rec_dir.path().to_path_buf()));
        editor.workspace.general.autosave = true;
        editor.workspace.general.autosave_minutes = 3; // 180 seconds
        assert!(editor.save_to_path(user_file.clone()));
        let original_user_bytes = std::fs::read_to_string(&user_file).unwrap();

        // Clean document: advancing 10 minutes writes nothing.
        assert!(editor.tick_autosave(0.0, 1_700_000_000).is_none());
        assert!(editor.tick_autosave(600.0, 1_700_000_600).is_none());
        assert_eq!(std::fs::read_dir(rec_dir.path()).unwrap().count(), 0);

        // Dirty document with `autosave = false`: writes nothing.
        editor.workspace.general.autosave = false;
        editor.transact(|e| e.project.name = "Hero v2".into());
        assert!(editor.dirty());
        assert!(editor.tick_autosave(700.0, 1_700_000_700).is_none());
        assert!(editor.tick_autosave(1500.0, 1_700_001_500).is_none());
        assert_eq!(std::fs::read_dir(rec_dir.path()).unwrap().count(), 0);

        // Enable autosave: countdown starts at t = 2000.0 and fires at t = 2180.0 (3 min).
        editor.workspace.general.autosave = true;
        assert!(editor.tick_autosave(2000.0, 1_700_002_000).is_some());
        assert!(editor.tick_autosave(2179.9, 1_700_002_179).is_some());
        assert_eq!(std::fs::read_dir(rec_dir.path()).unwrap().count(), 0);

        assert!(editor.tick_autosave(2180.0, 1_700_002_180).is_some());
        let id = editor.recovery.active_id().to_owned();
        let rec_nflash = rec_dir.path().join(format!("{id}.nflash"));
        let rec_sidecar = rec_dir.path().join(format!("{id}.json"));
        assert!(rec_nflash.exists());
        assert!(rec_sidecar.exists());
        assert!(!rec_dir.path().join(format!("{id}.nflash.tmp")).exists());
        assert!(!rec_dir.path().join(format!("{id}.json.tmp")).exists());

        // User's directory has only the original saved file, untouched by autosave.
        let user_entries: Vec<_> = std::fs::read_dir(user_dir.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(user_entries, vec![user_file.clone()]);
        assert_eq!(
            std::fs::read_to_string(&user_file).unwrap(),
            original_user_bytes
        );

        let recovered = Project::from_json(&std::fs::read_to_string(&rec_nflash).unwrap()).unwrap();
        assert_eq!(recovered.name, "Hero v2");
        let meta: RecoverySidecar =
            serde_json::from_str(&std::fs::read_to_string(&rec_sidecar).unwrap()).unwrap();
        assert_eq!(meta.original_path.as_deref(), Some(user_file.as_path()));
        assert_eq!(meta.document_name, "Hero.nflash");
        assert_eq!(meta.timestamp_unix_secs, 1_700_002_180);

        // Next edit writes again after another 3-minute interval.
        editor.transact(|e| e.project.name = "Hero v3".into());
        editor.tick_autosave(2360.0, 1_700_002_360);
        let recovered = Project::from_json(&std::fs::read_to_string(&rec_nflash).unwrap()).unwrap();
        assert_eq!(recovered.name, "Hero v3");
    }

    #[test]
    fn recovery_lifecycle_preserves_new_open_cancels_close_and_restores_in_tabs() {
        let user_dir = TempDir::new("lifecycle-user");
        let rec_dir = TempDir::new("lifecycle-rec");
        let hero_path = user_dir.path().join("Hero.nflash");
        let other_path = user_dir.path().join("Other.nflash");
        let mut other_proj = Project::blank();
        other_proj.name = "OtherDoc".into();
        std::fs::write(&other_path, other_proj.to_json().unwrap()).unwrap();

        let ctx = egui::Context::default();
        let count_rec_files = || std::fs::read_dir(rec_dir.path()).unwrap().count();
        let trigger_autosave = |editor: &mut Editor, t: f64| {
            editor.workspace.general.autosave = true;
            editor.workspace.general.autosave_minutes = 1;
            editor.tick_autosave(t, 1_700_010_000);
            editor.tick_autosave(t + 60.0, 1_700_010_060);
        };

        let mut editor = Editor::from_project(&ctx, Project::blank());
        editor.recovery.set_dir(Some(rec_dir.path().to_path_buf()));

        // 1. Untitled autosave is deleted on Save As (`save_to_path`).
        editor.transact(|e| e.project.name = "UntitledDraft".into());
        trigger_autosave(&mut editor, 10.0);
        assert_eq!(count_rec_files(), 2);
        assert!(editor.save_to_path(hero_path.clone()));
        assert_eq!(count_rec_files(), 0, "Save As deletes untitled recovery");

        // 2. New preserves the original document and its recovery in another tab.
        editor.transact(|e| e.project.name = "DiscardedBeforeNew".into());
        trigger_autosave(&mut editor, 100.0);
        assert_eq!(count_rec_files(), 2);
        editor.perform(Pending::New, &ctx);
        assert_eq!(count_rec_files(), 2, "New preserves recovery");
        assert_eq!(editor.session_count(), 2);

        // 3. Open preserves both existing documents and their independent recovery files.
        editor.transact(|e| e.project.name = "DiscardedBeforeOpen".into());
        trigger_autosave(&mut editor, 200.0);
        assert_eq!(count_rec_files(), 4);
        editor.open_path(other_path.clone(), &ctx);
        assert_eq!(count_rec_files(), 4, "Open preserves previous recovery");
        assert_eq!(editor.session_count(), 3);

        // 4. A cancelled close sequence preserves all content and recovery, including earlier Discard decisions.
        editor.transact(|e| e.project.name = "DiscardedOnClose".into());
        trigger_autosave(&mut editor, 300.0);
        assert_eq!(count_rec_files(), 6);
        editor.request_quit(&ctx);
        assert_eq!(editor.active_session, 0);
        let waiting = editor.project.clone();
        let saved = editor.saved_project.clone();
        assert!(!editor.try_open_path(other_path.clone(), &ctx));
        assert!(!editor.try_open_path(user_dir.path().join("blocked.fla"), &ctx));
        assert_eq!(editor.project, waiting);
        assert_eq!(editor.saved_project, saved);
        editor.continue_confirmation(&ctx); // Discard first, but deletion is deferred.
        assert_eq!(editor.active_session, 1);
        editor.cancel_confirmation(&ctx);
        assert_eq!(editor.session_count(), 3);
        assert_eq!(count_rec_files(), 6);
        editor.request_close_documents(&ctx, false);
        while editor.confirm.is_some() {
            editor.continue_confirmation(&ctx);
        }
        assert_eq!(editor.session_count(), 0);
        assert_eq!(
            count_rec_files(),
            0,
            "completed Close all discards every target recovery"
        );

        // Clean `on_exit` cleans up any stale active files, while dirty crash without `allow_close` keeps them.
        editor.allow_close = false;
        editor.open_path(hero_path.clone(), &ctx);
        editor.transact(|e| e.project.name = "CrashedHeroWork".into());
        trigger_autosave(&mut editor, 400.0);
        assert_eq!(count_rec_files(), 2);
        editor.on_exit();
        assert_eq!(
            count_rec_files(),
            2,
            "dirty exit without allow_close keeps recovery files"
        );
        drop(editor);

        // Add a corrupt `.nflash` file in the recovery folder to prove startup does not block on IO/parse errors.
        std::fs::write(rec_dir.path().join("corrupt-entry.nflash"), "not json").unwrap();

        // 5. Relaunch with `autosave = false`: existing recovery files are still offered!
        let mut relaunched = Editor::from_project(&ctx, Project::demo());
        relaunched.workspace.general.autosave = false;
        relaunched
            .recovery
            .set_dir(Some(rec_dir.path().to_path_buf()));
        relaunched.check_recovery_on_launch();
        assert!(relaunched.recovery.has_prompt());
        assert!(
            relaunched
                .output
                .iter()
                .any(|line| line.contains("corrupt-entry.nflash")),
            "corrupt recovery file logged to Output without blocking"
        );
        assert_eq!(
            relaunched.recovery.current_prompt().unwrap().document_name,
            "Hero.nflash"
        );

        // Restore opens the recovered project as dirty with its original path so Save writes to `hero_path`.
        relaunched.restore_recovery_prompt(&ctx);
        assert!(!relaunched.recovery.has_prompt());
        assert_eq!(relaunched.project.name, "CrashedHeroWork");
        assert_eq!(
            relaunched.session_count(),
            2,
            "the clean launch document remains in its tab"
        );
        assert!(relaunched.dirty(), "restored project opens as dirty");
        assert_eq!(relaunched.path.as_deref(), Some(hero_path.as_path()));
        assert!(relaunched.save(false));
        assert!(!relaunched.dirty());
        std::fs::remove_file(rec_dir.path().join("corrupt-entry.nflash")).unwrap();
        assert_eq!(
            count_rec_files(),
            0,
            "saving restored project deletes its recovery snapshot"
        );
        let saved_on_disk =
            Project::from_json(&std::fs::read_to_string(&hero_path).unwrap()).unwrap();
        assert_eq!(saved_on_disk.name, "CrashedHeroWork");

        // Clean `on_exit` leaves recovery folder empty.
        relaunched.on_exit();
        assert_eq!(count_rec_files(), 0);
    }

    #[test]
    fn external_file_changes_detect_modified_same_second_backwards_mtime_replaced_and_deleted_while_ignoring_own_saves()
     {
        let dir = TempDir::new("external-fingerprints");
        let path = dir.path().join("Scene.nflash");
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());

        // 1. Editor's own saves (`save_to_path` and `save(false)`) re-baseline immediately and never trigger a conflict.
        editor.project.name = "InitialSave".into();
        assert!(editor.save_to_path(path.clone()));
        assert!(!editor.check_external_modification());
        assert_eq!(editor.external_file_change(), None);
        assert!(editor.check_external_file_changes().is_empty());

        editor.transact(|e| e.project.name = "SecondOwnSave".into());
        assert!(editor.save(false));
        assert!(!editor.check_external_modification());
        assert_eq!(editor.external_file_change(), None);

        // 2. Same-second write: overwrite in place and force the exact same mtime as the baseline.
        let baseline_mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        let mut ext_proj = Project::blank();
        ext_proj.name = "SameSecondWrite".into();
        std::fs::write(&path, ext_proj.to_json().unwrap()).unwrap();
        {
            let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
            file.set_modified(baseline_mtime).unwrap();
        }
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            baseline_mtime
        );
        assert_eq!(
            editor.check_external_file_changes(),
            vec![(0, ExternalFileChange::Modified)]
        );
        assert_eq!(
            editor.external_file_change(),
            Some(ExternalFileChange::Modified)
        );
        assert!(editor.has_external_change_prompt());
        assert!(editor.reload_external_modification(&ctx));
        assert_eq!(editor.project.name, "SameSecondWrite");
        assert_eq!(editor.external_file_change(), None);
        assert!(!editor.has_external_change_prompt());

        // 3. Backwards-timestamp write: overwrite in place and set mtime 1 hour earlier than baseline.
        let baseline_mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        let older_mtime = baseline_mtime
            .checked_sub(Duration::from_secs(3600))
            .unwrap_or(UNIX_EPOCH + Duration::from_secs(1_000_000));
        ext_proj.name = "BackwardsTimestampWrite".into();
        std::fs::write(&path, ext_proj.to_json().unwrap()).unwrap();
        {
            let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
            file.set_modified(older_mtime).unwrap();
        }
        assert!(std::fs::metadata(&path).unwrap().modified().unwrap() < baseline_mtime);
        assert_eq!(
            editor.check_external_file_changes(),
            vec![(0, ExternalFileChange::Modified)]
        );
        assert_eq!(
            editor.external_file_change(),
            Some(ExternalFileChange::Modified)
        );
        assert!(editor.reload_external_modification(&ctx));
        assert_eq!(editor.project.name, "BackwardsTimestampWrite");
        assert_eq!(editor.external_file_change(), None);

        // 4. Atomic replacement (rename-over with new inode and new content).
        let tmp_replacement = dir.path().join("Scene.nflash.external-tmp");
        ext_proj.name = "AtomicReplacement".into();
        std::fs::write(&tmp_replacement, ext_proj.to_json().unwrap()).unwrap();
        std::fs::rename(&tmp_replacement, &path).unwrap();
        assert_eq!(
            editor.check_external_file_changes(),
            vec![(0, ExternalFileChange::Replaced)]
        );
        assert_eq!(
            editor.external_file_change(),
            Some(ExternalFileChange::Replaced)
        );
        assert!(editor.reload_external_modification(&ctx));
        assert_eq!(editor.project.name, "AtomicReplacement");
        assert_eq!(editor.external_file_change(), None);

        // 5. File deletion on disk.
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            editor.check_external_file_changes(),
            vec![(0, ExternalFileChange::Deleted)]
        );
        assert_eq!(
            editor.external_file_change(),
            Some(ExternalFileChange::Deleted)
        );
        assert!(editor.has_external_change_prompt());
        let (_, _, can_reload) = editor.external_change_prompt_summary().unwrap();
        assert!(!can_reload, "Deleted file cannot offer Reload");
    }

    #[test]
    fn automatic_external_poll_is_throttled_but_save_guard_forces_an_immediate_check() {
        let dir = TempDir::new("external-poll-throttle");
        let path = dir.path().join("Scene.nflash");
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        assert!(editor.save_to_path(path.clone()));

        // First update fingerprints once and arms the one-second per-document throttle.
        let _ = editor.tick_autosave(10.0, 1_700_000_000);
        let mut external = Project::blank();
        external.name = "ExternalWithinThrottle".into();
        std::fs::write(&path, external.to_json().unwrap()).unwrap();
        let _ = editor.tick_autosave(10.5, 1_700_000_001);
        assert_eq!(editor.external_file_change(), None);

        let _ = editor.tick_autosave(11.0, 1_700_000_002);
        assert_eq!(
            editor.external_file_change(),
            Some(ExternalFileChange::Modified)
        );
        assert!(editor.reload_external_modification(&ctx));

        // Automatic polling is metadata-first: a deliberately same-length/same-mtime edit does
        // not make the UI thread reread the entire file. A user action that may overwrite disk
        // still forces the definitive content hash and catches this edge case immediately.
        let original_mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        let mut same_metadata_bytes = std::fs::read(&path).unwrap();
        let marker = b"ExternalWithinThrottle";
        let marker_at = same_metadata_bytes
            .windows(marker.len())
            .position(|window| window == marker)
            .unwrap();
        same_metadata_bytes[marker_at] = b'F';
        std::fs::write(&path, &same_metadata_bytes).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(original_mtime)
            .unwrap();
        let _ = editor.tick_autosave(12.0, 1_700_000_003);
        assert_eq!(editor.external_file_change(), None);
        assert!(!editor.guard_save_on_external_conflict(Some(&path)));
        assert_eq!(
            editor.external_file_change(),
            Some(ExternalFileChange::Modified)
        );
        assert!(editor.reload_external_modification(&ctx));

        // A forced Save guard must not wait for the next automatic poll.
        let _ = editor.tick_autosave(20.0, 1_700_000_010);
        external.name = "ForcedSaveGuard".into();
        std::fs::write(&path, external.to_json().unwrap()).unwrap();
        assert!(!editor.guard_save_on_external_conflict(Some(&path)));
        assert_eq!(
            editor.external_file_change(),
            Some(ExternalFileChange::Modified)
        );

        // A monotonic-clock rewind re-arms polling instead of suppressing it indefinitely.
        assert!(editor.keep_editing_external_change());
        external.name = "AfterClockRewind".into();
        std::fs::write(&path, external.to_json().unwrap()).unwrap();
        let _ = editor.tick_autosave(1.0, 1_700_000_011);
        assert_eq!(
            editor.external_file_change(),
            Some(ExternalFileChange::Modified)
        );
    }

    #[test]
    fn directory_fingerprints_frame_paths_cover_deep_xfl_content_and_fail_closed_on_limits() {
        let dir = TempDir::new("external-tree-framing");
        let tree = dir.path().join("Tree");
        std::fs::create_dir_all(tree.join("a")).unwrap();
        std::fs::write(tree.join("a/bc"), b"payload").unwrap();
        let before = DiskFileStamp::capture(&tree).unwrap();

        // Raw concatenation of entry names made `a/bc` and `ab/c` indistinguishable. Framed
        // relative paths and entry types must classify the structural replacement.
        std::fs::remove_dir_all(tree.join("a")).unwrap();
        std::fs::create_dir_all(tree.join("ab")).unwrap();
        std::fs::write(tree.join("ab/c"), b"payload").unwrap();
        let restructured = DiskFileStamp::capture(&tree).unwrap();
        assert_ne!(before.content_hash, restructured.content_hash);
        assert_eq!(
            before.classify_change(&restructured),
            Some(ExternalFileChange::Modified)
        );

        // Opening an uncompressed XFL through DOMDocument.xml must include arbitrarily deep
        // LIBRARY folders. Keep length and mtime identical to prove the content digest catches it.
        let xfl = dir.path().join("MovieXfl");
        let library_leaf = xfl.join("LIBRARY/a/b/c/d/e/f/symbol.xml");
        std::fs::create_dir_all(library_leaf.parent().unwrap()).unwrap();
        let dom = xfl.join("DOMDocument.xml");
        std::fs::write(&dom, b"<DOMDocument/>").unwrap();
        std::fs::write(&library_leaf, b"first-value").unwrap();
        let deep_before = DiskFileStamp::capture(&dom).unwrap();
        let old_mtime = std::fs::metadata(&library_leaf)
            .unwrap()
            .modified()
            .unwrap();
        std::fs::write(&library_leaf, b"other-value").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&library_leaf)
            .unwrap()
            .set_modified(old_mtime)
            .unwrap();
        let deep_after = DiskFileStamp::capture(&dom).unwrap();
        assert_eq!(deep_before.len, deep_after.len);
        assert_ne!(deep_before.content_hash, deep_after.content_hash);
        assert_eq!(
            deep_before.classify_change(&deep_after),
            Some(ExternalFileChange::Modified)
        );

        let mut bounded = DiskTreeScan::with_limits(
            (0, 0),
            DiskScanLimits {
                entries: 1,
                bytes: 1,
            },
        );
        scan_directory_tree(&xfl, Path::new(""), &mut bounded);
        assert!(!bounded.complete);
        assert!(bounded.limit_hit);
    }

    #[test]
    fn external_file_changes_multi_tab_workflow_save_guard_keep_editing_save_as_and_corrupt_reload_refusal()
     {
        let dir = TempDir::new("external-multi-tab");
        let rec_dir = TempDir::new("external-multi-tab-rec");
        let path_a = dir.path().join("DocA.nflash");
        let path_b = dir.path().join("DocB.nflash");
        let save_as_path = dir.path().join("DocB_Fork.nflash");
        let xfl_dir = dir.path().join("ImportedXfl");
        std::fs::create_dir_all(&xfl_dir).unwrap();
        let dom_xml = xfl_dir.join("DOMDocument.xml");
        std::fs::write(
            &dom_xml,
            r#"<DOMDocument width="640" height="480" frameRate="24"><timelines><DOMTimeline name="Scene 1"><layers><DOMLayer name="Layer 1"><frames><DOMFrame index="0"/></frames></DOMLayer></layers></DOMTimeline></timelines></DOMDocument>"#,
        )
        .unwrap();

        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        editor.recovery.set_dir(Some(rec_dir.path().to_path_buf()));
        editor.workspace.general.autosave = true;
        editor.workspace.general.autosave_minutes = 1;

        // Tab 0: DocA.nflash
        editor.project.name = "DocA_v1".into();
        assert!(editor.save_to_path(path_a.clone()));
        assert_eq!(editor.active_session, 0);

        // Tab 1: DocB.nflash (with local dirty edits, selection, and autosave recovery snapshot)
        let mut proj_b = Project::blank();
        proj_b.name = "DocB_v1".into();
        std::fs::write(&path_b, proj_b.to_json().unwrap()).unwrap();
        editor.open_path(path_b.clone(), &ctx);
        assert_eq!(editor.session_count(), 2);
        assert_eq!(editor.active_session, 1);

        editor.add_element(
            neoflash_core::ElementKind::Rectangle {
                width: 120.0,
                height: 80.0,
                fill: [50, 100, 200, 255],
                stroke: [0, 0, 0, 255],
                stroke_width: 2.0,
            },
            40.0,
            50.0,
        );
        editor.transact(|e| e.project.name = "DocB_LocalDirty".into());
        assert!(editor.dirty());
        assert!(editor.history.can_undo());
        assert!(!editor.selection.is_empty());
        editor.tick_autosave(0.0, 1_700_100_000);
        editor.tick_autosave(65.0, 1_700_100_065);
        let rec_id_b = editor.recovery.active_id().to_owned();
        assert!(rec_dir.path().join(format!("{rec_id_b}.nflash")).exists());

        // Tab 2: Imported XFL document
        editor.import_legacy(dom_xml.clone(), &ctx);
        assert_eq!(editor.session_count(), 3);
        assert_eq!(editor.active_session, 2);
        assert_eq!(editor.project.width, 640);
        assert!(!editor.check_external_modification());

        // Modify all 3 backing files externally:
        // - Tab 0 (`DocA.nflash`, clean inactive tab): modified in place
        let mut ext_a = Project::blank();
        ext_a.name = "DocA_ExternalV2".into();
        std::fs::write(&path_a, ext_a.to_json().unwrap()).unwrap();

        // - Tab 1 (`DocB.nflash`, dirty inactive tab): replaced atomically
        let tmp_b = dir.path().join("DocB.nflash.tmp");
        let mut ext_b = Project::blank();
        ext_b.name = "DocB_ExternalReplaced".into();
        std::fs::write(&tmp_b, ext_b.to_json().unwrap()).unwrap();
        std::fs::rename(&tmp_b, &path_b).unwrap();

        // - Tab 2 (`DOMDocument.xml`, active imported tab): modified in place (width 640 -> 960)
        std::fs::write(
            &dom_xml,
            r#"<DOMDocument width="960" height="540" frameRate="30"><timelines><DOMTimeline name="Scene 1"><layers><DOMLayer name="Layer 1"><frames><DOMFrame index="0"/></frames></DOMLayer></layers></DOMTimeline></timelines></DOMDocument>"#,
        )
        .unwrap();

        let output_before = editor.output.len();
        let changes = editor.check_external_file_changes();
        assert_eq!(
            changes,
            vec![
                (0, ExternalFileChange::Modified),
                (1, ExternalFileChange::Replaced),
                (2, ExternalFileChange::Modified),
            ]
        );
        let output_after_first_poll = editor.output.len();
        assert_eq!(
            output_after_first_poll - output_before,
            3,
            "one warning notification per affected tab"
        );

        // Polling again without further disk changes does not spam duplicate warnings.
        let changes_again = editor.check_external_file_changes();
        assert_eq!(changes_again, changes);
        assert_eq!(editor.output.len(), output_after_first_poll);

        // Reload active imported XFL tab (Tab 2) and verify updated dimensions + preserved import_source.
        assert!(editor.reload_external_modification(&ctx));
        assert_eq!(editor.project.width, 960);
        assert_eq!(editor.project.height, 540);
        assert!(editor.import_source.is_some());
        assert_eq!(editor.external_file_change(), None);

        // Reload inactive clean Tab 0 (`DocA.nflash`) directly without switching away from Tab 2.
        assert!(editor.reload_session_external_modification(&ctx, 0));
        assert_eq!(editor.session_external_file_change(0), None);
        editor.switch_session_tab(&ctx, 0);
        assert_eq!(editor.project.name, "DocA_ExternalV2");
        assert!(!editor.dirty());

        // Switch to dirty Tab 1 (`DocB.nflash`) and verify its conflict state survived tab switches.
        editor.switch_session_tab(&ctx, 1);
        assert_eq!(
            editor.external_file_change(),
            Some(ExternalFileChange::Replaced)
        );
        assert!(editor.has_external_change_prompt());
        assert_eq!(editor.project.name, "DocB_LocalDirty");

        // Save Guard: `editor.save(false)` (`⌘S`) and `editor.save_to_path(path_b)` MUST NOT overwrite
        // the unacknowledged external replacement on disk.
        assert!(!editor.save(false));
        assert!(!editor.save_to_path(path_b.clone()));
        let on_disk_b = Project::from_json(&std::fs::read_to_string(&path_b).unwrap()).unwrap();
        assert_eq!(
            on_disk_b.name, "DocB_ExternalReplaced",
            "unacknowledged Save must not overwrite external file on disk"
        );
        assert!(editor.has_external_change_prompt());

        // Corrupt reload refusal: if the disk file is corrupt, Reload must fail and preserve exact
        // in-memory project, undo/redo history, selection, saved_project, and recovery files.
        let snapshot_project = editor.project.clone();
        let snapshot_selection = editor.selection.clone();
        let snapshot_saved_project = editor.saved_project.clone();
        let snapshot_undo_len = editor.history.undo_len();
        std::fs::write(&path_b, "{corrupt json on disk").unwrap();
        assert!(!editor.reload_external_modification(&ctx));
        assert_eq!(editor.project, snapshot_project);
        assert_eq!(editor.selection, snapshot_selection);
        assert_eq!(editor.saved_project, snapshot_saved_project);
        assert_eq!(editor.history.undo_len(), snapshot_undo_len);
        assert!(editor.history.can_undo());
        assert!(rec_dir.path().join(format!("{rec_id_b}.nflash")).exists());

        // Restore valid external content on disk, then test `Keep editing` (Ignore).
        // Because `path_b` was atomically replaced earlier and not yet acknowledged, its inode still
        // differs from the tab's baseline stamp (`ExternalFileChange::Replaced`).
        std::fs::write(&path_b, ext_b.to_json().unwrap()).unwrap();
        let _ = editor.check_external_file_changes();
        assert_eq!(
            editor.external_file_change(),
            Some(ExternalFileChange::Replaced)
        );
        assert!(editor.ignore_external_modification());
        assert_eq!(editor.external_file_change(), None);
        assert!(!editor.has_external_change_prompt());
        assert!(
            editor.dirty(),
            "Keep editing keeps the buffer dirty relative to disk"
        );
        // Subsequent poll does not nag because the disk stamp was re-baselined to the current disk file.
        assert!(editor.check_external_file_changes().is_empty());

        // Also verify `keep_editing_session_external_change` on background tab 0 when modified in place.
        let mut ext_a2 = ext_a.clone();
        ext_a2.name = "DocA_BackgroundModifiedAgain".into();
        std::fs::write(&path_a, ext_a2.to_json().unwrap()).unwrap();
        assert_eq!(
            editor.check_external_file_changes(),
            vec![(0, ExternalFileChange::Modified)]
        );
        assert!(editor.keep_editing_session_external_change(0));
        assert_eq!(editor.session_external_file_change(0), None);
        assert!(editor.check_external_file_changes().is_empty());

        // After `Keep editing` acknowledged the conflict, `Save` (`⌘S`) writes the buffer and re-baselines cleanly.
        assert!(editor.save(false));
        assert!(!editor.dirty());
        assert_eq!(editor.external_file_change(), None);
        let saved_b = Project::from_json(&std::fs::read_to_string(&path_b).unwrap()).unwrap();
        assert_eq!(saved_b.name, "DocB_LocalDirty");
        assert!(editor.check_external_file_changes().is_empty());

        // Now test `Save As…` conflict resolution: modify `path_b` externally again, edit locally,
        // and resolve by saving to `save_as_path` (`DocB_Fork.nflash`), leaving `path_b` untouched.
        ext_b.name = "DocB_SecondExternalChange".into();
        std::fs::write(&path_b, ext_b.to_json().unwrap()).unwrap();
        editor.transact(|e| e.project.name = "DocB_ForkedWork".into());
        assert_eq!(
            editor.check_external_file_changes(),
            vec![(1, ExternalFileChange::Modified)]
        );
        assert!(!editor.save(false), "Save to original path is guarded");
        assert!(editor.save_as_external_change(save_as_path.clone()));
        assert!(!editor.dirty());
        assert_eq!(editor.external_file_change(), None);
        assert_eq!(
            Project::from_json(&std::fs::read_to_string(&path_b).unwrap())
                .unwrap()
                .name,
            "DocB_SecondExternalChange",
            "original conflicting file remains untouched after Save As"
        );
        assert_eq!(
            Project::from_json(&std::fs::read_to_string(&save_as_path).unwrap())
                .unwrap()
                .name,
            "DocB_ForkedWork"
        );

        // Finally, test `Deleted` conflict + Save Guard + `Keep editing` + `Save` recreation.
        std::fs::remove_file(&save_as_path).unwrap();
        assert_eq!(
            editor.check_external_file_changes(),
            vec![(1, ExternalFileChange::Deleted)]
        );
        assert!(!editor.reload_external_modification(&ctx));
        assert_eq!(editor.project.name, "DocB_ForkedWork");
        assert!(
            !editor.save(false),
            "Save is guarded while Deleted conflict is unacknowledged"
        );
        assert!(!save_as_path.exists());
        assert!(editor.keep_editing_external_change());
        assert!(editor.dirty());
        assert!(
            editor.check_external_file_changes().is_empty(),
            "acknowledged Deleted does not nag"
        );
        assert!(
            editor.save(false),
            "Save after Keep editing recreates the deleted file"
        );
        assert!(save_as_path.exists());
        assert!(!editor.dirty());
        assert!(editor.check_external_file_changes().is_empty());
    }
}
