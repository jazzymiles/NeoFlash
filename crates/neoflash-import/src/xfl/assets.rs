use super::*;

impl Parser<'_> {
    pub(super) fn u64_attr(
        &mut self,
        node: Node<'_, '_>,
        attr: &str,
        default: u64,
        file: &str,
    ) -> u64 {
        let Some(raw) = node.attribute(attr) else {
            return default;
        };
        match raw.parse::<u64>() {
            Ok(v) => v,
            Err(_) => {
                self.report.error(
                    "invalid_number",
                    self.location(node, file),
                    format!("{attr} must be an unsigned integer."),
                );
                default
            }
        }
    }

    pub(super) fn u32_attr(
        &mut self,
        node: Node<'_, '_>,
        attr: &str,
        default: u32,
        file: &str,
    ) -> u32 {
        let Some(raw) = node.attribute(attr) else {
            return default;
        };
        match raw.parse::<u32>() {
            Ok(v) => v,
            Err(_) => {
                self.report.error(
                    "invalid_number",
                    self.location(node, file),
                    format!("{attr} must be a 32-bit unsigned integer."),
                );
                default
            }
        }
    }

    pub(super) fn parse_media(
        &mut self,
        media: Node<'_, '_>,
        binary_files: &BTreeMap<String, Vec<u8>>,
        video_previews: bool,
    ) {
        self.audit(
            media,
            "DOMDocument.xml",
            &[],
            &[
                "DOMBitmapItem",
                "DOMSoundItem",
                "DOMVideoItem",
                "DOMCompiledClipItem",
            ],
        );
        for item in elements(media) {
            match item.tag_name().name() {
                "DOMBitmapItem" => self.parse_bitmap_item(item, binary_files, video_previews),
                "DOMSoundItem" => self.parse_sound_item(item, binary_files),
                "DOMVideoItem" => self.parse_video_item(item),
                "DOMCompiledClipItem" => self.parse_compiled_clip_item(item),
                _ => {
                    self.report.error(
                        "unsupported_assets",
                        self.location(item, "DOMDocument.xml"),
                        "Unsupported media library asset is not imported yet.",
                    );
                }
            }
        }
    }

    pub(super) fn parse_video_item(&mut self, item: Node<'_, '_>) {
        let loc = self.location(item, "DOMDocument.xml");
        self.audit(
            item,
            "DOMDocument.xml",
            &[
                "name",
                "folder",
                "itemID",
                "sourceExternalFilepath",
                "sourceLastImported",
                "sourcePlatform",
                "externalFileSize",
                "externalFileCRC32",
                "href",
                "videoDataHRef",
                "videoType",
                "fps",
                "width",
                "height",
                "length",
                "channels",
                "rate",
                "bits",
                "lastModified",
                "linkageExportForAS",
                "linkageExportInFirstFrame",
                "linkageIdentifier",
                "linkageClassName",
                "linkageBaseClass",
                "linkageURL",
                "linkageExportForRS",
                "linkageImportForRS",
            ],
            &[],
        );
        let name = item.attribute("name").unwrap_or("");
        let w = self
            .number(item, "width", 320.0, "DOMDocument.xml")
            .clamp(1.0, 8192.0);
        let h = self
            .number(item, "height", 240.0, "DOMDocument.xml")
            .clamp(1.0, 8192.0);
        if !name.is_empty() {
            self.videos.insert(name.to_owned(), (w, h));
        }
        self.report.warning(
            "xfl_video_approximated",
            loc,
            "DOMVideoItem metadata is registered as a Video viewport; embedded FLV/video stream data is not bundled.",
        );
    }

    pub(super) fn parse_compiled_clip_item(&mut self, item: Node<'_, '_>) {
        let loc = self.location(item, "DOMDocument.xml");
        self.audit(
            item,
            "DOMDocument.xml",
            &[
                "name",
                "folder",
                "itemID",
                "sourceExternalFilepath",
                "sourceLastImported",
                "sourcePlatform",
                "lastModified",
                "tooltip",
                "customIconID",
                "actionscriptClass",
                "swfScmHRef",
                "swfScmSourceFilename",
                "persistLivePreview11",
                "livePreviewScmHRef",
                "livePreviewScmSourceFilename",
                "livePreview11ScmHRef",
                "livePreview11ScmSourceFilename",
                "editFrameIndex",
                "requiredMinimumPlayerVersion",
                "requiredMinimumASVersion",
                "parametersAreLocked",
                "swcPath",
                "rootSymbolLinkageID",
                "playerVersion",
                "actionscriptVersion",
                "displayAsComponent",
                "hashKey",
                "movieRectLeft",
                "movieRectTop",
                "movieRectRight",
                "movieRectBottom",
                "linkageExportForAS",
                "linkageExportInFirstFrame",
                "linkageIdentifier",
                "linkageClassName",
                "linkageBaseClass",
                "linkageURL",
                "linkageExportForRS",
                "linkageImportForRS",
            ],
            &[
                "classProperties",
                "customIcon",
                "SwcItem",
                "parametersAsXML",
                "dataBindingXML",
                "persistentData",
            ],
        );
        if let Some(pd_node) = child(item, "persistentData") {
            self.audit_persistent_data(pd_node, "DOMDocument.xml");
        }
        for meta_tag in [
            "classProperties",
            "customIcon",
            "SwcItem",
            "parametersAsXML",
            "dataBindingXML",
        ] {
            if let Some(sub) = child(item, meta_tag) {
                self.audit_unmodeled_xfl_subtree(sub, "DOMDocument.xml");
            }
        }
        let name = item.attribute("name").unwrap_or("");
        if !name.is_empty() && !self.symbols.contains_key(name) {
            let idx = self.extra_symbols.len() + 1;
            let sid = format!("xfl-compiled-clip-{idx}");
            let lid = format!("xfl-compiled-clip-layer-{idx}");
            let w = (self.number(item, "movieRectRight", 2000.0, "DOMDocument.xml")
                - self.number(item, "movieRectLeft", 0.0, "DOMDocument.xml"))
                / 20.0;
            let h = (self.number(item, "movieRectBottom", 2000.0, "DOMDocument.xml")
                - self.number(item, "movieRectTop", 0.0, "DOMDocument.xml"))
                / 20.0;
            let rect_el = Element {
                id: format!("xfl-compiled-clip-el-{idx}"),
                name: String::new(),
                accessibility: None,
                transform: Transform::default(),
                kind: ElementKind::Rectangle {
                    width: w.clamp(10.0, 4096.0),
                    height: h.clamp(10.0, 4096.0),
                    fill: [180, 180, 180, 64],
                    stroke: [120, 120, 120, 160],
                    stroke_width: 1.0,
                },
                scripts: vec![],
                ..Element::default()
            };
            let display_name = name.rsplit_once('/').map_or(name, |(_, b)| b);
            self.extra_symbols.push(Symbol {
                folder: String::new(),
                class_name: None,
                linkage_id: None,
                id: sid.clone(),
                name: display_name.to_owned(),
                kind: SymbolKind::MovieClip,
                timeline: Timeline {
                    guides: vec![],
                    frame_count: 1,
                    layers: vec![Layer {
                        properties: LayerProperties::default(),
                        id: lid,
                        name: "Layer 1".into(),
                        visible: true,
                        locked: false,
                        keyframes: vec![Keyframe {
                            frame: 0,
                            elements: vec![rect_el],
                            ..Default::default()
                        }],
                    }],
                    scripts: vec![],
                },
            });
            self.symbols
                .insert(name.to_owned(), (sid, SymbolKind::MovieClip));
        }
        self.report.warning(
            "xfl_compiled_clip_approximated",
            loc,
            "DOMCompiledClipItem SWC/compiled clip is imported as a placeholder MovieClip symbol.",
        );
    }

    pub(super) fn register_resource(
        &mut self,
        resource_id: &str,
        path: String,
        item_name: &str,
        media_type: &str,
        bytes: &[u8],
        location: &str,
    ) -> Option<ResourceUnit> {
        if self.resources.len() >= MAX_RESOURCES {
            self.report.error(
                "xfl_media_invalid",
                location,
                "Project exceeds the 1024-resource limit.",
            );
            return None;
        }
        let Some(next_total) = self
            .resource_bytes_total
            .checked_add(bytes.len())
            .filter(|&n| n <= MAX_RESOURCE_TOTAL_BYTES)
        else {
            self.report.error(
                "xfl_media_invalid",
                location,
                "Decoded media resources exceed the 64 MiB project budget.",
            );
            return None;
        };
        let unit = match ResourceUnit::from_bytes(resource_id, path, media_type, bytes) {
            Ok(unit) => unit,
            Err(err) => {
                self.report.error(
                    "xfl_media_invalid",
                    location,
                    format!("Invalid media resource {item_name:?}: {err}"),
                );
                return None;
            }
        };
        self.resource_bytes_total = next_total;
        self.resources.push(unit.clone());
        Some(unit)
    }

    pub(super) fn parse_bitmap_item(
        &mut self,
        item: Node<'_, '_>,
        binary_files: &BTreeMap<String, Vec<u8>>,
        video_previews: bool,
    ) {
        let loc = self.location(item, "DOMDocument.xml");
        self.audit(
            item,
            "DOMDocument.xml",
            &[
                "name",
                "folder",
                "itemID",
                "sourceExternalFilepath",
                "sourceLastImported",
                "sourcePlatform",
                "externalFileSize",
                "externalFileCRC32",
                "useImportedJPEGData",
                "compressionType",
                "originalCompressionType",
                "quality",
                "href",
                "bitmapDataHRef",
                "frameLeft",
                "frameTop",
                "frameRight",
                "frameBottom",
                "allowSmoothing",
                "useDeblocking",
                "isJPEG",
                "lastModified",
                "linkageExportForAS",
                "linkageExportInFirstFrame",
                "linkageIdentifier",
                "linkageClassName",
                "linkageBaseClass",
                "linkageURL",
                "linkageExportForRS",
                "linkageImportForRS",
            ],
            &[],
        );
        let smoothing = self.boolean(item, "allowSmoothing", false, "DOMDocument.xml");
        let _ = self.boolean(item, "useImportedJPEGData", true, "DOMDocument.xml");
        let _ = self.boolean(item, "useDeblocking", false, "DOMDocument.xml");
        let _ = self.boolean(item, "isJPEG", false, "DOMDocument.xml");
        let _ = self.boolean(item, "linkageExportInFirstFrame", true, "DOMDocument.xml");
        let export_as = self.boolean(item, "linkageExportForAS", false, "DOMDocument.xml");
        let export_rs = self.boolean(item, "linkageExportForRS", false, "DOMDocument.xml");
        let import_rs = self.boolean(item, "linkageImportForRS", false, "DOMDocument.xml");
        if export_as
            || export_rs
            || import_rs
            || item
                .attribute("linkageClassName")
                .is_some_and(|s| !s.is_empty())
            || item
                .attribute("linkageIdentifier")
                .is_some_and(|s| !s.is_empty())
        {
            self.report.warning(
                "xfl_bitmap_linkage_unsupported",
                &loc,
                "ActionScript BitmapData linkage on XFL bitmap items is not bound as a runtime class yet; the bitmap image is imported.",
            );
        }

        let name = item.attribute("name").unwrap_or("");
        if name.is_empty() || self.bitmaps.contains_key(name) {
            self.report.error(
                "xfl_media_invalid",
                &loc,
                "DOMBitmapItem name must be nonempty and unique.",
            );
            return;
        }

        let mut external_bytes: Option<&[u8]> = None;
        if let Some(href) = item.attribute("href").filter(|s| !s.is_empty()) {
            if let Err(err) = source::safe_name(href) {
                self.report.error("unsafe_reference", &loc, err.to_string());
                return;
            }
            external_bytes = get_binary_ci(binary_files, &format!("LIBRARY/{href}"))
                .or_else(|| get_binary_ci(binary_files, href));
        }
        if external_bytes.is_none() && source::safe_name(name).is_ok() {
            external_bytes = get_binary_ci(binary_files, &format!("LIBRARY/{name}"))
                .or_else(|| get_binary_ci(binary_files, name));
        }

        let idx = self.bitmaps.len() + 1;
        let resource_id = format!("xfl-bitmap-{idx}");

        let has_dat_href = item
            .attribute("bitmapDataHRef")
            .is_some_and(|s| !s.is_empty());

        if let Some(raw) = external_bytes {
            let owned_png: Option<Vec<u8>> =
                if detect_raster_format(raw).is_none() && raw.starts_with(b"BM") {
                    image::load_from_memory_with_format(raw, image::ImageFormat::Bmp)
                        .ok()
                        .and_then(|img| {
                            let mut buf = std::io::Cursor::new(Vec::new());
                            img.write_to(&mut buf, image::ImageFormat::Png).ok()?;
                            Some(buf.into_inner())
                        })
                } else {
                    None
                };
            let effective_raw = owned_png.as_deref().unwrap_or(raw);
            if let Some((media_type, ext)) = detect_raster_format(effective_raw) {
                let preview = if video_previews
                    && let Some(size) = item.attribute("href").and_then(media::video_preview_size)
                {
                    match media::decode_video_preview(effective_raw, ext, size) {
                        Ok(preview) => preview,
                        Err(error) => {
                            self.report.error(
                                "xfl_media_invalid",
                                &loc,
                                format!("Invalid video frame image {name:?}: {error}"),
                            );
                            return;
                        }
                    }
                } else {
                    None
                };
                let (effective_raw, source_scale) = preview
                    .as_ref()
                    .map_or((effective_raw, [1., 1.]), |(bytes, scale)| {
                        (bytes.as_slice(), *scale)
                    });
                let Some(unit) = self.register_resource(
                    &resource_id,
                    build_xfl_resource_path("assets/bitmaps", idx, name, ext),
                    name,
                    media_type,
                    effective_raw,
                    &loc,
                ) else {
                    return;
                };
                match unit.decode_image() {
                    Ok(decoded) => {
                        self.bitmaps.insert(
                            name.to_owned(),
                            XflBitmapAsset {
                                resource_id,
                                average_color: crate::average_bitmap_color(&decoded.rgba),
                                width: decoded.width,
                                height: decoded.height,
                                source_scale,
                                smoothing,
                            },
                        );
                        return;
                    }
                    Err(err) => {
                        self.resources.pop();
                        if !has_dat_href {
                            self.report.error(
                                "xfl_media_invalid",
                                &loc,
                                format!("Cannot decode bitmap image {name:?}: {err}"),
                            );
                            return;
                        }
                    }
                }
            } else if !has_dat_href {
                self.report.error(
                    "xfl_media_invalid",
                    &loc,
                    format!("Bitmap file for {name:?} is neither PNG, JPEG nor BMP."),
                );
                return;
            }
        }

        if let Some(dat_href) = item.attribute("bitmapDataHRef").filter(|s| !s.is_empty()) {
            if let Err(err) = source::safe_name(dat_href) {
                self.report.error("unsafe_reference", &loc, err.to_string());
                return;
            }
            if let Some(raw) = get_binary_ci(binary_files, &format!("bin/{dat_href}"))
                .or_else(|| get_binary_ci(binary_files, dat_href))
            {
                let (bytes_vec, media_type, ext) = if let Some((mt, ext)) =
                    detect_raster_format(raw)
                {
                    (raw.to_vec(), mt, ext)
                } else {
                    match crate::legacy::media::decode_binary_bitmap(raw, 0)
                        .and_then(|decoded| decoded.png_bytes())
                    {
                        Ok(png) => (png, "image/png", "png"),
                        Err(err) => {
                            self.report.warning(
                                    "xfl_media_invalid",
                                    &loc,
                                    format!(
                                        "Cannot decode binary bitmap cache {dat_href:?} ({err}); substituted a 1x1 transparent PNG placeholder."
                                    ),
                                );
                            (SYNTHETIC_1X1_PNG.to_vec(), "image/png", "png")
                        }
                    }
                };
                let Some(unit) = self.register_resource(
                    &resource_id,
                    build_xfl_resource_path("assets/bitmaps", idx, name, ext),
                    name,
                    media_type,
                    &bytes_vec,
                    &loc,
                ) else {
                    return;
                };
                match unit.decode_image() {
                    Ok(decoded) => {
                        self.bitmaps.insert(
                            name.to_owned(),
                            XflBitmapAsset {
                                resource_id,
                                average_color: crate::average_bitmap_color(&decoded.rgba),
                                width: decoded.width,
                                height: decoded.height,
                                source_scale: [1., 1.],
                                smoothing,
                            },
                        );
                    }
                    Err(err) => {
                        self.resources.pop();
                        self.report.error(
                            "xfl_media_invalid",
                            &loc,
                            format!("Cannot decode converted bitmap for {name:?}: {err}"),
                        );
                    }
                }
                return;
            }
        }

        self.report.error(
            "xfl_media_invalid",
            &loc,
            format!("Referenced bitmap file for {name:?} is missing."),
        );
    }

    pub(super) fn parse_sound_item(
        &mut self,
        item: Node<'_, '_>,
        binary_files: &BTreeMap<String, Vec<u8>>,
    ) {
        let loc = self.location(item, "DOMDocument.xml");
        self.audit(
            item,
            "DOMDocument.xml",
            &[
                "name",
                "folder",
                "itemID",
                "sourceExternalFilepath",
                "sourceLastImported",
                "sourcePlatform",
                "externalFileSize",
                "externalFileCRC32",
                "href",
                "soundDataHRef",
                "format",
                "sampleCount",
                "dataLength",
                "exportFormat",
                "exportBits",
                "sampleRate",
                "bits",
                "channels",
                "cacheFormat",
                "cachedSampleCount",
                "exportNative",
                "use8kSampleRate",
                "deviceSoundHRef",
                "isangan",
                "lastModified",
                "compressionType",
                "quality",
                "bitRate",
                "linkageExportForAS",
                "linkageExportInFirstFrame",
                "linkageIdentifier",
                "linkageClassName",
                "linkageBaseClass",
                "linkageURL",
                "linkageExportForRS",
                "linkageImportForRS",
            ],
            &[],
        );
        let _ = self.boolean(item, "exportNative", false, "DOMDocument.xml");
        let _ = self.boolean(item, "use8kSampleRate", false, "DOMDocument.xml");
        let _ = self.boolean(item, "linkageExportInFirstFrame", true, "DOMDocument.xml");
        let export_as = self.boolean(item, "linkageExportForAS", false, "DOMDocument.xml");
        let export_rs = self.boolean(item, "linkageExportForRS", false, "DOMDocument.xml");
        let import_rs = self.boolean(item, "linkageImportForRS", false, "DOMDocument.xml");
        if export_as
            || export_rs
            || import_rs
            || item
                .attribute("linkageClassName")
                .is_some_and(|s| !s.is_empty())
            || item
                .attribute("linkageIdentifier")
                .is_some_and(|s| !s.is_empty())
        {
            self.report.warning(
                "xfl_sound_linkage_unsupported",
                &loc,
                "ActionScript Sound linkage on XFL sound items is not bound as a runtime class yet; the audio asset is imported.",
            );
        }

        let sample_count = self.u32_attr(item, "sampleCount", 0, "DOMDocument.xml");
        let data_length = item
            .attribute("dataLength")
            .map(|_| self.u32_attr(item, "dataLength", 0, "DOMDocument.xml"));

        let name = item.attribute("name").unwrap_or("");
        if name.is_empty() || self.sounds.contains_key(name) {
            self.report.error(
                "xfl_media_invalid",
                &loc,
                "DOMSoundItem name must be nonempty and unique.",
            );
            return;
        }

        let format_str = item.attribute("format").unwrap_or("44kHz 16bit Stereo");
        let (base_flags, fmt_sample_rate) = parse_xfl_sound_format_flags(format_str);

        let mut external_bytes: Option<&[u8]> = None;
        if let Some(href) = item.attribute("href").filter(|s| !s.is_empty()) {
            if let Err(err) = source::safe_name(href) {
                self.report.error("unsafe_reference", &loc, err.to_string());
                return;
            }
            external_bytes = get_binary_ci(binary_files, &format!("LIBRARY/{href}"))
                .or_else(|| get_binary_ci(binary_files, href));
        }
        if external_bytes.is_none() && source::safe_name(name).is_ok() {
            external_bytes = get_binary_ci(binary_files, &format!("LIBRARY/{name}"))
                .or_else(|| get_binary_ci(binary_files, name));
        }

        let idx = self.sounds.len() + 1;
        let resource_id = format!("xfl-sound-{idx}");

        if let Some(raw) = external_bytes {
            if raw.len() >= 12 && &raw[0..4] == b"RIFF" && &raw[8..12] == b"WAVE" {
                let Some(duration_44) = parse_wav_duration_44(raw) else {
                    self.report.error(
                        "xfl_media_invalid",
                        &loc,
                        format!("Malformed WAV audio file for {name:?}."),
                    );
                    return;
                };
                if self
                    .register_resource(
                        &resource_id,
                        build_xfl_resource_path("assets/sounds", idx, name, "wav"),
                        name,
                        "audio/wav",
                        raw,
                        &loc,
                    )
                    .is_some()
                {
                    self.sounds.insert(
                        name.to_owned(),
                        XflSoundAsset {
                            resource_id,
                            duration_44,
                        },
                    );
                }
                return;
            }
            if looks_like_xfl_mp3(raw) {
                let duration_44 = if sample_count > 0 {
                    (u64::from(sample_count) * 44_100) / u64::from(fmt_sample_rate)
                } else {
                    0
                };
                if self
                    .register_resource(
                        &resource_id,
                        build_xfl_resource_path("assets/sounds", idx, name, "mp3"),
                        name,
                        "audio/mpeg",
                        raw,
                        &loc,
                    )
                    .is_some()
                {
                    self.sounds.insert(
                        name.to_owned(),
                        XflSoundAsset {
                            resource_id,
                            duration_44,
                        },
                    );
                }
                return;
            }
            self.report.error(
                "xfl_media_invalid",
                &loc,
                format!("Audio file for {name:?} is neither valid WAV nor MP3."),
            );
            return;
        }

        if let Some(dat_href) = item.attribute("soundDataHRef").filter(|s| !s.is_empty()) {
            if let Err(err) = source::safe_name(dat_href) {
                self.report.error("unsafe_reference", &loc, err.to_string());
                return;
            }
            if let Some(raw_slice) = get_binary_ci(binary_files, &format!("bin/{dat_href}"))
                .or_else(|| get_binary_ci(binary_files, dat_href))
            {
                let mut raw = raw_slice;
                let mut flags = base_flags;
                let href_lower = item.attribute("href").unwrap_or(name).to_ascii_lowercase();
                let fmt_lower = format_str.to_ascii_lowercase();
                let is_mp3 = (data_length.is_some_and(|d| d > 0)
                    || href_lower.ends_with(".mp3")
                    || fmt_lower.contains("mp3"))
                    && looks_like_xfl_mp3(raw);
                let mut effective_samples = sample_count;
                let encoded_byte_count = if is_mp3 {
                    flags |= 0x20;
                    let dl = data_length.filter(|&d| d > 0).unwrap_or(raw.len() as u32);
                    raw = &raw[..(dl as usize).min(raw.len())];
                    Some(dl)
                } else if fmt_lower.contains("adpcm") {
                    flags |= 0x10;
                    data_length
                } else {
                    let block_align = usize::from(1 + u16::from((flags & 2) != 0))
                        * usize::from(1 + u16::from((flags & 1) != 0));
                    if effective_samples == 0 && raw.len() >= block_align {
                        effective_samples = (raw.len() / block_align) as u32;
                    } else if effective_samples > 0 {
                        let expected = (effective_samples as usize).saturating_mul(block_align);
                        if expected > 0 && raw.len() > expected && raw.len() % block_align == 0 {
                            raw = &raw[..expected];
                        }
                    }
                    None
                };

                let settings = crate::legacy::contents::ContentsSoundSettings {
                    format_flags: flags,
                    sample_count: effective_samples,
                    encoded_byte_count,
                    secondary_format_flags: None,
                    secondary_sample_count: None,
                    ..Default::default()
                };
                match crate::legacy::media::decode_binary_sound(raw, 8, &settings) {
                    Ok(decoded) => {
                        if decoded.short_read {
                            self.report.warning(
                                "xfl_sound_short_read",
                                &loc,
                                "Binary sound cache ended before declared sample count; available audio frames were imported.",
                            );
                        }
                        if self
                            .register_resource(
                                &resource_id,
                                build_xfl_resource_path(
                                    "assets/sounds",
                                    idx,
                                    name,
                                    decoded.extension,
                                ),
                                name,
                                decoded.media_type,
                                &decoded.bytes,
                                &loc,
                            )
                            .is_some()
                        {
                            self.sounds.insert(
                                name.to_owned(),
                                XflSoundAsset {
                                    resource_id,
                                    duration_44: u64::from(decoded.duration_44),
                                },
                            );
                        }
                    }
                    Err(err) => {
                        self.report.error(
                            "xfl_media_invalid",
                            &loc,
                            format!("Cannot decode binary sound cache {dat_href:?}: {err}"),
                        );
                    }
                }
                return;
            }
        }

        self.report.error(
            "xfl_media_invalid",
            &loc,
            format!("Referenced audio file for {name:?} is missing."),
        );
    }

    pub(super) fn frame_sound(
        &mut self,
        frame_node: Node<'_, '_>,
        file: &str,
    ) -> Option<FrameSound> {
        let loc = self.location(frame_node, file);
        let mut raw_env_points: Vec<(u64, u32, u32)> = Vec::new();
        let has_env_node = if let Some(env_node) = child(frame_node, "SoundEnvelope") {
            self.audit(env_node, file, &[], &["SoundEnvelopePoint"]);
            for pt in elements(env_node).filter(|n| n.has_tag_name("SoundEnvelopePoint")) {
                self.audit(pt, file, &["mark44", "level0", "level1"], &[]);
                let mark44 = self.u64_attr(pt, "mark44", 0, file);
                let level0 = self.u32_attr(pt, "level0", 32_768, file);
                let level1 = self.u32_attr(pt, "level1", 32_768, file);
                raw_env_points.push((mark44, level0, level1));
            }
            if raw_env_points.is_empty() || raw_env_points.windows(2).any(|w| w[1].0 < w[0].0) {
                self.report.error(
                    "invalid_sound_envelope",
                    &loc,
                    "SoundEnvelope must contain at least one SoundEnvelopePoint sorted by mark44.",
                );
            }
            true
        } else {
            false
        };
        let raw_loops = self.u64_attr(frame_node, "soundLoop", 1, file);
        let in_point = self.u64_attr(frame_node, "inPoint44", 0, file);
        let out_point = self.u64_attr(frame_node, "outPoint44", 0, file);
        let _ = self.u64_attr(frame_node, "soundZoomLevel", 0, file);

        let Some(sound_name) = frame_node.attribute("soundName").filter(|s| !s.is_empty()) else {
            if has_env_node {
                self.report.error(
                    "invalid_sound_envelope",
                    &loc,
                    "SoundEnvelope requires a nonempty soundName on DOMFrame.",
                );
            }
            return None;
        };
        let Some(asset) = self.sounds.get(sound_name).cloned() else {
            self.report.error(
                "missing_sound",
                &loc,
                format!("Unknown sound library item {sound_name:?}."),
            );
            return None;
        };

        let sync = match frame_node.attribute("soundSync").unwrap_or("event") {
            "event" => SoundSync::Event,
            "start" => SoundSync::Start,
            "stop" => SoundSync::Stop,
            "stream" => SoundSync::Stream,
            other => {
                self.report.error(
                    "invalid_sound_sync",
                    &loc,
                    format!("Unsupported soundSync value {other:?}."),
                );
                SoundSync::Event
            }
        };

        let (loops, loop_forever) = match frame_node.attribute("soundLoopMode") {
            Some("loop") => (1, true),
            None | Some("repeat") => {
                if raw_loops > u64::from(FrameSound::MAX_LOOPS) {
                    self.report.warning(
                        "xfl_frame_sound",
                        &loc,
                        format!(
                            "Frame-sound repeat count {raw_loops} clamped to {}.",
                            FrameSound::MAX_LOOPS
                        ),
                    );
                    (FrameSound::MAX_LOOPS, false)
                } else {
                    ((raw_loops as u32).max(1), false)
                }
            }
            Some(other) => {
                self.report.error(
                    "invalid_sound_loop_mode",
                    &loc,
                    format!("Unsupported soundLoopMode value {other:?}."),
                );
                (1, false)
            }
        };

        let (effect, envelope) = match frame_node.attribute("soundEffect") {
            Some("none") => (SoundEffect::None, None),
            Some("left channel") => (SoundEffect::LeftChannel, None),
            Some("right channel") => (SoundEffect::RightChannel, None),
            Some("fade left to right") => (SoundEffect::FadeLeftToRight, None),
            Some("fade right to left") => (SoundEffect::FadeRightToLeft, None),
            Some("fade in") => (SoundEffect::FadeIn, None),
            Some("fade out") => (SoundEffect::FadeOut, None),
            Some("custom") => {
                if raw_env_points.is_empty() {
                    (SoundEffect::None, None)
                } else {
                    let points = build_xfl_sound_envelope(
                        raw_env_points,
                        asset.duration_44,
                        &loc,
                        self.report,
                    );
                    (SoundEffect::Custom, Some(points))
                }
            }
            None => {
                let is_unity = raw_env_points.is_empty()
                    || raw_env_points
                        .iter()
                        .all(|&(_, l0, l1)| l0 == 32_768 && l1 == 32_768);
                if is_unity {
                    (SoundEffect::None, None)
                } else {
                    let points = build_xfl_sound_envelope(
                        raw_env_points,
                        asset.duration_44,
                        &loc,
                        self.report,
                    );
                    (SoundEffect::Custom, Some(points))
                }
            }
            Some(other) => {
                self.report.error(
                    "invalid_sound_effect",
                    &loc,
                    format!("Unsupported soundEffect value {other:?}."),
                );
                (SoundEffect::None, None)
            }
        };

        let in_point_opt = (in_point > 0)
            .then(|| (in_point as f64 / 44_100.0).clamp(0.0, FrameSound::MAX_SOUND_TRIM_SECONDS));
        let out_point_opt = (out_point > 0 && out_point < 0x3fff_ffff)
            .then(|| (out_point as f64 / 44_100.0).clamp(0.0, FrameSound::MAX_SOUND_TRIM_SECONDS))
            .filter(|&out_s| in_point_opt.is_none_or(|in_s| out_s > in_s));

        Some(FrameSound {
            resource: asset.resource_id,
            sync,
            loops,
            loop_forever,
            effect,
            envelope,
            in_point: in_point_opt,
            out_point: out_point_opt,
        })
    }
}

pub(super) fn build_xfl_sound_envelope(
    mut raw_points: Vec<(u64, u32, u32)>,
    asset_duration_44: u64,
    loc: &str,
    report: &mut ImportReport,
) -> Vec<[f64; 3]> {
    raw_points.sort_by_key(|p| p.0);
    if raw_points.len() > FrameSound::MAX_ENVELOPE_POINTS {
        raw_points.truncate(FrameSound::MAX_ENVELOPE_POINTS);
        report.warning(
            "xfl_frame_sound",
            loc,
            format!(
                "Custom sound envelope truncated to {} control points.",
                FrameSound::MAX_ENVELOPE_POINTS
            ),
        );
    }
    if raw_points
        .iter()
        .any(|&(_, l0, l1)| l0 > 32_768 || l1 > 32_768)
    {
        report.warning(
            "xfl_frame_sound",
            loc,
            "Sound envelope gain above unity (>32768) is clamped to 1.0.",
        );
    }
    let max_mark = raw_points.iter().map(|p| p.0).max().unwrap_or(0);
    let denom = asset_duration_44.max(max_mark);
    let mut prev_pos = 0.0_f64;
    let mut points = Vec::with_capacity(raw_points.len());
    for (mark44, level0, level1) in raw_points {
        let pos = if denom == 0 {
            0.0
        } else {
            (mark44 as f64 / denom as f64).clamp(0.0, 1.0)
        }
        .max(prev_pos);
        prev_pos = pos;
        let left = (f64::from(level0) / 32_768.0).clamp(0.0, 1.0);
        let right = (f64::from(level1) / 32_768.0).clamp(0.0, 1.0);
        points.push([pos, left, right]);
    }
    points
}

pub(super) fn build_xfl_resource_path(
    prefix: &str,
    idx: usize,
    item_name: &str,
    ext: &str,
) -> String {
    let base = item_name.rsplit_once('/').map_or(item_name, |(_, b)| b);
    let dot_ext = format!(".{ext}");
    let stem = if base.len() > dot_ext.len()
        && base[base.len() - dot_ext.len()..].eq_ignore_ascii_case(&dot_ext)
    {
        &base[..base.len() - dot_ext.len()]
    } else {
        base
    };
    let mut sanitized = String::with_capacity(stem.len().min(64));
    for ch in stem.chars().take(64) {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
            sanitized.push(ch);
        } else {
            sanitized.push('_');
        }
    }
    let trimmed = sanitized.trim_matches('_');
    let final_stem = if trimmed.is_empty() { "asset" } else { trimmed };
    format!("{prefix}/{idx}-{final_stem}.{ext}")
}

pub(super) fn detect_raster_format(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(("image/png", "png"))
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some(("image/jpeg", "jpg"))
    } else {
        None
    }
}

pub(super) fn parse_xfl_sound_format_flags(format_str: &str) -> (u16, u32) {
    let lower = format_str.to_ascii_lowercase();
    let mut flags = 0u16;
    if lower.contains("stereo") {
        flags |= 0x01;
    }
    if lower.contains("16bit") || lower.contains("16 bit") {
        flags |= 0x02;
    }
    let sample_rate = if lower.contains("44khz") || lower.contains("44.1khz") {
        flags |= 0x0c;
        44_100
    } else if lower.contains("22khz") || lower.contains("22.05khz") {
        flags |= 0x08;
        22_050
    } else if lower.contains("11khz") || lower.contains("11.025khz") {
        flags |= 0x04;
        11_025
    } else if lower.contains("5.5khz") || lower.contains("5khz") || lower.contains("8khz") {
        5_512
    } else {
        flags |= 0x0c;
        44_100
    };
    (flags, sample_rate)
}

pub(super) const SYNTHETIC_1X1_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x60, 0x60, 0x60, 0x60,
    0x00, 0x00, 0x00, 0x05, 0x00, 0x01, 0xA5, 0xF6, 0x45, 0x40, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];
