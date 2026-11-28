//! Shared import and export handlers delegating directly to `neoflash_import`,
//! `neoflash_export`, and editor media helpers for headless CLI commands and batch pipelines.
use super::effects::HeadlessEffectTransaction;
use super::{
    CliError, OutputFormat, load_project_valid, resolve_layer_index, resolve_layer_index_mut,
    resolve_symbol, resolve_symbol_mut, save_project_atomic, serialize_json_value, take_flag_value,
    upgrade_project_format_versions, write_json_error,
};
use crate::resources::{
    is_audio_import_path, is_font_import_path, read_audio_resource, read_font_resource,
    read_resource, unique_font_resource_path, upsert_imported_font_embedding_metadata,
};
use neoflash_core::{
    Element, ElementKind, FrameSound, GlobalSwatch, InstancePlayback, Keyframe, Layer,
    LayerProperties, LayerRole, MAX_RESOURCE_BYTES, MAX_RESOURCE_TOTAL_BYTES, MAX_RESOURCES,
    Project, ResourceUnit, ScriptLanguage, SoundEffect, SoundSync, Symbol, SymbolKind, Timeline,
    TimelineVideoSpan, Transform, VideoDeploymentMode, VideoSyncMode, flatten_scenes_with_ranges,
    new_id, valid_library_folder,
};
use neoflash_export::{
    AepOptions, AtlasFormat, AtlasOptions, AudioExportOptions, AudioExportPolicy, AudioFormat,
    BannerOptions, FlaCompatibilityReport, FlaExportOptions, FlaTargetProfile, FontExportPolicy,
    HtmlPublishConfig, HtmlScaleMode, HtmlStageAlign, ImageExportFormat, ImageExportOptions,
    ImageExportRange, LottieFormat, LottieImageFormat, LottieImageMode, LottieOptions,
    LottieTextMode, PublishProfile, SizeLevel, SvgOptions, SwfPublishOptions, SwfPublishProfile,
    VideoFormat, VideoOptions, XcodeExportOptions, export_project_aep, export_project_with_profile,
    export_xcode_project, load_profiles_from_project,
};
use neoflash_import::{
    Diagnostic as ImportDiagnostic, FigmaImportOptions, ImageSequenceImportOptions,
    ImageSequencePlacement, ImportError, ImportFormat, ImportReport, PsdImportOptions,
    Severity as ImportSeverity, Statistics as ImportStatistics, SwcConflictPolicy, SwcLinkReport,
    VectorImportOptions, import_act_palette, import_ase_palette, import_clr_palette,
    import_figma_path, import_fxg_str, import_numbered_image_sequence_paths_with_options,
    import_pdf_ai_with_options, import_psd_path, import_svg_with_options, import_swc,
    link_swc_into_project,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::io::{Read as _, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static JPEG_STAGE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// Supported export target identifiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExportTarget {
    Html5,
    Swf,
    Image,
    Sequence,
    Aep,
    Fla,
    Xfl,
    Svg,
    Lottie,
    Atlas,
    Video,
    Audio,
    Banner,
    Desktop,
    Xcode,
}

impl ExportTarget {
    pub(crate) fn parse(raw: &str) -> Result<Self, CliError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "html5" | "html" | "web" => Ok(Self::Html5),
            "swf" => Ok(Self::Swf),
            "image" | "still" => Ok(Self::Image),
            "png-sequence" | "png_sequence" | "sequence" | "image-sequence" | "image_sequence"
            | "images" => Ok(Self::Sequence),
            "aep" | "aepx" | "after-effects" | "after_effects" => Ok(Self::Aep),
            "fla" => Ok(Self::Fla),
            "xfl" => Ok(Self::Xfl),
            "svg" => Ok(Self::Svg),
            "lottie" | "dotlottie" => Ok(Self::Lottie),
            "atlas" | "spritesheet" | "sprite-sheet" => Ok(Self::Atlas),
            "video" => Ok(Self::Video),
            "audio" => Ok(Self::Audio),
            "banner" => Ok(Self::Banner),
            "desktop" | "app" => Ok(Self::Desktop),
            "xcode" | "swift" | "xcode-swift" | "xcode_swift" => Ok(Self::Xcode),
            other => Err(CliError::usage(
                "unsupported_export_target",
                format!(
                    "Unsupported export target '{other}'. Expected one of: html5, swf, image, png-sequence, sequence, aep, fla, xfl, svg, lottie, dotlottie, atlas, video, audio, banner, desktop, xcode"
                ),
            )),
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Html5 => "html5",
            Self::Swf => "swf",
            Self::Image => "image",
            Self::Sequence => "png-sequence",
            Self::Aep => "aep",
            Self::Fla => "fla",
            Self::Xfl => "xfl",
            Self::Svg => "svg",
            Self::Lottie => "lottie",
            Self::Atlas => "atlas",
            Self::Video => "video",
            Self::Audio => "audio",
            Self::Banner => "banner",
            Self::Desktop => "desktop",
            Self::Xcode => "xcode",
        }
    }
}

/// Parameters controlling an export operation from either CLI flags or a JSON `batch` op.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExportSpec {
    #[serde(default)]
    pub(crate) target: String,
    #[serde(default)]
    pub(crate) out: PathBuf,
    #[serde(default)]
    pub(crate) export_profile: Option<PathBuf>,
    #[serde(default)]
    pub(crate) import_profile: Option<PathBuf>,
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
    pub(crate) no_file_fallback: bool,
    #[serde(default)]
    pub(crate) frame_index: Option<u32>,
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
    pub(crate) swf_target: Option<String>,
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
    pub(crate) scale: Option<f32>,
    #[serde(default)]
    pub(crate) width: Option<u32>,
    #[serde(default)]
    pub(crate) height: Option<u32>,
    #[serde(default)]
    pub(crate) transparent: bool,
    #[serde(default)]
    pub(crate) jpeg_quality: Option<u8>,
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
    pub(crate) banner_max_zip: Option<usize>,
    #[serde(default)]
    pub(crate) image_format: Option<String>,
    #[serde(default)]
    pub(crate) html_preview: bool,
    #[serde(default)]
    pub(crate) include_aepx: Option<bool>,
    #[serde(default)]
    pub(crate) include_extendscript: Option<bool>,
    #[serde(default)]
    pub(crate) translate_scripts: Option<bool>,
    #[serde(default)]
    pub(crate) shape_morph_step: Option<u32>,
    #[serde(default)]
    pub(crate) bundle_id_prefix: Option<String>,
    #[serde(default)]
    pub(crate) overwrite_app_shell: bool,
    #[serde(skip)]
    pub(crate) project_base_dir: Option<PathBuf>,
    #[serde(skip)]
    pub(crate) project_source_path: Option<PathBuf>,
}

fn check_export_profile_does_not_alias_source(
    source_project: Option<&Path>,
    spec: &ExportSpec,
) -> Result<(), CliError> {
    if let Some(proj_path) = source_project {
        if let Some(ref prof_out) = spec.export_profile
            && super::paths_refer_to_same_file(proj_path, prof_out)
        {
            return Err(CliError::runtime(
                "source_overwrite_refused",
                format!(
                    "Refusing to overwrite source project '{}' with --export-profile '{}'. Choose a separate .json output path.",
                    proj_path.display(),
                    prof_out.display(),
                ),
            ));
        }
        if !spec.out.as_os_str().is_empty() && super::paths_refer_to_same_file(proj_path, &spec.out)
        {
            return Err(CliError::runtime(
                "source_overwrite_refused",
                format!(
                    "Refusing to overwrite source project '{}' with export output '{}'. Choose a separate --out path.",
                    proj_path.display(),
                    spec.out.display(),
                ),
            ));
        }
    }
    Ok(())
}

/// Register the exact files or owned output trees that one batch export may mutate.
pub(crate) fn plan_export_effects(
    project: &Project,
    spec: &ExportSpec,
    effects: &HeadlessEffectTransaction,
) -> Result<(), CliError> {
    check_export_profile_does_not_alias_source(spec.project_source_path.as_deref(), spec)?;
    if let Some(ref prof_out) = spec.export_profile {
        effects.plan_path(prof_out)?;
    }
    if spec.target.trim().is_empty() && spec.export_profile.is_some() {
        return Ok(());
    }
    let target = ExportTarget::parse(&spec.target)?;
    if spec.out.as_os_str().is_empty() {
        return Err(CliError::usage(
            "missing_flag",
            "'export' requires 'out' / --out <path> when 'target' is specified",
        ));
    }
    let parent = || output_parent(&spec.out);
    match target {
        ExportTarget::Html5 => {
            let (_, _, route) = resolve_publish_profile_and_options(project, spec)?;
            match route {
                BuiltinHtmlRoute::Standard => {
                    neoflash_export::preflight_package_output_dir(&spec.out)
                        .map_err(|error| CliError::runtime("export_failed", error.to_string()))?;
                    effects.plan_tree(&spec.out)?;
                }
                BuiltinHtmlRoute::Criteo => {
                    effects.plan_path(criteo_output(project, &spec.out)?)?
                }
                BuiltinHtmlRoute::BannerNative => plan_banner_effects(&spec.out, effects)?,
                BuiltinHtmlRoute::DesktopApp => plan_desktop_effects(project, &spec.out, effects)?,
            }
        }
        ExportTarget::Desktop => plan_desktop_effects(project, &spec.out, effects)?,
        ExportTarget::Xcode => {
            neoflash_export::preflight_xcode_output_dir(&spec.out)
                .map_err(|error| CliError::runtime("export_failed", error.to_string()))?;
            effects.plan_tree(&spec.out)?;
        }
        ExportTarget::Banner => plan_banner_effects(&spec.out, effects)?,
        ExportTarget::Fla | ExportTarget::Xfl => effects.plan_path(&spec.out)?,
        ExportTarget::Swf | ExportTarget::Audio => effects.plan_path(&spec.out)?,
        ExportTarget::Image => {
            let format = match spec.image_format.as_deref() {
                Some(raw) => parse_image_export_format(raw)?,
                None => match ext_lower(&spec.out).as_deref() {
                    Some(ext) => parse_image_export_format(ext)?,
                    None => ImageExportFormat::Png,
                },
            };
            let path = if format == ImageExportFormat::Jpeg
                && ext_lower(&spec.out).as_deref() == Some("jpeg")
            {
                spec.out.clone()
            } else {
                spec.out.with_extension(format.extension())
            };
            effects.plan_path(path)?;
        }
        ExportTarget::Sequence => {
            let format = match spec.image_format.as_deref() {
                Some(raw) => parse_image_export_format(raw)?,
                None => match ext_lower(&spec.out).as_deref() {
                    Some(ext) => parse_image_export_format(ext).unwrap_or(ImageExportFormat::Png),
                    None => ImageExportFormat::Png,
                },
            };
            let extension = format.extension();
            let base =
                if !spec.out.is_file() && (spec.out.is_dir() || spec.out.extension().is_none()) {
                    spec.out.join(format!("frame.{extension}"))
                } else {
                    spec.out.clone()
                };
            if spec.frame.is_some() && spec.end_frame.is_none() {
                effects.plan_path(base.with_extension(extension))?;
            } else {
                let directory = output_parent(&base);
                let stem = sequence_stem(&base);
                let symbol_id = spec
                    .symbol
                    .as_deref()
                    .map(|selector| {
                        resolve_symbol(project, selector).map(|symbol| symbol.id.as_str())
                    })
                    .transpose()?;
                let range = match spec.frame {
                    Some(start) => ImageExportRange::Frames {
                        start,
                        end: spec.end_frame.unwrap_or(start),
                    },
                    None => ImageExportRange::WholeMovie,
                };
                let (start, end) = range.resolve_frames(project, symbol_id);
                let count = end.saturating_sub(start).saturating_add(1).max(1);
                let planned_names = planned_numbered_names(&stem, extension, count);
                plan_sequence_sidecars_and_prior_members(
                    &directory,
                    &stem,
                    extension,
                    &planned_names,
                    effects,
                )?;
                effects.plan_numbered_files(directory, stem, [extension], true, true)?;
            }
        }
        ExportTarget::Aep => {
            let aep = spec.out.with_extension("aep");
            let builder_requested = spec
                .out
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("jsx"));
            let xml_requested = spec
                .out
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("aepx"));
            let stem = aep
                .file_stem()
                .and_then(|value| value.to_str())
                .filter(|value| !value.is_empty())
                .unwrap_or("project");
            if !builder_requested {
                effects.plan_path(&aep)?;
                if spec.include_aepx.unwrap_or(true) || xml_requested {
                    effects.plan_path(aep.with_extension("aepx"))?;
                }
            }
            if spec.include_extendscript.unwrap_or(true) || builder_requested {
                effects.plan_path(aep.with_extension("jsx"))?;
            }
            effects.plan_tree(parent().join(format!("{stem}_Footage")))?;
        }
        ExportTarget::Svg => {
            let start = spec.frame.unwrap_or(0);
            let end = spec.end_frame.unwrap_or(start).max(start);
            if end == start {
                effects.plan_path(&spec.out)?;
            } else {
                let stem = spec
                    .out
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .filter(|value| !value.is_empty())
                    .unwrap_or("frame");
                let parent = parent();
                for frame in start..=end {
                    effects.plan_path(parent.join(format!("{stem}_{:04}.svg", frame + 1)))?;
                }
            }
        }
        ExportTarget::Lottie => {
            effects.plan_path(&spec.out)?;
            if spec.html_preview {
                effects.plan_path(spec.out.with_extension("preview.html"))?;
            }
            let dotlottie = spec.target.trim().eq_ignore_ascii_case("dotlottie")
                || ext_lower(&spec.out).as_deref() == Some("lottie");
            if spec.assets_folder || (dotlottie && spec.html_preview) {
                effects.plan_tree(parent().join("images"))?;
            }
        }
        ExportTarget::Atlas => {
            effects.plan_path(&spec.out)?;
            let image = spec.out.with_extension("png");
            let stem = image
                .file_stem()
                .and_then(|value| value.to_str())
                .filter(|value| !value.is_empty())
                .unwrap_or("spritesheet");
            effects.plan_numbered_files(parent(), stem, ["png"], true, false)?;
        }
        ExportTarget::Video
            if VideoFormat::from_path(&spec.out) == Some(VideoFormat::PngSequence) =>
        {
            let directory = parent();
            let stem = sequence_stem(&spec.out);
            let symbol_id = spec
                .symbol
                .as_deref()
                .map(|selector| resolve_symbol(project, selector).map(|symbol| symbol.id.as_str()))
                .transpose()?;
            let count = effective_timeline_frame_count(project, symbol_id);
            let raw_stem = spec
                .out
                .file_stem()
                .and_then(|value| value.to_str())
                .filter(|value| !value.is_empty())
                .unwrap_or("frame");
            let mut planned_names =
                if count == 1 && raw_stem == stem && spec.out.extension().is_some() {
                    vec![
                        spec.out
                            .file_name()
                            .and_then(|value| value.to_str())
                            .unwrap_or("frame.png")
                            .to_owned(),
                    ]
                } else {
                    planned_numbered_names(&stem, "png", count)
                };
            // PNG-video may stage mixed audio in the same transaction. Including the companion
            // name conservatively mirrors legacy exact-overlap migration before audio decoding.
            planned_names.push(format!("{stem}.wav"));
            plan_sequence_sidecars_and_prior_members(
                &directory,
                &stem,
                "png",
                &planned_names,
                effects,
            )?;
            effects.plan_numbered_files(directory, stem, ["png", "wav"], true, true)?;
        }
        ExportTarget::Video => effects.plan_path(&spec.out)?,
    }
    Ok(())
}

fn output_parent(path: &Path) -> PathBuf {
    path.parent()
        .filter(|value| !value.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()
}

fn sequence_stem(path: &Path) -> String {
    let raw = path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("frame");
    raw.strip_suffix("_0001")
        .filter(|value| !value.is_empty())
        .unwrap_or(raw)
        .to_owned()
}

fn planned_numbered_names(stem: &str, extension: &str, count: u32) -> Vec<String> {
    let count = count.max(1);
    if count == 1 {
        return vec![format!("{stem}.{extension}")];
    }
    (0..count)
        .map(|index| format!("{stem}_{:04}.{extension}", index + 1))
        .collect()
}

fn plan_sequence_sidecars_and_prior_members(
    directory: &Path,
    stem: &str,
    family: &str,
    planned_names: &[String],
    effects: &HeadlessEffectTransaction,
) -> Result<(), CliError> {
    let Ok((manifest, _)) = neoflash_export::read_sequence_manifest(directory) else {
        return Ok(());
    };
    for (key, entry) in manifest.sequences {
        let stem_matches = entry.stem.eq_ignore_ascii_case(stem)
            || (entry.family.is_none() && key.eq_ignore_ascii_case(stem));
        if !stem_matches {
            continue;
        }
        let family_matches = match entry.family.as_deref() {
            Some(recorded) => recorded.eq_ignore_ascii_case(family),
            None => {
                let exact_overlap = entry.files.iter().any(|owned| {
                    planned_names
                        .iter()
                        .any(|planned| owned.name.eq_ignore_ascii_case(planned))
                });
                exact_overlap || {
                    let mut saw_family = false;
                    entry.files.iter().all(|owned| {
                        let extension = Path::new(&owned.name)
                            .extension()
                            .and_then(|value| value.to_str())
                            .unwrap_or_default();
                        if extension.eq_ignore_ascii_case(family) {
                            saw_family = true;
                            true
                        } else {
                            family.eq_ignore_ascii_case("png")
                                && extension.eq_ignore_ascii_case("wav")
                        }
                    }) && saw_family
                }
            }
        };
        if family_matches {
            for owned in entry.files {
                neoflash_export::validate_sequence_member_name(&owned.name)
                    .map_err(|error| CliError::runtime("export_failed", error.to_string()))?;
                effects.plan_path(directory.join(owned.name))?;
            }
        }
    }
    Ok(())
}

fn plan_desktop_effects(
    project: &Project,
    output: &Path,
    effects: &HeadlessEffectTransaction,
) -> Result<(), CliError> {
    let safe_name = project
        .name
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || matches!(character, '-' | '_' | ' ') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    let safe_name = safe_name.trim();
    let stem = if safe_name.is_empty() {
        "NeoFlashApp"
    } else {
        safe_name
    };
    effects.plan_tree(output.join(format!("{stem}.app")))?;
    effects.plan_path(output.join(format!("Launch {stem}.bat")))?;
    effects.plan_path(output.join(format!("Launch {stem}.sh")))?;
    Ok(())
}

fn criteo_output(project: &Project, output: &Path) -> Result<PathBuf, CliError> {
    if output.is_dir() || output.extension().is_none() {
        return Ok(output.join(crate::output_paths::criteo_archive_name(project)));
    }
    if ext_lower(output).as_deref() != Some("zip") {
        return Err(CliError::usage(
            "invalid_argument",
            "Criteo output must be a .zip file or a directory",
        ));
    }
    Ok(output.to_owned())
}

fn plan_banner_effects(output: &Path, effects: &HeadlessEffectTransaction) -> Result<(), CliError> {
    let directory_target = output.is_dir() || output.extension().is_none();
    let final_path = if directory_target {
        output.join("index.html")
    } else {
        output.to_path_buf()
    };
    effects.plan_path(&final_path)?;
    if final_path
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("index.html"))
    {
        let directory = output_parent(&final_path);
        let marker = directory.join("neoflash-export.json");
        effects.plan_path(&marker)?;
        for relative in read_marker_owned_paths(&marker) {
            effects.plan_path(directory.join(relative))?;
        }
        effects.plan_tree(directory.join("assets"))?;
    }
    Ok(())
}

fn read_marker_owned_paths(marker: &Path) -> Vec<PathBuf> {
    let Ok(metadata) = fs::metadata(marker) else {
        return Vec::new();
    };
    if !metadata.is_file() {
        return Vec::new();
    }
    let Ok(bytes) = fs::read(marker) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
        return Vec::new();
    };
    if value["generator"] != "NeoFlash" || value["format_version"] != 1 {
        return Vec::new();
    }
    let Some(files) = value["files"].as_array() else {
        return Vec::new();
    };
    let mut seen = BTreeSet::new();
    let mut paths = Vec::with_capacity(files.len());
    for file in files {
        let Some(relative) = file.as_str() else {
            return Vec::new();
        };
        if neoflash_core::validate_external_video_path(relative).is_err()
            || !seen.insert(relative.to_ascii_lowercase())
        {
            return Vec::new();
        }
        paths.push(PathBuf::from(relative));
    }
    paths
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
    for env_key in [
        "NEOFLASH_WEB_RUNTIME_DIR",
        "NEOFLASH_WEB_RUNTIME",
        "NEOFLASH_RUNTIME_DIR",
    ] {
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

#[derive(Clone, Debug, Default)]
struct ImportTargetOptions {
    library_only: bool,
    resource_id: Option<String>,
    symbol: Option<String>,
    layer: Option<String>,
    frame: u32,
    folder: String,
    swc_conflict: Option<SwcConflictPolicy>,
    sequence_placement: Option<ImageSequencePlacement>,
    frame_step: Option<u32>,
}

/// Parameters for importing assets into the in-memory project owned by a batch transaction.
///
/// `input` is a convenience spelling for one path; `inputs` mirrors the standalone command's
/// ability to import a numbered image sequence or several independent assets in one step.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BatchImportSpec {
    #[serde(default)]
    input: Option<PathBuf>,
    #[serde(default)]
    inputs: Vec<PathBuf>,
    #[serde(default)]
    library_only: bool,
    #[serde(default, alias = "id", alias = "linkage_id")]
    resource_id: Option<String>,
    #[serde(default)]
    symbol: Option<String>,
    #[serde(default)]
    layer: Option<String>,
    #[serde(default)]
    frame: u32,
    #[serde(default)]
    folder: String,
    #[serde(default)]
    swc_conflict: Option<String>,
    #[serde(default)]
    sequence_placement: Option<String>,
    #[serde(default)]
    frame_step: Option<u32>,
}

/// Import one batch step into the current candidate project without writing a project file.
/// Any explicit save/export remains owned by the outer filesystem transaction.
pub(crate) fn import_project_with_spec(
    project: &mut Project,
    spec: &BatchImportSpec,
) -> Result<Value, CliError> {
    if spec.input.is_some() && !spec.inputs.is_empty() {
        return Err(CliError::usage(
            "conflicting_fields",
            "batch import accepts either 'input' or 'inputs', not both",
        ));
    }
    let inputs = if let Some(input) = &spec.input {
        vec![input.clone()]
    } else {
        spec.inputs.clone()
    };
    if inputs.is_empty() {
        return Err(CliError::usage(
            "missing_field",
            "batch import requires 'input' or a non-empty 'inputs' array",
        ));
    }
    if spec.frame_step == Some(0) {
        return Err(CliError::usage(
            "invalid_argument",
            "batch import frame_step must be >= 1",
        ));
    }

    if spec.resource_id.is_some() && inputs.len() > 1 {
        return Err(CliError::usage(
            "conflicting_fields",
            "batch import 'resource_id' requires a single 'input'",
        ));
    }

    let opts = ImportTargetOptions {
        library_only: spec.library_only,
        resource_id: spec
            .resource_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned),
        symbol: spec.symbol.clone(),
        layer: spec.layer.clone(),
        frame: spec.frame,
        folder: parse_library_folder_flag(&spec.folder)?,
        swc_conflict: spec
            .swc_conflict
            .as_deref()
            .map(parse_swc_conflict_policy)
            .transpose()?,
        sequence_placement: spec
            .sequence_placement
            .as_deref()
            .map(parse_sequence_placement)
            .transpose()?,
        frame_step: spec.frame_step,
    };

    let (report, swc_link) = import_inputs_into_project(project, &inputs, &opts)?;
    finalize_project_format_version(project);
    project
        .validate()
        .map_err(|e| CliError::runtime("validation_failed", e.to_string()))?;

    let mut result = json!({
        "inputs": inputs.iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
        "library_only": opts.library_only,
        "report": normalize_import_report_json(&report, &inputs),
        "project": {
            "name": project.name,
            "format_version": project.format_version,
            "symbols": project.symbols.len(),
            "layers": project.symbols.iter().map(|symbol| symbol.timeline.layers.len()).sum::<usize>(),
            "resources": project.resources.len(),
            "sources": project.sources.len(),
        },
    });
    if let Some(link) = swc_link
        && let Some(object) = result.as_object_mut()
    {
        object.insert("swc_link".to_owned(), json!(link));
    }
    Ok(result)
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
            write_json_error(&err, format, pretty, stdout, stderr);
            err.exit_code
        }
    }
}

fn parse_swc_conflict_policy(raw: &str) -> Result<SwcConflictPolicy, CliError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "prefer-newer" | "prefer_newer" | "newer" => Ok(SwcConflictPolicy::PreferNewer),
        "prefer-existing" | "prefer_existing" | "existing" => Ok(SwcConflictPolicy::PreferExisting),
        "error" | "error-on-conflict" | "error_on_conflict" => {
            Ok(SwcConflictPolicy::ErrorOnConflict)
        }
        other => Err(CliError::usage(
            "invalid_argument",
            format!(
                "Unsupported --swc-conflict '{other}' (expected prefer-newer|prefer-existing|error)"
            ),
        )),
    }
}

fn parse_sequence_placement(raw: &str) -> Result<ImageSequencePlacement, CliError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "single-layer" | "single_layer" | "keyframes" | "keyframes_on_single_layer" => {
            Ok(ImageSequencePlacement::KeyframesOnSingleLayer)
        }
        "separate-layers" | "separate_layers" | "layers" => {
            Ok(ImageSequencePlacement::SeparateLayers)
        }
        "graphic" | "convert-to-graphic-symbol" | "convert_to_graphic_symbol" => {
            Ok(ImageSequencePlacement::ConvertToGraphicSymbol)
        }
        "movieclip" | "movie-clip" | "convert-to-movie-clip" | "convert_to_movie_clip" => {
            Ok(ImageSequencePlacement::ConvertToMovieClip)
        }
        other => Err(CliError::usage(
            "invalid_argument",
            format!(
                "Unsupported --sequence-placement '{other}' (expected single-layer|separate-layers|graphic|movieclip)"
            ),
        )),
    }
}

fn run_cmd_import<W1: Write>(
    args: &[String],
    format: OutputFormat,
    pretty: bool,
    stdout: &mut W1,
) -> Result<(), CliError> {
    let mut inputs: Vec<PathBuf> = Vec::new();
    let mut into_path: Option<PathBuf> = None;
    let mut out_path: Option<PathBuf> = None;
    let mut in_place = false;
    let mut opts = ImportTargetOptions::default();

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
            "--into" | "--project" => {
                into_path = Some(PathBuf::from(take_flag_value(args, &mut i, "--into")?));
            }
            "--out" | "--output" | "-o" => {
                out_path = Some(PathBuf::from(take_flag_value(args, &mut i, "--out")?));
            }
            "--in-place" => in_place = true,
            "--library-only" | "--to-library" => opts.library_only = true,
            "--id" | "--resource-id" | "--linkage-id" => {
                let raw = take_flag_value(args, &mut i, "--resource-id")?;
                let trimmed = raw.trim();
                if trimmed.is_empty() {
                    return Err(CliError::usage(
                        "invalid_argument",
                        "--resource-id cannot be empty",
                    ));
                }
                opts.resource_id = Some(trimmed.to_owned());
            }
            "--symbol" => opts.symbol = Some(take_flag_value(args, &mut i, "--symbol")?),
            "--layer" => opts.layer = Some(take_flag_value(args, &mut i, "--layer")?),
            "--frame" => {
                let raw = take_flag_value(args, &mut i, "--frame")?;
                opts.frame = parse_u32_flag("--frame", &raw)?;
            }
            "--folder" => {
                let raw = take_flag_value(args, &mut i, "--folder")?;
                opts.folder = parse_library_folder_flag(&raw)?;
            }
            "--swc-conflict" => {
                let raw = take_flag_value(args, &mut i, "--swc-conflict")?;
                opts.swc_conflict = Some(parse_swc_conflict_policy(&raw)?);
            }
            "--sequence-placement" => {
                let raw = take_flag_value(args, &mut i, "--sequence-placement")?;
                opts.sequence_placement = Some(parse_sequence_placement(&raw)?);
            }
            "--frame-step" => {
                let raw = take_flag_value(args, &mut i, "--frame-step")?;
                let step = parse_u32_flag("--frame-step", &raw)?;
                if step == 0 {
                    return Err(CliError::usage(
                        "invalid_argument",
                        "--frame-step must be >= 1",
                    ));
                }
                opts.frame_step = Some(step);
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
    if opts.resource_id.is_some() && inputs.len() > 1 {
        return Err(CliError::usage(
            "conflicting_flags",
            "--resource-id requires a single --input file",
        ));
    }
    if out_path.is_some() && in_place {
        return Err(CliError::usage(
            "conflicting_flags",
            "--out and --in-place are mutually exclusive",
        ));
    }
    if in_place && into_path.is_none() {
        return Err(CliError::usage(
            "missing_flag",
            "--in-place requires --into <project.nflash>",
        ));
    }
    let save_target = if in_place {
        into_path.clone().expect("checked")
    } else {
        out_path.ok_or_else(|| {
            CliError::usage(
                "missing_flag",
                "'import' requires --out <project.nflash> (or --into <project.nflash> --in-place)",
            )
        })?
    };

    let (mut project, report, swc_link) = if let Some(ref base_path) = into_path {
        let mut dest = load_project_valid(base_path)?;
        let (rep, link_rep) = import_inputs_into_project(&mut dest, &inputs, &opts)?;
        (dest, rep, link_rep)
    } else {
        import_inputs_standalone(&inputs, &opts)?
    };

    finalize_project_format_version(&mut project);
    project
        .validate()
        .map_err(|e| CliError::runtime("validation_failed", e.to_string()))?;
    save_project_atomic(&mut project, &save_target)?;

    let symbol_count = project.symbols.len();
    let layer_count: usize = project
        .symbols
        .iter()
        .map(|s| s.timeline.layers.len())
        .sum();
    let resource_count = project.resources.len();

    match format {
        OutputFormat::Json => {
            let mut val = json!({
                "ok": true,
                "command": "import",
                "out": save_target.display().to_string(),
                "into": into_path.as_ref().map(|p| p.display().to_string()),
                "in_place": in_place,
                "library_only": opts.library_only,
                "inputs": inputs.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
                "report": normalize_import_report_json(&report, &inputs),
                "project": {
                    "name": project.name,
                    "width": project.width,
                    "height": project.height,
                    "fps": project.fps,
                    "format_version": project.format_version,
                    "symbols": symbol_count,
                    "layers": layer_count,
                    "resources": resource_count,
                    "sources": project.sources.len(),
                },
            });
            if let Some(link) = swc_link
                && let Some(obj) = val.as_object_mut()
            {
                obj.insert("swc_link".to_owned(), json!(link));
            }
            let text = serialize_json_value(&val, pretty)?;
            writeln!(stdout, "{text}").map_err(|e| {
                CliError::runtime("io_error", format!("Failed writing stdout: {e}"))
            })?;
        }
        OutputFormat::Text => {
            writeln!(
                stdout,
                "Imported {} input(s) -> {} ({} symbol(s), {} resource(s), format={:?}, diags={})",
                inputs.len(),
                save_target.display(),
                symbol_count,
                resource_count,
                report.format,
                report.diagnostics.len()
            )
            .map_err(|e| CliError::runtime("io_error", format!("Failed writing stdout: {e}")))?;
        }
    }
    Ok(())
}

pub(super) fn parse_library_folder_flag(raw: &str) -> Result<String, CliError> {
    let trimmed = raw.trim();
    if trimmed.contains("//") {
        return Err(CliError::usage(
            "invalid_argument",
            format!("Invalid --folder path '{raw}': folder path cannot contain empty segments"),
        ));
    }
    let normalized = trimmed.trim_matches('/');
    if !valid_library_folder(normalized) {
        return Err(CliError::usage(
            "invalid_argument",
            format!(
                "Invalid --folder path '{raw}': folder names cannot contain '..', '.', backslashes, control characters, or leading/trailing segment spaces"
            ),
        ));
    }
    Ok(normalized.to_owned())
}

fn finalize_project_format_version(project: &mut Project) {
    upgrade_project_format_versions(project);
}

fn ext_lower(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
}

fn stem_string(path: &Path) -> String {
    let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    if let Some(stripped) = file_name
        .strip_suffix(".figma.json")
        .or_else(|| file_name.strip_suffix(".FIGMA.JSON"))
        && !stripped.trim().is_empty()
    {
        return stripped.to_owned();
    }
    path.file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("Imported")
        .to_owned()
}

fn is_sequence_image_ext(ext: &str) -> bool {
    matches!(ext, "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp")
}

fn is_single_raster_ext(ext: &str) -> bool {
    matches!(
        ext,
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "tif" | "tiff"
    )
}

fn is_supported_video_ext(ext: &str) -> bool {
    matches!(ext, "mp4" | "m4v" | "webm" | "mov" | "ogv" | "avi")
}

fn is_unsupported_video_ext(ext: &str) -> bool {
    matches!(ext, "flv" | "f4v" | "mkv" | "wmv")
}

fn is_internal_metadata_resource(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower == "swatches/global.json"
        || lower == "swatches/custom.json"
        || lower == "publish/profiles.json"
        || lower == "publish/html-render-mode.txt"
}

fn normalize_import_report_json(report: &ImportReport, inputs: &[PathBuf]) -> Value {
    let mut val = json!(report);
    if let Some(obj) = val.as_object_mut() {
        if report.format == ImportFormat::Unknown
            && let Some(ver) = report.version.as_deref()
        {
            let lower = ver.trim().to_ascii_lowercase();
            let normalized = if lower == "audio/wav" || lower == "audio/x-wav" || lower == "wav" {
                Some("wav")
            } else if lower == "audio/mpeg" || lower == "mp3" {
                Some("mp3")
            } else if lower.starts_with("audio/") {
                Some("audio")
            } else if lower.starts_with("font/") {
                Some("font")
            } else if lower.starts_with("video/") {
                Some("video")
            } else if matches!(
                lower.as_str(),
                "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "tif" | "tiff"
            ) || lower.starts_with("image/")
            {
                Some("image")
            } else if matches!(lower.as_str(), "act" | "clr" | "ase") {
                Some("palette")
            } else if lower.starts_with("nflash-") {
                Some("nflash")
            } else {
                None
            };
            if let Some(fmt_label) = normalized {
                obj.insert("format".to_owned(), Value::String(fmt_label.to_owned()));
            }
        }
        if inputs.len() > 1 {
            obj.insert(
                "sources".to_owned(),
                Value::Array(
                    inputs
                        .iter()
                        .map(|p| Value::String(p.display().to_string()))
                        .collect(),
                ),
            );
        }
    }
    val
}

fn assign_explicit_resource_id(
    existing: &[ResourceUnit],
    unit: &mut ResourceUnit,
    explicit: Option<&str>,
) -> Result<(), CliError> {
    if let Some(req) = explicit {
        if existing.iter().any(|r| r.id == req) {
            return Err(CliError::runtime(
                "duplicate_resource_id",
                format!("Resource ID '{req}' already exists in project"),
            ));
        }
        unit.id = req.to_owned();
    }
    Ok(())
}

fn import_err(err: ImportError) -> CliError {
    CliError::runtime("import_failed", err.to_string())
}

fn remaining_resource_budget(project: &Project) -> Result<usize, CliError> {
    if project.resources.len() >= MAX_RESOURCES {
        return Err(CliError::runtime(
            "resource_limit_exceeded",
            format!("Project already has the maximum {MAX_RESOURCES} resources."),
        ));
    }
    let used: usize = project.resources.iter().map(ResourceUnit::byte_len).sum();
    Ok(MAX_RESOURCE_TOTAL_BYTES.saturating_sub(used))
}

fn unique_resource_path(existing: &[ResourceUnit], preferred: &str) -> String {
    let taken: HashSet<String> = existing.iter().map(|r| r.path.to_lowercase()).collect();
    if !taken.contains(&preferred.to_lowercase()) {
        return preferred.to_owned();
    }
    let p = Path::new(preferred);
    let parent = p
        .parent()
        .and_then(|d| d.to_str())
        .filter(|s| !s.is_empty());
    let stem = p
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("resource");
    let ext = p
        .extension()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty());
    for n in 2usize..100_000 {
        let leaf = match ext {
            Some(e) => format!("{stem}_{n}.{e}"),
            None => format!("{stem}_{n}"),
        };
        let candidate = match parent {
            Some(dir) => format!("{dir}/{leaf}"),
            None => leaf,
        };
        if !taken.contains(&candidate.to_lowercase()) {
            return candidate;
        }
    }
    format!("{stem}_{}", new_id("res"))
}

fn resolve_target_symbol_id(
    project: &Project,
    opts: &ImportTargetOptions,
) -> Result<String, CliError> {
    let sym = match opts.symbol.as_deref() {
        Some(sel) => resolve_symbol(project, sel)?,
        None => project.symbol(&project.root).ok_or_else(|| {
            CliError::runtime("symbol_not_found", "Root symbol not found in project")
        })?,
    };
    if let Some(layer_sel) = opts.layer.as_deref() {
        let idx = resolve_layer_index(&sym.timeline, layer_sel)?;
        if sym.timeline.layers[idx].properties.role == LayerRole::Folder {
            return Err(CliError::runtime(
                "invalid_layer",
                format!(
                    "Cannot import onto folder layer '{}'",
                    sym.timeline.layers[idx].name
                ),
            ));
        }
    }
    Ok(sym.id.clone())
}

fn ensure_editable_keyframe_in_symbol<'a>(
    project: &'a mut Project,
    sym_id: &str,
    opts: &ImportTargetOptions,
) -> Result<(usize, &'a mut Keyframe), CliError> {
    let sym = resolve_symbol_mut(project, sym_id)?;
    if opts.frame >= sym.timeline.frame_count {
        sym.timeline.frame_count = opts.frame + 1;
    }
    let layer_idx = match opts.layer.as_deref() {
        Some(sel) => resolve_layer_index_mut(&mut sym.timeline, sel)?,
        None => {
            if let Some(pos) = sym
                .timeline
                .layers
                .iter()
                .position(|l| l.properties.role != LayerRole::Folder)
            {
                pos
            } else {
                sym.timeline.layers.push(Layer {
                    id: new_id("layer"),
                    name: "Layer 1".to_owned(),
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe::default()],
                    properties: LayerProperties::default(),
                });
                sym.timeline.layers.len() - 1
            }
        }
    };
    let layer = &mut sym.timeline.layers[layer_idx];
    if layer.properties.role == LayerRole::Folder {
        return Err(CliError::runtime(
            "invalid_layer",
            format!("Cannot import onto folder layer '{}'", layer.name),
        ));
    }
    if !layer.keyframes.iter().any(|k| k.frame == opts.frame) {
        let prev_elements = layer
            .key_at(opts.frame)
            .map(|k| k.elements.clone())
            .unwrap_or_default();
        layer.keyframes.push(Keyframe {
            frame: opts.frame,
            elements: prev_elements,
            ..Keyframe::default()
        });
        layer.keyframes.sort_by_key(|k| k.frame);
    }
    let kf = layer
        .keyframes
        .iter_mut()
        .find(|k| k.frame == opts.frame)
        .expect("inserted above");
    kf.keyed_properties = None;
    Ok((layer_idx, kf))
}

fn normalize_layer_after_edit(project: &mut Project, sym_id: &str, layer_idx: usize) {
    if let Some(sym) = project.symbol_mut(sym_id)
        && let Some(layer) = sym.timeline.layers.get_mut(layer_idx)
    {
        layer.normalize_motion_keys();
    }
}

fn sequence_options_from_target(opts: &ImportTargetOptions) -> ImageSequenceImportOptions {
    let mut seq_opts = ImageSequenceImportOptions {
        center_on_stage: false,
        ..ImageSequenceImportOptions::default()
    };
    if let Some(placement) = opts.sequence_placement {
        seq_opts.placement = placement;
    }
    if let Some(step) = opts.frame_step {
        seq_opts.frame_step = step.max(1);
    }
    seq_opts
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

fn import_single_raster_into_project(
    project: &mut Project,
    path: &Path,
    opts: &ImportTargetOptions,
    size_project_to_image: bool,
) -> Result<ImportReport, CliError> {
    let target_sym_id = if opts.library_only {
        None
    } else {
        Some(resolve_target_symbol_id(project, opts)?)
    };
    let remaining = remaining_resource_budget(project)?;
    let mut unit =
        read_resource(path, remaining).map_err(|e| CliError::runtime("import_failed", e))?;
    assign_explicit_resource_id(&project.resources, &mut unit, opts.resource_id.as_deref())?;
    unit.path = unique_resource_path(&project.resources, &unit.path);
    let decoded = unit
        .decode_image()
        .map_err(|e| CliError::runtime("import_failed", format!("{}: {e}", path.display())))?;
    let w = decoded.width.max(1);
    let h = decoded.height.max(1);
    if size_project_to_image {
        project.name = stem_string(path);
        project.width = w;
        project.height = h;
    }
    let res_id = unit.id.clone();
    project.resources.push(unit);

    let folder = opts.folder.trim().trim_matches('/').to_owned();
    if !folder.is_empty() {
        let _ = project.add_library_folder(&folder);
    }
    let sym_name = stem_string(path);
    let graphic_sym_id = new_id("sym");
    let bitmap_el = Element {
        id: new_id("bitmap"),
        name: sym_name.clone(),
        transform: Transform::default(),
        kind: ElementKind::Bitmap {
            resource_id: res_id,
            width: w,
            height: h,
            smoothing: true,
        },
        ..Element::default()
    };
    project.symbols.push(Symbol {
        id: graphic_sym_id.clone(),
        name: sym_name,
        folder,
        kind: SymbolKind::Graphic,
        linkage_id: None,
        class_name: None,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![Layer {
                id: new_id("layer"),
                name: "Layer 1".to_owned(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![bitmap_el],
                    ..Keyframe::default()
                }],
                properties: LayerProperties::default(),
            }],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
    });

    if let Some(sym_id) = target_sym_id {
        let tx = if size_project_to_image {
            0.0
        } else {
            (f64::from(project.width) - f64::from(w)) * 0.5
        };
        let ty = if size_project_to_image {
            0.0
        } else {
            (f64::from(project.height) - f64::from(h)) * 0.5
        };
        let (layer_idx, kf) = ensure_editable_keyframe_in_symbol(project, &sym_id, opts)?;
        kf.elements.push(Element {
            id: new_id("instance"),
            name: String::new(),
            transform: Transform {
                x: tx,
                y: ty,
                ..Transform::default()
            },
            kind: ElementKind::Instance {
                symbol_id: graphic_sym_id,
                playback: InstancePlayback::SingleFrame(0),
            },
            ..Element::default()
        });
        normalize_layer_after_edit(project, &sym_id, layer_idx);
    }
    finalize_project_format_version(project);

    Ok(ImportReport {
        source: path.display().to_string(),
        format: ImportFormat::Unknown,
        version: ext_lower(path).map(|e| e.to_ascii_uppercase()),
        can_import: true,
        statistics: ImportStatistics {
            symbols: project.symbols.len(),
            layers: project
                .symbols
                .iter()
                .map(|s| s.timeline.layers.len())
                .sum(),
            keyframes: 1,
            elements: if opts.library_only { 1 } else { 2 },
            scripts: 0,
        },
        diagnostics: Vec::new(),
        binary_fla: None,
        swf: None,
        swc: None,
    })
}

fn import_audio_into_project(
    project: &mut Project,
    path: &Path,
    opts: &ImportTargetOptions,
    standalone_name: bool,
) -> Result<ImportReport, CliError> {
    let target_sym_id = if opts.library_only {
        None
    } else {
        Some(resolve_target_symbol_id(project, opts)?)
    };
    let remaining = remaining_resource_budget(project)?;
    let mut unit =
        read_audio_resource(path, remaining).map_err(|e| CliError::runtime("import_failed", e))?;
    assign_explicit_resource_id(&project.resources, &mut unit, opts.resource_id.as_deref())?;
    unit.path = unique_resource_path(&project.resources, &unit.path);
    if standalone_name {
        project.name = stem_string(path);
    }
    let res_id = unit.id.clone();
    let media_type = unit.media_type.clone();
    if standalone_name && let Some(sym_id) = &target_sym_id {
        use rodio::Source;
        let bytes = unit
            .decoded_bytes()
            .map_err(|e| CliError::runtime("import_failed", e.to_string()))?;
        let audio = rodio::Decoder::new(std::io::Cursor::new(bytes))
            .map_err(|e| CliError::runtime("import_failed", e.to_string()))?;
        let samples_per_second = u64::from(audio.sample_rate()) * u64::from(audio.channels());
        // Count decoded samples without retaining PCM or truncating to the preview budget.
        let frames =
            (audio.count() as u64 * u64::from(project.fps)).div_ceil(samples_per_second.max(1));
        if let Some(sym) = project.symbol_mut(sym_id) {
            sym.timeline.frame_count = opts
                .frame
                .saturating_add(frames.clamp(1, u32::MAX.into()) as u32);
        }
    }
    project.resources.push(unit);

    if let Some(sym_id) = target_sym_id {
        let (layer_idx, kf) = ensure_editable_keyframe_in_symbol(project, &sym_id, opts)?;
        kf.sound = Some(FrameSound {
            resource: res_id,
            sync: if standalone_name {
                SoundSync::Stream
            } else {
                SoundSync::Event
            },
            loops: 1,
            loop_forever: false,
            effect: SoundEffect::None,
            envelope: None,
            in_point: None,
            out_point: None,
        });
        normalize_layer_after_edit(project, &sym_id, layer_idx);
    }
    finalize_project_format_version(project);

    Ok(ImportReport {
        source: path.display().to_string(),
        format: ImportFormat::Unknown,
        version: Some(media_type),
        can_import: true,
        statistics: ImportStatistics {
            symbols: project.symbols.len(),
            layers: project
                .symbols
                .iter()
                .map(|s| s.timeline.layers.len())
                .sum(),
            keyframes: 1,
            elements: 0,
            scripts: 0,
        },
        diagnostics: Vec::new(),
        binary_fla: None,
        swf: None,
        swc: None,
    })
}

fn import_font_into_project(
    project: &mut Project,
    path: &Path,
    standalone_name: bool,
) -> Result<ImportReport, CliError> {
    let remaining = remaining_resource_budget(project)?;
    let mut unit =
        read_font_resource(path, remaining).map_err(|e| CliError::runtime("import_failed", e))?;
    unit.path = unique_font_resource_path(&project.resources, &unit.path);
    if standalone_name {
        project.name = stem_string(path);
    }
    let media_type = unit.media_type.clone();
    project.resources.push(unit.clone());
    upsert_imported_font_embedding_metadata(&mut project.resources, &unit)
        .map_err(|e| CliError::runtime("import_failed", e))?;
    project.format_version = project.format_version.max(3);
    finalize_project_format_version(project);

    Ok(ImportReport {
        source: path.display().to_string(),
        format: ImportFormat::Unknown,
        version: Some(media_type),
        can_import: true,
        statistics: ImportStatistics {
            symbols: project.symbols.len(),
            layers: project
                .symbols
                .iter()
                .map(|s| s.timeline.layers.len())
                .sum(),
            keyframes: project
                .symbols
                .iter()
                .flat_map(|s| &s.timeline.layers)
                .map(|l| l.keyframes.len())
                .sum(),
            elements: 0,
            scripts: 0,
        },
        diagnostics: Vec::new(),
        binary_fla: None,
        swf: None,
        swc: None,
    })
}

fn import_video_into_project(
    project: &mut Project,
    path: &Path,
    opts: &ImportTargetOptions,
    standalone_name: bool,
) -> Result<ImportReport, CliError> {
    let target_sym_id = if opts.library_only {
        None
    } else {
        Some(resolve_target_symbol_id(project, opts)?)
    };
    let remaining = remaining_resource_budget(project)?;
    let byte_limit = MAX_RESOURCE_BYTES.min(remaining);
    let file = fs::File::open(path).map_err(|e| {
        CliError::runtime(
            "io_error",
            format!("Could not read video '{}': {e}", path.display()),
        )
    })?;
    let mut raw_bytes = Vec::new();
    file.take(byte_limit as u64 + 1)
        .read_to_end(&mut raw_bytes)
        .map_err(|e| {
            CliError::runtime(
                "io_error",
                format!("Could not read video '{}': {e}", path.display()),
            )
        })?;
    if raw_bytes.len() > byte_limit {
        return Err(CliError::runtime(
            "resource_limit_exceeded",
            format!(
                "Video '{}' exceeds the remaining project resource budget.",
                path.display()
            ),
        ));
    }

    let ext = ext_lower(path).unwrap_or_else(|| "mp4".to_owned());
    let media_type = if raw_bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        "video/webm"
    } else if raw_bytes.starts_with(b"OggS") {
        "video/ogg"
    } else if neoflash_core::is_avi_bytes(&raw_bytes) {
        "video/x-msvideo"
    } else if raw_bytes.len() >= 12 && &raw_bytes[4..8] == b"ftyp" {
        "video/mp4"
    } else if ext == "webm" {
        "video/webm"
    } else if ext == "ogv" || ext == "ogg" {
        "video/ogg"
    } else if ext == "avi" {
        "video/x-msvideo"
    } else {
        "video/mp4"
    };

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("video.mp4")
        .to_owned();
    let v_diags = neoflash_core::inspect_video_resource(&file_name, media_type, &raw_bytes, None);
    let errors: Vec<&str> = v_diags
        .iter()
        .filter(|d| d.is_error)
        .map(|d| d.message.as_str())
        .collect();
    if !errors.is_empty() {
        return Err(CliError::runtime("import_failed", errors.join(" ")));
    }

    let probe = neoflash_export::decode_video_stream_bytes(&file_name, media_type, &raw_bytes);
    let probe_error = probe.as_ref().err().cloned();
    let probed = probe.ok();
    let (vid_w, vid_h) = probed
        .as_ref()
        .filter(|s| s.width > 0 && s.height > 0)
        .map(|s| (s.width, s.height))
        .unwrap_or((320, 240));
    let duration_sec = probed
        .as_ref()
        .filter(|s| s.duration_ms > 0)
        .map(|s| (f64::from(s.duration_ms) / 1000.0).clamp(0.1, 600.0))
        .unwrap_or(4.0);

    if standalone_name {
        project.name = stem_string(path);
        project.width = vid_w.max(1);
        project.height = vid_h.max(1);
        if let Some(ref s) = probed
            && s.fps.is_finite()
            && s.fps >= 1.0
        {
            project.fps = (s.fps.round() as u32).clamp(1, 120);
        }
    }

    let preferred_rel = format!("video/{file_name}");
    let rel_path = unique_resource_path(&project.resources, &preferred_rel);
    let res_id = new_id("res");
    let resource = ResourceUnit::from_bytes(res_id.clone(), rel_path, media_type, &raw_bytes)
        .map_err(|e| CliError::runtime("import_failed", e.to_string()))?;
    project.resources.push(resource);

    if let Some(sym_id) = target_sym_id {
        let fps = project.fps.max(1);
        let needed_frames = opts
            .frame
            .saturating_add(((duration_sec * f64::from(fps)).ceil() as u32).max(1));
        let tx = if standalone_name {
            0.0
        } else {
            (f64::from(project.width) - f64::from(vid_w)) * 0.5
        };
        let ty = if standalone_name {
            0.0
        } else {
            (f64::from(project.height) - f64::from(vid_h)) * 0.5
        };
        let (layer_idx, kf) = ensure_editable_keyframe_in_symbol(project, &sym_id, opts)?;
        kf.video = Some(TimelineVideoSpan {
            resource: res_id,
            sync: VideoSyncMode::Stream,
            deployment: VideoDeploymentMode::Embedded,
            external_path: None,
            stream_url: None,
            start_offset_ms: 0,
            end_offset_ms: None,
            loop_video: false,
            muted: false,
            cue_points: Vec::new(),
        });
        kf.elements.push(Element {
            id: new_id("video"),
            name: file_name,
            transform: Transform {
                x: tx,
                y: ty,
                ..Transform::default()
            },
            kind: ElementKind::Video {
                width: f64::from(vid_w),
                height: f64::from(vid_h),
            },
            ..Element::default()
        });
        if let Some(sym) = project.symbol_mut(&sym_id) {
            if sym.timeline.frame_count < needed_frames {
                sym.timeline.frame_count = needed_frames;
            }
            if let Some(layer) = sym.timeline.layers.get_mut(layer_idx)
                && let Some(end) = &mut layer.properties.end_frame
                && *end < needed_frames
            {
                *end = needed_frames;
            }
        }
        normalize_layer_after_edit(project, &sym_id, layer_idx);
    }
    finalize_project_format_version(project);

    let mut report_diags: Vec<ImportDiagnostic> = v_diags
        .into_iter()
        .map(|d| ImportDiagnostic {
            severity: if d.is_error {
                ImportSeverity::Error
            } else {
                ImportSeverity::Warning
            },
            code: d.code.to_owned(),
            location: path.display().to_string(),
            message: d.message,
        })
        .collect();
    if let Some(detail) = probe_error {
        report_diags.push(ImportDiagnostic {
            severity: ImportSeverity::Warning,
            code: "video_metadata_unavailable".into(),
            location: path.display().to_string(),
            message: format!("Stored the source video. Size and duration use 320×240 and four seconds because decoding failed: {detail}"),
        });
    }

    Ok(ImportReport {
        source: path.display().to_string(),
        format: ImportFormat::Unknown,
        version: Some(media_type.to_owned()),
        can_import: true,
        statistics: ImportStatistics {
            symbols: project.symbols.len(),
            layers: project
                .symbols
                .iter()
                .map(|s| s.timeline.layers.len())
                .sum(),
            keyframes: 1,
            elements: usize::from(!opts.library_only),
            scripts: 0,
        },
        diagnostics: report_diags,
        binary_fla: None,
        swf: None,
        swc: None,
    })
}

fn as3_source_def_name(path: &str) -> Option<String> {
    let trimmed = path.trim_start_matches('/');
    let rel = trimmed
        .strip_prefix("scripts/")
        .or_else(|| trimmed.strip_prefix("src/"))
        .unwrap_or(trimmed);
    let stem = rel.strip_suffix(".as")?;
    (!stem.is_empty()).then(|| stem.replace('/', "."))
}

fn import_swc_into_project(
    project: &mut Project,
    path: &Path,
    opts: &ImportTargetOptions,
    standalone: bool,
) -> Result<(ImportReport, SwcLinkReport), CliError> {
    let mut lib = import_swc(path).map_err(import_err)?;
    // When `library.swf` contains `DoABC` (`src/<pkg>/<Class>.as`), drop any redundant synthesized
    // `scripts/<pkg>/<Class>.as` stub for the same qualified class so AS3 compilation stays conflict-free.
    let decompiled_defs: BTreeSet<String> = lib
        .project
        .sources
        .iter()
        .filter(|u| u.path.starts_with("src/"))
        .filter_map(|u| as3_source_def_name(&u.path))
        .collect();
    if !decompiled_defs.is_empty() {
        lib.project.sources.retain(|u| {
            !(u.path.starts_with("scripts/")
                && as3_source_def_name(&u.path).is_some_and(|d| decompiled_defs.contains(&d)))
        });
    }

    if standalone {
        project.name = lib.name.clone();
        if !lib.project.sources.is_empty() || !lib.inspection.definitions.is_empty() {
            project.script_language = Some(ScriptLanguage::ActionScript3);
        }
    }
    let policy = opts.swc_conflict.unwrap_or(SwcConflictPolicy::PreferNewer);
    let mut existing_defs: BTreeSet<String> = project
        .symbols
        .iter()
        .filter_map(|s| s.class_name.clone())
        .collect();
    for src in &project.sources {
        if let Some(def) = as3_source_def_name(&src.path) {
            existing_defs.insert(def);
        }
    }
    let incoming_defs: BTreeSet<String> = lib
        .inspection
        .definitions
        .iter()
        .cloned()
        .chain(
            lib.project
                .sources
                .iter()
                .filter_map(|u| as3_source_def_name(&u.path)),
        )
        .collect();

    match policy {
        SwcConflictPolicy::ErrorOnConflict => {
            if let Some(conflict_def) = incoming_defs.iter().find(|d| existing_defs.contains(*d)) {
                return Err(CliError::runtime(
                    "import_failed",
                    format!(
                        "SWC definition conflict on {conflict_def:?} between existing project and {:?}",
                        lib.name
                    ),
                ));
            }
        }
        SwcConflictPolicy::PreferExisting => {
            lib.project.symbols.retain(|s| {
                s.id == lib.project.root
                    || s.class_name
                        .as_deref()
                        .is_none_or(|c| !existing_defs.contains(c))
            });
            lib.project.sources.retain(|u| {
                as3_source_def_name(&u.path).is_none_or(|d| !existing_defs.contains(&d))
            });
        }
        SwcConflictPolicy::PreferNewer => {
            project.sources.retain(|u| {
                as3_source_def_name(&u.path).is_none_or(|d| !incoming_defs.contains(&d))
            });
        }
    }

    let folder = opts.folder.trim().trim_matches('/');
    if !folder.is_empty() {
        let lib_root = lib.project.root.clone();
        for sym in &mut lib.project.symbols {
            if sym.id != lib_root {
                sym.folder = if sym.folder.is_empty() {
                    folder.to_owned()
                } else {
                    format!("{folder}/{}", sym.folder)
                };
            }
        }
    }
    let link_report = link_swc_into_project(project, &lib, policy).map_err(import_err)?;
    if !folder.is_empty() && link_report.linked_symbols > 0 {
        let _ = project.add_library_folder(folder);
    }
    finalize_project_format_version(project);

    let report = ImportReport {
        source: path.display().to_string(),
        format: ImportFormat::Swc,
        version: lib.inspection.swc_version.clone(),
        can_import: true,
        statistics: ImportStatistics {
            symbols: project.symbols.len(),
            layers: project
                .symbols
                .iter()
                .map(|s| s.timeline.layers.len())
                .sum(),
            keyframes: project
                .symbols
                .iter()
                .flat_map(|s| &s.timeline.layers)
                .map(|l| l.keyframes.len())
                .sum(),
            elements: project
                .symbols
                .iter()
                .flat_map(|s| &s.timeline.layers)
                .flat_map(|l| &l.keyframes)
                .map(|k| k.elements.len())
                .sum(),
            scripts: project.sources.len(),
        },
        diagnostics: Vec::new(),
        binary_fla: None,
        swf: None,
        swc: Some(lib.inspection),
    };
    Ok((report, link_report))
}

fn resolve_xfl_import_path(path: &Path) -> PathBuf {
    if ext_lower(path).as_deref() == Some("xfl") && path.is_file() {
        let mut magic = [0u8; 4];
        let is_zip = fs::File::open(path)
            .and_then(|mut f| {
                f.read(&mut magic)
                    .map(|n| n == 4 && magic == *b"PK\x03\x04")
            })
            .unwrap_or(false);
        if !is_zip && let Some(parent) = path.parent() {
            let dir = if parent.as_os_str().is_empty() {
                Path::new(".")
            } else {
                parent
            };
            if dir.join("DOMDocument.xml").is_file() {
                return dir.to_path_buf();
            }
        }
    }
    path.to_path_buf()
}

fn import_document_format_standalone(
    path: &Path,
    opts: &ImportTargetOptions,
) -> Result<(Project, ImportReport), CliError> {
    if path.is_dir() && !path.join("DOMDocument.xml").exists() {
        crate::graphic_import::validate_unique_sequence_frame_numbers(
            std::slice::from_ref(&path),
            true,
        )
        .map_err(|msg| CliError::runtime("duplicate_sequence_frame", msg))?;
        let seq_opts = sequence_options_from_target(opts);
        let imported = import_numbered_image_sequence_paths_with_options(
            std::slice::from_ref(&path),
            true,
            &seq_opts,
        )
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
            "nflash" => {
                let project = load_project_valid(path)?;
                let report = ImportReport {
                    source: path.display().to_string(),
                    format: ImportFormat::Unknown,
                    version: Some(format!("nflash-v{}", project.format_version)),
                    can_import: true,
                    statistics: ImportStatistics {
                        symbols: project.symbols.len(),
                        layers: project
                            .symbols
                            .iter()
                            .map(|s| s.timeline.layers.len())
                            .sum(),
                        keyframes: project
                            .symbols
                            .iter()
                            .flat_map(|s| &s.timeline.layers)
                            .map(|l| l.keyframes.len())
                            .sum(),
                        elements: project
                            .symbols
                            .iter()
                            .flat_map(|s| &s.timeline.layers)
                            .flat_map(|l| &l.keyframes)
                            .map(|k| k.elements.len())
                            .sum(),
                        scripts: project.sources.len(),
                    },
                    diagnostics: Vec::new(),
                    binary_fla: None,
                    swf: None,
                    swc: None,
                };
                return Ok((project, report));
            }
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
            "pdf" | "ai" | "eps" => {
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
            _ => {}
        }
    }

    let resolved = resolve_xfl_import_path(path);
    let project = neoflash_import::import_project(&resolved).map_err(import_err)?;
    let report = neoflash_import::inspect(&resolved).map_err(import_err)?;
    Ok((project, report))
}

fn merge_imported_project_into(
    dest: &mut Project,
    mut imported: Project,
    source_label: &str,
    is_palette_only: bool,
    report: &mut ImportReport,
    opts: &ImportTargetOptions,
) -> Result<(), CliError> {
    let target_sym_id = if is_palette_only || opts.library_only {
        None
    } else {
        Some(resolve_target_symbol_id(dest, opts)?)
    };
    let imported_global = imported.global_swatches();
    if !imported_global.is_empty() {
        let mut merged = dest.global_swatches();
        for sw in imported_global {
            if !merged.iter().any(|existing| existing.name == sw.name) {
                merged.push(sw);
            }
        }
        dest.set_global_swatches(&merged);
    }
    let imported_custom = imported.custom_swatches();
    if !imported_custom.is_empty() {
        let mut merged = dest.custom_swatches();
        for c in imported_custom {
            if !merged.contains(&c) && merged.len() < 256 {
                merged.push(c);
            }
        }
        dest.set_custom_swatches(&merged);
    }
    if is_palette_only {
        finalize_project_format_version(dest);
        return Ok(());
    }

    imported.freshen_all_ids(true, |kind| new_id(&format!("imp-{kind}")));

    // Merge non-metadata resources with resource-budget & case-insensitive path uniqueness.
    let media_resources: Vec<ResourceUnit> = imported
        .resources
        .into_iter()
        .filter(|r| !is_internal_metadata_resource(&r.path))
        .collect();
    if dest.resources.len() + media_resources.len() > MAX_RESOURCES {
        return Err(CliError::runtime(
            "resource_limit_exceeded",
            format!("Importing resources would exceed the {MAX_RESOURCES}-resource project limit."),
        ));
    }
    let incoming_bytes: usize = media_resources.iter().map(ResourceUnit::byte_len).sum();
    let remaining = remaining_resource_budget(dest)?;
    if incoming_bytes > remaining {
        return Err(CliError::runtime(
            "resource_limit_exceeded",
            "Imported resources exceed the project's 64 MiB resource budget.",
        ));
    }
    for mut res in media_resources {
        res.path = unique_resource_path(&dest.resources, &res.path);
        dest.resources.push(res);
    }

    // Merge ActionScript/JS/TS source units cleanly.
    if !imported.sources.is_empty() {
        dest.enable_program();
        for mut src in imported.sources {
            if dest
                .sources
                .iter()
                .any(|existing| existing.path == src.path && existing.source == src.source)
            {
                continue;
            }
            if dest
                .sources
                .iter()
                .any(|existing| existing.path.eq_ignore_ascii_case(&src.path))
            {
                let p = Path::new(&src.path);
                let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("Source");
                let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("as");
                let parent = p
                    .parent()
                    .and_then(|d| d.to_str())
                    .filter(|s| !s.is_empty());
                let mut n = 2usize;
                loop {
                    let candidate = match parent {
                        Some(dir) => format!("{dir}/{stem}_{n}.{ext}"),
                        None => format!("{stem}_{n}.{ext}"),
                    };
                    if !dest
                        .sources
                        .iter()
                        .any(|e| e.path.eq_ignore_ascii_case(&candidate))
                    {
                        src.path = candidate;
                        break;
                    }
                    n += 1;
                }
            }
            src.id = new_id("src");
            dest.sources.push(src);
        }
    }

    // Merge symbols (turning imported root into a library MovieClip symbol and deduplicating linkage/class_name).
    let mut existing_linkages: HashSet<String> = dest
        .symbols
        .iter()
        .filter_map(|s| s.linkage_id.clone())
        .collect();
    let mut existing_exported_classes: HashSet<String> = dest
        .symbols
        .iter()
        .filter(|s| s.linkage_id.is_some())
        .filter_map(|s| s.class_name.clone())
        .collect();

    let imported_root_id = imported.root.clone();
    let folder = opts.folder.trim().trim_matches('/');
    if !folder.is_empty() && !imported.symbols.is_empty() {
        let _ = dest.add_library_folder(folder);
    }
    for mut sym in imported.symbols {
        if sym.id == imported_root_id {
            sym.kind = SymbolKind::MovieClip;
            if sym.name.eq_ignore_ascii_case("Scene 1")
                || sym.name.eq_ignore_ascii_case("Main Timeline")
                || sym.name.trim().is_empty()
            {
                sym.name = source_label.to_owned();
            }
            sym.linkage_id = None;
            sym.class_name = None;
        } else {
            if let Some(ref lid) = sym.linkage_id
                && (!matches!(sym.kind, SymbolKind::MovieClip | SymbolKind::Button)
                    || !existing_linkages.insert(lid.clone()))
            {
                report.diagnostics.push(ImportDiagnostic {
                    severity: ImportSeverity::Warning,
                    code: "duplicate_linkage_cleared".to_owned(),
                    location: sym.name.clone(),
                    message: format!(
                        "Cleared duplicate or non-exportable linkage identifier '{lid}' during --into merge."
                    ),
                });
                sym.linkage_id = None;
            }
            if sym.linkage_id.is_some()
                && let Some(ref cls) = sym.class_name
                && !existing_exported_classes.insert(cls.clone())
            {
                report.diagnostics.push(ImportDiagnostic {
                    severity: ImportSeverity::Warning,
                    code: "duplicate_class_cleared".to_owned(),
                    location: sym.name.clone(),
                    message: format!(
                        "Cleared duplicate exported class '{cls}' during --into merge."
                    ),
                });
                sym.class_name = None;
            }
        }
        if !folder.is_empty() {
            sym.folder = if sym.folder.is_empty() {
                folder.to_owned()
            } else {
                format!("{folder}/{}", sym.folder)
            };
        }
        dest.symbols.push(sym);
    }

    if let Some(sym_id) = target_sym_id
        && dest.symbol(&imported_root_id).is_some()
    {
        let (layer_idx, kf) = ensure_editable_keyframe_in_symbol(dest, &sym_id, opts)?;
        kf.elements.push(Element {
            id: new_id("instance"),
            name: String::new(),
            transform: Transform::default(),
            kind: ElementKind::Instance {
                symbol_id: imported_root_id,
                playback: InstancePlayback::Independent,
            },
            ..Element::default()
        });
        normalize_layer_after_edit(dest, &sym_id, layer_idx);
    }

    dest.format_version = dest.format_version.max(imported.format_version);
    finalize_project_format_version(dest);
    Ok(())
}

fn import_single_input_into_project(
    dest: &mut Project,
    path: &Path,
    opts: &ImportTargetOptions,
    standalone_blank: bool,
) -> Result<(ImportReport, Option<SwcLinkReport>), CliError> {
    if is_audio_import_path(path) {
        let rep = import_audio_into_project(dest, path, opts, standalone_blank)?;
        return Ok((rep, None));
    }
    if is_font_import_path(path) {
        let rep = import_font_into_project(dest, path, standalone_blank)?;
        return Ok((rep, None));
    }

    if let Some(ext) = ext_lower(path) {
        if is_unsupported_video_ext(&ext) {
            return Err(CliError::runtime(
                "unsupported_video_format",
                format!(
                    "Video container '.{ext}' is not supported. Convert to .mp4, .webm, .mov, .ogv, or .avi."
                ),
            ));
        }
        if is_supported_video_ext(&ext) {
            let rep = import_video_into_project(dest, path, opts, standalone_blank)?;
            return Ok((rep, None));
        }
        if ext == "swc" {
            let (rep, link_rep) = import_swc_into_project(dest, path, opts, standalone_blank)?;
            return Ok((rep, Some(link_rep)));
        }
        if is_single_raster_ext(&ext) {
            // Check numbered sequence first when >= 2 numbered frames exist in the directory.
            if is_sequence_image_ext(&ext) {
                crate::graphic_import::validate_unique_sequence_frame_numbers(
                    std::slice::from_ref(&path),
                    true,
                )
                .map_err(|msg| CliError::runtime("duplicate_sequence_frame", msg))?;
                let seq_opts = sequence_options_from_target(opts);
                if let Some(imported_seq) = import_numbered_image_sequence_paths_with_options(
                    std::slice::from_ref(&path),
                    true,
                    &seq_opts,
                )
                .map_err(import_err)?
                {
                    let mut rep = imported_seq.report;
                    if standalone_blank {
                        *dest = imported_seq.project;
                        apply_folder_to_library_symbols(dest, &opts.folder);
                        finalize_project_format_version(dest);
                    } else {
                        let label = stem_string(path);
                        merge_imported_project_into(
                            dest,
                            imported_seq.project,
                            &label,
                            false,
                            &mut rep,
                            opts,
                        )?;
                    }
                    return Ok((rep, None));
                }
            }
            let rep = import_single_raster_into_project(dest, path, opts, standalone_blank)?;
            return Ok((rep, None));
        }
    }

    let is_palette = ext_lower(path)
        .as_deref()
        .is_some_and(|e| matches!(e, "act" | "clr" | "ase"));
    let (mut imported, mut report) = import_document_format_standalone(path, opts)?;
    if standalone_blank {
        apply_folder_to_library_symbols(&mut imported, &opts.folder);
        *dest = imported;
        finalize_project_format_version(dest);
    } else {
        let label = stem_string(path);
        merge_imported_project_into(dest, imported, &label, is_palette, &mut report, opts)?;
    }
    Ok((report, None))
}

fn apply_folder_to_library_symbols(project: &mut Project, folder: &str) {
    let trimmed = folder.trim().trim_matches('/');
    if trimmed.is_empty() {
        return;
    }
    let root_id = project.root.clone();
    let scene_ids: HashSet<String> = project.scenes.iter().cloned().collect();
    let mut moved_any = false;
    for sym in &mut project.symbols {
        if sym.id != root_id && !scene_ids.contains(&sym.id) {
            sym.folder = if sym.folder.is_empty() {
                trimmed.to_owned()
            } else {
                format!("{trimmed}/{}", sym.folder)
            };
            moved_any = true;
        }
    }
    if moved_any {
        let _ = project.add_library_folder(trimmed);
    }
}

fn import_inputs_standalone(
    inputs: &[PathBuf],
    opts: &ImportTargetOptions,
) -> Result<(Project, ImportReport, Option<SwcLinkReport>), CliError> {
    if inputs.len() > 1 {
        let all_seq_candidates = inputs
            .iter()
            .all(|p| ext_lower(p).as_deref().is_some_and(is_sequence_image_ext));
        if all_seq_candidates {
            crate::graphic_import::validate_unique_sequence_frame_numbers(inputs, true)
                .map_err(|msg| CliError::runtime("duplicate_sequence_frame", msg))?;
            let seq_opts = sequence_options_from_target(opts);
            if let Some(imported) =
                import_numbered_image_sequence_paths_with_options(inputs, true, &seq_opts)
                    .map_err(import_err)?
            {
                let mut proj = imported.project;
                apply_folder_to_library_symbols(&mut proj, &opts.folder);
                finalize_project_format_version(&mut proj);
                return Ok((proj, imported.report, None));
            }
        }
        let mut project = Project::blank();
        project.name = stem_string(&inputs[0]);
        let (rep, link_rep) = import_inputs_into_project(&mut project, inputs, opts)?;
        return Ok((project, rep, link_rep));
    }

    let mut project = Project::blank();
    let (rep, link_rep) = import_single_input_into_project(&mut project, &inputs[0], opts, true)?;
    Ok((project, rep, link_rep))
}

fn import_inputs_into_project(
    dest: &mut Project,
    inputs: &[PathBuf],
    opts: &ImportTargetOptions,
) -> Result<(ImportReport, Option<SwcLinkReport>), CliError> {
    if inputs.len() > 1 {
        let all_seq_candidates = inputs
            .iter()
            .all(|p| ext_lower(p).as_deref().is_some_and(is_sequence_image_ext));
        if all_seq_candidates {
            crate::graphic_import::validate_unique_sequence_frame_numbers(inputs, true)
                .map_err(|msg| CliError::runtime("duplicate_sequence_frame", msg))?;
            let seq_opts = sequence_options_from_target(opts);
            if let Some(imported) =
                import_numbered_image_sequence_paths_with_options(inputs, true, &seq_opts)
                    .map_err(import_err)?
            {
                let mut rep = imported.report;
                let label = stem_string(&inputs[0]);
                merge_imported_project_into(dest, imported.project, &label, false, &mut rep, opts)?;
                return Ok((rep, None));
            }
        }
    }

    let mut combined_report: Option<ImportReport> = None;
    let mut last_swc_link: Option<SwcLinkReport> = None;
    for path in inputs {
        let (rep, link_rep) = import_single_input_into_project(dest, path, opts, false)?;
        if let Some(l) = link_rep {
            last_swc_link = Some(l);
        }
        match combined_report {
            None => combined_report = Some(rep),
            Some(ref mut acc) => {
                acc.diagnostics.extend(rep.diagnostics);
                acc.statistics.symbols = dest.symbols.len();
                acc.statistics.layers = dest.symbols.iter().map(|s| s.timeline.layers.len()).sum();
                acc.statistics.scripts = dest.sources.len();
                if acc.swc.is_none() {
                    acc.swc = rep.swc;
                }
            }
        }
    }
    let mut final_report = combined_report.expect("inputs non-empty");
    final_report.statistics.symbols = dest.symbols.len();
    final_report.statistics.layers = dest.symbols.iter().map(|s| s.timeline.layers.len()).sum();
    Ok((final_report, last_swc_link))
}

/// Legacy helper kept `pub(crate)` for unit tests calling `import_paths_with_neoflash_import`.
#[allow(dead_code)]
pub(crate) fn import_paths_with_neoflash_import(
    inputs: &[PathBuf],
) -> Result<(Project, ImportReport), CliError> {
    if inputs.is_empty() {
        return Err(CliError::usage("missing_input", "Missing input path"));
    }
    let (project, report, _) = import_inputs_standalone(inputs, &ImportTargetOptions::default())?;
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
            write_json_error(&err, format, pretty, stdout, stderr);
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
            "--export-profile" => {
                let raw = take_flag_value(args, &mut i, "--export-profile")?;
                if raw.trim().is_empty() {
                    return Err(CliError::usage(
                        "invalid_argument",
                        "--export-profile requires a non-empty .json output path",
                    ));
                }
                spec.export_profile = Some(PathBuf::from(raw));
            }
            "--import-profile" => {
                let raw = take_flag_value(args, &mut i, "--import-profile")?;
                if raw.trim().is_empty() {
                    return Err(CliError::usage(
                        "invalid_argument",
                        "--import-profile requires a non-empty .json input path",
                    ));
                }
                spec.import_profile = Some(PathBuf::from(raw));
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
            "--no-sprite-sheets" => spec.sprite_sheets = false,
            "--assets-folder" => spec.assets_folder = true,
            "--no-file-fallback" | "--omit-file-fallback" => spec.no_file_fallback = true,
            "--frame-index" => {
                let raw = take_flag_value(args, &mut i, "--frame-index")?;
                spec.frame_index = Some(parse_u32_flag("--frame-index", &raw)?);
            }
            "--scale-mode" => {
                spec.scale_mode = Some(take_flag_value(args, &mut i, "--scale-mode")?);
            }
            "--stage-align" => {
                spec.stage_align = Some(take_flag_value(args, &mut i, "--stage-align")?);
            }
            "--fullscreen" => spec.allow_fullscreen = Some(true),
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
            "--swf-target" => {
                spec.swf_target = Some(take_flag_value(args, &mut i, "--swf-target")?);
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
            "--scale" => {
                let raw = take_flag_value(args, &mut i, "--scale")?;
                let parsed = raw.parse::<f32>().map_err(|_| {
                    CliError::usage("invalid_argument", format!("Invalid --scale value '{raw}'"))
                })?;
                if !parsed.is_finite() || parsed <= 0.0 {
                    return Err(CliError::usage(
                        "invalid_argument",
                        "--scale must be a finite number > 0",
                    ));
                }
                spec.scale = Some(parsed);
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
            "--jpeg-quality" => {
                let raw = take_flag_value(args, &mut i, "--jpeg-quality")?;
                let q = raw.parse::<u8>().map_err(|_| {
                    CliError::usage(
                        "invalid_argument",
                        format!("Invalid --jpeg-quality value '{raw}' (expected 1..=100)"),
                    )
                })?;
                if !(1..=100).contains(&q) {
                    return Err(CliError::usage(
                        "invalid_argument",
                        "--jpeg-quality must be in 1..=100",
                    ));
                }
                spec.jpeg_quality = Some(q);
            }
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
            "--banner-max-zip" => {
                let raw = take_flag_value(args, &mut i, "--banner-max-zip")?;
                spec.banner_max_zip = Some(raw.parse::<usize>().map_err(|_| {
                    CliError::usage(
                        "invalid_argument",
                        format!("Invalid --banner-max-zip value '{raw}'"),
                    )
                })?);
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
            "--no-aepx" => spec.include_aepx = Some(false),
            "--no-jsx" => spec.include_extendscript = Some(false),
            "--no-translate-scripts" => spec.translate_scripts = Some(false),
            "--shape-morph-step" => {
                let raw = take_flag_value(args, &mut i, "--shape-morph-step")?;
                let step = parse_u32_flag("--shape-morph-step", &raw)?;
                if step == 0 {
                    return Err(CliError::usage(
                        "invalid_argument",
                        "--shape-morph-step must be >= 1",
                    ));
                }
                spec.shape_morph_step = Some(step);
            }
            "--bundle-id-prefix" => {
                spec.bundle_id_prefix = Some(take_flag_value(args, &mut i, "--bundle-id-prefix")?);
            }
            "--overwrite-app-shell" => {
                spec.overwrite_app_shell = true;
            }
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
    if !has_target && spec.export_profile.is_none() {
        return Err(CliError::usage(
            "missing_flag",
            "'export' requires --target <html5|swf|image|png-sequence|sequence|aep|fla|xfl|svg|lottie|dotlottie|atlas|video|audio|banner|desktop|xcode> (or --export-profile <file.json>)",
        ));
    }
    if has_target && (!has_out || spec.out.as_os_str().is_empty()) {
        return Err(CliError::usage(
            "missing_flag",
            "'export' requires --out <path>",
        ));
    }

    spec.project_source_path = Some(project_path.clone());
    check_export_profile_does_not_alias_source(Some(&project_path), &spec)?;

    spec.project_base_dir = project_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf);

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
        "original" | "preserve" => Ok(AudioExportPolicy::Original),
        "disabled" | "strip" | "none" => Ok(AudioExportPolicy::Disabled),
        "wav16" | "wav-16" | "wav_16" | "wav" => Ok(AudioExportPolicy::Wav16),
        "mp3_64" | "mp3-64" | "64" => Ok(AudioExportPolicy::Mp3_64),
        "mp3_96" | "mp3-96" | "96" => Ok(AudioExportPolicy::Mp3_96),
        "mp3" | "mp3_128" | "mp3-128" | "128" => Ok(AudioExportPolicy::Mp3_128),
        "mp3_160" | "mp3-160" | "160" => Ok(AudioExportPolicy::Mp3_160),
        "mp3_192" | "mp3-192" | "192" => Ok(AudioExportPolicy::Mp3_192),
        "mp3_256" | "mp3-256" | "256" => Ok(AudioExportPolicy::Mp3_256),
        "mp3_320" | "mp3-320" | "320" => Ok(AudioExportPolicy::Mp3_320),
        other => Err(CliError::usage(
            "invalid_argument",
            format!(
                "Unsupported --audio-policy '{other}' (expected original|preserve|strip|disabled|wav16|mp3-64..mp3-320)"
            ),
        )),
    }
}

fn parse_scale_mode(raw: &str) -> Result<HtmlScaleMode, CliError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "fit" | "fit_keep_ratio" | "fit-keep-ratio" | "showall" | "show-all" | "show_all" => {
            Ok(HtmlScaleMode::FitKeepRatio)
        }
        "noscale" | "no_scale" | "no-scale" => Ok(HtmlScaleMode::NoScale),
        "exactfit" | "exact_fit" | "exact-fit" => Ok(HtmlScaleMode::ExactFit),
        "fill" | "fill_crop" | "fill-crop" | "noborder" | "no-border" | "no_border" => {
            Ok(HtmlScaleMode::FillCrop)
        }
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

fn parse_swf_target(raw: &str) -> Result<(SwfPublishProfile, bool, &'static str), CliError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "fp8" | "flash8" | "avm1" => Ok((SwfPublishProfile::FlashPlayer8Avm1, false, "fp8")),
        "fp9" | "flash9" | "avm2_9" => Ok((SwfPublishProfile::FlashPlayer9Avm2, false, "fp9")),
        "fp10" | "flash10" | "avm2" => Ok((SwfPublishProfile::FlashPlayer10Avm2, false, "fp10")),
        "fp10-scriptless" | "fp10_scriptless" | "flash10-scriptless" | "scriptless" => Ok((
            SwfPublishProfile::FlashPlayer10Avm2,
            true,
            "fp10-scriptless",
        )),
        other => Err(CliError::usage(
            "invalid_argument",
            format!("Unsupported SWF target '{other}' (expected fp8|fp9|fp10|fp10-scriptless)"),
        )),
    }
}

fn parse_image_export_format(raw: &str) -> Result<ImageExportFormat, CliError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "png" => Ok(ImageExportFormat::Png),
        "jpg" | "jpeg" => Ok(ImageExportFormat::Jpeg),
        "webp" => Ok(ImageExportFormat::Webp),
        "gif" => Ok(ImageExportFormat::Gif),
        "bmp" => Ok(ImageExportFormat::Bmp),
        "psd" => Ok(ImageExportFormat::Psd),
        "pdf" => Ok(ImageExportFormat::Pdf),
        "ai" => Ok(ImageExportFormat::Ai),
        "svg" => Ok(ImageExportFormat::Svg),
        other => Err(CliError::usage(
            "invalid_argument",
            format!(
                "Unsupported image format '{other}' (expected png|jpeg|webp|gif|bmp|psd|pdf|ai|svg)"
            ),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BuiltinHtmlRoute {
    Standard,
    BannerNative,
    Criteo,
    DesktopApp,
}

fn normalize_profile_key(raw: &str) -> String {
    raw.trim()
        .to_lowercase()
        .replace('·', " ")
        .replace(['-', '_', '/'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn resolve_builtin_profile(
    selector: &str,
    runtime_url: Option<&str>,
) -> Result<Option<(PublishProfile, BuiltinHtmlRoute)>, CliError> {
    let key = normalize_profile_key(selector);
    let mut profile = PublishProfile::default();
    match key.as_str() {
        "default" | "profile default" => {
            profile.name = "Default".to_owned();
            profile.options.size = SizeLevel::Full;
            Ok(Some((profile, BuiltinHtmlRoute::Standard)))
        }
        "full" | "web full" | "high quality" | "preview debug" => {
            profile.id = format!("builtin-{}", key.replace(' ', "-"));
            profile.name = selector.trim().to_owned();
            profile.options.size = SizeLevel::Full;
            Ok(Some((profile, BuiltinHtmlRoute::Standard)))
        }
        "balanced" | "web balanced" => {
            profile.id = "builtin-balanced".to_owned();
            profile.name = "Balanced".to_owned();
            profile.options.size = SizeLevel::Balanced;
            profile.options.audio = AudioExportPolicy::Mp3_160;
            Ok(Some((profile, BuiltinHtmlRoute::Standard)))
        }
        "compact" | "web compact" => {
            profile.id = "builtin-compact".to_owned();
            profile.name = "Compact".to_owned();
            profile.options.size = SizeLevel::Compact;
            profile.options.fonts = FontExportPolicy::Subset;
            profile.options.audio = AudioExportPolicy::Mp3_128;
            Ok(Some((profile, BuiltinHtmlRoute::Standard)))
        }
        "shared" | "web shared" => {
            profile.id = "builtin-shared".to_owned();
            profile.name = "Shared".to_owned();
            profile.options.size = SizeLevel::parse("shared", runtime_url)
                .map_err(|e| CliError::usage("invalid_argument", e.to_string()))?;
            Ok(Some((profile, BuiltinHtmlRoute::Standard)))
        }
        "web app dom" | "dom" => {
            profile.id = "builtin-web-app-dom".to_owned();
            profile.name = "Web App · DOM".to_owned();
            profile.options.size = SizeLevel::Balanced;
            profile.render_mode = neoflash_core::HtmlRenderMode::Dom;
            Ok(Some((profile, BuiltinHtmlRoute::Standard)))
        }
        "web banner native" | "native banner" => {
            profile.id = "builtin-web-banner-native".to_owned();
            profile.name = "Web Banner · Native".to_owned();
            profile.options.size = SizeLevel::Compact;
            profile.options.fonts = FontExportPolicy::Subset;
            profile.options.audio = AudioExportPolicy::Mp3_128;
            Ok(Some((profile, BuiltinHtmlRoute::BannerNative)))
        }
        "criteo" | "criteo html5 zip" | "builtin criteo" => {
            profile.id = "builtin-criteo".to_owned();
            profile.name = crate::workspace::PublishProfilePreset::Criteo
                .label()
                .to_owned();
            profile.options.size = SizeLevel::Compact;
            profile.options.fonts = FontExportPolicy::Subset;
            profile.options.audio = AudioExportPolicy::Mp3_128;
            Ok(Some((profile, BuiltinHtmlRoute::Criteo)))
        }
        "desktop app" | "desktop" => {
            profile.id = "builtin-desktop-app".to_owned();
            profile.name = "Desktop app".to_owned();
            profile.options.size = SizeLevel::Full;
            Ok(Some((profile, BuiltinHtmlRoute::DesktopApp)))
        }
        _ => Ok(None),
    }
}

fn resolve_publish_profile_and_options(
    project: &Project,
    spec: &ExportSpec,
) -> Result<(PublishProfile, bool, BuiltinHtmlRoute), CliError> {
    let (stored_profiles, active_id) = load_profiles_from_project(project)
        .map_err(|e| CliError::runtime("export_failed", e.to_string()))?;

    let (mut profile, has_stored_or_preset, route) = if let Some(ref import_path) =
        spec.import_profile
    {
        let raw_json = fs::read_to_string(import_path).map_err(|e| {
            CliError::runtime(
                "io_error",
                format!(
                    "Failed reading --import-profile '{}': {e}",
                    import_path.display()
                ),
            )
        })?;
        let imported = neoflash_export::import_profile_json(&raw_json)
            .map_err(|e| CliError::runtime("invalid_profile_json", e.to_string()))?;
        (imported, true, BuiltinHtmlRoute::Standard)
    } else if let Some(ref selector) = spec.profile {
        if let Some(found) = stored_profiles
            .into_iter()
            .find(|p| p.id == *selector || p.name.eq_ignore_ascii_case(selector))
        {
            (found, true, BuiltinHtmlRoute::Standard)
        } else if let Some((builtin, builtin_route)) =
            resolve_builtin_profile(selector, spec.runtime_url.as_deref())?
        {
            (builtin, true, builtin_route)
        } else {
            return Err(CliError::runtime(
                "profile_not_found",
                format!(
                    "Publish profile '{selector}' not found in project or built-in presets (Default, Full, Balanced, Compact, Shared, Web App · DOM, Web Banner · Native, Criteo · HTML5 ZIP, Desktop app)"
                ),
            ));
        }
    } else if let Some(ref act_id) = active_id {
        if let Some(found) = stored_profiles
            .iter()
            .find(|p| p.id == *act_id)
            .cloned()
            .or_else(|| stored_profiles.into_iter().next())
        {
            (found, true, BuiltinHtmlRoute::Standard)
        } else {
            (PublishProfile::default(), false, BuiltinHtmlRoute::Standard)
        }
    } else if let Some(first) = stored_profiles.into_iter().next() {
        (first, true, BuiltinHtmlRoute::Standard)
    } else {
        (PublishProfile::default(), false, BuiltinHtmlRoute::Standard)
    };

    if let Some(ref lvl) = spec.size_level {
        profile.options.size = SizeLevel::parse(lvl, spec.runtime_url.as_deref())
            .map_err(|e| CliError::usage("invalid_argument", e.to_string()))?;
    } else if !has_stored_or_preset {
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
    let route = if profile.criteo_max_zip_kib.is_some() {
        BuiltinHtmlRoute::Criteo
    } else {
        route
    };
    Ok((profile, has_stored_or_preset, route))
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
    let host_override = spec.clean
        || spec.no_file_fallback
        || spec.scale_mode.is_some()
        || spec.stage_align.is_some()
        || spec.allow_fullscreen.is_some()
        || parsed_render_mode.is_some();
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
    if spec.no_file_fallback {
        cfg.omit_file_fallback = true;
    }
    Ok((cfg, host_override))
}

fn fla_report_json(report: &FlaCompatibilityReport) -> Value {
    compatibility_report_json_with_label(report, report.profile.label())
}

fn compatibility_report_json_with_label(
    report: &FlaCompatibilityReport,
    profile_label: &str,
) -> Value {
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
        "profile": profile_label,
        "blocking_count": report.blocking_items().len(),
        "item_count": items.len(),
        "items": items,
    })
}

fn effective_timeline_frame_count(project: &Project, symbol_id: Option<&str>) -> u32 {
    if let Some(id) = symbol_id {
        project
            .symbol(id)
            .map_or(1, |s| s.timeline.frame_count.max(1))
    } else {
        let (flat, _) = flatten_scenes_with_ranges(project);
        flat.symbol(&flat.root)
            .map_or(1, |s| s.timeline.frame_count.max(1))
    }
}

/// Export a single JPEG to the exact requested `.jpeg` path without touching the sibling `.jpg`.
///
/// `export_project_image` normalizes JPEG paths to `.jpg`. Passing a user-requested `foo.jpeg`
/// directly would therefore replace `foo.jpg` first, then require a non-portable rename over
/// `foo.jpeg`. Stage the normalized output in a private sibling directory and atomically publish
/// only the requested destination instead.
fn export_single_jpeg_to_exact_path(
    project: &Project,
    output: &Path,
    options: &ImageExportOptions,
) -> Result<neoflash_export::ImageExportReport, CliError> {
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|e| {
        CliError::runtime(
            "io_error",
            format!(
                "Failed creating output directory '{}': {e}",
                parent.display()
            ),
        )
    })?;

    let stage_dir = (0..64)
        .find_map(|_| {
            let sequence = JPEG_STAGE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let candidate = parent.join(format!(
                ".neoflash-jpeg-stage-{}-{sequence}",
                std::process::id()
            ));
            match fs::create_dir(&candidate) {
                Ok(()) => Some(Ok(candidate)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => None,
                Err(error) => Some(Err(error)),
            }
        })
        .transpose()
        .map_err(|e| {
            CliError::runtime(
                "io_error",
                format!("Failed creating a temporary JPEG directory: {e}"),
            )
        })?
        .ok_or_else(|| {
            CliError::runtime(
                "io_error",
                "Failed reserving a unique temporary JPEG directory",
            )
        })?;

    let staged_request = stage_dir.join("output.jpeg");
    let result = (|| {
        let mut report = neoflash_export::export_project_image(project, &staged_request, options)
            .map_err(|e| CliError::runtime("export_failed", e.to_string()))?;
        let staged_file = report.files.first().ok_or_else(|| {
            CliError::runtime("export_failed", "JPEG export produced no output file")
        })?;
        let bytes = fs::read(staged_file).map_err(|e| {
            CliError::runtime(
                "io_error",
                format!(
                    "Failed reading staged JPEG '{}': {e}",
                    staged_file.display()
                ),
            )
        })?;
        crate::windows_platform::atomic_replace_file(output, &bytes).map_err(|e| {
            CliError::runtime(
                "io_error",
                format!("Failed publishing JPEG '{}': {e}", output.display()),
            )
        })?;
        report.files.clear();
        report.files.push(output.to_path_buf());
        Ok(report)
    })();
    let _ = fs::remove_dir_all(&stage_dir);
    result
}

fn isolate_symbol_for_html5_export(project: &Project, sym_id: &str) -> Project {
    let mut cloned = project.clone();
    cloned.script_language = Some(project.effective_script_language());
    let selected_kind = project.symbol(sym_id).map(|symbol| symbol.kind);
    // Runtime roots receive `Project::document_class`; `Symbol::class_name` is used only when a
    // symbol is instantiated below the root. Promoting a library symbol/secondary scene therefore
    // also promotes its class binding, while isolating an unbound symbol must not run the original
    // document class against unrelated artwork.
    if sym_id != project.root && selected_kind != Some(SymbolKind::Button) {
        cloned.document_class = project
            .symbol(sym_id)
            .and_then(|symbol| symbol.class_name.clone());
    }
    cloned
        .symbols
        .retain(|s| s.id == sym_id || !project.is_scene(&s.id));
    cloned.scenes.clear();

    if selected_kind == Some(SymbolKind::Button) {
        // A document root must be a MovieClip, while a bound Button class must extend
        // SimpleButton. Preserve the selected Button and instantiate it under a deterministic
        // one-frame wrapper instead of mutating its kind and applying an incompatible document
        // class to the wrapper.
        cloned.document_class = None;
        let mut occupied = crate::clipboard::collect_occupied_project_ids(&cloned);
        let mut reserve_id = |base: &str| {
            let mut suffix = 1usize;
            let mut candidate = base.to_owned();
            while occupied.contains(&candidate) {
                suffix += 1;
                candidate = format!("{base}-{suffix}");
            }
            occupied.insert(candidate.clone());
            candidate
        };
        let wrapper_id = reserve_id(&format!("{sym_id}-export-root"));
        let layer_id = reserve_id(&format!("{wrapper_id}-layer"));
        let instance_id = reserve_id(&format!("{wrapper_id}-instance"));
        cloned.symbols.push(Symbol {
            id: wrapper_id.clone(),
            name: project
                .symbol(sym_id)
                .map(|symbol| format!("{} — export root", symbol.name))
                .unwrap_or_else(|| "Export root".into()),
            folder: String::new(),
            kind: SymbolKind::MovieClip,
            linkage_id: None,
            class_name: None,
            timeline: Timeline {
                frame_count: 1,
                layers: vec![Layer {
                    id: layer_id,
                    name: "Export root".into(),
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![Element {
                            id: instance_id,
                            name: String::new(),
                            transform: Transform::default(),
                            kind: ElementKind::Instance {
                                symbol_id: sym_id.to_owned(),
                                playback: InstancePlayback::Independent,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    }],
                    properties: LayerProperties::default(),
                }],
                scripts: Vec::new(),
                guides: Vec::new(),
            },
        });
        cloned.root = wrapper_id;
    } else {
        cloned.root = sym_id.to_owned();
        if let Some(root_sym) = cloned.symbol_mut(sym_id) {
            root_sym.kind = SymbolKind::MovieClip;
            root_sym.folder.clear();
            root_sym.linkage_id = None;
        }
    }
    finalize_project_format_version(&mut cloned);
    cloned
}

/// Shared export dispatcher used by both `neoflash-editor --headless export` and `batch` `export` steps.
pub(crate) fn export_project_with_spec(
    project: &Project,
    spec: &ExportSpec,
) -> Result<Value, CliError> {
    check_export_profile_does_not_alias_source(spec.project_source_path.as_deref(), spec)?;
    project
        .validate()
        .map_err(|e| CliError::runtime("validation_failed", e.to_string()))?;
    let hydrated = crate::app::standalone_script::hydrate_project_includes_from_disk(
        project,
        spec.project_base_dir.as_deref(),
    );
    let resolved_project = hydrated.with_resolved_script_language();
    let project = resolved_project.as_ref();

    let exported_profile_summary = if let Some(ref prof_out) = spec.export_profile {
        let (profile, _, _) = resolve_publish_profile_and_options(project, spec)?;
        let json_str = neoflash_export::export_profile_json(&profile)
            .map_err(|e| CliError::runtime("export_failed", e.to_string()))?;
        crate::windows_platform::atomic_replace_file(prof_out, json_str.as_bytes()).map_err(
            |e| {
                CliError::runtime(
                    "io_error",
                    format!(
                        "Failed writing --export-profile '{}': {e}",
                        prof_out.display()
                    ),
                )
            },
        )?;
        Some((prof_out.display().to_string(), profile.name, profile.id))
    } else {
        None
    };

    if spec.target.trim().is_empty()
        && let Some((out_str, prof_name, prof_id)) = exported_profile_summary
    {
        return Ok(json!({
            "target": "publish-profile",
            "output_path": out_str,
            "exported_profile_path": out_str,
            "profile": prof_name,
            "profile_id": prof_id,
        }));
    }

    let target = ExportTarget::parse(&spec.target)?;
    if spec.out.as_os_str().is_empty() {
        return Err(CliError::usage(
            "missing_flag",
            "'export' requires --out <path>",
        ));
    }
    let mut target_summary = export_target_with_spec_inner(project, spec, target)?;
    if let (Some((prof_path, _, _)), Some(obj)) =
        (exported_profile_summary, target_summary.as_object_mut())
    {
        obj.insert("exported_profile_path".into(), Value::String(prof_path));
    }
    Ok(target_summary)
}

fn export_target_with_spec_inner(
    project: &Project,
    spec: &ExportSpec,
    target: ExportTarget,
) -> Result<Value, CliError> {
    let symbol_id = match spec.symbol.as_deref() {
        Some(sel) => Some(resolve_symbol(project, sel)?.id.clone()),
        None => None,
    };

    if spec.width == Some(0) || spec.height == Some(0) {
        return Err(CliError::usage(
            "invalid_argument",
            "width and height must be >= 1",
        ));
    }
    if let (Some(start), Some(end)) = (spec.frame, spec.end_frame)
        && end < start
    {
        return Err(CliError::usage(
            "invalid_argument",
            format!("--end-frame ({end}) must be >= --frame ({start})"),
        ));
    }
    if let Some(scale) = spec.scale
        && (!scale.is_finite() || scale <= 0.0)
    {
        return Err(CliError::usage(
            "invalid_argument",
            "scale must be a finite number > 0",
        ));
    }
    if let Some(q) = spec.jpeg_quality
        && !(1..=100).contains(&q)
    {
        return Err(CliError::usage(
            "invalid_argument",
            "jpeg_quality must be in 1..=100",
        ));
    }

    match target {
        ExportTarget::Html5 => {
            let isolated_symbol_project = symbol_id
                .as_deref()
                .map(|sym_id| isolate_symbol_for_html5_export(project, sym_id));
            let project = isolated_symbol_project.as_ref().unwrap_or(project);
            let (profile, _has_stored, route) = resolve_publish_profile_and_options(project, spec)?;
            match route {
                BuiltinHtmlRoute::Criteo => {
                    let path = criteo_output(project, &spec.out)?;
                    let budget = spec
                        .banner_max_zip
                        .or_else(|| profile.criteo_max_zip_kib.map(|kib| kib as usize * 1024))
                        .unwrap_or(neoflash_export::DEFAULT_CRITEO_MAX_ZIP_BYTES);
                    let report = neoflash_export::export_criteo_banner_zip(project, &path, budget)
                        .map_err(|e| CliError::runtime("export_failed", e.to_string()))?;
                    return Ok(json!({
                        "target": target.as_str(),
                        "profile": profile.name,
                        "render_mode": "criteo-native",
                        "output_path": path,
                        "html_bytes": report.html_bytes,
                        "zip_bytes": report.zip_bytes,
                    }));
                }
                BuiltinHtmlRoute::BannerNative => {
                    let mut opts = BannerOptions::default();
                    if let Some(max_gz) = spec.banner_max_gzip {
                        opts.max_gzip_bytes = Some(max_gz);
                    }
                    let report =
                        neoflash_export::export_native_banner_html(project, &spec.out, &opts)
                            .map_err(|e| CliError::runtime("export_failed", e.to_string()))?;
                    return Ok(json!({
                        "target": target.as_str(),
                        "profile": profile.name,
                        "render_mode": "native-banner",
                        "output_path": report.html_path.display().to_string(),
                        "raw_bytes": report.raw_bytes,
                        "gzip_bytes": report.gzip_bytes,
                        "frame_count": report.frame_count,
                    }));
                }
                BuiltinHtmlRoute::DesktopApp => {
                    let runtime_dir = resolve_web_runtime(spec.runtime.as_deref())?;
                    let (cfg, _) = build_html_publish_config(spec, true)?;
                    let app_dir = neoflash_export::export_desktop_app(
                        project,
                        &spec.out,
                        &runtime_dir,
                        &profile.options,
                        &cfg,
                    )
                    .map_err(|e| CliError::runtime("export_failed", e.to_string()))?;
                    return Ok(json!({
                        "target": target.as_str(),
                        "profile": profile.name,
                        "output_path": app_dir.display().to_string(),
                        "runtime_dir": runtime_dir.display().to_string(),
                    }));
                }
                BuiltinHtmlRoute::Standard => {}
            }

            let runtime_dir = resolve_web_runtime(spec.runtime.as_deref())?;
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
            let (mut cfg, host_override) = build_html_publish_config(spec, false)?;
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
            let use_custom_cfg = (host_override
                || spec.sprite_sheets
                || spec.assets_folder
                || spec.no_file_fallback)
                && !(explicit_render_mode == Some(neoflash_core::HtmlRenderMode::Canvas)
                    && cfg == HtmlPublishConfig::default());
            let index = if use_custom_cfg {
                let wrapper = profile.html_template.as_deref().filter(|_| !host_override);
                neoflash_export::export_project_with_config_and_wrapper(
                    effective_project,
                    &spec.out,
                    &runtime_dir,
                    &profile.options,
                    &cfg,
                    wrapper,
                )
            } else if !host_override
                && (spec.profile.is_some()
                    || profile.html_template.is_some()
                    || profile.render_mode != neoflash_core::HtmlRenderMode::Canvas)
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
            let isolated_symbol_project = symbol_id
                .as_deref()
                .map(|sym_id| isolate_symbol_for_html5_export(project, sym_id));
            let project = isolated_symbol_project.as_ref().unwrap_or(project);
            let runtime_dir = resolve_web_runtime(spec.runtime.as_deref())?;
            let (profile, _, _) = resolve_publish_profile_and_options(project, spec)?;
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
            let isolated_symbol_project = symbol_id
                .as_deref()
                .map(|sym_id| isolate_symbol_for_html5_export(project, sym_id));
            let project = isolated_symbol_project.as_ref().unwrap_or(project);
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
            let target_slug = if let Some(ref raw) = spec.swf_target {
                let (prof, sl, slug) = parse_swf_target(raw)?;
                opts.profile = prof;
                opts.scriptless_mode = sl || spec.scriptless;
                if opts.scriptless_mode && prof == SwfPublishProfile::FlashPlayer10Avm2 {
                    "fp10-scriptless"
                } else {
                    slug
                }
            } else if let Some(ref raw) = spec.swf_profile {
                let (prof, sl, slug) = parse_swf_target(raw)?;
                opts.profile = prof;
                opts.scriptless_mode = sl || spec.scriptless;
                if opts.scriptless_mode && prof == SwfPublishProfile::FlashPlayer10Avm2 {
                    "fp10-scriptless"
                } else {
                    slug
                }
            } else if matches!(
                project.effective_script_language(),
                ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
            ) {
                opts.profile = SwfPublishProfile::FlashPlayer8Avm1;
                opts.scriptless_mode = spec.scriptless;
                "fp8"
            } else {
                opts.profile = SwfPublishProfile::FlashPlayer10Avm2;
                opts.scriptless_mode = spec.scriptless;
                if opts.scriptless_mode {
                    "fp10-scriptless"
                } else {
                    "fp10"
                }
            };
            opts.outline_text = spec.outline_text || spec.text_as_outlines;
            let report = neoflash_export::publish_swf(project, &spec.out, &opts)
                .map_err(|e| CliError::runtime("export_failed", e.to_string()))?;
            Ok(json!({
                "target": target.as_str(),
                "output_path": spec.out.display().to_string(),
                "swf_target": target_slug,
                "swf_version": opts.profile.swf_version(),
                "scriptless": opts.scriptless_mode,
                "compatibility": compatibility_report_json_with_label(&report, opts.profile.label()),
            }))
        }
        ExportTarget::Image => {
            let img_fmt = match spec.image_format.as_deref() {
                Some(raw) => parse_image_export_format(raw)?,
                None => match ext_lower(&spec.out).as_deref() {
                    Some(ext) => parse_image_export_format(ext)?,
                    None => ImageExportFormat::Png,
                },
            };
            let total_frames = effective_timeline_frame_count(project, symbol_id.as_deref());
            if spec.frame.is_some() && spec.frame_index.is_some() {
                return Err(CliError::usage(
                    "conflicting_flags",
                    "--frame and --frame-index are mutually exclusive",
                ));
            }
            let frame_zero_based = if let Some(idx) = spec.frame_index {
                if idx >= total_frames {
                    return Err(CliError::runtime(
                        "frame_out_of_range",
                        format!(
                            "0-based --frame-index {idx} exceeds timeline range 0..{total_frames}"
                        ),
                    ));
                }
                idx
            } else {
                match spec.frame {
                    Some(0) => {
                        return Err(CliError::usage(
                            "invalid_argument",
                            "--frame for '--target image' is 1-based (>= 1); pass --frame 1 or 0-based --frame-index 0",
                        ));
                    }
                    Some(one_based) => {
                        if one_based > total_frames {
                            return Err(CliError::runtime(
                                "frame_out_of_range",
                                format!(
                                    "1-based --frame {one_based} exceeds timeline length ({total_frames} frame(s))"
                                ),
                            ));
                        }
                        one_based - 1
                    }
                    None => 0,
                }
            };
            let mut opts = ImageExportOptions::for_project(project, frame_zero_based);
            opts.format = img_fmt;
            opts.symbol_id = symbol_id;
            opts.range = ImageExportRange::CurrentFrame(frame_zero_based);
            if let Some(w) = spec.width {
                opts.width = w;
            }
            if let Some(h) = spec.height {
                opts.height = h;
            }
            if let Some(scale) = spec.scale {
                opts.scale = scale;
            }
            opts.transparent_background = spec.transparent;
            if let Some(q) = spec.jpeg_quality {
                opts.jpeg_quality = q;
            }
            if spec.outline_text || spec.text_as_outlines {
                opts.svg_text_as_outlines = true;
                opts.ai_text_as_outlines = true;
            }
            let report = if img_fmt == ImageExportFormat::Jpeg
                && ext_lower(&spec.out).as_deref() == Some("jpeg")
            {
                export_single_jpeg_to_exact_path(project, &spec.out, &opts)?
            } else {
                neoflash_export::export_project_image(project, &spec.out, &opts)
                    .map_err(|e| CliError::runtime("export_failed", e.to_string()))?
            };
            let primary_path = report
                .files
                .first()
                .cloned()
                .unwrap_or_else(|| spec.out.clone());

            // Evaluate visible non-folder/non-guide layers and rendered elements/paths on the
            // exported frame so raster formats (`png`/`jpeg`/`webp`/`gif`/`bmp`) never report
            // hardcoded `layer_count: 1, path_count: 0` (`[FO-14]`).
            let eval_proj_cow: std::borrow::Cow<'_, Project> =
                if opts.symbol_id.is_none() && !project.scenes.is_empty() {
                    std::borrow::Cow::Owned(flatten_scenes_with_ranges(project).0)
                } else {
                    std::borrow::Cow::Borrowed(project)
                };
            let eval_sym_id = opts
                .symbol_id
                .as_deref()
                .unwrap_or(eval_proj_cow.root.as_str());
            let evaluated_layer_count = eval_proj_cow.symbol(eval_sym_id).map_or(1, |sym| {
                sym.timeline
                    .layers
                    .iter()
                    .filter(|layer| {
                        !matches!(
                            layer.properties.role,
                            neoflash_core::LayerRole::Folder | neoflash_core::LayerRole::Guide
                        ) && neoflash_core::layer_effectively_visible(&sym.timeline, layer)
                    })
                    .count()
            });
            let evaluated_scene = neoflash_core::evaluate_scene(
                eval_proj_cow.as_ref(),
                eval_sym_id,
                frame_zero_based,
                false,
                true,
            );
            let element_count = evaluated_scene.draws.len();
            let outline_text_active = opts.svg_text_as_outlines || opts.ai_text_as_outlines;
            let evaluated_path_count = evaluated_scene
                .draws
                .iter()
                .filter(|draw| {
                    matches!(
                        draw.item.kind,
                        ElementKind::Rectangle { .. }
                            | ElementKind::Ellipse { .. }
                            | ElementKind::Path { .. }
                            | ElementKind::PaintedPath { .. }
                    ) || (outline_text_active
                        && matches!(
                            draw.item.kind,
                            ElementKind::Text { .. } | ElementKind::TextBox { .. }
                        ))
                })
                .count();
            let effective_layer_count = match img_fmt {
                ImageExportFormat::Svg | ImageExportFormat::Ai | ImageExportFormat::Psd => {
                    report.layer_count.max(evaluated_layer_count)
                }
                _ => evaluated_layer_count,
            };
            let effective_path_count = if report.path_count > 0 {
                report.path_count
            } else {
                evaluated_path_count
            };

            Ok(json!({
                "target": target.as_str(),
                "output_path": primary_path.display().to_string(),
                "files": report.files.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
                "frame": frame_zero_based + 1,
                "frame_index": frame_zero_based,
                "total_bytes": report.total_bytes,
                "width": report.width,
                "height": report.height,
                "layer_count": effective_layer_count,
                "path_count": effective_path_count,
                "element_count": element_count,
                "format": report.format.label(),
            }))
        }
        ExportTarget::Sequence => {
            let img_fmt = match spec.image_format.as_deref() {
                Some(raw) => parse_image_export_format(raw)?,
                None => match ext_lower(&spec.out).as_deref() {
                    Some(ext) => parse_image_export_format(ext).unwrap_or(ImageExportFormat::Png),
                    None => ImageExportFormat::Png,
                },
            };
            let base_out =
                if !spec.out.is_file() && (spec.out.is_dir() || spec.out.extension().is_none()) {
                    spec.out.join(format!("frame.{}", img_fmt.extension()))
                } else {
                    spec.out.clone()
                };
            let mut opts = ImageExportOptions::for_project(project, spec.frame.unwrap_or(0));
            opts.format = img_fmt;
            opts.symbol_id = symbol_id.clone();
            if let Some(w) = spec.width {
                opts.width = w;
            }
            if let Some(h) = spec.height {
                opts.height = h;
            }
            if let Some(scale) = spec.scale {
                opts.scale = scale;
            }
            opts.transparent_background = spec.transparent;
            if let Some(q) = spec.jpeg_quality {
                opts.jpeg_quality = q;
            }
            opts.range = if let Some(start) = spec.frame {
                let end = spec.end_frame.unwrap_or(start);
                if end == start && spec.end_frame.is_none() {
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
            let report = neoflash_export::export_project_image(project, &base_out, &opts)
                .map_err(|e| CliError::runtime("export_failed", e.to_string()))?;
            Ok(json!({
                "target": target.as_str(),
                "output_path": spec.out.display().to_string(),
                "files": report.files.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
                "frame_count": report.frame_count,
                "total_bytes": report.total_bytes,
                "width": report.width,
                "height": report.height,
                "format": report.format.label(),
            }))
        }
        ExportTarget::Aep => {
            let opts = AepOptions {
                symbol: symbol_id,
                use_native_writer: true,
                include_aepx: spec.include_aepx.unwrap_or(true),
                include_extendscript: spec.include_extendscript.unwrap_or(true),
                translate_scripts: spec.translate_scripts.unwrap_or(true),
                jpeg_quality: spec.jpeg_quality.unwrap_or(80),
                shape_morph_step: spec.shape_morph_step.unwrap_or(1).max(1),
                base_dir: spec.project_base_dir.clone(),
            };
            let report = export_project_aep(project, &spec.out, &opts)
                .map_err(|e| CliError::runtime("export_failed", e.to_string()))?;
            Ok(json!({
                "target": target.as_str(),
                "output_path": report.output_path,
                "aep_path": report.aep_path,
                "aepx_path": report.aepx_path.map(|p| p.display().to_string()),
                "jsx_path": report.jsx_path.map(|p| p.display().to_string()),
                "footage_dir": report.footage_dir.display().to_string(),
                "footage_files": report.footage_files.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
                "composition_count": report.composition_count,
                "vector_shape_layer_count": report.vector_shape_layer_count,
                "text_layer_count": report.text_layer_count,
                "precomp_layer_count": report.precomp_layer_count,
                "footage_layer_count": report.footage_layer_count,
                "expression_count": report.expression_count,
                "marker_count": report.marker_count,
                "diagnostics": report.diagnostics,
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
                text_as_outlines: spec.text_as_outlines || spec.outline_text,
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
                let end_0 = spec.end_frame.unwrap_or(start_0);
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
                    start_frame: spec.frame.unwrap_or(0),
                    end_frame: spec.end_frame.or(spec.frame),
                    frame_width: spec.width,
                    frame_height: spec.height,
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
        ExportTarget::Video => {
            let fmt = VideoFormat::from_path(&spec.out).unwrap_or(VideoFormat::Mp4);
            let report = neoflash_export::export_project_video(
                project,
                &spec.out,
                &VideoOptions {
                    format: fmt,
                    codec: fmt.default_codec(),
                    symbol: symbol_id,
                    from_frame: spec.frame.unwrap_or(0).saturating_add(1),
                    to_frame: spec.end_frame.or(spec.frame).map(|f| f.saturating_add(1)),
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
                "source_summary": report.source_summary,
                "sample_rate": report.sample_rate,
                "channels": report.channels,
                "format": report.format.label(),
            }))
        }
        ExportTarget::Xcode => {
            let isolated_symbol_project = symbol_id
                .as_deref()
                .map(|sym_id| isolate_symbol_for_html5_export(project, sym_id));
            let effective_project = isolated_symbol_project.as_ref().unwrap_or(project);
            let mut opts = XcodeExportOptions {
                overwrite_app_shell: spec.overwrite_app_shell,
                base_dir: spec.project_base_dir.clone(),
                ..XcodeExportOptions::default()
            };
            if let Some(q) = spec.jpeg_quality {
                opts.jpeg_quality = q;
            }
            if let Some(ref prefix) = spec.bundle_id_prefix {
                let trimmed = prefix.trim();
                if !trimmed.is_empty() {
                    opts.bundle_identifier_prefix = trimmed.to_owned();
                }
            }
            let report = export_xcode_project(effective_project, &spec.out, &opts, None)
                .map_err(|e| CliError::runtime("export_failed", e.to_string()))?;
            let appended_count = report.appended_host_methods.len();
            Ok(json!({
                "target": target.as_str(),
                "experimental": true,
                "output_path": report.xcodeproj_path.display().to_string(),
                "output_dir": report.project_dir.display().to_string(),
                "xcodeproj_path": report.xcodeproj_path.display().to_string(),
                "pbxproj_path": report.pbxproj_path.display().to_string(),
                "scheme_name": report.scheme_name,
                "target_name": report.scheme_name,
                "bundle_id_prefix": opts.bundle_identifier_prefix,
                "bundle_identifier": report.bundle_identifier,
                "generated_swift_files": report.generated_swift_files.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
                "app_swift_files": report.app_swift_files.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
                "preserved_app_files": report.preserved_app_files.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
                "copied_resources": report.resource_files.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
                "appended_host_methods": report.appended_host_methods,
                "appended_host_method_count": appended_count,
                "host_method_count": report.host_method_count,
                "symbol_count": report.symbol_count,
                "preflight_count": report.preflight_items.len(),
                "preflight_items": report.preflight_items,
            }))
        }
    }
}
