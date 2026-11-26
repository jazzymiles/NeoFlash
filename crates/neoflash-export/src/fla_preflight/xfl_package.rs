use super::*;

pub(super) struct XflPackage {
    pub(super) text_files: BTreeMap<String, String>,
    pub(super) binary_files: BTreeMap<String, Vec<u8>>,
}

pub(super) fn validate_xfl_package_paths(pkg: &XflPackage) -> Result<(), ExportError> {
    let mut seen_lower: BTreeSet<String> = BTreeSet::new();
    for path in pkg.text_files.keys().chain(pkg.binary_files.keys()) {
        if path.is_empty()
            || path.starts_with('/')
            || path.contains('\\')
            || path
                .split('/')
                .any(|seg| seg.is_empty() || seg == "." || seg == "..")
        {
            return Err(ExportError::InvalidProject(format!(
                "Invalid XFL archive entry path {path:?}."
            )));
        }
        let lower = path.to_ascii_lowercase();
        if !seen_lower.insert(lower) {
            return Err(ExportError::InvalidProject(format!(
                "Case-insensitive XFL archive path collision for {path:?}."
            )));
        }
    }
    Ok(())
}

pub(super) fn write_uncompressed_xfl_dir(
    project: &Project,
    profile: FlaTargetProfile,
    stem: &str,
    dir: &Path,
) -> Result<(), ExportError> {
    let pkg = build_xfl_package(project, profile, stem)?;
    validate_xfl_package_paths(&pkg)?;
    fs::create_dir_all(dir)?;
    for (rel, content) in &pkg.text_files {
        let full = dir.join(rel);
        if let Some(p) = full.parent() {
            fs::create_dir_all(p)?;
        }
        fs::write(full, content.as_bytes())?;
    }
    for (rel, bytes) in &pkg.binary_files {
        let full = dir.join(rel);
        if let Some(p) = full.parent() {
            fs::create_dir_all(p)?;
        }
        fs::write(full, bytes)?;
    }
    Ok(())
}

pub(super) fn write_zipped_xfl_bytes(
    project: &Project,
    profile: FlaTargetProfile,
    stem: &str,
) -> Result<Vec<u8>, ExportError> {
    let pkg = build_xfl_package(project, profile, stem)?;
    validate_xfl_package_paths(&pkg)?;
    let cursor = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(cursor);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    for (rel, content) in &pkg.text_files {
        zip.start_file(rel, opts)
            .map_err(|e| ExportError::Io(std::io::Error::other(e.to_string())))?;
        zip.write_all(content.as_bytes())?;
    }
    for (rel, bytes) in &pkg.binary_files {
        zip.start_file(rel, opts)
            .map_err(|e| ExportError::Io(std::io::Error::other(e.to_string())))?;
        zip.write_all(bytes)?;
    }
    let finished = zip
        .finish()
        .map_err(|e| ExportError::Io(std::io::Error::other(e.to_string())))?;
    Ok(finished.into_inner())
}

pub(super) fn detect_project_script_language(
    project: &Project,
    profile: FlaTargetProfile,
) -> ScriptLanguage {
    let allow_as3 = profile.supports_as3();
    if let Some(proj_lang) = project.script_language {
        match proj_lang {
            ScriptLanguage::ActionScript3 if allow_as3 => return ScriptLanguage::ActionScript3,
            ScriptLanguage::ActionScript2 => return ScriptLanguage::ActionScript2,
            ScriptLanguage::ActionScript1 => return ScriptLanguage::ActionScript1,
            _ => {}
        }
    }
    let mut found_avm1: Option<ScriptLanguage> = None;

    if allow_as3
        && (project
            .document_class
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
            || project.symbols.iter().any(|s| {
                s.class_name
                    .as_deref()
                    .is_some_and(|c| !c.trim().is_empty())
            }))
    {
        return ScriptLanguage::ActionScript3;
    }

    for src in &project.sources {
        match src.language {
            ScriptLanguage::ActionScript3 if allow_as3 => {
                return ScriptLanguage::ActionScript3;
            }
            ScriptLanguage::ActionScript2 | ScriptLanguage::ActionScript3 => {
                found_avm1 = Some(ScriptLanguage::ActionScript2);
            }
            ScriptLanguage::ActionScript1 => {
                found_avm1.get_or_insert(ScriptLanguage::ActionScript1);
            }
            _ => {}
        }
    }

    for sym in &project.symbols {
        for fs in &sym.timeline.scripts {
            match fs.script.language {
                ScriptLanguage::ActionScript3 if allow_as3 => {
                    return ScriptLanguage::ActionScript3;
                }
                ScriptLanguage::ActionScript2 | ScriptLanguage::ActionScript3 => {
                    found_avm1 = Some(ScriptLanguage::ActionScript2);
                }
                ScriptLanguage::ActionScript1 => {
                    found_avm1.get_or_insert(ScriptLanguage::ActionScript1);
                }
                _ => {}
            }
        }
        for layer in &sym.timeline.layers {
            for kf in &layer.keyframes {
                for el in &kf.elements {
                    for os in &el.scripts {
                        match os.script.language {
                            ScriptLanguage::ActionScript3 if allow_as3 => {
                                return ScriptLanguage::ActionScript3;
                            }
                            ScriptLanguage::ActionScript2 | ScriptLanguage::ActionScript3 => {
                                found_avm1 = Some(ScriptLanguage::ActionScript2);
                            }
                            ScriptLanguage::ActionScript1 => {
                                found_avm1.get_or_insert(ScriptLanguage::ActionScript1);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    if let Some(avm1) = found_avm1 {
        return avm1;
    }
    if allow_as3 {
        ScriptLanguage::ActionScript3
    } else {
        ScriptLanguage::ActionScript2
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct VideoKey(Option<Id>, u32, u32);

impl VideoKey {
    pub(super) fn from_dims(resource: Option<Id>, width: f64, height: f64) -> Self {
        let w = if width.is_finite() {
            width.round().clamp(1.0, 8192.0) as u32
        } else {
            320
        };
        let h = if height.is_finite() {
            height.round().clamp(1.0, 8192.0) as u32
        } else {
            240
        };
        Self(resource, w, h)
    }
}

pub(super) fn explicit_fla_media_linkage_id(res: &ResourceUnit) -> Option<&str> {
    let trimmed = res.id.trim();
    if trimmed.is_empty()
        || trimmed == "root"
        || trimmed.len() > 512
        || trimmed.chars().any(char::is_control)
    {
        return None;
    }
    for prefix in [
        "resource-",
        "legacy-sound-",
        "xfl-sound-",
        "swf-sound-",
        "legacy-bitmap-",
        "xfl-bitmap-",
        "swf-bitmap-",
        "legacy-cc-",
        "fla-",
        "res-",
        "asset-",
        "audio-",
        "bitmap-",
        "image-",
        "img-",
        "bmp-",
        "baked-",
        "video-audio-",
    ] {
        if trimmed.starts_with(prefix) {
            return None;
        }
    }
    for prefix in ["snd-", "sound-", "s", "b"] {
        if let Some(rest) = trimmed.strip_prefix(prefix)
            && !rest.is_empty()
            && rest.bytes().all(|b| b.is_ascii_digit())
        {
            return None;
        }
    }
    Some(trimmed)
}

#[allow(dead_code)]
pub(super) fn explicit_fla_sound_linkage_id(res: &ResourceUnit) -> Option<&str> {
    explicit_fla_media_linkage_id(res)
}

fn project_has_authored_scripts(project: &Project) -> bool {
    !project.sources.is_empty()
        || project
            .document_class
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
        || project.resources.iter().any(is_actionscript_resource)
        || project.symbols.iter().any(|sym| {
            sym.class_name
                .as_deref()
                .is_some_and(|s| !s.trim().is_empty())
                || sym
                    .timeline
                    .scripts
                    .iter()
                    .any(|fs| !fs.script.source.trim().is_empty())
                || sym.timeline.layers.iter().any(|layer| {
                    layer.keyframes.iter().any(|kf| {
                        kf.elements.iter().any(|el| {
                            el.scripts
                                .iter()
                                .any(|os| !os.script.source.trim().is_empty())
                        })
                    })
                })
        })
}

fn derive_fla_media_linkage_id(
    res: &ResourceUnit,
    is_audio: bool,
    has_scripts: bool,
    fallback_idx: usize,
    used_linkage_ids: &mut BTreeSet<String>,
) -> Option<String> {
    let raw_candidate = if let Some(explicit) = explicit_fla_media_linkage_id(res) {
        explicit.to_owned()
    } else if has_scripts {
        let normalized = res
            .path
            .trim()
            .replace('\\', "/")
            .trim_start_matches("./")
            .trim_start_matches('/')
            .to_owned();
        let basename = normalized.rsplit('/').next().unwrap_or(&normalized);
        let stem = if is_audio {
            basename
                .split_once('.')
                .filter(|(s, e)| !s.is_empty() && !e.is_empty())
                .map_or(basename, |(s, _)| s)
        } else {
            basename
                .rsplit_once('.')
                .filter(|(s, e)| !s.is_empty() && !e.is_empty())
                .map_or(basename, |(s, _)| s)
        };
        let sanitized: String = stem
            .chars()
            .filter(|c| !c.is_control())
            .collect::<String>()
            .trim()
            .chars()
            .take(512)
            .collect();
        if sanitized.is_empty() || sanitized == "root" {
            format!(
                "{}_{}",
                if is_audio { "sound" } else { "bitmap" },
                fallback_idx + 1
            )
        } else {
            sanitized
        }
    } else {
        return None;
    };

    let mut candidate = raw_candidate.clone();
    let mut suffix = 2usize;
    while !used_linkage_ids.insert(candidate.clone()) {
        candidate = format!("{raw_candidate}_{suffix}");
        suffix += 1;
    }
    Some(candidate)
}

fn format_fla_media_linkage_attrs(
    linkage_id: Option<&str>,
    as_version_num: &str,
    is_bitmap: bool,
) -> String {
    match linkage_id {
        Some(lid) if as_version_num == "3" && is_valid_as3_qname(lid) => {
            let base_attr = if is_bitmap {
                " linkageBaseClass=\"flash.display.BitmapData\""
            } else {
                ""
            };
            format!(
                " linkageExportForAS=\"true\" linkageExportInFirstFrame=\"true\" linkageIdentifier=\"{}\" linkageClassName=\"{}\"{base_attr}",
                xml_escape(lid),
                xml_escape(lid)
            )
        }
        Some(lid) => format!(
            " linkageExportForAS=\"true\" linkageExportInFirstFrame=\"true\" linkageIdentifier=\"{}\"",
            xml_escape(lid)
        ),
        None => String::new(),
    }
}

pub(super) fn build_xfl_package(
    project: &Project,
    profile: FlaTargetProfile,
    stem: &str,
) -> Result<XflPackage, ExportError> {
    let mut text_files = BTreeMap::new();
    let mut binary_files = BTreeMap::new();

    let clean_stem: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let marker_stem = if clean_stem.is_empty() {
        "Document"
    } else {
        &clean_stem
    };
    text_files.insert(format!("{marker_stem}.xfl"), "PROXY-CS5\n".to_owned());

    // META-INF/metadata.xml
    let metadata_xml = format!(
        r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description rdf:about="" xmlns:xmp="http://ns.adobe.com/xap/1.0/">
      <xmp:CreatorTool>{}</xmp:CreatorTool>
    </rdf:Description>
  </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>"#,
        xml_escape(profile.creator_info())
    );
    text_files.insert("META-INF/metadata.xml".to_owned(), metadata_xml);

    // PublishSettings.xml
    let script_lang = detect_project_script_language(project, profile);
    let as_version_num = match script_lang {
        ScriptLanguage::ActionScript1 => "1",
        ScriptLanguage::ActionScript2 => "2",
        _ => "3",
    };
    let doc_class_xml = if as_version_num == "3"
        && let Some(dc) = project
            .document_class
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    {
        format!("\n      <DocumentClass>{}</DocumentClass>", xml_escape(dc))
    } else {
        String::new()
    };
    let publish_xml = format!(
        r#"<flash_profiles>
  <flash_profile version="1.0" name="Default" current="true">
    <PublishFlashProperties enabled="true">
      <ActionScriptVersion>{as_version_num}</ActionScriptVersion>
      <InvisibleLayer>1</InvisibleLayer>{doc_class_xml}
    </PublishFlashProperties>
  </flash_profile>
</flash_profiles>
"#
    );
    text_files.insert("PublishSettings.xml".to_owned(), publish_xml);

    // Bundled .as / .js / .ts source units
    let mut source_count = 0usize;
    let mut used_paths_lower: BTreeSet<String> = text_files
        .keys()
        .map(|k| k.to_ascii_lowercase())
        .chain(["mimetype".to_owned(), "domdocument.xml".to_owned()])
        .collect();
    for (idx, src) in project.sources.iter().enumerate() {
        let rel_path = sanitize_source_rel_path(&src.path, idx);
        let lower = rel_path.to_ascii_lowercase();
        if !used_paths_lower.insert(lower) {
            return Err(ExportError::InvalidProject(format!(
                "Source path {:?} collides with reserved or duplicate archive path {:?}.",
                src.path, rel_path
            )));
        }
        source_count += 1;
        let marker = match src.language {
            ScriptLanguage::JavaScript => "/* neoflash:language=javascript */\n",
            ScriptLanguage::TypeScript => "/* neoflash:language=typescript */\n",
            ScriptLanguage::ActionScript1 => "/* neoflash:language=actionscript1 */\n",
            ScriptLanguage::ActionScript2 => "/* neoflash:language=actionscript2 */\n",
            ScriptLanguage::ActionScript3 => "/* neoflash:language=actionscript3 */\n",
        };
        text_files.insert(rel_path, format!("{marker}{}", src.source));
    }

    // Also inspect smoothed bitmaps, custom sound envelopes, and video viewport items
    let embedded_video_ids: BTreeSet<Id> = project
        .resources
        .iter()
        .filter(|r| !is_internal_provenance_resource(r) && r.is_video())
        .map(|r| r.id.clone())
        .collect();
    let bitmap_smoothing_refs = bitmap_smoothing_usage(project);
    let mut sound_custom_env: BTreeSet<Id> = BTreeSet::new();
    let mut video_names: BTreeMap<VideoKey, String> = BTreeMap::new();
    let mut placed_video_ids: BTreeSet<Id> = BTreeSet::new();
    let mut used_video_item_names: BTreeSet<String> = BTreeSet::new();
    for sym in &project.symbols {
        for layer in &sym.timeline.layers {
            let mut active_vid_res: Option<Id> = None;
            for kf in &layer.keyframes {
                if let Some(snd) = &kf.sound
                    && (snd.effect == SoundEffect::Custom
                        || snd.envelope.as_ref().is_some_and(|v| !v.is_empty()))
                {
                    sound_custom_env.insert(snd.resource.clone());
                }
                if let Some(span) = &kf.video {
                    active_vid_res = embedded_video_ids
                        .contains(&span.resource)
                        .then(|| span.resource.clone());
                } else if !kf
                    .elements
                    .iter()
                    .any(|e| matches!(e.kind, ElementKind::Video { .. }))
                {
                    active_vid_res = None;
                }
                for el in &kf.elements {
                    if let ElementKind::Video { width, height } = &el.kind {
                        let eff_res = kf
                            .video
                            .as_ref()
                            .filter(|v| embedded_video_ids.contains(&v.resource))
                            .map(|v| v.resource.clone())
                            .or_else(|| active_vid_res.clone());
                        if let Some(rid) = &eff_res {
                            placed_video_ids.insert(rid.clone());
                        }
                        let key = VideoKey::from_dims(eff_res, *width, *height);
                        video_names.entry(key.clone()).or_insert_with(|| {
                            let base = format!("Video_{}x{}", key.1, key.2);
                            let mut cand = base.clone();
                            let mut n = 2usize;
                            while !used_video_item_names.insert(cand.to_ascii_lowercase()) {
                                cand = format!("{base}_{n}");
                                n += 1;
                            }
                            cand
                        });
                    }
                }
            }
        }
    }

    // Collect scene symbol IDs vs library symbols
    let scene_ids: Vec<Id> = if project.scenes.is_empty() {
        vec![project.root.clone()]
    } else {
        project.scenes.clone()
    };
    let scene_set: BTreeSet<&str> = scene_ids.iter().map(String::as_str).collect();

    // Assign unique library item names for non-scene symbols
    let mut symbol_lib_names: BTreeMap<Id, String> = BTreeMap::new();
    let mut used_lib_names_lower = BTreeSet::new();
    for sym in &project.symbols {
        if scene_set.contains(sym.id.as_str()) {
            continue;
        }
        let base_name = sanitize_xfl_item_name(&sym.name, "Symbol");
        let full_name = if sym.folder.trim().is_empty() {
            base_name
        } else {
            format!("{}/{}", sym.folder.trim_matches('/'), base_name)
        };
        let mut candidate = full_name.clone();
        let mut suffix = 2usize;
        while !used_lib_names_lower.insert(candidate.to_ascii_lowercase()) {
            candidate = format!("{full_name}_{suffix}");
            suffix += 1;
        }
        symbol_lib_names.insert(sym.id.clone(), candidate);
    }

    // Media resources (bitmaps & sounds) & bundled .as include resources
    let has_authored_scripts = project_has_authored_scripts(project);
    let mut used_media_linkage_ids: BTreeSet<String> = project
        .symbols
        .iter()
        .flat_map(|sym| {
            sym.linkage_id
                .iter()
                .chain(sym.class_name.iter())
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
        })
        .collect();
    let mut video_preview_marker = "";
    let mut bitmap_names: BTreeMap<Id, String> = BTreeMap::new();
    let mut sound_names: BTreeMap<Id, String> = BTreeMap::new();
    let mut sound_durations: BTreeMap<Id, u64> = BTreeMap::new();
    let mut video_dat_hrefs: BTreeMap<Id, String> = BTreeMap::new();
    let mut media_xml = String::new();
    for (idx, res) in project.resources.iter().enumerate() {
        if is_internal_provenance_resource(res) {
            continue;
        }
        let mime = res.media_type.trim().to_ascii_lowercase();
        if is_actionscript_resource(res) {
            let bytes = res.decoded_bytes().map_err(|err| {
                ExportError::InvalidProject(format!(
                    "ActionScript include resource {:?} failed base64 decoding: {err}",
                    res.path
                ))
            })?;
            let text = std::str::from_utf8(&bytes).map_err(|err| {
                ExportError::InvalidProject(format!(
                    "ActionScript include resource {:?} is not UTF-8: {err}",
                    res.path
                ))
            })?;
            let rel_path = sanitize_source_rel_path(&res.path, idx);
            let lower = rel_path.to_ascii_lowercase();
            if used_paths_lower.contains(&lower) {
                if text_files.get(&rel_path).map(|s| s.as_str()) == Some(text)
                    || project.sources.iter().enumerate().any(|(i, s)| {
                        sanitize_source_rel_path(&s.path, i).eq_ignore_ascii_case(&rel_path)
                            && s.source == text
                    })
                {
                    continue;
                }
                return Err(ExportError::InvalidProject(format!(
                    "ActionScript include resource {:?} collides with archive path {:?}.",
                    res.path, rel_path
                )));
            }
            used_paths_lower.insert(lower);
            source_count += 1;
            text_files.insert(rel_path, text.to_owned());
            continue;
        }
        if source_count > 1024 {
            return Err(ExportError::InvalidProject(
                "Bundled ActionScript source file count exceeds 1024-file XFL limit.".to_owned(),
            ));
        }

        let bytes = res.decoded_bytes().map_err(|err| {
            ExportError::InvalidProject(format!(
                "Media resource {:?} failed base64 decoding: {err}",
                res.path
            ))
        })?;
        if mime.starts_with("image/") {
            let (norm_bytes, ext, width, height) =
                normalize_fla_bitmap_bytes(&bytes).ok_or_else(|| {
                    ExportError::InvalidProject(format!(
                        "Image resource {:?} ({}) cannot be decoded as a supported bitmap.",
                        res.path, res.media_type
                    ))
                })?;
            let item_name = format!("bitmap_{}.{ext}", idx + 1);
            let allow_smoothing = match bitmap_smoothing_refs.get(res.id.as_str()).copied() {
                Some((true, _)) | None => true,
                Some((false, true)) => false,
                Some((false, false)) => true,
            };
            bitmap_names.insert(res.id.clone(), item_name.clone());
            let href = match crate::video_frames::xfl_editor_preview(res) {
                Some((w, h)) if w <= width && h <= height && (w, h) != (width, height) => {
                    video_preview_marker = "<?neoflash-video-frame-previews v1?>\n";
                    format!("media/neoflash-video-frames/{w}x{h}/{item_name}")
                }
                _ => item_name.clone(),
            };
            let bitmap_linkage = if href == item_name {
                derive_fla_media_linkage_id(
                    res,
                    false,
                    has_authored_scripts,
                    idx,
                    &mut used_media_linkage_ids,
                )
            } else {
                None
            };
            let linkage_attrs =
                format_fla_media_linkage_attrs(bitmap_linkage.as_deref(), as_version_num, true);
            binary_files.insert(format!("LIBRARY/{href}"), norm_bytes);
            media_xml.push_str(&format!(
                "    <DOMBitmapItem name=\"{}\" href=\"{}\" allowSmoothing=\"{allow_smoothing}\"{linkage_attrs}/>\n",
                xml_escape(&item_name),
                xml_escape(&href)
            ));
        } else if mime.starts_with("audio/") {
            let (ext, duration_44) = inspect_audio_codec(&bytes).ok_or_else(|| {
                ExportError::InvalidProject(format!(
                    "Audio resource {:?} ({}) is not a valid PCM WAV or MP3 stream.",
                    res.path, res.media_type
                ))
            })?;
            let item_name = format!("sound_{}.{ext}", idx + 1);
            sound_names.insert(res.id.clone(), item_name.clone());
            sound_durations.insert(res.id.clone(), duration_44);
            let sound_linkage = derive_fla_media_linkage_id(
                res,
                true,
                has_authored_scripts,
                idx,
                &mut used_media_linkage_ids,
            );
            let linkage_attrs =
                format_fla_media_linkage_attrs(sound_linkage.as_deref(), as_version_num, false);
            let cache = neoflash_import::build_flash_sound_cache(&bytes);
            binary_files.insert(format!("LIBRARY/{item_name}"), bytes);
            if let Some(cache) = cache {
                let dat_href = format!("sound{}.dat", idx + 1);
                binary_files.insert(format!("bin/{dat_href}"), cache.cache_bytes);
                let compressed_attrs = cache
                    .data_length
                    .map(|dl| format!(" dataLength=\"{dl}\" exportNative=\"true\""))
                    .unwrap_or_default();
                media_xml.push_str(&format!(
                    "    <DOMSoundItem name=\"{}\" href=\"{}\" soundDataHRef=\"{}\" format=\"{}\" sampleCount=\"{}\"{compressed_attrs}{linkage_attrs}/>\n",
                    xml_escape(&item_name),
                    xml_escape(&item_name),
                    xml_escape(&dat_href),
                    xml_escape(&cache.format_label),
                    cache.sample_count,
                ));
            } else {
                let sample_count_attr = if ext == "mp3" && sound_custom_env.contains(&res.id) {
                    format!(" sampleCount=\"{duration_44}\"")
                } else {
                    String::new()
                };
                media_xml.push_str(&format!(
                    "    <DOMSoundItem name=\"{}\" href=\"{}\" format=\"44kHz 16bit Stereo\"{sample_count_attr}{linkage_attrs}/>\n",
                    xml_escape(&item_name),
                    xml_escape(&item_name)
                ));
            }
        } else if mime.starts_with("video/") {
            if neoflash_core::inspect_video_resource(&res.path, &res.media_type, &bytes, None)
                .iter()
                .any(|d| d.is_error)
            {
                return Err(ExportError::InvalidProject(format!(
                    "Video resource {:?} ({}) is not a valid supported video container.",
                    res.path, res.media_type
                )));
            }
            let dat_href = format!("video{}.dat", idx + 1);
            binary_files.insert(format!("bin/{dat_href}"), bytes);
            video_dat_hrefs.insert(res.id.clone(), dat_href);
            if !placed_video_ids.contains(&res.id) {
                let key = VideoKey::from_dims(Some(res.id.clone()), 320.0, 240.0);
                video_names.entry(key).or_insert_with(|| {
                    let base = format!("video_{}", idx + 1);
                    let mut cand = base.clone();
                    let mut n = 2usize;
                    while !used_video_item_names.insert(cand.to_ascii_lowercase()) {
                        cand = format!("{base}_{n}");
                        n += 1;
                    }
                    cand
                });
            }
        } else if !crate::fonts::is_font_resource(res)
            && !is_unreferenced_sidecar_resource(project, res)
        {
            return Err(ExportError::InvalidProject(format!(
                "Unsupported resource {:?} ({}) cannot be represented in XFL.",
                res.path, res.media_type
            )));
        }
    }
    if source_count > 1024 {
        return Err(ExportError::InvalidProject(
            "Bundled ActionScript source file count exceeds 1024-file XFL limit.".to_owned(),
        ));
    }
    for (v_key, v_name) in &video_names {
        if let Some(rid) = &v_key.0
            && let Some(dat_href) = video_dat_hrefs.get(rid)
        {
            media_xml.push_str(&format!(
                "    <DOMVideoItem name=\"{}\" videoDataHRef=\"{}\" width=\"{}\" height=\"{}\"/>\n",
                xml_escape(v_name),
                xml_escape(dat_href),
                v_key.1,
                v_key.2
            ));
        } else {
            media_xml.push_str(&format!(
                "    <DOMVideoItem name=\"{}\" width=\"{}\" height=\"{}\"/>\n",
                xml_escape(v_name),
                v_key.1,
                v_key.2
            ));
        }
    }

    // Write LIBRARY/*.xml for each library symbol
    let mut includes_xml = String::new();
    for sym in &project.symbols {
        let Some(lib_name) = symbol_lib_names.get(&sym.id) else {
            continue;
        };
        let href = format!("{lib_name}.xml");
        includes_xml.push_str(&format!(
            "    <Include href=\"{}\" loadImmediate=\"false\"/>\n",
            xml_escape(&href)
        ));
        let sym_xml = write_dom_symbol_item_xml(
            sym,
            lib_name,
            project,
            &symbol_lib_names,
            &bitmap_names,
            &sound_names,
            &sound_durations,
            &video_names,
        );
        text_files.insert(format!("LIBRARY/{href}"), sym_xml);
    }

    // Folders XML
    let mut all_folders: BTreeSet<String> = BTreeSet::new();
    for f in &project.library_folders {
        let norm = f.trim().trim_matches('/');
        if !norm.is_empty() {
            all_folders.insert(norm.to_owned());
        }
    }
    for sym in &project.symbols {
        if !scene_set.contains(sym.id.as_str()) {
            let norm = sym.folder.trim().trim_matches('/');
            if !norm.is_empty() {
                all_folders.insert(norm.to_owned());
            }
        }
    }
    let folders_section = if all_folders.is_empty() {
        String::new()
    } else {
        let mut s = String::from("  <folders>\n");
        for folder in &all_folders {
            s.push_str(&format!(
                "    <DOMFolderItem name=\"{}\" isExpanded=\"true\"/>\n",
                xml_escape(folder)
            ));
        }
        s.push_str("  </folders>\n");
        s
    };

    let media_section = if media_xml.is_empty() {
        String::new()
    } else {
        format!("  <media>\n{media_xml}  </media>\n")
    };

    let symbols_section = if includes_xml.is_empty() {
        String::new()
    } else {
        format!("  <symbols>\n{includes_xml}  </symbols>\n")
    };

    // Scene timelines XML
    let mut timelines_xml = String::from("  <timelines>\n");
    for (scene_idx, scene_id) in scene_ids.iter().enumerate() {
        if let Some(scene_sym) = project.symbols.iter().find(|s| &s.id == scene_id) {
            let scene_name = if scene_sym.name.trim().is_empty() {
                format!("Scene {}", scene_idx + 1)
            } else {
                scene_sym.name.clone()
            };
            timelines_xml.push_str(&write_dom_timeline_xml(
                &scene_sym.timeline,
                &scene_name,
                project,
                &symbol_lib_names,
                &bitmap_names,
                &sound_names,
                &sound_durations,
                &video_names,
                "    ",
            ));
        }
    }
    timelines_xml.push_str("  </timelines>\n");

    let bg_hex = format!(
        "#{:02X}{:02X}{:02X}",
        project.background[0], project.background[1], project.background[2]
    );
    let doc_class_attr = project
        .document_class
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|dc| format!(" documentClass=\"{}\"", xml_escape(dc)))
        .unwrap_or_default();
    let dom_document_xml = format!(
        r#"{video_preview_marker}<DOMDocument xmlns="http://ns.adobe.com/xfl/2008/" width="{}" height="{}" frameRate="{}" backgroundColor="{}" xflVersion="{}" creatorInfo="{}"{}>
{}{}{}{}</DOMDocument>
"#,
        project.width.max(1),
        project.height.max(1),
        project.fps.max(1),
        bg_hex,
        profile.xfl_version(),
        xml_escape(profile.creator_info()),
        doc_class_attr,
        folders_section,
        media_section,
        symbols_section,
        timelines_xml
    );
    text_files.insert("DOMDocument.xml".to_owned(), dom_document_xml);

    Ok(XflPackage {
        text_files,
        binary_files,
    })
}
