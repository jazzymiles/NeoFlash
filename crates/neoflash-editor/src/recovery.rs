//! Autosave timer and crash-recovery storage (`Preferences ▸ General ▸ Autosave`).
//!
//! Recovery snapshots live exclusively under `<app_support>/Recovery/<stable_id>.nflash` paired
//! with a `<stable_id>.json` metadata sidecar, never next to the user's authored file.
use crate::app::Editor;
use eframe::egui;
use neoflash_core::{History, Project};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static UNTITLED_SEQ: AtomicU64 = AtomicU64::new(1);

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
        }
    }
}

impl RecoveryManager {
    /// Platform app-support recovery folder (`<eframe::storage_dir("NeoFlash")>/Recovery`).
    pub(crate) fn default_recovery_dir() -> Option<PathBuf> {
        eframe::storage_dir("NeoFlash").map(|dir| dir.join("Recovery"))
    }

    pub(crate) fn set_dir(&mut self, dir: Option<PathBuf>) {
        self.dir = dir;
    }

    #[cfg(test)]
    pub(crate) fn active_id(&self) -> &str {
        &self.active_id
    }

    /// Deterministic filename-safe ID for a saved document path (64-bit FNV-1a).
    pub(crate) fn stable_id_for_path(path: &Path) -> String {
        let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
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
    }

    pub(crate) fn reset_timer(&mut self) {
        self.dirty_anchor_secs = None;
    }

    /// Remove all recovery files owned by the current document session.
    pub(crate) fn clear_active(&mut self) -> Vec<String> {
        self.dirty_anchor_secs = None;
        let mut errors = Vec::new();
        if let Some(dir) = &self.dir {
            for id in &self.tracked_ids {
                errors.extend(remove_id_files(dir, id));
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
        }
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
    /// Advance the autosave timer using monotonic `now_secs`; returns time until next check when
    /// a dirty document is waiting for autosave.
    pub(crate) fn tick_autosave(&mut self, now_secs: f64, wall_unix_secs: u64) -> Option<Duration> {
        if !self.dirty() || !self.workspace.general.autosave {
            self.recovery.reset_timer();
            return None;
        }
        if self.recovery.has_prompt() {
            return None;
        }
        let minutes = self.workspace.general.autosave_minutes.clamp(1, 120);
        let interval_secs = f64::from(minutes) * 60.0;
        let anchor = self.recovery.dirty_anchor_secs.get_or_insert(now_secs);
        if now_secs < *anchor {
            *anchor = now_secs;
        }
        let elapsed = now_secs - *anchor;
        if elapsed >= interval_secs {
            self.recovery.dirty_anchor_secs = Some(now_secs);
            let doc_name = self
                .path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| self.project.name.clone());
            if let Err(err) = self.recovery.write_active(
                &self.project,
                self.path.as_deref(),
                &doc_name,
                wall_unix_secs,
            ) {
                self.message(format!("Autosave failed: {err}"));
            }
            Some(Duration::from_secs_f64(interval_secs))
        } else {
            Some(Duration::from_secs_f64((interval_secs - elapsed).max(0.25)))
        }
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
        let path = self.path.clone();
        for err in self.recovery.clear_and_retarget(path.as_deref()) {
            self.message(format!("Recovery cleanup failed: {err}"));
        }
    }

    /// Restore the front crash-recovery candidate as a dirty document preserving its original path.
    pub(crate) fn restore_recovery_prompt(&mut self, ctx: &egui::Context) {
        let Some(cand) = self.recovery.pop_prompt() else {
            return;
        };
        self.cancel_edits();
        self.project = cand.project;
        self.clipboard = Default::default();
        self.resource_panel = Default::default();
        self.library_panel = Default::default();
        self.project_panel = Default::default();
        self.document_settings = None;
        self.edit_envelope = None;
        self.cancel_sound_properties(ctx);
        self.cancel_layer_properties(ctx);
        self.close_symbols_popover(ctx);
        self.timeline_edit = Default::default();
        self.graphic_drop_position = None;
        self.bitmap_textures = Default::default();
        self.editing = vec![self.project.root.clone()];
        self.frame = 0;
        self.layer = 0;
        self.clear_selection();
        self.path = cand.original_path;
        self.saved_project = None;
        self.history = History::new(self.workspace.undo_levels as usize);
        self.playing = false;
        self.load_project_fonts(ctx);
        self.normalize_selection();
        self.recovery.adopt_restored(&cand.id, self.path.as_deref());
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
    fn recovery_lifecycle_deletes_on_save_discard_new_open_quit_and_restores_on_launch() {
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

        // 2. Autosave is deleted when replaced by File ▸ New (discarding changes).
        editor.transact(|e| e.project.name = "DiscardedBeforeNew".into());
        trigger_autosave(&mut editor, 100.0);
        assert_eq!(count_rec_files(), 2);
        editor.perform(Pending::New, &ctx);
        assert_eq!(count_rec_files(), 0, "New deletes discarded recovery");

        // 3. Autosave is deleted when replaced by Open (`open_path`).
        editor.transact(|e| e.project.name = "DiscardedBeforeOpen".into());
        trigger_autosave(&mut editor, 200.0);
        assert_eq!(count_rec_files(), 2);
        editor.open_path(other_path.clone(), &ctx);
        assert_eq!(count_rec_files(), 0, "Open deletes previous recovery");

        // 4. Autosave is deleted on Discard/Close (`perform(Pending::Close)`) and clean `on_exit`.
        editor.transact(|e| e.project.name = "DiscardedOnClose".into());
        trigger_autosave(&mut editor, 300.0);
        assert_eq!(count_rec_files(), 2);
        editor.perform(Pending::Close, &ctx);
        assert_eq!(count_rec_files(), 0, "Close after Discard deletes recovery");

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
}
