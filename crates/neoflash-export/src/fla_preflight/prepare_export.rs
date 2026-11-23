use super::*;

/// Prepare a derived copy of `project` tailored to `profile` and `options`, returning
/// both the derived `Project` and its `FlaCompatibilityReport`. Never mutates `project`.
pub fn prepare_project_for_fla_profile(
    project: &Project,
    profile: FlaTargetProfile,
    options: &FlaExportOptions,
) -> (Project, FlaCompatibilityReport) {
    let mut report = preflight_fla_export_with_options(project, profile, options);
    let mut derived = crate::strip_skipped_foreign_scripts(project).into_owned();

    if report.can_export()
        && options.allow_approximations
        && let Err(e) = crate::video_frames::prepare(&mut derived, Some(&mut report))
    {
        report.push(
            "video_frame_decode_failed",
            FlaFeatureCategory::MediaAndSound,
            FlaFeatureDisposition::Blocking,
            "Video",
            e.to_string(),
        );
    }
    if profile.is_legacy_binary() && options.allow_approximations {
        derived.resources.retain(|r| !r.is_video());
        for sym in &mut derived.symbols {
            for layer in &mut sym.timeline.layers {
                for kf in &mut layer.keyframes {
                    kf.video = None;
                }
            }
        }
    }

    // Flash 8's smaller stage is fitted after baking so the sampled timeline keeps its proportions.
    if profile != FlaTargetProfile::LegacyFlash8 {
        derived.width = derived.width.clamp(1, 8192);
        derived.height = derived.height.clamp(1, 8192);
    }
    derived.fps = derived.fps.clamp(1, 120);

    // 2. Strip unsupported scripts & resolve mixed-VM conflicts when requested
    if options.strip_unsupported_scripts {
        let allow_as3 = profile.supports_as3();
        let target_as3 = allow_as3
            && detect_project_script_language(&derived, profile) == ScriptLanguage::ActionScript3;
        let allow_script_lang = |lang: ScriptLanguage| -> bool {
            if !allow_as3 {
                matches!(
                    lang,
                    ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
                )
            } else if target_as3 {
                lang == ScriptLanguage::ActionScript3
            } else {
                matches!(
                    lang,
                    ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
                )
            }
        };
        derived.sources.retain(|s| allow_script_lang(s.language));
        if !allow_as3 {
            derived.document_class = None;
        }
        let symbol_kinds: BTreeMap<Id, SymbolKind> = derived
            .symbols
            .iter()
            .map(|s| (s.id.clone(), s.kind))
            .collect();
        for sym in &mut derived.symbols {
            if !allow_as3 && sym.linkage_id.is_none() && sym.class_name.is_some() {
                sym.linkage_id = sym.class_name.take();
            }
            sym.timeline
                .scripts
                .retain(|fs| allow_script_lang(fs.script.language));
            for layer in &mut sym.timeline.layers {
                for kf in &mut layer.keyframes {
                    for el in &mut kf.elements {
                        let valid_target_kind = match &el.kind {
                            ElementKind::Instance {
                                symbol_id,
                                playback,
                            } => {
                                let k = symbol_kinds.get(symbol_id).copied();
                                matches!(playback, InstancePlayback::Independent)
                                    && matches!(k, Some(SymbolKind::MovieClip | SymbolKind::Button))
                            }
                            _ => false,
                        };
                        let is_button_target = match &el.kind {
                            ElementKind::Instance { symbol_id, .. } => {
                                symbol_kinds.get(symbol_id).copied() == Some(SymbolKind::Button)
                            }
                            _ => false,
                        };
                        el.scripts.retain(|os| {
                            matches!(
                                os.script.language,
                                ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
                            ) && !target_as3
                                && valid_target_kind
                                && (!is_button_target
                                    || matches!(os.event, ObjectEvent::Click)
                                    || script_starts_with_event_wrapper(&os.script.source))
                        });
                    }
                }
            }
        }
    } else if !profile.supports_as3() {
        for sym in &mut derived.symbols {
            if sym.linkage_id.is_none() && sym.class_name.is_some() {
                sym.linkage_id = sym.class_name.take();
            }
        }
    }

    // 3. Apply font policy / text outlining on the derived project
    if options.outline_unsupported_text || options.font_policy == FontExportPolicy::Outlines {
        convert_project_text_to_outlines(&mut derived);
        outline_remaining_textboxes(&mut derived);
    } else if options.font_policy != FontExportPolicy::Device {
        let _ = apply_font_policy(&mut derived, options.font_policy);
    }

    // 4. Bake non-classic animation spans & modifiers when requested
    if options.bake_unsupported_tweens {
        bake_derived_timelines(&mut derived, profile);
    }

    // 5. Propagate layer filters/blends & expand parametric/variable-width strokes
    normalize_derived_elements(&mut derived, profile);

    if profile == FlaTargetProfile::LegacyFlash8
        && options.allow_approximations
        && let Some((width, height, scale)) = flash8_stage_fit(derived.width, derived.height)
    {
        derived.width = width;
        derived.height = height;
        for scene in &mut derived.symbols {
            if scene.id != derived.root && !derived.scenes.contains(&scene.id) {
                continue;
            }
            for guide in &mut scene.timeline.guides {
                guide.position *= scale;
            }
            for element in scene
                .timeline
                .layers
                .iter_mut()
                .flat_map(|layer| &mut layer.keyframes)
                .flat_map(|key| &mut key.elements)
            {
                element.transform.x *= scale;
                element.transform.y *= scale;
                element.transform.scale_x *= scale;
                element.transform.scale_y *= scale;
                // CFB flattens a scene's vector transforms into the edges and writes stroke widths in stage twips.
                match &mut element.kind {
                    ElementKind::Rectangle { stroke_width, .. }
                    | ElementKind::Ellipse { stroke_width, .. }
                    | ElementKind::Path { stroke_width, .. }
                    | ElementKind::PaintedPath { stroke_width, .. } => *stroke_width *= scale,
                    _ => {}
                }
            }
        }
    }

    (derived, report)
}

pub(super) fn outline_remaining_textboxes(project: &mut Project) {
    for sym in &mut project.symbols {
        for layer in &mut sym.timeline.layers {
            for kf in &mut layer.keyframes {
                for el in &mut kf.elements {
                    if matches!(el.kind, ElementKind::TextBox { .. })
                        && let Some(field) = neoflash_core::TextFieldState::from_kind(&el.kind)
                    {
                        let commands = field.to_outline_commands(None);
                        let rgb = field.format.color;
                        let fill = [
                            ((rgb >> 16) & 0xFF) as u8,
                            ((rgb >> 8) & 0xFF) as u8,
                            (rgb & 0xFF) as u8,
                            field.alpha,
                        ];
                        el.kind = ElementKind::Path {
                            commands,
                            fill_rule: neoflash_core::FillRule::NonZero,
                            fill,
                            stroke: [0, 0, 0, 0],
                            stroke_width: 0.0,
                        };
                    }
                }
            }
        }
    }
}

pub(super) fn layer_needs_baking(
    layer: &Layer,
    timeline: &Timeline,
    profile: FlaTargetProfile,
) -> bool {
    if layer.properties.z_depth != 0.0 || layer.properties.attach_to_camera {
        return true;
    }
    if timeline
        .layers
        .iter()
        .flat_map(|l| &l.keyframes)
        .any(|k| k.camera.is_some())
    {
        return true;
    }
    if profile.is_legacy_binary() && layer.properties.motion_guide.is_some() {
        return true;
    }
    for kf in &layer.keyframes {
        if kf.tween.is_some() && kf.tween_kind == TweenKind::Motion {
            return true;
        }
        if kf.tween.is_some() && kf.tween_kind == TweenKind::Shape && profile.is_legacy_binary() {
            return true;
        }
        if let Some(easing) = kf.tween
            && matches!(easing, Easing::Bounce | Easing::Elastic)
        {
            return true;
        }
        if !kf.ik_armatures.is_empty()
            || !kf.asset_warps.is_empty()
            || !kf.element_parents.is_empty()
            || !kf.appearances.is_empty()
            || !kf.vector_effects.is_empty()
            || !kf.property_expressions.is_empty()
        {
            return true;
        }
    }
    false
}

pub(super) fn bake_derived_timelines(project: &mut Project, profile: FlaTargetProfile) {
    for sym in &mut project.symbols {
        let timeline_snapshot = sym.timeline.clone();
        let frame_count = timeline_snapshot.frame_count.max(1);
        for (li, layer) in sym.timeline.layers.iter_mut().enumerate() {
            if matches!(layer.properties.role, LayerRole::Folder | LayerRole::Guide) {
                continue;
            }
            if !layer_needs_baking(layer, &timeline_snapshot, profile) {
                continue;
            }
            let orig_layer = &timeline_snapshot.layers[li];
            let max_frame = orig_layer
                .properties
                .end_frame
                .unwrap_or(frame_count)
                .min(frame_count);
            if max_frame <= 1 && orig_layer.keyframes.len() <= 1 {
                if let Some(kf) = layer.keyframes.first_mut() {
                    kf.elements = sample_layer_in(&timeline_snapshot, orig_layer, 0);
                    clear_baked_keyframe_fields(kf);
                }
                continue;
            }
            let mut baked_keyframes = Vec::with_capacity(max_frame as usize);
            for f in 0..max_frame {
                let sampled_elements = sample_layer_in(&timeline_snapshot, orig_layer, f);
                let orig_kf = orig_layer.keyframes.iter().find(|k| k.frame == f);
                let bitmap_fills = orig_layer
                    .key_at(f)
                    .map(|k| {
                        k.bitmap_fills
                            .iter()
                            .filter(|fill| {
                                sampled_elements.iter().any(|el| el.id == fill.element_id)
                            })
                            .cloned()
                            .collect()
                    })
                    .unwrap_or_default();
                let mut kf = Keyframe {
                    frame: f,
                    bitmap_fills,
                    elements: sampled_elements,
                    tween: None,
                    tween_kind: TweenKind::Classic,
                    label: orig_kf.map(|k| k.label.clone()).unwrap_or_default(),
                    sound: orig_kf.and_then(|k| k.sound.clone()),
                    ..Default::default()
                };
                clear_baked_keyframe_fields(&mut kf);
                baked_keyframes.push(kf);
            }
            layer.keyframes = baked_keyframes;
            if profile.is_legacy_binary() {
                layer.properties.motion_guide = None;
            }
            layer.properties.z_depth = 0.0;
            layer.properties.attach_to_camera = false;
        }
    }
}

pub(super) fn clear_baked_keyframe_fields(kf: &mut Keyframe) {
    kf.tween = None;
    kf.tween_kind = TweenKind::Classic;
    kf.keyed_properties = None;
    kf.motion_keys = None;
    kf.motion_tracks.clear();
    kf.motion_path = None;
    kf.property_eases = Vec::new();
    kf.roving = false;
    kf.orient_to_path = false;
    kf.ik_armatures.clear();
    kf.asset_warps.clear();
    kf.camera = None;
    kf.element_parents.clear();
    kf.appearances.clear();
    kf.vector_effects.clear();
    kf.property_expressions.clear();
}

pub(super) fn normalize_derived_elements(project: &mut Project, profile: FlaTargetProfile) {
    for sym in &mut project.symbols {
        for layer in &mut sym.timeline.layers {
            let layer_blend = layer.properties.blend_mode;
            let layer_filters = std::mem::take(&mut layer.properties.filters);
            layer.properties.blend_mode = BlendMode::Normal;
            if layer.properties.role == LayerRole::Mask {
                layer.properties.mask_mode = MaskMode::Geometry;
                layer.properties.mask_invert = false;
            }
            for kf in &mut layer.keyframes {
                let keyframe_filters = std::mem::take(&mut kf.filters);
                for el in &mut kf.elements {
                    if el.blend_mode.is_normal() && !layer_blend.is_normal() {
                        el.blend_mode = layer_blend;
                    }
                    el.filters.extend(keyframe_filters.iter().cloned());
                    el.filters.extend(layer_filters.iter().cloned());
                    // Expand variable-width stroke profiles or legacy parametric primitives
                    if (!el.stroke_width_profile().is_default()
                        || (profile.is_legacy_binary() && el.has_parametric_options()))
                        && let Some(expanded_kind) = el.parametric_path_kind()
                    {
                        el.kind = expanded_kind;
                        el.transform.primitive = None;
                    }
                }
            }
        }
    }
}

pub(super) fn remove_path_if_exists(path: &Path) -> std::io::Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

pub(super) fn replace_staged_path_recoverably_with(
    staged_path: &Path,
    target_path: &Path,
    backup_path: &Path,
    mut rename: impl FnMut(&Path, &Path) -> std::io::Result<()>,
) -> std::io::Result<()> {
    if fs::symlink_metadata(backup_path).is_ok() {
        let _ = remove_path_if_exists(staged_path);
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!(
                "refusing to replace '{}' because backup path '{}' already exists",
                target_path.display(),
                backup_path.display()
            ),
        ));
    }

    let target_exists = match fs::symlink_metadata(target_path) {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => {
            let _ = remove_path_if_exists(staged_path);
            return Err(error);
        }
    };
    if !target_exists {
        let result = rename(staged_path, target_path);
        if result.is_err() {
            let _ = remove_path_if_exists(staged_path);
        }
        return result;
    }

    if let Err(error) = rename(target_path, backup_path) {
        let _ = remove_path_if_exists(staged_path);
        return Err(error);
    }

    match rename(staged_path, target_path) {
        Ok(()) => {
            if let Err(error) = remove_path_if_exists(backup_path) {
                return Err(std::io::Error::new(
                    error.kind(),
                    format!(
                        "replaced '{}', but could not remove recoverable backup '{}': {error}",
                        target_path.display(),
                        backup_path.display()
                    ),
                ));
            }
            Ok(())
        }
        Err(install_error) => {
            let rollback_result = rename(backup_path, target_path);
            let cleanup_result = remove_path_if_exists(staged_path);
            if let Err(rollback_error) = rollback_result {
                return Err(std::io::Error::new(
                    rollback_error.kind(),
                    format!(
                        "could not install staged XFL at '{}': {install_error}; rollback also failed, but the previous document remains at '{}': {rollback_error}",
                        target_path.display(),
                        backup_path.display()
                    ),
                ));
            }
            if let Err(cleanup_error) = cleanup_result {
                return Err(std::io::Error::new(
                    cleanup_error.kind(),
                    format!(
                        "could not install staged XFL at '{}': {install_error}; the previous document was restored, but temporary '{}' could not be removed: {cleanup_error}",
                        target_path.display(),
                        staged_path.display()
                    ),
                ));
            }
            Err(install_error)
        }
    }
}

pub(super) fn replace_staged_path_recoverably(
    staged_path: &Path,
    target_path: &Path,
    backup_path: &Path,
) -> std::io::Result<()> {
    replace_staged_path_recoverably_with(staged_path, target_path, backup_path, |from, to| {
        fs::rename(from, to)
    })
}

/// Export `project` to `path` using `profile` and `options`, verifying the written artifact
/// via `neoflash_import::import_project` before atomically replacing `path` (`T-188`).
pub fn export_fla_with_profile(
    project: &Project,
    profile: FlaTargetProfile,
    options: &FlaExportOptions,
    path: impl AsRef<Path>,
) -> Result<FlaCompatibilityReport, ExportError> {
    let target_path = path.as_ref();
    let (derived, report) = prepare_project_for_fla_profile(project, profile, options);
    if report.has_blocking() {
        let first = report.blocking_items()[0];
        return Err(ExportError::UnsupportedScript(format!(
            "FLA export blocked for {} at {}: {}",
            profile.label(),
            first.location,
            first.message
        )));
    }

    let parent_dir = target_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent_dir)?;

    let stem = target_path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("Document");
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);

    if profile.is_directory() {
        let temp_dir = parent_dir.join(format!(".{stem}.xfl-tmp-{pid}-{nanos}"));
        let backup_path = parent_dir.join(format!(".{stem}.xfl-backup-{pid}-{nanos}"));
        remove_path_if_exists(&temp_dir)?;
        remove_path_if_exists(&backup_path)?;
        let _temp_guard = crate::TemporaryPathGuard::new(temp_dir.clone());
        write_uncompressed_xfl_dir(&derived, profile, stem, &temp_dir)?;
        verify_reimported_fla(&temp_dir, profile)?;
        replace_staged_path_recoverably(&temp_dir, target_path, &backup_path)?;
        return Ok(report);
    }

    let ext = target_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("fla");
    let temp_file = parent_dir.join(format!(".{stem}.tmp-{pid}-{nanos}.{ext}"));
    let bytes = if profile.is_legacy_binary() {
        crate::legacy_fla::write_legacy_binary_fla_from_prepared(&derived, profile)?
    } else {
        write_zipped_xfl_bytes(&derived, profile, stem)?
    };
    let _temp_guard = crate::TemporaryPathGuard::new(temp_file.clone());
    fs::write(&temp_file, &bytes)?;
    verify_reimported_fla(&temp_file, profile)?;
    fs::rename(&temp_file, target_path)?;
    Ok(report)
}

/// Export `project` to in-memory bytes using `profile` and `options` (for `.fla` archives and
/// legacy binary `.fla` streams), verifying the bytes through `neoflash_import::import_fla_bytes`.
pub fn export_fla_bytes_with_profile(
    project: &Project,
    profile: FlaTargetProfile,
    options: &FlaExportOptions,
) -> Result<(Vec<u8>, FlaCompatibilityReport), ExportError> {
    let (derived, report) = prepare_project_for_fla_profile(project, profile, options);
    if report.has_blocking() {
        let first = report.blocking_items()[0];
        return Err(ExportError::UnsupportedScript(format!(
            "FLA export blocked for {} at {}: {}",
            profile.label(),
            first.location,
            first.message
        )));
    }
    let bytes = if profile.is_legacy_binary() {
        crate::legacy_fla::write_legacy_binary_fla_from_prepared(&derived, profile)?
    } else {
        write_zipped_xfl_bytes(&derived, profile, &derived.name)?
    };
    let _reimported = crate::legacy_fla::import_fla_bytes(&bytes)?;
    Ok((bytes, report))
}

pub(super) fn verify_reimported_fla(
    path: &Path,
    profile: FlaTargetProfile,
) -> Result<(), ExportError> {
    neoflash_import::import_project(path).map_err(|e| {
        ExportError::InvalidProject(format!(
            "Generated {} at {} failed round-trip import verification: {e}",
            profile.label(),
            path.display()
        ))
    })?;
    Ok(())
}
