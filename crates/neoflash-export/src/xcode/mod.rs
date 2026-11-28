//! Self-contained Xcode + Swift 6 (`@MainActor`) Scene & Animation Exporter (`T-455`).
//!
//! Exports a NeoFlash [`Project`] into a native multiplatform (macOS 15+ / iOS 17+) Xcode project
//! with strict `Generated/` vs developer-owned `App/` separation so repeated exports from NeoFlash
//! never overwrite developer Swift code while automatically appending newly added
//! `SwiftHostProtocol` stubs to `App/AppSwiftHost.swift`.

mod app_shell;
mod audio_swift;
mod data_swift;
mod pbxproj;
mod preflight;
mod renderer_swift;
mod runtime_swift;
mod scene_codegen;
#[cfg(test)]
mod tests;
mod value_swift;

pub use preflight::{XcodePreflightItem, XcodePreflightSeverity, preflight_xcode_project};

use crate::fla_preflight::is_internal_provenance_resource;
use crate::{
    ExportError, apply_bitmap_export_settings, atomic_write, bitmap_encode::encode_rgba_png,
    font_family_for_resource, is_font_resource, publishable,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use neoflash_core::{
    ObjectEvent, Project, ResourceUnit, sfnt_view_bytes, validate_external_video_path,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

/// Parameter descriptor of a typed `SwiftHostProtocol` method (`T-455`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct XcodeSwiftProxyParam {
    pub name: String,
    pub swift_type: String,
}

/// Method signature in `Generated/Scenes/SwiftHostContract.swift` (`T-455`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct XcodeSwiftProxyMethod {
    pub name: String,
    pub params: Vec<XcodeSwiftProxyParam>,
    pub return_swift_type: Option<String>,
}

/// Converted Swift 6 script payload for one `Symbol` timeline (`T-455`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct XcodeSymbolScriptBundle {
    pub symbol_id: String,
    pub swift_class_name: String,
    #[serde(default)]
    pub is_linked_source_class: bool,
    pub properties_swift: String,
    pub frame_methods_swift: BTreeMap<u32, String>,
    pub helper_methods_swift: String,
    pub instance_handlers_swift: Vec<(String, ObjectEvent, String)>,
}

/// Complete Swift 6 script bundle passed to [`export_xcode_project`] (`T-455`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct XcodeScriptBundle {
    pub host_methods: Vec<XcodeSwiftProxyMethod>,
    pub symbols: Vec<XcodeSymbolScriptBundle>,
    pub source_classes_swift: String,
}

fn default_bundle_id_prefix() -> String {
    "com.neoflash.generated".to_owned()
}

const fn default_jpeg_quality() -> u8 {
    85
}

/// Configuration for [`export_xcode_project`] (`T-455`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct XcodeExportOptions {
    #[serde(default = "default_bundle_id_prefix")]
    pub bundle_identifier_prefix: String,
    #[serde(default)]
    pub overwrite_app_shell: bool,
    #[serde(default = "default_jpeg_quality")]
    pub jpeg_quality: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_dir: Option<PathBuf>,
}

impl Default for XcodeExportOptions {
    fn default() -> Self {
        Self {
            bundle_identifier_prefix: default_bundle_id_prefix(),
            overwrite_app_shell: false,
            jpeg_quality: default_jpeg_quality(),
            base_dir: None,
        }
    }
}

/// Summary of a completed Xcode project export (`T-455`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct XcodeExportReport {
    pub project_dir: PathBuf,
    pub xcodeproj_path: PathBuf,
    pub pbxproj_path: PathBuf,
    pub info_plist_path: PathBuf,
    pub scheme_name: String,
    pub bundle_identifier: String,
    pub generated_swift_files: Vec<PathBuf>,
    pub app_swift_files: Vec<PathBuf>,
    pub preserved_app_files: Vec<PathBuf>,
    pub appended_host_methods: Vec<String>,
    #[serde(default)]
    pub host_method_count: usize,
    pub resource_files: Vec<PathBuf>,
    pub symbol_count: usize,
    pub preflight_items: Vec<XcodePreflightItem>,
}

/// Exports `project` to a native Apple Xcode + Swift 6 (`@MainActor`) project inside `out_dir` (`T-455`).
pub fn export_xcode_project(
    project: &Project,
    out_dir: &Path,
    options: &XcodeExportOptions,
    scripts: Option<&XcodeScriptBundle>,
) -> Result<XcodeExportReport, ExportError> {
    let resolved_cow = project.with_resolved_script_language();
    let project = resolved_cow.as_ref();
    if project.symbol(&project.root).is_none() {
        return Err(ExportError::MissingRootSymbol(project.root.clone()));
    }
    if !(1..=100).contains(&options.jpeg_quality) {
        return Err(ExportError::Options(format!(
            "Xcode export jpeg_quality must be in 1..=100, got {}",
            options.jpeg_quality
        )));
    }

    ensure_safe_xcode_destination(out_dir)?;

    let mut preflight_items = preflight_xcode_project(project);
    let mut derived = publishable(project).unwrap_or_else(|| project.clone());
    let (_bmp_stats, bmp_warnings) =
        apply_bitmap_export_settings(&mut derived, options.jpeg_quality);
    for warning in bmp_warnings {
        preflight_items.push(XcodePreflightItem::new(
            "bitmap_alpha_flattened",
            XcodePreflightSeverity::Warning,
            "project resources",
            warning,
        ));
    }

    // Automatically compile ActionScript timelines, object handlers, project classes, and
    // `SwiftHost` / ad-hoc Flash proxy calls to Swift 6 when no explicit override bundle is supplied.
    // Note: run on `project` (prior to AVM1/AVM2 `strip_skipped_foreign_scripts`) because the Swift 6
    // AST compiler natively lowers both AS1/AS2 (`on(release)`) and AS3 scripts into one target!
    let auto_compiled_bundle = if scripts.is_none() {
        let (flattened_for_scripts, _) = neoflash_core::flatten_scenes_cow(project);
        let compiled =
            neoflash_script::compile_project_scripts_to_swift(flattened_for_scripts.as_ref());
        for issue in &compiled.issues {
            let severity = match issue.kind {
                neoflash_script::ConversionIssueKind::ShimInjected => XcodePreflightSeverity::Info,
                neoflash_script::ConversionIssueKind::Approximated
                | neoflash_script::ConversionIssueKind::ManualAttention => {
                    XcodePreflightSeverity::Warning
                }
            };
            preflight_items.push(XcodePreflightItem::new(
                "swift_script_conversion",
                severity,
                format!("{}:{}:{}", issue.location, issue.line, issue.column),
                issue.message.clone(),
            ));
        }
        Some(XcodeScriptBundle {
            host_methods: compiled
                .host_methods
                .into_iter()
                .map(|m| XcodeSwiftProxyMethod {
                    name: m.name,
                    params: m
                        .params
                        .into_iter()
                        .map(|p| XcodeSwiftProxyParam {
                            name: p.name,
                            swift_type: p.swift_type,
                        })
                        .collect(),
                    return_swift_type: m.return_swift_type,
                })
                .collect(),
            symbols: compiled
                .symbols
                .into_iter()
                .map(|s| XcodeSymbolScriptBundle {
                    symbol_id: s.symbol_id,
                    swift_class_name: s.swift_class_name,
                    is_linked_source_class: s.is_linked_source_class,
                    properties_swift: s.properties_swift,
                    frame_methods_swift: s.frame_methods_swift,
                    helper_methods_swift: s.helper_methods_swift,
                    instance_handlers_swift: s.instance_handlers_swift,
                })
                .collect(),
            source_classes_swift: compiled.source_classes_swift,
        })
    } else {
        None
    };
    let scripts = scripts.or(auto_compiled_bundle.as_ref());

    let target_name = pascal_project_name(&derived.name);
    let bundle_identifier =
        build_bundle_identifier(&options.bundle_identifier_prefix, &target_name);

    let generated_dir = out_dir.join("Generated");
    let kit_dir = generated_dir.join("NeoFlashKit");
    let scenes_dir = generated_dir.join("Scenes");
    let resources_dir = generated_dir.join("Resources");
    let app_dir = out_dir.join("App");

    // `Generated/` is 100% owned by NeoFlash and cleanly regenerated on every export.
    if fs::symlink_metadata(&generated_dir).is_ok() {
        fs::remove_dir_all(&generated_dir)?;
    }
    fs::create_dir_all(&kit_dir)?;
    fs::create_dir_all(&scenes_dir)?;
    fs::create_dir_all(&resources_dir)?;
    fs::create_dir_all(&app_dir)?;
    let marker_path = generated_dir.join(".neoflash-xcode-export.json");
    atomic_write(
        &marker_path,
        br#"{"generator":"neoflash-xcode","schema":1}"#,
    )?;

    // 1. Extract resources & Info.plist into Generated/Resources/
    let extracted = extract_xcode_resources(
        &derived,
        &resources_dir,
        options.base_dir.as_deref(),
        &mut preflight_items,
    )?;

    let info_plist_path = resources_dir.join("Info.plist");
    let info_plist_xml = render_resources_info_plist(&extracted.font_filenames);
    atomic_write(&info_plist_path, info_plist_xml.as_bytes())?;

    // 2. Write NeoFlashKit runtime files into Generated/NeoFlashKit/
    let kit_files = [
        ("NFValue.swift", runtime_swift::render_nf_value_swift()),
        (
            "NFDisplayList.swift",
            runtime_swift::render_nf_display_list_swift(),
        ),
        ("NFData.swift", data_swift::render_nf_data_swift()),
        (
            "NFRenderer.swift",
            renderer_swift::render_nf_renderer_swift(),
        ),
        ("NFAudio.swift", audio_swift::render_nf_audio_swift()),
    ];
    let mut generated_swift_files = Vec::new();
    let mut kit_swift_names = Vec::new();
    for (filename, contents) in kit_files {
        let path = kit_dir.join(filename);
        atomic_write(&path, contents.as_bytes())?;
        generated_swift_files.push(path);
        kit_swift_names.push(filename.to_owned());
    }

    // 3. Generate SwiftHostContract.swift & GeneratedScenes.swift into Generated/Scenes/
    let scene_gen = scene_codegen::generate_scenes_and_contract(
        &derived,
        &extracted.resource_filenames,
        &extracted.font_postscript_overrides,
        scripts,
    );
    let contract_path = scenes_dir.join("SwiftHostContract.swift");
    let scenes_path = scenes_dir.join("GeneratedScenes.swift");
    atomic_write(&contract_path, scene_gen.host_contract_swift.as_bytes())?;
    atomic_write(&scenes_path, scene_gen.scenes_swift.as_bytes())?;
    generated_swift_files.push(contract_path);
    generated_swift_files.push(scenes_path);
    let scenes_swift_names = vec![
        "SwiftHostContract.swift".to_owned(),
        "GeneratedScenes.swift".to_owned(),
    ];

    // 4. Write or non-destructively update developer-owned App/ shell
    let host_methods = scripts.map(|s| s.host_methods.as_slice()).unwrap_or(&[]);
    let app_entry_filename = format!("{target_name}App.swift");
    let app_entry_path = app_dir.join(&app_entry_filename);
    let content_view_path = app_dir.join("ContentView.swift");
    let app_host_path = app_dir.join("AppSwiftHost.swift");

    let mut preserved_app_files = Vec::new();
    let mut appended_host_methods = Vec::new();

    if options.overwrite_app_shell || !app_entry_path.exists() {
        let code = app_shell::render_app_entry_swift(&target_name);
        atomic_write(&app_entry_path, code.as_bytes())?;
    } else {
        preserved_app_files.push(app_entry_path.clone());
    }

    if options.overwrite_app_shell || !content_view_path.exists() {
        let code = app_shell::render_content_view_swift();
        atomic_write(&content_view_path, code.as_bytes())?;
    } else {
        preserved_app_files.push(content_view_path.clone());
    }

    if options.overwrite_app_shell || !app_host_path.exists() {
        let code = app_shell::render_initial_app_swift_host(host_methods);
        atomic_write(&app_host_path, code.as_bytes())?;
    } else {
        let existing_host = fs::read_to_string(&app_host_path)?;
        let (updated_host, newly_appended) =
            app_shell::append_missing_host_stubs(&existing_host, host_methods);
        preserved_app_files.push(app_host_path.clone());
        if !newly_appended.is_empty() {
            atomic_write(&app_host_path, updated_host.as_bytes())?;
            appended_host_methods = newly_appended;
        }
    }

    // Collect all `.swift` files in `App/` (including any custom files added by the developer)
    let mut app_swift_names = Vec::new();
    let mut app_swift_files = Vec::new();
    for entry in fs::read_dir(&app_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file()
            && path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("swift"))
            && let Some(name) = path.file_name().and_then(|n| n.to_str())
        {
            app_swift_names.push(name.to_owned());
        }
    }
    app_swift_names.sort();
    for name in &app_swift_names {
        app_swift_files.push(app_dir.join(name));
    }

    // 5. Generate `<PascalProjectName>.xcodeproj/project.pbxproj` + shared `.xcscheme`
    let xcodeproj_path = out_dir.join(format!("{target_name}.xcodeproj"));
    let schemes_dir = xcodeproj_path.join("xcshareddata").join("xcschemes");
    fs::create_dir_all(&schemes_dir)?;

    let pbx_spec = pbxproj::PbxProjectSpec {
        target_name: &target_name,
        bundle_identifier: &bundle_identifier,
        app_swift_files: &app_swift_names,
        kit_swift_files: &kit_swift_names,
        scenes_swift_files: &scenes_swift_names,
        resource_files: &extracted.ordered_resource_names,
    };
    let pbxproj_path = xcodeproj_path.join("project.pbxproj");
    let pbx_contents = pbxproj::generate_pbxproj(&pbx_spec);
    atomic_write(&pbxproj_path, pbx_contents.as_bytes())?;

    let scheme_path = schemes_dir.join(format!("{target_name}.xcscheme"));
    let scheme_xml = pbxproj::generate_xcscheme(&target_name);
    atomic_write(&scheme_path, scheme_xml.as_bytes())?;

    let host_method_count = host_methods.len();

    Ok(XcodeExportReport {
        project_dir: out_dir.to_path_buf(),
        xcodeproj_path,
        pbxproj_path,
        info_plist_path,
        scheme_name: target_name,
        bundle_identifier,
        generated_swift_files,
        app_swift_files,
        preserved_app_files,
        appended_host_methods,
        host_method_count,
        resource_files: extracted.written_paths,
        symbol_count: scene_gen.exported_symbol_count,
        preflight_items,
    })
}

struct ExtractedResources {
    resource_filenames: BTreeMap<String, String>,
    ordered_resource_names: Vec<String>,
    font_filenames: Vec<String>,
    font_postscript_overrides: BTreeMap<String, String>,
    written_paths: Vec<PathBuf>,
}

fn is_internal_metadata_resource(res: &ResourceUnit) -> bool {
    let p = res.path.as_str();
    p.eq_ignore_ascii_case(Project::SWATCHES_RESOURCE_PATH)
        || p.eq_ignore_ascii_case(Project::GLOBAL_SWATCHES_RESOURCE_PATH)
        || p.eq_ignore_ascii_case(Project::VECTOR_STYLES_RESOURCE_PATH)
        || p.eq_ignore_ascii_case(Project::DOCUMENT_ACCESSIBILITY_RESOURCE_PATH)
        || p.eq_ignore_ascii_case(Project::SHARED_LIBRARIES_RESOURCE_PATH)
        || p.eq_ignore_ascii_case(Project::PUBLISH_SETTINGS_RESOURCE_PATH)
        || p.eq_ignore_ascii_case("fonts/embedding.json")
        || is_internal_provenance_resource(res)
}

fn extract_xcode_resources(
    project: &Project,
    resources_dir: &Path,
    base_dir: Option<&Path>,
    preflight_items: &mut Vec<XcodePreflightItem>,
) -> Result<ExtractedResources, ExportError> {
    let mut resource_filenames = BTreeMap::new();
    let mut ordered_resource_names = Vec::new();
    let mut font_filenames = Vec::new();
    let mut font_postscript_overrides = BTreeMap::new();
    let mut written_paths = Vec::new();
    let mut occupied_names: BTreeSet<String> = BTreeSet::from(["info.plist".to_owned()]);

    let alloc_unique_filename =
        |stem_raw: &str, ext_raw: &str, occupied: &mut BTreeSet<String>| -> String {
            let clean_stem: String = stem_raw
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                        c
                    } else {
                        '_'
                    }
                })
                .collect();
            let trimmed_stem = clean_stem.trim_matches('_');
            let base_cow = if trimmed_stem.is_empty() {
                "asset".to_owned()
            } else if is_windows_reserved_stem(trimmed_stem) {
                format!("_{trimmed_stem}")
            } else {
                trimmed_stem.to_owned()
            };
            let base = base_cow.as_str();
            let raw_trimmed = ext_raw.trim_matches('.');
            let ext: &str = if raw_trimmed.is_empty() {
                ""
            } else if raw_trimmed.len() <= 16
                && raw_trimmed.chars().all(|c| c.is_ascii_alphanumeric())
                && !raw_trimmed.eq_ignore_ascii_case("swift")
            {
                raw_trimmed
            } else {
                "bin"
            };
            let first = if ext.is_empty() {
                base.to_owned()
            } else {
                format!("{base}.{ext}")
            };
            if occupied.insert(first.to_ascii_lowercase()) {
                return first;
            }
            let mut n = 2usize;
            loop {
                let cand = if ext.is_empty() {
                    format!("{base}_{n}")
                } else {
                    format!("{base}_{n}.{ext}")
                };
                if occupied.insert(cand.to_ascii_lowercase()) {
                    return cand;
                }
                n += 1;
            }
        };

    for res in &project.resources {
        if is_internal_metadata_resource(res) {
            continue;
        }

        let basename = res
            .path
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(res.id.as_str());
        let (raw_stem, raw_ext) = match basename.rsplit_once('.') {
            Some((s, e)) => (s, e),
            None => (basename, ""),
        };

        if is_font_resource(res) {
            let Ok(raw_bytes) = STANDARD.decode(res.data_base64.as_ref()) else {
                continue;
            };
            if raw_bytes.is_empty() {
                continue;
            }
            let sfnt_cow = sfnt_view_bytes(&raw_bytes);
            let font_bytes: &[u8] = sfnt_cow.as_deref().unwrap_or(&raw_bytes);
            let ext = if font_bytes.starts_with(b"OTTO") {
                "otf"
            } else {
                "ttf"
            };
            let filename = alloc_unique_filename(raw_stem, ext, &mut occupied_names);
            let dest = resources_dir.join(&filename);
            atomic_write(&dest, font_bytes)?;

            let authored_family = font_family_for_resource(res);
            if let Some(ps_name) = parse_sfnt_postscript_name(font_bytes) {
                font_postscript_overrides.insert(authored_family.to_ascii_lowercase(), ps_name);
            }

            resource_filenames.insert(res.id.clone(), filename.clone());
            ordered_resource_names.push(filename.clone());
            font_filenames.push(filename);
            written_paths.push(dest);
            continue;
        }

        let mime = res.media_type.trim().to_ascii_lowercase();
        if mime.starts_with("image/") {
            let Ok(raw_bytes) = STANDARD.decode(res.data_base64.as_ref()) else {
                continue;
            };
            if raw_bytes.is_empty() {
                continue;
            }
            let (out_bytes, ext) = if mime == "image/jpeg" || mime == "image/jpg" {
                (raw_bytes, "jpg")
            } else if mime == "image/png" {
                (raw_bytes, "png")
            } else if let Ok(decoded) = image::load_from_memory(&raw_bytes) {
                let rgba = decoded.to_rgba8();
                let png = encode_rgba_png(rgba.as_raw(), rgba.width(), rgba.height())?;
                (png, "png")
            } else {
                let fallback_ext = if raw_ext.is_empty() { "bin" } else { raw_ext };
                (raw_bytes, fallback_ext)
            };
            let filename = alloc_unique_filename(raw_stem, ext, &mut occupied_names);
            let dest = resources_dir.join(&filename);
            atomic_write(&dest, &out_bytes)?;
            resource_filenames.insert(res.id.clone(), filename.clone());
            ordered_resource_names.push(filename);
            written_paths.push(dest);
            continue;
        }

        // Audio / Video / Generic binary resources
        if res.data_base64.is_empty() && res.is_video() {
            if let Some(base) = base_dir
                && validate_external_video_path(&res.path).is_ok()
            {
                match read_safe_external_video_bytes(base, &res.path) {
                    Ok(bytes) => {
                        let ext = if raw_ext.is_empty() { "mp4" } else { raw_ext };
                        let filename = alloc_unique_filename(raw_stem, ext, &mut occupied_names);
                        let dest = resources_dir.join(&filename);
                        atomic_write(&dest, &bytes)?;
                        resource_filenames.insert(res.id.clone(), filename.clone());
                        ordered_resource_names.push(filename);
                        written_paths.push(dest);
                    }
                    Err(reason) => {
                        preflight_items.push(XcodePreflightItem::new(
                            "missing_external_video_file",
                            XcodePreflightSeverity::Warning,
                            format!("resource '{}'", res.path),
                            format!(
                                "External video file '{}' could not be staged safely: {reason}",
                                res.path
                            ),
                        ));
                    }
                }
            }
            continue;
        }

        let Ok(raw_bytes) = STANDARD.decode(res.data_base64.as_ref()) else {
            continue;
        };
        if raw_bytes.is_empty() {
            continue;
        }
        let ext = if !raw_ext.is_empty() {
            raw_ext
        } else {
            match mime.as_str() {
                "audio/wav" | "audio/x-wav" | "audio/wave" => "wav",
                "audio/mpeg" | "audio/mp3" => "mp3",
                "audio/mp4" | "audio/aac" | "audio/x-m4a" => "m4a",
                "video/mp4" => "mp4",
                "video/quicktime" => "mov",
                _ => "dat",
            }
        };
        let filename = alloc_unique_filename(raw_stem, ext, &mut occupied_names);
        let dest = resources_dir.join(&filename);
        atomic_write(&dest, &raw_bytes)?;
        resource_filenames.insert(res.id.clone(), filename.clone());
        ordered_resource_names.push(filename);
        written_paths.push(dest);
    }

    Ok(ExtractedResources {
        resource_filenames,
        ordered_resource_names,
        font_filenames,
        font_postscript_overrides,
        written_paths,
    })
}

const MAX_XCODE_VIDEO_INPUT_BYTES: u64 = 512 * 1024 * 1024;

fn read_safe_external_video_bytes(base: &Path, rel_path: &str) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let canon_base = fs::canonicalize(base)
        .map_err(|e| format!("base directory '{}' is inaccessible: {e}", base.display()))?;
    let mut cur = base.to_path_buf();
    for comp in Path::new(rel_path).components() {
        cur.push(comp.as_os_str());
        let smeta = fs::symlink_metadata(&cur)
            .map_err(|e| format!("'{}' could not be inspected: {e}", cur.display()))?;
        if smeta.file_type().is_symlink() {
            return Err(format!(
                "refusing symlink component '{}' while reading external video '{}'",
                cur.display(),
                rel_path
            ));
        }
    }
    let canon_target = fs::canonicalize(&cur)
        .map_err(|e| format!("'{}' could not be canonicalized: {e}", cur.display()))?;
    if !canon_target.starts_with(&canon_base) {
        return Err(format!(
            "external video '{}' escapes base directory '{}'",
            rel_path,
            canon_base.display()
        ));
    }
    let file = fs::File::open(&canon_target)
        .map_err(|e| format!("'{}' could not be opened: {e}", canon_target.display()))?;
    let meta = file
        .metadata()
        .map_err(|e| format!("'{}' metadata failed: {e}", canon_target.display()))?;
    if !meta.is_file() {
        return Err(format!(
            "'{}' is not a regular file",
            canon_target.display()
        ));
    }
    if meta.len() > MAX_XCODE_VIDEO_INPUT_BYTES {
        return Err(format!(
            "external video '{}' ({} bytes) exceeds 512 MiB limit",
            rel_path,
            meta.len()
        ));
    }
    let mut buf = Vec::with_capacity(meta.len() as usize);
    file.take(MAX_XCODE_VIDEO_INPUT_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|e| format!("'{}' read failed: {e}", canon_target.display()))?;
    if buf.len() as u64 > MAX_XCODE_VIDEO_INPUT_BYTES {
        return Err(format!(
            "external video '{}' exceeded 512 MiB limit while reading",
            rel_path
        ));
    }
    Ok(buf)
}

fn render_resources_info_plist(font_filenames: &[String]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
    let _ = writeln!(
        out,
        "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">"
    );
    let _ = writeln!(out, "<plist version=\"1.0\">");
    let _ = writeln!(out, "<dict>");
    let _ = writeln!(out, "  <key>ATSApplicationFontsPath</key>");
    let _ = writeln!(out, "  <string>.</string>");
    let _ = writeln!(out, "  <key>UIAppFonts</key>");
    if font_filenames.is_empty() {
        let _ = writeln!(out, "  <array/>");
    } else {
        let _ = writeln!(out, "  <array>");
        for font in font_filenames {
            let _ = writeln!(out, "    <string>{}</string>", plist_escape(font));
        }
        let _ = writeln!(out, "  </array>");
    }
    let _ = writeln!(out, "</dict>");
    let _ = writeln!(out, "</plist>");
    out
}

fn plist_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
}

/// Reads OpenType/TrueType `name` table Name ID 6 (PostScript Name) so `NSFont` / `UIFont`
/// resolves custom bundled fonts reliably across macOS and iOS.
fn parse_sfnt_postscript_name(sfnt: &[u8]) -> Option<String> {
    if sfnt.len() < 12 {
        return None;
    }
    let num_tables = u16::from_be_bytes([sfnt[4], sfnt[5]]) as usize;
    let dir_end = 12usize.checked_add(num_tables.checked_mul(16)?)?;
    if sfnt.len() < dir_end {
        return None;
    }
    let mut name_offset = None;
    let mut name_len = 0usize;
    for i in 0..num_tables {
        let rec = 12 + i * 16;
        if &sfnt[rec..rec + 4] == b"name" {
            let off =
                u32::from_be_bytes([sfnt[rec + 8], sfnt[rec + 9], sfnt[rec + 10], sfnt[rec + 11]])
                    as usize;
            let len = u32::from_be_bytes([
                sfnt[rec + 12],
                sfnt[rec + 13],
                sfnt[rec + 14],
                sfnt[rec + 15],
            ]) as usize;
            name_offset = Some(off);
            name_len = len;
            break;
        }
    }
    let table_off = name_offset?;
    let table_end = table_off.checked_add(name_len)?;
    if sfnt.len() < table_end || name_len < 6 {
        return None;
    }
    let table = &sfnt[table_off..table_end];
    let count = u16::from_be_bytes([table[2], table[3]]) as usize;
    let string_offset = u16::from_be_bytes([table[4], table[5]]) as usize;
    let records_end = 6usize.checked_add(count.checked_mul(12)?)?;
    if table.len() < records_end || table.len() < string_offset {
        return None;
    }

    // Prefer Windows/Unicode UTF-16BE (platformID 3 or 0), fallback to Mac Roman (platformID 1)
    let mut mac_fallback: Option<String> = None;
    for i in 0..count {
        let r = 6 + i * 12;
        let platform_id = u16::from_be_bytes([table[r], table[r + 1]]);
        let name_id = u16::from_be_bytes([table[r + 6], table[r + 7]]);
        if name_id != 6 {
            continue;
        }
        let length = u16::from_be_bytes([table[r + 8], table[r + 9]]) as usize;
        let offset = u16::from_be_bytes([table[r + 10], table[r + 11]]) as usize;
        let s_start = string_offset.checked_add(offset)?;
        let s_end = s_start.checked_add(length)?;
        if s_end > table.len() || length == 0 {
            continue;
        }
        let raw = &table[s_start..s_end];
        if (platform_id == 3 || platform_id == 0) && length.is_multiple_of(2) {
            let units: Vec<u16> = raw
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&pair| u16::from_be_bytes(pair))
                .collect();
            let decoded = String::from_utf16_lossy(&units).trim().to_owned();
            if !decoded.is_empty() {
                return Some(decoded);
            }
        } else if platform_id == 1 && mac_fallback.is_none() {
            let decoded: String = raw
                .iter()
                .filter(|&&b| b.is_ascii_graphic() || b == b'-')
                .map(|&b| b as char)
                .collect();
            if !decoded.is_empty() {
                mac_fallback = Some(decoded);
            }
        }
    }
    mac_fallback
}

/// Read-only ownership and symlink preflight for `out_dir` before Xcode export or headless snapshot.
pub fn preflight_xcode_output_dir(out_dir: &Path) -> Result<(), ExportError> {
    ensure_safe_xcode_destination(out_dir)
}

fn is_windows_reserved_stem(stem: &str) -> bool {
    let up = stem.trim().to_ascii_uppercase();
    matches!(
        up.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

fn is_root_or_home_dir(path: &Path) -> bool {
    if path.as_os_str().is_empty() || path.parent().is_none() {
        return true;
    }
    let check_canon = fs::canonicalize(path).ok();
    if let Some(ref c) = check_canon
        && c.parent().is_none()
    {
        return true;
    }
    for env_key in ["HOME", "USERPROFILE"] {
        if let Some(raw_home) = std::env::var_os(env_key) {
            let home_path = PathBuf::from(raw_home);
            if path == home_path {
                return true;
            }
            if let (Some(c_out), Ok(c_home)) = (&check_canon, fs::canonicalize(&home_path))
                && *c_out == c_home
            {
                return true;
            }
        }
    }
    false
}

fn is_regular_nonsymlink_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|m| m.file_type().is_file() && !m.file_type().is_symlink())
        .unwrap_or(false)
}

fn ensure_safe_xcode_destination(out_dir: &Path) -> Result<(), ExportError> {
    if is_root_or_home_dir(out_dir) {
        return Err(ExportError::Destination(out_dir.display().to_string()));
    }
    let smeta = match fs::symlink_metadata(out_dir) {
        Ok(m) => m,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(ExportError::Io(err)),
    };
    if smeta.file_type().is_symlink() || !smeta.file_type().is_dir() {
        return Err(ExportError::Destination(out_dir.display().to_string()));
    }

    let mut has_user_entries = false;
    let mut has_legacy_pbxproj = false;
    for entry in fs::read_dir(out_dir)? {
        let entry = entry?;
        let ftype = entry.file_type()?;
        let name = entry.file_name();
        let name_str = name.to_str().unwrap_or("");
        if matches!(name_str, ".DS_Store" | "Thumbs.db") && !ftype.is_symlink() {
            continue;
        }
        if ftype.is_symlink() {
            return Err(ExportError::Destination(out_dir.display().to_string()));
        }
        has_user_entries = true;
        if name_str.ends_with(".xcodeproj") && ftype.is_dir() {
            let pbx = entry.path().join("project.pbxproj");
            if is_regular_nonsymlink_file(&pbx)
                && fs::read_to_string(&pbx)
                    .map(|s| s.contains("NeoFlashKit"))
                    .unwrap_or(false)
            {
                has_legacy_pbxproj = true;
            }
        }
    }
    if !has_user_entries {
        return Ok(());
    }

    for sub in ["Generated", "App"] {
        let sub_path = out_dir.join(sub);
        if let Ok(m) = fs::symlink_metadata(&sub_path)
            && (m.file_type().is_symlink() || !m.file_type().is_dir())
        {
            return Err(ExportError::Destination(out_dir.display().to_string()));
        }
    }

    let marker_path = out_dir.join("Generated/.neoflash-xcode-export.json");
    let has_marker = is_regular_nonsymlink_file(&marker_path)
        && fs::read_to_string(&marker_path)
            .map(|s| s.contains("neoflash-xcode"))
            .unwrap_or(false);
    let has_legacy_kit = has_legacy_pbxproj
        && is_regular_nonsymlink_file(&out_dir.join("Generated/NeoFlashKit/NFValue.swift"));

    if !has_marker && !has_legacy_kit {
        return Err(ExportError::Destination(out_dir.display().to_string()));
    }
    Ok(())
}

/// Converts a raw project name into a safe PascalCase Swift / Xcode target name.
#[must_use]
pub fn pascal_project_name(raw_name: &str) -> String {
    let base = pascal_case_identifier(raw_name, "NeoFlashApp");
    if is_windows_reserved_stem(&base)
        || matches!(
            base.as_str(),
            "App"
                | "MainScene"
                | "ContentView"
                | "AppSwiftHost"
                | "SwiftUI"
                | "Foundation"
                | "CoreGraphics"
                | "Type"
                | "Protocol"
                | "Actor"
        )
    {
        format!("NF{base}")
    } else {
        base
    }
}

pub(super) fn pascal_case_identifier(raw: &str, fallback: &str) -> String {
    let mut out = String::new();
    let mut capitalize_next = true;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            if out.is_empty() && ch.is_ascii_digit() {
                out.push_str("NF");
            }
            if capitalize_next {
                out.push(ch.to_ascii_uppercase());
                capitalize_next = false;
            } else {
                out.push(ch);
            }
        } else {
            capitalize_next = true;
        }
    }
    if out.is_empty() {
        fallback.to_owned()
    } else {
        out
    }
}

pub(super) fn sanitize_swift_identifier(raw: &str, fallback: &str) -> String {
    let mut out = String::new();
    for ch in raw.trim().chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else if !out.is_empty() && !out.ends_with('_') {
            out.push('_');
        }
    }
    let trimmed = out.trim_matches('_');
    let candidate = if trimmed.is_empty() {
        fallback
    } else {
        trimmed
    };
    if candidate.as_bytes()[0].is_ascii_digit() {
        format!("_{candidate}")
    } else if is_swift_keyword(candidate) {
        format!("{candidate}_")
    } else {
        candidate.to_owned()
    }
}

fn is_swift_keyword(word: &str) -> bool {
    matches!(
        word,
        "associatedtype"
            | "class"
            | "deinit"
            | "enum"
            | "extension"
            | "fileprivate"
            | "func"
            | "import"
            | "init"
            | "inout"
            | "internal"
            | "let"
            | "open"
            | "operator"
            | "private"
            | "precedencegroup"
            | "protocol"
            | "public"
            | "rethrows"
            | "static"
            | "struct"
            | "subscript"
            | "typealias"
            | "var"
            | "break"
            | "case"
            | "catch"
            | "continue"
            | "default"
            | "defer"
            | "do"
            | "else"
            | "fallthrough"
            | "for"
            | "guard"
            | "if"
            | "in"
            | "repeat"
            | "return"
            | "throw"
            | "switch"
            | "where"
            | "while"
            | "Any"
            | "as"
            | "await"
            | "false"
            | "is"
            | "nil"
            | "self"
            | "Self"
            | "super"
            | "throws"
            | "true"
            | "try"
            | "_"
    )
}

fn build_bundle_identifier(prefix: &str, target_name: &str) -> String {
    let clean_prefix: String = prefix
        .trim()
        .trim_matches('.')
        .split('.')
        .filter_map(|seg| {
            let s: String = seg
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
                .collect();
            (!s.is_empty()).then_some(s)
        })
        .collect::<Vec<_>>()
        .join(".");
    let effective_prefix = if clean_prefix.is_empty() {
        default_bundle_id_prefix()
    } else {
        clean_prefix
    };
    let slug: String = target_name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    let effective_slug = if slug.is_empty() {
        "app".to_owned()
    } else {
        slug
    };
    format!("{effective_prefix}.{effective_slug}")
}
