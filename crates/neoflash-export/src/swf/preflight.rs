//! SWF compatibility preflight diagnostics (`preflight_swf_publish`).

use super::*;

fn as3_spec_extends_target(
    spec: &AbcClassSpec,
    known: &BTreeMap<String, AbcClassSpec>,
    target_pkg: &str,
    target_name: &str,
) -> bool {
    let mut cur = spec;
    for _ in 0..32 {
        if cur.super_pkg == target_pkg && cur.super_name == target_name {
            return true;
        }
        let sup_qname = if cur.super_pkg.is_empty() {
            cur.super_name.clone()
        } else {
            format!("{}.{}", cur.super_pkg, cur.super_name)
        };
        if let Some(next) = known.get(&sup_qname) {
            cur = next;
        } else if let Some(next) = known.values().find(|c| c.class_name == cur.super_name) {
            cur = next;
        } else {
            break;
        }
    }
    false
}

/// Run a comprehensive compatibility preflight for publishing `project` as a `.swf` movie.
#[must_use]
pub fn preflight_swf_publish(
    project: &Project,
    options: &SwfPublishOptions,
) -> FlaCompatibilityReport {
    let equivalent_fla_profile = match options.profile {
        SwfPublishProfile::FlashPlayer8Avm1 => FlaTargetProfile::LegacyFlash8,
        SwfPublishProfile::FlashPlayer9Avm2 => FlaTargetProfile::LegacyFlashCS4,
        SwfPublishProfile::FlashPlayer10Avm2 => FlaTargetProfile::FlashCS6,
    };
    let mut report = FlaCompatibilityReport::new(equivalent_fla_profile);

    // 1. Document settings & scenes
    report.push(
        "swf_document_header",
        FlaCompatibilityCategory::DocumentSettings,
        FlaCompatibilityStatus::Exact,
        format!("project:{}", project.name),
        format!(
            "SWF header {}x{} px ({}x{} twips), profile {:?} ({:?}).",
            project.width,
            project.height,
            project.width.saturating_mul(20),
            project.height.saturating_mul(20),
            options.profile,
            options.compression
        ),
    );
    if !(1..=255).contains(&project.fps) {
        report.push(
            "swf_frame_rate_limit",
            FlaCompatibilityCategory::DocumentSettings,
            FlaCompatibilityStatus::Blocking,
            format!("project:{}", project.name),
            format!(
                "Project frame rate is {} fps; this SWF writer can represent integer frame rates from 1 through 255 without changing playback speed.",
                project.fps
            ),
        );
    } else {
        report.push(
            "swf_frame_rate_exact",
            FlaCompatibilityCategory::DocumentSettings,
            FlaCompatibilityStatus::Exact,
            "Project.fps",
            format!("Frame rate {} fps is represented exactly.", project.fps),
        );
    }

    if project.scenes.len() > 1 {
        report.push(
            "swf_scenes_flattened",
            FlaCompatibilityCategory::Scenes,
            FlaCompatibilityStatus::Converted,
            "project.scenes",
            format!(
                "{} scenes are flattened into the root SWF timeline and indexed via DefineSceneAndFrameLabelData.",
                project.scenes.len()
            ),
        );
    }

    let (flattened_project, _) = flatten_scenes_with_ranges(project);
    let root_has_timeline = flattened_project
        .symbol(&flattened_project.root)
        .is_some_and(|root| {
            if root.timeline.frame_count > u32::from(u16::MAX) {
                report.push(
                    "swf_flattened_scene_frame_limit",
                    FlaCompatibilityCategory::Scenes,
                    FlaCompatibilityStatus::Blocking,
                    "project.scenes",
                    format!(
                        "Flattened scenes need {} frames; SWF declares at most {} frames.",
                        root.timeline.frame_count,
                        u16::MAX
                    ),
                );
            }
            if let Err(issue) = compute_swf_timeline_layout(&root.timeline) {
                push_swf_layout_issue(&mut report, "project.scenes".to_owned(), true, issue);
            }
            root.timeline.frame_count > 1
                || root
                    .timeline
                    .scripts
                    .iter()
                    .any(|s| s.script.language == ScriptLanguage::ActionScript3)
        });

    let (used_bitmaps, used_sounds) = used_swf_media_resources(project);
    for resource_id in used_bitmaps {
        let location = format!("resource:{resource_id}");
        let Some(resource) = project.resource(&resource_id) else {
            report.push(
                "swf_bitmap_resource_missing",
                FlaCompatibilityCategory::MediaAndSound,
                FlaCompatibilityStatus::Blocking,
                location,
                format!("Placed bitmap references missing resource '{resource_id}'."),
            );
            continue;
        };
        match resource.decode_image() {
            Ok(_) => {
                let jpeg = embeds_original_jpeg(resource);
                report.push(
                    if jpeg {
                        "swf_bitmap_jpeg"
                    } else {
                        "swf_bitmap_lossless"
                    },
                    FlaCompatibilityCategory::MediaAndSound,
                    FlaCompatibilityStatus::Exact,
                    location,
                    format!(
                        "Bitmap '{}' embeds as {}.",
                        resource.path,
                        if jpeg {
                            "DefineBitsJPEG2"
                        } else {
                            "DefineBitsLossless2"
                        }
                    ),
                );
            }
            Err(error) => report.push(
                "swf_bitmap_data_unsupported",
                FlaCompatibilityCategory::MediaAndSound,
                FlaCompatibilityStatus::Blocking,
                location,
                format!("Bitmap '{}' cannot be decoded: {error}", resource.path),
            ),
        }
    }
    for resource_id in used_sounds.into_keys() {
        let location = format!("resource:{resource_id}");
        let Some(resource) = project.resource(&resource_id) else {
            report.push(
                "swf_sound_resource_missing",
                FlaCompatibilityCategory::MediaAndSound,
                FlaCompatibilityStatus::Blocking,
                location,
                format!("Frame sound references missing resource '{resource_id}'."),
            );
            continue;
        };
        let is_supported_codec = resource.media_type.eq_ignore_ascii_case("audio/wav")
            || resource.media_type.eq_ignore_ascii_case("audio/x-wav")
            || resource.media_type.eq_ignore_ascii_case("audio/wave")
            || resource.media_type.eq_ignore_ascii_case("audio/mpeg")
            || resource.media_type.eq_ignore_ascii_case("audio/mp3");
        if !is_supported_codec {
            report.push(
                "swf_sound_codec_unsupported",
                FlaCompatibilityCategory::MediaAndSound,
                FlaCompatibilityStatus::Blocking,
                location,
                format!(
                    "Frame sound '{}' uses {}; this SWF writer currently embeds only proven PCM WAV and MP3 layouts. Convert it to a supported WAV or MP3 first.",
                    resource.path, resource.media_type
                ),
            );
            continue;
        }
        let Ok(raw) = resource.decoded_bytes() else {
            report.push(
                "swf_sound_data_invalid",
                FlaCompatibilityCategory::MediaAndSound,
                FlaCompatibilityStatus::Blocking,
                location,
                format!("Frame sound '{}' has invalid resource data.", resource.path),
            );
            continue;
        };
        match analyze_swf_sound_resource(&resource.media_type, &raw) {
            Ok(info) => report.push(
                "swf_sound_embedded",
                FlaCompatibilityCategory::MediaAndSound,
                FlaCompatibilityStatus::Exact,
                location,
                format!(
                    "Frame sound '{}' is embedded with {} samples at {} Hz ({}).",
                    resource.path,
                    info.sample_count,
                    info.sample_rate,
                    if info.stereo { "stereo" } else { "mono" }
                ),
            ),
            Err(issue) => report.push(
                "swf_sound_layout_unsupported",
                FlaCompatibilityCategory::MediaAndSound,
                FlaCompatibilityStatus::Blocking,
                location,
                format!(
                    "Frame sound '{}' cannot be embedded without changing or corrupting it: {issue}",
                    resource.path
                ),
            ),
        }
    }

    // Track script languages for mixed-VM detection & SymbolClass/linkage uniqueness
    for (loc, msg, skipped) in crate::foreign_script_policy_entries(project) {
        report.push(
            if skipped {
                "swf_skipped_foreign_script"
            } else {
                "swf_compatible_script_compiled"
            },
            FlaCompatibilityCategory::Scripts,
            if skipped {
                FlaCompatibilityStatus::Omitted
            } else {
                FlaCompatibilityStatus::Approximated
            },
            loc,
            msg,
        );
    }
    let project_cow = crate::strip_skipped_foreign_scripts(project);
    let project = project_cow.as_ref();

    let mut has_avm1_scripts = false;
    let mut has_avm2_scripts = false;
    let mut parsed_external_as3_classes: BTreeMap<String, AbcClassSpec> = BTreeMap::new();
    let mut used_avm2_symbol_classes: BTreeMap<String, String> = BTreeMap::new();
    let mut used_avm1_linkages: BTreeMap<String, String> = BTreeMap::new();

    let included_source_paths = swf_collect_included_source_paths(project);

    // 2. Inspect external sources first so class definitions are available for document/symbol validation
    for (idx, src) in project.sources.iter().enumerate() {
        let loc = format!("sources[{idx}]:{}", src.path);
        match src.language {
            ScriptLanguage::JavaScript | ScriptLanguage::TypeScript => {
                report.push(
                    "swf_js_ts_source_unsupported",
                    FlaCompatibilityCategory::Scripts,
                    if options.scriptless_mode {
                        FlaCompatibilityStatus::Omitted
                    } else {
                        FlaCompatibilityStatus::Blocking
                    },
                    loc,
                    format!(
                        "{:?} source '{}' cannot be compiled into SWF AVM bytecode.",
                        src.language, src.path
                    ),
                );
            }
            ScriptLanguage::ActionScript3 => {
                has_avm2_scripts = true;
                let norm_src_lower = src
                    .path
                    .trim()
                    .replace('\\', "/")
                    .trim_start_matches('/')
                    .to_ascii_lowercase();
                let expanded_res =
                    expand_swf_includes(project, src.language, &src.path, &src.source);
                let is_included_snippet = included_source_paths.contains(&norm_src_lower)
                    && expanded_res
                        .as_ref()
                        .is_ok_and(|expanded| !as3_source_declares_package_or_class(expanded));
                let parsed_spec = if is_included_snippet {
                    None
                } else {
                    expanded_res
                        .as_ref()
                        .ok()
                        .and_then(|expanded| try_compile_avm2_source(expanded, true).ok())
                };
                let compile_issue = match &expanded_res {
                    Err(err) => Some(err.clone()),
                    Ok(expanded) if is_included_snippet => {
                        try_compile_avm2_source(expanded, false).err()
                    }
                    Ok(expanded) => try_compile_avm2_source(expanded, true).err(),
                };
                let duplicate_qname = if compile_issue.is_none()
                    && options.profile.is_avm2()
                    && !options.scriptless_mode
                    && let Some(spec) = &parsed_spec
                {
                    let qname = if spec.pkg.is_empty() {
                        spec.class_name.clone()
                    } else {
                        format!("{}.{}", spec.pkg, spec.class_name)
                    };
                    match parsed_external_as3_classes.entry(qname) {
                        std::collections::btree_map::Entry::Occupied(entry) => {
                            Some(entry.key().clone())
                        }
                        std::collections::btree_map::Entry::Vacant(entry) => {
                            entry.insert(spec.clone());
                            None
                        }
                    }
                } else {
                    None
                };
                report.push(
                    if duplicate_qname.is_some() {
                        "swf_duplicate_as3_class"
                    } else if options.profile.is_avm2() && compile_issue.is_none() {
                        if is_included_snippet {
                            "swf_as3_included_source"
                        } else {
                            "swf_as3_source_doabc"
                        }
                    } else if options.profile.is_avm2() {
                        "swf_as3_source_unsupported_syntax"
                    } else {
                        "swf_as3_source_in_avm1"
                    },
                    FlaCompatibilityCategory::Scripts,
                    if options.scriptless_mode {
                        FlaCompatibilityStatus::Omitted
                    } else if options.profile.is_avm2()
                        && compile_issue.is_none()
                        && duplicate_qname.is_none()
                    {
                        FlaCompatibilityStatus::Exact
                    } else {
                        FlaCompatibilityStatus::Blocking
                    },
                    loc,
                    if let Some(qname) = duplicate_qname {
                        format!("Duplicate AS3 class definition '{qname}' in project sources.")
                    } else if options.profile.is_avm2() && compile_issue.is_none() {
                        if is_included_snippet {
                            format!(
                                "AS3 include source '{}' expanded into referencing script(s).",
                                src.path
                            )
                        } else {
                            format!("AS3 source '{}' compiled into DoABC2 tag.", src.path)
                        }
                    } else if let Some(issue) = compile_issue {
                        format!(
                            "AS3 source '{}' cannot be compiled losslessly: {issue}.",
                            src.path
                        )
                    } else {
                        format!(
                            "AS3 source '{}' requires FlashPlayer9Avm2 or FlashPlayer10Avm2.",
                            src.path
                        )
                    },
                );
            }
            ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2 => {
                has_avm1_scripts = true;
                let compile_issue = swf_script_compilation_issue(
                    project,
                    src.language,
                    &src.path,
                    &src.source,
                    true,
                    false,
                );
                report.push(
                    if !options.profile.is_avm2() && compile_issue.is_none() {
                        "swf_avm1_source"
                    } else if !options.profile.is_avm2() {
                        "swf_avm1_source_unsupported_syntax"
                    } else {
                        "swf_avm1_source_in_avm2"
                    },
                    FlaCompatibilityCategory::Scripts,
                    if options.scriptless_mode {
                        FlaCompatibilityStatus::Omitted
                    } else if !options.profile.is_avm2() && compile_issue.is_none() {
                        FlaCompatibilityStatus::Exact
                    } else {
                        FlaCompatibilityStatus::Blocking
                    },
                    loc,
                    if !options.profile.is_avm2() && compile_issue.is_none() {
                        format!("AVM1 source '{}' compiled into DoInitAction.", src.path)
                    } else if let Some(issue) = compile_issue {
                        format!(
                            "AS1/AS2 source '{}' cannot be compiled losslessly: {issue}.",
                            src.path
                        )
                    } else {
                        format!(
                            "AS1/AS2 source '{}' cannot run in an AVM2 SWF movie.",
                            src.path
                        )
                    },
                );
            }
        }
    }

    if let Some(doc_cls) = &project.document_class
        && !doc_cls.trim().is_empty()
    {
        let clean = doc_cls.trim();
        has_avm2_scripts = true;
        if !options.profile.is_avm2() {
            report.push(
                "swf_document_class_avm1_unsupported",
                FlaCompatibilityCategory::Scripts,
                if options.scriptless_mode {
                    FlaCompatibilityStatus::Omitted
                } else {
                    FlaCompatibilityStatus::Blocking
                },
                "project.document_class",
                format!(
                    "AS3 document class '{clean}' cannot be bound in FlashPlayer8Avm1 SWF profile."
                ),
            );
        } else if options.scriptless_mode {
            report.push(
                "swf_document_class_omitted",
                FlaCompatibilityCategory::Scripts,
                FlaCompatibilityStatus::Omitted,
                "project.document_class",
                format!("Document class '{clean}' is omitted in scriptless_mode."),
            );
        } else if !is_valid_as3_qname(clean) {
            report.push(
                "swf_invalid_document_class",
                FlaCompatibilityCategory::Scripts,
                FlaCompatibilityStatus::Blocking,
                "project.document_class",
                format!("Document class '{clean}' is not a valid ActionScript 3 class name."),
            );
        } else if let Some(ext_spec) = parsed_external_as3_classes.get(clean)
            && root_has_timeline
            && !as3_spec_extends_target(
                ext_spec,
                &parsed_external_as3_classes,
                "flash.display",
                "MovieClip",
            )
        {
            report.push(
                "swf_document_class_requires_movieclip",
                FlaCompatibilityCategory::Scripts,
                FlaCompatibilityStatus::Blocking,
                "project.document_class",
                format!(
                    "Document class '{clean}' extends {}.{} but main timeline has multiple frames or frame scripts requiring flash.display.MovieClip.",
                    ext_spec.super_pkg, ext_spec.super_name
                ),
            );
        } else {
            used_avm2_symbol_classes.insert(clean.to_owned(), "project.document_class".to_owned());
            report.push(
                "swf_document_class",
                FlaCompatibilityCategory::Scripts,
                FlaCompatibilityStatus::Exact,
                "project.document_class",
                format!("Document class '{clean}' bound to root via SymbolClass (ID 0)."),
            );
        }
    }

    // 3. Inspect symbols, timelines, elements, tweens, and scripts
    let mut font_probe = SwfOutlineFonts::default();
    for sym in &project.symbols {
        let sym_loc = format!("symbol:{}", sym.name);
        if sym.timeline.frame_count > u32::from(u16::MAX) {
            report.push(
                "swf_timeline_frame_limit",
                FlaCompatibilityCategory::AnimationAndRigging,
                FlaCompatibilityStatus::Blocking,
                sym_loc.clone(),
                format!(
                    "Timeline has {} frames; SWF declares at most {} frames without truncation.",
                    sym.timeline.frame_count,
                    u16::MAX
                ),
            );
        }
        if let Err(issue) = compute_swf_timeline_layout(&sym.timeline) {
            push_swf_layout_issue(&mut report, sym_loc.clone(), false, issue);
        }

        let trimmed_class = sym
            .class_name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let trimmed_linkage = sym
            .linkage_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let has_as3_sym_scripts = sym
            .timeline
            .scripts
            .iter()
            .any(|s| s.script.language == ScriptLanguage::ActionScript3);

        if let Some(cls) = trimmed_class {
            has_avm2_scripts = true;
            if !options.profile.is_avm2() {
                report.push(
                    "swf_symbol_class_in_avm1",
                    FlaCompatibilityCategory::SymbolsAndLibrary,
                    if options.scriptless_mode {
                        FlaCompatibilityStatus::Omitted
                    } else {
                        FlaCompatibilityStatus::Blocking
                    },
                    sym_loc.clone(),
                    format!("Symbol AS3 class '{cls}' requires an AVM2 SWF profile."),
                );
            } else if !options.scriptless_mode {
                if !is_valid_as3_qname(cls) {
                    report.push(
                        "swf_invalid_symbol_class",
                        FlaCompatibilityCategory::SymbolsAndLibrary,
                        FlaCompatibilityStatus::Blocking,
                        sym_loc.clone(),
                        format!("Symbol class '{cls}' is not a valid ActionScript 3 class name."),
                    );
                } else if sym.kind == SymbolKind::Graphic {
                    report.push(
                        "swf_graphic_symbol_class_unsupported",
                        FlaCompatibilityCategory::SymbolsAndLibrary,
                        FlaCompatibilityStatus::Blocking,
                        sym_loc.clone(),
                        format!(
                            "Graphic symbol '{}' cannot be bound to AS3 class '{cls}'; convert it to MovieClip or Button.",
                            sym.name
                        ),
                    );
                } else if let Some(prev_loc) =
                    used_avm2_symbol_classes.insert(cls.to_owned(), sym_loc.clone())
                {
                    report.push(
                        "swf_duplicate_symbol_class",
                        FlaCompatibilityCategory::SymbolsAndLibrary,
                        FlaCompatibilityStatus::Blocking,
                        sym_loc.clone(),
                        format!("Duplicate AVM2 SymbolClass '{cls}' also bound at {prev_loc}."),
                    );
                } else if let Some(ext_spec) = parsed_external_as3_classes.get(cls) {
                    if sym.kind == SymbolKind::MovieClip
                        && (has_as3_sym_scripts || sym.timeline.frame_count > 1)
                        && !as3_spec_extends_target(
                            ext_spec,
                            &parsed_external_as3_classes,
                            "flash.display",
                            "MovieClip",
                        )
                    {
                        report.push(
                            "swf_symbol_class_requires_movieclip",
                            FlaCompatibilityCategory::SymbolsAndLibrary,
                            FlaCompatibilityStatus::Blocking,
                            sym_loc.clone(),
                            format!(
                                "Symbol class '{cls}' extends {}.{} but symbol timeline has multiple frames or frame scripts requiring flash.display.MovieClip.",
                                ext_spec.super_pkg, ext_spec.super_name
                            ),
                        );
                    } else if sym.kind == SymbolKind::Button
                        && !as3_spec_extends_target(
                            ext_spec,
                            &parsed_external_as3_classes,
                            "flash.display",
                            "SimpleButton",
                        )
                    {
                        report.push(
                            "swf_button_class_requires_simplebutton",
                            FlaCompatibilityCategory::SymbolsAndLibrary,
                            FlaCompatibilityStatus::Blocking,
                            sym_loc.clone(),
                            format!(
                                "Button symbol class '{cls}' extends {}.{} instead of flash.display.SimpleButton.",
                                ext_spec.super_pkg, ext_spec.super_name
                            ),
                        );
                    }
                }
            }
        }

        if let Some(link) = trimmed_linkage {
            if !options.profile.is_avm2() {
                if let Some(prev_loc) = used_avm1_linkages.insert(link.to_owned(), sym_loc.clone())
                {
                    report.push(
                        "swf_duplicate_linkage_id",
                        FlaCompatibilityCategory::SymbolsAndLibrary,
                        FlaCompatibilityStatus::Blocking,
                        sym_loc.clone(),
                        format!(
                            "Duplicate AVM1 ExportAssets linkage identifier '{link}' (already used at {prev_loc})."
                        ),
                    );
                }
            } else if !options.scriptless_mode {
                if sym.kind == SymbolKind::Graphic {
                    report.push(
                        "swf_graphic_linkage_unsupported",
                        FlaCompatibilityCategory::SymbolsAndLibrary,
                        FlaCompatibilityStatus::Blocking,
                        sym_loc.clone(),
                        format!(
                            "Graphic symbol '{}' cannot export linkage identifier '{link}' in AVM2 SWF.",
                            sym.name
                        ),
                    );
                } else if let Some(cls) = trimmed_class {
                    if cls != link {
                        report.push(
                            "swf_avm2_linkage_superseded_by_class",
                            FlaCompatibilityCategory::SymbolsAndLibrary,
                            FlaCompatibilityStatus::Omitted,
                            sym_loc.clone(),
                            format!(
                                "Symbol '{}' defines AVM1 linkage_id '{link}' alongside AS3 class_name '{cls}'; AVM2 SymbolClass binds '{cls}' and omits the AVM1 linkage identifier.",
                                sym.name
                            ),
                        );
                    }
                } else if !is_valid_as3_qname(link) {
                    report.push(
                        "swf_avm2_invalid_linkage_class",
                        FlaCompatibilityCategory::SymbolsAndLibrary,
                        FlaCompatibilityStatus::Blocking,
                        sym_loc.clone(),
                        format!(
                            "Linkage identifier '{link}' on '{}' is not a valid AS3 SymbolClass name.",
                            sym.name
                        ),
                    );
                } else if let Some(prev_loc) =
                    used_avm2_symbol_classes.insert(link.to_owned(), sym_loc.clone())
                {
                    report.push(
                        "swf_duplicate_symbol_class",
                        FlaCompatibilityCategory::SymbolsAndLibrary,
                        FlaCompatibilityStatus::Blocking,
                        sym_loc.clone(),
                        format!("Duplicate AVM2 SymbolClass '{link}' also bound at {prev_loc}."),
                    );
                } else {
                    report.push(
                        "swf_avm2_linkage_to_symbol_class",
                        FlaCompatibilityCategory::SymbolsAndLibrary,
                        FlaCompatibilityStatus::Converted,
                        sym_loc.clone(),
                        format!(
                            "Linkage identifier '{link}' exported as AVM2 SymbolClass subclass."
                        ),
                    );
                }
            }
        }

        if !sym.timeline.scripts.is_empty() && !options.scriptless_mode {
            if sym.kind == SymbolKind::Button {
                report.push(
                    "swf_button_frame_script_unsupported",
                    FlaCompatibilityCategory::Scripts,
                    FlaCompatibilityStatus::Blocking,
                    sym_loc.clone(),
                    format!(
                        "Button symbol '{}' cannot execute timeline frame scripts inside DefineButton2; attach click scripts on the Button instance instead.",
                        sym.name
                    ),
                );
            } else if sym.kind == SymbolKind::Graphic {
                report.push(
                    "swf_graphic_frame_script_unsupported",
                    FlaCompatibilityCategory::Scripts,
                    FlaCompatibilityStatus::Blocking,
                    sym_loc.clone(),
                    format!(
                        "Graphic symbol '{}' cannot contain timeline frame scripts; convert it to MovieClip.",
                        sym.name
                    ),
                );
            }
        }

        for fs in &sym.timeline.scripts {
            let fs_loc = format!("{sym_loc}:frame[{}].script", fs.frame);
            match fs.script.language {
                ScriptLanguage::JavaScript | ScriptLanguage::TypeScript => {
                    report.push(
                        "swf_js_ts_frame_script",
                        FlaCompatibilityCategory::Scripts,
                        if options.scriptless_mode {
                            FlaCompatibilityStatus::Omitted
                        } else {
                            FlaCompatibilityStatus::Blocking
                        },
                        fs_loc,
                        format!(
                            "{:?} frame script cannot be compiled to SWF bytecode.",
                            fs.script.language
                        ),
                    );
                }
                ScriptLanguage::ActionScript3 => {
                    has_avm2_scripts = true;
                    let compile_issue = swf_script_compilation_issue(
                        project,
                        fs.script.language,
                        "",
                        &fs.script.source,
                        false,
                        false,
                    );
                    report.push(
                        if options.profile.is_avm2() && compile_issue.is_none() {
                            "swf_avm2_frame_script"
                        } else if options.profile.is_avm2() {
                            "swf_avm2_frame_script_unsupported_syntax"
                        } else {
                            "swf_avm2_frame_script_in_avm1"
                        },
                        FlaCompatibilityCategory::Scripts,
                        if options.scriptless_mode {
                            FlaCompatibilityStatus::Omitted
                        } else if options.profile.is_avm2() && compile_issue.is_none() {
                            FlaCompatibilityStatus::Exact
                        } else {
                            FlaCompatibilityStatus::Blocking
                        },
                        fs_loc,
                        compile_issue.map_or_else(
                            || {
                                "AS3 frame script compiled into AVM2 addFrameScript method."
                                    .to_owned()
                            },
                            |issue| {
                                format!("AS3 frame script cannot be compiled losslessly: {issue}.")
                            },
                        ),
                    );
                }
                ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2 => {
                    has_avm1_scripts = true;
                    let compile_issue = swf_script_compilation_issue(
                        project,
                        fs.script.language,
                        "",
                        &fs.script.source,
                        false,
                        false,
                    );
                    report.push(
                        if !options.profile.is_avm2() && compile_issue.is_none() {
                            "swf_avm1_frame_script"
                        } else if !options.profile.is_avm2() {
                            "swf_avm1_frame_script_unsupported_syntax"
                        } else {
                            "swf_avm1_frame_script_in_avm2"
                        },
                        FlaCompatibilityCategory::Scripts,
                        if options.scriptless_mode {
                            FlaCompatibilityStatus::Omitted
                        } else if !options.profile.is_avm2() && compile_issue.is_none() {
                            FlaCompatibilityStatus::Exact
                        } else {
                            FlaCompatibilityStatus::Blocking
                        },
                        fs_loc,
                        compile_issue.map_or_else(
                            || "AS1/AS2 frame script compiled into AVM1 DoAction tag.".to_owned(),
                            |issue| {
                                format!(
                                    "AS1/AS2 frame script cannot be compiled losslessly: {issue}."
                                )
                            },
                        ),
                    );
                }
            }
        }

        let mut timeline_stream_occupied: HashSet<u32> = HashSet::new();
        let sym_total_frames = sym.timeline.frame_count.max(1);
        for layer in &sym.timeline.layers {
            let bake_video = crate::video_frames::bakeable(project, layer);
            let layer_loc = format!("{sym_loc}:layer:{}", layer.name);
            let layer_has_authored_content = layer
                .keyframes
                .iter()
                .any(|k| !k.elements.is_empty() || k.sound.is_some() || !k.label.is_empty());
            if !layer.visible {
                if layer_has_authored_content {
                    report.push(
                        "swf_hidden_layer_excluded",
                        FlaCompatibilityCategory::LayersAndMasks,
                        FlaCompatibilityStatus::Omitted,
                        layer_loc.clone(),
                        format!(
                            "Hidden layer '{}' is excluded from published SWF output.",
                            layer.name
                        ),
                    );
                }
                continue;
            }
            if layer.properties.role == LayerRole::Guide {
                if layer_has_authored_content {
                    report.push(
                        "swf_guide_layer_excluded",
                        FlaCompatibilityCategory::LayersAndMasks,
                        FlaCompatibilityStatus::Omitted,
                        layer_loc.clone(),
                        format!(
                            "Guide layer '{}' graphics are omitted from SWF (motion guide paths remain baked into guided tweens).",
                            layer.name
                        ),
                    );
                }
                continue;
            }
            if layer.properties.role != LayerRole::Folder
                && (!layer.properties.filters.is_empty()
                    || layer.properties.blend_mode != BlendMode::Normal
                    || layer.properties.z_depth.abs() > 1e-6
                    || layer.properties.attach_to_camera)
            {
                report.push(
                    "swf_layer_effects_unsupported",
                    FlaCompatibilityCategory::FiltersAndBlends,
                    FlaCompatibilityStatus::Blocking,
                    layer_loc.clone(),
                    format!(
                        "Layer '{}' uses layer-level filters, blend mode, z-depth, or camera attachment not supported by SWF display-list tags.",
                        layer.name
                    ),
                );
            }

            for (k_idx, kf) in layer.keyframes.iter().enumerate() {
                let kf_loc = format!("{layer_loc}:frame[{}]", kf.frame);
                if !kf.filters.is_empty() {
                    report.push(
                        "swf_keyframe_filters_unsupported",
                        FlaCompatibilityCategory::FiltersAndBlends,
                        FlaCompatibilityStatus::Blocking,
                        kf_loc.clone(),
                        "Keyframe-level filter stacks are not represented in SWF PlaceObject3."
                            .to_owned(),
                    );
                }
                if bake_video && kf.video.is_some() {
                    report.push("swf_video_bitmap_frames",FlaCompatibilityCategory::MediaAndSound,FlaCompatibilityStatus::Baked,kf_loc.clone(),
                        "Embedded video is converted to timed bitmap frames. Its unmuted soundtrack uses StartSound at the start of the span; seeking the SWF timeline cannot reposition this sound.".to_owned());
                } else if kf.video.is_some() {
                    report.push(
                        "swf_video_unsupported",
                        FlaCompatibilityCategory::MediaAndSound,
                        FlaCompatibilityStatus::Blocking,
                        kf_loc.clone(),
                        "Timeline frame video streams are not supported by the SWF publisher."
                            .to_owned(),
                    );
                }
                for fill in &kf.bitmap_fills {
                    let supported_target = kf.elements.iter().any(|el| {
                        el.id == fill.element_id
                            && matches!(
                                el.kind,
                                ElementKind::Rectangle { .. }
                                    | ElementKind::Ellipse { .. }
                                    | ElementKind::Path { .. }
                                    | ElementKind::PaintedPath { .. }
                            )
                    });
                    // A MATRIX stores at most 31 signed bits. Bitmap scale/rotation use
                    // 16.16 fixed-point in twips per pixel, translation uses integer twips.
                    let in_range = fill.matrix.iter().enumerate().all(|(i, v)| {
                        let encoded = (v * if i < 4 { 20. * 65536. } else { 20. }).round();
                        (-1_073_741_824. ..=1_073_741_823.).contains(&encoded)
                    });
                    let supported = supported_target && in_range;
                    report.push(
                        if supported { "swf_bitmap_fill" } else { "swf_bitmap_fill_unrepresentable" },
                        FlaCompatibilityCategory::ShapesAndPaints,
                        if supported { FlaCompatibilityStatus::Exact } else { FlaCompatibilityStatus::Blocking },
                        format!("{kf_loc}:element:{}", fill.element_id),
                        if supported { "Shape uses a native transformed bitmap fill." } else { "Bitmap fill requires a vector shape and a transform within the SWF MATRIX range." },
                    );
                }
                if kf.camera.is_some() {
                    report.push(
                        "swf_camera_unsupported",
                        FlaCompatibilityCategory::AnimationAndRigging,
                        FlaCompatibilityStatus::Blocking,
                        kf_loc.clone(),
                        "Keyframe camera poses are not supported by SWF output.".to_owned(),
                    );
                }
                if !kf.ik_armatures.is_empty() {
                    report.push(
                        "swf_ik_unsupported",
                        FlaCompatibilityCategory::AnimationAndRigging,
                        FlaCompatibilityStatus::Blocking,
                        kf_loc.clone(),
                        "IK armatures are not solved by the current SWF exporter.".to_owned(),
                    );
                }
                if !kf.asset_warps.is_empty() {
                    report.push(
                        "swf_asset_warp_unsupported",
                        FlaCompatibilityCategory::AnimationAndRigging,
                        FlaCompatibilityStatus::Blocking,
                        kf_loc.clone(),
                        "Asset warp rigs are not baked by the current SWF exporter.".to_owned(),
                    );
                }
                if let Some(snd) = &kf.sound {
                    if snd.in_point.is_some_and(|t| !t.is_finite() || t < 0.0)
                        || snd.out_point.is_some_and(|t| !t.is_finite() || t < 0.0)
                    {
                        report.push(
                            "swf_sound_trim_invalid",
                            FlaCompatibilityCategory::MediaAndSound,
                            FlaCompatibilityStatus::Blocking,
                            kf_loc.clone(),
                            "Frame sound trim in/out points must be finite non-negative seconds."
                                .to_owned(),
                        );
                    }
                    if snd.sync == SoundSync::Stream {
                        let eff_end = layer
                            .properties
                            .end_frame
                            .unwrap_or(sym_total_frames)
                            .min(sym_total_frames);
                        let span_end = layer.keyframes[k_idx + 1..]
                            .iter()
                            .find(|nk| nk.sound.is_some())
                            .map_or(eff_end, |nk| nk.frame.min(eff_end));
                        let overlaps =
                            (kf.frame..span_end).any(|f| timeline_stream_occupied.contains(&f));
                        if overlaps {
                            report.push(
                                "swf_overlapping_stream_sounds_approximated",
                                FlaCompatibilityCategory::MediaAndSound,
                                FlaCompatibilityStatus::Approximated,
                                kf_loc.clone(),
                                "Overlapping timeline Stream sound on the same timeline falls back to StartSound (tag 15) because SWF provides one SoundStreamHead2 channel per timeline."
                                    .to_owned(),
                            );
                        } else {
                            for f in kf.frame..span_end {
                                timeline_stream_occupied.insert(f);
                            }
                            report.push(
                                "swf_stream_sound",
                                FlaCompatibilityCategory::MediaAndSound,
                                FlaCompatibilityStatus::Exact,
                                kf_loc.clone(),
                                "Timeline Stream sound encoded as SoundStreamHead2 and per-frame SoundStreamBlock tags."
                                    .to_owned(),
                            );
                        }
                    }
                }
                if let Some(easing) = &kf.tween {
                    match kf.tween_kind {
                        TweenKind::Motion => {
                            report.push(
                                "swf_motion_tween_baked",
                                FlaCompatibilityCategory::AnimationAndRigging,
                                if options.bake_unsupported_tweens {
                                    FlaCompatibilityStatus::Baked
                                } else {
                                    FlaCompatibilityStatus::Approximated
                                },
                                kf_loc.clone(),
                                "Motion tween span evaluated into frame-by-frame SWF display-list placements."
                                    .to_owned(),
                            );
                        }
                        TweenKind::Shape => {
                            let can_morph = layer
                                .keyframes
                                .get(k_idx + 1)
                                .is_some_and(|next| can_morph_keys(kf, next));
                            report.push(
                                if can_morph {
                                    "swf_define_morph_shape"
                                } else {
                                    "swf_shape_tween_baked"
                                },
                                FlaCompatibilityCategory::AnimationAndRigging,
                                if can_morph {
                                    FlaCompatibilityStatus::Exact
                                } else if options.bake_unsupported_tweens {
                                    FlaCompatibilityStatus::Baked
                                } else {
                                    FlaCompatibilityStatus::Approximated
                                },
                                kf_loc.clone(),
                                if can_morph {
                                    "Shape tween compiled into DefineMorphShape / DefineMorphShape2 (tags 46/84) with per-frame ratio."
                                        .to_owned()
                                } else {
                                    "Shape tween sampled into separate shapes for each frame to preserve its geometry, paint and timing."
                                        .to_owned()
                                },
                            );
                        }
                        TweenKind::Classic => {
                            let status = if matches!(easing, Easing::CubicBezier { .. })
                                && !options.bake_unsupported_tweens
                            {
                                FlaCompatibilityStatus::Approximated
                            } else {
                                FlaCompatibilityStatus::Exact
                            };
                            report.push(
                                "swf_classic_tween",
                                FlaCompatibilityCategory::AnimationAndRigging,
                                status,
                                kf_loc.clone(),
                                "Classic tween compiled into per-frame PlaceObject2 matrix/cxform updates."
                                    .to_owned(),
                            );
                        }
                    }
                }

                for el in &kf.elements {
                    let el_loc = format!("{kf_loc}:element:{}", el.id);
                    let eff_kind = el.effective_symbol_kind(project);
                    let valid_object_script_host =
                        matches!(eff_kind, Some(SymbolKind::MovieClip | SymbolKind::Button));
                    let is_button_host = eff_kind == Some(SymbolKind::Button);

                    for os in &el.scripts {
                        match os.script.language {
                            ScriptLanguage::JavaScript | ScriptLanguage::TypeScript => {
                                report.push(
                                    "swf_js_ts_object_script",
                                    FlaCompatibilityCategory::Scripts,
                                    if options.scriptless_mode {
                                        FlaCompatibilityStatus::Omitted
                                    } else {
                                        FlaCompatibilityStatus::Blocking
                                    },
                                    format!("{el_loc}:script"),
                                    "JS/TS object script cannot be compiled into SWF.".to_owned(),
                                );
                            }
                            ScriptLanguage::ActionScript3 => {
                                has_avm2_scripts = true;
                                report.push(
                                    "swf_as3_object_script_unsupported",
                                    FlaCompatibilityCategory::Scripts,
                                    if options.scriptless_mode {
                                        FlaCompatibilityStatus::Omitted
                                    } else {
                                        FlaCompatibilityStatus::Blocking
                                    },
                                    format!("{el_loc}:script"),
                                    "AS3 does not allow inline object scripts on symbol/button instances."
                                        .to_owned(),
                                );
                            }
                            ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2 => {
                                has_avm1_scripts = true;
                                if !options.scriptless_mode && !valid_object_script_host {
                                    report.push(
                                        "swf_object_script_invalid_host",
                                        FlaCompatibilityCategory::Scripts,
                                        FlaCompatibilityStatus::Blocking,
                                        format!("{el_loc}:script"),
                                        format!(
                                            "AVM1 object script on '{}' requires a MovieClip or Button instance.",
                                            el.name
                                        ),
                                    );
                                    continue;
                                }
                                if !options.scriptless_mode && is_button_host {
                                    if os.event != ObjectEvent::Click {
                                        report.push(
                                            "swf_button_event_unsupported",
                                            FlaCompatibilityCategory::Scripts,
                                            FlaCompatibilityStatus::Blocking,
                                            format!("{el_loc}:script"),
                                            format!(
                                                "Button instance '{}' does not support {:?} object events; only Click / 'on(...)' button handlers are valid in DefineButton2.",
                                                el.name, os.event
                                            ),
                                        );
                                        continue;
                                    }
                                    if !options.profile.is_avm2()
                                        && let Err(issue) =
                                            validate_button_script_syntax(project, os)
                                    {
                                        report.push(
                                            "swf_button_event_unsupported",
                                            FlaCompatibilityCategory::Scripts,
                                            FlaCompatibilityStatus::Blocking,
                                            format!("{el_loc}:script"),
                                            issue,
                                        );
                                        continue;
                                    }
                                }
                                let compile_issue = swf_script_compilation_issue(
                                    project,
                                    os.script.language,
                                    "",
                                    &os.script.source,
                                    false,
                                    true,
                                );
                                report.push(
                                    if !options.profile.is_avm2() && compile_issue.is_none() {
                                        "swf_avm1_object_script"
                                    } else if !options.profile.is_avm2() {
                                        "swf_avm1_object_script_unsupported_syntax"
                                    } else {
                                        "swf_avm1_object_script_in_avm2"
                                    },
                                    FlaCompatibilityCategory::Scripts,
                                    if options.scriptless_mode {
                                        FlaCompatibilityStatus::Omitted
                                    } else if !options.profile.is_avm2()
                                        && compile_issue.is_none()
                                    {
                                        FlaCompatibilityStatus::Exact
                                    } else {
                                        FlaCompatibilityStatus::Blocking
                                    },
                                    format!("{el_loc}:script"),
                                    compile_issue.map_or_else(
                                        || {
                                            "Object script compiled into AVM1 CLIPACTIONS / BUTTONCONDACTION."
                                                .to_owned()
                                        },
                                        |issue| {
                                            format!(
                                                "AS1/AS2 object script cannot be compiled losslessly: {issue}."
                                            )
                                        },
                                    ),
                                );
                            }
                        }
                    }
                    if !options.profile.is_avm2()
                        && !options.scriptless_mode
                        && is_button_host
                        && let Err(issue) = encode_button2_cond_actions(project, &el.scripts)
                    {
                        report.push(
                            "swf_avm1_button_action_capacity",
                            FlaCompatibilityCategory::Scripts,
                            FlaCompatibilityStatus::Blocking,
                            format!("{el_loc}:script"),
                            format!(
                                "Button actions exceed an SWF limit or invalid event: {issue}."
                            ),
                        );
                    }
                    if !options.profile.is_avm2()
                        && !options.scriptless_mode
                        && !is_button_host
                        && !el.scripts.is_empty()
                        && let Err(issue) = encode_clip_actions(project, &el.scripts)
                    {
                        report.push(
                            "swf_avm1_clip_action_invalid",
                            FlaCompatibilityCategory::Scripts,
                            FlaCompatibilityStatus::Blocking,
                            format!("{el_loc}:script"),
                            format!(
                                "MovieClip clip actions use an invalid event or exceed SWF limits: {issue}."
                            ),
                        );
                    }

                    let active_filters: Vec<&FilterSpec> =
                        el.filters.iter().filter(|f| f.is_enabled()).collect();
                    if active_filters.len() > 16 {
                        report.push(
                            "swf_filter_count_limit",
                            FlaCompatibilityCategory::FiltersAndBlends,
                            FlaCompatibilityStatus::Blocking,
                            el_loc.clone(),
                            format!(
                                "Element '{}' has {} enabled filters; SWF PlaceObject3 supports at most 16 filters.",
                                el.name,
                                active_filters.len()
                            ),
                        );
                    }
                    for filter in active_filters {
                        let grad_stops = match filter {
                            FilterSpec::GradientGlow {
                                colors,
                                alphas,
                                ratios,
                                ..
                            }
                            | FilterSpec::GradientBevel {
                                colors,
                                alphas,
                                ratios,
                                ..
                            } => Some(colors.len().min(alphas.len()).min(ratios.len())),
                            _ => None,
                        };
                        if let Some(n) = grad_stops
                            && !(1..=16).contains(&n)
                        {
                            report.push(
                                "swf_filter_gradient_stop_limit",
                                FlaCompatibilityCategory::FiltersAndBlends,
                                FlaCompatibilityStatus::Blocking,
                                el_loc.clone(),
                                format!(
                                    "Gradient filter on '{}' has {n} stops; SWF requires 1..=16.",
                                    el.name
                                ),
                            );
                        }
                    }

                    match &el.kind {
                        ElementKind::Video { .. } if !bake_video => {
                            report.push(
                                "swf_video_unsupported",
                                FlaCompatibilityCategory::MediaAndSound,
                                FlaCompatibilityStatus::Blocking,
                                el_loc,
                                format!(
                                    "Video element '{}' has no bundled FLV/H.264 stream and cannot be published to SWF.",
                                    el.name
                                ),
                            );
                        }
                        ElementKind::Instance { symbol_id, .. }
                            if project.symbol(symbol_id).is_none() =>
                        {
                            report.push(
                                "swf_missing_symbol_ref",
                                FlaCompatibilityCategory::SymbolsAndLibrary,
                                FlaCompatibilityStatus::Blocking,
                                el_loc,
                                format!(
                                    "Element '{}' references missing symbol '{symbol_id}'.",
                                    el.name
                                ),
                            );
                        }
                        ElementKind::PaintedPath {
                            fill_gradient,
                            stroke_gradient,
                            ..
                        } => {
                            for grad in [fill_gradient.as_ref(), stroke_gradient.as_ref()]
                                .into_iter()
                                .flatten()
                            {
                                if grad.stops.is_empty() || grad.stops.len() > 15 {
                                    report.push(
                                        "swf_gradient_stop_count_limit",
                                        FlaCompatibilityCategory::ShapesAndPaints,
                                        FlaCompatibilityStatus::Blocking,
                                        el_loc.clone(),
                                        format!(
                                            "Shape gradient has {} stops, outside SWF supported range (1..=15).",
                                            grad.stops.len()
                                        ),
                                    );
                                }
                            }
                            if !el.stroke_width_profile().is_default() {
                                report.push(
                                    "swf_variable_width_stroke",
                                    FlaCompatibilityCategory::ShapesAndPaints,
                                    if options.expand_variable_width_strokes {
                                        FlaCompatibilityStatus::Baked
                                    } else {
                                        FlaCompatibilityStatus::Approximated
                                    },
                                    el_loc.clone(),
                                    "Variable-width stroke expanded into closed filled contour for SWF."
                                        .to_owned(),
                                );
                            }
                        }
                        ElementKind::Path { .. } => {
                            if !el.stroke_width_profile().is_default() {
                                report.push(
                                    "swf_variable_width_stroke",
                                    FlaCompatibilityCategory::ShapesAndPaints,
                                    if options.expand_variable_width_strokes {
                                        FlaCompatibilityStatus::Baked
                                    } else {
                                        FlaCompatibilityStatus::Approximated
                                    },
                                    el_loc.clone(),
                                    "Variable-width stroke expanded into closed filled contour for SWF."
                                        .to_owned(),
                                );
                            }
                        }
                        ElementKind::TextBox { .. } | ElementKind::Text { .. } => {
                            if !options.outline_text {
                                if let Err(issue) =
                                    check_element_font3_support(project, el, &mut font_probe)
                                {
                                    report.push(
                                        "swf_text_requires_outlines",
                                        FlaCompatibilityCategory::TextAndFonts,
                                        FlaCompatibilityStatus::Blocking,
                                        el_loc,
                                        format!(
                                            "Text cannot be embedded as DefineFont3: {issue}. Enable outline_text for supplementary-plane or unmapped glyphs."
                                        ),
                                    );
                                } else if let ElementKind::TextBox { format, .. } = &el.kind
                                    && let Some(fopts) = format.field_options()
                                    && (fopts.html || !fopts.format_runs.is_empty())
                                {
                                    report.push(
                                        "swf_textfield_rich_text_approximated",
                                        FlaCompatibilityCategory::TextAndFonts,
                                        FlaCompatibilityStatus::Approximated,
                                        el_loc,
                                        "DefineEditText embeds DefineFont3 vector glyph outlines and field metrics; HTML or per-run rich-text formatting is flattened."
                                            .to_owned(),
                                    );
                                } else {
                                    report.push(
                                        "swf_font3_embedded",
                                        FlaCompatibilityCategory::TextAndFonts,
                                        FlaCompatibilityStatus::Exact,
                                        el_loc,
                                        "Text published with embedded DefineFont3 vector glyph outlines, CodeTable, and FontAdvanceTable."
                                            .to_owned(),
                                    );
                                }
                            } else if let Some(issue) = swf_text_outline_issue(el) {
                                report.push(
                                    "swf_text_outline_unsupported",
                                    FlaCompatibilityCategory::TextAndFonts,
                                    FlaCompatibilityStatus::Blocking,
                                    el_loc,
                                    format!(
                                        "Text cannot be converted faithfully to static glyph outlines: {issue}. Simplify the field or keep it out of this SWF publish."
                                    ),
                                );
                            } else {
                                report.push(
                                    "swf_text_outlined",
                                    FlaCompatibilityCategory::TextAndFonts,
                                    FlaCompatibilityStatus::Outlined,
                                    el_loc,
                                    "Text is intentionally converted to static vector glyph contours; editing, selection, binding and input behavior are not retained."
                                        .to_owned(),
                                );
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    if has_avm1_scripts && has_avm2_scripts && !options.scriptless_mode {
        report.push(
            "swf_mixed_avm1_avm2_scripts",
            FlaCompatibilityCategory::Scripts,
            FlaCompatibilityStatus::Blocking,
            "project",
            "Project mixes AVM1 (AS1/AS2) and AVM2 (AS3) scripts; enable scriptless_mode or unify script language before SWF publish."
                .to_owned(),
        );
    }

    report
}
