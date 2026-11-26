//! Headless scriptable CLI for AI agents, automation scripts, and CI pipelines (`T-302`).
//!
//! Dispatched from `main.rs` before initializing `eframe` so all project creation, inspection,
//! validation, compilation, playback evaluation, sandboxed JSFL scripting, strict JSON batching,
//! import, and export workflows run with zero windowing/display dependency. Batch and standalone
//! JSFL filesystem effects share a command-level rollback boundary with validated project output.
mod batch;
mod effects;
mod import_export;
mod jsfl_fs;
#[cfg(test)]
mod tests;

use crate::build_info::BuildIdentity;
use crate::panels::compiler_errors::{LocatedScriptDiagnostic, collect_located_script_diagnostics};
use neoflash_core::{
    AuthoredDraw, AuthoredPlayback, Color, Easing, Element, ElementKind, FrameScript, Layer,
    LayerRole, MAX_NFLASH_BUNDLE_BYTES, MAX_PROJECT_JSON_BYTES, NFLASH_BUNDLE_MAGIC, ObjectEvent,
    PathCommand, Project, ResourceUnit, Script, ScriptLanguage, Symbol, SymbolKind, TweenKind,
    flatten_scenes_cow, has_nflash_bundle_prefix, layer_effectively_visible,
    sample_layer_in_at_fps, scene_ranges, validate as validate_project_model,
};
use neoflash_export::{embedded_font_families, load_profiles_from_project};
use neoflash_script::DiagnosticSeverity;
use serde_json::{Value, json};
use std::borrow::Cow;
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OutputFormat {
    Json,
    Text,
}

#[derive(Clone, Debug)]
pub(crate) struct CliError {
    pub(crate) exit_code: i32,
    pub(crate) code: &'static str,
    pub(crate) message: String,
    pub(crate) details: Option<Value>,
}

impl CliError {
    pub(crate) fn usage(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            exit_code: 2,
            code,
            message: message.into(),
            details: None,
        }
    }

    pub(crate) fn runtime(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            exit_code: 1,
            code,
            message: message.into(),
            details: None,
        }
    }

    pub(crate) fn runtime_with_details(
        code: &'static str,
        message: impl Into<String>,
        details: Value,
    ) -> Self {
        Self {
            exit_code: 1,
            code,
            message: message.into(),
            details: Some(details),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct InspectOptions {
    pub(crate) symbol: Option<String>,
    pub(crate) frame: Option<u32>,
    pub(crate) no_elements: bool,
    pub(crate) no_scripts: bool,
    pub(crate) no_resources: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum EvalVmMode {
    #[default]
    Preview,
    Swf,
}

#[derive(Clone, Debug)]
pub(crate) struct EvalOptions {
    pub(crate) scene: Option<String>,
    pub(crate) symbol: Option<String>,
    pub(crate) start_frame: u32,
    pub(crate) end_frame: u32,
    pub(crate) scripts: bool,
    pub(crate) strict: bool,
    pub(crate) vm: EvalVmMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CliDetection {
    pub(crate) active: bool,
    pub(crate) explicit_headless: bool,
}

/// Inspect process environment + arguments and execute headless CLI if triggered, returning
/// `Some(exit_code)` when headless mode handled the invocation or `None` when GUI mode should run.
pub(crate) fn maybe_run_from_env() -> Option<i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let headless_env = std::env::var_os("NEOFLASH_HEADLESS");
    let detection = detect_headless_mode(&args, headless_env.as_deref());
    if !detection.active {
        return None;
    }
    let mut stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    let exit_code = run_cli_with_io(
        &args,
        detection.explicit_headless,
        &mut stdin,
        &mut stdout,
        &mut stderr,
    );
    let _ = stdout.flush();
    let _ = stderr.flush();
    Some(exit_code)
}

/// Detect whether CLI mode should handle the launch while ignoring macOS Finder `-psn_*` args.
pub(crate) fn detect_headless_mode(args: &[String], headless_env: Option<&OsStr>) -> CliDetection {
    let env_headless = headless_env.is_some_and(|v| {
        let s = v.to_string_lossy();
        let trimmed = s.trim();
        !trimmed.is_empty()
            && trimmed != "0"
            && !trimmed.eq_ignore_ascii_case("false")
            && !trimmed.eq_ignore_ascii_case("no")
    });

    let mut has_headless_arg = false;
    let mut has_help_or_version = false;
    let mut saw_positional = false;
    for arg in args {
        if arg.starts_with("-psn_") {
            continue;
        }
        match arg.as_str() {
            "--headless" | "--headless=1" | "--headless=true" => has_headless_arg = true,
            "headless" if !saw_positional => has_headless_arg = true,
            "--help" | "-h" | "--version" | "-V" => has_help_or_version = true,
            other => {
                if !other.starts_with('-') {
                    saw_positional = true;
                }
            }
        }
    }

    let explicit_headless = env_headless || has_headless_arg;
    CliDetection {
        active: explicit_headless || has_help_or_version,
        explicit_headless,
    }
}

fn detect_error_output_prefs(
    raw_args: &[String],
    default_format: OutputFormat,
) -> (OutputFormat, bool) {
    let mut format = default_format;
    let mut explicit_format = false;
    let mut pretty = false;
    let mut saw_subcmd = false;
    let mut saw_headless = false;
    let mut i = 0usize;
    while i < raw_args.len() {
        let arg = raw_args[i].as_str();
        if arg.starts_with("-psn_") {
            i += 1;
            continue;
        }
        match arg {
            "--headless" | "--headless=1" | "--headless=true" => {
                if !explicit_format {
                    format = OutputFormat::Json;
                }
                saw_headless = true;
            }
            "headless" if !saw_subcmd && !saw_headless => {
                if !explicit_format {
                    format = OutputFormat::Json;
                }
                saw_headless = true;
            }
            "--pretty" => pretty = true,
            "--format" => {
                if let Some(next) = raw_args.get(i + 1) {
                    if next.eq_ignore_ascii_case("json") {
                        format = OutputFormat::Json;
                        explicit_format = true;
                    } else if next.eq_ignore_ascii_case("text") {
                        format = OutputFormat::Text;
                        explicit_format = true;
                    }
                    i += 1;
                }
            }
            _ if arg.starts_with("--format=") => {
                let val = &arg["--format=".len()..];
                if val.eq_ignore_ascii_case("json") {
                    format = OutputFormat::Json;
                    explicit_format = true;
                } else if val.eq_ignore_ascii_case("text") {
                    format = OutputFormat::Text;
                    explicit_format = true;
                }
            }
            _ => {
                if !arg.starts_with('-') {
                    saw_subcmd = true;
                }
            }
        }
        i += 1;
    }
    (format, pretty)
}

/// Execute headless CLI against explicit I/O streams (used by both binary entry and unit tests).
pub(crate) fn run_cli_with_io<R: Read, W1: Write, W2: Write>(
    raw_args: &[String],
    explicit_headless: bool,
    stdin: &mut R,
    stdout: &mut W1,
    stderr: &mut W2,
) -> i32 {
    let default_format = if explicit_headless {
        OutputFormat::Json
    } else {
        OutputFormat::Text
    };
    let (fallback_format, fallback_pretty) = detect_error_output_prefs(raw_args, default_format);
    let parsed = match parse_global_args(raw_args, default_format) {
        Ok(p) => p,
        Err(err) => {
            write_json_error(&err, fallback_format, fallback_pretty, stdout, stderr);
            return err.exit_code;
        }
    };

    if parsed.wants_help {
        return emit_help(
            parsed.subcommand.as_deref(),
            parsed.format,
            parsed.pretty,
            stdout,
            stderr,
        );
    }
    if parsed.wants_version && parsed.subcommand.is_none() {
        return emit_version(parsed.format, parsed.pretty, stdout, stderr);
    }

    let Some(subcmd) = parsed.subcommand.as_deref() else {
        return emit_help(None, parsed.format, parsed.pretty, stdout, stderr);
    };

    match subcmd {
        "capabilities" | "schema" => {
            if !parsed.rest.is_empty() {
                let err = CliError::usage(
                    "unknown_flag",
                    format!("Unknown argument '{}' for '{subcmd}'", parsed.rest[0]),
                );
                write_json_error(&err, parsed.format, parsed.pretty, stdout, stderr);
                return err.exit_code;
            }
            cmd_capabilities(parsed.format, parsed.pretty, stdout, stderr)
        }
        "help" => {
            if parsed.rest.len() > 1 {
                let err = CliError::usage(
                    "unknown_flag",
                    format!("Unexpected argument '{}' for 'help'", parsed.rest[1]),
                );
                write_json_error(&err, parsed.format, parsed.pretty, stdout, stderr);
                return err.exit_code;
            }
            let topic = parsed.rest.first().map(String::as_str);
            emit_help(topic, parsed.format, parsed.pretty, stdout, stderr)
        }
        "version" => {
            if !parsed.rest.is_empty() {
                let err = CliError::usage(
                    "unknown_flag",
                    format!("Unknown argument '{}' for 'version'", parsed.rest[0]),
                );
                write_json_error(&err, parsed.format, parsed.pretty, stdout, stderr);
                return err.exit_code;
            }
            emit_version(parsed.format, parsed.pretty, stdout, stderr)
        }
        "new" => cmd_new(&parsed.rest, parsed.format, parsed.pretty, stdout, stderr),
        "inspect" => cmd_inspect(&parsed.rest, parsed.format, parsed.pretty, stdout, stderr),
        "validate" => cmd_validate(&parsed.rest, parsed.format, parsed.pretty, stdout, stderr),
        "compile" => cmd_compile(&parsed.rest, parsed.format, parsed.pretty, stdout, stderr),
        "eval" => cmd_eval(&parsed.rest, parsed.format, parsed.pretty, stdout, stderr),
        "jsfl" => jsfl_fs::cmd_jsfl(
            &parsed.rest,
            parsed.format,
            parsed.pretty,
            stdin,
            stdout,
            stderr,
        ),
        "batch" => batch::cmd_batch(
            &parsed.rest,
            parsed.format,
            parsed.pretty,
            stdin,
            stdout,
            stderr,
        ),
        "import" => {
            import_export::cmd_import(&parsed.rest, parsed.format, parsed.pretty, stdout, stderr)
        }
        "export" => {
            import_export::cmd_export(&parsed.rest, parsed.format, parsed.pretty, stdout, stderr)
        }
        other => {
            let err = CliError::usage(
                "unknown_subcommand",
                format!(
                    "Unknown headless subcommand '{other}'. Run with --help or 'capabilities' for available commands."
                ),
            );
            write_json_error(&err, parsed.format, parsed.pretty, stdout, stderr);
            err.exit_code
        }
    }
}

struct ParsedGlobalArgs {
    format: OutputFormat,
    pretty: bool,
    wants_help: bool,
    wants_version: bool,
    subcommand: Option<String>,
    rest: Vec<String>,
}

fn parse_global_args(
    raw_args: &[String],
    default_format: OutputFormat,
) -> Result<ParsedGlobalArgs, CliError> {
    let mut format = default_format;
    let mut explicit_format = false;
    let mut pretty = false;
    let mut wants_help = false;
    let mut wants_version = false;
    let mut subcommand: Option<String> = None;
    let mut rest: Vec<String> = Vec::new();
    let mut saw_headless_token = false;

    let mut i = 0usize;
    while i < raw_args.len() {
        let arg = &raw_args[i];
        if arg.starts_with("-psn_") {
            i += 1;
            continue;
        }
        match arg.as_str() {
            "--headless" | "--headless=1" | "--headless=true" => {
                if !explicit_format {
                    format = OutputFormat::Json;
                }
                saw_headless_token = true;
            }
            "headless" if subcommand.is_none() && !saw_headless_token => {
                if !explicit_format {
                    format = OutputFormat::Json;
                }
                saw_headless_token = true;
            }
            "--pretty" => {
                pretty = true;
                if subcommand.as_deref() == Some("export") {
                    rest.push(arg.clone());
                }
            }
            "--help" | "-h" => wants_help = true,
            "--version" | "-V" if subcommand.is_none() => wants_version = true,
            "--format" => {
                let raw = take_flag_value(raw_args, &mut i, "--format")?;
                format = match raw.trim().to_ascii_lowercase().as_str() {
                    "json" => OutputFormat::Json,
                    "text" => OutputFormat::Text,
                    other => {
                        return Err(CliError::usage(
                            "invalid_argument",
                            format!("Unsupported --format '{other}' (expected json|text)"),
                        ));
                    }
                };
                explicit_format = true;
            }
            other if other.starts_with("--format=") => {
                let raw = &other["--format=".len()..];
                format = match raw.trim().to_ascii_lowercase().as_str() {
                    "json" => OutputFormat::Json,
                    "text" => OutputFormat::Text,
                    bad => {
                        return Err(CliError::usage(
                            "invalid_argument",
                            format!("Unsupported --format '{bad}' (expected json|text)"),
                        ));
                    }
                };
                explicit_format = true;
            }
            _ => {
                if subcommand.is_none() && !arg.starts_with('-') {
                    subcommand = Some(arg.clone());
                } else if subcommand.is_none() && arg.starts_with('-') {
                    return Err(CliError::usage(
                        "unknown_flag",
                        format!("Unknown global flag '{arg}'"),
                    ));
                } else {
                    rest.push(arg.clone());
                }
            }
        }
        i += 1;
    }

    Ok(ParsedGlobalArgs {
        format,
        pretty,
        wants_help,
        wants_version,
        subcommand,
        rest,
    })
}

pub(crate) fn take_flag_value(
    args: &[String],
    index: &mut usize,
    flag: &str,
) -> Result<String, CliError> {
    if *index + 1 >= args.len() {
        return Err(CliError::usage(
            "missing_value",
            format!("{flag} requires a value"),
        ));
    }
    *index += 1;
    Ok(args[*index].clone())
}

pub(crate) fn serialize_json_value(value: &Value, pretty: bool) -> Result<String, CliError> {
    if pretty {
        serde_json::to_string_pretty(value)
    } else {
        serde_json::to_string(value)
    }
    .map_err(|e| {
        CliError::runtime(
            "serialization_error",
            format!("JSON serialization failed: {e}"),
        )
    })
}

/// Emit a structured CLI error.
///
/// Contract (`T-321`): When `format == OutputFormat::Json`, `stdout` ALWAYS receives a single valid
/// JSON object (`ok: false`, `exit_code`, `error: { code, message, details? }`) and no plain-text
/// output is written to `stdout`. When `format == OutputFormat::Text`, `stderr` receives a concise
/// human-readable error line.
pub(crate) fn write_json_error<W1: Write, W2: Write>(
    err: &CliError,
    format: OutputFormat,
    pretty: bool,
    stdout: &mut W1,
    stderr: &mut W2,
) {
    match format {
        OutputFormat::Json => {
            let mut err_obj = serde_json::Map::new();
            err_obj.insert("code".into(), Value::String(err.code.to_owned()));
            err_obj.insert("message".into(), Value::String(err.message.clone()));
            if let Some(ref details) = err.details {
                err_obj.insert("details".into(), details.clone());
            }
            let payload = json!({
                "ok": false,
                "exit_code": err.exit_code,
                "error": Value::Object(err_obj),
            });
            let line = serialize_json_value(&payload, pretty).unwrap_or_else(|_| {
                format!(
                    "{{\"ok\":false,\"exit_code\":{},\"error\":{{\"code\":{},\"message\":{}}}}}",
                    err.exit_code,
                    serde_json::to_string(err.code).unwrap_or_else(|_| "\"error\"".into()),
                    serde_json::to_string(&err.message).unwrap_or_else(|_| "\"error\"".into()),
                )
            });
            let _ = writeln!(stdout, "{line}");
        }
        OutputFormat::Text => {
            let _ = writeln!(stderr, "Error [{}]: {}", err.code, err.message);
        }
    }
}

/// Load a `.nflash` file into a `Project`, tolerating validation issues so `inspect`/`validate`
/// can report every `ValidationIssue` cleanly.
pub(crate) fn load_project_lenient(path: &Path) -> Result<Project, CliError> {
    let (prefix, prefix_len) = peek_file_prefix_8(path, "project")?;
    if &prefix[..prefix_len] == NFLASH_BUNDLE_MAGIC {
        let bytes = read_bytes_file_limited(path, MAX_NFLASH_BUNDLE_BYTES, "project bundle")?;
        return Project::from_nflash_bytes(&bytes).map_err(|e| {
            CliError::runtime(
                "invalid_bundle",
                format!("Invalid project bundle '{}': {e}", path.display()),
            )
        });
    }
    if has_nflash_bundle_prefix(&prefix[..prefix_len]) {
        return Err(CliError::runtime(
            "invalid_bundle",
            format!(
                "Truncated or unsupported NFLASHB1 bundle header in '{}'",
                path.display()
            ),
        ));
    }
    let raw = read_utf8_file_limited(path, MAX_PROJECT_JSON_BYTES, "project")?;
    match Project::from_json(&raw) {
        Ok(project) => Ok(project),
        Err(neoflash_core::ProjectError::Invalid(_)) => serde_json::from_str::<Project>(&raw)
            .map_err(|e| {
                CliError::runtime(
                    "invalid_json",
                    format!("Invalid project JSON '{}': {e}", path.display()),
                )
            }),
        Err(neoflash_core::ProjectError::Json(e)) => Err(CliError::runtime(
            "invalid_json",
            format!("Invalid project JSON '{}': {e}", path.display()),
        )),
    }
}

/// Load and strictly validate a `.nflash` project before mutating, compiling, evaluating, or exporting.
pub(crate) fn load_project_valid(path: &Path) -> Result<Project, CliError> {
    let (prefix, prefix_len) = peek_file_prefix_8(path, "project")?;
    if &prefix[..prefix_len] == NFLASH_BUNDLE_MAGIC {
        let bytes = read_bytes_file_limited(path, MAX_NFLASH_BUNDLE_BYTES, "project bundle")?;
        return Project::from_nflash_bytes(&bytes).map_err(|e| {
            CliError::runtime(
                "invalid_project",
                format!("Failed loading '{}': {e}", path.display()),
            )
        });
    }
    if has_nflash_bundle_prefix(&prefix[..prefix_len]) {
        return Err(CliError::runtime(
            "invalid_project",
            format!(
                "Failed loading '{}': truncated or unsupported NFLASHB1 bundle header",
                path.display()
            ),
        ));
    }
    let raw = read_utf8_file_limited(path, MAX_PROJECT_JSON_BYTES, "project")?;
    Project::from_json(&raw).map_err(|e| {
        CliError::runtime(
            "invalid_project",
            format!("Failed loading '{}': {e}", path.display()),
        )
    })
}

fn peek_file_prefix_8(path: &Path, label: &str) -> Result<([u8; 8], usize), CliError> {
    let mut file = fs::File::open(path).map_err(|e| {
        CliError::runtime(
            "io_error",
            format!("Failed to read {label} '{}': {e}", path.display()),
        )
    })?;
    let mut buf = [0u8; 8];
    let mut read = 0usize;
    while read < buf.len() {
        match file.read(&mut buf[read..]) {
            Ok(0) => break,
            Ok(n) => read += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => {
                return Err(CliError::runtime(
                    "io_error",
                    format!("Failed to read {label} '{}': {e}", path.display()),
                ));
            }
        }
    }
    Ok((buf, read))
}

fn read_bytes_file_limited(
    path: &Path,
    max_bytes: usize,
    label: &str,
) -> Result<Vec<u8>, CliError> {
    let file = fs::File::open(path).map_err(|e| {
        CliError::runtime(
            "io_error",
            format!("Failed to read {label} '{}': {e}", path.display()),
        )
    })?;
    let declared_len = file.metadata().map_err(|e| {
        CliError::runtime(
            "io_error",
            format!("Failed to inspect {label} '{}': {e}", path.display()),
        )
    })?;
    if declared_len.len() > max_bytes as u64 {
        return Err(CliError::runtime(
            "resource_limit_exceeded",
            format!(
                "{} '{}' exceeds the {} MiB encoded input limit",
                label.to_ascii_uppercase(),
                path.display(),
                max_bytes / (1024 * 1024)
            ),
        ));
    }
    let mut bytes = Vec::with_capacity(
        usize::try_from(declared_len.len())
            .unwrap_or(max_bytes)
            .min(max_bytes),
    );
    file.take(max_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| {
            CliError::runtime(
                "io_error",
                format!("Failed to read {label} '{}': {e}", path.display()),
            )
        })?;
    if bytes.len() > max_bytes {
        return Err(CliError::runtime(
            "resource_limit_exceeded",
            format!(
                "{} '{}' exceeds the {} MiB encoded input limit",
                label.to_ascii_uppercase(),
                path.display(),
                max_bytes / (1024 * 1024)
            ),
        ));
    }
    Ok(bytes)
}

/// Read a UTF-8 CLI input without allocating beyond its established encoded-size boundary.
/// Metadata rejects ordinary oversized files before reading, while `take(max + 1)` also handles
/// files that grow between the metadata check and the read.
fn read_utf8_file_limited(path: &Path, max_bytes: usize, label: &str) -> Result<String, CliError> {
    let file = fs::File::open(path).map_err(|e| {
        CliError::runtime(
            "io_error",
            format!("Failed to read {label} '{}': {e}", path.display()),
        )
    })?;
    let declared_len = file.metadata().map_err(|e| {
        CliError::runtime(
            "io_error",
            format!("Failed to inspect {label} '{}': {e}", path.display()),
        )
    })?;
    if declared_len.len() > max_bytes as u64 {
        return Err(CliError::runtime(
            "resource_limit_exceeded",
            format!(
                "{} '{}' exceeds the {} MiB encoded input limit",
                label.to_ascii_uppercase(),
                path.display(),
                max_bytes / (1024 * 1024)
            ),
        ));
    }

    let mut raw = String::with_capacity(
        usize::try_from(declared_len.len())
            .unwrap_or(max_bytes)
            .min(max_bytes),
    );
    file.take(max_bytes as u64 + 1)
        .read_to_string(&mut raw)
        .map_err(|e| {
            CliError::runtime(
                "io_error",
                format!("Failed to read {label} '{}': {e}", path.display()),
            )
        })?;
    if raw.len() > max_bytes {
        return Err(CliError::runtime(
            "resource_limit_exceeded",
            format!(
                "{} '{}' exceeds the {} MiB encoded input limit",
                label.to_ascii_uppercase(),
                path.display(),
                max_bytes / (1024 * 1024)
            ),
        ));
    }
    Ok(raw)
}

/// Upgrade document `format_version` across all v2..=v13 feature gates enforced by `validation.rs`.
pub(crate) fn upgrade_project_format_versions(project: &mut Project) {
    if project.has_program() {
        project.format_version = project.format_version.max(2);
    }
    let has_bitmap_el = project
        .symbols
        .iter()
        .flat_map(|s| &s.timeline.layers)
        .flat_map(|l| &l.keyframes)
        .flat_map(|k| &k.elements)
        .any(|e| matches!(e.kind, ElementKind::Bitmap { .. }));
    if !project.resources.is_empty() || has_bitmap_el {
        project.format_version = project.format_version.max(3);
    }
    if !project.library_folders.is_empty() || project.symbols.iter().any(|s| !s.folder.is_empty()) {
        project.format_version = project.format_version.max(4);
    }
    let uses_v5 = project.symbols.iter().any(|s| {
        s.timeline.layers.iter().any(|l| {
            l.properties.end_frame.is_some()
                || l.keyframes.iter().any(|k| {
                    matches!(k.tween, Some(Easing::Flash { .. }))
                        || k.property_eases
                            .iter()
                            .any(|(_, e)| matches!(e, Easing::Flash { .. }))
                        || k.elements.iter().any(|el| {
                            el.transform.skew != 0.0
                                || !el.transform.color_transform.is_identity()
                                || matches!(el.kind, ElementKind::PaintedPath { .. })
                        })
                })
        })
    });
    if uses_v5 {
        project.format_version = project.format_version.max(5);
    }
    let has_timeline_video = project
        .symbols
        .iter()
        .flat_map(|s| &s.timeline.layers)
        .flat_map(|l| &l.keyframes)
        .any(|k| {
            k.video.is_some()
                || k.elements
                    .iter()
                    .any(|e| matches!(e.kind, ElementKind::Video { .. }))
        });
    if has_timeline_video {
        project.format_version = project.format_version.max(9);
    }
    project.upgrade_format_for_features();
}

/// Upgrade document format version, validate invariants, serialize `.nflash` (UTF-8 JSON or
/// `NFLASHB1` bundle), and save atomically.
pub(crate) fn save_project_atomic(project: &mut Project, path: &Path) -> Result<(), CliError> {
    upgrade_project_format_versions(project);
    let bytes = project
        .to_nflash_bytes()
        .map_err(|e| CliError::runtime("validation_failed", e.to_string()))?;
    crate::windows_platform::atomic_replace_file(path, &bytes).map_err(|e| {
        CliError::runtime(
            "io_error",
            format!("Failed saving project '{}': {e}", path.display()),
        )
    })
}

pub(crate) fn parse_color(raw: &str) -> Result<Color, CliError> {
    let s = raw.trim();
    if let Some(hex) = s.strip_prefix('#') {
        match hex.len() {
            3 => {
                let r = u8::from_str_radix(&hex[0..1], 16).map_err(bad_color(raw))? * 17;
                let g = u8::from_str_radix(&hex[1..2], 16).map_err(bad_color(raw))? * 17;
                let b = u8::from_str_radix(&hex[2..3], 16).map_err(bad_color(raw))? * 17;
                return Ok([r, g, b, 255]);
            }
            4 => {
                let r = u8::from_str_radix(&hex[0..1], 16).map_err(bad_color(raw))? * 17;
                let g = u8::from_str_radix(&hex[1..2], 16).map_err(bad_color(raw))? * 17;
                let b = u8::from_str_radix(&hex[2..3], 16).map_err(bad_color(raw))? * 17;
                let a = u8::from_str_radix(&hex[3..4], 16).map_err(bad_color(raw))? * 17;
                return Ok([r, g, b, a]);
            }
            6 => {
                let r = u8::from_str_radix(&hex[0..2], 16).map_err(bad_color(raw))?;
                let g = u8::from_str_radix(&hex[2..4], 16).map_err(bad_color(raw))?;
                let b = u8::from_str_radix(&hex[4..6], 16).map_err(bad_color(raw))?;
                return Ok([r, g, b, 255]);
            }
            8 => {
                let r = u8::from_str_radix(&hex[0..2], 16).map_err(bad_color(raw))?;
                let g = u8::from_str_radix(&hex[2..4], 16).map_err(bad_color(raw))?;
                let b = u8::from_str_radix(&hex[4..6], 16).map_err(bad_color(raw))?;
                let a = u8::from_str_radix(&hex[6..8], 16).map_err(bad_color(raw))?;
                return Ok([r, g, b, a]);
            }
            _ => {}
        }
    }
    match s.to_ascii_lowercase().as_str() {
        "transparent" | "none" => Ok([0, 0, 0, 0]),
        "black" => Ok([0, 0, 0, 255]),
        "white" => Ok([255, 255, 255, 255]),
        "red" => Ok([255, 0, 0, 255]),
        "green" => Ok([0, 128, 0, 255]),
        "blue" => Ok([0, 0, 255, 255]),
        _ => Err(CliError::usage(
            "invalid_color",
            format!("Invalid color '{raw}'. Expected #RGB, #RRGGBB, #RRGGBBAA, or named color"),
        )),
    }
}

fn bad_color<'a>(raw: &'a str) -> impl FnOnce(std::num::ParseIntError) -> CliError + 'a {
    move |_| CliError::usage("invalid_color", format!("Invalid hex color '{raw}'"))
}

pub(crate) fn parse_language(raw: &str) -> Result<ScriptLanguage, CliError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "as1" | "actionscript1" | "actionscript_1" => Ok(ScriptLanguage::ActionScript1),
        "as2" | "actionscript2" | "actionscript_2" => Ok(ScriptLanguage::ActionScript2),
        "as3" | "actionscript3" | "actionscript_3" => Ok(ScriptLanguage::ActionScript3),
        "js" | "javascript" => Ok(ScriptLanguage::JavaScript),
        "ts" | "typescript" => Ok(ScriptLanguage::TypeScript),
        other => Err(CliError::usage(
            "invalid_language",
            format!("Unsupported script language '{other}' (expected as1|as2|as3|js|ts)"),
        )),
    }
}

pub(crate) fn parse_symbol_kind(raw: &str) -> Result<SymbolKind, CliError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "movie_clip" | "movieclip" | "mc" => Ok(SymbolKind::MovieClip),
        "graphic" => Ok(SymbolKind::Graphic),
        "button" | "btn" => Ok(SymbolKind::Button),
        other => Err(CliError::usage(
            "invalid_argument",
            format!("Unsupported symbol kind '{other}' (expected movie_clip|graphic|button)"),
        )),
    }
}

pub(crate) fn parse_object_event(raw: &str) -> Result<ObjectEvent, CliError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "load" => Ok(ObjectEvent::Load),
        "click" | "release" | "press" => Ok(ObjectEvent::Click),
        "enter_frame" | "enterframe" => Ok(ObjectEvent::EnterFrame),
        other => Err(CliError::usage(
            "invalid_argument",
            format!("Unsupported object event '{other}' (expected load|click|enter_frame)"),
        )),
    }
}

pub(crate) fn parse_easing(raw: &str) -> Result<Easing, CliError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "linear" => Ok(Easing::Linear),
        "ease_in" | "ease-in" | "easein" => Ok(Easing::EaseIn),
        "ease_out" | "ease-out" | "easeout" => Ok(Easing::EaseOut),
        "ease_in_out" | "ease-in-out" | "easeinout" => Ok(Easing::EaseInOut),
        other => Err(CliError::usage(
            "invalid_argument",
            format!("Unsupported easing '{other}' (expected linear|ease_in|ease_out|ease_in_out)"),
        )),
    }
}

pub(crate) fn parse_tween_kind(raw: &str) -> Result<TweenKind, CliError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "classic" => Ok(TweenKind::Classic),
        "shape" => Ok(TweenKind::Shape),
        "motion" => Ok(TweenKind::Motion),
        other => Err(CliError::usage(
            "invalid_argument",
            format!("Unsupported tween kind '{other}' (expected classic|shape|motion)"),
        )),
    }
}

pub(crate) fn parse_frame_range_str(raw: &str) -> Result<(u32, u32), CliError> {
    let s = raw.trim();
    if let Some((a, b)) = s.split_once("..=") {
        let start = parse_u32_range(a, s)?;
        let end = parse_u32_range(b, s)?;
        if start > end {
            return Err(CliError::usage(
                "invalid_frame_range",
                format!("Invalid frame range '{raw}': start > end"),
            ));
        }
        return Ok((start, end));
    }
    if let Some((a, b)) = s.split_once("..") {
        let start = parse_u32_range(a, s)?;
        let end_excl = parse_u32_range(b, s)?;
        if start >= end_excl {
            return Err(CliError::usage(
                "invalid_frame_range",
                format!("Invalid exclusive frame range '{raw}'"),
            ));
        }
        return Ok((start, end_excl - 1));
    }
    let single = parse_u32_range(s, s)?;
    Ok((single, single))
}

fn parse_u32_range(part: &str, full: &str) -> Result<u32, CliError> {
    part.trim().parse::<u32>().map_err(|_| {
        CliError::usage(
            "invalid_frame_range",
            format!("Invalid frame range '{full}' (expected N or start..=end)"),
        )
    })
}

pub(crate) fn resolve_symbol<'a>(
    project: &'a Project,
    selector: &str,
) -> Result<&'a Symbol, CliError> {
    if let Some(sym) = project.symbols.iter().find(|s| s.id == selector) {
        return Ok(sym);
    }
    let exact: Vec<&Symbol> = project
        .symbols
        .iter()
        .filter(|s| s.name == selector)
        .collect();
    if exact.len() == 1 {
        return Ok(exact[0]);
    }
    if exact.len() > 1 {
        return Err(CliError::runtime(
            "ambiguous_symbol",
            format!("Multiple symbols named '{selector}'; specify symbol ID"),
        ));
    }
    let ci: Vec<&Symbol> = project
        .symbols
        .iter()
        .filter(|s| s.name.eq_ignore_ascii_case(selector))
        .collect();
    if ci.len() == 1 {
        return Ok(ci[0]);
    }
    if ci.len() > 1 {
        return Err(CliError::runtime(
            "ambiguous_symbol",
            format!("Multiple symbols match '{selector}'; specify symbol ID"),
        ));
    }
    Err(CliError::runtime(
        "symbol_not_found",
        format!("Symbol '{selector}' not found in project"),
    ))
}

pub(crate) fn resolve_symbol_mut<'a>(
    project: &'a mut Project,
    id: &str,
) -> Result<&'a mut Symbol, CliError> {
    project
        .symbol_mut(id)
        .ok_or_else(|| CliError::runtime("symbol_not_found", format!("Symbol '{id}' not found")))
}

pub(crate) fn resolve_layer_index(
    timeline: &neoflash_core::Timeline,
    selector: &str,
) -> Result<usize, CliError> {
    if let Some(idx) = timeline.layers.iter().position(|l| l.id == selector) {
        return Ok(idx);
    }
    let exact: Vec<usize> = timeline
        .layers
        .iter()
        .enumerate()
        .filter_map(|(idx, l)| (l.name == selector).then_some(idx))
        .collect();
    if exact.len() == 1 {
        return Ok(exact[0]);
    }
    if exact.len() > 1 {
        return Err(CliError::runtime(
            "ambiguous_layer",
            format!("Multiple layers named '{selector}'; specify layer ID"),
        ));
    }
    let ci: Vec<usize> = timeline
        .layers
        .iter()
        .enumerate()
        .filter_map(|(idx, l)| l.name.eq_ignore_ascii_case(selector).then_some(idx))
        .collect();
    if ci.len() == 1 {
        return Ok(ci[0]);
    }
    Err(CliError::runtime(
        "layer_not_found",
        format!("Layer '{selector}' not found"),
    ))
}

pub(crate) fn resolve_layer_index_mut(
    timeline: &mut neoflash_core::Timeline,
    selector: &str,
) -> Result<usize, CliError> {
    resolve_layer_index(timeline, selector)
}

fn color_hex(c: Color) -> String {
    if c[3] == 255 {
        format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
    } else {
        format!("#{:02X}{:02X}{:02X}{:02X}", c[0], c[1], c[2], c[3])
    }
}

pub(crate) fn build_new_project(
    name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    fps: Option<u32>,
    bg: Option<Color>,
    language: Option<ScriptLanguage>,
    demo: bool,
) -> Result<Project, CliError> {
    let mut project = if demo {
        Project::demo()
    } else {
        Project::blank()
    };
    if let Some(n) = name {
        project.name = n;
    }
    if let Some(w) = width {
        if w == 0 {
            return Err(CliError::usage("invalid_argument", "--width must be > 0"));
        }
        project.width = w;
    }
    if let Some(h) = height {
        if h == 0 {
            return Err(CliError::usage("invalid_argument", "--height must be > 0"));
        }
        project.height = h;
    }
    if let Some(f) = fps {
        if f == 0 {
            return Err(CliError::usage("invalid_argument", "--fps must be > 0"));
        }
        project.fps = f;
    }
    if let Some(color) = bg {
        project.background = color;
    }
    if let Some(lang) = language
        && !demo
        && let Some(root) = project.symbol_mut(&project.root.clone())
        && root.timeline.scripts.is_empty()
    {
        root.timeline.scripts.push(FrameScript {
            layer: None,
            frame: 0,
            script: Script {
                language: lang,
                source: "// Frame 1\n".to_owned(),
            },
        });
    }
    upgrade_project_format_versions(&mut project);
    project
        .validate()
        .map_err(|e| CliError::runtime("validation_failed", e.to_string()))?;
    Ok(project)
}

fn emit_version<W1: Write, W2: Write>(
    format: OutputFormat,
    pretty: bool,
    stdout: &mut W1,
    stderr: &mut W2,
) -> i32 {
    let id = BuildIdentity::current();
    match format {
        OutputFormat::Json => {
            let val = json!({
                "ok": true,
                "command": "version",
                "name": "neoflash-editor",
                "version": id.version,
                "build_number": id.build_number,
                "commit": id.commit,
                "commit_short": id.commit_short(),
                "label": id.build_label(),
            });
            match serialize_json_value(&val, pretty) {
                Ok(text) => {
                    let _ = writeln!(stdout, "{text}");
                    0
                }
                Err(err) => {
                    write_json_error(&err, format, pretty, stdout, stderr);
                    err.exit_code
                }
            }
        }
        OutputFormat::Text => {
            let _ = writeln!(
                stdout,
                "neoflash-editor {} ({}, commit {})",
                id.version,
                id.build_label(),
                id.commit_short()
            );
            0
        }
    }
}

fn subcommand_help_text(topic: &str) -> Option<&'static str> {
    match topic {
        "capabilities" | "schema" => {
            Some("Usage: neoflash-editor --headless capabilities [--format json|text] [--pretty]")
        }
        "new" => Some(
            "Usage: neoflash-editor --headless new --output <file.nflash> [--name STR] [--width N] [--height N] [--fps F] [--bg #RRGGBB[AA]] [--script as3|as2|as1|ts|js] [--language as1|as2|as3|js|ts] [--demo]",
        ),
        "inspect" => Some(
            "Usage: neoflash-editor --headless inspect --project <file.nflash> [--symbol ID|NAME] [--frame N] [--no-elements] [--no-scripts] [--no-resources]",
        ),
        "validate" => {
            Some("Usage: neoflash-editor --headless validate --project <file.nflash> [--strict]")
        }
        "compile" => {
            Some("Usage: neoflash-editor --headless compile --project <file.nflash> [--strict]")
        }
        "eval" => Some(
            "Usage: neoflash-editor --headless eval --project <file.nflash> [--scene ID|NAME] [--symbol ID|NAME] [--frame N | --frames START..=END] [--scripts]",
        ),
        "jsfl" => Some(
            "Usage: neoflash-editor --headless jsfl --project <file.nflash> (--script FILE | --eval CODE | --stdin) [--out FILE | --in-place] [--symbol ID|NAME] [--frame N] [--flfile-root DIR] [--allow-read DIR] [--allow-write DIR] [--allow-delete]",
        ),
        "batch" => Some(
            "Usage: neoflash-editor --headless batch (--file OPS.json | --stdin) [--project IN.nflash] [--out OUT.nflash | --in-place]",
        ),
        "import" => Some(
            "Usage: neoflash-editor --headless import --input <path...> [--into <project.nflash>] (--out <project.nflash> | --in-place) [--library-only] [--symbol ID|NAME] [--layer ID|NAME] [--frame N] [--folder STR] [--swc-conflict prefer-newer|prefer-existing|error] [--sequence-placement single-layer|separate-layers|graphic|movieclip] [--frame-step N]",
        ),
        "export" => Some(
            "Usage: neoflash-editor --headless export --project <file.nflash> --target html5|swf|image|png-sequence|sequence|aep|fla|xfl|svg|lottie|dotlottie|atlas|video|audio|banner|desktop --out <path> [--profile NAME|ID] [--render-mode canvas|dom] [--swf-target fp8|fp9|fp10|fp10-scriptless] [--image-format png|jpeg|webp|gif|bmp|psd|pdf|ai|svg] [--frame N (image: 1-based; sequence/svg/lottie: 0-based)] [--scale F] [--symbol ID|NAME] ...",
        ),
        "help" => Some("Usage: neoflash-editor --headless help [<subcommand>]"),
        "version" => {
            Some("Usage: neoflash-editor --headless version [--format json|text] [--pretty]")
        }
        _ => None,
    }
}

fn global_help_text() -> &'static str {
    concat!(
        "NeoFlash Editor Headless CLI\n",
        "\n",
        "Triggers: --headless, headless, --help/-h, --version/-V, or NEOFLASH_HEADLESS=1\n",
        "Global options: --format <json|text>  --pretty\n",
        "Exit codes: 0 = success, 1 = validation/compile/JSFL/runtime/IO error, 2 = CLI usage/flag/schema error\n",
        "\n",
        "Subcommands:\n",
        "  capabilities | schema   Print machine-readable capabilities, build identity, and batch schema\n",
        "  new                     Create and validate a new .nflash project\n",
        "  inspect                 Inspect metadata, scenes, symbols, elements, scripts, resources, and fonts\n",
        "  validate                Validate document model invariants and compile project scripts\n",
        "  compile                 Compile/check project scripts with R-102 language policy and located diagnostics\n",
        "  eval                    Evaluate deterministic frame poses and optional native AuthoredPlayback script traces\n",
        "  jsfl                    Run capability-gated JSFL scripts cleanly against a project\n",
        "  batch                   Execute a strict JSON batch with transactional filesystem effects\n",
        "  import                  Import FLA/XFL/SWF/SWC/Figma/SVG/PSD/PDF/AI/EPS/FXG/image/audio/video/font/palette into .nflash\n",
        "  export                  Publish/export HTML5, SWF, Image, PNG Sequence, AEP, FLA, XFL, SVG, Lottie, Atlas, Video, Audio\n"
    )
}

fn emit_help<W1: Write, W2: Write>(
    topic: Option<&str>,
    format: OutputFormat,
    pretty: bool,
    stdout: &mut W1,
    stderr: &mut W2,
) -> i32 {
    let usage = match topic {
        Some(t) => match subcommand_help_text(t) {
            Some(text) => text,
            None => {
                let err = CliError::usage(
                    "unknown_subcommand",
                    format!(
                        "Unknown headless subcommand '{t}'. Run with --help or 'capabilities' for available commands."
                    ),
                );
                write_json_error(&err, format, pretty, stdout, stderr);
                return err.exit_code;
            }
        },
        None => global_help_text(),
    };
    match format {
        OutputFormat::Json => {
            let val = json!({
                "ok": true,
                "command": "help",
                "topic": topic,
                "usage": usage,
                "subcommands": subcommands_schema(),
            });
            match serialize_json_value(&val, pretty) {
                Ok(text) => {
                    let _ = writeln!(stdout, "{text}");
                    0
                }
                Err(err) => {
                    write_json_error(&err, format, pretty, stdout, stderr);
                    err.exit_code
                }
            }
        }
        OutputFormat::Text => {
            let _ = writeln!(stdout, "{usage}");
            0
        }
    }
}

fn subcommands_schema() -> Value {
    json!([
        {
            "name": "capabilities",
            "aliases": ["schema"],
            "summary": "Report build identity, supported subcommands, batch ops schema, JSFL security, import formats, export targets, and exit code contracts",
            "flags": ["--pretty", "--format <json|text>", "-h", "--help"]
        },
        {
            "name": "new",
            "summary": "Create, validate, and atomically write a new .nflash document",
            "flags": ["--output <file.nflash>", "--name <str>", "--width <u32>", "--height <u32>", "--fps <u32>", "--bg <#RRGGBB[AA]>", "--script <as3|as2|as1|ts|js>", "--language <as1|as2|as3|js|ts>", "--demo", "-h", "--help"]
        },
        {
            "name": "inspect",
            "summary": "Dump structured JSON inspection of document metadata, scenes, symbols, layers/keyframes/elements, scripts, resources, fonts, publish profiles, and validation issues",
            "flags": ["--project <file.nflash>", "--symbol <id|name>", "--frame <u32>", "--no-elements", "--no-scripts", "--no-resources", "-h", "--help"]
        },
        {
            "name": "validate",
            "summary": "Validate document model invariants and R-102 script compilation",
            "flags": ["--project <file.nflash>", "--strict", "-h", "--help"]
        },
        {
            "name": "compile",
            "summary": "Compile and check project scripts honoring R-102 language policy and returning located diagnostics",
            "flags": ["--project <file.nflash>", "--strict", "-h", "--help"]
        },
        {
            "name": "eval",
            "summary": "Evaluate frame display poses, transforms, and optional AuthoredPlayback script execution with trace() output",
            "flags": ["--project <file.nflash>", "--scene <id|name>", "--symbol <id|name>", "--frame <u32>", "--frames <start..=end>", "--scripts", "-h", "--help"]
        },
        {
            "name": "jsfl",
            "summary": "Run sandboxed JSFL scripts with deny-all FLfile defaults and one rollback boundary for project and permitted filesystem effects",
            "flags": ["--project <file.nflash>", "--script <file.jsfl> | --eval <code> | --stdin", "--out <out.nflash> | --in-place", "--symbol <id|name>", "--frame <u32>", "--flfile-root <dir>", "--allow-read <dir>", "--allow-write <dir>", "--allow-delete", "-h", "--help"]
        },
        {
            "name": "batch",
            "summary": "Execute a strict JSON sequence with one rollback boundary for saves, exports, permitted FLfile effects, and final project output",
            "flags": ["--file <ops.json> | --stdin", "--project <in.nflash>", "--out <out.nflash> | --in-place", "-h", "--help"]
        },
        {
            "name": "import",
            "summary": "Import document/interchange (.nflash, .fla, .xfl, .swf, .fig, .svg, .psd/.psb, .pdf/.ai/.eps, .fxg) or library/asset (.swc, images, sequences, audio, video, fonts, palettes) files standalone or into an existing .nflash",
            "flags": ["--input <path...>", "--into <project.nflash>", "--out <project.nflash> | --in-place", "--library-only", "--symbol <id|name>", "--layer <id|name>", "--frame <u32>", "--folder <str>", "--swc-conflict <prefer-newer|prefer-existing|error>", "--sequence-placement <single-layer|separate-layers|graphic|movieclip>", "--frame-step <u32>", "-h", "--help"]
        },
        {
            "name": "export",
            "summary": "Publish or export HTML5, SWF, Image (PNG/JPEG/WebP/GIF/BMP/PSD/PDF/AI), PNG/Image Sequence, After Effects (AEP), FLA, XFL, SVG, Lottie/dotLottie, Atlas, Video, Audio, Banner, or Desktop targets",
            "flags": ["--project <file.nflash>", "--target <html5|swf|image|png-sequence|sequence|aep|fla|xfl|svg|lottie|dotlottie|atlas|video|audio|banner|desktop>", "--out <path>", "--profile <name|id>", "--size-level <full|balanced|compact|shared>", "--runtime <dir>", "--runtime-url <url>", "--font-policy <device|embed|subset|outlines>", "--audio-policy <original|preserve|strip|disabled|wav16|mp3-64|mp3-96|mp3-128|mp3-160|mp3-192|mp3-256|mp3-320>", "--render-mode <canvas|dom>", "--clean", "--sprite-sheets", "--no-sprite-sheets", "--assets-folder", "--scale-mode <show-all|no-scale|exact-fit|no-border>", "--stage-align <str>", "--fullscreen", "--fla-profile <...>", "--swf-target <fp8|fp9|fp10|fp10-scriptless>", "--swf-profile <fp8|fp9|fp10>", "--scriptless", "--outline-text", "--symbol <id|name>", "--frame <u32>", "--end-frame <u32>", "--scale <f32>", "--width <u32>", "--height <u32>", "--transparent", "--jpeg-quality <1..=100>", "--text-as-outlines", "--pretty", "--atlas-format <...>", "--atlas-extrude <u32>", "--audio-format <wav|mp3|ogg|flac|aac>", "--banner-max-gzip <usize>", "--banner-max-zip <bytes>", "--image-format <original|png|jpeg|webp|gif|bmp|psd|pdf|ai|svg>", "--html-preview", "--no-aepx", "--no-jsx", "--no-translate-scripts", "--shape-morph-step <u32>", "-h", "--help"]
        }
    ])
}

fn batch_operations_schema() -> Value {
    let mut ops = serde_json::Map::new();
    ops.insert(
        "new".into(),
        json!([
            "name?",
            "width?",
            "height?",
            "fps?",
            "bg?",
            "language?",
            "script_language?",
            "also_compile_compatible_scripts?",
            "demo?"
        ]),
    );
    ops.insert("open".into(), json!(["path"]));
    ops.insert(
        "set_project".into(),
        json!([
            "name?",
            "width?",
            "height?",
            "fps?",
            "bg?",
            "script_language?",
            "also_compile_compatible_scripts?"
        ]),
    );
    ops.insert(
        "add_symbol".into(),
        json!([
            "id?",
            "name",
            "kind?",
            "frame_count?",
            "linkage_id?",
            "class_name?",
            "folder?",
            "as_scene?"
        ]),
    );
    ops.insert(
        "add_layer".into(),
        json!([
            "symbol?",
            "id?",
            "name",
            "role?",
            "visible?",
            "locked?",
            "mask_layer?",
            "motion_guide?"
        ]),
    );
    ops.insert(
        "add_keyframe".into(),
        json!([
            "symbol?",
            "layer",
            "frame",
            "label?",
            "tween?",
            "tween_kind?",
            "copy_previous?",
            "clear_elements?"
        ]),
    );
    ops.insert(
        "add_element".into(),
        json!([
            "symbol?",
            "layer?",
            "frame?",
            "id?",
            "name?",
            "kind (rectangle|oval|ellipse|text|text_box|path|instance)",
            "transform?",
            "x?",
            "y?",
            "scale_x?",
            "scale_y?",
            "rotation?",
            "skew?",
            "alpha?",
            "visible?",
            "width?",
            "height?",
            "fill?",
            "stroke?",
            "stroke_width?",
            "text?",
            "font_family?",
            "font_size?",
            "color?",
            "word_wrap?",
            "multiline?",
            "commands?",
            "d?",
            "fill_rule?",
            "symbol_id?",
            "playback?"
        ]),
    );
    ops.insert(
        "set_script".into(),
        json!([
            "symbol?",
            "layer?",
            "frame?",
            "element?",
            "event?",
            "path?",
            "id?",
            "language?",
            "source",
            "remove_empty?"
        ]),
    );
    ops.insert(
        "remove_element".into(),
        json!(["symbol?", "layer?", "frame?", "element"]),
    );
    ops.insert(
        "apply_command_steps".into(),
        json!(["symbol?", "layer?", "frame?", "elements", "steps"]),
    );
    ops.insert(
        "jsfl".into(),
        json!([
            "exactly one of code? | source? | script_file?",
            "symbol?",
            "frame?",
            "flfile_root?",
            "allow_read?",
            "allow_write?",
            "allow_delete?"
        ]),
    );
    ops.insert("validate".into(), json!(["strict?"]));
    ops.insert("compile".into(), json!(["strict?"]));
    ops.insert(
        "eval".into(),
        json!(["scene?", "symbol?", "frame?", "frames?", "scripts?"]),
    );
    ops.insert(
        "inspect".into(),
        json!([
            "symbol?",
            "frame?",
            "no_elements?",
            "no_scripts?",
            "no_resources?"
        ]),
    );
    ops.insert(
        "import".into(),
        json!([
            "exactly one of input? | inputs?",
            "library_only?",
            "symbol?",
            "layer?",
            "frame?",
            "folder?",
            "swc_conflict?",
            "sequence_placement?",
            "frame_step?"
        ]),
    );
    ops.insert(
        "export".into(),
        json!([
            "target",
            "out",
            "profile?",
            "size_level?",
            "runtime?",
            "runtime_url?",
            "font_policy?",
            "audio_policy?",
            "render_mode?",
            "clean?",
            "sprite_sheets?",
            "assets_folder?",
            "scale_mode?",
            "stage_align?",
            "allow_fullscreen?",
            "fla_profile?",
            "swf_target?",
            "swf_profile?",
            "scriptless?",
            "outline_text?",
            "symbol?",
            "frame?",
            "end_frame?",
            "scale?",
            "width?",
            "height?",
            "transparent?",
            "jpeg_quality?",
            "text_as_outlines?",
            "pretty?",
            "atlas_format?",
            "atlas_extrude?",
            "audio_format?",
            "banner_max_gzip?",
            "banner_max_zip?",
            "image_format?",
            "html_preview?",
            "include_aepx?",
            "include_extendscript?",
            "translate_scripts?",
            "shape_morph_step?"
        ]),
    );
    ops.insert("save".into(), json!(["path?"]));
    Value::Object(ops)
}

fn batch_schema_json() -> Value {
    json!({
        "containers": ["{ \"ops\": [ ... ] }", "{ \"steps\": [ ... ] }", "[ ... ]"],
        "strict": "Unknown wrapper, operation, nested transform, path-command, authoring-command, and export fields are rejected.",
        "atomicity": "Operations mutate an in-memory candidate project. Saves, exports, permitted JSFL FLfile mutations, and the implicit final --out / --in-place project output retain step-order read-your-writes behavior but share one rollback journal; any later operation, validation, save, or response-write failure restores their pre-command filesystem state.",
        "operations": batch_operations_schema(),
        "nested_schemas": {
            "transform": ["x?", "y?", "scale_x?", "scale_y?", "rotation?", "skew?", "alpha?"],
            "frame_range_object": ["start", "end"],
            "path_command_ops": ["move_to{x,y}", "line_to{x,y}", "quadratic_to{cx,cy,x,y}", "cubic_to{cx1,cy1,cx2,cy2,x,y}", "close"]
        },
        "command_step_ops": [
            "translate",
            "scale",
            "rotate",
            "skew",
            "set_alpha",
            "set_fill_color",
            "set_stroke_color",
            "set_stroke_width",
            "set_visible",
            "set_blend_mode",
            "rename_selection",
            "duplicate_selection",
            "delete_selection",
            "swap_symbol"
        ]
    })
}

fn cmd_capabilities<W1: Write, W2: Write>(
    format: OutputFormat,
    pretty: bool,
    stdout: &mut W1,
    stderr: &mut W2,
) -> i32 {
    let id = BuildIdentity::current();
    let batch_schema = batch_schema_json();
    let subcommands = subcommands_schema();
    let import_formats = json!([
        "nflash",
        "fla",
        "xfl",
        "swf",
        "swc",
        "fig",
        "figma",
        "figma.json",
        "svg",
        "psd",
        "psb",
        "pdf",
        "ai",
        "eps",
        "fxg",
        "png",
        "jpg",
        "jpeg",
        "gif",
        "webp",
        "bmp",
        "tif",
        "tiff",
        "image-sequence",
        "mp3",
        "wav",
        "ogg",
        "flac",
        "aac",
        "m4a",
        "mp4",
        "m4v",
        "webm",
        "mov",
        "ogv",
        "avi",
        "ttf",
        "otf",
        "ttc",
        "otc",
        "woff",
        "woff2",
        "act",
        "clr",
        "ase"
    ]);
    let export_targets = json!([
        "html5",
        "swf",
        "image",
        "png-sequence",
        "sequence",
        "aep",
        "fla",
        "xfl",
        "svg",
        "lottie",
        "dotlottie",
        "atlas",
        "video",
        "audio",
        "banner",
        "desktop"
    ]);
    let html5_profiles = json!([
        "Default",
        "Full",
        "Balanced",
        "Compact",
        "Shared",
        "Web App · DOM",
        "Web Banner · Native",
        "Criteo · HTML5 ZIP",
        "Desktop app",
        "High Quality",
        "Preview / Debug"
    ]);
    let payload = json!({
        "ok": true,
        "command": "capabilities",
        "build": {
            "version": id.version,
            "build_number": id.build_number,
            "commit": id.commit,
            "commit_short": id.commit_short(),
            "label": id.build_label(),
        },
        "triggers": ["--headless", "headless", "--help", "-h", "--version", "-V", "NEOFLASH_HEADLESS=1"],
        "output_formats": ["json", "text"],
        "json_contract": "When --format json is active (default for --headless), stdout is always a single valid JSON object on both success and failure.",
        "exit_codes": {
            "0": "success",
            "1": "domain_validation_compile_jsfl_or_io_error",
            "2": "cli_usage_flag_or_schema_error"
        },
        "subcommands": subcommands,
        "script_languages": [
            { "id": "actionscript1", "aliases": ["as1"] },
            { "id": "actionscript2", "aliases": ["as2"] },
            { "id": "actionscript3", "aliases": ["as3"] },
            { "id": "javascript", "aliases": ["js"] },
            { "id": "typescript", "aliases": ["ts"] }
        ],
        "jsfl": {
            "sandbox": "JsflSandbox",
            "flfile_default": "deny_all",
            "trace_fields": ["traces", "output_lines"],
            "capability_flags": ["--flfile-root <dir>", "--allow-read <dir>", "--allow-write <dir>", "--allow-delete"]
        },
        "batch_schema": batch_schema,
        "import_formats": import_formats,
        "export_targets": export_targets,
        "html5_profiles": html5_profiles,
        "swf_targets": ["fp8", "fp9", "fp10", "fp10-scriptless"],
        "image_formats": ["png", "jpeg", "jpg", "webp", "gif", "bmp", "psd", "pdf", "ai", "svg"],
        "size_levels": ["full", "balanced", "compact", "shared"],
        "html_render_modes": ["canvas", "dom"]
    });
    match format {
        OutputFormat::Json => match serialize_json_value(&payload, pretty) {
            Ok(text) => {
                let _ = writeln!(stdout, "{text}");
                0
            }
            Err(err) => {
                write_json_error(&err, format, pretty, stdout, stderr);
                err.exit_code
            }
        },
        OutputFormat::Text => {
            let _ = writeln!(
                stdout,
                "NeoFlash {} ({}) headless CLI: capabilities, new, inspect, validate, compile, eval, jsfl, batch, import, export | import_formats=nflash,fla,xfl,swf,swc,fig,svg,psd,pdf,ai,eps,fxg,png,jpg,gif,webp,bmp,mp3,wav,ogg,flac,aac,m4a,mp4,webm,mov,avi,ttf,otf,ttc,otc,woff,woff2,act,clr,ase | export_targets=html5,swf,image,png-sequence,aep,fla,xfl,svg,lottie,dotlottie,atlas,video,audio,banner,desktop | exit_codes=0,1,2",
                id.version,
                id.commit_short()
            );
            0
        }
    }
}

fn cmd_new<W1: Write, W2: Write>(
    args: &[String],
    format: OutputFormat,
    pretty: bool,
    stdout: &mut W1,
    stderr: &mut W2,
) -> i32 {
    match run_cmd_new(args, format, pretty, stdout) {
        Ok(()) => 0,
        Err(err) => {
            write_json_error(&err, format, pretty, stdout, stderr);
            err.exit_code
        }
    }
}

fn run_cmd_new<W1: Write>(
    args: &[String],
    format: OutputFormat,
    pretty: bool,
    stdout: &mut W1,
) -> Result<(), CliError> {
    let mut output: Option<String> = None;
    let mut name: Option<String> = None;
    let mut width: Option<u32> = None;
    let mut height: Option<u32> = None;
    let mut fps: Option<u32> = None;
    let mut bg: Option<Color> = None;
    let mut script: Option<ScriptLanguage> = None;
    let mut language: Option<ScriptLanguage> = None;
    let mut demo = false;

    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--output" | "--out" | "-o" => {
                output = Some(take_flag_value(args, &mut i, "--output")?)
            }
            "--name" => name = Some(take_flag_value(args, &mut i, "--name")?),
            "--width" => {
                let raw = take_flag_value(args, &mut i, "--width")?;
                width = Some(raw.parse::<u32>().map_err(|_| {
                    CliError::usage("invalid_argument", format!("Invalid --width '{raw}'"))
                })?);
            }
            "--height" => {
                let raw = take_flag_value(args, &mut i, "--height")?;
                height = Some(raw.parse::<u32>().map_err(|_| {
                    CliError::usage("invalid_argument", format!("Invalid --height '{raw}'"))
                })?);
            }
            "--fps" => {
                let raw = take_flag_value(args, &mut i, "--fps")?;
                let parsed = raw.parse::<f64>().map_err(|_| {
                    CliError::usage("invalid_argument", format!("Invalid --fps '{raw}'"))
                })?;
                if !parsed.is_finite() || parsed < 1.0 {
                    return Err(CliError::usage("invalid_argument", "--fps must be >= 1"));
                }
                fps = Some(parsed.round() as u32);
            }
            "--bg" => {
                let raw = take_flag_value(args, &mut i, "--bg")?;
                bg = Some(parse_color(&raw)?);
            }
            "--script" => {
                let raw = take_flag_value(args, &mut i, "--script")?;
                script = Some(parse_language(&raw)?);
            }
            "--language" | "--lang" => {
                let raw = take_flag_value(args, &mut i, "--language")?;
                language = Some(parse_language(&raw)?);
            }
            "--demo" => demo = true,
            other => {
                return Err(CliError::usage(
                    "unknown_flag",
                    format!("Unknown flag '{other}' for 'new'"),
                ));
            }
        }
        i += 1;
    }

    let out_str = output
        .ok_or_else(|| CliError::usage("missing_flag", "'new' requires --output <file.nflash>"))?;
    let out_path = Path::new(&out_str);
    let mut project = build_new_project(name, width, height, fps, bg, language, demo)?;
    let resolved_script = if demo {
        script
            .or(language)
            .unwrap_or_else(|| project.infer_script_language())
    } else {
        script.or(language).unwrap_or(ScriptLanguage::ActionScript3)
    };
    project.script_language = Some(resolved_script);
    project.also_compile_compatible_scripts = true;
    upgrade_project_format_versions(&mut project);
    project
        .validate()
        .map_err(|e| CliError::runtime("validation_failed", e.to_string()))?;
    save_project_atomic(&mut project, out_path)?;

    match format {
        OutputFormat::Json => {
            let val = json!({
                "ok": true,
                "command": "new",
                "path": out_path.display().to_string(),
                "name": project.name,
                "width": project.width,
                "height": project.height,
                "fps": project.fps,
                "background": color_hex(project.background),
                "background_rgba": project.background,
                "root_symbol": project.root,
                "script_language": project.script_language.map(ScriptLanguage::short_slug),
                "also_compile_compatible_scripts": project.also_compile_compatible_scripts,
                "language": language.map(|l| format!("{l:?}").to_ascii_lowercase()),
                "demo": demo,
                "format_version": project.format_version,
                "symbols": project.symbols.len(),
            });
            let text = serialize_json_value(&val, pretty)?;
            writeln!(stdout, "{text}").map_err(|e| {
                CliError::runtime("io_error", format!("Failed writing stdout: {e}"))
            })?;
        }
        OutputFormat::Text => {
            writeln!(
                stdout,
                "Created {} ({}x{} @ {}fps, {} symbol(s))",
                out_path.display(),
                project.width,
                project.height,
                project.fps,
                project.symbols.len()
            )
            .map_err(|e| CliError::runtime("io_error", format!("Failed writing stdout: {e}")))?;
        }
    }
    Ok(())
}

fn cmd_inspect<W1: Write, W2: Write>(
    args: &[String],
    format: OutputFormat,
    pretty: bool,
    stdout: &mut W1,
    stderr: &mut W2,
) -> i32 {
    match run_cmd_inspect(args, format, pretty, stdout) {
        Ok(()) => 0,
        Err(err) => {
            write_json_error(&err, format, pretty, stdout, stderr);
            err.exit_code
        }
    }
}

fn run_cmd_inspect<W1: Write>(
    args: &[String],
    format: OutputFormat,
    pretty: bool,
    stdout: &mut W1,
) -> Result<(), CliError> {
    let mut project_path: Option<String> = None;
    let mut opts = InspectOptions::default();

    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--project" => project_path = Some(take_flag_value(args, &mut i, "--project")?),
            "--symbol" => opts.symbol = Some(take_flag_value(args, &mut i, "--symbol")?),
            "--frame" => {
                let raw = take_flag_value(args, &mut i, "--frame")?;
                opts.frame = Some(raw.parse::<u32>().map_err(|_| {
                    CliError::usage("invalid_argument", format!("Invalid --frame '{raw}'"))
                })?);
            }
            "--no-elements" => opts.no_elements = true,
            "--no-scripts" => opts.no_scripts = true,
            "--no-resources" => opts.no_resources = true,
            other => {
                return Err(CliError::usage(
                    "unknown_flag",
                    format!("Unknown flag '{other}' for 'inspect'"),
                ));
            }
        }
        i += 1;
    }

    let path_str = project_path.ok_or_else(|| {
        CliError::usage("missing_flag", "'inspect' requires --project <file.nflash>")
    })?;
    let project = load_project_lenient(Path::new(&path_str))?;
    let inspect_val = build_inspect_value(&project, &opts)?;

    match format {
        OutputFormat::Json => {
            let mut obj = serde_json::Map::new();
            obj.insert("ok".into(), Value::Bool(true));
            obj.insert("command".into(), Value::String("inspect".into()));
            obj.insert("path".into(), Value::String(path_str));
            if let Value::Object(map) = inspect_val {
                obj.extend(map);
            }
            let text = serialize_json_value(&Value::Object(obj), pretty)?;
            writeln!(stdout, "{text}").map_err(|e| {
                CliError::runtime("io_error", format!("Failed writing stdout: {e}"))
            })?;
        }
        OutputFormat::Text => {
            writeln!(
                stdout,
                "Project '{}' ({}x{} @ {}fps): {} symbol(s), {} scene(s), {} resource(s), valid={}",
                project.name,
                project.width,
                project.height,
                project.fps,
                project.symbols.len(),
                inspect_val["scenes"].as_array().map_or(0, Vec::len),
                project.resources.len(),
                inspect_val["validation"]["valid"]
                    .as_bool()
                    .unwrap_or(false),
            )
            .map_err(|e| CliError::runtime("io_error", format!("Failed writing stdout: {e}")))?;
        }
    }
    Ok(())
}

pub(crate) fn build_inspect_value(
    project: &Project,
    opts: &InspectOptions,
) -> Result<Value, CliError> {
    let issues = validate_project_model(project);
    let issue_json: Vec<Value> = issues
        .iter()
        .map(|issue| {
            json!({
                "path": issue.path,
                "message": issue.message,
            })
        })
        .collect();

    let target_symbols: Vec<&Symbol> = if let Some(ref sel) = opts.symbol {
        vec![resolve_symbol(project, sel)?]
    } else {
        project.symbols.iter().collect()
    };

    let scenes_json: Vec<Value> = {
        let ranges = scene_ranges(project);
        if ranges.is_empty() {
            project
                .symbol(&project.root)
                .map(|root| {
                    vec![json!({
                        "id": root.id,
                        "name": root.name,
                        "start_frame": 0,
                        "frame_count": root.timeline.frame_count,
                    })]
                })
                .unwrap_or_default()
        } else {
            project
                .scenes
                .iter()
                .zip(ranges.iter())
                .map(|(id, range)| {
                    json!({
                        "id": id,
                        "name": range.name,
                        "start_frame": range.start,
                        "frame_count": range.frames,
                    })
                })
                .collect()
        }
    };

    let mut referenced_fonts = BTreeSet::new();
    for symbol in &project.symbols {
        for layer in &symbol.timeline.layers {
            for kf in &layer.keyframes {
                for el in &kf.elements {
                    match &el.kind {
                        ElementKind::Text { font_family, .. } => {
                            referenced_fonts.insert(font_family.clone());
                        }
                        ElementKind::TextBox { format, .. } => {
                            referenced_fonts.insert(format.font.clone());
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    let embedded_fonts: Vec<String> = embedded_font_families(project).into_iter().collect();

    let symbols_json: Vec<Value> = target_symbols
        .iter()
        .map(|sym| inspect_symbol_json(project, sym, opts))
        .collect();

    let sources_json: Vec<Value> = if opts.no_scripts {
        Vec::new()
    } else {
        project
            .sources
            .iter()
            .map(|unit| {
                json!({
                    "id": unit.id,
                    "path": unit.path,
                    "language": format!("{:?}", unit.language).to_ascii_lowercase(),
                    "line_count": unit.source.lines().count(),
                    "bytes": unit.source.len(),
                })
            })
            .collect()
    };

    let resources_json: Vec<Value> = if opts.no_resources {
        Vec::new()
    } else {
        project
            .resources
            .iter()
            .map(inspect_resource_json)
            .collect()
    };

    let profiles_json = match load_profiles_from_project(project) {
        Ok((profiles, active_id)) => {
            let list: Vec<Value> = profiles
                .iter()
                .map(|p| {
                    json!({
                        "id": p.id,
                        "name": p.name,
                        "size_level": p.options.size.name(),
                        "fonts": format!("{:?}", p.options.fonts).to_ascii_lowercase(),
                        "audio": format!("{:?}", p.options.audio).to_ascii_lowercase(),
                    })
                })
                .collect();
            json!({ "active_profile_id": active_id, "profiles": list })
        }
        Err(err) => json!({ "error": err.to_string(), "profiles": [] }),
    };

    Ok(json!({
        "metadata": {
            "name": project.name,
            "format_version": project.format_version,
            "width": project.width,
            "height": project.height,
            "fps": project.fps,
            "background": color_hex(project.background),
            "background_rgba": project.background,
            "root_symbol": project.root,
            "document_class": project.document_class,
            "script_language": project.script_language.map(ScriptLanguage::short_slug),
            "effective_script_language": project.effective_script_language().short_slug(),
            "also_compile_compatible_scripts": project.also_compile_compatible_scripts,
        },
        "scenes": scenes_json,
        "symbols": symbols_json,
        "sources": sources_json,
        "resources": resources_json,
        "fonts": {
            "embedded": embedded_fonts,
            "referenced": referenced_fonts.into_iter().collect::<Vec<_>>(),
        },
        "publish": profiles_json,
        "validation": {
            "valid": issue_json.is_empty(),
            "issue_count": issue_json.len(),
            "issues": issue_json,
        }
    }))
}

fn inspect_symbol_json(project: &Project, sym: &Symbol, opts: &InspectOptions) -> Value {
    let layers_json: Vec<Value> = sym
        .timeline
        .layers
        .iter()
        .map(|layer| {
            let keyframes_json: Vec<Value> = if let Some(frame) = opts.frame {
                match layer.keyframes.iter().rfind(|k| k.frame <= frame) {
                    Some(kf) => vec![inspect_keyframe_json(kf, opts.no_elements, opts.no_scripts)],
                    None => Vec::new(),
                }
            } else {
                layer
                    .keyframes
                    .iter()
                    .map(|kf| inspect_keyframe_json(kf, opts.no_elements, opts.no_scripts))
                    .collect()
            };
            json!({
                "id": layer.id,
                "name": layer.name,
                "role": format!("{:?}", layer.properties.role).to_ascii_lowercase(),
                "visible": layer.visible,
                "locked": layer.locked,
                "mask": layer.properties.mask,
                "motion_guide": layer.properties.motion_guide,
                "keyframe_count": layer.keyframes.len(),
                "keyframes": keyframes_json,
            })
        })
        .collect();

    let scripts_json: Vec<Value> = if opts.no_scripts {
        Vec::new()
    } else {
        sym.timeline
            .scripts
            .iter()
            .filter(|fs| opts.frame.is_none_or(|f| f == fs.frame))
            .map(|fs| {
                json!({
                    "frame": fs.frame,
                    "language": format!("{:?}", fs.script.language).to_ascii_lowercase(),
                    "line_count": fs.script.source.lines().count(),
                    "source": fs.script.source,
                })
            })
            .collect()
    };

    json!({
        "id": sym.id,
        "name": sym.name,
        "folder": sym.folder,
        "kind": format!("{:?}", sym.kind).to_ascii_lowercase(),
        "is_scene": project.is_scene(&sym.id),
        "linkage_id": sym.linkage_id,
        "class_name": sym.class_name,
        "frame_count": sym.timeline.frame_count,
        "layer_count": sym.timeline.layers.len(),
        "layers": layers_json,
        "frame_scripts": scripts_json,
    })
}

fn inspect_keyframe_json(
    kf: &neoflash_core::Keyframe,
    no_elements: bool,
    no_scripts: bool,
) -> Value {
    let elements_json: Vec<Value> = if no_elements {
        Vec::new()
    } else {
        kf.elements
            .iter()
            .map(|el| inspect_element_json(el, no_scripts))
            .collect()
    };
    json!({
        "frame": kf.frame,
        "label": (!kf.label.is_empty()).then_some(&kf.label),
        "tween": kf.tween.as_ref().map(|t| format!("{t:?}")),
        "tween_kind": format!("{:?}", kf.tween_kind).to_ascii_lowercase(),
        "has_sound": kf.sound.is_some(),
        "element_count": kf.elements.len(),
        "elements": elements_json,
    })
}

fn inspect_element_json(el: &Element, no_scripts: bool) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("id".into(), Value::String(el.id.clone()));
    obj.insert("name".into(), Value::String(el.name.clone()));
    obj.insert("visible".into(), Value::Bool(el.visible));
    obj.insert("transform".into(), json!(el.transform));
    obj.insert("kind".into(), summarize_element_kind(&el.kind));
    if !no_scripts && !el.scripts.is_empty() {
        let scripts: Vec<Value> = el
            .scripts
            .iter()
            .map(|os| {
                json!({
                    "event": format!("{:?}", os.event).to_ascii_lowercase(),
                    "language": format!("{:?}", os.script.language).to_ascii_lowercase(),
                    "line_count": os.script.source.lines().count(),
                    "source": os.script.source,
                })
            })
            .collect();
        obj.insert("scripts".into(), Value::Array(scripts));
    }
    Value::Object(obj)
}

fn inspect_resource_json(res: &ResourceUnit) -> Value {
    json!({
        "id": res.id,
        "path": res.path,
        "media_type": res.media_type,
        "byte_len": res.byte_len(),
        "encoded_len": res.data_base64.len(),
        "is_audio": res.is_audio(),
        "is_video": res.is_video(),
    })
}

fn cmd_validate<W1: Write, W2: Write>(
    args: &[String],
    format: OutputFormat,
    pretty: bool,
    stdout: &mut W1,
    stderr: &mut W2,
) -> i32 {
    match run_cmd_validate(args, format, pretty, stdout) {
        Ok(()) => 0,
        Err(err) => {
            write_json_error(&err, format, pretty, stdout, stderr);
            err.exit_code
        }
    }
}

fn run_cmd_validate<W1: Write>(
    args: &[String],
    format: OutputFormat,
    pretty: bool,
    stdout: &mut W1,
) -> Result<(), CliError> {
    let mut project_path: Option<String> = None;
    let mut strict = false;
    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--project" => project_path = Some(take_flag_value(args, &mut i, "--project")?),
            "--strict" => strict = true,
            other => {
                return Err(CliError::usage(
                    "unknown_flag",
                    format!("Unknown flag '{other}' for 'validate'"),
                ));
            }
        }
        i += 1;
    }
    let path_str = project_path.ok_or_else(|| {
        CliError::usage(
            "missing_flag",
            "'validate' requires --project <file.nflash>",
        )
    })?;
    let project_file = Path::new(&path_str);
    let base_dir = project_file.parent().filter(|p| !p.as_os_str().is_empty());
    let project = load_project_lenient(project_file)?;
    let report = validate_loaded_project(&project, base_dir, strict)?;

    match format {
        OutputFormat::Json => {
            let mut obj = serde_json::Map::new();
            obj.insert("ok".into(), Value::Bool(true));
            obj.insert("command".into(), Value::String("validate".into()));
            obj.insert("path".into(), Value::String(path_str));
            if let Value::Object(map) = report {
                obj.extend(map);
            }
            let text = serialize_json_value(&Value::Object(obj), pretty)?;
            writeln!(stdout, "{text}").map_err(|e| {
                CliError::runtime("io_error", format!("Failed writing stdout: {e}"))
            })?;
        }
        OutputFormat::Text => {
            writeln!(
                stdout,
                "Valid project '{}' (format_version={}, warnings={})",
                project.name,
                project.format_version,
                report["script_warning_count"].as_u64().unwrap_or(0)
            )
            .map_err(|e| CliError::runtime("io_error", format!("Failed writing stdout: {e}")))?;
        }
    }
    Ok(())
}

pub(crate) fn validate_loaded_project(
    project: &Project,
    base_dir: Option<&Path>,
    strict: bool,
) -> Result<Value, CliError> {
    let model_issues: Vec<Value> = validate_project_model(project)
        .into_iter()
        .map(|issue| json!({ "path": issue.path, "message": issue.message }))
        .collect();
    let hydrated =
        crate::app::standalone_script::hydrate_project_includes_from_disk(project, base_dir);
    let resolved = hydrated.with_resolved_script_language();
    let script_diags = collect_located_script_diagnostics(&resolved);
    let script_errors: Vec<Value> = script_diags
        .iter()
        .filter(|d| d.severity == DiagnosticSeverity::Error)
        .map(located_diag_to_json)
        .collect();
    let script_warnings: Vec<Value> = script_diags
        .iter()
        .filter(|d| d.severity == DiagnosticSeverity::Warning)
        .map(located_diag_to_json)
        .collect();

    let failed = !model_issues.is_empty()
        || !script_errors.is_empty()
        || (strict && !script_warnings.is_empty());

    let report = json!({
        "valid": !failed,
        "format_version": project.format_version,
        "script_language": project.script_language.map(ScriptLanguage::short_slug),
        "effective_script_language": resolved.effective_script_language().short_slug(),
        "also_compile_compatible_scripts": resolved.also_compile_compatible_scripts,
        "validation_issue_count": model_issues.len(),
        "validation_issues": model_issues,
        "script_error_count": script_errors.len(),
        "script_errors": script_errors,
        "script_warning_count": script_warnings.len(),
        "script_warnings": script_warnings,
    });

    if failed {
        return Err(CliError::runtime_with_details(
            "validation_failed",
            format!(
                "Project validation failed ({} model issue(s), {} script error(s), {} warning(s))",
                report["validation_issue_count"].as_u64().unwrap_or(0),
                report["script_error_count"].as_u64().unwrap_or(0),
                report["script_warning_count"].as_u64().unwrap_or(0),
            ),
            report,
        ));
    }

    Ok(report)
}

fn cmd_compile<W1: Write, W2: Write>(
    args: &[String],
    format: OutputFormat,
    pretty: bool,
    stdout: &mut W1,
    stderr: &mut W2,
) -> i32 {
    match run_cmd_compile(args, format, pretty, stdout) {
        Ok(()) => 0,
        Err(err) => {
            write_json_error(&err, format, pretty, stdout, stderr);
            err.exit_code
        }
    }
}

fn run_cmd_compile<W1: Write>(
    args: &[String],
    format: OutputFormat,
    pretty: bool,
    stdout: &mut W1,
) -> Result<(), CliError> {
    let mut project_path: Option<String> = None;
    let mut strict = false;
    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--project" => project_path = Some(take_flag_value(args, &mut i, "--project")?),
            "--strict" => strict = true,
            other => {
                return Err(CliError::usage(
                    "unknown_flag",
                    format!("Unknown flag '{other}' for 'compile'"),
                ));
            }
        }
        i += 1;
    }
    let path_str = project_path.ok_or_else(|| {
        CliError::usage("missing_flag", "'compile' requires --project <file.nflash>")
    })?;
    let project_file = Path::new(&path_str);
    let base_dir = project_file.parent().filter(|p| !p.as_os_str().is_empty());
    let project = load_project_valid(project_file)?;
    let report = compile_loaded_project_scripts(&project, base_dir);
    let error_count = report["error_count"].as_u64().unwrap_or(0);
    let warning_count = report["warning_count"].as_u64().unwrap_or(0);
    if error_count > 0 || (strict && warning_count > 0) {
        return Err(CliError::runtime_with_details(
            "compile_failed",
            format!(
                "Script compilation failed ({error_count} error(s), {warning_count} warning(s))"
            ),
            report,
        ));
    }

    match format {
        OutputFormat::Json => {
            let mut obj = serde_json::Map::new();
            obj.insert("ok".into(), Value::Bool(true));
            obj.insert("command".into(), Value::String("compile".into()));
            obj.insert("path".into(), Value::String(path_str));
            if let Value::Object(map) = report {
                obj.extend(map);
            }
            let text = serialize_json_value(&Value::Object(obj), pretty)?;
            writeln!(stdout, "{text}").map_err(|e| {
                CliError::runtime("io_error", format!("Failed writing stdout: {e}"))
            })?;
        }
        OutputFormat::Text => {
            writeln!(
                stdout,
                "Compile ok: 0 errors, {warning_count} warning(s) across {}",
                path_str
            )
            .map_err(|e| CliError::runtime("io_error", format!("Failed writing stdout: {e}")))?;
        }
    }
    Ok(())
}

pub(crate) fn compile_loaded_project_scripts(project: &Project, base_dir: Option<&Path>) -> Value {
    let hydrated =
        crate::app::standalone_script::hydrate_project_includes_from_disk(project, base_dir);
    let resolved = hydrated.with_resolved_script_language();
    let eff_lang = resolved.effective_script_language();
    let also_compat = resolved.also_compile_compatible_scripts;
    let diags = collect_located_script_diagnostics(&resolved);
    let error_count = diags
        .iter()
        .filter(|d| d.severity == DiagnosticSeverity::Error)
        .count();
    let warning_count = diags
        .iter()
        .filter(|d| d.severity == DiagnosticSeverity::Warning)
        .count();
    let diag_json: Vec<Value> = diags.iter().map(located_diag_to_json).collect();

    let mut compiled_scripts_count = 0usize;
    let mut skipped_foreign_scripts_count = 0usize;
    let mut tally = |lang: ScriptLanguage, empty: bool| {
        if empty {
            return;
        }
        if resolved
            .script_disposition_for(eff_lang, also_compat, lang)
            .is_compiled()
        {
            compiled_scripts_count += 1;
        } else {
            skipped_foreign_scripts_count += 1;
        }
    };
    for unit in &resolved.sources {
        tally(unit.language, false);
    }
    for sym in &resolved.symbols {
        for fs in &sym.timeline.scripts {
            tally(fs.script.language, fs.script.source.trim().is_empty());
        }
        for layer in &sym.timeline.layers {
            for kf in &layer.keyframes {
                for el in &kf.elements {
                    for os in &el.scripts {
                        tally(os.script.language, os.script.source.trim().is_empty());
                    }
                }
            }
        }
    }
    let scripts_checked = project.sources.len()
        + project
            .symbols
            .iter()
            .map(|s| {
                s.timeline.scripts.len()
                    + s.timeline
                        .layers
                        .iter()
                        .flat_map(|l| &l.keyframes)
                        .flat_map(|k| &k.elements)
                        .map(|e| e.scripts.len())
                        .sum::<usize>()
            })
            .sum::<usize>();
    json!({
        "script_language": project.script_language.map(ScriptLanguage::short_slug),
        "effective_script_language": eff_lang.short_slug(),
        "also_compile_compatible_scripts": also_compat,
        "scripts_checked": scripts_checked,
        "compiled_scripts_count": compiled_scripts_count,
        "skipped_foreign_scripts_count": skipped_foreign_scripts_count,
        "error_count": error_count,
        "warning_count": warning_count,
        "diagnostics": diag_json,
    })
}

fn located_diag_to_json(d: &LocatedScriptDiagnostic) -> Value {
    json!({
        "symbol": d.symbol,
        "symbol_id": d.symbol_id,
        "layer": d.layer,
        "frame": d.frame,
        "element_id": d.element_id,
        "element_name": d.element_name,
        "event": d.event.map(|ev| format!("{ev:?}").to_ascii_lowercase()),
        "path": d.path,
        "language": format!("{:?}", d.language).to_ascii_lowercase(),
        "severity": format!("{:?}", d.severity).to_ascii_lowercase(),
        "code": d.code,
        "line": d.line,
        "column": d.column,
        "end_line": d.end_line,
        "end_column": d.end_column,
        "message": d.message,
        "location_label": d.location_label,
        "quick_fixes": d.quick_fixes,
    })
}

fn cmd_eval<W1: Write, W2: Write>(
    args: &[String],
    format: OutputFormat,
    pretty: bool,
    stdout: &mut W1,
    stderr: &mut W2,
) -> i32 {
    match run_cmd_eval(args, format, pretty, stdout) {
        Ok(()) => 0,
        Err(err) => {
            write_json_error(&err, format, pretty, stdout, stderr);
            err.exit_code
        }
    }
}

fn run_cmd_eval<W1: Write>(
    args: &[String],
    format: OutputFormat,
    pretty: bool,
    stdout: &mut W1,
) -> Result<(), CliError> {
    let mut project_path: Option<String> = None;
    let mut scene: Option<String> = None;
    let mut symbol: Option<String> = None;
    let mut frame_arg: Option<u32> = None;
    let mut frames_arg: Option<(u32, u32)> = None;
    let mut scripts = false;
    let mut strict = false;
    let mut vm = EvalVmMode::Preview;

    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--project" => project_path = Some(take_flag_value(args, &mut i, "--project")?),
            "--scene" => scene = Some(take_flag_value(args, &mut i, "--scene")?),
            "--symbol" => symbol = Some(take_flag_value(args, &mut i, "--symbol")?),
            "--frame" => {
                let raw = take_flag_value(args, &mut i, "--frame")?;
                frame_arg = Some(raw.parse::<u32>().map_err(|_| {
                    CliError::usage("invalid_argument", format!("Invalid --frame '{raw}'"))
                })?);
            }
            "--frames" => {
                let raw = take_flag_value(args, &mut i, "--frames")?;
                frames_arg = Some(parse_frame_range_str(&raw)?);
            }
            "--scripts" => scripts = true,
            "--strict" => strict = true,
            "--vm" => {
                let raw = take_flag_value(args, &mut i, "--vm")?;
                vm = match raw.trim().to_ascii_lowercase().as_str() {
                    "preview" | "authored" => EvalVmMode::Preview,
                    "swf" | "avm" | "bytecode" => EvalVmMode::Swf,
                    other => {
                        return Err(CliError::usage(
                            "invalid_argument",
                            format!("Unsupported --vm '{other}' (expected preview|swf)"),
                        ));
                    }
                };
                if vm == EvalVmMode::Swf {
                    scripts = true;
                }
            }
            other => {
                return Err(CliError::usage(
                    "unknown_flag",
                    format!("Unknown flag '{other}' for 'eval'"),
                ));
            }
        }
        i += 1;
    }

    if frame_arg.is_some() && frames_arg.is_some() {
        return Err(CliError::usage(
            "conflicting_flags",
            "--frame and --frames are mutually exclusive",
        ));
    }
    let path_str = project_path.ok_or_else(|| {
        CliError::usage("missing_flag", "'eval' requires --project <file.nflash>")
    })?;
    let (start_frame, end_frame) = match (frame_arg, frames_arg) {
        (Some(f), None) => (f, f),
        (None, Some((s, e))) => (s, e),
        _ => (0, 0),
    };

    let project_file = Path::new(&path_str);
    let base_dir = project_file.parent().filter(|p| !p.as_os_str().is_empty());
    let project = load_project_valid(project_file)?;
    let report = evaluate_loaded_project_frame(
        &project,
        base_dir,
        &EvalOptions {
            scene,
            symbol,
            start_frame,
            end_frame,
            scripts,
            strict,
            vm,
        },
    )?;

    match format {
        OutputFormat::Json => {
            let mut obj = serde_json::Map::new();
            obj.insert("ok".into(), Value::Bool(true));
            obj.insert("command".into(), Value::String("eval".into()));
            obj.insert("path".into(), Value::String(path_str));
            if let Value::Object(map) = report {
                obj.extend(map);
            }
            let text = serialize_json_value(&Value::Object(obj), pretty)?;
            writeln!(stdout, "{text}").map_err(|e| {
                CliError::runtime("io_error", format!("Failed writing stdout: {e}"))
            })?;
        }
        OutputFormat::Text => {
            let frame_count = report["frames"].as_array().map_or(0, Vec::len);
            writeln!(
                stdout,
                "Evaluated {} frame(s) of symbol '{}' (traces={})",
                frame_count,
                report["symbol_name"].as_str().unwrap_or(""),
                report["trace_lines"].as_array().map_or(0, Vec::len),
            )
            .map_err(|e| CliError::runtime("io_error", format!("Failed writing stdout: {e}")))?;
            if let Some(traces) = report["trace_lines"].as_array() {
                for line in traces.iter().filter_map(Value::as_str) {
                    writeln!(stdout, "trace: {line}").map_err(|e| {
                        CliError::runtime("io_error", format!("Failed writing stdout: {e}"))
                    })?;
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn evaluate_loaded_project_frame(
    project: &Project,
    base_dir: Option<&Path>,
    opts: &EvalOptions,
) -> Result<Value, CliError> {
    if opts.scene.is_some() && opts.symbol.is_some() {
        return Err(CliError::usage(
            "conflicting_flags",
            "--scene and --symbol cannot both be set",
        ));
    }
    let hydrated =
        crate::app::standalone_script::hydrate_project_includes_from_disk(project, base_dir);
    let project = hydrated.as_ref();
    if opts.vm == EvalVmMode::Swf {
        return evaluate_loaded_project_swf_vm(project, opts);
    }
    let (eval_project, sym_id): (Cow<'_, Project>, String) = if let Some(ref sym_sel) = opts.symbol
    {
        let sym = resolve_symbol(project, sym_sel)?;
        (Cow::Borrowed(project), sym.id.clone())
    } else if let Some(ref scene_sel) = opts.scene {
        let scene_sym = resolve_scene_symbol(project, scene_sel)?;
        (Cow::Borrowed(project), scene_sym.id.clone())
    } else {
        let (flat, _) = flatten_scenes_cow(project);
        let root = flat.root.clone();
        (flat, root)
    };

    let sym = eval_project.symbol(&sym_id).ok_or_else(|| {
        CliError::runtime("symbol_not_found", format!("Symbol '{sym_id}' missing"))
    })?;
    let total_frames = sym.timeline.frame_count.max(1);
    if opts.end_frame >= total_frames {
        return Err(CliError::runtime(
            "frame_out_of_range",
            format!(
                "Frame {} is outside timeline '{}' range 0..{total_frames}",
                opts.end_frame, sym.name
            ),
        ));
    }

    let mut playback =
        AuthoredPlayback::with_live_scripts(&eval_project, &sym_id, opts.start_frame, opts.scripts);
    playback.set_looping(false);

    let mut all_traces: Vec<String> = Vec::new();
    let mut all_diagnostics: Vec<String> = Vec::new();
    if opts.scripts && !eval_project.sources.is_empty() {
        all_diagnostics.push(format!(
            "project defines {} class/module source file(s) outside the bounded native stage preview subset — use `--headless eval --vm swf` or HTML5 export for full runtime execution.",
            eval_project.sources.len()
        ));
    }
    let mut frames_out: Vec<Value> = Vec::new();

    for step_frame in opts.start_frame..=opts.end_frame {
        if step_frame > opts.start_frame {
            playback.tick(&eval_project);
        }
        let actual_frame = playback.frame();
        let halted = playback.is_root_halted_by_script();
        let step_traces = playback.drain_traces();
        let step_diags: Vec<String> = playback
            .drain_diagnostics()
            .into_iter()
            .map(|d| {
                d.replace(
                    " — use Test Movie (Cmd+Return) for full runtime execution.",
                    " — use `--headless eval --vm swf` or HTML5 export for full runtime execution.",
                )
            })
            .collect();
        all_traces.extend(step_traces.iter().cloned());
        all_diagnostics.extend(step_diags.iter().cloned());

        let scene = playback.scene(&eval_project, false, true);
        let display_list: Vec<Value> = scene.draws.iter().map(summarize_authored_draw).collect();
        let layers: Vec<Value> = sym
            .timeline
            .layers
            .iter()
            .map(|layer| summarize_evaluated_layer(sym, layer, actual_frame, eval_project.fps))
            .collect();

        frames_out.push(json!({
            "frame": step_frame,
            "playback_frame": actual_frame,
            "halted_by_script": halted,
            "traces": step_traces,
            "diagnostics": step_diags,
            "layers": layers,
            "display_list": display_list,
        }));
    }

    let report = json!({
        "vm": "preview",
        "symbol_id": sym.id,
        "symbol_name": sym.name,
        "fps": eval_project.fps,
        "total_frames": total_frames,
        "start_frame": opts.start_frame,
        "end_frame": opts.end_frame,
        "scripts_executed": opts.scripts,
        "trace_lines": all_traces,
        "script_diagnostics": all_diagnostics,
        "frames": frames_out,
    });
    if opts.strict && !all_diagnostics.is_empty() {
        return Err(CliError::runtime_with_details(
            "eval_strict_failed",
            format!(
                "Strict preview evaluation produced {} diagnostic(s); use --vm swf for full AVM1/AVM2 bytecode execution",
                all_diagnostics.len()
            ),
            report,
        ));
    }
    Ok(report)
}

fn evaluate_loaded_project_swf_vm(
    project: &Project,
    opts: &EvalOptions,
) -> Result<Value, CliError> {
    if opts.scene.is_some() || opts.symbol.is_some() {
        return Err(CliError::usage(
            "unsupported_flag",
            "'eval --vm swf' evaluates the published movie root timeline; omit --scene/--symbol",
        ));
    }
    let resolved = project.with_resolved_script_language();
    let eff_lang = resolved.effective_script_language();
    let profile = match eff_lang {
        ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2 => {
            neoflash_export::SwfPublishProfile::FlashPlayer8Avm1
        }
        ScriptLanguage::ActionScript3 => neoflash_export::SwfPublishProfile::FlashPlayer10Avm2,
        other => {
            return Err(CliError::usage(
                "unsupported_language",
                format!(
                    "'eval --vm swf' supports ActionScript 1/2/3 projects (found {})",
                    other.display_label()
                ),
            ));
        }
    };
    static EVAL_SWF_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let seq = EVAL_SWF_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp_swf =
        std::env::temp_dir().join(format!("neoflash-eval-vm-{}-{seq}.swf", std::process::id()));
    let swf_opts = neoflash_export::SwfPublishOptions {
        profile,
        scriptless_mode: false,
        outline_text: false,
        ..neoflash_export::SwfPublishOptions::default()
    };
    let compile_res = neoflash_export::publish_swf(&resolved, &tmp_swf, &swf_opts);
    if let Err(err) = compile_res {
        let _ = fs::remove_file(&tmp_swf);
        return Err(CliError::runtime(
            "eval_swf_compile_failed",
            err.to_string(),
        ));
    }
    let player_res = neoflash_import::SwfBytecodePlayer::from_path(&tmp_swf);
    let _ = fs::remove_file(&tmp_swf);
    let mut player =
        player_res.map_err(|e| CliError::runtime("eval_swf_load_failed", e.to_string()))?;

    let root_sym = resolved
        .symbol(&resolved.root)
        .ok_or_else(|| CliError::runtime("symbol_not_found", "Root symbol missing"))?;
    let total_frames = player.total_frames().max(1);
    let mut all_traces: Vec<String> = Vec::new();
    let mut all_audio_events: Vec<String> = Vec::new();
    let mut all_diagnostics: Vec<String> = Vec::new();
    let mut frames_out: Vec<Value> = Vec::new();

    for tick_idx in 0..=opts.end_frame {
        let state = player.step_frame();
        if tick_idx >= opts.start_frame {
            let step_audio: Vec<String> = state
                .audio_events
                .iter()
                .map(|ev| format!("{ev:?}"))
                .collect();
            all_traces.extend(state.traces.iter().cloned());
            all_audio_events.extend(step_audio.iter().cloned());
            all_diagnostics.extend(state.unsupported_diagnostics.iter().cloned());
            frames_out.push(json!({
                "frame": tick_idx,
                "playback_frame": state.frame,
                "halted_by_script": !state.playing,
                "traces": state.traces,
                "audio_events": step_audio,
                "diagnostics": state.unsupported_diagnostics,
                "display_count": state.display_list.len(),
            }));
        }
    }

    let report = json!({
        "vm": "swf",
        "swf_version": player.swf_version(),
        "symbol_id": root_sym.id,
        "symbol_name": root_sym.name,
        "fps": resolved.fps,
        "total_frames": total_frames,
        "start_frame": opts.start_frame,
        "end_frame": opts.end_frame,
        "scripts_executed": true,
        "trace_lines": all_traces,
        "audio_events": all_audio_events,
        "script_diagnostics": all_diagnostics,
        "frames": frames_out,
    });
    if opts.strict && !all_diagnostics.is_empty() {
        return Err(CliError::runtime_with_details(
            "eval_strict_failed",
            format!(
                "Strict SWF VM evaluation produced {} diagnostic(s)",
                all_diagnostics.len()
            ),
            report,
        ));
    }
    Ok(report)
}

fn resolve_scene_symbol<'a>(project: &'a Project, selector: &str) -> Result<&'a Symbol, CliError> {
    let scene_ids = project.scene_ids();
    for id in &scene_ids {
        if let Some(sym) = project.symbol(id)
            && sym.id == selector
        {
            return Ok(sym);
        }
    }
    let matches: Vec<&Symbol> = scene_ids
        .iter()
        .filter_map(|id| project.symbol(id))
        .filter(|sym| sym.name.eq_ignore_ascii_case(selector))
        .collect();
    if matches.len() == 1 {
        return Ok(matches[0]);
    }
    if matches.len() > 1 {
        return Err(CliError::runtime(
            "ambiguous_scene",
            format!("Multiple scenes match '{selector}'; specify scene ID"),
        ));
    }
    Err(CliError::runtime(
        "scene_not_found",
        format!("Scene '{selector}' not found"),
    ))
}

fn summarize_evaluated_layer(sym: &Symbol, layer: &Layer, frame: u32, fps: u32) -> Value {
    let eff_visible = layer_effectively_visible(&sym.timeline, layer);
    let sampled = if eff_visible && !matches!(layer.properties.role, LayerRole::Folder) {
        sample_layer_in_at_fps(&sym.timeline, layer, frame, f64::from(fps.max(1)))
    } else {
        Vec::new()
    };
    let elements_json: Vec<Value> = sampled
        .iter()
        .map(|el| {
            json!({
                "id": el.id,
                "name": el.name,
                "visible": el.visible,
                "transform": el.transform,
                "kind": summarize_element_kind(&el.kind),
            })
        })
        .collect();
    json!({
        "layer_id": layer.id,
        "name": layer.name,
        "role": format!("{:?}", layer.properties.role).to_ascii_lowercase(),
        "visible": layer.visible,
        "effective_visible": eff_visible,
        "element_count": elements_json.len(),
        "elements": elements_json,
    })
}

fn summarize_authored_draw(draw: &AuthoredDraw) -> Value {
    let item = &draw.item;
    json!({
        "element_id": item.element_id,
        "name": item.name,
        "layer_id": item.layer_id,
        "instance_path": item.instance_path,
        "transform": item.transform,
        "alpha": item.alpha,
        "blend_mode": format!("{:?}", draw.blend_mode).to_ascii_lowercase(),
        "mask_group": draw.mask_group,
        "kind": summarize_element_kind(&item.kind),
    })
}

fn summarize_element_kind(kind: &ElementKind) -> Value {
    match kind {
        ElementKind::Video { width, height } => json!({
            "type": "video",
            "width": width,
            "height": height,
        }),
        ElementKind::Bitmap {
            resource_id,
            width,
            height,
            smoothing,
        } => json!({
            "type": "bitmap",
            "resource_id": resource_id,
            "width": width,
            "height": height,
            "smoothing": smoothing,
        }),
        ElementKind::Rectangle {
            width,
            height,
            fill,
            stroke,
            stroke_width,
        } => json!({
            "type": "rectangle",
            "width": width,
            "height": height,
            "fill": color_hex(*fill),
            "stroke": color_hex(*stroke),
            "stroke_width": stroke_width,
        }),
        ElementKind::Ellipse {
            width,
            height,
            fill,
            stroke,
            stroke_width,
        } => json!({
            "type": "ellipse",
            "width": width,
            "height": height,
            "fill": color_hex(*fill),
            "stroke": color_hex(*stroke),
            "stroke_width": stroke_width,
        }),
        ElementKind::Path {
            commands,
            fill,
            stroke,
            stroke_width,
            ..
        }
        | ElementKind::PaintedPath {
            commands,
            fill,
            stroke,
            stroke_width,
            ..
        } => json!({
            "type": if matches!(kind, ElementKind::PaintedPath { .. }) { "gradient_path" } else { "path" },
            "command_count": commands.len(),
            "bounds": path_commands_bounds(commands),
            "fill": color_hex(*fill),
            "stroke": color_hex(*stroke),
            "stroke_width": stroke_width,
        }),
        ElementKind::Text {
            text,
            font_family,
            font_size,
            color,
        } => json!({
            "type": "text",
            "text": text,
            "font_family": font_family,
            "font_size": font_size,
            "color": color_hex(*color),
        }),
        ElementKind::TextBox {
            text,
            width,
            height,
            format,
            word_wrap,
            multiline,
        } => json!({
            "type": "text_box",
            "text": text,
            "width": width,
            "height": height,
            "font_family": format.font,
            "font_size": format.size,
            "word_wrap": word_wrap,
            "multiline": multiline,
        }),
        ElementKind::Instance {
            symbol_id,
            playback,
        } => json!({
            "type": "instance",
            "symbol_id": symbol_id,
            "playback": playback,
        }),
    }
}

fn path_commands_bounds(commands: &[PathCommand]) -> Option<[f64; 4]> {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let mut push = |x: f64, y: f64| {
        if x.is_finite() && y.is_finite() {
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    };
    for cmd in commands {
        match *cmd {
            PathCommand::MoveTo { x, y } | PathCommand::LineTo { x, y } => push(x, y),
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                push(cx, cy);
                push(x, y);
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                push(cx1, cy1);
                push(cx2, cy2);
                push(x, y);
            }
            PathCommand::Close => {}
        }
    }
    min_x.is_finite().then_some([min_x, min_y, max_x, max_y])
}
