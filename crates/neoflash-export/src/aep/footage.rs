use super::*;

type ExtractedAepFootage = (
    Vec<AeFootageItem>,
    Vec<PathBuf>,
    BTreeMap<String, AeFootageItem>,
);

pub(super) fn extract_aep_footage(
    project: &Project,
    footage_dir: &Path,
    footage_dir_name: &str,
    base_dir: Option<&Path>,
    diagnostics: &mut Vec<AepDiagnostic>,
) -> Result<ExtractedAepFootage, ExportError> {
    let mut items = Vec::new();
    let mut written_files = Vec::new();
    let mut map = BTreeMap::new();
    let mut used_filenames_lower = BTreeSet::new();

    let alloc_safe_filename =
        |raw_stem_src: &str, ext: &str, used_lower: &mut BTreeSet<String>| -> (String, String) {
            let filtered: String = raw_stem_src
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                        c
                    } else {
                        '_'
                    }
                })
                .collect();
            let trimmed = filtered.trim_matches(|c| c == '_' || c == '-');
            let safe_stem = if trimmed.is_empty() {
                "asset".to_owned()
            } else {
                trimmed.to_owned()
            };
            let mut candidate = format!("{safe_stem}.{ext}");
            let mut seq = 2usize;
            while !used_lower.insert(candidate.to_ascii_lowercase()) {
                candidate = format!("{safe_stem}_{seq}.{ext}");
                seq += 1;
            }
            (safe_stem, candidate)
        };

    let mut ensured_dir = false;
    for (idx, res) in project.resources.iter().enumerate() {
        if res.data_base64.is_empty() {
            continue;
        }
        let Ok(mut bytes) = STANDARD.decode(res.data_base64.as_ref()) else {
            diagnostics.push(AepDiagnostic {
                code: "AE_INVALID_RESOURCE",
                kind: AepDiagnosticKind::Skipped,
                location: format!("resource '{}'", res.id),
                message: "Skipped resource with invalid base64 payload.".into(),
            });
            continue;
        };
        if bytes.is_empty() {
            continue;
        }

        let mut effective_media_type = res.media_type.clone();
        let explicit_webp = res
            .bitmap_export
            .is_some_and(|s| matches!(s.compression, neoflash_core::BitmapCompression::Webp));
        if !explicit_webp
            && effective_media_type.eq_ignore_ascii_case("image/webp")
            && let Ok(decoded_webp) = image::load_from_memory(&bytes)
        {
            let rgba = decoded_webp.to_rgba8();
            if let Ok(png_buf) =
                crate::bitmap_encode::encode_rgba_png(rgba.as_raw(), rgba.width(), rgba.height())
            {
                bytes = png_buf;
                effective_media_type = "image/png".to_owned();
                diagnostics.push(AepDiagnostic {
                    code: "AE_WEBP_TRANSCODED_PNG",
                    kind: AepDiagnosticKind::Approximated,
                    location: format!("resource '{}'", res.id),
                    message: "Transcoded WebP resource to lossless PNG footage for universal After Effects import.".into(),
                });
            }
        }

        let ext = extension_for_media_type(&effective_media_type, &res.path);
        let raw_stem = Path::new(&res.path)
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(&res.id);
        let (safe_stem, file_name) = alloc_safe_filename(raw_stem, &ext, &mut used_filenames_lower);

        if !ensured_dir {
            fs::create_dir_all(footage_dir)?;
            ensured_dir = true;
        }
        let dest = footage_dir.join(&file_name);
        atomic_write(&dest, &bytes)?;
        written_files.push(dest);

        let (w, h) = if effective_media_type.starts_with("image/") {
            image::load_from_memory(&bytes)
                .ok()
                .map(|img| (Some(img.width()), Some(img.height())))
                .unwrap_or((None, None))
        } else {
            (None, None)
        };

        let ft_item = AeFootageItem {
            id: format!("ft_{idx}_{safe_stem}"),
            name: file_name.clone(),
            relative_path: format!("{footage_dir_name}/{file_name}"),
            media_type: effective_media_type,
            width: w,
            height: h,
        };
        map.insert(res.id.clone(), ft_item.clone());
        items.push(ft_item);
    }

    // Also stage any external timeline videos (`VideoDeploymentMode::External`) resolvable from `base_dir`.
    if let Some(base) = base_dir {
        let mut seen_ext = BTreeSet::new();
        for sym in &project.symbols {
            for layer in &sym.timeline.layers {
                for kf in &layer.keyframes {
                    if let Some(vs) = &kf.video
                        && let Some(ext_rel) = &vs.external_path
                        && neoflash_core::validate_external_video_path(ext_rel).is_ok()
                        && seen_ext.insert(ext_rel.clone())
                    {
                        let src_path = base.join(ext_rel);
                        if let Ok(bytes) = fs::read(&src_path)
                            && !bytes.is_empty()
                        {
                            let ext = extension_for_media_type("", ext_rel);
                            let raw_stem = Path::new(ext_rel)
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("video");
                            let (safe_stem, file_name) =
                                alloc_safe_filename(raw_stem, &ext, &mut used_filenames_lower);
                            if !ensured_dir {
                                fs::create_dir_all(footage_dir)?;
                                ensured_dir = true;
                            }
                            let dest = footage_dir.join(&file_name);
                            atomic_write(&dest, &bytes)?;
                            written_files.push(dest);
                            let idx = items.len();
                            let ft_item = AeFootageItem {
                                id: format!("ft_ext_{idx}_{safe_stem}"),
                                name: file_name.clone(),
                                relative_path: format!("{footage_dir_name}/{file_name}"),
                                media_type: format!("video/{ext}"),
                                width: None,
                                height: None,
                            };
                            map.insert(format!("__ext_video__:{ext_rel}"), ft_item.clone());
                            if !vs.resource.is_empty() && !map.contains_key(&vs.resource) {
                                map.insert(vs.resource.clone(), ft_item.clone());
                            }
                            items.push(ft_item);
                        }
                    }
                }
            }
        }
    }

    Ok((items, written_files, map))
}

fn extension_for_media_type(media_type: &str, fallback_path: &str) -> String {
    match media_type.to_ascii_lowercase().as_str() {
        "image/png" => "png".to_owned(),
        "image/jpeg" | "image/jpg" => "jpg".to_owned(),
        "image/webp" => "webp".to_owned(),
        "image/gif" => "gif".to_owned(),
        "image/bmp" => "bmp".to_owned(),
        "image/tiff" => "tif".to_owned(),
        "audio/wav" | "audio/x-wav" => "wav".to_owned(),
        "audio/mpeg" | "audio/mp3" => "mp3".to_owned(),
        "audio/ogg" => "ogg".to_owned(),
        "video/mp4" => "mp4".to_owned(),
        "video/quicktime" => "mov".to_owned(),
        "video/webm" => "webm".to_owned(),
        "video/x-msvideo" | "video/avi" => "avi".to_owned(),
        _ => Path::new(fallback_path)
            .extension()
            .and_then(|e| e.to_str())
            .filter(|e| {
                !e.is_empty() && e.len() <= 8 && e.chars().all(|c| c.is_ascii_alphanumeric())
            })
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_else(|| "bin".to_owned()),
    }
}
