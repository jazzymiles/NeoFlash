//! Shared import and export handlers delegating directly to `neoflash_import` and
//! `neoflash_export` for headless CLI commands and batch pipelines.
use super::{
    CliError, OutputFormat, load_project_valid, resolve_symbol, save_project_atomic,
    serialize_json_value, take_flag_value, write_json_error,
};
use neoflash_core::{GlobalSwatch, Project};
use neoflash_export::{
    AtlasFormat, AtlasOptions, AudioExportOptions, AudioExportPolicy, AudioFormat, BannerOptions,
    FlaCompatibilityReport, FlaExportOptions, FlaTargetProfile, FontExportPolicy,
    HtmlPublishConfig, HtmlScaleMode, HtmlStageAlign, ImageExportFormat, ImageExportOptions,
    ImageExportRange, LottieFormat, LottieImageFormat, LottieImageMode, LottieOptions,
    LottieTextMode, PublishProfile, SizeLevel, SvgOptions, SwfPublishOptions, SwfPublishProfile,
    VideoFormat, VideoOptions, export_project_with_profile, load_profiles_from_project,
};
use neoflash_import::{
    Diagnostic as ImportDiagnostic, FigmaImportOptions, ImportError, ImportFormat, ImportReport,
    PsdImportOptions, Severity as ImportSeverity, Statistics as ImportStatistics,
    VectorImportOptions, import_act_palette, import_ase_palette, import_clr_palette,
    import_figma_path, import_fxg_str, import_numbered_image_sequence_paths,
    import_pdf_ai_with_options, import_psd_path, import_svg_with_options,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Supported export target identifiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExportTarget {
    Html5,
    Fla,
    Xfl,
    Swf,
    Svg,
    Lottie,
    Atlas,
    Sequence,
    Video,
    Audio,
    Banner,
    Desktop,
}

impl ExportTarget {
    pub(crate) fn parse(raw: &str) -> Result<Self, CliError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "html5" | "html" | "web" => Ok(Self::Html5),
            "fla" => Ok(Self::Fla),
            "xfl" => Ok(Self::Xfl),
            "swf" => Ok(Self::Swf),
            "svg" => Ok(Self::Svg),
            "lottie" | "dotlottie" => Ok(Self::Lottie),
            "atlas" | "spritesheet" | "sprite-sheet" => Ok(Self::Atlas),
            "sequence" | "image" | "images" => Ok(Self::Sequence),
            "video" => Ok(Self::Video),
            "audio" => Ok(Self::Audio),
            "banner" => Ok(Self::Banner),
            "desktop" | "app" => Ok(Self::Desktop),
            other => Err(CliError::usage(
                "unsupported_export_target",
                format!(
                    "Unsupported export target '{other}'. Expected one of: html5, fla, xfl, swf, svg, lottie, atlas, sequence, video, audio, banner, desktop"
                ),
            )),
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Html5 => "html5",
            Self::Fla => "fla",
            Self::Xfl => "xfl",
            Self::Swf => "swf",
            Self::Svg => "svg",
            Self::Lottie => "lottie",
            Self::Atlas => "atlas",
            Self::Sequence => "sequence",
            Self::Video => "video",
            Self::Audio => "audio",
            Self::Banner => "banner",
            Self::Desktop => "desktop",
        }
    }

    fn requires_web_runtime(self) -> bool {
        matches!(self, Self::Html5 | Self::Desktop)
    }
}

/// Parameters controlling an export operation from either CLI flags or a JSON `batch` op.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExportSpec {
    pub(crate) target: String,
    pub(crate) out: PathBuf,
    #[serde(default)]
    pub(crate) profile: Option<String>,
    #[serde(default)]
    pub(crate) size_level: Option<String>,
    #[serde(default)]
    pub(crate) runtime: Option<PathBuf>,
    #[serde(default)]
    pub(crate) runtime_url: Option<String>,
    #[serde(default)]
    pub(crate) font_policy: Option<String>,
    #[serde(default)]
    pub(crate) audio_policy: Option<String>,
    #[serde(default)]
    pub(crate) clean: bool,
    #[serde(default)]
    pub(crate) sprite_sheets: bool,
    #[serde(default)]
    pub(crate) assets_folder: bool,
    #[serde(default)]
    pub(crate) scale_mode: Option<String>,
    #[serde(default)]
    pub(crate) stage_align: Option<String>,
    #[serde(default)]
    pub(crate) allow_fullscreen: Option<bool>,
    #[serde(default)]
    pub(crate) render_mode: Option<String>,
    #[serde(default)]
    pub(crate) fla_profile: Option<String>,
    #[serde(default)]
    pub(crate) swf_profile: Option<String>,
    #[serde(default)]
    pub(crate) scriptless: bool,
    #[serde(default)]
    pub(crate) outline_text: bool,
    #[serde(default)]
    pub(crate) symbol: Option<String>,
    #[serde(default)]
    pub(crate) frame: Option<u32>,
    #[serde(default)]
    pub(crate) end_frame: Option<u32>,
    #[serde(default)]
    pub(crate) width: Option<u32>,
    #[serde(default)]
    pub(crate) height: Option<u32>,
    #[serde(default)]
    pub(crate) transparent: bool,
    #[serde(default)]
    pub(crate) text_as_outlines: bool,
    #[serde(default)]
    pub(crate) pretty: bool,
    #[serde(default)]
    pub(crate) atlas_format: Option<String>,
    #[serde(default)]
    pub(crate) atlas_extrude: u32,
    #[serde(default)]
    pub(crate) audio_format: Option<String>,
    #[serde(default)]
    pub(crate) banner_max_gzip: Option<usize>,
    #[serde(default)]
    pub(crate) image_format: Option<String>,
    #[serde(default)]
    pub(crate) html_preview: bool,
}

/// Locate the compiled `target/web-runtime` directory for HTML5 and Desktop exports.
pub(crate) fn resolve_web_runtime(explicit: Option<&Path>) -> Result<PathBuf, CliError> {
    if let Some(dir) = explicit {
        if dir.join("neoflash_web.js").is_file() {
            return Ok(dir.to_path_buf());
        }
        return Err(CliError::runtime(
            "missing_web_runtime",
            format!(
                "Specified --runtime directory '{}' does not contain neoflash_web.js",
                dir.display()
            ),
        ));
    }

    let mut candidates: Vec<PathBuf> = Vec::new();
    for env_key in ["NEOFLASH_WEB_RUNTIME", "NEOFLASH_RUNTIME_DIR"] {
        if let Some(val) = std::env::var_os(env_key)
            && !val.is_empty()
        {
            candidates.push(PathBuf::from(val));
        }
    }

    if let Ok(exe) = std::env::current_exe()
        && let Some(exe_dir) = exe.parent()
    {
        if let Some(contents) = exe_dir.parent() {
            candidates.push(contents.join("Resources").join("web-runtime"));
            candidates.push(contents.join("Resources").join("runtime"));
            candidates.push(contents.join("web-runtime"));
        }
        candidates.push(exe_dir.join("web-runtime"));
        candidates.push(exe_dir.join("runtime"));
        for ancestor in exe_dir.ancestors() {
            candidates.push(ancestor.join("target/web-runtime"));
        }
    }

    #[cfg(debug_assertions)]
    candidates.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/web-runtime"));
    candidates.push(PathBuf::from("target/web-runtime"));
    candidates.push(PathBuf::from("../target/web-runtime"));

    for cand in &candidates {
        if cand.join("neoflash_web.js").is_file() {
            return Ok(cand.clone());
        }
    }

    Err(CliError::runtime(
        "missing_web_runtime",
        "Could not locate NeoFlash web runtime (neoflash_web.js). Pass --runtime <dir>, set NEOFLASH_WEB_RUNTIME, or run ./scripts/build-web.sh.",
    ))
}

pub(crate) fn cmd_import<W1: Write, W2: Write>(
    args: &[String],
    format: OutputFormat,
    pretty: bool,
    stdout: &mut W1,
    stderr: &mut W2,
) -> i32 {
    match run_cmd_import(args, format, pretty, stdout) {
        Ok(()) => 0,
        Err(err) => {
            write_json_error(&err, format, pretty, stderr);
            err.exit_code
        }
    }
}

fn run_cmd_import<W1: Write>(
    args: &[String],
    format: OutputFormat,
    pretty: bool,
    stdout: &mut W1,
) -> Result<(), CliError> {
    let mut inputs: Vec<PathBuf> = Vec::new();
    let mut out_path: Option<PathBuf> = None;

    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--input" | "-i" => {
                let first = take_flag_value(args, &mut i, "--input")?;
                inputs.push(PathBuf::from(first));
                while i + 1 < args.len() && !args[i + 1].starts_with('-') {
                    i += 1;
                    inputs.push(PathBuf::from(&args[i]));
                }
            }
            "--out" | "--output" | "-o" => {
                out_path = Some(PathBuf::from(take_flag_value(args, &mut i, "--out")?));
            }
            other if !other.starts_with('-') => inputs.push(PathBuf::from(other)),
            other => {
                return Err(CliError::usage(
                    "unknown_flag",
                    format!("Unknown flag '{other}' for 'import'"),
                ));
            }
        }
        i += 1;
    }

    if inputs.is_empty() {
        return Err(CliError::usage(
            "missing_flag",
            "'import' requires at least one --input <path>",
        ));
    }
    let out_path = out_path.ok_or_else(|| {
        CliError::usage("missing_flag", "'import' requires --out <project.nflash>")
    })?;

    let (mut project, report) = import_paths_with_neoflash_import(&inputs)?;
    save_project_atomic(&mut project, &out_path)?;

    let symbol_count = project.symbols.len();
    let layer_count: usize = project
        .symbols
        .iter()
        .map(|s| s.timeline.layers.len())
        .sum();
    let resource_count = project.resources.len();

    match format {
        OutputFormat::Json => {
            let val = json!({
                "ok": true,
                "command": "import",
                "out": out_path.display().to_string(),
                "inputs": inputs.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
                "report": report,
                "project": {
                    "name": project.name,
                    "width": project.width,
                    "height": project.height,
                    "fps": project.fps,
                    "format_version": project.format_version,
                    "symbols": symbol_count,
                    "layers": layer_count,
                    "resources": resource_count,
                },
            });
            let text = serialize_json_value(&val, pretty)?;
            writeln!(stdout, "{text}").map_err(|e| {
                CliError::runtime("io_error", format!("Failed writing stdout: {e}"))
            })?;
        }
        OutputFormat::Text => {
            writeln!(
                stdout,
                "Imported {} symbol(s) into {} (format={:?}, warnings/diags={})",
                symbol_count,
                out_path.display(),
                report.format,
                report.diagnostics.len()
            )
            .map_err(|e| CliError::runtime("io_error", format!("Failed writing stdout: {e}")))?;
        }
    }
    Ok(())
}

fn ext_lower(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
}

fn stem_string(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("Imported")
        .to_owned()
}

fn is_sequence_image_ext(ext: &str) -> bool {
    matches!(ext, "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp")
}

fn import_err(err: ImportError) -> CliError {
    CliError::runtime("import_failed", err.to_string())
}

fn import_palette_path(path: &Path, ext: &str) -> Result<(Project, ImportReport), CliError> {
    let bytes = fs::read(path).map_err(|e| {
        CliError::runtime(
            "io_error",
            format!("Failed reading palette '{}': {e}", path.display()),
        )
    })?;
    let source_name = stem_string(path);
    let swatches: Vec<GlobalSwatch> = match ext {
        "act" => import_act_palette(&bytes, &source_name).map_err(import_err)?,
        "clr" => import_clr_palette(&bytes, &source_name).map_err(import_err)?,
        "ase" => import_ase_palette(&bytes).map_err(import_err)?,
        _ => {
            return Err(CliError::runtime(
                "import_failed",
                format!("Unsupported palette extension '.{ext}'"),
            ));
        }
    };
    if swatches.is_empty() {
        return Err(CliError::runtime(
            "import_failed",
            format!("{}: palette contains no colour swatches", path.display()),
        ));
    }
    let colors: Vec<_> = swatches.iter().map(|s| s.color).take(256).collect();
    let mut project = Project::blank();
    project.name = source_name;
    project.set_global_swatches(&swatches);
    project.set_custom_swatches(&colors);

    let report = ImportReport {
        source: path.display().to_string(),
        format: ImportFormat::Unknown,
        version: Some(ext.to_ascii_uppercase()),
        can_import: true,
        statistics: ImportStatistics {
            symbols: project.symbols.len(),
            layers: 1,
            keyframes: 1,
            elements: colors.len(),
            scripts: 0,
        },
        diagnostics: vec![ImportDiagnostic {
            severity: ImportSeverity::Info,
            code: "palette-imported".to_owned(),
            location: path.display().to_string(),
            message: format!(
                "Imported {} colour swatch(es) from .{ext} palette.",
                colors.len()
            ),
        }],
        binary_fla: None,
        swf: None,
        swc: None,
    };
    Ok((project, report))
}

/// Import one or more inputs directly via `neoflash_import`.
pub(crate) fn import_paths_with_neoflash_import(
    inputs: &[PathBuf],
) -> Result<(Project, ImportReport), CliError> {
    if inputs.is_empty() {
        return Err(CliError::usage("missing_input", "Missing input path"));
    }
    if inputs.len() > 1 {
        let imported = import_numbered_image_sequence_paths(inputs, true)
            .map_err(import_err)?
            .ok_or_else(|| {
                CliError::runtime(
                    "import_failed",
                    "Inputs do not form a numbered image sequence (for example frame_0001.png)",
                )
            })?;
        return Ok((imported.project, imported.report));
    }

    let path = &inputs[0];
    if path.is_dir() && !path.join("DOMDocument.xml").exists() {
        let imported = import_numbered_image_sequence_paths(inputs, true)
            .map_err(import_err)?
            .ok_or_else(|| {
                CliError::runtime(
                    "import_failed",
                    format!(
                        "Directory '{}' does not contain XFL DOMDocument.xml or a numbered image sequence",
                        path.display()
                    ),
                )
            })?;
        return Ok((imported.project, imported.report));
    }

    if path
        .to_string_lossy()
        .to_ascii_lowercase()
        .ends_with(".figma.json")
    {
        return import_figma_path(path, &FigmaImportOptions::default()).map_err(import_err);
    }

    if let Some(ext) = ext_lower(path) {
        match ext.as_str() {
            "fig" | "figma" => {
                return import_figma_path(path, &FigmaImportOptions::default()).map_err(import_err);
            }
            "psd" | "psb" => {
                return import_psd_path(path, &PsdImportOptions::default()).map_err(import_err);
            }
            "svg" => {
                let text = fs::read_to_string(path).map_err(|e| {
                    CliError::runtime(
                        "io_error",
                        format!("Failed reading SVG '{}': {e}", path.display()),
                    )
                })?;
                let source_name = stem_string(path);
                let (project, report) =
                    import_svg_with_options(&text, &source_name, &VectorImportOptions::default())
                        .map_err(import_err)?;
                if !report.can_import {
                    return Err(CliError::runtime_with_details(
                        "import_failed",
                        format!("SVG '{}' could not be imported cleanly", path.display()),
                        json!({ "report": report }),
                    ));
                }
                return Ok((project, report));
            }
            "pdf" | "ai" => {
                let bytes = fs::read(path).map_err(|e| {
                    CliError::runtime(
                        "io_error",
                        format!("Failed reading PDF/AI '{}': {e}", path.display()),
                    )
                })?;
                let source_name = stem_string(path);
                let (project, mut report) = import_pdf_ai_with_options(
                    &bytes,
                    &source_name,
                    &VectorImportOptions::default(),
                )
                .map_err(import_err)?;
                report.statistics.symbols = project.symbols.len();
                if report.statistics.elements == 0 {
                    report.can_import = false;
                    return Err(CliError::runtime_with_details(
                        "import_failed",
                        "PDF/AI file contains no importable vector or text operators.",
                        json!({ "report": report }),
                    ));
                }
                return Ok((project, report));
            }
            "fxg" => {
                let xml = fs::read_to_string(path).map_err(|e| {
                    CliError::runtime(
                        "io_error",
                        format!("Failed reading FXG '{}': {e}", path.display()),
                    )
                })?;
                let source_name = stem_string(path);
                let (mut project, mut report) =
                    import_fxg_str(&xml, &source_name).map_err(import_err)?;
                if project
                    .symbols
                    .iter()
                    .any(|s| s.linkage_id.is_some() || s.class_name.is_some())
                {
                    project.format_version = project.format_version.max(2);
                }
                report.statistics.symbols = project.symbols.len();
                if report.statistics.elements == 0 && report.statistics.symbols <= 1 {
                    report.can_import = false;
                    return Err(CliError::runtime_with_details(
                        "import_failed",
                        "FXG document contains no graphic elements or library definitions.",
                        json!({ "report": report }),
                    ));
                }
                report.can_import = true;
                return Ok((project, report));
            }
            "act" | "clr" | "ase" => return import_palette_path(path, &ext),
            ext_str if is_sequence_image_ext(ext_str) => {
                if let Some(imported) =
                    import_numbered_image_sequence_paths(inputs, true).map_err(import_err)?
                {
                    return Ok((imported.project, imported.report));
                }
            }
            _ => {}
        }
    }

    let project = neoflash_import::import_project(path).map_err(import_err)?;
    let report = neoflash_import::inspect(path).map_err(import_err)?;
    Ok((project, report))
}

pub(crate) fn cmd_export<W1: Write, W2: Write>(
    args: &[String],
    format: OutputFormat,
    pretty: bool,
    stdout: &mut W1,
    stderr: &mut W2,
) -> i32 {
    match run_cmd_export(args, format, pretty, stdout) {
        Ok(()) => 0,
        Err(err) => {
            write_json_error(&err, format, pretty, stderr);
            err.exit_code
        }
    }
}

fn run_cmd_export<W1: Write>(
    args: &[String],
    format: OutputFormat,
    pretty: bool,
    stdout: &mut W1,
) -> Result<(), CliError> {
    let mut project_path: Option<PathBuf> = None;
    let mut spec = ExportSpec::default();
    let mut has_target = false;
    let mut has_out = false;

    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--project" => {
                project_path = Some(PathBuf::from(take_flag_value(args, &mut i, "--project")?));
            }
            "--target" | "-t" => {
                spec.target = take_flag_value(args, &mut i, "--target")?;
                has_target = true;
            }
            "--out" | "--output" | "-o" => {
                spec.out = PathBuf::from(take_flag_value(args, &mut i, "--out")?);
                has_out = true;
            }
            "--profile" => spec.profile = Some(take_flag_value(args, &mut i, "--profile")?),
            "--size-level" => {
                spec.size_level = Some(take_flag_value(args, &mut i, "--size-level")?);
            }
            "--runtime" => {
                spec.runtime = Some(PathBuf::from(take_flag_value(args, &mut i, "--runtime")?));
            }
            "--runtime-url" => {
                spec.runtime_url = Some(take_flag_value(args, &mut i, "--runtime-url")?);
            }
            "--font-policy" => {
                spec.font_policy = Some(take_flag_value(args, &mut i, "--font-policy")?);
            }
            "--audio-policy" => {
                spec.audio_policy = Some(take_flag_value(args, &mut i, "--audio-policy")?);
            }
            "--clean" => spec.clean = true,
            "--sprite-sheets" | "--pack-sprite-sheet" => spec.sprite_sheets = true,
            "--assets-folder" => spec.assets_folder = true,
            "--scale-mode" => {
                spec.scale_mode = Some(take_flag_value(args, &mut i, "--scale-mode")?);
            }
            "--stage-align" => {
                spec.stage_align = Some(take_flag_value(args, &mut i, "--stage-align")?);
            }
            "--allow-fullscreen" => {
                let raw = take_flag_value(args, &mut i, "--allow-fullscreen")?;
                spec.allow_fullscreen = Some(match raw.to_ascii_lowercase().as_str() {
                    "true" | "1" | "yes" => true,
                    "false" | "0" | "no" => false,
                    _ => {
                        return Err(CliError::usage(
                            "invalid_argument",
                            format!("Invalid boolean for --allow-fullscreen: '{raw}'"),
                        ));
                    }
                });
            }
            "--render-mode" => {
                spec.render_mode = Some(take_flag_value(args, &mut i, "--render-mode")?);
            }
            "--fla-profile" => {
                spec.fla_profile = Some(take_flag_value(args, &mut i, "--fla-profile")?);
            }
            "--swf-profile" => {
                spec.swf_profile = Some(take_flag_value(args, &mut i, "--swf-profile")?);
            }
            "--scriptless" => spec.scriptless = true,
            "--outline-text" => spec.outline_text = true,
            "--symbol" => spec.symbol = Some(take_flag_value(args, &mut i, "--symbol")?),
            "--frame" => {
                let raw = take_flag_value(args, &mut i, "--frame")?;
                spec.frame = Some(parse_u32_flag("--frame", &raw)?);
            }
            "--end-frame" => {
                let raw = take_flag_value(args, &mut i, "--end-frame")?;
                spec.end_frame = Some(parse_u32_flag("--end-frame", &raw)?);
            }
            "--width" => {
                let raw = take_flag_value(args, &mut i, "--width")?;
                spec.width = Some(parse_u32_flag("--width", &raw)?);
            }
            "--height" => {
                let raw = take_flag_value(args, &mut i, "--height")?;
                spec.height = Some(parse_u32_flag("--height", &raw)?);
            }
            "--transparent" => spec.transparent = true,
            "--text-as-outlines" => spec.text_as_outlines = true,
            "--pretty" => spec.pretty = true,
            "--atlas-format" => {
                spec.atlas_format = Some(take_flag_value(args, &mut i, "--atlas-format")?);
            }
            "--atlas-extrude" => {
                let raw = take_flag_value(args, &mut i, "--atlas-extrude")?;
                spec.atlas_extrude = parse_u32_flag("--atlas-extrude", &raw)?;
            }
            "--audio-format" => {
                spec.audio_format = Some(take_flag_value(args, &mut i, "--audio-format")?);
            }
            "--banner-max-gzip" => {
                let raw = take_flag_value(args, &mut i, "--banner-max-gzip")?;
                spec.banner_max_gzip = Some(raw.parse::<usize>().map_err(|_| {
                    CliError::usage(
                        "invalid_argument",
                        format!("Invalid --banner-max-gzip value '{raw}'"),
                    )
                })?);
            }
            "--image-format" => {
                spec.image_format = Some(take_flag_value(args, &mut i, "--image-format")?);
            }
            "--html-preview" => spec.html_preview = true,
            other => {
                return Err(CliError::usage(
                    "unknown_flag",
                    format!("Unknown flag '{other}' for 'export'"),
                ));
            }
        }
        i += 1;
    }

    let project_path = project_path.ok_or_else(|| {
        CliError::usage("missing_flag", "'export' requires --project <file.nflash>")
    })?;
    if !has_target {
        return Err(CliError::usage(
            "missing_flag",
            "'export' requires --target <html5|fla|xfl|swf|svg|lottie|dotlottie|atlas|sequence|video|audio|banner|desktop>",
        ));
    }
    if !has_out || spec.out.as_os_str().is_empty() {
        return Err(CliError::usage(
            "missing_flag",
            "'export' requires --out <path>",
        ));
    }

    let project = load_project_valid(&project_path)?;
    let summary = export_project_with_spec(&project, &spec)?;

    match format {
        OutputFormat::Json => {
            let mut obj = serde_json::Map::new();
            obj.insert("ok".into(), Value::Bool(true));
            obj.insert("command".into(), Value::String("export".into()));
            if let Value::Object(map) = summary {
                obj.extend(map);
            }
            let text = serialize_json_value(&Value::Object(obj), pretty)?;
            writeln!(stdout, "{text}").map_err(|e| {
                CliError::runtime("io_error", format!("Failed writing stdout: {e}"))
            })?;
        }
        OutputFormat::Text => {
            let target = summary
                .get("target")
                .and_then(Value::as_str)
                .unwrap_or("export");
            let out = summary
                .get("output_path")
                .and_then(Value::as_str)
                .unwrap_or("");
            writeln!(stdout, "Exported {target} -> {out}").map_err(|e| {
                CliError::runtime("io_error", format!("Failed writing stdout: {e}"))
            })?;
        }
    }
    Ok(())
}

fn parse_u32_flag(flag: &str, raw: &str) -> Result<u32, CliError> {
    raw.parse::<u32>()
        .map_err(|_| CliError::usage("invalid_argument", format!("Invalid {flag} value '{raw}'")))
}

fn parse_font_policy(raw: &str) -> Result<FontExportPolicy, CliError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "device" => Ok(FontExportPolicy::Device),
        "embed" => Ok(FontExportPolicy::Embed),
        "subset" => Ok(FontExportPolicy::Subset),
        "outlines" => Ok(FontExportPolicy::Outlines),
        other => Err(CliError::usage(
            "invalid_argument",
            format!("Unsupported --font-policy '{other}' (expected device|embed|subset|outlines)"),
        )),
    }
}

fn parse_audio_policy(raw: &str) -> Result<AudioExportPolicy, CliError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "original" => Ok(AudioExportPolicy::Original),
        "mp3_64" | "mp3-64" | "64" => Ok(AudioExportPolicy::Mp3_64),
        "mp3_96" | "mp3-96" | "96" => Ok(AudioExportPolicy::Mp3_96),
        "mp3_128" | "mp3-128" | "128" => Ok(AudioExportPolicy::Mp3_128),
        "mp3_160" | "mp3-160" | "160" => Ok(AudioExportPolicy::Mp3_160),
        "mp3_192" | "mp3-192" | "192" => Ok(AudioExportPolicy::Mp3_192),
        "mp3_256" | "mp3-256" | "256" => Ok(AudioExportPolicy::Mp3_256),
        "mp3_320" | "mp3-320" | "320" => Ok(AudioExportPolicy::Mp3_320),
        other => Err(CliError::usage(
            "invalid_argument",
            format!("Unsupported --audio-policy '{other}'"),
        )),
    }
}

fn parse_scale_mode(raw: &str) -> Result<HtmlScaleMode, CliError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "fit" | "fit_keep_ratio" | "fit-keep-ratio" | "showall" => Ok(HtmlScaleMode::FitKeepRatio),
        "noscale" | "no_scale" | "no-scale" => Ok(HtmlScaleMode::NoScale),
        "exactfit" | "exact_fit" | "exact-fit" => Ok(HtmlScaleMode::ExactFit),
        "fill" | "fill_crop" | "fill-crop" | "noborder" => Ok(HtmlScaleMode::FillCrop),
        other => Err(CliError::usage(
            "invalid_argument",
            format!("Unsupported --scale-mode '{other}'"),
        )),
    }
}

fn parse_stage_align(raw: &str) -> Result<HtmlStageAlign, CliError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "center" => Ok(HtmlStageAlign::Center),
        "top" | "t" => Ok(HtmlStageAlign::Top),
        "bottom" | "b" => Ok(HtmlStageAlign::Bottom),
        "left" | "l" => Ok(HtmlStageAlign::Left),
        "right" | "r" => Ok(HtmlStageAlign::Right),
        "top_left" | "top-left" | "tl" => Ok(HtmlStageAlign::TopLeft),
        "top_right" | "top-right" | "tr" => Ok(HtmlStageAlign::TopRight),
        "bottom_left" | "bottom-left" | "bl" => Ok(HtmlStageAlign::BottomLeft),
        "bottom_right" | "bottom-right" | "br" => Ok(HtmlStageAlign::BottomRight),
        other => Err(CliError::usage(
            "invalid_argument",
            format!("Unsupported --stage-align '{other}'"),
        )),
    }
}

fn parse_fla_profile(raw: &str) -> Result<FlaTargetProfile, CliError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "animate" | "animatecc" | "animate_cc" | "cc" => Ok(FlaTargetProfile::AnimateCC),
        "cs6" | "flash_cs6" | "flash-cs6" => Ok(FlaTargetProfile::FlashCS6),
        "cs5.5" | "cs5_5" | "cs5-5" | "flash_cs5_5" => Ok(FlaTargetProfile::FlashCS5_5),
        "cs5" | "flash_cs5" | "flash-cs5" => Ok(FlaTargetProfile::FlashCS5),
        "xfl" | "uncompressed_xfl" | "uncompressed-xfl" => Ok(FlaTargetProfile::UncompressedXfl),
        "legacy_cs4" | "legacy-cs4" | "cs4" | "flash_cs4" => Ok(FlaTargetProfile::LegacyFlashCS4),
        "legacy_flash8" | "legacy-flash8" | "flash8" | "f8" => Ok(FlaTargetProfile::LegacyFlash8),
        other => Err(CliError::usage(
            "invalid_argument",
            format!("Unsupported --fla-profile '{other}'"),
        )),
    }
}

fn parse_swf_profile(raw: &str) -> Result<SwfPublishProfile, CliError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "fp8" | "flash8" | "avm1" => Ok(SwfPublishProfile::FlashPlayer8Avm1),
        "fp9" | "flash9" | "avm2_9" => Ok(SwfPublishProfile::FlashPlayer9Avm2),
        "fp10" | "flash10" | "avm2" => Ok(SwfPublishProfile::FlashPlayer10Avm2),
        other => Err(CliError::usage(
            "invalid_argument",
            format!("Unsupported --swf-profile '{other}' (expected fp8|fp9|fp10)"),
        )),
    }
}

fn parse_atlas_format(raw: &str) -> Result<AtlasFormat, CliError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "json" | "json-hash" | "json_hash" | "hash" => Ok(AtlasFormat::JsonHash),
        "json-array" | "json_array" | "array" => Ok(AtlasFormat::JsonArray),
        "xml" | "starling" | "starling-xml" => Ok(AtlasFormat::StarlingXml),
        "easel" | "easeljs" | "createjs" => Ok(AtlasFormat::EaselJs),
        "css" => Ok(AtlasFormat::Css),
        other => Err(CliError::usage(
            "invalid_argument",
            format!("Unsupported --atlas-format '{other}'"),
        )),
    }
}

fn resolve_publish_profile_and_options(
    project: &Project,
    spec: &ExportSpec,
) -> Result<(PublishProfile, bool), CliError> {
    let (stored_profiles, active_id) = load_profiles_from_project(project)
        .map_err(|e| CliError::runtime("export_failed", e.to_string()))?;

    let (mut profile, has_stored) = if let Some(ref selector) = spec.profile {
        let found = stored_profiles
            .into_iter()
            .find(|p| p.id == *selector || p.name.eq_ignore_ascii_case(selector))
            .ok_or_else(|| {
                CliError::runtime(
                    "profile_not_found",
                    format!("Publish profile '{selector}' not found in project"),
                )
            })?;
        (found, true)
    } else if let Some(ref act_id) = active_id {
        if let Some(found) = stored_profiles
            .iter()
            .find(|p| p.id == *act_id)
            .cloned()
            .or_else(|| stored_profiles.into_iter().next())
        {
            (found, true)
        } else {
            (PublishProfile::default(), false)
        }
    } else if let Some(first) = stored_profiles.into_iter().next() {
        (first, true)
    } else {
        (PublishProfile::default(), false)
    };

    if let Some(ref lvl) = spec.size_level {
        profile.options.size = SizeLevel::parse(lvl, spec.runtime_url.as_deref())
            .map_err(|e| CliError::usage("invalid_argument", e.to_string()))?;
    } else if !has_stored {
        profile.options.size = SizeLevel::parse("full", spec.runtime_url.as_deref())
            .map_err(|e| CliError::usage("invalid_argument", e.to_string()))?;
    }
    if let Some(ref fp) = spec.font_policy {
        profile.options.fonts = parse_font_policy(fp)?;
    }
    if let Some(ref ap) = spec.audio_policy {
        profile.options.audio = parse_audio_policy(ap)?;
    }
    if let Some(ref rm) = spec.render_mode {
        profile.render_mode = neoflash_core::HtmlRenderMode::parse(rm)
            .map_err(|e| CliError::usage("invalid_argument", e))?;
    }
    Ok((profile, has_stored))
}

fn build_html_publish_config(
    spec: &ExportSpec,
    clean_base: bool,
) -> Result<(HtmlPublishConfig, bool), CliError> {
    let parsed_render_mode = spec
        .render_mode
        .as_deref()
        .map(|rm| {
            neoflash_core::HtmlRenderMode::parse(rm)
                .map_err(|e| CliError::usage("invalid_argument", e))
        })
        .transpose()?;
    let custom = spec.clean
        || spec.scale_mode.is_some()
        || spec.stage_align.is_some()
        || spec.allow_fullscreen.is_some()
        || parsed_render_mode.is_some()
        || spec.sprite_sheets
        || spec.assets_folder;
    let mut cfg = if spec.clean || clean_base {
        HtmlPublishConfig::clean_publish_default()
    } else {
        HtmlPublishConfig::default()
    };
    if let Some(ref sm) = spec.scale_mode {
        cfg.scale_mode = parse_scale_mode(sm)?;
    }
    if let Some(ref sa) = spec.stage_align {
        cfg.stage_align = parse_stage_align(sa)?;
    }
    if let Some(fs_ok) = spec.allow_fullscreen {
        cfg.allow_fullscreen = fs_ok;
    }
    if let Some(rm) = parsed_render_mode {
        cfg.render_mode = rm;
    }
    if spec.sprite_sheets {
        cfg.sprite_sheets = true;
    }
    if spec.assets_folder {
        cfg.assets_folder = true;
    }
    Ok((cfg, custom))
}

fn fla_report_json(report: &FlaCompatibilityReport) -> Value {
    let items: Vec<Value> = report
        .items
        .iter()
        .map(|item| {
            json!({
                "code": item.code,
                "category": format!("{:?}", item.category),
                "disposition": format!("{:?}", item.disposition),
                "location": item.location,
                "message": item.message,
            })
        })
        .collect();
    json!({
        "profile": report.profile.label(),
        "blocking_count": report.blocking_items().len(),
        "item_count": items.len(),
        "items": items,
    })
}

/// Shared export dispatcher used by both `neoflash-editor --headless export` and `batch` `export` steps.
pub(crate) fn export_project_with_spec(
    project: &Project,
    spec: &ExportSpec,
) -> Result<Value, CliError> {
    project
        .validate()
        .map_err(|e| CliError::runtime("validation_failed", e.to_string()))?;
    let resolved_project = project.with_resolved_script_language();
    let project = resolved_project.as_ref();
    let target = ExportTarget::parse(&spec.target)?;
    let symbol_id = match spec.symbol.as_deref() {
        Some(sel) => Some(resolve_symbol(project, sel)?.id.clone()),
        None => None,
    };
    let runtime = if target.requires_web_runtime() {
        Some(resolve_web_runtime(spec.runtime.as_deref())?)
    } else {
        None
    };

    match target {
        ExportTarget::Html5 => {
            let runtime_dir = runtime.expect("html5 requires runtime");
            let (profile, _has_stored) = resolve_publish_profile_and_options(project, spec)?;
            let explicit_render_mode = spec
                .render_mode
                .as_deref()
                .map(|rm| {
                    neoflash_core::HtmlRenderMode::parse(rm)
                        .map_err(|e| CliError::usage("invalid_argument", e))
                })
                .transpose()?;
            let mut overridden_project: Option<Project> = None;
            if explicit_render_mode == Some(neoflash_core::HtmlRenderMode::Canvas)
                && project.html_render_mode() != neoflash_core::HtmlRenderMode::Canvas
            {
                let mut cloned = project.clone();
                cloned.set_html_render_mode(neoflash_core::HtmlRenderMode::Canvas);
                overridden_project = Some(cloned);
            }
            let effective_project = overridden_project.as_ref().unwrap_or(project);
            let (mut cfg, custom_cfg) = build_html_publish_config(spec, false)?;
            if explicit_render_mode.is_none()
                && cfg.render_mode == neoflash_core::HtmlRenderMode::Canvas
                && profile.render_mode != neoflash_core::HtmlRenderMode::Canvas
            {
                cfg.render_mode = profile.render_mode;
            }
            let effective_render_mode = if let Some(rm) = explicit_render_mode {
                rm
            } else if cfg.render_mode != neoflash_core::HtmlRenderMode::Canvas {
                cfg.render_mode
            } else {
                effective_project.html_render_mode()
            };
            let use_custom_cfg = custom_cfg
                && !(explicit_render_mode == Some(neoflash_core::HtmlRenderMode::Canvas)
                    && cfg == HtmlPublishConfig::default());
            let index = if use_custom_cfg {
                neoflash_export::export_project_with_config(
                    effective_project,
                    &spec.out,
                    &runtime_dir,
                    &profile.options,
                    &cfg,
                )
            } else if spec.profile.is_some()
                || profile.html_template.is_some()
                || profile.render_mode != neoflash_core::HtmlRenderMode::Canvas
            {
                export_project_with_profile(effective_project, &spec.out, &runtime_dir, &profile)
            } else {
                neoflash_export::export_project_with_options(
                    effective_project,
                    &spec.out,
                    &runtime_dir,
                    &profile.options,
                )
            }
            .map_err(|e| CliError::runtime("export_failed", e.to_string()))?;
            let mut warnings = neoflash_export::export_warnings(effective_project);
            if effective_render_mode == neoflash_core::HtmlRenderMode::Dom
                && effective_project.html_render_mode() != neoflash_core::HtmlRenderMode::Dom
            {
                warnings.extend(neoflash_export::dom_publish_diagnostics(effective_project));
            }
            Ok(json!({
                "target": target.as_str(),
                "output_path": index.display().to_string(),
                "output_dir": spec.out.display().to_string(),
                "runtime_dir": runtime_dir.display().to_string(),
                "profile": profile.name,
                "size_level": profile.options.size.name(),
                "render_mode": effective_render_mode.as_str(),
                "warnings": warnings,
            }))
        }
        ExportTarget::Desktop => {
            let runtime_dir = runtime.expect("desktop requires runtime");
            let (profile, _) = resolve_publish_profile_and_options(project, spec)?;
            let (cfg, _) = build_html_publish_config(spec, true)?;
            let app_dir = neoflash_export::export_desktop_app(
                project,
                &spec.out,
                &runtime_dir,
                &profile.options,
                &cfg,
            )
            .map_err(|e| CliError::runtime("export_failed", e.to_string()))?;
            Ok(json!({
                "target": target.as_str(),
                "output_path": app_dir.display().to_string(),
                "runtime_dir": runtime_dir.display().to_string(),
            }))
        }
        ExportTarget::Banner => {
            let mut opts = BannerOptions::default();
            if let Some(max_gz) = spec.banner_max_gzip {
                opts.max_gzip_bytes = Some(max_gz);
            }
            let report = neoflash_export::export_native_banner_html(project, &spec.out, &opts)
                .map_err(|e| CliError::runtime("export_failed", e.to_string()))?;
            Ok(json!({
                "target": target.as_str(),
                "output_path": report.html_path.display().to_string(),
                "raw_bytes": report.raw_bytes,
                "gzip_bytes": report.gzip_bytes,
                "frame_count": report.frame_count,
            }))
        }
        ExportTarget::Fla => {
            let fla_profile = match spec.fla_profile.as_deref() {
                Some(raw) => parse_fla_profile(raw)?,
                None => FlaTargetProfile::AnimateCC,
            };
            let report = neoflash_export::export_fla_with_profile(
                project,
                fla_profile,
                &FlaExportOptions::default(),
                &spec.out,
            )
            .map_err(|e| CliError::runtime("export_failed", e.to_string()))?;
            Ok(json!({
                "target": target.as_str(),
                "output_path": spec.out.display().to_string(),
                "compatibility": fla_report_json(&report),
            }))
        }
        ExportTarget::Xfl => {
            let report = neoflash_export::export_fla_with_profile(
                project,
                FlaTargetProfile::UncompressedXfl,
                &FlaExportOptions::default(),
                &spec.out,
            )
            .map_err(|e| CliError::runtime("export_failed", e.to_string()))?;
            Ok(json!({
                "target": target.as_str(),
                "output_path": spec.out.display().to_string(),
                "compatibility": fla_report_json(&report),
            }))
        }
        ExportTarget::Swf => {
            let mut opts = SwfPublishOptions::default();
            if let Some(ref raw) = spec.swf_profile {
                opts.profile = parse_swf_profile(raw)?;
            } else if matches!(
                project.effective_script_language(),
                neoflash_core::ScriptLanguage::ActionScript1
                    | neoflash_core::ScriptLanguage::ActionScript2
            ) {
                opts.profile = SwfPublishProfile::FlashPlayer8Avm1;
            }
            opts.scriptless_mode = spec.scriptless;
            opts.outline_text = spec.outline_text;
            let report = neoflash_export::publish_swf(project, &spec.out, &opts)
                .map_err(|e| CliError::runtime("export_failed", e.to_string()))?;
            Ok(json!({
                "target": target.as_str(),
                "output_path": spec.out.display().to_string(),
                "swf_version": opts.profile.swf_version(),
                "compatibility": fla_report_json(&report),
            }))
        }
        ExportTarget::Svg => {
            let opts = SvgOptions {
                symbol_id,
                frame: spec.frame.unwrap_or(0),
                end_frame: spec.end_frame,
                width: spec.width,
                height: spec.height,
                transparent_background: spec.transparent,
                text_as_outlines: spec.text_as_outlines,
                ..SvgOptions::default()
            };
            let report = neoflash_export::export_project_svg(project, &spec.out, &opts)
                .map_err(|e| CliError::runtime("export_failed", e.to_string()))?;
            Ok(json!({
                "target": target.as_str(),
                "output_path": spec.out.display().to_string(),
                "files": report.files.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
                "width": report.width,
                "height": report.height,
                "frame_count": report.frame_count,
                "element_count": report.element_count,
                "defs_count": report.defs_count,
            }))
        }
        ExportTarget::Lottie => {
            let format = if spec.target.trim().eq_ignore_ascii_case("dotlottie")
                || ext_lower(&spec.out).as_deref() == Some("lottie")
            {
                LottieFormat::DotLottie
            } else {
                LottieFormat::Json
            };
            let image_format = match spec.image_format.as_deref() {
                Some(raw) => match raw.trim().to_ascii_lowercase().as_str() {
                    "original" => LottieImageFormat::Original,
                    "png" => LottieImageFormat::Png,
                    "jpg" | "jpeg" => LottieImageFormat::Jpeg,
                    "webp" => LottieImageFormat::Webp,
                    other => {
                        return Err(CliError::usage(
                            "invalid_argument",
                            format!(
                                "Unsupported --image-format '{other}'. Expected original, png, jpeg, or webp"
                            ),
                        ));
                    }
                },
                None => LottieImageFormat::Original,
            };
            let frame_range = if let Some(start_0) = spec.frame {
                let end_0 = spec.end_frame.unwrap_or(start_0).max(start_0);
                Some((start_0.saturating_add(1), end_0.saturating_add(1)))
            } else {
                spec.end_frame.map(|end_0| (1, end_0.saturating_add(1)))
            };
            let report = neoflash_export::export_project_lottie(
                project,
                &spec.out,
                &LottieOptions {
                    format,
                    symbol: symbol_id,
                    width: spec.width,
                    height: spec.height,
                    pretty: spec.pretty,
                    include_background: !spec.transparent,
                    image_mode: if spec.assets_folder {
                        LottieImageMode::Folder
                    } else {
                        LottieImageMode::Embed
                    },
                    image_format,
                    text_mode: if spec.outline_text || spec.text_as_outlines {
                        LottieTextMode::Shapes
                    } else {
                        LottieTextMode::TextLayers
                    },
                    frame_range,
                    html_preview: spec.html_preview,
                    ..LottieOptions::default()
                },
            )
            .map_err(|e| CliError::runtime("export_failed", e.to_string()))?;
            Ok(json!({
                "target": target.as_str(),
                "format": match format {
                    LottieFormat::Json => "json",
                    LottieFormat::DotLottie => "dotlottie",
                },
                "output_path": spec.out.display().to_string(),
                "html_preview_path": report.html_preview_path.map(|p| p.display().to_string()),
                "bytes": report.bytes,
                "width": report.width,
                "height": report.height,
                "fps": report.fps,
                "layer_count": report.layer_count,
                "frame_count": report.frame_count,
                "asset_count": report.asset_count,
                "exported_count": report.exported_count,
                "approximated_count": report.approximated_count,
                "not_exported_count": report.not_exported_count,
                "notes": report.notes,
                "warnings": report.warnings,
            }))
        }
        ExportTarget::Atlas => {
            let ext = ext_lower(&spec.out).unwrap_or_else(|| "json".to_owned());
            let atlas_format = match spec.atlas_format.as_deref() {
                Some(raw) => parse_atlas_format(raw)?,
                None => match ext.as_str() {
                    "xml" => AtlasFormat::StarlingXml,
                    "css" => AtlasFormat::Css,
                    _ => AtlasFormat::JsonHash,
                },
            };
            let report = neoflash_export::export_project_atlas(
                project,
                &spec.out,
                &AtlasOptions {
                    symbol_id,
                    format: atlas_format,
                    extrude: spec.atlas_extrude,
                    ..AtlasOptions::default()
                },
            )
            .map_err(|e| CliError::runtime("export_failed", e.to_string()))?;
            Ok(json!({
                "target": target.as_str(),
                "output_path": report.metadata_path.display().to_string(),
                "image_path": report.image_path.display().to_string(),
                "metadata_path": report.metadata_path.display().to_string(),
                "sheet_width": report.sheet_width,
                "sheet_height": report.sheet_height,
                "frame_count": report.frame_count,
                "unique_sprites": report.unique_sprites,
            }))
        }
        ExportTarget::Sequence => {
            let ext = ext_lower(&spec.out).unwrap_or_else(|| "png".to_owned());
            let img_fmt = match ext.as_str() {
                "jpg" | "jpeg" => ImageExportFormat::Jpeg,
                "webp" => ImageExportFormat::Webp,
                "gif" => ImageExportFormat::Gif,
                "bmp" => ImageExportFormat::Bmp,
                "svg" => ImageExportFormat::Svg,
                "ai" => ImageExportFormat::Ai,
                "pdf" => ImageExportFormat::Pdf,
                "psd" => ImageExportFormat::Psd,
                _ => ImageExportFormat::Png,
            };
            let mut opts = ImageExportOptions::for_project(project, spec.frame.unwrap_or(0));
            opts.format = img_fmt;
            opts.symbol_id = symbol_id.clone();
            opts.range = if let Some(start) = spec.frame {
                let end = spec.end_frame.unwrap_or(start).max(start);
                if end == start {
                    ImageExportRange::CurrentFrame(start)
                } else {
                    ImageExportRange::Frames { start, end }
                }
            } else if let Some(ref sym_id) = symbol_id {
                let fc = project
                    .symbol(sym_id)
                    .map_or(1, |s| s.timeline.frame_count.max(1));
                ImageExportRange::Frames {
                    start: 0,
                    end: fc.saturating_sub(1),
                }
            } else {
                ImageExportRange::WholeMovie
            };
            let report = neoflash_export::export_project_image(project, &spec.out, &opts)
                .map_err(|e| CliError::runtime("export_failed", e.to_string()))?;
            Ok(json!({
                "target": target.as_str(),
                "output_path": spec.out.display().to_string(),
                "files": report.files.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
                "total_bytes": report.total_bytes,
                "width": report.width,
                "height": report.height,
                "format": report.format.label(),
            }))
        }
        ExportTarget::Video => {
            let fmt = VideoFormat::from_path(&spec.out).unwrap_or(VideoFormat::Mp4);
            let report = neoflash_export::export_project_video(
                project,
                &spec.out,
                &VideoOptions {
                    format: fmt,
                    codec: fmt.default_codec(),
                    symbol: symbol_id,
                    width: spec.width.unwrap_or(0),
                    height: spec.height.unwrap_or(0),
                    ..VideoOptions::default()
                },
            )
            .map_err(|e| CliError::runtime("export_failed", e.to_string()))?;
            Ok(json!({
                "target": target.as_str(),
                "output_path": report.output_path.display().to_string(),
                "bytes": report.bytes,
                "frame_count": report.frame_count,
                "width": report.width,
                "height": report.height,
                "warnings": report.warnings,
            }))
        }
        ExportTarget::Audio => {
            let inferred_fmt = match spec.audio_format.as_deref() {
                Some(raw) => {
                    AudioFormat::parse(raw).map_err(|e| CliError::usage("invalid_argument", e))?
                }
                None => ext_lower(&spec.out)
                    .and_then(|e| AudioFormat::parse(&e).ok())
                    .unwrap_or(AudioFormat::Mp3),
            };
            let report = neoflash_export::export_project_audio(
                project,
                &spec.out,
                &AudioExportOptions {
                    format: inferred_fmt,
                    ..AudioExportOptions::default()
                },
            )
            .map_err(|e| CliError::runtime("export_failed", e.to_string()))?;
            Ok(json!({
                "target": target.as_str(),
                "output_path": spec.out.display().to_string(),
                "bytes": report.bytes,
                "duration_seconds": report.duration_seconds,
                "sample_rate": report.sample_rate,
                "channels": report.channels,
                "format": report.format.label(),
            }))
        }
    }
}
