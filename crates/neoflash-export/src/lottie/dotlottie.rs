//! `.lottie` (dotLottie v1.0) ZIP archive packaging, manifest generation, and container validation.

use super::{
    LottieExportReport, LottieFormat, LottieOptions, LottieProgress,
    assets::{DetectedRaster, ExternalImageFile, detect_raster_magic},
    check_cancel_flag, export_lottie_internal, export_project_lottie,
};
use crate::ExportError;
use neoflash_core::Project;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read as _, Write as _},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

/// Build an in-memory `.lottie` ZIP archive and [`LottieExportReport`] from `project`.
pub fn export_dotlottie_bytes(
    project: &Project,
    options: &LottieOptions,
) -> Result<(Vec<u8>, LottieExportReport), ExportError> {
    let cancel = AtomicBool::new(false);
    export_dotlottie_bytes_with_cancel(project, options, &cancel, |_| true)
}

/// Build an in-memory `.lottie` ZIP archive with [`AtomicBool`] cancellation and progress reporting.
pub fn export_dotlottie_bytes_with_cancel<F>(
    project: &Project,
    options: &LottieOptions,
    cancel: &AtomicBool,
    mut on_progress: F,
) -> Result<(Vec<u8>, LottieExportReport), ExportError>
where
    F: FnMut(&LottieProgress) -> bool,
{
    let dot_opts = LottieOptions {
        format: LottieFormat::DotLottie,
        ..options.clone()
    };
    let (value, mut report, external_images) =
        export_lottie_internal(project, &dot_opts, Some(cancel), &mut on_progress)?;
    check_cancel_flag(Some(cancel))?;
    let json = if dot_opts.pretty {
        serde_json::to_string_pretty(&value)?
    } else {
        serde_json::to_string(&value)?
    };
    let zip_bytes = build_dotlottie_zip(json.as_bytes(), &external_images, Some(cancel))?;
    validate_dotlottie_bytes(&zip_bytes)?;
    report.bytes = zip_bytes.len();
    if cancel.load(Ordering::Relaxed)
        || !on_progress(&LottieProgress {
            completed_steps: 4,
            total_steps: 4,
            current_stage: "Completed".into(),
        })
    {
        return Err(ExportError::Options("Export cancelled.".into()));
    }
    Ok((zip_bytes, report))
}

/// Write a dotLottie (`.lottie`) ZIP package atomically to `output_file`.
pub fn export_dotlottie(
    project: &Project,
    output_file: &Path,
    options: &LottieOptions,
) -> Result<LottieExportReport, ExportError> {
    let dot_opts = LottieOptions {
        format: LottieFormat::DotLottie,
        ..options.clone()
    };
    export_project_lottie(project, output_file, &dot_opts)
}

pub(super) fn validate_zip_entry_path(name: &str) -> Result<(), ExportError> {
    if name.is_empty()
        || name.starts_with('/')
        || name.starts_with('\\')
        || name.contains('\\')
        || name.contains(':')
        || name.chars().any(char::is_control)
    {
        return Err(ExportError::Options(format!(
            "Invalid dotLottie archive entry path '{name}'."
        )));
    }
    let parts: Vec<&str> = name.split('/').collect();
    if parts
        .iter()
        .any(|seg| seg.is_empty() || *seg == "." || *seg == "..")
    {
        return Err(ExportError::Options(format!(
            "Unsafe or non-normalized dotLottie archive entry path '{name}'."
        )));
    }
    Ok(())
}

pub(super) fn build_dotlottie_zip(
    animation_json: &[u8],
    external_images: &[ExternalImageFile],
    cancel: Option<&AtomicBool>,
) -> Result<Vec<u8>, ExportError> {
    check_cancel_flag(cancel)?;
    let manifest = json!({
        "version": "1.0",
        "generator": "NeoFlash",
        "animations": [{
            "id": "animation",
            "speed": 1,
            "loop": true
        }]
    });
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    let deflate_opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let stored_opts =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

    let cursor = Cursor::new(Vec::new());
    let mut zw = zip::ZipWriter::new(cursor);

    zw.start_file("manifest.json", deflate_opts)
        .map_err(|e| ExportError::Io(std::io::Error::other(e.to_string())))?;
    zw.write_all(&manifest_bytes)?;
    check_cancel_flag(cancel)?;

    zw.start_file("animations/animation.json", deflate_opts)
        .map_err(|e| ExportError::Io(std::io::Error::other(e.to_string())))?;
    zw.write_all(animation_json)?;
    check_cancel_flag(cancel)?;

    let mut seen_paths = BTreeSet::new();
    seen_paths.insert("manifest.json".to_owned());
    seen_paths.insert("animations/animation.json".to_owned());
    for img in external_images {
        check_cancel_flag(cancel)?;
        validate_zip_entry_path(&img.relative_path)?;
        if !seen_paths.insert(img.relative_path.clone()) {
            return Err(ExportError::Options(format!(
                "Duplicate dotLottie archive entry '{}'.",
                img.relative_path
            )));
        }
        zw.start_file(&img.relative_path, stored_opts)
            .map_err(|e| ExportError::Io(std::io::Error::other(e.to_string())))?;
        zw.write_all(&img.bytes)?;
    }

    let finished = zw
        .finish()
        .map_err(|e| ExportError::Io(std::io::Error::other(e.to_string())))?;
    Ok(finished.into_inner())
}

/// Validate a `.lottie` ZIP archive against the dotLottie container specification,
/// checking path safety, `manifest.json`, `animations/<id>.json`, and every referenced
/// `assets[]` / `images/*` entry. Returns the parsed primary animation JSON [`Value`].
pub fn validate_dotlottie_bytes(bytes: &[u8]) -> Result<Value, ExportError> {
    if !bytes.starts_with(b"PK\x03\x04") {
        return Err(ExportError::Options(
            "dotLottie archive does not start with ZIP local header signature.".into(),
        ));
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| ExportError::Options(format!("Invalid dotLottie ZIP archive: {e}")))?;
    if archive.len() > 4096 {
        return Err(ExportError::Options(
            "dotLottie archive exceeds 4096 entry limit.".into(),
        ));
    }

    let mut entries: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for idx in 0..archive.len() {
        let mut entry = archive.by_index(idx).map_err(|e| {
            ExportError::Options(format!("Invalid dotLottie ZIP entry #{idx}: {e}"))
        })?;
        let name = entry.name().to_owned();
        if entry.is_dir() {
            return Err(ExportError::Options(format!(
                "dotLottie archive contains unexpected directory entry '{name}'."
            )));
        }
        validate_zip_entry_path(&name)?;
        if entries.contains_key(&name) {
            return Err(ExportError::Options(format!(
                "Duplicate entry '{name}' in dotLottie archive."
            )));
        }
        let mut data = Vec::new();
        entry.read_to_end(&mut data)?;
        entries.insert(name, data);
    }

    let manifest_bytes = entries.get("manifest.json").ok_or_else(|| {
        ExportError::Options("dotLottie archive is missing 'manifest.json'.".into())
    })?;
    let manifest: Value = serde_json::from_slice(manifest_bytes).map_err(|e| {
        ExportError::Options(format!("Invalid JSON in dotLottie 'manifest.json': {e}"))
    })?;
    let version = manifest["version"].as_str().unwrap_or("");
    if version.trim().is_empty() {
        return Err(ExportError::Options(
            "dotLottie 'manifest.json' is missing non-empty 'version'.".into(),
        ));
    }
    let generator = manifest["generator"].as_str().unwrap_or("");
    if generator.trim().is_empty() {
        return Err(ExportError::Options(
            "dotLottie 'manifest.json' is missing 'generator'.".into(),
        ));
    }
    let anims = manifest["animations"].as_array().ok_or_else(|| {
        ExportError::Options("dotLottie 'manifest.json' is missing 'animations' array.".into())
    })?;
    if anims.is_empty() {
        return Err(ExportError::Options(
            "dotLottie 'manifest.json' must declare at least one animation.".into(),
        ));
    }

    let mut primary_anim: Option<Value> = None;
    for anim_meta in anims {
        let anim_id = anim_meta["id"].as_str().unwrap_or("");
        if anim_id.is_empty() || anim_id.contains('/') || anim_id.contains('\\') || anim_id == ".."
        {
            return Err(ExportError::Options(format!(
                "Invalid animation id '{anim_id}' in dotLottie manifest."
            )));
        }
        let anim_entry_path = format!("animations/{anim_id}.json");
        let anim_bytes = entries.get(&anim_entry_path).ok_or_else(|| {
            ExportError::Options(format!(
                "dotLottie archive is missing animation file '{anim_entry_path}'."
            ))
        })?;
        let anim_val: Value = serde_json::from_slice(anim_bytes).map_err(|e| {
            ExportError::Options(format!("Invalid JSON in '{anim_entry_path}': {e}"))
        })?;
        validate_lottie_asset_references(&anim_val, Some(&entries))?;
        if primary_anim.is_none() {
            primary_anim = Some(anim_val);
        }
    }

    primary_anim.ok_or_else(|| ExportError::Options("No animation found in dotLottie.".into()))
}

fn validate_lottie_asset_references(
    lottie: &Value,
    archive_entries: Option<&BTreeMap<String, Vec<u8>>>,
) -> Result<(), ExportError> {
    let assets = lottie["assets"].as_array().ok_or_else(|| {
        ExportError::Options("Lottie JSON is missing root 'assets' array.".into())
    })?;
    let mut asset_ids = BTreeSet::new();
    for asset in assets {
        let id = asset["id"].as_str().unwrap_or("");
        if id.is_empty() {
            return Err(ExportError::Options(
                "Lottie asset entry is missing non-empty 'id'.".into(),
            ));
        }
        if !asset_ids.insert(id.to_owned()) {
            return Err(ExportError::Options(format!(
                "Duplicate Lottie asset id '{id}'."
            )));
        }
        if let Some(p) = asset["p"].as_str() {
            let embedded = asset["e"].as_i64().unwrap_or(0) == 1;
            if embedded {
                let audio = asset["t"].as_u64() == Some(2);
                let valid = if audio {
                    p.starts_with("data:audio/wav;base64,")
                        || p.starts_with("data:audio/mpeg;base64,")
                } else {
                    p.starts_with("data:image/")
                };
                if !valid {
                    return Err(ExportError::Options(format!(
                        "Embedded Lottie asset '{id}' has a data URI inconsistent with its media type."
                    )));
                }
            } else {
                let u = asset["u"].as_str().unwrap_or("");
                let rel = format!("{u}{p}");
                validate_zip_entry_path(&rel)?;
                if let Some(entries) = archive_entries {
                    let img_bytes = entries.get(&rel).ok_or_else(|| {
                        ExportError::Options(format!(
                            "dotLottie archive is missing referenced image asset '{rel}' (id '{id}')."
                        ))
                    })?;
                    if detect_raster_magic(img_bytes) == DetectedRaster::Other {
                        return Err(ExportError::Options(format!(
                            "dotLottie image entry '{rel}' does not have valid PNG/JPEG/WebP/GIF magic bytes."
                        )));
                    }
                }
            }
        }
    }

    let check_layers = |layers: &[Value], ctx: &str| -> Result<(), ExportError> {
        for layer in layers {
            if let Some(ref_id) = layer["refId"].as_str()
                && !asset_ids.contains(ref_id)
            {
                return Err(ExportError::Options(format!(
                    "Dangling refId '{ref_id}' in {ctx} layer '{}'.",
                    layer["nm"].as_str().unwrap_or("<unnamed>")
                )));
            }
        }
        Ok(())
    };

    let root_layers = lottie["layers"].as_array().ok_or_else(|| {
        ExportError::Options("Lottie JSON is missing root 'layers' array.".into())
    })?;
    check_layers(root_layers, "root")?;
    for asset in assets {
        if let Some(comp_layers) = asset["layers"].as_array() {
            let id = asset["id"].as_str().unwrap_or("<precomp>");
            check_layers(comp_layers, id)?;
        }
    }
    Ok(())
}
