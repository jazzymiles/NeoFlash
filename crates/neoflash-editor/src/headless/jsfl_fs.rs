//! Host filesystem adapter and CLI execution helpers for sandboxed JSFL scripts.
use super::{
    CliError, OutputFormat, load_project_valid, resolve_symbol, save_project_atomic,
    serialize_json_value, take_flag_value, write_json_error,
};
use neoflash_core::{
    FlFileCapabilities, JsflAuthoringState, JsflDiagnostic, JsflFileSystem, JsflResult,
    JsflSandbox, JsflValue, Project,
};
use serde_json::{Value, json};
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

/// Capability-bounded host filesystem adapter for headless JSFL `FLfile` calls.
///
/// `neoflash_core::JsflSandbox` always enforces [`FlFileCapabilities`] before touching the
/// [`JsflFileSystem`]. This host adapter provides defense-in-depth by canonicalizing paths
/// against explicit read/write roots and preventing symlink or `..` escapes.
#[derive(Debug)]
pub(crate) struct HostJsflFileSystem {
    read_roots: Vec<PathBuf>,
    write_roots: Vec<PathBuf>,
    allow_delete: bool,
}

impl HostJsflFileSystem {
    pub(crate) fn new(
        read_roots: Vec<PathBuf>,
        write_roots: Vec<PathBuf>,
        allow_delete: bool,
    ) -> Self {
        Self {
            read_roots,
            write_roots,
            allow_delete,
        }
    }

    fn canonicalize_anchored(path: &Path) -> Result<PathBuf, String> {
        for comp in path.components() {
            if matches!(comp, Component::ParentDir) {
                return Err(format!(
                    "Path traversal ('..') is forbidden in JSFL path '{}'.",
                    path.display()
                ));
            }
        }
        if path.exists() {
            return fs::canonicalize(path)
                .map_err(|e| format!("Failed to canonicalize '{}': {e}", path.display()));
        }
        let abs = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|e| format!("Failed to determine working directory: {e}"))?
                .join(path)
        };
        let mut cursor = abs.as_path();
        let mut tail = Vec::new();
        while !cursor.exists() {
            if let Some(name) = cursor.file_name() {
                tail.push(name.to_os_string());
            }
            let Some(parent) = cursor.parent() else {
                break;
            };
            cursor = parent;
        }
        let mut resolved = fs::canonicalize(cursor).map_err(|e| {
            format!(
                "Failed to canonicalize ancestor '{}': {e}",
                cursor.display()
            )
        })?;
        for part in tail.into_iter().rev() {
            resolved.push(part);
        }
        Ok(resolved)
    }

    fn check_in_roots(path: &Path, roots: &[PathBuf], op: &str) -> Result<PathBuf, String> {
        if roots.is_empty() {
            return Err(format!("JSFL {op} denied: capability is not enabled."));
        }
        let canon = Self::canonicalize_anchored(path)?;
        if roots.iter().any(|root| canon.starts_with(root)) {
            Ok(canon)
        } else {
            Err(format!(
                "JSFL {op} denied for '{}': outside allowed root.",
                path.display()
            ))
        }
    }

    fn all_roots(&self) -> Vec<PathBuf> {
        let mut all = self.read_roots.clone();
        all.extend(self.write_roots.iter().cloned());
        all
    }
}

impl JsflFileSystem for HostJsflFileSystem {
    fn read_file(&self, path: &Path) -> Result<String, String> {
        let target = Self::check_in_roots(path, &self.read_roots, "read")?;
        fs::read_to_string(&target)
            .map_err(|e| format!("Failed to read '{}': {e}", target.display()))
    }

    fn write_file(&self, path: &Path, content: &str) -> Result<(), String> {
        let target = Self::check_in_roots(path, &self.write_roots, "write")?;
        if let Some(parent) = target.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create parent dir '{}': {e}", parent.display()))?;
        }
        fs::write(&target, content)
            .map_err(|e| format!("Failed to write '{}': {e}", target.display()))
    }

    fn append_file(&self, path: &Path, content: &str) -> Result<(), String> {
        let target = Self::check_in_roots(path, &self.write_roots, "append")?;
        if let Some(parent) = target.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create parent dir '{}': {e}", parent.display()))?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&target)
            .map_err(|e| format!("Failed to open '{}' for append: {e}", target.display()))?;
        file.write_all(content.as_bytes())
            .map_err(|e| format!("Failed to append to '{}': {e}", target.display()))
    }

    fn create_folder(&self, path: &Path) -> Result<(), String> {
        // `JsflSandbox::with_flfile_capabilities` calls `create_folder(allowed_root)` even when
        // only read access was granted; treat an already-existing read root as a no-op.
        if let Ok(canon) = Self::canonicalize_anchored(path)
            && self.read_roots.iter().any(|root| root == &canon)
            && canon.is_dir()
        {
            return Ok(());
        }
        let target = Self::check_in_roots(path, &self.write_roots, "createFolder")?;
        fs::create_dir_all(&target)
            .map_err(|e| format!("Failed to create folder '{}': {e}", target.display()))
    }

    fn remove(&self, path: &Path) -> Result<bool, String> {
        if !self.allow_delete {
            return Err("JSFL remove denied: delete capability is not enabled.".to_owned());
        }
        let target = Self::check_in_roots(path, &self.write_roots, "remove")?;
        if !target.exists() {
            return Ok(false);
        }
        if target.is_dir() {
            fs::remove_dir_all(&target)
                .map_err(|e| format!("Failed to remove dir '{}': {e}", target.display()))?;
        } else {
            fs::remove_file(&target)
                .map_err(|e| format!("Failed to remove file '{}': {e}", target.display()))?;
        }
        Ok(true)
    }

    fn list_folder(&self, path: &Path, files_only: bool) -> Result<Vec<String>, String> {
        let target = Self::check_in_roots(path, &self.read_roots, "listFolder")?;
        let mut entries = Vec::new();
        for entry in fs::read_dir(&target)
            .map_err(|e| format!("Failed to read folder '{}': {e}", target.display()))?
        {
            let entry = entry.map_err(|e| e.to_string())?;
            let meta = entry.metadata().map_err(|e| e.to_string())?;
            if files_only && meta.is_dir() {
                continue;
            }
            if let Some(name) = entry.file_name().to_str() {
                entries.push(name.to_owned());
            }
        }
        entries.sort();
        Ok(entries)
    }

    fn exists(&self, path: &Path) -> bool {
        let roots = self.all_roots();
        Self::check_in_roots(path, &roots, "exists").is_ok_and(|p| p.exists())
    }

    fn is_folder(&self, path: &Path) -> bool {
        let roots = self.all_roots();
        Self::check_in_roots(path, &roots, "isFolder").is_ok_and(|p| p.is_dir())
    }

    fn canonicalize(&self, path: &Path) -> Result<PathBuf, String> {
        Self::canonicalize_anchored(path)
    }
}

fn common_path_ancestor(a: &Path, b: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for (ca, cb) in a.components().zip(b.components()) {
        if ca == cb {
            out.push(ca.as_os_str());
        } else {
            break;
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

pub(crate) struct JsflExecRequest<'a> {
    pub(crate) script_name: &'a str,
    pub(crate) source: &'a str,
    pub(crate) symbol_selector: Option<&'a str>,
    pub(crate) frame: Option<u32>,
    pub(crate) allow_read: Option<&'a Path>,
    pub(crate) allow_write: Option<&'a Path>,
    pub(crate) allow_delete: bool,
}

/// Execute JSFL against a project inside [`JsflSandbox`], keeping `FLfile` deny-all unless
/// `--allow-read` / `--allow-write` explicitly grant a sandboxed host root.
pub(crate) fn execute_jsfl_on_project(
    project: &Project,
    req: JsflExecRequest<'_>,
) -> Result<JsflResult, CliError> {
    if req.allow_delete && req.allow_write.is_none() {
        return Err(CliError::usage(
            "invalid_jsfl_capabilities",
            "--allow-delete requires --allow-write <dir>",
        ));
    }

    let mut state = JsflAuthoringState::for_project(project);
    if let Some(sel) = req.symbol_selector {
        let sym = resolve_symbol(project, sel)?;
        state.active_symbol = sym.id.clone();
    }
    if let Some(frame) = req.frame {
        let sym_id = state.active_symbol.clone();
        let total = project
            .symbol(&sym_id)
            .map_or(1, |s| s.timeline.frame_count.max(1));
        if frame >= total {
            return Err(CliError::runtime(
                "frame_out_of_range",
                format!("Frame {frame} is outside symbol '{sym_id}' range 0..{total}"),
            ));
        }
        state.current_frame = frame;
    }

    let mut sandbox = JsflSandbox::new();
    if req.allow_read.is_some() || req.allow_write.is_some() {
        let mut read_roots = Vec::new();
        let mut write_roots = Vec::new();
        if let Some(read_dir) = req.allow_read {
            let canon = HostJsflFileSystem::canonicalize_anchored(read_dir).map_err(|e| {
                CliError::usage(
                    "invalid_jsfl_root",
                    format!("Invalid --allow-read dir: {e}"),
                )
            })?;
            if !canon.is_dir() {
                return Err(CliError::usage(
                    "invalid_jsfl_root",
                    format!(
                        "--allow-read path '{}' is not an existing directory",
                        read_dir.display()
                    ),
                ));
            }
            read_roots.push(canon);
        }
        if let Some(write_dir) = req.allow_write {
            fs::create_dir_all(write_dir).map_err(|e| {
                CliError::runtime(
                    "invalid_jsfl_root",
                    format!(
                        "Failed to create --allow-write dir '{}': {e}",
                        write_dir.display()
                    ),
                )
            })?;
            let canon = HostJsflFileSystem::canonicalize_anchored(write_dir).map_err(|e| {
                CliError::runtime(
                    "invalid_jsfl_root",
                    format!("Invalid --allow-write dir: {e}"),
                )
            })?;
            write_roots.push(canon);
        }

        let allowed_root = match (read_roots.first(), write_roots.first()) {
            (Some(r), Some(w)) if r == w => Some(r.clone()),
            (Some(r), Some(w)) => common_path_ancestor(r, w),
            (Some(r), None) => Some(r.clone()),
            (None, Some(w)) => Some(w.clone()),
            (None, None) => None,
        };
        let host_fs = Arc::new(HostJsflFileSystem::new(
            read_roots,
            write_roots,
            req.allow_delete,
        ));
        sandbox = sandbox
            .with_file_system(host_fs)
            .with_flfile_capabilities(FlFileCapabilities {
                allowed_root,
                allow_read: req.allow_read.is_some(),
                allow_write: req.allow_write.is_some(),
                allow_delete: req.allow_delete,
            });
    }

    sandbox
        .execute(project, &state, req.script_name, req.source)
        .map_err(jsfl_diag_to_cli_error)
}

fn jsfl_diag_to_cli_error(diag: JsflDiagnostic) -> CliError {
    CliError::runtime_with_details(
        "jsfl_error",
        format!(
            "JSFL error in {} ({}:{}): {}",
            diag.file, diag.line, diag.column, diag.message
        ),
        json!({
            "file": diag.file,
            "line": diag.line,
            "column": diag.column,
            "message": diag.message,
        }),
    )
}

pub(crate) fn cmd_jsfl<R: Read, W1: Write, W2: Write>(
    args: &[String],
    format: OutputFormat,
    pretty: bool,
    stdin: &mut R,
    stdout: &mut W1,
    stderr: &mut W2,
) -> i32 {
    match run_cmd_jsfl(args, format, pretty, stdin, stdout) {
        Ok(()) => 0,
        Err(err) => {
            write_json_error(&err, format, pretty, stderr);
            err.exit_code
        }
    }
}

fn run_cmd_jsfl<R: Read, W1: Write>(
    args: &[String],
    format: OutputFormat,
    pretty: bool,
    stdin: &mut R,
    stdout: &mut W1,
) -> Result<(), CliError> {
    let mut project_path: Option<PathBuf> = None;
    let mut script_path: Option<PathBuf> = None;
    let mut eval_code: Option<String> = None;
    let mut use_stdin = false;
    let mut out_path: Option<PathBuf> = None;
    let mut in_place = false;
    let mut symbol_selector: Option<String> = None;
    let mut frame: Option<u32> = None;
    let mut allow_read: Option<PathBuf> = None;
    let mut allow_write: Option<PathBuf> = None;
    let mut allow_delete = false;

    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--project" => {
                project_path = Some(PathBuf::from(take_flag_value(args, &mut i, "--project")?));
            }
            "--script" => {
                script_path = Some(PathBuf::from(take_flag_value(args, &mut i, "--script")?));
            }
            "--eval" => {
                eval_code = Some(take_flag_value(args, &mut i, "--eval")?);
            }
            "--stdin" => use_stdin = true,
            "--out" | "--output" => {
                out_path = Some(PathBuf::from(take_flag_value(args, &mut i, "--out")?));
            }
            "--in-place" => in_place = true,
            "--symbol" => {
                symbol_selector = Some(take_flag_value(args, &mut i, "--symbol")?);
            }
            "--frame" => {
                let raw = take_flag_value(args, &mut i, "--frame")?;
                frame = Some(raw.parse::<u32>().map_err(|_| {
                    CliError::usage("invalid_argument", format!("Invalid --frame value '{raw}'"))
                })?);
            }
            "--allow-read" => {
                allow_read = Some(PathBuf::from(take_flag_value(
                    args,
                    &mut i,
                    "--allow-read",
                )?));
            }
            "--allow-write" => {
                allow_write = Some(PathBuf::from(take_flag_value(
                    args,
                    &mut i,
                    "--allow-write",
                )?));
            }
            "--allow-delete" => allow_delete = true,
            other => {
                return Err(CliError::usage(
                    "unknown_flag",
                    format!("Unknown flag '{other}' for 'jsfl'"),
                ));
            }
        }
        i += 1;
    }

    let project_path = project_path.ok_or_else(|| {
        CliError::usage("missing_flag", "'jsfl' requires --project <file.nflash>")
    })?;
    let source_count = usize::from(script_path.is_some())
        + usize::from(eval_code.is_some())
        + usize::from(use_stdin);
    if source_count != 1 {
        return Err(CliError::usage(
            "invalid_arguments",
            "'jsfl' requires exactly one of --script <file>, --eval <code>, or --stdin",
        ));
    }
    if out_path.is_some() && in_place {
        return Err(CliError::usage(
            "conflicting_flags",
            "--out and --in-place are mutually exclusive",
        ));
    }

    let (script_name, source) = if let Some(ref sp) = script_path {
        let name = sp
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("script.jsfl")
            .to_owned();
        let text = fs::read_to_string(sp).map_err(|e| {
            CliError::runtime(
                "io_error",
                format!("Failed to read JSFL script '{}': {e}", sp.display()),
            )
        })?;
        (name, text)
    } else if let Some(code) = eval_code {
        ("eval.jsfl".to_owned(), code)
    } else {
        let mut buf = String::new();
        stdin
            .read_to_string(&mut buf)
            .map_err(|e| CliError::runtime("io_error", format!("Failed reading stdin: {e}")))?;
        ("stdin.jsfl".to_owned(), buf)
    };

    let project = load_project_valid(&project_path)?;
    let mut result = execute_jsfl_on_project(
        &project,
        JsflExecRequest {
            script_name: &script_name,
            source: &source,
            symbol_selector: symbol_selector.as_deref(),
            frame,
            allow_read: allow_read.as_deref(),
            allow_write: allow_write.as_deref(),
            allow_delete,
        },
    )?;

    result.project.upgrade_format_for_features();
    result.project.validate().map_err(|e| {
        CliError::runtime(
            "validation_failed",
            format!("Resulting JSFL project failed validation: {e}"),
        )
    })?;

    let mut saved_path: Option<String> = None;
    if let Some(dest) = out_path {
        save_project_atomic(&mut result.project, &dest)?;
        saved_path = Some(dest.display().to_string());
    } else if in_place && (result.mutations > 0 || result.project != project) {
        save_project_atomic(&mut result.project, &project_path)?;
        saved_path = Some(project_path.display().to_string());
    }

    match format {
        OutputFormat::Json => {
            let val = json!({
                "ok": true,
                "command": "jsfl",
                "output_lines": result.output_lines,
                "mutations": result.mutations,
                "steps_executed": result.steps_executed,
                "return_value": jsfl_value_to_json(&result.return_value),
                "saved_path": saved_path,
            });
            let text = serialize_json_value(&val, pretty)?;
            writeln!(stdout, "{text}").map_err(io_write_err)?;
        }
        OutputFormat::Text => {
            for line in &result.output_lines {
                writeln!(stdout, "{line}").map_err(io_write_err)?;
            }
            writeln!(
                stdout,
                "JSFL ok: {} step(s), {} mutation(s){}",
                result.steps_executed,
                result.mutations,
                saved_path
                    .as_deref()
                    .map_or_else(String::new, |p| format!(", saved to {p}"))
            )
            .map_err(io_write_err)?;
        }
    }
    Ok(())
}

pub(crate) fn jsfl_value_to_json(val: &JsflValue) -> Value {
    match val {
        JsflValue::Undefined | JsflValue::Null => Value::Null,
        JsflValue::Bool(b) => Value::Bool(*b),
        JsflValue::Number(n) => serde_json::Number::from_f64(*n)
            .map_or_else(|| Value::String(n.to_string()), Value::Number),
        JsflValue::String(s) => Value::String(s.clone()),
        JsflValue::Array(items) => Value::Array(items.iter().map(jsfl_value_to_json).collect()),
        JsflValue::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), jsfl_value_to_json(v)))
                .collect(),
        ),
        other => serde_json::to_value(other).unwrap_or(Value::Null),
    }
}

fn io_write_err(err: io::Error) -> CliError {
    CliError::runtime("io_error", format!("Failed writing stdout: {err}"))
}
