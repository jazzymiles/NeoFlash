use super::*;

/// Run compatibility preflight for `project` against `profile` using default `FlaExportOptions`.
pub fn preflight_fla_export(
    project: &Project,
    profile: FlaTargetProfile,
) -> FlaCompatibilityReport {
    preflight_fla_export_with_options(project, profile, &FlaExportOptions::default())
}

/// Run compatibility preflight for `project` against `profile` with explicit `options`.
pub fn preflight_fla_export_with_options(
    project: &Project,
    profile: FlaTargetProfile,
    options: &FlaExportOptions,
) -> FlaCompatibilityReport {
    let mut report = FlaCompatibilityReport::new(profile);

    // 1. Document settings & scenes
    let max_dim = if matches!(profile, FlaTargetProfile::LegacyFlash8) {
        FLASH8_STAGE_LIMIT
    } else {
        8192
    };
    if project.width > max_dim || project.height > max_dim {
        let fit = (profile == FlaTargetProfile::LegacyFlash8 && options.allow_approximations)
            .then(|| flash8_stage_fit(project.width, project.height))
            .flatten();
        let detail = fit.map_or_else(String::new, |(width, height, _)| {
            format!(
                " The stage and its contents will be scaled proportionally to {width}x{height} px."
            )
        });
        report.push(
            "stage_dimensions_clamped",
            FlaFeatureCategory::DocumentSettings,
            if options.allow_approximations {
                FlaFeatureDisposition::Approximated
            } else {
                FlaFeatureDisposition::Blocking
            },
            "Project",
            format!(
                "Stage size {}x{} exceeds {} maximum {}x{} px.{detail}",
                project.width,
                project.height,
                profile.label(),
                max_dim,
                max_dim
            ),
        );
    } else {
        report.push(
            "document_settings_exact",
            FlaFeatureCategory::DocumentSettings,
            FlaFeatureDisposition::Exact,
            "Project",
            format!(
                "Stage {}x{} and background RGBA {:?} are supported.",
                project.width, project.height, project.background
            ),
        );
    }

    if project.fps > 120 {
        report.push(
            "frame_rate_clamped",
            FlaFeatureCategory::DocumentSettings,
            if options.allow_approximations {
                FlaFeatureDisposition::Approximated
            } else {
                FlaFeatureDisposition::Blocking
            },
            "Project.fps",
            format!(
                "Frame rate {} fps exceeds the FLA/XFL export maximum of 120 fps; export uses 120 fps.",
                project.fps
            ),
        );
    } else {
        report.push(
            "document_frame_rate_exact",
            FlaFeatureCategory::DocumentSettings,
            FlaFeatureDisposition::Exact,
            "Project.fps",
            format!("Frame rate {} fps is supported.", project.fps),
        );
    }

    let scene_ids: Vec<&str> = if project.scenes.is_empty() {
        vec![project.root.as_str()]
    } else {
        project.scenes.iter().map(String::as_str).collect()
    };
    let scene_set: BTreeSet<&str> = scene_ids.iter().copied().collect();
    let library_symbol_count = project
        .symbols
        .iter()
        .filter(|sym| !scene_set.contains(sym.id.as_str()))
        .count();

    if profile.is_legacy_binary() {
        if scene_ids.len() >= usize::from(u16::MAX) {
            report.push(
                "legacy_scene_count_limit",
                FlaFeatureCategory::Scenes,
                FlaFeatureDisposition::Blocking,
                "Project.scenes",
                format!(
                    "Scene count {} reaches or exceeds legacy binary FLA u16 limit ({}).",
                    scene_ids.len(),
                    usize::from(u16::MAX) - 1
                ),
            );
        }
        if library_symbol_count >= usize::from(u16::MAX) {
            report.push(
                "legacy_symbol_count_limit",
                FlaFeatureCategory::SymbolsAndLibrary,
                FlaFeatureDisposition::Blocking,
                "Project.symbols",
                format!(
                    "Library symbol count {library_symbol_count} reaches or exceeds legacy binary FLA u16 limit ({}).",
                    usize::from(u16::MAX) - 1
                ),
            );
        }
    }

    if project.scenes.len() > 1 {
        report.push(
            "scenes_preserved",
            FlaFeatureCategory::Scenes,
            FlaFeatureDisposition::Exact,
            "Project.scenes",
            format!(
                "{} scenes are preserved in play order.",
                project.scenes.len()
            ),
        );
    }

    // 2. Collect script languages across the project & check bundled paths / limits
    for (loc, msg, skipped) in crate::foreign_script_policy_entries(project) {
        report.push(
            if skipped {
                "skipped_foreign_script"
            } else {
                "compatible_script_compiled"
            },
            FlaFeatureCategory::Scripts,
            if skipped {
                FlaFeatureDisposition::Omitted
            } else {
                FlaFeatureDisposition::Converted
            },
            loc,
            msg,
        );
    }
    let project_cow = crate::strip_skipped_foreign_scripts(project);
    let project = project_cow.as_ref();

    let mut has_avm1 = false;
    let mut has_avm2 = false;
    let mut has_web_scripts = false;
    let mut source_archive_paths: BTreeMap<String, String> = BTreeMap::new();
    let mut bundled_script_count = 0usize;

    for (idx, src) in project.sources.iter().enumerate() {
        let loc = format!("SourceUnit({})", src.path);
        classify_script_language(
            src.language,
            &loc,
            profile,
            options,
            &mut has_avm1,
            &mut has_avm2,
            &mut has_web_scripts,
            &mut report,
        );
        let lang_supported = matches!(
            src.language,
            ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
        ) || (profile.supports_as3()
            && src.language == ScriptLanguage::ActionScript3);
        if lang_supported {
            bundled_script_count += 1;
            let rel_path = sanitize_source_rel_path(&src.path, idx);
            let lower = rel_path.to_ascii_lowercase();
            if lower == "mimetype"
                || lower == "publishsettings.xml"
                || lower == "domdocument.xml"
                || lower.starts_with("meta-inf/")
                || lower.starts_with("library/")
                || source_archive_paths
                    .insert(lower, format!("SourceUnit({})", src.path))
                    .is_some()
            {
                report.push(
                    "archive_path_collision",
                    FlaFeatureCategory::Scripts,
                    FlaFeatureDisposition::Blocking,
                    &loc,
                    format!(
                        "Source path {:?} (normalized {:?}) collides with an existing bundled script or XFL package path.",
                        src.path, rel_path
                    ),
                );
            }
            if profile.is_legacy_binary() {
                report.push(
                    "legacy_fla_external_source_approximated",
                    FlaFeatureCategory::Scripts,
                    if options.allow_approximations || options.strip_unsupported_scripts {
                        FlaFeatureDisposition::Approximated
                    } else {
                        FlaFeatureDisposition::Blocking
                    },
                    &loc,
                    format!(
                        "External source file {:?} cannot be stored as a standalone file inside a single-file binary CFB .fla archive ({}); it is preserved in Contents document properties.",
                        src.path,
                        profile.label()
                    ),
                );
            }
        }
    }

    if let Some(doc_class) = &project.document_class
        && !doc_class.trim().is_empty()
    {
        has_avm2 = true;
        let trimmed = doc_class.trim();
        if !is_valid_as3_qname(trimmed) {
            report.push(
                "document_class_invalid",
                FlaFeatureCategory::Scripts,
                FlaFeatureDisposition::Blocking,
                "Project.document_class",
                format!(
                    "Document class {:?} is not a valid ActionScript 3 qualified class name.",
                    trimmed
                ),
            );
        } else if profile.supports_as3() {
            report.push(
                "document_class_preserved",
                FlaFeatureCategory::Scripts,
                FlaFeatureDisposition::Exact,
                "Project.document_class",
                format!("AS3 document class {:?} is preserved.", doc_class),
            );
        } else {
            report.push(
                "document_class_unsupported",
                FlaFeatureCategory::Scripts,
                if options.strip_unsupported_scripts {
                    FlaFeatureDisposition::Omitted
                } else {
                    FlaFeatureDisposition::Blocking
                },
                "Project.document_class",
                format!(
                    "Document class {:?} requires ActionScript 3.0 and cannot be used in {}.",
                    doc_class,
                    profile.label()
                ),
            );
        }
    }

    // 3. Library folders & resources (bitmaps, sounds, fonts, .as includes)
    if !project.library_folders.is_empty() {
        report.push(
            "library_folders_preserved",
            FlaFeatureCategory::SymbolsAndLibrary,
            FlaFeatureDisposition::Exact,
            "Project.library_folders",
            format!(
                "{} library folders are preserved.",
                project.library_folders.len()
            ),
        );
    }

    let bitmap_smoothing_flags = bitmap_smoothing_usage(project);

    let baked_videos = crate::video_frames::used_resources(project);
    let mut legacy_media_count = 0usize;
    let mut seen_res_names_lower: BTreeSet<String> = BTreeSet::new();
    let mut sound_ids: BTreeSet<Id> = BTreeSet::new();
    let mut bitmap_ids: BTreeSet<Id> = BTreeSet::new();

    for res in &project.resources {
        if is_internal_provenance_resource(res) {
            continue;
        }
        let loc = format!("Resource({})", res.path);
        let mime = res.media_type.trim().to_ascii_lowercase();
        if mime.starts_with("image/") {
            bitmap_ids.insert(res.id.clone());
            legacy_media_count += 1;
            match res.decoded_bytes() {
                Err(err) => {
                    report.push(
                        "media_resource_corrupt",
                        FlaFeatureCategory::MediaAndSound,
                        FlaFeatureDisposition::Blocking,
                        loc,
                        format!("Image resource payload failed base64 decoding: {err}"),
                    );
                }
                Ok(raw) => match normalize_fla_bitmap_bytes(&raw) {
                    None => {
                        report.push(
                            "media_resource_unsupported_codec",
                            FlaFeatureCategory::MediaAndSound,
                            FlaFeatureDisposition::Blocking,
                            loc,
                            format!(
                                "Image resource ({}) cannot be decoded into a supported Flash bitmap (PNG/JPEG/GIF/BMP/WebP/TIFF).",
                                res.media_type
                            ),
                        );
                    }
                    Some((_, _, w, h)) => {
                        if profile.is_legacy_binary() && (w > 4096 || h > 4096) {
                            report.push(
                                "legacy_bitmap_dimension_limit",
                                FlaFeatureCategory::MediaAndSound,
                                FlaFeatureDisposition::Blocking,
                                loc,
                                format!(
                                    "Bitmap dimensions {w}x{h} exceed legacy binary FLA maximum of 4096x4096 px."
                                ),
                            );
                        } else if bitmap_smoothing_flags
                            .get(res.id.as_str())
                            .is_some_and(|&(t, f)| t && f)
                        {
                            report.push(
                                "bitmap_smoothing_shared_item",
                                FlaFeatureCategory::MediaAndSound,
                                if options.allow_approximations {
                                    FlaFeatureDisposition::Approximated
                                } else {
                                    FlaFeatureDisposition::Blocking
                                },
                                loc,
                                "Bitmap instances referencing this library asset disagree on allowSmoothing; FLA/XFL stores smoothing per library DOMBitmapItem.".to_owned(),
                            );
                        } else {
                            report.push(
                                "media_resource_preserved",
                                FlaFeatureCategory::MediaAndSound,
                                FlaFeatureDisposition::Exact,
                                loc,
                                format!("Media asset ({}) is bundled.", res.media_type),
                            );
                        }
                    }
                },
            }
        } else if mime.starts_with("audio/") {
            sound_ids.insert(res.id.clone());
            legacy_media_count += 1;
            match res.decoded_bytes() {
                Err(err) => {
                    report.push(
                        "media_resource_corrupt",
                        FlaFeatureCategory::MediaAndSound,
                        FlaFeatureDisposition::Blocking,
                        loc,
                        format!("Audio resource payload failed base64 decoding: {err}"),
                    );
                }
                Ok(raw) => match inspect_audio_codec(&raw) {
                    None => {
                        report.push(
                            "media_resource_unsupported_codec",
                            FlaFeatureCategory::MediaAndSound,
                            FlaFeatureDisposition::Blocking,
                            loc,
                            format!(
                                "Audio resource ({}) is not a supported Flash audio stream (PCM WAV or valid MP3); export is blocked instead of silently corrupting or dropping it.",
                                res.media_type
                            ),
                        );
                    }
                    Some((ext, _)) => {
                        report.push(
                            "media_resource_preserved",
                            FlaFeatureCategory::MediaAndSound,
                            FlaFeatureDisposition::Exact,
                            loc,
                            format!("Audio asset ({ext}) is bundled."),
                        );
                    }
                },
            }
        } else if options.allow_approximations && baked_videos.contains(res.id.as_str()) {
            report.push(
                "video_bitmap_frames",
                FlaFeatureCategory::MediaAndSound,
                FlaFeatureDisposition::Baked,
                loc,
                "Embedded video is converted to timed bitmap frames and a separate soundtrack.",
            );
        } else if mime.starts_with("video/") {
            legacy_media_count += 1;
            match res.decoded_bytes() {
                Err(err) => {
                    report.push(
                        "media_resource_corrupt",
                        FlaFeatureCategory::MediaAndSound,
                        FlaFeatureDisposition::Blocking,
                        loc,
                        format!("Video resource payload failed base64 decoding: {err}"),
                    );
                }
                Ok(raw) => {
                    let diags = neoflash_core::inspect_video_resource(
                        &res.path,
                        &res.media_type,
                        &raw,
                        None,
                    );
                    if diags.iter().any(|d| d.is_error) {
                        report.push(
                            "media_resource_unsupported_codec",
                            FlaFeatureCategory::MediaAndSound,
                            FlaFeatureDisposition::Blocking,
                            loc,
                            format!(
                                "Video resource ({}) is not a valid supported video container.",
                                res.media_type
                            ),
                        );
                    } else if raw.len() > 16 * 1024 * 1024 {
                        report.push(
                            "video_resource_too_large",
                            FlaFeatureCategory::MediaAndSound,
                            FlaFeatureDisposition::Blocking,
                            loc,
                            format!(
                                "Video resource ({}) exceeds 16 MiB XFL entry limit.",
                                res.media_type
                            ),
                        );
                    } else if profile.is_legacy_binary() {
                        report.push(
                            "video_legacy_binary_approximated",
                            FlaFeatureCategory::MediaAndSound,
                            if options.allow_approximations {
                                FlaFeatureDisposition::Approximated
                            } else {
                                FlaFeatureDisposition::Blocking
                            },
                            loc,
                            format!(
                                "Embedded video resource ({}) is approximated as stage video viewports in {}.",
                                res.media_type,
                                profile.label()
                            ),
                        );
                    } else {
                        report.push(
                            "media_resource_preserved",
                            FlaFeatureCategory::MediaAndSound,
                            FlaFeatureDisposition::Exact,
                            loc,
                            format!("Embedded video asset ({}) is bundled.", res.media_type),
                        );
                    }
                }
            }
        } else if crate::fonts::is_font_resource(res) {
            if let Err(err) = res.decoded_bytes() {
                report.push(
                    "font_resource_corrupt",
                    FlaFeatureCategory::TextAndFonts,
                    FlaFeatureDisposition::Blocking,
                    loc,
                    format!("Font resource failed decoding: {err}"),
                );
            } else {
                let disp = if options.outline_unsupported_text
                    || options.font_policy == FontExportPolicy::Outlines
                {
                    FlaFeatureDisposition::Outlined
                } else if profile.is_legacy_binary() {
                    FlaFeatureDisposition::Approximated
                } else {
                    FlaFeatureDisposition::Exact
                };
                report.push(
                    "font_resource",
                    FlaFeatureCategory::TextAndFonts,
                    disp,
                    loc,
                    "Embedded font resource inspected for target profile.".to_owned(),
                );
            }
        } else if is_actionscript_resource(res) {
            match res.decoded_bytes() {
                Err(err) => {
                    report.push(
                        "script_resource_invalid",
                        FlaFeatureCategory::Scripts,
                        FlaFeatureDisposition::Blocking,
                        loc,
                        format!("ActionScript resource failed base64 decoding: {err}"),
                    );
                }
                Ok(bytes) => {
                    if bytes.len() > 1_048_576 {
                        report.push(
                            "script_resource_invalid",
                            FlaFeatureCategory::Scripts,
                            FlaFeatureDisposition::Blocking,
                            loc,
                            "ActionScript #include resource exceeds 1 MiB limit.".to_owned(),
                        );
                    } else if let Err(err) = std::str::from_utf8(&bytes) {
                        report.push(
                            "script_resource_invalid",
                            FlaFeatureCategory::Scripts,
                            FlaFeatureDisposition::Blocking,
                            loc,
                            format!("ActionScript #include resource is not valid UTF-8: {err}"),
                        );
                    } else {
                        let rel = sanitize_source_rel_path(&res.path, 0);
                        let lower = rel.to_ascii_lowercase();
                        if let Some(existing) = source_archive_paths.get(&lower) {
                            let same_as_source =
                                project.sources.iter().enumerate().any(|(i, s)| {
                                    sanitize_source_rel_path(&s.path, i).eq_ignore_ascii_case(&rel)
                                        && s.source.as_bytes() == bytes.as_slice()
                                });
                            if !same_as_source {
                                report.push(
                                    "archive_path_collision",
                                    FlaFeatureCategory::Scripts,
                                    FlaFeatureDisposition::Blocking,
                                    &loc,
                                    format!(
                                        "ActionScript resource {:?} collides with {existing} at archive path {:?}.",
                                        res.path, rel
                                    ),
                                );
                            }
                        } else if !seen_res_names_lower.insert(lower) {
                            report.push(
                                "archive_path_collision",
                                FlaFeatureCategory::Scripts,
                                FlaFeatureDisposition::Blocking,
                                &loc,
                                format!("Duplicate ActionScript resource archive path {:?}.", rel),
                            );
                        } else {
                            bundled_script_count += 1;
                            if profile.is_legacy_binary() {
                                report.push(
                                    "legacy_fla_external_source_approximated",
                                    FlaFeatureCategory::Scripts,
                                    if options.allow_approximations
                                        || options.strip_unsupported_scripts
                                    {
                                        FlaFeatureDisposition::Approximated
                                    } else {
                                        FlaFeatureDisposition::Blocking
                                    },
                                    &loc,
                                    format!(
                                        "Bundled ActionScript include {:?} is preserved in Contents document properties for {}.",
                                        res.path,
                                        profile.label()
                                    ),
                                );
                            } else {
                                report.push(
                                    "script_include_preserved",
                                    FlaFeatureCategory::Scripts,
                                    FlaFeatureDisposition::Exact,
                                    &loc,
                                    format!("ActionScript include {:?} is bundled.", res.path),
                                );
                            }
                        }
                    }
                }
            }
        } else if is_unreferenced_sidecar_resource(project, res) {
            report.push(
                "non_flash_sidecar_omitted",
                FlaFeatureCategory::MediaAndSound,
                if options.allow_approximations {
                    FlaFeatureDisposition::Approximated
                } else {
                    FlaFeatureDisposition::Blocking
                },
                loc,
                format!(
                    "Unreferenced text/JSON sidecar resource {:?} ({}) is omitted from {}.",
                    res.path,
                    res.media_type,
                    profile.label()
                ),
            );
        } else {
            report.push(
                "resource_unsupported",
                FlaFeatureCategory::MediaAndSound,
                FlaFeatureDisposition::Blocking,
                loc,
                format!(
                    "Resource media type {:?} is not representable in {} and cannot be silently omitted.",
                    res.media_type,
                    profile.label()
                ),
            );
        }
    }

    if bundled_script_count > 1024 {
        report.push(
            "source_count_limit",
            FlaFeatureCategory::Scripts,
            FlaFeatureDisposition::Blocking,
            "Project.sources",
            format!(
                "Project contains {bundled_script_count} external ActionScript files, exceeding the 1024-file FLA/XFL limit."
            ),
        );
    }
    if profile.is_legacy_binary() && legacy_media_count >= usize::from(u16::MAX) {
        report.push(
            "legacy_media_count_limit",
            FlaFeatureCategory::MediaAndSound,
            FlaFeatureDisposition::Blocking,
            "Project.resources",
            format!(
                "Media library item count {legacy_media_count} reaches or exceeds legacy binary FLA u16 limit ({}).",
                usize::from(u16::MAX) - 1
            ),
        );
    }

    // 4. Symbols, timelines, layers, keyframes, elements, scripts
    let mut used_classes: BTreeSet<String> = BTreeSet::new();
    let mut used_linkages: BTreeSet<String> = BTreeSet::new();
    for sym in &project.symbols {
        let sym_loc = format!("Symbol({})", sym.name);
        if sym.scale9_grid().is_some() {
            let disp = if profile.is_legacy_binary() {
                FlaFeatureDisposition::Approximated
            } else {
                FlaFeatureDisposition::Exact
            };
            report.push(
                "symbol_scale9_grid",
                FlaFeatureCategory::SymbolsAndLibrary,
                disp,
                &sym_loc,
                "Symbol 9-slice scaling grid inspected.".to_owned(),
            );
        }

        if let Some(cls) = &sym.class_name
            && !cls.trim().is_empty()
        {
            let trimmed = cls.trim();
            if !is_valid_as3_qname(trimmed) {
                report.push(
                    "symbol_class_invalid",
                    FlaFeatureCategory::SymbolsAndLibrary,
                    FlaFeatureDisposition::Blocking,
                    &sym_loc,
                    format!(
                        "Symbol class {:?} is not a valid ActionScript 3 qualified class name.",
                        trimmed
                    ),
                );
            } else if !used_classes.insert(trimmed.to_owned()) {
                report.push(
                    "symbol_duplicate_linkage",
                    FlaFeatureCategory::SymbolsAndLibrary,
                    FlaFeatureDisposition::Blocking,
                    &sym_loc,
                    format!(
                        "Duplicate symbol className {:?} across library symbols.",
                        trimmed
                    ),
                );
            } else if profile.supports_as3() {
                has_avm2 = true;
            } else {
                report.push(
                    "symbol_as3_class_converted",
                    FlaFeatureCategory::SymbolsAndLibrary,
                    FlaFeatureDisposition::Converted,
                    &sym_loc,
                    format!(
                        "Symbol class {:?} converted to AVM1 linkage identifier for {}.",
                        trimmed,
                        profile.label()
                    ),
                );
            }
        }
        if let Some(lid) = &sym.linkage_id
            && !lid.trim().is_empty()
            && !used_linkages.insert(lid.trim().to_owned())
        {
            report.push(
                "symbol_duplicate_linkage",
                FlaFeatureCategory::SymbolsAndLibrary,
                FlaFeatureDisposition::Blocking,
                &sym_loc,
                format!(
                    "Duplicate symbol linkageIdentifier {:?} across library symbols.",
                    lid.trim()
                ),
            );
        }

        if profile.is_legacy_binary() && sym.timeline.layers.len() >= usize::from(u16::MAX) {
            report.push(
                "legacy_layer_count_limit",
                FlaFeatureCategory::LayersAndMasks,
                FlaFeatureDisposition::Blocking,
                &sym_loc,
                format!(
                    "Layer count {} reaches or exceeds legacy binary FLA u16 limit.",
                    sym.timeline.layers.len()
                ),
            );
        }

        for fs in &sym.timeline.scripts {
            let fs_loc = format!("{}:frame[{}]", sym_loc, fs.frame);
            classify_script_language(
                fs.script.language,
                &fs_loc,
                profile,
                options,
                &mut has_avm1,
                &mut has_avm2,
                &mut has_web_scripts,
                &mut report,
            );
        }

        for layer in &sym.timeline.layers {
            let layer_loc = format!("{}/Layer({})", sym_loc, layer.name);
            preflight_layer(
                project,
                sym,
                layer,
                &layer_loc,
                profile,
                options,
                &bitmap_ids,
                &sound_ids,
                &mut has_avm1,
                &mut has_avm2,
                &mut has_web_scripts,
                &mut report,
            );
        }
    }

    // 5. Mixed AVM1 + AVM2 check
    if has_avm1 && has_avm2 {
        report.push(
            "mixed_avm1_avm2_scripts",
            FlaFeatureCategory::Scripts,
            if options.strip_unsupported_scripts {
                FlaFeatureDisposition::Omitted
            } else {
                FlaFeatureDisposition::Blocking
            },
            "Project.scripts",
            "Project mixes ActionScript 1/2 (AVM1) and ActionScript 3 (AVM2) scripts in one document.".to_owned(),
        );
    }

    report
}
