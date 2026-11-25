//! Strict JSON batch automation runner with one rollback boundary for the candidate project and
//! every explicit export/save/JSFL filesystem effect (`neoflash-editor --headless batch`).
use super::effects::HeadlessEffectTransaction;
use super::import_export::{
    BatchImportSpec, ExportSpec, export_project_with_spec, import_project_with_spec,
    parse_library_folder_flag, plan_export_effects,
};
use super::jsfl_fs::{
    JsflExecRequest, execute_jsfl_on_project, jsfl_value_to_json, read_jsfl_source_file,
};
use super::{
    CliError, EvalOptions, InspectOptions, OutputFormat, build_inspect_value, build_new_project,
    compile_loaded_project_scripts, evaluate_loaded_project_frame, load_project_valid, parse_color,
    parse_easing, parse_frame_range_str, parse_language, parse_object_event, parse_symbol_kind,
    parse_tween_kind, resolve_layer_index, resolve_layer_index_mut, resolve_symbol,
    resolve_symbol_mut, save_project_atomic, serialize_json_value, take_flag_value,
    upgrade_project_format_versions, validate_loaded_project, write_json_error,
};
use neoflash_core::{
    AuthoringCommandStep, Color, Element, ElementKind, FillRule, FrameScript, InstancePlayback,
    Keyframe, Layer, LayerProperties, LayerRole, ObjectScript, PathCommand, Project, Script,
    ScriptLanguage, SourceUnit, Symbol, TextFormat, Timeline, Transform,
    apply_command_steps_to_elements, new_id,
};
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

fn parse_batch_ops(raw_ops: &str) -> Result<Vec<BatchOp>, CliError> {
    let root: Value = serde_json::from_str(raw_ops)
        .map_err(|e| CliError::usage("invalid_batch_json", format!("Invalid batch JSON: {e}")))?;
    let items = match root {
        Value::Array(items) => items,
        Value::Object(mut map) => {
            let has_ops = map.contains_key("ops");
            let has_steps = map.contains_key("steps");
            if has_ops && has_steps {
                return Err(CliError::usage(
                    "invalid_batch_json",
                    "Invalid batch JSON: specify exactly one of 'ops' or 'steps', not both",
                ));
            }
            let key = if has_ops {
                "ops"
            } else if has_steps {
                "steps"
            } else {
                return Err(CliError::usage(
                    "invalid_batch_json",
                    "Invalid batch JSON: expected an array or object with an 'ops' or 'steps' array",
                ));
            };
            let list = map.remove(key).ok_or_else(|| {
                CliError::usage(
                    "invalid_batch_json",
                    "Invalid batch JSON: expected an array or object with an 'ops' or 'steps' array",
                )
            })?;
            if !map.is_empty() {
                let fields = map.keys().cloned().collect::<Vec<_>>().join(", ");
                return Err(CliError::usage(
                    "invalid_batch_json",
                    format!("Invalid batch JSON: unknown wrapper field(s): {fields}"),
                ));
            }
            match list {
                Value::Array(items) => items,
                _ => {
                    return Err(CliError::usage(
                        "invalid_batch_json",
                        "Invalid batch JSON: 'ops'/'steps' must be an array",
                    ));
                }
            }
        }
        _ => {
            return Err(CliError::usage(
                "invalid_batch_json",
                "Invalid batch JSON: expected a JSON array or object",
            ));
        }
    };

    items
        .into_iter()
        .enumerate()
        .map(|(idx, item)| {
            serde_json::from_value::<BatchOp>(item).map_err(|e| {
                CliError::usage(
                    "invalid_batch_json",
                    format!("Invalid batch JSON at step {idx}: {e}"),
                )
            })
        })
        .collect()
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
enum FrameRangeParam {
    Text(String),
    Object { start: u32, end: u32 },
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct TransformSpec {
    x: Option<f64>,
    y: Option<f64>,
    scale_x: Option<f64>,
    scale_y: Option<f64>,
    rotation: Option<f64>,
    skew: Option<f64>,
    alpha: Option<f64>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum StrictPathCommand {
    MoveTo {
        x: f64,
        y: f64,
    },
    LineTo {
        x: f64,
        y: f64,
    },
    QuadraticTo {
        cx: f64,
        cy: f64,
        x: f64,
        y: f64,
    },
    CubicTo {
        cx1: f64,
        cy1: f64,
        cx2: f64,
        cy2: f64,
        x: f64,
        y: f64,
    },
    Close,
}

impl From<StrictPathCommand> for PathCommand {
    fn from(command: StrictPathCommand) -> Self {
        match command {
            StrictPathCommand::MoveTo { x, y } => Self::MoveTo { x, y },
            StrictPathCommand::LineTo { x, y } => Self::LineTo { x, y },
            StrictPathCommand::QuadraticTo { cx, cy, x, y } => Self::QuadraticTo { cx, cy, x, y },
            StrictPathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => Self::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            },
            StrictPathCommand::Close => Self::Close,
        }
    }
}

fn deserialize_optional_path_commands<'de, D>(
    deserializer: D,
) -> Result<Option<Vec<PathCommand>>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<Vec<StrictPathCommand>>::deserialize(deserializer).map(|commands| {
        commands.map(|commands| commands.into_iter().map(PathCommand::from).collect())
    })
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum StrictAuthoringCommandStep {
    Translate {
        dx: f64,
        dy: f64,
    },
    Scale {
        sx: f64,
        sy: f64,
    },
    Rotate {
        degrees: f64,
    },
    Skew {
        degrees: f64,
    },
    SetAlpha {
        alpha: f64,
    },
    SetFillColor {
        color: Color,
    },
    SetStrokeColor {
        color: Color,
    },
    SetStrokeWidth {
        width: f64,
    },
    SetVisible {
        visible: bool,
    },
    SetBlendMode {
        blend_mode: neoflash_core::BlendMode,
    },
    RenameSelection {
        name: String,
    },
    DuplicateSelection {
        offset_x: f64,
        offset_y: f64,
    },
    DeleteSelection,
    SwapSymbol {
        target_symbol_id: String,
    },
}

impl From<StrictAuthoringCommandStep> for AuthoringCommandStep {
    fn from(step: StrictAuthoringCommandStep) -> Self {
        match step {
            StrictAuthoringCommandStep::Translate { dx, dy } => Self::Translate { dx, dy },
            StrictAuthoringCommandStep::Scale { sx, sy } => Self::Scale { sx, sy },
            StrictAuthoringCommandStep::Rotate { degrees } => Self::Rotate { degrees },
            StrictAuthoringCommandStep::Skew { degrees } => Self::Skew { degrees },
            StrictAuthoringCommandStep::SetAlpha { alpha } => Self::SetAlpha { alpha },
            StrictAuthoringCommandStep::SetFillColor { color } => Self::SetFillColor { color },
            StrictAuthoringCommandStep::SetStrokeColor { color } => Self::SetStrokeColor { color },
            StrictAuthoringCommandStep::SetStrokeWidth { width } => Self::SetStrokeWidth { width },
            StrictAuthoringCommandStep::SetVisible { visible } => Self::SetVisible { visible },
            StrictAuthoringCommandStep::SetBlendMode { blend_mode } => {
                Self::SetBlendMode { blend_mode }
            }
            StrictAuthoringCommandStep::RenameSelection { name } => Self::RenameSelection { name },
            StrictAuthoringCommandStep::DuplicateSelection { offset_x, offset_y } => {
                Self::DuplicateSelection { offset_x, offset_y }
            }
            StrictAuthoringCommandStep::DeleteSelection => Self::DeleteSelection,
            StrictAuthoringCommandStep::SwapSymbol { target_symbol_id } => {
                Self::SwapSymbol { target_symbol_id }
            }
        }
    }
}

fn deserialize_authoring_command_steps<'de, D>(
    deserializer: D,
) -> Result<Vec<AuthoringCommandStep>, D::Error>
where
    D: Deserializer<'de>,
{
    Vec::<StrictAuthoringCommandStep>::deserialize(deserializer)
        .map(|steps| steps.into_iter().map(AuthoringCommandStep::from).collect())
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AddElementSpec {
    symbol: Option<String>,
    layer: Option<String>,
    #[serde(default)]
    frame: u32,
    id: Option<String>,
    #[serde(default)]
    name: String,
    kind: String,
    transform: Option<TransformSpec>,
    x: Option<f64>,
    y: Option<f64>,
    scale_x: Option<f64>,
    scale_y: Option<f64>,
    rotation: Option<f64>,
    skew: Option<f64>,
    alpha: Option<f64>,
    visible: Option<bool>,
    width: Option<f64>,
    height: Option<f64>,
    fill: Option<String>,
    stroke: Option<String>,
    stroke_width: Option<f64>,
    text: Option<String>,
    font_family: Option<String>,
    font_size: Option<f64>,
    color: Option<String>,
    #[serde(default)]
    word_wrap: bool,
    #[serde(default)]
    multiline: bool,
    #[serde(default, deserialize_with = "deserialize_optional_path_commands")]
    commands: Option<Vec<PathCommand>>,
    d: Option<String>,
    fill_rule: Option<String>,
    #[serde(alias = "target")]
    symbol_id: Option<String>,
    playback: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum BatchOp {
    New {
        name: Option<String>,
        width: Option<u32>,
        height: Option<u32>,
        fps: Option<u32>,
        bg: Option<String>,
        language: Option<String>,
        script_language: Option<String>,
        also_compile_compatible_scripts: Option<bool>,
        #[serde(default)]
        demo: bool,
    },
    Open {
        path: PathBuf,
    },
    SetProject {
        name: Option<String>,
        width: Option<u32>,
        height: Option<u32>,
        fps: Option<u32>,
        bg: Option<String>,
        script_language: Option<String>,
        also_compile_compatible_scripts: Option<bool>,
    },
    AddSymbol {
        id: Option<String>,
        name: String,
        kind: Option<String>,
        frame_count: Option<u32>,
        linkage_id: Option<String>,
        class_name: Option<String>,
        #[serde(default)]
        folder: String,
        #[serde(default)]
        as_scene: bool,
    },
    AddLayer {
        symbol: Option<String>,
        id: Option<String>,
        name: String,
        role: Option<String>,
        visible: Option<bool>,
        locked: Option<bool>,
        mask_layer: Option<String>,
        motion_guide: Option<String>,
    },
    AddKeyframe {
        symbol: Option<String>,
        layer: String,
        frame: u32,
        #[serde(default)]
        label: String,
        tween: Option<String>,
        tween_kind: Option<String>,
        #[serde(default)]
        copy_previous: bool,
        #[serde(default)]
        clear_elements: bool,
    },
    AddElement(Box<AddElementSpec>),
    SetScript {
        symbol: Option<String>,
        layer: Option<String>,
        #[serde(default)]
        frame: u32,
        element: Option<String>,
        event: Option<String>,
        path: Option<String>,
        id: Option<String>,
        language: Option<String>,
        #[serde(alias = "code")]
        source: String,
        #[serde(default)]
        remove_empty: bool,
    },
    RemoveElement {
        symbol: Option<String>,
        layer: Option<String>,
        #[serde(default)]
        frame: u32,
        element: String,
    },
    ApplyCommandSteps {
        symbol: Option<String>,
        layer: Option<String>,
        #[serde(default)]
        frame: u32,
        elements: Vec<String>,
        #[serde(deserialize_with = "deserialize_authoring_command_steps")]
        steps: Vec<AuthoringCommandStep>,
    },
    Jsfl {
        code: Option<String>,
        source: Option<String>,
        script_file: Option<PathBuf>,
        symbol: Option<String>,
        frame: Option<u32>,
        flfile_root: Option<PathBuf>,
        allow_read: Option<PathBuf>,
        allow_write: Option<PathBuf>,
        #[serde(default)]
        allow_delete: bool,
    },
    Validate {
        #[serde(default)]
        strict: bool,
    },
    Compile {
        #[serde(default)]
        strict: bool,
    },
    Eval {
        scene: Option<String>,
        symbol: Option<String>,
        frame: Option<u32>,
        frames: Option<FrameRangeParam>,
        #[serde(default)]
        scripts: bool,
    },
    Inspect {
        symbol: Option<String>,
        frame: Option<u32>,
        #[serde(default)]
        no_elements: bool,
        #[serde(default)]
        no_scripts: bool,
        #[serde(default)]
        no_resources: bool,
    },
    Import(Box<BatchImportSpec>),
    Export(Box<ExportSpec>),
    Save {
        path: Option<PathBuf>,
    },
}

struct BatchContext<'a> {
    /// Directory against which external project media is resolved for exporters such as AEP.
    project_base_dir: Option<PathBuf>,
    effects: &'a HeadlessEffectTransaction,
}

fn project_base_dir(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::canonicalize(parent).unwrap_or_else(|_| {
        if parent.is_absolute() {
            parent.to_path_buf()
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(parent)
        }
    })
}

pub(crate) fn cmd_batch<R: Read, W1: Write, W2: Write>(
    args: &[String],
    format: OutputFormat,
    pretty: bool,
    stdin: &mut R,
    stdout: &mut W1,
    stderr: &mut W2,
) -> i32 {
    match run_cmd_batch(args, format, pretty, stdin, stdout) {
        Ok(()) => 0,
        Err(err) => {
            write_json_error(&err, format, pretty, stdout, stderr);
            err.exit_code
        }
    }
}

fn run_cmd_batch<R: Read, W1: Write>(
    args: &[String],
    format: OutputFormat,
    pretty: bool,
    stdin: &mut R,
    stdout: &mut W1,
) -> Result<(), CliError> {
    let mut ops_file: Option<PathBuf> = None;
    let mut use_stdin = false;
    let mut project_path: Option<PathBuf> = None;
    let mut out_path: Option<PathBuf> = None;
    let mut in_place = false;

    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--file" | "-f" => {
                ops_file = Some(PathBuf::from(take_flag_value(args, &mut i, "--file")?));
            }
            "--stdin" => use_stdin = true,
            "--project" => {
                project_path = Some(PathBuf::from(take_flag_value(args, &mut i, "--project")?));
            }
            "--out" | "--output" | "-o" => {
                out_path = Some(PathBuf::from(take_flag_value(args, &mut i, "--out")?));
            }
            "--in-place" => in_place = true,
            other => {
                return Err(CliError::usage(
                    "unknown_flag",
                    format!("Unknown flag '{other}' for 'batch'"),
                ));
            }
        }
        i += 1;
    }

    if ops_file.is_some() == use_stdin {
        return Err(CliError::usage(
            "invalid_arguments",
            "'batch' requires exactly one of --file <ops.json> or --stdin",
        ));
    }
    if out_path.is_some() && in_place {
        return Err(CliError::usage(
            "conflicting_flags",
            "--out and --in-place are mutually exclusive",
        ));
    }
    if in_place && project_path.is_none() {
        return Err(CliError::usage(
            "missing_flag",
            "--in-place requires --project <in.nflash>",
        ));
    }

    let raw_ops = if let Some(ref path) = ops_file {
        fs::read_to_string(path).map_err(|e| {
            CliError::runtime(
                "io_error",
                format!("Failed reading batch file '{}': {e}", path.display()),
            )
        })?
    } else {
        let mut buf = String::new();
        stdin
            .read_to_string(&mut buf)
            .map_err(|e| CliError::runtime("io_error", format!("Failed reading stdin: {e}")))?;
        buf
    };

    let ops = parse_batch_ops(&raw_ops)?;

    let mut project = match project_path {
        Some(ref path) => load_project_valid(path)?,
        None => Project::blank(),
    };
    let final_save_target = if in_place {
        project_path.clone()
    } else {
        out_path.clone()
    };

    let effects = HeadlessEffectTransaction::default();
    let mut context = BatchContext {
        project_base_dir: project_path.as_deref().map(project_base_dir),
        effects: &effects,
    };
    for op in &ops {
        op.validate_effects(final_save_target.as_deref())?;
    }

    let result = run_batch_transaction(
        ops,
        &mut project,
        final_save_target.as_deref(),
        format,
        pretty,
        stdout,
        &mut context,
    );
    match result {
        Ok(()) => {
            effects.finalize_after_response();
            Ok(())
        }
        Err(mut error) => {
            if let Err(rollback_error) = effects.rollback() {
                error.message = format!(
                    "{}; filesystem rollback also failed: {}",
                    error.message, rollback_error.message
                );
            }
            Err(error)
        }
    }
}

fn run_batch_transaction<W: Write>(
    ops: Vec<BatchOp>,
    project: &mut Project,
    final_save_target: Option<&Path>,
    format: OutputFormat,
    pretty: bool,
    stdout: &mut W,
    context: &mut BatchContext<'_>,
) -> Result<(), CliError> {
    let mut step_results = Vec::with_capacity(ops.len());
    for (idx, op) in ops.into_iter().enumerate() {
        let op_name = op.name_str();
        let step_out =
            execute_batch_op(project, op, final_save_target, context).map_err(|err| {
                let mut details = err.details.unwrap_or(Value::Null);
                let step_info = json!({
                    "step_index": idx,
                    "op": op_name,
                    "cause": details.take(),
                });
                CliError {
                    exit_code: err.exit_code,
                    code: err.code,
                    message: format!("Batch step {idx} ({op_name}) failed: {}", err.message),
                    details: Some(step_info),
                }
            })?;
        step_results.push(json!({
            "step": idx,
            "op": op_name,
            "result": step_out,
        }));
    }

    upgrade_project_format_versions(project);
    project.validate().map_err(|e| {
        CliError::runtime(
            "validation_failed",
            format!("Final batch project failed validation: {e}"),
        )
    })?;

    let mut saved_path: Option<String> = None;
    if let Some(dest) = final_save_target {
        context.effects.plan_path(dest)?;
        context.effects.activate()?;
        save_project_atomic(project, dest)?;
        saved_path = Some(dest.display().to_string());
    }

    match format {
        OutputFormat::Json => {
            let val = json!({
                "ok": true,
                "command": "batch",
                "steps_executed": step_results.len(),
                "saved_path": saved_path,
                "project": {
                    "name": project.name,
                    "width": project.width,
                    "height": project.height,
                    "fps": project.fps,
                    "format_version": project.format_version,
                    "symbols": project.symbols.len(),
                },
                "results": step_results,
            });
            let text = serialize_json_value(&val, pretty)?;
            writeln!(stdout, "{text}").map_err(|e| {
                CliError::runtime("io_error", format!("Failed writing stdout: {e}"))
            })?;
        }
        OutputFormat::Text => {
            writeln!(
                stdout,
                "Batch completed {} step(s){}",
                step_results.len(),
                saved_path
                    .as_deref()
                    .map_or_else(String::new, |p| format!(", saved to {p}"))
            )
            .map_err(|e| CliError::runtime("io_error", format!("Failed writing stdout: {e}")))?;
        }
    }
    Ok(())
}

impl BatchOp {
    fn validate_effects(&self, default_save_target: Option<&Path>) -> Result<(), CliError> {
        if let Self::Save { path } = self {
            path.as_deref().or(default_save_target).ok_or_else(|| {
                CliError::usage(
                    "missing_field",
                    "save step requires 'path' when CLI --out/--in-place is omitted",
                )
            })?;
        }
        Ok(())
    }

    fn name_str(&self) -> &'static str {
        match self {
            Self::New { .. } => "new",
            Self::Open { .. } => "open",
            Self::SetProject { .. } => "set_project",
            Self::AddSymbol { .. } => "add_symbol",
            Self::AddLayer { .. } => "add_layer",
            Self::AddKeyframe { .. } => "add_keyframe",
            Self::AddElement(_) => "add_element",
            Self::SetScript { .. } => "set_script",
            Self::RemoveElement { .. } => "remove_element",
            Self::ApplyCommandSteps { .. } => "apply_command_steps",
            Self::Jsfl { .. } => "jsfl",
            Self::Validate { .. } => "validate",
            Self::Compile { .. } => "compile",
            Self::Eval { .. } => "eval",
            Self::Inspect { .. } => "inspect",
            Self::Import(_) => "import",
            Self::Export(_) => "export",
            Self::Save { .. } => "save",
        }
    }
}

fn execute_batch_op(
    project: &mut Project,
    op: BatchOp,
    default_save_target: Option<&Path>,
    context: &mut BatchContext<'_>,
) -> Result<Value, CliError> {
    match op {
        BatchOp::New {
            name,
            width,
            height,
            fps,
            bg,
            language,
            script_language,
            also_compile_compatible_scripts,
            demo,
        } => {
            let lang = language.as_deref().map(parse_language).transpose()?;
            let explicit_script_lang =
                script_language.as_deref().map(parse_language).transpose()?;
            let bg_color = bg.as_deref().map(parse_color).transpose()?;
            *project = build_new_project(name, width, height, fps, bg_color, lang, demo)?;
            context.project_base_dir = None;
            if let Some(sl) = explicit_script_lang.or(lang) {
                project.script_language = Some(sl);
            }
            if let Some(compat) = also_compile_compatible_scripts {
                project.also_compile_compatible_scripts = compat;
            }
            if project.script_language.is_some() || also_compile_compatible_scripts.is_some() {
                upgrade_project_format_versions(project);
                project
                    .validate()
                    .map_err(|e| CliError::runtime("validation_failed", e.to_string()))?;
            }
            Ok(json!({
                "name": project.name,
                "width": project.width,
                "height": project.height,
                "fps": project.fps,
                "root_symbol": project.root,
                "script_language": project.script_language.map(|l| l.short_slug()),
                "also_compile_compatible_scripts": project.also_compile_compatible_scripts,
            }))
        }
        BatchOp::Open { path } => {
            *project = load_project_valid(&path)?;
            context.project_base_dir = Some(project_base_dir(&path));
            Ok(json!({
                "path": path.display().to_string(),
                "name": project.name,
                "symbols": project.symbols.len(),
            }))
        }
        BatchOp::SetProject {
            name,
            width,
            height,
            fps,
            bg,
            script_language,
            also_compile_compatible_scripts,
        } => {
            if let Some(n) = name {
                project.name = n;
            }
            if let Some(w) = width {
                if w == 0 {
                    return Err(CliError::usage("invalid_argument", "width must be > 0"));
                }
                project.width = w;
            }
            if let Some(h) = height {
                if h == 0 {
                    return Err(CliError::usage("invalid_argument", "height must be > 0"));
                }
                project.height = h;
            }
            if let Some(f) = fps {
                if f == 0 {
                    return Err(CliError::usage("invalid_argument", "fps must be > 0"));
                }
                project.fps = f;
            }
            if let Some(ref bg_str) = bg {
                project.background = parse_color(bg_str)?;
            }
            if let Some(ref sl_str) = script_language {
                project.script_language = Some(parse_language(sl_str)?);
            }
            if let Some(compat) = also_compile_compatible_scripts {
                project.also_compile_compatible_scripts = compat;
            }
            if script_language.is_some() || also_compile_compatible_scripts.is_some() {
                upgrade_project_format_versions(project);
            }
            Ok(json!({
                "name": project.name,
                "width": project.width,
                "height": project.height,
                "fps": project.fps,
                "script_language": project.script_language.map(|l| l.short_slug()),
                "also_compile_compatible_scripts": project.also_compile_compatible_scripts,
            }))
        }
        BatchOp::AddSymbol {
            id,
            name,
            kind,
            frame_count,
            linkage_id,
            class_name,
            folder,
            as_scene,
        } => {
            let normalized_folder = parse_library_folder_flag(&folder)?;
            if as_scene && !normalized_folder.is_empty() {
                return Err(CliError::usage(
                    "invalid_argument",
                    "Scene symbols (as_scene: true) cannot be placed in a library folder",
                ));
            }
            let sym_id = id.unwrap_or_else(|| new_id("symbol"));
            if project.symbol(&sym_id).is_some() {
                return Err(CliError::runtime(
                    "duplicate_id",
                    format!("Symbol ID '{sym_id}' already exists"),
                ));
            }
            let sym_kind = kind
                .as_deref()
                .map(parse_symbol_kind)
                .transpose()?
                .unwrap_or(neoflash_core::SymbolKind::MovieClip);
            let fc = frame_count.unwrap_or(1).max(1);
            if linkage_id.is_some() || class_name.is_some() {
                project.enable_program();
            }
            if !normalized_folder.is_empty() {
                let _ = project.add_library_folder(&normalized_folder);
            }
            project.symbols.push(Symbol {
                id: sym_id.clone(),
                name: name.clone(),
                folder: normalized_folder,
                kind: sym_kind,
                timeline: Timeline {
                    frame_count: fc,
                    layers: vec![Layer {
                        id: new_id("layer"),
                        name: "Layer 1".to_owned(),
                        visible: true,
                        locked: false,
                        keyframes: vec![Keyframe::default()],
                        properties: LayerProperties::default(),
                    }],
                    scripts: Vec::new(),
                    guides: Vec::new(),
                },
                class_name,
                linkage_id,
            });
            if as_scene {
                if project.scenes.is_empty() {
                    project.scenes.push(project.root.clone());
                }
                project.scenes.push(sym_id.clone());
            }
            upgrade_project_format_versions(project);
            Ok(json!({
                "symbol_id": sym_id,
                "name": name,
                "frame_count": fc,
                "as_scene": as_scene,
            }))
        }
        BatchOp::AddLayer {
            symbol,
            id,
            name,
            role,
            visible,
            locked,
            mask_layer,
            motion_guide,
        } => {
            let sym_id = match symbol.as_deref() {
                Some(sel) => resolve_symbol(project, sel)?.id.clone(),
                None => project.root.clone(),
            };
            let sym = project
                .symbol_mut(&sym_id)
                .ok_or_else(|| CliError::runtime("symbol_not_found", "Root symbol not found"))?;
            let layer_id = id.unwrap_or_else(|| new_id("layer"));
            if sym.timeline.layers.iter().any(|l| l.id == layer_id) {
                return Err(CliError::runtime(
                    "duplicate_id",
                    format!("Layer ID '{layer_id}' already exists"),
                ));
            }
            let mask_id = mask_layer
                .as_deref()
                .map(|sel| {
                    resolve_layer_index(&sym.timeline, sel)
                        .map(|idx| sym.timeline.layers[idx].id.clone())
                })
                .transpose()?;
            let guide_id = motion_guide
                .as_deref()
                .map(|sel| {
                    resolve_layer_index(&sym.timeline, sel)
                        .map(|idx| sym.timeline.layers[idx].id.clone())
                })
                .transpose()?;
            let parsed_role = match role.as_deref() {
                Some(raw) => match raw.trim().to_ascii_lowercase().as_str() {
                    "normal" => LayerRole::Normal,
                    "mask" => LayerRole::Mask,
                    "guide" => LayerRole::Guide,
                    "folder" => LayerRole::Folder,
                    other => {
                        return Err(CliError::usage(
                            "invalid_argument",
                            format!("Invalid layer role '{other}'"),
                        ));
                    }
                },
                None => LayerRole::Normal,
            };
            let keyframes = if parsed_role == LayerRole::Folder {
                Vec::new()
            } else {
                vec![Keyframe::default()]
            };
            let before_tl = sym.timeline.clone();
            sym.timeline.layers.push(Layer {
                id: layer_id.clone(),
                name: name.clone(),
                visible: visible.unwrap_or(true),
                locked: locked.unwrap_or(false),
                keyframes,
                properties: LayerProperties {
                    role: parsed_role,
                    mask: mask_id,
                    motion_guide: guide_id,
                    ..LayerProperties::default()
                },
            });
            sym.timeline.pin_displaced_frame_script_layers(&before_tl);
            upgrade_project_format_versions(project);
            Ok(json!({ "symbol_id": sym_id, "layer_id": layer_id, "name": name }))
        }
        BatchOp::AddKeyframe {
            symbol,
            layer,
            frame,
            label,
            tween,
            tween_kind,
            copy_previous,
            clear_elements,
        } => {
            let sym_id = match symbol.as_deref() {
                Some(sel) => resolve_symbol(project, sel)?.id.clone(),
                None => project.root.clone(),
            };
            let sym = resolve_symbol_mut(project, &sym_id)?;
            let before_tl = sym.timeline.clone();
            if frame >= sym.timeline.frame_count {
                sym.timeline.frame_count = frame + 1;
            }
            let layer_idx = resolve_layer_index_mut(&mut sym.timeline, &layer)?;
            let parsed_tween = tween.as_deref().map(parse_easing).transpose()?;
            let parsed_kind = tween_kind.as_deref().map(parse_tween_kind).transpose()?;

            let layer_ref = &mut sym.timeline.layers[layer_idx];
            if let Some(existing) = layer_ref.keyframes.iter_mut().find(|k| k.frame == frame) {
                if clear_elements {
                    existing.elements.clear();
                }
                if !label.is_empty() {
                    existing.label = label;
                }
                if parsed_tween.is_some() {
                    existing.tween = parsed_tween;
                }
                if let Some(tk) = parsed_kind {
                    existing.tween_kind = tk;
                }
            } else {
                let elements = if copy_previous {
                    layer_ref
                        .keyframes
                        .iter()
                        .rfind(|k| k.frame < frame)
                        .map(|k| k.elements.clone())
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
                let mut kf = Keyframe {
                    frame,
                    elements,
                    tween: parsed_tween,
                    label,
                    ..Keyframe::default()
                };
                if let Some(tk) = parsed_kind {
                    kf.tween_kind = tk;
                }
                layer_ref.keyframes.push(kf);
                layer_ref.keyframes.sort_by_key(|k| k.frame);
            }
            sym.timeline.pin_displaced_frame_script_layers(&before_tl);
            upgrade_project_format_versions(project);
            Ok(json!({ "symbol_id": sym_id, "frame": frame }))
        }
        BatchOp::AddElement(spec) => {
            let AddElementSpec {
                symbol,
                layer,
                frame,
                id,
                name,
                kind,
                transform,
                x,
                y,
                scale_x,
                scale_y,
                rotation,
                skew,
                alpha,
                visible,
                width,
                height,
                fill,
                stroke,
                stroke_width,
                text,
                font_family,
                font_size,
                color,
                word_wrap,
                multiline,
                commands,
                d,
                fill_rule,
                symbol_id: target_sym_sel,
                playback,
            } = *spec;
            let owner_sym_id = match symbol.as_deref() {
                Some(sel) => resolve_symbol(project, sel)?.id.clone(),
                None => project.root.clone(),
            };
            let resolved_target_symbol_id = if let Some(sel) = target_sym_sel.as_deref() {
                let sym = resolve_symbol(project, sel)?;
                if project.is_scene(&sym.id) {
                    return Err(CliError::runtime(
                        "invalid_instance_target",
                        format!("Scene symbol '{}' cannot be instantiated", sym.name),
                    ));
                }
                Some(sym.id.clone())
            } else {
                None
            };

            let el_id = id.unwrap_or_else(|| new_id("el"));
            let tf = Transform {
                x: x.or_else(|| transform.as_ref().and_then(|t| t.x))
                    .unwrap_or(0.0),
                y: y.or_else(|| transform.as_ref().and_then(|t| t.y))
                    .unwrap_or(0.0),
                scale_x: scale_x
                    .or_else(|| transform.as_ref().and_then(|t| t.scale_x))
                    .unwrap_or(1.0),
                scale_y: scale_y
                    .or_else(|| transform.as_ref().and_then(|t| t.scale_y))
                    .unwrap_or(1.0),
                rotation: rotation
                    .or_else(|| transform.as_ref().and_then(|t| t.rotation))
                    .unwrap_or(0.0),
                skew: skew
                    .or_else(|| transform.as_ref().and_then(|t| t.skew))
                    .unwrap_or(0.0),
                alpha: alpha
                    .or_else(|| transform.as_ref().and_then(|t| t.alpha))
                    .unwrap_or(1.0),
                ..Transform::default()
            };

            let fill_color: Color = fill
                .as_deref()
                .map(parse_color)
                .transpose()?
                .unwrap_or([59, 130, 246, 255]);
            let stroke_color: Color = stroke
                .as_deref()
                .map(parse_color)
                .transpose()?
                .unwrap_or([0, 0, 0, 0]);
            let sw = stroke_width.unwrap_or(0.0);

            let el_kind = match kind.trim().to_ascii_lowercase().as_str() {
                "rectangle" | "rect" => ElementKind::Rectangle {
                    width: width.unwrap_or(100.0),
                    height: height.unwrap_or(80.0),
                    fill: fill_color,
                    stroke: stroke_color,
                    stroke_width: sw,
                },
                "oval" | "ellipse" | "circle" => ElementKind::Ellipse {
                    width: width.unwrap_or(80.0),
                    height: height.unwrap_or(80.0),
                    fill: fill_color,
                    stroke: stroke_color,
                    stroke_width: sw,
                },
                "text" => ElementKind::Text {
                    text: text.unwrap_or_default(),
                    font_family: font_family.unwrap_or_else(|| "Inter".to_owned()),
                    font_size: font_size.unwrap_or(24.0),
                    color: color
                        .as_deref()
                        .map(parse_color)
                        .transpose()?
                        .unwrap_or([0, 0, 0, 255]),
                },
                "text_box" | "textbox" => {
                    let text_str = text.unwrap_or_default();
                    let mut fmt = TextFormat::default();
                    if let Some(ff) = font_family {
                        fmt.font = ff;
                    }
                    if let Some(sz) = font_size {
                        fmt.size = sz;
                    }
                    if let Some(ref c_str) = color {
                        let [r, g, b, _] = parse_color(c_str)?;
                        fmt.color = u32::from_be_bytes([0, r, g, b]);
                    }
                    ElementKind::TextBox {
                        text: text_str,
                        width: width.unwrap_or(200.0),
                        height: height.unwrap_or(60.0),
                        format: fmt,
                        word_wrap,
                        multiline,
                    }
                }
                "path" => {
                    let path_cmds = match (commands, d) {
                        (Some(cmds), _) => cmds,
                        (None, Some(svg_d)) => {
                            let parsed =
                                neoflash_import::vector_fidelity::parse_svg_path_data(&svg_d);
                            if parsed.is_empty() {
                                return Err(CliError::usage(
                                    "invalid_argument",
                                    format!("SVG path string 'd' produced no commands: '{svg_d}'"),
                                ));
                            }
                            parsed
                        }
                        (None, None) => {
                            return Err(CliError::usage(
                                "missing_field",
                                "add_element kind 'path' requires 'commands' or SVG 'd'",
                            ));
                        }
                    };
                    let rule = match fill_rule.as_deref() {
                        Some("even_odd") | Some("evenodd") => FillRule::EvenOdd,
                        _ => FillRule::NonZero,
                    };
                    ElementKind::Path {
                        commands: path_cmds,
                        fill_rule: rule,
                        fill: fill_color,
                        stroke: stroke_color,
                        stroke_width: sw,
                    }
                }
                "instance" => {
                    let target_id = resolved_target_symbol_id.ok_or_else(|| {
                        CliError::usage(
                            "missing_field",
                            "add_element kind 'instance' requires 'symbol_id'",
                        )
                    })?;
                    let pb = parse_instance_playback(playback.as_deref())?;
                    ElementKind::Instance {
                        symbol_id: target_id,
                        playback: pb,
                    }
                }
                other => {
                    return Err(CliError::usage(
                        "invalid_argument",
                        format!("Unsupported element kind '{other}'"),
                    ));
                }
            };

            let sym = resolve_symbol_mut(project, &owner_sym_id)?;
            if frame >= sym.timeline.frame_count {
                sym.timeline.frame_count = frame + 1;
            }
            let layer_idx = match layer.as_deref() {
                Some(sel) => resolve_layer_index_mut(&mut sym.timeline, sel)?,
                None => sym
                    .timeline
                    .layers
                    .iter()
                    .position(|l| l.properties.role != LayerRole::Folder)
                    .ok_or_else(|| {
                        CliError::runtime("layer_not_found", "Symbol has no editable layer")
                    })?,
            };
            let layer_mut = &mut sym.timeline.layers[layer_idx];
            if layer_mut.keyframes.iter().any(|k| k.frame == frame) {
                let kf = layer_mut
                    .keyframes
                    .iter_mut()
                    .find(|k| k.frame == frame)
                    .expect("checked");
                kf.elements.push(Element {
                    id: el_id.clone(),
                    name: name.clone(),
                    transform: tf,
                    kind: el_kind,
                    visible: visible.unwrap_or(true),
                    ..Element::default()
                });
            } else {
                layer_mut.keyframes.push(Keyframe {
                    frame,
                    elements: vec![Element {
                        id: el_id.clone(),
                        name: name.clone(),
                        transform: tf,
                        kind: el_kind,
                        visible: visible.unwrap_or(true),
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                });
                layer_mut.keyframes.sort_by_key(|k| k.frame);
            }
            let layer_id = layer_mut.id.clone();
            upgrade_project_format_versions(project);
            Ok(json!({
                "symbol_id": owner_sym_id,
                "layer_id": layer_id,
                "frame": frame,
                "element_id": el_id,
            }))
        }
        BatchOp::SetScript {
            symbol,
            layer,
            frame,
            element,
            event,
            path,
            id,
            language,
            source,
            remove_empty,
        } => {
            let explicit_lang = language.as_deref().map(parse_language).transpose()?;
            let project_default_lang = project
                .symbol(&project.root)
                .and_then(|s| s.timeline.scripts.first())
                .map(|fs| fs.script.language)
                .unwrap_or(ScriptLanguage::ActionScript3);

            if let Some(src_path) = path {
                project.enable_program();
                if let Some(existing) = project.sources.iter_mut().find(|u| u.path == src_path) {
                    let resolved = explicit_lang.unwrap_or(existing.language);
                    existing.language = resolved;
                    existing.source = source;
                    let uid = existing.id.clone();
                    upgrade_project_format_versions(project);
                    return Ok(json!({ "target": "source", "source_id": uid, "path": src_path }));
                }
                let resolved = explicit_lang.unwrap_or(project_default_lang);
                let uid = id.unwrap_or_else(|| new_id("src"));
                project.sources.push(SourceUnit {
                    id: uid.clone(),
                    path: src_path.clone(),
                    language: resolved,
                    source,
                });
                upgrade_project_format_versions(project);
                return Ok(json!({ "target": "source", "source_id": uid, "path": src_path }));
            }

            let sym_id = match symbol.as_deref() {
                Some(sel) => resolve_symbol(project, sel)?.id.clone(),
                None => project.root.clone(),
            };
            let sym = resolve_symbol_mut(project, &sym_id)?;
            let sym_default_lang = sym
                .timeline
                .scripts
                .first()
                .map(|s| s.script.language)
                .unwrap_or(project_default_lang);

            if let Some(el_sel) = element {
                if frame >= sym.timeline.frame_count {
                    return Err(CliError::runtime(
                        "frame_out_of_range",
                        format!(
                            "Frame {frame} is outside symbol '{}' range 0..{}",
                            sym.name, sym.timeline.frame_count
                        ),
                    ));
                }
                let obj_event = event
                    .as_deref()
                    .map(parse_object_event)
                    .transpose()?
                    .unwrap_or(neoflash_core::ObjectEvent::Load);
                let mut target_el_id: Option<String> = None;
                let layer_indices: Vec<usize> = match layer.as_deref() {
                    Some(sel) => vec![resolve_layer_index(&sym.timeline, sel)?],
                    None => (0..sym.timeline.layers.len()).collect(),
                };
                for li in layer_indices {
                    let lyr = &mut sym.timeline.layers[li];
                    let Some(kf) = lyr.key_at_mut(frame) else {
                        continue;
                    };
                    if let Some(el) = kf
                        .elements
                        .iter_mut()
                        .find(|e| e.id == el_sel || (!e.name.is_empty() && e.name == el_sel))
                    {
                        if remove_empty && source.trim().is_empty() {
                            el.scripts.retain(|s| s.event != obj_event);
                        } else if let Some(os) =
                            el.scripts.iter_mut().find(|s| s.event == obj_event)
                        {
                            let resolved = explicit_lang.unwrap_or(os.script.language);
                            os.script = Script {
                                language: resolved,
                                source: source.clone(),
                            };
                        } else {
                            let resolved = explicit_lang.unwrap_or_else(|| {
                                el.scripts
                                    .first()
                                    .map(|s| s.script.language)
                                    .unwrap_or(sym_default_lang)
                            });
                            el.scripts.push(ObjectScript {
                                event: obj_event,
                                script: Script {
                                    language: resolved,
                                    source: source.clone(),
                                },
                            });
                        }
                        target_el_id = Some(el.id.clone());
                        break;
                    }
                }
                let eid = target_el_id.ok_or_else(|| {
                    CliError::runtime(
                        "element_not_found",
                        format!("Element '{el_sel}' not found at frame {frame}"),
                    )
                })?;
                upgrade_project_format_versions(project);
                return Ok(json!({
                    "target": "object",
                    "symbol_id": sym_id,
                    "frame": frame,
                    "element_id": eid,
                    "event": format!("{obj_event:?}").to_ascii_lowercase(),
                }));
            }

            let resolved_lang =
                if let Some(existing) = sym.timeline.scripts.iter().find(|s| s.frame == frame) {
                    explicit_lang.unwrap_or(existing.script.language)
                } else {
                    explicit_lang.unwrap_or(sym_default_lang)
                };
            if remove_empty && source.trim().is_empty() {
                sym.timeline.scripts.retain(|s| s.frame != frame);
            } else {
                if frame >= sym.timeline.frame_count {
                    sym.timeline.frame_count = frame + 1;
                }
                if let Some(existing) = sym.timeline.scripts.iter_mut().find(|s| s.frame == frame) {
                    existing.script = Script {
                        language: resolved_lang,
                        source,
                    };
                } else {
                    sym.timeline.scripts.push(FrameScript {
                        layer: None,
                        frame,
                        script: Script {
                            language: resolved_lang,
                            source,
                        },
                    });
                    sym.timeline.scripts.sort_by_key(|s| s.frame);
                }
            }
            upgrade_project_format_versions(project);
            Ok(json!({
                "target": "frame",
                "symbol_id": sym_id,
                "frame": frame,
                "language": format!("{resolved_lang:?}").to_ascii_lowercase(),
            }))
        }
        BatchOp::RemoveElement {
            symbol,
            layer,
            frame,
            element,
        } => {
            let sym_id = match symbol.as_deref() {
                Some(sel) => resolve_symbol(project, sel)?.id.clone(),
                None => project.root.clone(),
            };
            let sym = resolve_symbol_mut(project, &sym_id)?;
            if frame >= sym.timeline.frame_count {
                return Err(CliError::runtime(
                    "frame_out_of_range",
                    format!(
                        "Frame {frame} is outside symbol '{}' range 0..{}",
                        sym.name, sym.timeline.frame_count
                    ),
                ));
            }
            let layer_indices: Vec<usize> = match layer.as_deref() {
                Some(sel) => vec![resolve_layer_index(&sym.timeline, sel)?],
                None => (0..sym.timeline.layers.len()).collect(),
            };
            let mut removed_id: Option<String> = None;
            for li in layer_indices {
                let Some(kf) = sym.timeline.layers[li].key_at_mut(frame) else {
                    continue;
                };
                if let Some(pos) = kf
                    .elements
                    .iter()
                    .position(|e| e.id == element || (!e.name.is_empty() && e.name == element))
                {
                    removed_id = Some(kf.elements.remove(pos).id);
                    break;
                }
            }
            let eid = removed_id.ok_or_else(|| {
                CliError::runtime(
                    "element_not_found",
                    format!("Element '{element}' not found at frame {frame}"),
                )
            })?;
            Ok(json!({ "removed_element_id": eid, "frame": frame }))
        }
        BatchOp::ApplyCommandSteps {
            symbol,
            layer,
            frame,
            elements,
            steps,
        } => {
            let sym_id = match symbol.as_deref() {
                Some(sel) => resolve_symbol(project, sel)?.id.clone(),
                None => project.root.clone(),
            };
            let valid_symbols: HashSet<String> = project
                .symbols
                .iter()
                .filter(|s| !project.is_scene(&s.id))
                .map(|s| s.id.clone())
                .collect();
            let valid_symbol_refs: HashSet<&str> =
                valid_symbols.iter().map(String::as_str).collect();
            let sym = resolve_symbol_mut(project, &sym_id)?;
            if frame >= sym.timeline.frame_count {
                return Err(CliError::runtime(
                    "frame_out_of_range",
                    format!(
                        "Frame {frame} is outside symbol '{}' range 0..{}",
                        sym.name, sym.timeline.frame_count
                    ),
                ));
            }
            let layer_indices: Vec<usize> = match layer.as_deref() {
                Some(sel) => vec![resolve_layer_index(&sym.timeline, sel)?],
                None => (0..sym.timeline.layers.len()).collect(),
            };
            let mut total_applied = 0usize;
            for li in layer_indices {
                let Some(kf) = sym.timeline.layers[li].key_at_mut(frame) else {
                    continue;
                };
                let mut selected_ids: Vec<String> = kf
                    .elements
                    .iter()
                    .filter(|el| {
                        elements
                            .iter()
                            .any(|q| q == &el.id || (!el.name.is_empty() && q == &el.name))
                    })
                    .map(|el| el.id.clone())
                    .collect();
                if selected_ids.is_empty() {
                    continue;
                }
                total_applied += apply_command_steps_to_elements(
                    &mut kf.elements,
                    &mut selected_ids,
                    &steps,
                    &valid_symbol_refs,
                )
                .map_err(|e| CliError::runtime("command_steps_failed", e))?;
            }
            if total_applied == 0 {
                return Err(CliError::runtime(
                    "element_not_found",
                    "No matching elements found for apply_command_steps",
                ));
            }
            upgrade_project_format_versions(project);
            Ok(json!({ "steps_applied": total_applied }))
        }
        BatchOp::Jsfl {
            code,
            source,
            script_file,
            symbol,
            frame,
            flfile_root,
            allow_read,
            allow_write,
            allow_delete,
        } => {
            let (script_name, script_src) = match (code, source, script_file) {
                (Some(src), None, None) | (None, Some(src), None) => {
                    ("batch-inline.jsfl".to_owned(), src)
                }
                (None, None, Some(path)) => {
                    let name = path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("batch.jsfl")
                        .to_owned();
                    let text = read_jsfl_source_file(&path)?;
                    (name, text)
                }
                _ => {
                    return Err(CliError::usage(
                        "invalid_arguments",
                        "batch jsfl step requires exactly one of 'code', 'source', or 'script_file'",
                    ));
                }
            };
            let eff_read = allow_read.or_else(|| flfile_root.clone());
            let eff_write = allow_write.or(flfile_root);
            context.effects.activate()?;
            let res = execute_jsfl_on_project(
                project,
                JsflExecRequest {
                    script_name: &script_name,
                    source: &script_src,
                    symbol_selector: symbol.as_deref(),
                    frame,
                    allow_read: eff_read.as_deref(),
                    allow_write: eff_write.as_deref(),
                    allow_delete,
                    effects: Some(context.effects.handle()),
                },
            )?;
            *project = res.project;
            upgrade_project_format_versions(project);
            Ok(json!({
                "traces": res.output_lines,
                "output_lines": res.output_lines,
                "mutations": res.mutations,
                "steps_executed": res.steps_executed,
                "return_value": jsfl_value_to_json(&res.return_value),
            }))
        }
        BatchOp::Validate { strict } => {
            validate_loaded_project(project, context.project_base_dir.as_deref(), strict)
        }
        BatchOp::Compile { strict } => {
            let report =
                compile_loaded_project_scripts(project, context.project_base_dir.as_deref());
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
            Ok(report)
        }
        BatchOp::Eval {
            scene,
            symbol,
            frame,
            frames,
            scripts,
        } => {
            let (start_frame, end_frame) = match (frame, frames) {
                (Some(_), Some(_)) => {
                    return Err(CliError::usage(
                        "conflicting_fields",
                        "eval op cannot specify both 'frame' and 'frames'",
                    ));
                }
                (Some(f), None) => (f, f),
                (None, Some(FrameRangeParam::Text(raw))) => parse_frame_range_str(&raw)?,
                (None, Some(FrameRangeParam::Object { start, end })) => {
                    if start > end {
                        return Err(CliError::usage(
                            "invalid_frame_range",
                            format!("Invalid frame range {start}..={end}"),
                        ));
                    }
                    (start, end)
                }
                (None, None) => (0, 0),
            };
            evaluate_loaded_project_frame(
                project,
                context.project_base_dir.as_deref(),
                &EvalOptions {
                    scene,
                    symbol,
                    start_frame,
                    end_frame,
                    scripts,
                },
            )
        }
        BatchOp::Inspect {
            symbol,
            frame,
            no_elements,
            no_scripts,
            no_resources,
        } => build_inspect_value(
            project,
            &InspectOptions {
                symbol,
                frame,
                no_elements,
                no_scripts,
                no_resources,
            },
        ),
        BatchOp::Import(spec) => import_project_with_spec(project, &spec),
        BatchOp::Export(mut spec) => {
            if spec.project_base_dir.is_none() {
                spec.project_base_dir.clone_from(&context.project_base_dir);
            }
            plan_export_effects(project, &spec, context.effects)?;
            context.effects.activate()?;
            export_project_with_spec(project, &spec)
        }
        BatchOp::Save { path } => {
            let dest = path
                .or_else(|| default_save_target.map(Path::to_path_buf))
                .ok_or_else(|| {
                    CliError::usage(
                        "missing_field",
                        "save step requires 'path' when CLI --out/--in-place is omitted",
                    )
                })?;
            context.effects.plan_path(&dest)?;
            context.effects.activate()?;
            save_project_atomic(project, &dest)?;
            Ok(json!({ "saved_path": dest.display().to_string() }))
        }
    }
}

fn parse_instance_playback(raw: Option<&str>) -> Result<InstancePlayback, CliError> {
    match raw.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        None | Some("independent") => Ok(InstancePlayback::Independent),
        Some("loop") => Ok(InstancePlayback::Loop),
        Some("play_once") | Some("play-once") => Ok(InstancePlayback::PlayOnce),
        Some(other) => {
            if let Some(rest) = other.strip_prefix("loop_from:") {
                let n = rest.parse::<u32>().map_err(|_| {
                    CliError::usage("invalid_argument", format!("Invalid playback '{other}'"))
                })?;
                return Ok(InstancePlayback::LoopFrom(n));
            }
            if let Some(rest) = other.strip_prefix("single_frame:") {
                let n = rest.parse::<u32>().map_err(|_| {
                    CliError::usage("invalid_argument", format!("Invalid playback '{other}'"))
                })?;
                return Ok(InstancePlayback::SingleFrame(n));
            }
            Err(CliError::usage(
                "invalid_argument",
                format!("Unsupported instance playback '{other}'"),
            ))
        }
    }
}

#[cfg(test)]
mod t414_batch_tests {
    use super::*;

    #[test]
    fn t414_headless_batch_validate_compile_and_eval_hydrate_disk_includes() {
        let tmp = std::env::temp_dir().join(format!(
            "neoflash-t414-batch-inc-{}-{}",
            std::process::id(),
            new_id("dir")
        ));
        fs::create_dir_all(tmp.join("sub")).unwrap();
        fs::write(tmp.join("sub/defs.as"), "var SCORE:Number = 777;\n").unwrap();

        let mut project = Project::blank();
        project.name = "BatchIncludes".into();
        project.script_language = Some(ScriptLanguage::ActionScript2);
        let root_id = project.root.clone();
        project
            .symbol_mut(&root_id)
            .unwrap()
            .timeline
            .scripts
            .push(FrameScript {
                layer: None,
                frame: 0,
                script: Script {
                    language: ScriptLanguage::ActionScript2,
                    source: "#include \"sub/defs.as\"\ntrace(\"BATCH_SCORE=\" + SCORE);\n".into(),
                },
            });
        upgrade_project_format_versions(&mut project);
        let project_path = tmp.join("BatchIncludes.nflash");
        fs::write(&project_path, project.to_json().unwrap()).unwrap();

        let ops_json = r#"[
            {"op": "validate", "strict": true},
            {"op": "compile", "strict": true},
            {"op": "eval", "frame": 0, "scripts": true}
        ]"#;
        let args = vec![
            "--project".to_owned(),
            project_path.display().to_string(),
            "--stdin".to_owned(),
        ];
        let mut stdin = std::io::Cursor::new(ops_json.as_bytes());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let rc = cmd_batch(
            &args,
            OutputFormat::Json,
            false,
            &mut stdin,
            &mut stdout,
            &mut stderr,
        );
        assert_eq!(
            rc,
            0,
            "batch failed: stdout={} stderr={}",
            String::from_utf8_lossy(&stdout),
            String::from_utf8_lossy(&stderr)
        );
        let parsed: Value = serde_json::from_slice(&stdout).unwrap();
        assert_eq!(parsed["ok"], json!(true));
        let results = parsed["results"].as_array().expect("step results");
        assert_eq!(results.len(), 3);
        assert_eq!(results[0]["result"]["valid"], json!(true));
        assert_eq!(results[0]["result"]["script_error_count"], json!(0));
        assert_eq!(results[1]["result"]["error_count"], json!(0));
        let traces = results[2]["result"]["trace_lines"]
            .as_array()
            .expect("eval trace_lines");
        assert!(
            traces.iter().any(|t| t == "BATCH_SCORE=777"),
            "expected hydrated disk include trace in batch eval, got: {traces:?}"
        );

        let _ = fs::remove_dir_all(&tmp);
    }
}
