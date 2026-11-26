//! Rollback journal for filesystem effects performed by one headless command.
//!
//! Exporters keep their established per-target preflight and ownership rules. The outer journal
//! snapshots only the paths those operations may change. JSFL adds each canonical path lazily,
//! immediately before its first mutation, so an `--allow-write` root is never copied wholesale.

use super::CliError;
use neoflash_export::SequenceCommitLease;
use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::fs::{self, Permissions};
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

const SEQUENCE_MANIFEST_FILENAME: &str = ".neoflash-sequences.json";
const SEQUENCE_LOCK_FILENAME: &str = ".neoflash-sequences.lock";

fn is_stable_sequence_lock_name(name: &OsStr) -> bool {
    name.to_str()
        .is_some_and(|n| n.eq_ignore_ascii_case(SEQUENCE_LOCK_FILENAME))
}

fn snapshot_preserves_directory(base: &Path, node: &SnapshotNode, target_dir: &Path) -> bool {
    let Ok(rel) = target_dir.strip_prefix(base) else {
        return false;
    };
    let mut current = node;
    for component in rel.components() {
        let std::path::Component::Normal(name) = component else {
            return false;
        };
        let SnapshotNode::Directory { entries, .. } = current else {
            return false;
        };
        let Some((_, child)) = entries.iter().find(|(n, _)| n == name) else {
            return false;
        };
        current = child;
    }
    matches!(current, SnapshotNode::Directory { .. })
}

#[derive(Debug)]
enum SnapshotNode {
    File {
        backup: PathBuf,
        permissions: Permissions,
    },
    Directory {
        permissions: Permissions,
        entries: Vec<(OsString, SnapshotNode)>,
    },
    Symlink {
        target: PathBuf,
        points_to_directory: bool,
    },
}

#[derive(Debug)]
enum OriginalPath {
    Absent,
    Present(SnapshotNode),
}

#[derive(Debug)]
struct JournalEntry {
    path: PathBuf,
    original: OriginalPath,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PlannedNumberedFiles {
    directory: PathBuf,
    stem: String,
    extensions: Vec<String>,
    include_plain: bool,
}

#[derive(Debug)]
struct NumberedFilesJournal {
    plan: PlannedNumberedFiles,
    original_paths: BTreeSet<PathBuf>,
    /// Earlier exact/tree journal entries own these paths' command-start baseline. Numbered
    /// rollback must neither delete nor restore them from a later intermediate snapshot.
    protected_paths: BTreeSet<PathBuf>,
    original_entries: Vec<JournalEntry>,
}

impl PlannedNumberedFiles {
    fn matches(&self, name: &OsStr) -> bool {
        let Some(name) = name.to_str() else {
            return false;
        };
        let Some((base, extension)) = name.rsplit_once('.') else {
            return false;
        };
        if !self
            .extensions
            .iter()
            .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        {
            return false;
        }
        if self.include_plain && base.eq_ignore_ascii_case(&self.stem) {
            return true;
        }
        let Some(prefix) = base.get(..self.stem.len()) else {
            return false;
        };
        let Some(suffix) = base.get(self.stem.len()..) else {
            return false;
        };
        prefix.eq_ignore_ascii_case(&self.stem)
            && suffix.strip_prefix('_').is_some_and(|digits| {
                !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
            })
    }
}

#[derive(Debug, Default)]
struct EffectState {
    planned: Vec<PathBuf>,
    planned_exact: Vec<PathBuf>,
    planned_numbered_files: Vec<PlannedNumberedFiles>,
    numbered_files_journal: Vec<NumberedFilesJournal>,
    leased_sequence_directories: BTreeSet<PathBuf>,
    sequence_leases: Vec<(PathBuf, SequenceCommitLease)>,
    journal: Vec<JournalEntry>,
    backup_dir: Option<PathBuf>,
    next_backup_id: u64,
    active: bool,
    finished: bool,
}

impl EffectState {
    fn numbered_path_was_absent(&self, path: &Path) -> bool {
        self.numbered_files_journal.iter().any(|entry| {
            entry.plan.directory == path.parent().unwrap_or_else(|| Path::new(""))
                && path
                    .file_name()
                    .is_some_and(|name| entry.plan.matches(name))
                && !entry.original_paths.contains(path)
        })
    }

    fn insert_planned_path(&mut self, path: PathBuf) {
        if self
            .planned
            .iter()
            .any(|existing| path.starts_with(existing))
        {
            return;
        }
        self.planned.retain(|existing| !existing.starts_with(&path));
        self.planned.push(path);
        self.planned.sort();
    }

    fn insert_planned_exact_path(&mut self, path: PathBuf) {
        if !self.planned_exact.contains(&path) {
            self.planned_exact.push(path);
            self.planned_exact.sort();
        }
    }

    fn expand_numbered_file_plans(&mut self) -> io::Result<()> {
        for plan in self.planned_numbered_files.clone() {
            let mut original_paths = Vec::new();
            match fs::symlink_metadata(&plan.directory) {
                Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                    for entry in fs::read_dir(&plan.directory)? {
                        let entry = entry?;
                        if plan.matches(&entry.file_name()) {
                            original_paths.push(entry.path());
                        }
                    }
                }
                Ok(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!(
                            "numbered export directory '{}' is not a directory",
                            plan.directory.display()
                        ),
                    ));
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            original_paths.sort();
            let original_entries = self.snapshot_numbered_originals(&original_paths)?;
            self.numbered_files_journal.push(NumberedFilesJournal {
                plan,
                original_paths: original_paths.into_iter().collect(),
                protected_paths: BTreeSet::new(),
                original_entries,
            });
        }
        Ok(())
    }

    fn snapshot_numbered_originals(
        &mut self,
        original_paths: &[PathBuf],
    ) -> io::Result<Vec<JournalEntry>> {
        original_paths
            .iter()
            .map(|path| {
                Ok(JournalEntry {
                    path: path.clone(),
                    original: OriginalPath::Present(snapshot_node(path, self)?),
                })
            })
            .collect()
    }

    fn capture_path_and_missing_parents(&mut self, path: &Path) -> io::Result<()> {
        if path.file_name().is_some_and(is_stable_sequence_lock_name) {
            return Ok(());
        }
        if self.numbered_path_was_absent(path) {
            if !self
                .journal
                .iter()
                .any(|entry| path.starts_with(&entry.path))
            {
                self.journal.push(JournalEntry {
                    path: path.to_path_buf(),
                    original: OriginalPath::Absent,
                });
            }
            return Ok(());
        }
        if self
            .journal
            .iter()
            .any(|entry| path.starts_with(&entry.path))
        {
            return Ok(());
        }

        let mut highest_missing = None;
        let mut cursor = path;
        loop {
            match fs::symlink_metadata(cursor) {
                Ok(_) => break,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    highest_missing = Some(cursor.to_path_buf());
                    let Some(parent) = cursor.parent() else {
                        break;
                    };
                    cursor = parent;
                }
                Err(error) => return Err(error),
            }
        }

        if let Some(path) = highest_missing {
            self.journal.push(JournalEntry {
                path,
                original: OriginalPath::Absent,
            });
            return Ok(());
        }

        let original = snapshot_node(path, self)?;
        self.journal.push(JournalEntry {
            path: path.to_path_buf(),
            original: OriginalPath::Present(original),
        });
        Ok(())
    }

    fn capture_exact_path(&mut self, path: &Path) -> io::Result<()> {
        if path.file_name().is_some_and(is_stable_sequence_lock_name) {
            return Ok(());
        }
        if self
            .journal
            .iter()
            .any(|entry| path.starts_with(&entry.path))
        {
            return Ok(());
        }
        let original = match fs::symlink_metadata(path) {
            Ok(_) => OriginalPath::Present(snapshot_node(path, self)?),
            Err(error) if error.kind() == io::ErrorKind::NotFound => OriginalPath::Absent,
            Err(error) => return Err(error),
        };
        self.journal.push(JournalEntry {
            path: path.to_path_buf(),
            original,
        });
        Ok(())
    }

    fn acquire_sequence_lease(&mut self, directory: &Path) -> io::Result<()> {
        if self.leased_sequence_directories.contains(directory) {
            return Ok(());
        }
        fs::create_dir_all(directory)?;
        let lease = SequenceCommitLease::acquire(directory)
            .map_err(|error| io::Error::other(error.to_string()))?;
        self.leased_sequence_directories
            .insert(directory.to_path_buf());
        self.sequence_leases.push((directory.to_path_buf(), lease));
        Ok(())
    }

    fn rollback(&mut self) -> io::Result<()> {
        if self.finished {
            return Ok(());
        }
        if !self.active {
            self.clear_journal()?;
            self.finished = true;
            return Ok(());
        }

        let mut failures = Vec::new();
        for entry in self.journal.iter().rev() {
            self.sequence_leases.retain(|(dir, _)| {
                let preserved = match &entry.original {
                    OriginalPath::Absent => !dir.starts_with(&entry.path),
                    OriginalPath::Present(node) => {
                        !dir.starts_with(&entry.path)
                            || snapshot_preserves_directory(&entry.path, node, dir)
                    }
                };
                if preserved {
                    true
                } else {
                    self.leased_sequence_directories.remove(dir);
                    false
                }
            });
            if let Err(error) = restore_entry(entry) {
                failures.push(format!("'{}': {error}", entry.path.display()));
            }
        }
        // A tree planned after an earlier sequence export necessarily snapshots the intermediate
        // generated files. Restore broad trees first, then replay numbered baselines in reverse
        // operation order so an intermediate family can never become the command's final state.
        for entry in self.numbered_files_journal.iter().rev() {
            if let Err(error) = remove_matching_files(&entry.plan, &entry.protected_paths) {
                failures.push(format!(
                    "numbered files in '{}': {error}",
                    entry.plan.directory.display()
                ));
                continue;
            }
            for original in &entry.original_entries {
                if let Err(error) = restore_entry(original) {
                    failures.push(format!("'{}': {error}", original.path.display()));
                }
            }
        }
        if failures.is_empty() {
            self.clear_journal()?;
            self.finished = true;
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "could not restore {} headless effect target(s): {}",
                failures.len(),
                failures.join("; ")
            )))
        }
    }

    fn clear_journal(&mut self) -> io::Result<()> {
        if let Some(directory) = self.backup_dir.as_ref() {
            match fs::remove_dir_all(directory) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        self.backup_dir = None;
        self.journal.clear();
        self.numbered_files_journal.clear();
        self.sequence_leases.clear();
        self.leased_sequence_directories.clear();
        self.next_backup_id = 0;
        Ok(())
    }

    fn allocate_backup_file(&mut self, source: &Path) -> io::Result<PathBuf> {
        let directory = match self.backup_dir.as_ref() {
            Some(directory) => {
                if directory.starts_with(source) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!(
                            "cannot journal broad target '{}' because it contains the rollback store",
                            source.display()
                        ),
                    ));
                }
                if source.starts_with(directory) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!(
                            "cannot journal rollback-store member '{}' into the same rollback store",
                            source.display()
                        ),
                    ));
                }
                directory.clone()
            }
            None => {
                let directory = std::env::temp_dir().join(format!(
                    "neoflash-headless-rollback-{}-{}",
                    std::process::id(),
                    neoflash_core::new_id("journal")
                ));
                if directory.starts_with(source) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!(
                            "cannot journal broad target '{}' inside the system temporary directory",
                            source.display()
                        ),
                    ));
                }
                create_private_backup_dir(&directory)?;
                self.backup_dir = Some(directory.clone());
                directory
            }
        };
        let path = directory.join(format!("{:016x}.bin", self.next_backup_id));
        self.next_backup_id = self.next_backup_id.saturating_add(1);
        Ok(path)
    }
}

/// One all-or-nothing boundary for the real filesystem effects of a headless command.
#[derive(Debug, Default)]
pub(crate) struct HeadlessEffectTransaction {
    state: Arc<Mutex<EffectState>>,
}

/// Non-owning hook used by `FLfile` immediately before a concrete host mutation.
#[derive(Clone, Debug)]
pub(crate) struct HeadlessEffectHandle {
    state: Arc<Mutex<EffectState>>,
}

impl HeadlessEffectTransaction {
    pub(crate) fn plan_path(&self, path: impl AsRef<Path>) -> Result<(), CliError> {
        self.plan_resolved_path(path.as_ref(), false)
    }

    /// Track a directory whose contents are mutated in place, following a final directory symlink.
    pub(crate) fn plan_tree(&self, path: impl AsRef<Path>) -> Result<(), CliError> {
        let (path, transient_paths) =
            resolve_effect_path(path.as_ref(), true).map_err(effect_error)?;
        reject_snapshot_containing_backup_base(&path).map_err(effect_error)?;
        let linked_paths = tree_symlink_targets(&path).map_err(effect_error)?;
        let mut state = lock_state(&self.state)?;
        for transient in transient_paths {
            plan_or_capture_path(&mut state, transient).map_err(effect_error)?;
        }
        plan_or_capture_path(&mut state, path).map_err(effect_error)?;
        for linked_path in linked_paths {
            plan_or_capture_path(&mut state, linked_path).map_err(effect_error)?;
        }
        Ok(())
    }

    fn plan_resolved_path(
        &self,
        path: &Path,
        follow_final_directory: bool,
    ) -> Result<(), CliError> {
        let (path, transient_paths) =
            resolve_effect_path(path, follow_final_directory).map_err(effect_error)?;
        reject_snapshot_containing_backup_base(&path).map_err(effect_error)?;
        let mut state = lock_state(&self.state)?;
        for transient in transient_paths {
            plan_or_capture_path(&mut state, transient).map_err(effect_error)?;
        }
        plan_or_capture_path(&mut state, path).map_err(effect_error)?;
        Ok(())
    }

    /// Track one output family (`name.ext`, `name_0001.ext`, atlas `name_2.ext`, …) without
    /// snapshotting unrelated siblings in the destination directory.
    pub(crate) fn plan_numbered_files(
        &self,
        directory: impl AsRef<Path>,
        stem: impl Into<String>,
        extensions: impl IntoIterator<Item = impl Into<String>>,
        include_plain: bool,
        sequence_sidecars: bool,
    ) -> Result<(), CliError> {
        let (directory, transient_paths) =
            resolve_effect_path(directory.as_ref(), true).map_err(effect_error)?;
        let stem = stem.into();
        let mut extensions = extensions
            .into_iter()
            .map(Into::into)
            .map(|value: String| value.to_ascii_lowercase())
            .collect::<Vec<_>>();
        extensions.sort();
        extensions.dedup();
        if stem.is_empty() || extensions.is_empty() {
            return Err(CliError::runtime(
                "effect_transaction_error",
                "Numbered export effects require a non-empty stem and extension",
            ));
        }

        let mut state = lock_state(&self.state)?;
        for transient in transient_paths {
            plan_or_capture_path(&mut state, transient).map_err(effect_error)?;
        }
        let plan = PlannedNumberedFiles {
            directory: directory.clone(),
            stem,
            extensions,
            include_plain,
        };
        if state.active {
            state
                .acquire_sequence_lease(&directory)
                .map_err(effect_error)?;
        }
        // A previously captured ancestor tree already owns the command-start baseline for every
        // member of this sequence. A second, later numbered baseline would observe intermediate
        // output and overwrite the correct tree restoration during rollback.
        let covered_by_existing_journal = state
            .journal
            .iter()
            .any(|entry| directory.starts_with(&entry.path));
        if state.active && covered_by_existing_journal {
            return Ok(());
        }
        if sequence_sidecars {
            let manifest = directory.join(SEQUENCE_MANIFEST_FILENAME);
            if state.active {
                state.capture_exact_path(&manifest).map_err(effect_error)?;
            } else {
                state.insert_planned_exact_path(manifest);
            }
        }
        if state.active
            && state
                .numbered_files_journal
                .iter()
                .any(|entry| entry.plan == plan)
        {
            return Ok(());
        }
        if state.active {
            let mut paths = matching_paths(&plan).map_err(effect_error)?;
            paths.sort();
            let original_paths = paths.iter().cloned().collect();
            let protected_paths = paths
                .iter()
                .filter(|path| {
                    state
                        .journal
                        .iter()
                        .any(|entry| journal_path_covers(&entry.path, path))
                })
                .cloned()
                .collect::<BTreeSet<_>>();
            let snapshot_paths = paths
                .iter()
                .filter(|path| !protected_paths.contains(*path))
                .cloned()
                .collect::<Vec<_>>();
            let original_entries = state
                .snapshot_numbered_originals(&snapshot_paths)
                .map_err(effect_error)?;
            state.numbered_files_journal.push(NumberedFilesJournal {
                plan,
                original_paths,
                protected_paths,
                original_entries,
            });
        } else if !state.planned_numbered_files.contains(&plan) {
            state.planned_numbered_files.push(plan);
        }
        Ok(())
    }

    /// Capture every statically planned target immediately before the first external mutation.
    pub(crate) fn activate(&self) -> Result<(), CliError> {
        let mut state = lock_state(&self.state)?;
        if state.active {
            return Ok(());
        }
        // Hold every stable sequence lock before inspecting any frame or manifest. This keeps the
        // command baseline coherent with concurrent exporters through the eventual commit/rollback.
        let sequence_directories = state
            .planned_numbered_files
            .iter()
            .map(|plan| plan.directory.clone())
            .collect::<BTreeSet<_>>();
        for directory in sequence_directories {
            if let Err(error) = state.acquire_sequence_lease(&directory) {
                return Err(clear_after_activation_error(&mut state, error));
            }
        }
        if let Err(error) = state.expand_numbered_file_plans() {
            return Err(clear_after_activation_error(&mut state, error));
        }
        for path in state.planned.clone() {
            if let Err(error) = state.capture_path_and_missing_parents(&path) {
                return Err(clear_after_activation_error(&mut state, error));
            }
        }
        for path in state.planned_exact.clone() {
            if let Err(error) = state.capture_exact_path(&path) {
                return Err(clear_after_activation_error(&mut state, error));
            }
        }
        state.active = true;
        Ok(())
    }

    pub(crate) fn handle(&self) -> HeadlessEffectHandle {
        HeadlessEffectHandle {
            state: Arc::clone(&self.state),
        }
    }

    /// Mark a command committed after its success response has been written.
    ///
    /// Cleanup is deliberately best-effort at this point: a cleanup failure must never emit a
    /// second protocol response or roll filesystem effects back after success was already visible.
    pub(crate) fn finalize_after_response(self) {
        if let Ok(mut state) = self.state.lock() {
            state.finished = true;
            let _ = state.clear_journal();
        }
    }

    pub(crate) fn rollback(&self) -> Result<(), CliError> {
        let mut state = lock_state(&self.state)?;
        state
            .rollback()
            .map_err(|error| CliError::runtime("effect_rollback_failed", error.to_string()))
    }
}

impl HeadlessEffectHandle {
    pub(crate) fn capture_path_before_write(&self, path: &Path) -> Result<(), String> {
        let (path, transient_paths) =
            resolve_effect_path(path, false).map_err(|error| error.to_string())?;
        reject_snapshot_containing_backup_base(&path).map_err(|error| error.to_string())?;
        for transient in &transient_paths {
            reject_snapshot_containing_backup_base(transient).map_err(|error| error.to_string())?;
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| "Headless effect transaction lock was poisoned".to_owned())?;
        if !state.active || state.finished {
            return Err("Headless effect transaction is not active".to_owned());
        }
        for transient in transient_paths {
            state
                .capture_path_and_missing_parents(&transient)
                .map_err(|error| error.to_string())?;
        }
        state
            .capture_path_and_missing_parents(&path)
            .map_err(|error| error.to_string())
    }
}

impl Drop for HeadlessEffectTransaction {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.lock() {
            if state.finished {
                let _ = state.clear_journal();
            } else {
                let _ = state.rollback();
            }
        }
    }
}

fn lock_state(state: &Arc<Mutex<EffectState>>) -> Result<MutexGuard<'_, EffectState>, CliError> {
    state.lock().map_err(|_| {
        CliError::runtime(
            "effect_transaction_error",
            "Headless effect transaction lock was poisoned",
        )
    })
}

fn effect_error(error: io::Error) -> CliError {
    CliError::runtime("effect_transaction_error", error.to_string())
}

fn clear_after_activation_error(state: &mut EffectState, original: io::Error) -> CliError {
    match state.clear_journal() {
        Ok(()) => effect_error(original),
        Err(cleanup) => effect_error(io::Error::other(format!(
            "{original}; rollback-store cleanup also failed: {cleanup}"
        ))),
    }
}

#[cfg(unix)]
fn create_private_backup_dir(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt as _;

    fs::DirBuilder::new().mode(0o700).create(path)
}

#[cfg(not(unix))]
fn create_private_backup_dir(path: &Path) -> io::Result<()> {
    fs::create_dir(path)
}

fn reject_snapshot_containing_backup_base(path: &Path) -> io::Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Ok(());
    }
    let snapshot_root = fs::canonicalize(path)?;
    let backup_base = fs::canonicalize(std::env::temp_dir())?;
    if backup_base.starts_with(&snapshot_root) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "headless effect target '{}' contains the rollback-store base '{}'; choose a dedicated output path",
                snapshot_root.display(),
                backup_base.display()
            ),
        ));
    }
    Ok(())
}

fn plan_or_capture_path(state: &mut EffectState, path: PathBuf) -> io::Result<()> {
    if state.active {
        state.capture_path_and_missing_parents(&path)
    } else {
        state.insert_planned_path(path);
        Ok(())
    }
}

/// Resolve existing directory ancestors with filesystem semantics (including symlinks and `..`)
/// while preserving a final file symlink that an atomic replacement would replace as a path.
fn resolve_effect_path(
    path: &Path,
    follow_final_directory: bool,
) -> io::Result<(PathBuf, Vec<PathBuf>)> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };

    if follow_final_directory && fs::metadata(&absolute).is_ok_and(|target| target.is_dir()) {
        return fs::canonicalize(&absolute).map(|path| (path, Vec::new()));
    }

    let Some(file_name) = absolute.file_name() else {
        return resolve_directory_path(&absolute);
    };
    let parent = absolute.parent().unwrap_or_else(|| Path::new("/"));
    let (parent, transient_paths) = resolve_directory_path(parent)?;
    Ok((parent.join(file_name), transient_paths))
}

fn resolve_directory_path(path: &Path) -> io::Result<(PathBuf, Vec<PathBuf>)> {
    let mut resolved = PathBuf::new();
    let mut missing = Vec::<OsString>::new();
    let mut missing_root = None;
    let mut transient_paths = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::Prefix(prefix) => resolved.push(prefix.as_os_str()),
            std::path::Component::RootDir => resolved.push(component.as_os_str()),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if missing.pop().is_none() {
                    resolved.pop();
                } else if missing.is_empty()
                    && let Some(transient) = missing_root.take()
                {
                    transient_paths.push(transient);
                }
            }
            std::path::Component::Normal(name) if !missing.is_empty() => {
                missing.push(name.to_os_string());
            }
            std::path::Component::Normal(name) => {
                let candidate = resolved.join(name);
                match fs::metadata(&candidate) {
                    Ok(metadata) if metadata.is_dir() => resolved = fs::canonicalize(candidate)?,
                    Ok(_) => {
                        return Err(io::Error::new(
                            io::ErrorKind::NotADirectory,
                            format!("'{}' is not a directory", candidate.display()),
                        ));
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        missing_root = Some(candidate);
                        missing.push(name.to_os_string());
                    }
                    Err(error) => return Err(error),
                }
            }
        }
    }
    for component in missing {
        resolved.push(component);
    }
    transient_paths.sort();
    transient_paths.dedup();
    Ok((resolved, transient_paths))
}

/// Return file and directory targets reachable through symlinks inside an owned output tree.
/// Exporters are then free to keep their existing symlink behaviour without letting a write through
/// an internal link escape the command-level rollback boundary.
fn tree_symlink_targets(root: &Path) -> io::Result<Vec<PathBuf>> {
    let metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Ok(Vec::new());
    }

    let canonical_root = fs::canonicalize(root)?;
    let mut stack = vec![canonical_root.clone()];
    let mut visited = BTreeSet::new();
    let mut targets = BTreeSet::new();
    while let Some(directory) = stack.pop() {
        let canonical = fs::canonicalize(&directory)?;
        if !visited.insert(canonical.clone()) {
            continue;
        }
        for entry in fs::read_dir(&canonical)? {
            let entry = entry?;
            let metadata = fs::symlink_metadata(entry.path())?;
            if metadata.file_type().is_symlink() {
                match fs::metadata(entry.path()) {
                    Ok(target) if target.is_dir() => {
                        let target = fs::canonicalize(entry.path())?;
                        if canonical_root.starts_with(&target) && target != canonical_root {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidInput,
                                format!(
                                    "output tree symlink '{}' points to ancestor '{}' and cannot be journalled safely",
                                    entry.path().display(),
                                    target.display()
                                ),
                            ));
                        }
                        if !target.starts_with(&canonical_root) {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidInput,
                                format!(
                                    "output tree directory symlink '{}' escapes to '{}'; use a real directory inside the output tree",
                                    entry.path().display(),
                                    target.display()
                                ),
                            ));
                        }
                        stack.push(target);
                    }
                    Ok(_) => {
                        let target = fs::canonicalize(entry.path())?;
                        if !target.starts_with(&canonical_root) {
                            targets.insert(target);
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        let link = fs::read_link(entry.path())?;
                        let raw_target = if link.is_absolute() {
                            link
                        } else {
                            entry
                                .path()
                                .parent()
                                .unwrap_or_else(|| Path::new("."))
                                .join(link)
                        };
                        let (target, transient_paths) = resolve_effect_path(&raw_target, false)?;
                        for transient in transient_paths {
                            if !transient.starts_with(&canonical_root) {
                                targets.insert(transient);
                            }
                        }
                        if !target.starts_with(&canonical_root) {
                            targets.insert(target);
                        }
                    }
                    Err(error) => return Err(error),
                }
            } else if metadata.is_dir() {
                stack.push(entry.path());
            }
        }
    }
    Ok(targets.into_iter().collect())
}

fn snapshot_node(path: &Path, state: &mut EffectState) -> io::Result<SnapshotNode> {
    let metadata = fs::symlink_metadata(path)?;
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        return Ok(SnapshotNode::Symlink {
            target: fs::read_link(path)?,
            points_to_directory: symlink_points_to_directory(path, &file_type),
        });
    }
    if file_type.is_file() {
        let backup = state.allocate_backup_file(path)?;
        fs::copy(path, &backup)?;
        return Ok(SnapshotNode::File {
            backup,
            permissions: metadata.permissions(),
        });
    }
    if file_type.is_dir() {
        // Freeze the entry list before the first file snapshot can create the rollback store.
        // This prevents a broad directory walk from observing and recursively copying its own
        // journal even if the platform's `read_dir` iterator reflects concurrent additions.
        let mut children = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
        children.sort_by_key(fs::DirEntry::file_name);
        let mut entries = Vec::with_capacity(children.len());
        for entry in children {
            if is_stable_sequence_lock_name(&entry.file_name())
                || state.numbered_path_was_absent(&entry.path())
            {
                continue;
            }
            entries.push((entry.file_name(), snapshot_node(&entry.path(), state)?));
        }
        return Ok(SnapshotNode::Directory {
            permissions: metadata.permissions(),
            entries,
        });
    }
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        format!(
            "headless effect target '{}' is not a regular file, directory, or symlink",
            path.display()
        ),
    ))
}

#[cfg(windows)]
fn symlink_points_to_directory(path: &Path, file_type: &fs::FileType) -> bool {
    use std::os::windows::fs::FileTypeExt as _;

    file_type.is_symlink_dir() || fs::metadata(path).is_ok_and(|value| value.is_dir())
}

#[cfg(not(windows))]
fn symlink_points_to_directory(path: &Path, _file_type: &fs::FileType) -> bool {
    fs::metadata(path).is_ok_and(|value| value.is_dir())
}

fn remove_matching_files(
    plan: &PlannedNumberedFiles,
    protected_paths: &BTreeSet<PathBuf>,
) -> io::Result<()> {
    let mut paths = matching_paths(plan)?;
    paths.sort();
    for path in paths.into_iter().rev() {
        if !protected_paths
            .iter()
            .any(|protected| paths_resolve_to_same_entry(protected, &path))
        {
            remove_path_if_present(&path)?;
        }
    }
    Ok(())
}

fn journal_path_covers(journal_path: &Path, candidate: &Path) -> bool {
    if candidate.starts_with(journal_path) {
        return true;
    }
    match (fs::canonicalize(journal_path), fs::canonicalize(candidate)) {
        (Ok(journal_path), Ok(candidate)) => candidate.starts_with(journal_path),
        _ => false,
    }
}

fn paths_resolve_to_same_entry(left: &Path, right: &Path) -> bool {
    left == right
        || matches!(
            (fs::canonicalize(left), fs::canonicalize(right)),
            (Ok(left), Ok(right)) if left == right
        )
}

fn matching_paths(plan: &PlannedNumberedFiles) -> io::Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(&plan.directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry?;
        if plan.matches(&entry.file_name()) {
            paths.push(entry.path());
        }
    }
    Ok(paths)
}

fn restore_entry(entry: &JournalEntry) -> io::Result<()> {
    match &entry.original {
        OriginalPath::Absent => remove_path_if_present(&entry.path),
        OriginalPath::Present(node) => {
            if let Some(parent) = entry.path.parent()
                && !parent.as_os_str().is_empty()
            {
                fs::create_dir_all(parent)?;
            }
            restore_node(&entry.path, node)
        }
    }
}

fn remove_path_if_present(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        #[cfg(windows)]
        Ok(metadata) if windows_directory_symlink(&metadata.file_type()) => fs::remove_dir(path),
        Ok(metadata) if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() => {
            fs::remove_dir_all(path)
        }
        Ok(_) => fs::remove_file(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(windows)]
fn windows_directory_symlink(file_type: &fs::FileType) -> bool {
    use std::os::windows::fs::FileTypeExt as _;

    file_type.is_symlink_dir()
}

fn restore_node(path: &Path, node: &SnapshotNode) -> io::Result<()> {
    match node {
        SnapshotNode::File {
            backup,
            permissions,
        } => {
            if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file())
                && files_equal(path, backup)?
            {
                if fs::metadata(path)?.permissions() != *permissions {
                    fs::set_permissions(path, permissions.clone())?;
                }
                return Ok(());
            }
            remove_path_if_present(path)?;
            fs::copy(backup, path)?;
            fs::set_permissions(path, permissions.clone())
        }
        SnapshotNode::Directory {
            permissions,
            entries,
        } => {
            if !fs::symlink_metadata(path)
                .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
            {
                remove_path_if_present(path)?;
                fs::create_dir(path)?;
            }
            let original_names = entries
                .iter()
                .map(|(name, _)| name)
                .collect::<BTreeSet<_>>();
            for current in fs::read_dir(path)? {
                let current = current?;
                if !is_stable_sequence_lock_name(&current.file_name())
                    && !original_names.contains(&current.file_name())
                {
                    remove_path_if_present(&current.path())?;
                }
            }
            for (name, child) in entries {
                if is_stable_sequence_lock_name(name) {
                    continue;
                }
                restore_node(&path.join(name), child)?;
            }
            if fs::metadata(path)?.permissions() != *permissions {
                fs::set_permissions(path, permissions.clone())?;
            }
            Ok(())
        }
        SnapshotNode::Symlink {
            target,
            points_to_directory,
        } => {
            if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink())
                && fs::read_link(path).is_ok_and(|current| current == *target)
            {
                return Ok(());
            }
            remove_path_if_present(path)?;
            recreate_symlink(target, path, *points_to_directory)
        }
    }
}

fn files_equal(left: &Path, right: &Path) -> io::Result<bool> {
    if fs::metadata(left)?.len() != fs::metadata(right)?.len() {
        return Ok(false);
    }
    let mut left = BufReader::new(fs::File::open(left)?);
    let mut right = BufReader::new(fs::File::open(right)?);
    let mut left_buffer = [0_u8; 64 * 1024];
    let mut right_buffer = [0_u8; 64 * 1024];
    loop {
        let left_count = left.read(&mut left_buffer)?;
        let right_count = right.read(&mut right_buffer)?;
        if left_count != right_count || left_buffer[..left_count] != right_buffer[..right_count] {
            return Ok(false);
        }
        if left_count == 0 {
            return Ok(true);
        }
    }
}

#[cfg(unix)]
fn recreate_symlink(target: &Path, path: &Path, _points_to_directory: bool) -> io::Result<()> {
    std::os::unix::fs::symlink(target, path)
}

#[cfg(windows)]
fn recreate_symlink(target: &Path, path: &Path, points_to_directory: bool) -> io::Result<()> {
    if points_to_directory {
        std::os::windows::fs::symlink_dir(target, path)
    } else {
        std::os::windows::fs::symlink_file(target, path)
    }
}

#[cfg(not(any(unix, windows)))]
fn recreate_symlink(_target: &Path, path: &Path, _points_to_directory: bool) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        format!(
            "cannot restore symlink '{}' on this platform",
            path.display()
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "neoflash-headless-effects-{label}-{}-{}",
            std::process::id(),
            neoflash_core::new_id("test")
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn rollback_restores_files_directories_and_absent_parent_chain() {
        let root = temp_root("basic");
        fs::create_dir_all(root.join("tree/sub")).unwrap();
        fs::write(root.join("tree/original.txt"), b"original").unwrap();
        fs::write(root.join("tree/sub/keep.bin"), [0_u8, 1, 2, 3]).unwrap();

        let tx = HeadlessEffectTransaction::default();
        tx.plan_path(root.join("tree")).unwrap();
        tx.plan_path(root.join("new/deep/output.txt")).unwrap();
        tx.activate().unwrap();

        fs::write(root.join("tree/original.txt"), b"changed").unwrap();
        fs::remove_file(root.join("tree/sub/keep.bin")).unwrap();
        fs::write(root.join("tree/created.txt"), b"created").unwrap();
        fs::create_dir_all(root.join("new/deep")).unwrap();
        fs::write(root.join("new/deep/output.txt"), b"new").unwrap();

        tx.rollback().unwrap();
        assert_eq!(
            fs::read(root.join("tree/original.txt")).unwrap(),
            b"original"
        );
        assert_eq!(
            fs::read(root.join("tree/sub/keep.bin")).unwrap(),
            [0_u8, 1, 2, 3]
        );
        assert!(!root.join("tree/created.txt").exists());
        assert!(!root.join("new").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn numbered_rollback_is_scoped_and_commit_keeps_replacements() {
        let root = temp_root("numbered");
        fs::write(root.join("frames_0001.png"), b"old-frame").unwrap();
        fs::write(root.join("unrelated.txt"), b"before").unwrap();

        let tx = HeadlessEffectTransaction::default();
        tx.plan_numbered_files(&root, "frames", ["png"], true, true)
            .unwrap();
        tx.activate().unwrap();
        fs::write(root.join("frames_0001.png"), b"new-frame").unwrap();
        fs::write(root.join("frames_0002.png"), b"new-tail").unwrap();
        fs::write(root.join("unrelated.txt"), b"after").unwrap();
        tx.rollback().unwrap();

        assert_eq!(
            fs::read(root.join("frames_0001.png")).unwrap(),
            b"old-frame"
        );
        assert!(!root.join("frames_0002.png").exists());
        assert_eq!(fs::read(root.join("unrelated.txt")).unwrap(), b"after");

        let committed = HeadlessEffectTransaction::default();
        committed.plan_path(root.join("frames_0001.png")).unwrap();
        committed.activate().unwrap();
        fs::write(root.join("frames_0001.png"), b"committed").unwrap();
        committed.finalize_after_response();
        assert_eq!(
            fs::read(root.join("frames_0001.png")).unwrap(),
            b"committed"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn numbered_then_ancestor_tree_rollback_restores_the_command_baseline() {
        let root = temp_root("numbered-then-tree");
        let frame_one = root.join("frames_0001.png");
        let frame_two = root.join("frames_0002.png");
        let unrelated = root.join("notes.txt");
        fs::write(&frame_one, b"original frame").unwrap();
        fs::write(&unrelated, b"before tree operation").unwrap();

        let tx = HeadlessEffectTransaction::default();
        tx.plan_numbered_files(&root, "frames", ["png"], true, true)
            .unwrap();
        tx.activate().unwrap();
        fs::write(&frame_one, b"intermediate replacement").unwrap();
        fs::write(&frame_two, b"intermediate new tail").unwrap();

        // A later package-style export owns the ancestor tree. Its snapshot sees the intermediate
        // sequence, but rollback must still reach the baseline from before the whole command, and
        // must never snapshot `.neoflash-sequences.lock` (which holds a mandatory byte-range lock
        // on Windows via `LockFileEx`).
        tx.plan_tree(&root).unwrap();
        {
            let state = tx.state.lock().unwrap();
            let snap_has_lock = state.journal.iter().any(|entry| match &entry.original {
                OriginalPath::Present(SnapshotNode::Directory { entries, .. }) => entries
                    .iter()
                    .any(|(name, _)| is_stable_sequence_lock_name(name)),
                _ => false,
            });
            assert!(
                !snap_has_lock,
                "directory snapshot must never capture .neoflash-sequences.lock"
            );
        }
        fs::write(&unrelated, b"after tree operation").unwrap();
        fs::write(root.join("package-only.txt"), b"temporary").unwrap();
        tx.rollback().unwrap();

        assert_eq!(fs::read(&frame_one).unwrap(), b"original frame");
        assert!(!frame_two.exists());
        assert_eq!(fs::read(&unrelated).unwrap(), b"before tree operation");
        assert!(!root.join("package-only.txt").exists());
        assert!(root.join(".neoflash-sequences.lock").is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn ancestor_tree_then_numbered_rollback_restores_the_command_baseline() {
        let root = temp_root("tree-then-numbered");
        let frame_one = root.join("frames_0001.png");
        let frame_two = root.join("frames_0002.png");
        let unrelated = root.join("notes.txt");
        fs::write(&frame_one, b"original frame").unwrap();
        fs::write(&unrelated, b"original note").unwrap();

        let tx = HeadlessEffectTransaction::default();
        tx.plan_tree(&root).unwrap();
        tx.activate().unwrap();
        fs::write(&unrelated, b"intermediate tree output").unwrap();

        tx.plan_numbered_files(&root, "frames", ["png"], true, true)
            .unwrap();
        let nested_seq = root.join("nested_seq");
        tx.plan_numbered_files(&nested_seq, "subframes", ["png"], true, true)
            .unwrap();
        fs::write(nested_seq.join("subframes_0001.png"), b"nested frame").unwrap();
        fs::write(&frame_one, b"new frame").unwrap();
        fs::write(&frame_two, b"new tail").unwrap();
        tx.rollback().unwrap();

        assert_eq!(fs::read(&frame_one).unwrap(), b"original frame");
        assert!(!frame_two.exists());
        assert!(!nested_seq.exists());
        assert_eq!(fs::read(&unrelated).unwrap(), b"original note");
        assert!(root.join(".neoflash-sequences.lock").is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn exact_paths_then_numbered_rollback_keep_the_first_snapshots() {
        let root = temp_root("exact-then-numbered");
        let existing = root.join("frames_0001.png");
        let initially_absent = root.join("frames_0002.png");
        fs::write(&existing, b"command baseline").unwrap();

        let tx = HeadlessEffectTransaction::default();
        tx.plan_path(&existing).unwrap();
        tx.plan_path(&initially_absent).unwrap();
        tx.activate().unwrap();
        fs::write(&existing, b"intermediate exact export").unwrap();
        fs::write(&initially_absent, b"intermediate new exact export").unwrap();

        tx.plan_numbered_files(&root, "frames", ["png"], true, true)
            .unwrap();
        fs::write(&existing, b"later sequence frame one").unwrap();
        fs::write(&initially_absent, b"later sequence frame two").unwrap();
        fs::write(root.join("frames_0003.png"), b"later sequence tail").unwrap();
        tx.rollback().unwrap();

        assert_eq!(fs::read(&existing).unwrap(), b"command baseline");
        assert!(!initially_absent.exists());
        assert!(!root.join("frames_0003.png").exists());
        assert!(root.join(".neoflash-sequences.lock").is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn case_only_exact_then_numbered_overlap_keeps_the_first_snapshot() {
        let root = temp_root("case-exact-then-numbered");
        let stored = root.join("frames_0001.png");
        let requested = root.join("FRAMES_0001.PNG");
        fs::write(&stored, b"command baseline").unwrap();
        if fs::metadata(&requested).is_err() {
            let _ = fs::remove_dir_all(root);
            return;
        }

        let tx = HeadlessEffectTransaction::default();
        tx.plan_path(&requested).unwrap();
        tx.activate().unwrap();
        fs::write(&requested, b"intermediate exact export").unwrap();
        tx.plan_numbered_files(&root, "frames", ["png"], true, true)
            .unwrap();
        fs::write(&stored, b"later sequence frame").unwrap();
        tx.rollback().unwrap();

        assert_eq!(fs::read(&stored).unwrap(), b"command baseline");
        assert!(root.join(".neoflash-sequences.lock").is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn broad_snapshot_root_containing_the_temp_store_is_rejected_before_scanning() {
        let tx = HeadlessEffectTransaction::default();
        let error = tx
            .plan_tree(std::env::temp_dir())
            .expect_err("the rollback store base cannot be snapshotted recursively");
        assert_eq!(error.code, "effect_transaction_error");
        assert!(error.message.contains("rollback-store base"));
    }

    #[test]
    fn dynamic_jsfl_capture_rejects_the_temp_store_ancestor_before_scanning() {
        let tx = HeadlessEffectTransaction::default();
        tx.activate().unwrap();
        let error = tx
            .handle()
            .capture_path_before_write(&std::env::temp_dir())
            .expect_err("dynamic FLfile capture must not snapshot the rollback-store base");
        assert!(error.contains("rollback-store base"));
        tx.rollback().unwrap();
    }

    #[test]
    fn finalize_cleanup_failure_never_rolls_back_visible_success() {
        let root = temp_root("finalize-cleanup-failure");
        let target = root.join("target.txt");
        let cleanup_blocker = root.join("cleanup-blocker");
        fs::write(&target, b"before").unwrap();
        fs::write(&cleanup_blocker, b"not a directory").unwrap();

        let tx = HeadlessEffectTransaction::default();
        tx.plan_path(&target).unwrap();
        tx.activate().unwrap();
        fs::write(&target, b"committed").unwrap();
        let actual_backup = {
            let mut state = tx.state.lock().unwrap();
            let actual = state.backup_dir.clone().unwrap();
            state.backup_dir = Some(cleanup_blocker.clone());
            actual
        };

        tx.finalize_after_response();
        assert_eq!(fs::read(&target).unwrap(), b"committed");

        // The injected cleanup error intentionally hid the real journal directory from cleanup.
        fs::remove_file(cleanup_blocker).unwrap();
        fs::remove_dir_all(actual_backup).unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn filesystem_resolution_honors_symlink_parent_before_dotdot() {
        use std::os::unix::fs::symlink;

        let root = temp_root("symlink-dotdot");
        fs::create_dir_all(root.join("real/nested")).unwrap();
        symlink(root.join("real/nested"), root.join("alias")).unwrap();
        let victim = root.join("real/victim.txt");
        fs::write(&victim, b"original").unwrap();
        let through_symlink = root.join("alias/../victim.txt");

        let tx = HeadlessEffectTransaction::default();
        tx.activate().unwrap();
        tx.handle()
            .capture_path_before_write(&through_symlink)
            .unwrap();
        fs::write(&through_symlink, b"changed").unwrap();
        tx.rollback().unwrap();

        assert_eq!(fs::read(&victim).unwrap(), b"original");
        assert_eq!(
            fs::read_link(root.join("alias")).unwrap(),
            root.join("real/nested")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rollback_removes_transient_missing_directory_before_dotdot() {
        let root = temp_root("missing-dotdot");
        fs::create_dir_all(root.join("existing")).unwrap();
        let raw_output = root.join("existing/transient/../output/result.txt");

        let tx = HeadlessEffectTransaction::default();
        tx.plan_path(&raw_output).unwrap();
        tx.activate().unwrap();

        // `create_dir_all` implementations may materialize the lexical component before `..`.
        fs::create_dir_all(root.join("existing/transient")).unwrap();
        fs::create_dir_all(root.join("existing/output")).unwrap();
        fs::write(root.join("existing/output/result.txt"), b"temporary").unwrap();
        tx.rollback().unwrap();

        assert!(!root.join("existing/transient").exists());
        assert!(!root.join("existing/output").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn tree_rollback_covers_directory_symlinks_inside_the_owned_tree() {
        use std::os::unix::fs::symlink;

        let root = temp_root("tree-symlink-target");
        let tree = root.join("tree");
        let owned = tree.join("owned");
        fs::create_dir_all(&owned).unwrap();
        fs::write(owned.join("owned.txt"), b"original").unwrap();
        symlink(&owned, tree.join("linked")).unwrap();

        let tx = HeadlessEffectTransaction::default();
        tx.plan_tree(&tree).unwrap();
        tx.activate().unwrap();
        fs::write(tree.join("linked/owned.txt"), b"changed").unwrap();
        fs::write(tree.join("linked/new.txt"), b"temporary").unwrap();
        tx.rollback().unwrap();

        assert_eq!(fs::read(owned.join("owned.txt")).unwrap(), b"original");
        assert!(!owned.join("new.txt").exists());
        assert_eq!(fs::read_link(tree.join("linked")).unwrap(), owned);
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn tree_planning_rejects_directory_symlinks_outside_the_owned_tree() {
        use std::os::unix::fs::symlink;

        let root = temp_root("tree-external-directory-symlink");
        let tree = root.join("tree");
        let outside = root.join("outside");
        fs::create_dir_all(&tree).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("unrelated.txt"), b"must remain untouched").unwrap();
        symlink(&outside, tree.join("linked")).unwrap();

        let tx = HeadlessEffectTransaction::default();
        let error = tx.plan_tree(&tree).unwrap_err();
        assert_eq!(error.code, "effect_transaction_error");
        assert!(error.message.contains("escapes"));
        assert_eq!(
            fs::read(outside.join("unrelated.txt")).unwrap(),
            b"must remain untouched"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn tree_rollback_covers_existing_and_dangling_file_symlink_targets() {
        use std::os::unix::fs::symlink;

        let root = temp_root("tree-file-symlink-target");
        let tree = root.join("tree");
        let outside = root.join("outside");
        fs::create_dir_all(&tree).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("existing.txt"), b"original").unwrap();
        symlink(outside.join("existing.txt"), tree.join("existing-link")).unwrap();
        symlink(outside.join("created.txt"), tree.join("dangling-link")).unwrap();

        let tx = HeadlessEffectTransaction::default();
        tx.plan_tree(&tree).unwrap();
        tx.activate().unwrap();
        fs::write(tree.join("existing-link"), b"changed").unwrap();
        fs::write(tree.join("dangling-link"), b"temporary").unwrap();
        tx.rollback().unwrap();

        assert_eq!(fs::read(outside.join("existing.txt")).unwrap(), b"original");
        assert!(!outside.join("created.txt").exists());
        assert_eq!(
            fs::read_link(tree.join("existing-link")).unwrap(),
            outside.join("existing.txt")
        );
        assert_eq!(
            fs::read_link(tree.join("dangling-link")).unwrap(),
            outside.join("created.txt")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn rollback_store_is_private_and_cleanup_failure_keeps_its_retry_path() {
        use std::os::unix::fs::PermissionsExt as _;

        let root = temp_root("private-backup");
        let target = root.join("target.txt");
        fs::write(&target, b"private bytes").unwrap();
        let tx = HeadlessEffectTransaction::default();
        tx.plan_path(&target).unwrap();
        tx.activate().unwrap();
        let backup_dir = {
            let state = tx.state.lock().unwrap();
            state.backup_dir.clone().unwrap()
        };
        assert_eq!(
            fs::metadata(&backup_dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        tx.rollback().unwrap();
        assert!(!backup_dir.exists());

        let retry_marker = root.join("not-a-directory");
        fs::write(&retry_marker, b"marker").unwrap();
        let mut state = EffectState {
            backup_dir: Some(retry_marker.clone()),
            journal: vec![JournalEntry {
                path: root.join("kept"),
                original: OriginalPath::Absent,
            }],
            ..EffectState::default()
        };
        assert!(state.clear_journal().is_err());
        assert_eq!(state.backup_dir.as_deref(), Some(retry_marker.as_path()));
        assert_eq!(state.journal.len(), 1);
        fs::remove_file(retry_marker).unwrap();
        state.backup_dir = None;
        let _ = fs::remove_dir_all(root);
    }
}
