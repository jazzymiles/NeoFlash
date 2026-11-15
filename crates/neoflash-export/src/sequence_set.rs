//! Transactional multi-file frame sequence writer with manifest-tracked ownership (`T-331`).
//!
//! Sequence exports (`PNG`, `JPEG`, `WebP`, `GIF`, `BMP`, `SVG`, `AI`, `PSD`, `PDF`, video
//! `PngSequence`, and Publish `ImageSequence`) write multiple sibling files into a folder that
//! may also hold sprite-sheet atlases (`spritesheet.png` / `spritesheet.json`, `atlas.png` /
//! `atlas.json`), HTML5 bundles, or user assets.
//!
//! Contract:
//! 1. NeoFlash records exact owned filenames, byte lengths, and SHA-256 digests per sequence stem
//!    in a hidden sidecar manifest (`.neoflash-sequences.json`) inside the output directory.
//! 2. Before mutating any final path, [`SequenceSetTransaction::preflight`] inspects every planned
//!    output path (honoring the target directory's real filesystem case-sensitivity policy) and
//!    refuses if any path already exists on disk and is not an unmodified file owned by the prior
//!    manifest entry for the same sequence stem.
//! 3. All generated frames (and optional companion `.wav` audio) are written and synced inside a
//!    private sibling staging directory (`.neoflash-seq-stage-<pid>-<nanos>-<seq>`) on the same
//!    filesystem. Any error or cancellation before commit removes the staging directory and any
//!    newly created empty output directories without touching pre-existing files.
//! 4. Commit moves prior manifest-owned files (including stale tail frames from a longer earlier
//!    export) and the prior manifest into a rollback directory, renames the new staged set and
//!    updated manifest into place, and removes the rollback directory only after every rename
//!    succeeds. Any mid-swap failure restores the prior set and manifest byte-for-byte.

use crate::{ExportError, sha256_hex};
use neoflash_core::sha256::Sha256;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

/// Hidden manifest filename recording NeoFlash-owned sequence files in an output directory.
pub const SEQUENCE_MANIFEST_FILENAME: &str = ".neoflash-sequences.json";

/// Prefix for temporary sibling staging directories inside the target folder.
pub const SEQUENCE_STAGE_PREFIX: &str = ".neoflash-seq-stage-";

/// Default upper bound on staged files in one sequence transaction.
pub const MAX_SEQUENCE_STAGED_FILES: usize = 65_536;

/// Default upper bound on aggregate staged bytes in one sequence transaction (`2 GiB`).
pub const MAX_SEQUENCE_STAGED_BYTES: usize = 2 * 1024 * 1024 * 1024;

const MAX_MANIFEST_FILE_BYTES: u64 = 16 * 1024 * 1024;

static NEXT_SEQUENCE_STAGE_ID: AtomicU64 = AtomicU64::new(1);

/// Metadata for a single NeoFlash-owned file belonging to a sequence set.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SequenceOwnedFileRecord {
    pub name: String,
    pub bytes: u64,
    pub sha256: String,
}

/// Ownership record for one sequence stem inside a directory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SequenceManifestEntry {
    pub stem: String,
    pub files: Vec<SequenceOwnedFileRecord>,
}

/// Directory-level hidden manifest sidecar (`.neoflash-sequences.json`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SequenceDirectoryManifest {
    pub generator: String,
    pub format_version: u32,
    #[serde(default)]
    pub sequences: BTreeMap<String, SequenceManifestEntry>,
}

impl Default for SequenceDirectoryManifest {
    fn default() -> Self {
        Self {
            generator: "NeoFlash".to_owned(),
            format_version: 1,
            sequences: BTreeMap::new(),
        }
    }
}

struct StagedSequenceFile {
    file_name: String,
    staged_path: PathBuf,
    final_path: PathBuf,
    bytes: u64,
    sha256: String,
}

struct VerifiedPriorState {
    manifest: SequenceDirectoryManifest,
    manifest_existed: bool,
    prior_key: Option<String>,
    present_owned_names: Vec<String>,
}

/// Recoverable multi-file sequence transaction staged in the destination directory.
pub struct SequenceSetTransaction {
    directory: PathBuf,
    stem: String,
    stage_dir: PathBuf,
    files_dir: PathBuf,
    rollback_dir: PathBuf,
    created_dirs: Vec<PathBuf>,
    case_insensitive: bool,
    max_bytes: usize,
    max_files: usize,
    staged_bytes: usize,
    staged_folded_names: BTreeSet<String>,
    files: Vec<StagedSequenceFile>,
    committed: bool,
    cleanup_on_drop: bool,
}

impl SequenceSetTransaction {
    /// Start a new sequence transaction in `target_dir` for `stem` with default staging limits.
    pub fn begin(target_dir: &Path, stem: &str) -> Result<Self, ExportError> {
        Self::begin_with_limits(
            target_dir,
            stem,
            MAX_SEQUENCE_STAGED_BYTES,
            MAX_SEQUENCE_STAGED_FILES,
        )
    }

    /// Start a new sequence transaction with explicit aggregate byte and file limits.
    pub fn begin_with_limits(
        target_dir: &Path,
        stem: &str,
        max_bytes: usize,
        max_files: usize,
    ) -> Result<Self, ExportError> {
        let trimmed_stem = stem.trim();
        if trimmed_stem.is_empty()
            || trimmed_stem == "."
            || trimmed_stem == ".."
            || trimmed_stem.contains(['/', '\\', '\0'])
        {
            return Err(ExportError::Options(format!(
                "Invalid sequence base name '{stem}'."
            )));
        }

        let directory = if target_dir.as_os_str().is_empty() {
            PathBuf::from(".")
        } else {
            target_dir.to_path_buf()
        };

        let created_dirs = ensure_directory_chain_tracked(&directory)?;
        let (stage_dir, stage_name) = match allocate_staging_directory(&directory) {
            Ok(pair) => pair,
            Err(err) => {
                remove_empty_dirs_reverse(&created_dirs);
                return Err(ExportError::Io(err));
            }
        };
        let files_dir = stage_dir.join("files");
        let rollback_dir = stage_dir.join("rollback");
        if let Err(err) = fs::create_dir(&files_dir).and_then(|()| fs::create_dir(&rollback_dir)) {
            let _ = fs::remove_dir_all(&stage_dir);
            remove_empty_dirs_reverse(&created_dirs);
            return Err(ExportError::Io(err));
        }

        let case_insensitive =
            probe_case_insensitive_with_stage_dir(&directory, &stage_dir, &stage_name);

        Ok(Self {
            directory,
            stem: trimmed_stem.to_owned(),
            stage_dir,
            files_dir,
            rollback_dir,
            created_dirs,
            case_insensitive,
            max_bytes,
            max_files,
            staged_bytes: 0,
            staged_folded_names: BTreeSet::new(),
            files: Vec::new(),
            committed: false,
            cleanup_on_drop: true,
        })
    }

    /// Whether the target directory's underlying filesystem treats ASCII case variants as the same entry.
    pub fn is_case_insensitive(&self) -> bool {
        self.case_insensitive
    }

    /// Override detected filesystem case-sensitivity (used in deterministic tests).
    pub fn set_case_insensitive_for_test(&mut self, case_insensitive: bool) {
        self.case_insensitive = case_insensitive;
    }

    /// Output directory where committed sequence files will reside.
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// Sequence stem key in `.neoflash-sequences.json`.
    pub fn stem(&self) -> &str {
        &self.stem
    }

    /// Preflight every planned output filename before rendering or writing final destinations.
    ///
    /// Refuses if any planned filename is invalid, duplicated under the filesystem's case policy,
    /// points at a directory or symlink, or already exists on disk without belonging (unmodified)
    /// to the prior manifest entry for `self.stem`. Also verifies that any stale tail files from
    /// the prior manifest entry were not replaced by directories/symlinks or modified outside NeoFlash.
    pub fn preflight(&self, planned_filenames: &[String]) -> Result<(), ExportError> {
        self.verify_planned_set(planned_filenames)?;
        Ok(())
    }

    fn verify_planned_set(
        &self,
        planned_filenames: &[String],
    ) -> Result<VerifiedPriorState, ExportError> {
        if planned_filenames.is_empty() {
            return Err(ExportError::Options(
                "Sequence export produced zero output filenames.".into(),
            ));
        }
        if planned_filenames.len() > self.max_files {
            return Err(ExportError::Options(format!(
                "Sequence export exceeds the {} file staging limit.",
                self.max_files
            )));
        }

        let mut seen_folded = BTreeSet::new();
        for name in planned_filenames {
            validate_sequence_member_name(name)?;
            let key = fold_filename_key(name, self.case_insensitive);
            if !seen_folded.insert(key) {
                return Err(ExportError::Options(format!(
                    "Duplicate output filename '{name}' in sequence export."
                )));
            }
        }

        let (manifest, manifest_existed) = read_sequence_manifest(&self.directory)?;
        let prior_match = manifest
            .sequences
            .iter()
            .find(|(k, entry)| {
                names_match(k, &self.stem, self.case_insensitive)
                    || names_match(&entry.stem, &self.stem, self.case_insensitive)
            })
            .map(|(k, entry)| (k.clone(), entry.clone()));

        let (prior_key, prior_entry) = match prior_match {
            Some((k, e)) => (Some(k), Some(e)),
            None => (None, None),
        };

        let mut verified_owned_by_key: BTreeMap<String, (PathBuf, SequenceOwnedFileRecord)> =
            BTreeMap::new();
        let mut present_owned_names = Vec::new();

        if let Some(entry) = &prior_entry {
            for owned in &entry.files {
                validate_sequence_member_name(&owned.name)?;
                let owned_path = self.directory.join(&owned.name);
                match fs::symlink_metadata(&owned_path) {
                    Ok(meta) => {
                        if meta.file_type().is_symlink() {
                            return Err(ExportError::Destination(format!(
                                "'{}' is a symbolic link",
                                owned_path.display()
                            )));
                        }
                        if meta.file_type().is_dir() {
                            return Err(ExportError::Destination(format!(
                                "'{}' is a directory",
                                owned_path.display()
                            )));
                        }
                        let actual_sha = sha256_file_hex(&owned_path)?;
                        if meta.len() != owned.bytes || actual_sha != owned.sha256 {
                            return Err(ExportError::Destination(format!(
                                "'{}' was modified outside NeoFlash since the last sequence export",
                                owned_path.display()
                            )));
                        }
                        present_owned_names.push(owned.name.clone());
                        verified_owned_by_key.insert(
                            fold_filename_key(&owned.name, self.case_insensitive),
                            (owned_path, owned.clone()),
                        );
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                    Err(err) => return Err(ExportError::Io(err)),
                }
            }
        }

        for name in planned_filenames {
            let target_path = self.directory.join(name);
            let folded = fold_filename_key(name, self.case_insensitive);
            match fs::symlink_metadata(&target_path) {
                Ok(meta) => {
                    if meta.file_type().is_symlink() {
                        return Err(ExportError::Destination(format!(
                            "'{}' is a symbolic link",
                            target_path.display()
                        )));
                    }
                    if meta.file_type().is_dir() {
                        return Err(ExportError::Destination(format!(
                            "'{}' is a directory",
                            target_path.display()
                        )));
                    }
                    let Some((owned_path, _)) = verified_owned_by_key.get(&folded) else {
                        return Err(ExportError::Destination(target_path.display().to_string()));
                    };
                    if !paths_refer_to_same_entry(&target_path, owned_path) {
                        return Err(ExportError::Destination(target_path.display().to_string()));
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    // When `self.case_insensitive` was forced true in a test on a case-sensitive
                    // filesystem, also scan directory entries so simulated case-insensitive
                    // collisions (`FRAME_0002.PNG` vs `frame_0002.png`) are caught identically.
                    if self.case_insensitive
                        && let Some(colliding_path) = find_case_folded_unowned_entry(
                            &self.directory,
                            &folded,
                            &verified_owned_by_key,
                        )?
                    {
                        return Err(ExportError::Destination(
                            colliding_path.display().to_string(),
                        ));
                    }
                }
                Err(err) => return Err(ExportError::Io(err)),
            }
        }

        Ok(VerifiedPriorState {
            manifest,
            manifest_existed,
            prior_key,
            present_owned_names,
        })
    }

    /// Stage one generated sequence member inside the private sibling staging directory.
    pub fn stage_file(&mut self, file_name: &str, bytes: &[u8]) -> Result<PathBuf, ExportError> {
        validate_sequence_member_name(file_name)?;
        let folded = fold_filename_key(file_name, self.case_insensitive);
        if self.staged_folded_names.contains(&folded) {
            return Err(ExportError::Options(format!(
                "Duplicate output filename '{file_name}' in sequence export."
            )));
        }

        if self.files.len() >= self.max_files {
            return Err(ExportError::Options(format!(
                "Sequence export exceeds the {} file staging limit.",
                self.max_files
            )));
        }
        let next_total = self
            .staged_bytes
            .checked_add(bytes.len())
            .ok_or_else(|| ExportError::Options("Sequence staging byte size overflow.".into()))?;
        if next_total > self.max_bytes {
            return Err(ExportError::Options(format!(
                "Sequence export requires {next_total} staged bytes, exceeding the {} MiB transactional staging limit.",
                self.max_bytes / (1024 * 1024)
            )));
        }

        let index = self.files.len();
        let staged_path = self.files_dir.join(format!("part-{index:08}.staged"));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged_path)?;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;

        let sha256 = sha256_hex(bytes);
        let final_path = self.directory.join(file_name);
        self.staged_bytes = next_total;
        self.staged_folded_names.insert(folded);
        self.files.push(StagedSequenceFile {
            file_name: file_name.to_owned(),
            staged_path,
            final_path: final_path.clone(),
            bytes: bytes.len() as u64,
            sha256,
        });
        Ok(final_path)
    }

    /// Atomically swap the staged sequence set and updated `.neoflash-sequences.json` into place,
    /// removing stale tail frames from any longer prior export of the same sequence stem.
    pub fn commit(self) -> Result<Vec<PathBuf>, ExportError> {
        self.commit_with_fault_hook(|_| Ok(()))
    }

    /// Commit with a per-step fault hook (`step_index` in `0..=staged_files.len()`), allowing
    /// deterministic integration tests of mid-swap failure and byte-identical rollback.
    pub fn commit_with_fault_hook<F>(
        mut self,
        mut fault_hook: F,
    ) -> Result<Vec<PathBuf>, ExportError>
    where
        F: FnMut(usize) -> std::io::Result<()>,
    {
        if self.files.is_empty() {
            return Err(ExportError::Options(
                "Cannot commit an empty sequence transaction.".into(),
            ));
        }

        let planned_names: Vec<String> = self.files.iter().map(|f| f.file_name.clone()).collect();
        let mut prior = self.verify_planned_set(&planned_names)?;

        if let Some(old_key) = &prior.prior_key {
            prior.manifest.sequences.remove(old_key);
        }
        let new_records: Vec<SequenceOwnedFileRecord> = self
            .files
            .iter()
            .map(|f| SequenceOwnedFileRecord {
                name: f.file_name.clone(),
                bytes: f.bytes,
                sha256: f.sha256.clone(),
            })
            .collect();
        prior.manifest.sequences.insert(
            self.stem.clone(),
            SequenceManifestEntry {
                stem: self.stem.clone(),
                files: new_records,
            },
        );

        let manifest_bytes = serde_json::to_vec_pretty(&prior.manifest)?;
        let staged_manifest_path = self.files_dir.join("manifest.staged.json");
        {
            let mut mf = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&staged_manifest_path)?;
            mf.write_all(&manifest_bytes)?;
            mf.flush()?;
            mf.sync_all()?;
        }

        let final_manifest_path = self.directory.join(SEQUENCE_MANIFEST_FILENAME);
        let mut moved_to_rollback: Vec<(PathBuf, PathBuf)> =
            Vec::with_capacity(prior.present_owned_names.len() + 1);

        // Phase 1: Move prior manifest-owned files (overlapping + stale tail frames) and prior
        // manifest into `rollback/` so target paths are clear even for case-only renames.
        for (idx, old_name) in prior.present_owned_names.iter().enumerate() {
            let orig_path = self.directory.join(old_name);
            let bak_path = self.rollback_dir.join(format!("owned-{idx:08}.bak"));
            match fs::rename(&orig_path, &bak_path) {
                Ok(()) => moved_to_rollback.push((orig_path, bak_path)),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => {
                    return Err(self.rollback_and_fail(&[], &moved_to_rollback, err));
                }
            }
        }

        if prior.manifest_existed {
            let manifest_bak = self.rollback_dir.join("manifest.bak");
            match fs::rename(&final_manifest_path, &manifest_bak) {
                Ok(()) => moved_to_rollback.push((final_manifest_path.clone(), manifest_bak)),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => {
                    return Err(self.rollback_and_fail(&[], &moved_to_rollback, err));
                }
            }
        }

        // Phase 2: Move all newly staged files + updated manifest into `self.directory`.
        let mut placed_targets: Vec<PathBuf> = Vec::with_capacity(self.files.len() + 1);
        for (idx, staged) in self.files.iter().enumerate() {
            if let Err(err) = fault_hook(idx) {
                return Err(self.rollback_and_fail(&placed_targets, &moved_to_rollback, err));
            }
            if let Ok(unexpected_meta) = fs::symlink_metadata(&staged.final_path) {
                let kind = if unexpected_meta.file_type().is_dir() {
                    std::io::ErrorKind::IsADirectory
                } else {
                    std::io::ErrorKind::AlreadyExists
                };
                let err = std::io::Error::new(
                    kind,
                    format!(
                        "unowned destination '{}' appeared during sequence commit",
                        staged.final_path.display()
                    ),
                );
                return Err(self.rollback_and_fail(&placed_targets, &moved_to_rollback, err));
            }
            if let Err(err) = fs::rename(&staged.staged_path, &staged.final_path) {
                return Err(self.rollback_and_fail(&placed_targets, &moved_to_rollback, err));
            }
            placed_targets.push(staged.final_path.clone());
        }

        if let Err(err) = fault_hook(self.files.len()) {
            return Err(self.rollback_and_fail(&placed_targets, &moved_to_rollback, err));
        }
        if let Err(err) = fs::rename(&staged_manifest_path, &final_manifest_path) {
            return Err(self.rollback_and_fail(&placed_targets, &moved_to_rollback, err));
        }

        sync_directory_best_effort(&self.directory);
        self.committed = true;
        Ok(self.files.iter().map(|f| f.final_path.clone()).collect())
    }

    fn rollback_and_fail(
        &mut self,
        placed_targets: &[PathBuf],
        moved_to_rollback: &[(PathBuf, PathBuf)],
        cause: std::io::Error,
    ) -> ExportError {
        let mut restore_failed = false;
        for placed in placed_targets.iter().rev() {
            if let Err(err) = fs::remove_file(placed)
                && err.kind() != std::io::ErrorKind::NotFound
            {
                restore_failed = true;
            }
        }
        for (orig_path, bak_path) in moved_to_rollback.iter().rev() {
            if let Err(err) = fs::rename(bak_path, orig_path)
                && err.kind() != std::io::ErrorKind::NotFound
            {
                restore_failed = true;
            }
        }
        if restore_failed {
            self.cleanup_on_drop = false;
            return ExportError::Options(format!(
                "Sequence commit failed ({cause}); rollback was incomplete and backups remain in '{}'.",
                self.rollback_dir.display()
            ));
        }
        ExportError::Io(cause)
    }
}

impl Drop for SequenceSetTransaction {
    fn drop(&mut self) {
        if self.cleanup_on_drop {
            let _ = fs::remove_dir_all(&self.stage_dir);
        }
        if !self.committed {
            remove_empty_dirs_reverse(&self.created_dirs);
        }
    }
}

/// Read the hidden `.neoflash-sequences.json` manifest in `directory`, if present.
pub fn read_sequence_manifest(
    directory: &Path,
) -> Result<(SequenceDirectoryManifest, bool), ExportError> {
    let manifest_path = directory.join(SEQUENCE_MANIFEST_FILENAME);
    let meta = match fs::symlink_metadata(&manifest_path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok((SequenceDirectoryManifest::default(), false));
        }
        Err(err) => return Err(ExportError::Io(err)),
    };
    if meta.file_type().is_symlink() {
        return Err(ExportError::Destination(format!(
            "'{}' is a symbolic link",
            manifest_path.display()
        )));
    }
    if meta.file_type().is_dir() {
        return Err(ExportError::Destination(format!(
            "'{}' is a directory",
            manifest_path.display()
        )));
    }
    if meta.len() > MAX_MANIFEST_FILE_BYTES {
        return Err(ExportError::Destination(format!(
            "'{}' exceeds maximum manifest size",
            manifest_path.display()
        )));
    }
    let text = fs::read_to_string(&manifest_path)?;
    let parsed: SequenceDirectoryManifest = serde_json::from_str(&text).map_err(|_| {
        ExportError::Destination(format!(
            "'{}' is not a valid NeoFlash sequence manifest",
            manifest_path.display()
        ))
    })?;
    if parsed.generator != "NeoFlash" || parsed.format_version != 1 {
        return Err(ExportError::Destination(format!(
            "'{}' has unsupported manifest metadata",
            manifest_path.display()
        )));
    }
    Ok((parsed, true))
}

fn validate_sequence_member_name(name: &str) -> Result<(), ExportError> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', '\0'])
        || name.eq_ignore_ascii_case(SEQUENCE_MANIFEST_FILENAME)
        || name.to_ascii_lowercase().starts_with(SEQUENCE_STAGE_PREFIX)
    {
        return Err(ExportError::Options(format!(
            "Invalid sequence output filename '{name}'."
        )));
    }
    Ok(())
}

fn fold_filename_key(name: &str, case_insensitive: bool) -> String {
    if case_insensitive {
        name.to_lowercase()
    } else {
        name.to_owned()
    }
}

fn names_match(a: &str, b: &str, case_insensitive: bool) -> bool {
    if case_insensitive {
        a.to_lowercase() == b.to_lowercase()
    } else {
        a == b
    }
}

fn sha256_file_hex(path: &Path) -> Result<String, ExportError> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = hasher.finalize();
    let mut out = String::with_capacity(64);
    for b in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{b:02x}");
    }
    Ok(out)
}

pub(crate) fn ensure_directory_chain_tracked(dir: &Path) -> Result<Vec<PathBuf>, ExportError> {
    if dir.as_os_str().is_empty() || dir == Path::new(".") {
        return Ok(Vec::new());
    }
    if let Ok(meta) = fs::symlink_metadata(dir) {
        if meta.is_dir() {
            return Ok(Vec::new());
        }
        // Allow an existing symlink that resolves to an existing directory, but reject regular files.
        if fs::metadata(dir).is_ok_and(|m| m.is_dir()) {
            return Ok(Vec::new());
        }
        return Err(ExportError::Destination(format!(
            "'{}' exists and is not a directory",
            dir.display()
        )));
    }

    let mut missing = Vec::new();
    let mut cur = Some(dir);
    while let Some(p) = cur {
        if p.as_os_str().is_empty() || p == Path::new(".") || p.exists() {
            break;
        }
        missing.push(p.to_path_buf());
        cur = p.parent();
    }

    let mut created = Vec::with_capacity(missing.len());
    for p in missing.into_iter().rev() {
        match fs::create_dir(&p) {
            Ok(()) => created.push(p),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists && p.is_dir() => {}
            Err(err) => {
                remove_empty_dirs_reverse(&created);
                return Err(ExportError::Io(err));
            }
        }
    }
    Ok(created)
}

pub(crate) fn remove_empty_dirs_reverse(created_dirs: &[PathBuf]) {
    for dir in created_dirs.iter().rev() {
        let _ = fs::remove_dir(dir);
    }
}

fn allocate_staging_directory(parent: &Path) -> std::io::Result<(PathBuf, String)> {
    let pid = std::process::id();
    for _ in 0..256 {
        let seq = NEXT_SEQUENCE_STAGE_ID.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let name = format!("{SEQUENCE_STAGE_PREFIX}{pid}-{nanos}-{seq}");
        let candidate = parent.join(&name);
        match create_private_dir(&candidate) {
            Ok(()) => return Ok((candidate, name)),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not allocate unique sequence staging directory",
    ))
}

#[cfg(unix)]
fn create_private_dir(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt as _;
    fs::DirBuilder::new().mode(0o700).create(path)
}

#[cfg(not(unix))]
fn create_private_dir(path: &Path) -> std::io::Result<()> {
    fs::create_dir(path)
}

fn probe_case_insensitive_with_stage_dir(
    parent: &Path,
    stage_dir: &Path,
    stage_name: &str,
) -> bool {
    let upper_name = stage_name.to_ascii_uppercase();
    if upper_name == stage_name {
        return false;
    }
    let upper_path = parent.join(upper_name);
    paths_refer_to_same_entry(stage_dir, &upper_path)
}

#[cfg(unix)]
fn paths_refer_to_same_entry(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    let (Ok(ma), Ok(mb)) = (fs::symlink_metadata(a), fs::symlink_metadata(b)) else {
        return false;
    };
    ma.dev() == mb.dev() && ma.ino() == mb.ino()
}

#[cfg(not(unix))]
fn paths_refer_to_same_entry(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => fs::symlink_metadata(a).is_ok() && fs::symlink_metadata(b).is_ok() && a == b,
    }
}

fn find_case_folded_unowned_entry(
    directory: &Path,
    folded_target: &str,
    verified_owned_by_key: &BTreeMap<String, (PathBuf, SequenceOwnedFileRecord)>,
) -> Result<Option<PathBuf>, ExportError> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name_os = entry.file_name();
        let Some(name_str) = name_os.to_str() else {
            continue;
        };
        if name_str.starts_with(SEQUENCE_STAGE_PREFIX) || name_str == SEQUENCE_MANIFEST_FILENAME {
            continue;
        }
        if name_str.to_lowercase() == folded_target
            && !verified_owned_by_key.contains_key(folded_target)
        {
            return Ok(Some(entry.path()));
        }
    }
    Ok(None)
}

#[cfg(unix)]
fn sync_directory_best_effort(dir: &Path) {
    if let Ok(f) = fs::File::open(dir) {
        let _ = f.sync_all();
    }
}

#[cfg(not(unix))]
fn sync_directory_best_effort(_dir: &Path) {}
