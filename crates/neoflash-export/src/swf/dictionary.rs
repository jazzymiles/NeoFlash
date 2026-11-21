//! SWF character dictionary & timeline display-list compiler (`SwfDictionaryCompiler`).

use super::*;

pub(super) struct SwfDictionaryCompiler<'a> {
    pub(super) project: &'a Project,
    pub(super) options: &'a SwfPublishOptions,
    pub(super) next_char_id: u32,
    pub(super) dictionary_overflow: bool,
    pub(super) root_tags: SwfBitWriter,
    pub(super) bitmap_ids: BTreeMap<String, u16>,
    pub(super) bitmap_meta: BTreeMap<String, (u32, u32, Option<bool>)>,
    pub(super) bitmap_quad_ids: BTreeMap<(String, u32, u32, bool), u16>,
    pub(super) sound_ids: BTreeMap<String, u16>,
    pub(super) sound_durations_44: BTreeMap<String, u64>,
    pub(super) stream_sounds: BTreeMap<String, PreparedSwfStreamSound>,
    pub(super) font_ids: BTreeMap<(String, bool, bool), u16>,
    pub(super) font_tables: BTreeMap<(String, bool, bool), SwfFont3Table>,
    pub(super) font_usage: ProjectFont3Usage,
    pub(super) outline_fonts: SwfOutlineFonts,
    pub(super) shape_characters: HashMap<[u8; 32], u16>,
    pub(super) anonymous_sprites: HashMap<(u16, [u8; 32]), u16>,
    pub(super) symbol_char_ids: BTreeMap<String, u16>,
    pub(super) button_variant_ids: BTreeMap<(String, String), u16>,
    pub(super) sprite_variant_ids: BTreeMap<String, u16>,
    pub(super) avm1_exports: Vec<(u16, String)>,
    pub(super) avm2_symbol_classes: Vec<(u16, String)>,
    pub(super) avm2_symbol_kinds: BTreeMap<String, SymbolKind>,
    pub(super) avm2_symbol_frame_scripts: BTreeMap<String, Vec<(u32, String)>>,
    pub(super) avm2_anon_symbol_scripts: Vec<(u16, Vec<(u32, String)>)>,
    pub(super) avm2_classes: Vec<AbcClassSpec>,
}

pub(super) fn element_character_key(
    element: &Element,
    fill: Option<&neoflash_core::AuthoredBitmapFill>,
) -> Option<[u8; 32]> {
    let paint = fill.map(|f| (&f.resource_id, f.matrix, f.repeat, f.smoothing));
    let has_inst_name = !element.name.trim().is_empty();
    serde_json::to_vec(&(
        &element.kind,
        &element.transform.primitive,
        has_inst_name,
        paint,
    ))
    .ok()
    .map(|bytes| crate::sha256_bytes(&bytes))
}

pub(super) fn button_scripts_variant_key(scripts: &[ObjectScript]) -> String {
    if scripts.is_empty() {
        String::new()
    } else {
        serde_json::to_string(scripts).unwrap_or_default()
    }
}

pub(super) fn collect_expanded_as3_frame_scripts(
    project: &Project,
    timeline: &Timeline,
) -> Result<Vec<(u32, String)>, ExportError> {
    let mut scripts_by_frame: BTreeMap<u32, String> = BTreeMap::new();
    for fs in &timeline.scripts {
        if fs.script.language == ScriptLanguage::ActionScript3 {
            let expanded = expand_swf_includes(project, fs.script.language, "", &fs.script.source)
                .map_err(|err| {
                    ExportError::Options(format!(
                        "Frame {} AS3 include failed: {err}",
                        fs.frame + 1
                    ))
                })?;
            scripts_by_frame
                .entry(fs.frame)
                .and_modify(|existing| {
                    if !existing.ends_with('\n') {
                        existing.push('\n');
                    }
                    existing.push_str(&expanded);
                })
                .or_insert(expanded);
        }
    }
    Ok(scripts_by_frame.into_iter().collect())
}

pub(super) fn spec_qname(spec: &AbcClassSpec) -> String {
    if spec.pkg.is_empty() {
        spec.class_name.clone()
    } else {
        format!("{}.{}", spec.pkg, spec.class_name)
    }
}

impl<'a> SwfDictionaryCompiler<'a> {
    pub(super) fn new(project: &'a Project, options: &'a SwfPublishOptions) -> Self {
        Self {
            project,
            options,
            next_char_id: 1,
            dictionary_overflow: false,
            root_tags: SwfBitWriter::new(),
            bitmap_ids: BTreeMap::new(),
            bitmap_meta: BTreeMap::new(),
            bitmap_quad_ids: BTreeMap::new(),
            sound_ids: BTreeMap::new(),
            sound_durations_44: BTreeMap::new(),
            stream_sounds: BTreeMap::new(),
            font_ids: BTreeMap::new(),
            font_tables: BTreeMap::new(),
            font_usage: collect_project_font3_usage(project),
            outline_fonts: SwfOutlineFonts::default(),
            shape_characters: HashMap::new(),
            anonymous_sprites: HashMap::new(),
            symbol_char_ids: BTreeMap::new(),
            button_variant_ids: BTreeMap::new(),
            sprite_variant_ids: BTreeMap::new(),
            avm1_exports: Vec::new(),
            avm2_symbol_classes: Vec::new(),
            avm2_symbol_kinds: BTreeMap::new(),
            avm2_symbol_frame_scripts: BTreeMap::new(),
            avm2_anon_symbol_scripts: Vec::new(),
            avm2_classes: Vec::new(),
        }
    }

    pub(super) fn alloc_char_id(&mut self) -> u16 {
        if self.next_char_id > u32::from(u16::MAX) {
            self.dictionary_overflow = true;
            return 0;
        }
        let id = self.next_char_id as u16;
        self.next_char_id += 1;
        id
    }

    pub(super) fn ensure_media_resources(&mut self) -> Result<(), ExportError> {
        let (used_bitmaps, used_sounds) = used_swf_media_resources(self.project);
        for res in &self.project.resources {
            if !used_bitmaps.contains(&res.id) && !used_sounds.contains(&res.id) {
                continue;
            }
            let Ok(raw) = res.decoded_bytes() else {
                continue;
            };
            if used_bitmaps.contains(&res.id) {
                let bid = self.alloc_char_id();
                let image = res
                    .decode_image()
                    .map_err(|e| ExportError::Options(e.to_string()))?;
                if embeds_original_jpeg(res) && raw.starts_with(&[0xff, 0xd8]) {
                    let mut jpeg = SwfBitWriter::new();
                    jpeg.u16(bid);
                    jpeg.bytes(&raw);
                    self.root_tags.write_tag(21, &jpeg.buf);
                } else {
                    let payload =
                        build_define_bits_lossless2_payload(bid, &image).ok_or_else(|| {
                            ExportError::Options(format!(
                                "Failed to compress SWF bitmap '{}'",
                                res.path
                            ))
                        })?;
                    self.root_tags.write_tag(36, &payload);
                }
                self.bitmap_ids.insert(res.id.clone(), bid);
                self.bitmap_meta.insert(
                    res.id.clone(),
                    (
                        image.width,
                        image.height,
                        res.bitmap_export.map(|s| s.allow_smoothing),
                    ),
                );
            }
            if used_sounds.contains(&res.id) {
                let mut needs_event_def = false;
                let mut needs_stream_def = false;
                for sym in &self.project.symbols {
                    for layer in &sym.timeline.layers {
                        for kf in &layer.keyframes {
                            if let Some(snd) = &kf.sound
                                && snd.resource == res.id
                            {
                                if snd.sync == SoundSync::Stream {
                                    needs_stream_def = true;
                                } else {
                                    needs_event_def = true;
                                }
                            }
                        }
                    }
                }
                let info = analyze_swf_sound_resource(&res.media_type, &raw).map_err(|issue| {
                    ExportError::Options(format!(
                        "Frame sound '{}' cannot be encoded as SWF audio: {issue}",
                        res.path
                    ))
                })?;
                if needs_stream_def {
                    let prepared =
                        prepare_swf_stream_sound(&res.media_type, &raw).map_err(|issue| {
                            ExportError::Options(format!(
                                "Stream sound '{}' cannot be encoded as SoundStreamBlock: {issue}",
                                res.path
                            ))
                        })?;
                    self.stream_sounds.insert(res.id.clone(), prepared);
                }
                if needs_event_def || !needs_stream_def {
                    let duration_44 = u64::from(info.sample_count).saturating_mul(44_100)
                        / u64::from(info.sample_rate.max(1));
                    let sid = self.alloc_char_id();
                    let payload = build_define_sound_payload(sid, &res.media_type, &raw).map_err(
                        |issue| {
                            ExportError::Options(format!(
                                "Frame sound '{}' cannot be encoded as DefineSound: {issue}",
                                res.path
                            ))
                        },
                    )?;
                    self.root_tags.write_tag(14, &payload);
                    self.sound_ids.insert(res.id.clone(), sid);
                    self.sound_durations_44
                        .insert(res.id.clone(), duration_44.max(1));
                }
            }
        }
        Ok(())
    }

    pub(super) fn ensure_font_table(
        &mut self,
        name: &str,
        bold: bool,
        italic: bool,
    ) -> Result<SwfFont3Table, ExportError> {
        let key = (name.to_owned(), bold, italic);
        if let Some(tbl) = self.font_tables.get(&key) {
            return Ok(tbl.clone());
        }
        let fid = self.alloc_char_id();
        let used_codes = self
            .font_usage
            .required_chars
            .get(&key)
            .cloned()
            .unwrap_or_else(|| BTreeSet::from([0x0020]));
        let req_chars: Vec<char> = used_codes
            .iter()
            .filter_map(|&c| char::from_u32(u32::from(c)))
            .collect();
        let is_dynamic = self.font_usage.dynamic_faces.contains(&key);
        if let Ok(sfnt) = resolve_swf_font3_sfnt_bytes(
            self.project,
            name,
            bold,
            italic,
            &req_chars,
            &mut self.outline_fonts,
        ) && let Ok((table, payload)) = build_define_font3_with_glyphs_payload(
            fid,
            name,
            bold,
            italic,
            &sfnt,
            &used_codes,
            is_dynamic,
        ) {
            self.root_tags.write_tag(75, &payload);
            self.font_ids.insert(key.clone(), fid);
            self.font_tables.insert(key, table.clone());
            return Ok(table);
        }
        let payload = build_define_font3_payload(fid, name, bold, italic);
        self.root_tags.write_tag(75, &payload);
        let stub = SwfFont3Table {
            font_id: fid,
            code_to_glyph: BTreeMap::new(),
            advances_20480: Vec::new(),
            ascent_20480: 16384,
            descent_20480: 4096,
            leading_20480: 0,
        };
        self.font_ids.insert(key.clone(), fid);
        self.font_tables.insert(key, stub.clone());
        Ok(stub)
    }

    pub(super) fn ensure_timeline_element_character(
        &mut self,
        element: &Element,
        fill: Option<&neoflash_core::AuthoredBitmapFill>,
    ) -> Result<Option<u16>, ExportError> {
        if matches!(element.kind, ElementKind::Instance { .. }) {
            return self.ensure_element_character(element, fill);
        }
        // Character definitions are immutable throughout the movie. Reuse the same outline or
        // brush across sprites, preserving independent placement, blend, mask and transform tags.
        let Some(character_key) = element_character_key(element, fill) else {
            return self.ensure_element_character(element, fill);
        };
        if let Some(&cached) = self.shape_characters.get(&character_key) {
            return Ok(Some(cached));
        }
        let cid = self.ensure_element_character(element, fill)?;
        if let Some(cid) = cid {
            self.shape_characters.insert(character_key, cid);
        }
        Ok(cid)
    }

    pub(super) fn ensure_element_character(
        &mut self,
        element: &Element,
        fill: Option<&neoflash_core::AuthoredBitmapFill>,
    ) -> Result<Option<u16>, ExportError> {
        match &element.kind {
            ElementKind::Instance { symbol_id, .. } => {
                if let Some(sym) = self.project.symbol(symbol_id).cloned() {
                    let eff_kind = element.effective_symbol_kind_for(&sym);
                    if eff_kind == SymbolKind::Button
                        && (sym.kind != SymbolKind::Button
                            || (!self.options.profile.is_avm2()
                                && !self.options.scriptless_mode
                                && !element.scripts.is_empty()))
                    {
                        let scripts =
                            if self.options.profile.is_avm2() || self.options.scriptless_mode {
                                &[][..]
                            } else {
                                &element.scripts[..]
                            };
                        let key = button_scripts_variant_key(scripts);
                        if let Some(&cid) = self
                            .button_variant_ids
                            .get(&(symbol_id.clone(), key.clone()))
                        {
                            return Ok(Some(cid));
                        }
                        let cid = self.compile_button_symbol(&sym, scripts)?;
                        self.button_variant_ids
                            .insert((symbol_id.clone(), key), cid);
                        return Ok(Some(cid));
                    }
                    if eff_kind != SymbolKind::Button && sym.kind == SymbolKind::Button {
                        if let Some(&sid) = self.sprite_variant_ids.get(symbol_id) {
                            return Ok(Some(sid));
                        }
                        let (sprite_tags, frame_count) =
                            self.build_timeline_tags(&sym.timeline, false)?;
                        let sid = self.alloc_char_id();
                        let mut w = SwfBitWriter::new();
                        w.u16(sid);
                        w.u16(frame_count);
                        w.bytes(&sprite_tags);
                        self.root_tags.write_tag(39, &w.buf);
                        self.sprite_variant_ids.insert(symbol_id.clone(), sid);
                        return Ok(Some(sid));
                    }
                }
                Ok(self.symbol_char_ids.get(symbol_id).copied())
            }
            ElementKind::Bitmap {
                resource_id,
                width,
                height,
                smoothing,
            } => {
                let Some(&bid) = self.bitmap_ids.get(resource_id) else {
                    return Ok(None);
                };
                let (bw, bh, override_smooth) = self
                    .bitmap_meta
                    .get(resource_id)
                    .copied()
                    .unwrap_or(((*width).max(1), (*height).max(1), None));
                let eff_smooth = override_smooth.unwrap_or(*smoothing);
                let quad_key = (resource_id.clone(), *width, *height, eff_smooth);
                if let Some(&existing) = self.bitmap_quad_ids.get(&quad_key) {
                    return Ok(Some(existing));
                }
                let sid = self.alloc_char_id();
                let payload = build_define_bitmap_quad_shape_payload(
                    sid, bid, bw, bh, *width, *height, eff_smooth,
                );
                self.root_tags.write_tag(32, &payload);
                self.bitmap_quad_ids.insert(quad_key, sid);
                Ok(Some(sid))
            }
            ElementKind::TextBox {
                text,
                width,
                height,
                format,
                word_wrap,
                multiline,
            } if !self.options.outline_text => {
                let table = self.ensure_font_table(&format.font, format.bold, format.italic)?;
                let tid = self.alloc_char_id();
                let is_dynamic =
                    !element.name.trim().is_empty() || format.field_options().is_some();
                if !is_dynamic && table.has_glyphs() && !word_wrap {
                    let r = ((format.color >> 16) & 0xff) as u8;
                    let g = ((format.color >> 8) & 0xff) as u8;
                    let b = (format.color & 0xff) as u8;
                    let payload = build_define_text2_payload(
                        tid,
                        &table,
                        text,
                        *width,
                        *height,
                        format.size,
                        [r, g, b, 255],
                        format.align,
                        format.leading,
                    );
                    self.root_tags.write_tag(33, &payload);
                } else {
                    let payload = build_define_edit_text_payload(
                        tid,
                        table.font_id,
                        table.has_glyphs(),
                        text,
                        *width,
                        *height,
                        format,
                        *word_wrap,
                        *multiline,
                        1.0,
                    );
                    self.root_tags.write_tag(37, &payload);
                }
                Ok(Some(tid))
            }
            ElementKind::Text {
                text,
                font_family,
                font_size,
                color,
            } if !self.options.outline_text => {
                let table = self.ensure_font_table(font_family, false, false)?;
                let tid = self.alloc_char_id();
                let w = ((text.chars().count().max(1) as f64) * (*font_size) * 0.65).max(40.0);
                let h = ((*font_size) * 1.4).max(20.0);
                if table.has_glyphs() {
                    let payload = build_define_text2_payload(
                        tid,
                        &table,
                        text,
                        w,
                        h,
                        *font_size,
                        *color,
                        TextAlign::Left,
                        0.0,
                    );
                    self.root_tags.write_tag(33, &payload);
                } else {
                    let fmt = neoflash_core::TextFormat {
                        font: font_family.clone(),
                        size: *font_size,
                        color: (u32::from(color[0]) << 16)
                            | (u32::from(color[1]) << 8)
                            | u32::from(color[2]),
                        ..Default::default()
                    };
                    let payload = build_define_edit_text_payload(
                        tid,
                        table.font_id,
                        false,
                        text,
                        w,
                        h,
                        &fmt,
                        false,
                        text.contains('\n'),
                        f64::from(color[3]) / 255.0,
                    );
                    self.root_tags.write_tag(37, &payload);
                }
                Ok(Some(tid))
            }
            _ => {
                let sid = self.alloc_char_id();
                let bitmap = fill
                    .map(|f| {
                        self.bitmap_ids
                            .get(&f.resource_id)
                            .map(|&character| SwfBitmapBrush {
                                character,
                                matrix: f.matrix,
                                repeat: f.repeat,
                                smoothing: f.smoothing,
                            })
                            .ok_or_else(|| {
                                ExportError::Options(format!(
                                    "Missing SWF bitmap fill resource '{}'",
                                    f.resource_id
                                ))
                            })
                    })
                    .transpose()?;
                let Some((tag_code, payload)) = build_define_shape_for_element(
                    self.project,
                    sid,
                    element,
                    &mut self.outline_fonts,
                    bitmap,
                    self.options,
                )?
                else {
                    return Ok(None);
                };
                self.root_tags.write_tag(tag_code, &payload);
                Ok(Some(sid))
            }
        }
    }

    pub(super) fn initial_button_scripts_for_symbol(&self, sym: &Symbol) -> Vec<ObjectScript> {
        if self.options.profile.is_avm2() || self.options.scriptless_mode {
            return Vec::new();
        }
        let has_exported_name = sym
            .linkage_id
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
            || sym
                .class_name
                .as_deref()
                .is_some_and(|s| !s.trim().is_empty());
        if has_exported_name {
            return Vec::new();
        }
        let mut seen_unscripted = false;
        let mut first_scripted: Option<Vec<ObjectScript>> = None;
        for timeline in
            std::iter::once(&sym.timeline).chain(self.project.symbols.iter().map(|s| &s.timeline))
        {
            for layer in &timeline.layers {
                for kf in &layer.keyframes {
                    for el in &kf.elements {
                        if let ElementKind::Instance { symbol_id, .. } = &el.kind
                            && symbol_id == &sym.id
                        {
                            if el.scripts.is_empty() {
                                seen_unscripted = true;
                            } else if first_scripted.is_none() {
                                first_scripted = Some(el.scripts.clone());
                            }
                        }
                    }
                }
            }
        }
        if seen_unscripted {
            Vec::new()
        } else {
            first_scripted.unwrap_or_default()
        }
    }

    pub(super) fn compile_button_symbol(
        &mut self,
        sym: &Symbol,
        scripts: &[ObjectScript],
    ) -> Result<u16, ExportError> {
        type ButtonStateRecord = (
            u8,
            u16,
            u16,
            [f64; 6],
            ColorTransform,
            f64,
            BlendMode,
            Vec<FilterSpec>,
        );
        let mut state_records: Vec<ButtonStateRecord> = Vec::new();
        let depth_bands = compute_swf_timeline_layout(&sym.timeline)
            .map_err(|issue| {
                ExportError::Options(format!(
                    "Button '{}' timeline cannot be laid out in SWF: {issue:?}",
                    sym.name
                ))
            })?
            .depth_bands;

        for (l_idx, layer) in sym.timeline.layers.iter().enumerate() {
            if !layer.visible
                || matches!(layer.properties.role, LayerRole::Folder | LayerRole::Guide)
            {
                continue;
            }
            for state_frame in 0..4u32 {
                if layer
                    .properties
                    .end_frame
                    .is_some_and(|end| state_frame >= end)
                {
                    continue;
                }
                let state_bit = 1u8 << state_frame;
                let Some(kf) = layer.keyframes.iter().rfind(|k| k.frame <= state_frame) else {
                    continue;
                };
                for (element_offset, el) in kf.elements.iter().enumerate() {
                    let Some(cid) =
                        self.ensure_timeline_element_character(el, kf.bitmap_fill(&el.id))?
                    else {
                        continue;
                    };
                    // Button states may use different characters at the same depth as long as
                    // their state masks are disjoint. Reuse the timeline's checked layer bands
                    // instead of the former `layer * 16` arithmetic, which collided for wide
                    // layers and overflowed for large but valid button timelines.
                    let depth = depth_bands[l_idx]
                        .0
                        .checked_add(
                            u16::try_from(element_offset)
                                .expect("button element offset is checked by depth preflight"),
                        )
                        .expect("button depth is checked by preflight");
                    state_records.push((
                        state_bit,
                        cid,
                        depth,
                        transform_matrix(el.transform),
                        el.transform.color_transform,
                        el.transform.alpha,
                        el.blend_mode,
                        el.filters.clone(),
                    ));
                }
            }
        }

        // Ensure a valid DefineButton2 has at least one state record
        if state_records.is_empty() {
            let dummy_el = Element {
                id: "btn-fallback".to_owned(),
                kind: ElementKind::Rectangle {
                    width: 20.0,
                    height: 20.0,
                    fill: [0, 0, 0, 0],
                    stroke: [0, 0, 0, 0],
                    stroke_width: 0.0,
                },
                ..Element::default()
            };
            if let Some(cid) = self.ensure_element_character(&dummy_el, None)? {
                state_records.push((
                    0x0F,
                    cid,
                    1,
                    [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                    ColorTransform::default(),
                    1.0,
                    BlendMode::Normal,
                    Vec::new(),
                ));
            }
        }
        state_records.sort_by_key(|(state_mask, _, depth, ..)| (*depth, *state_mask));

        let button_id = self.alloc_char_id();
        let mut rec_writer = SwfBitWriter::new();
        for (state_mask, cid, depth, mat, ct, alpha, blend, filters) in state_records {
            let has_filters = !filters.is_empty();
            let has_blend = blend != BlendMode::Normal;
            let flags: u8 = (state_mask & 0x0F)
                | (if has_filters { 0x10 } else { 0 })
                | (if has_blend { 0x20 } else { 0 });
            rec_writer.u8(flags);
            rec_writer.u16(cid);
            rec_writer.u16(depth);
            rec_writer.matrix_px(mat);
            rec_writer.cxform_with_alpha(&ct, alpha);
            if has_filters {
                write_swf_filter_list(&mut rec_writer, &filters);
            }
            if has_blend {
                rec_writer.u8(encode_swf_blend_mode(blend));
            }
        }
        rec_writer.u8(0); // End of button records

        let cond_bytes = if !self.options.profile.is_avm2()
            && !self.options.scriptless_mode
            && !scripts.is_empty()
        {
            encode_button2_cond_actions(self.project, scripts).map_err(|error| {
                ExportError::Options(format!(
                    "Button '{}' cannot encode its AVM1 actions: {error}",
                    sym.name
                ))
            })?
        } else {
            Vec::new()
        };

        let mut w = SwfBitWriter::new();
        w.u16(button_id);
        w.u8(0); // flags (trackAsMenu = 0)
        if cond_bytes.is_empty() {
            w.u16(0); // action_offset = 0
            w.bytes(&rec_writer.buf);
        } else {
            let action_offset = rec_writer
                .buf
                .len()
                .checked_add(2)
                .and_then(|len| u16::try_from(len).ok())
                .ok_or_else(|| {
                    ExportError::Options(format!(
                        "Button '{}' state records occupy {} bytes; DefineButton2 with AVM1 actions supports at most {} bytes before its action block",
                        sym.name,
                        rec_writer.buf.len(),
                        usize::from(u16::MAX) - 2
                    ))
                })?;
            w.u16(action_offset);
            w.bytes(&rec_writer.buf);
            w.bytes(&cond_bytes);
        }
        self.root_tags.write_tag(34, &w.buf);
        Ok(button_id)
    }

    pub(super) fn build_timeline_tags(
        &mut self,
        timeline: &Timeline,
        is_root: bool,
    ) -> Result<(Vec<u8>, u16), ExportError> {
        let total_frames = timeline.frame_count.max(1);
        let layout = compute_swf_timeline_layout(timeline).map_err(|issue| {
            ExportError::Options(format!(
                "SWF timeline layout is not representable: {issue:?}"
            ))
        })?;
        let depth_bands = layout.depth_bands;
        let mask_clip_depths = layout.mask_clip_depths;

        // Pre-allocate morph shapes for compatible shape tween spans
        let mut morph_spans: BTreeMap<(usize, usize), (u16, u32, u32, Easing)> = BTreeMap::new();

        for (l_idx, layer) in timeline.layers.iter().enumerate() {
            if !layer.visible
                || matches!(layer.properties.role, LayerRole::Folder | LayerRole::Guide)
            {
                continue;
            }
            let effective_end = layer
                .properties
                .end_frame
                .unwrap_or(total_frames)
                .min(total_frames);
            for (k_idx, kf) in layer.keyframes.iter().enumerate() {
                if kf.frame >= effective_end {
                    break;
                }
                if let Some(easing) = kf.tween
                    && kf.tween_kind == TweenKind::Shape
                    && let Some(next_kf) = layer.keyframes.get(k_idx + 1)
                    && next_kf.frame < effective_end
                    && can_morph_keys(kf, next_kf)
                {
                    let morph_id = self.alloc_char_id();
                    if let Some(payload) = build_define_morph_shape_payload(
                        morph_id,
                        &kf.elements[0],
                        &next_kf.elements[0],
                    ) {
                        self.root_tags.write_tag(46, &payload);
                        morph_spans
                            .insert((l_idx, k_idx), (morph_id, kf.frame, next_kf.frame, easing));
                        continue;
                    }
                }
                for el in &kf.elements {
                    let _ = self.ensure_timeline_element_character(el, kf.bitmap_fill(&el.id))?;
                }
            }
        }

        let stream_plan = plan_timeline_stream_sounds(
            timeline,
            total_frames,
            self.project.fps.clamp(1, 255),
            &self.stream_sounds,
        );
        // Ensure fallback DefineSound entries exist if overlapping Stream sounds fell back to StartSound.
        for &(l_idx, f_idx) in &stream_plan.fallback_event_frames {
            if let Some(snd) = timeline
                .layers
                .get(l_idx)
                .and_then(|l| l.keyframes.iter().find(|k| k.frame == f_idx))
                .and_then(|k| k.sound.as_ref())
                && !self.sound_ids.contains_key(&snd.resource)
                && let Some(res) = self.project.resources.iter().find(|r| r.id == snd.resource)
                && let Ok(raw) = res.decoded_bytes()
                && let Ok(info) = analyze_swf_sound_resource(&res.media_type, &raw)
                && let Ok(payload) =
                    build_define_sound_payload(self.next_char_id as u16, &res.media_type, &raw)
            {
                let sid = self.alloc_char_id();
                let fixed_payload =
                    build_define_sound_payload(sid, &res.media_type, &raw).unwrap_or(payload);
                self.root_tags.write_tag(14, &fixed_payload);
                let duration_44 = u64::from(info.sample_count).saturating_mul(44_100)
                    / u64::from(info.sample_rate.max(1));
                self.sound_ids.insert(res.id.clone(), sid);
                self.sound_durations_44
                    .insert(res.id.clone(), duration_44.max(1));
            }
        }

        let mut stream_tags = SwfBitWriter::new();
        let mut active_slots: BTreeMap<u16, SlotPlacement> = BTreeMap::new();
        let layer_idx_by_id: HashMap<&str, usize> = timeline
            .layers
            .iter()
            .enumerate()
            .map(|(idx, layer)| (layer.id.as_str(), idx))
            .collect();

        for frame in 0..total_frames {
            let mut next_slots: BTreeMap<u16, SlotPlacement> = BTreeMap::new();
            let mut frame_labels: Vec<(&str, bool)> = Vec::new();
            let mut frame_sounds: Vec<&FrameSound> = Vec::new();
            let mut max_placed_layer_idx: Option<usize> = None;

            for (l_idx, layer) in timeline.layers.iter().enumerate() {
                let effective_end = layer
                    .properties
                    .end_frame
                    .unwrap_or(total_frames)
                    .min(total_frames);
                if frame < effective_end {
                    for kf in &layer.keyframes {
                        if kf.frame == frame {
                            if kf.is_navigable_label() {
                                frame_labels.push((
                                    kf.label.as_str(),
                                    kf.label_type == FrameLabelType::Anchor,
                                ));
                            }
                            if let Some(snd) = &kf.sound
                                && (snd.sync != SoundSync::Stream
                                    || stream_plan.fallback_event_frames.contains(&(l_idx, frame)))
                            {
                                frame_sounds.push(snd);
                            }
                        }
                    }
                }

                if !layer.visible
                    || matches!(layer.properties.role, LayerRole::Folder | LayerRole::Guide)
                    || frame >= effective_end
                {
                    continue;
                }

                let base_depth = depth_bands[l_idx].0;
                let clip_depth = mask_clip_depths.get(&l_idx).copied();

                // Check if frame falls inside a DefineMorphShape span
                let active_k_idx = layer.keyframes.iter().rposition(|k| k.frame <= frame);
                if let Some(k_idx) = active_k_idx
                    && let Some(&(morph_id, s_frame, e_frame, ease)) =
                        morph_spans.get(&(l_idx, k_idx))
                    && frame >= s_frame
                    && frame <= e_frame
                {
                    let span = (e_frame - s_frame).max(1) as f64;
                    let raw_t = f64::from(frame - s_frame) / span;
                    let eased_t = neoflash_core::easing(raw_t, ease).clamp(0.0, 1.0);
                    let ratio = (eased_t * 65535.0).round() as u16;
                    let start_el = &layer.keyframes[k_idx].elements[0];
                    let end_el = layer
                        .keyframes
                        .get(k_idx + 1)
                        .and_then(|nk| nk.elements.first())
                        .unwrap_or(start_el);
                    let interp_alpha = start_el.transform.alpha
                        + (end_el.transform.alpha - start_el.transform.alpha) * eased_t;
                    max_placed_layer_idx =
                        Some(max_placed_layer_idx.map_or(l_idx, |prev| prev.max(l_idx)));
                    next_slots.insert(
                        base_depth,
                        SlotPlacement {
                            character_id: morph_id,
                            matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                            color_transform: start_el.transform.color_transform,
                            alpha: interp_alpha,
                            ratio: Some(ratio),
                            name: String::new(),
                            clip_depth,
                            visible: start_el.visible,
                            cache_as_bitmap: start_el.cache_as_bitmap,
                            blend_mode: start_el.blend_mode,
                            filters: start_el.filters.clone(),
                            clip_actions: None,
                        },
                    );
                    continue;
                }

                // Evaluate layer at `frame`
                let sampled_elements = neoflash_core::sample_layer_in(timeline, layer, frame);
                for (e_offset, el) in sampled_elements.iter().enumerate() {
                    let Some(cid) = self.ensure_timeline_element_character(
                        el,
                        active_k_idx.and_then(|k| layer.keyframes[k].bitmap_fill(&el.id)),
                    )?
                    else {
                        continue;
                    };
                    let depth = base_depth
                        .checked_add(
                            u16::try_from(e_offset)
                                .expect("element offset is checked by SWF depth preflight"),
                        )
                        .expect("display depth is checked by SWF preflight");
                    let is_button_inst =
                        el.effective_symbol_kind(self.project) == Some(SymbolKind::Button);
                    let clip_actions = if !self.options.profile.is_avm2()
                        && !self.options.scriptless_mode
                        && !is_button_inst
                    {
                        encode_clip_actions(self.project, &el.scripts).map_err(|error| {
                            ExportError::Options(format!(
                                "MovieClip instance '{}' cannot encode its AVM1 actions: {error}",
                                el.name
                            ))
                        })?
                    } else {
                        None
                    };
                    max_placed_layer_idx =
                        Some(max_placed_layer_idx.map_or(l_idx, |prev| prev.max(l_idx)));
                    next_slots.insert(
                        depth,
                        SlotPlacement {
                            character_id: cid,
                            matrix: transform_matrix(el.transform),
                            color_transform: el.transform.color_transform,
                            alpha: el.transform.alpha,
                            ratio: None,
                            name: el.name.clone(),
                            clip_depth: if e_offset == 0 { clip_depth } else { None },
                            visible: el.visible,
                            cache_as_bitmap: el.cache_as_bitmap,
                            blend_mode: el.blend_mode,
                            filters: el.filters.clone(),
                            clip_actions,
                        },
                    );
                }
            }

            // Emit RemoveObject2 for depths no longer present
            let removed_depths: Vec<u16> = active_slots
                .keys()
                .copied()
                .filter(|d| !next_slots.contains_key(d))
                .collect();
            for d in removed_depths {
                active_slots.remove(&d);
                stream_tags.write_tag(28, &d.to_le_bytes());
            }

            for &(lbl, is_anchor) in &frame_labels {
                let mut lw = SwfBitWriter::new();
                lw.c_string(lbl);
                if is_anchor && self.options.profile.swf_version() >= 6 {
                    lw.u8(1);
                }
                stream_tags.write_tag(43, &lw.buf);
            }

            // Pre-placement DoAction: scripts owned by a top Actions layer above all placed art
            // on this frame (such as SWF frames where DoAction preceded PlaceObject2).
            if !self.options.profile.is_avm2() && !self.options.scriptless_mode {
                for fs in &timeline.scripts {
                    let is_pre_place = fs
                        .layer
                        .as_deref()
                        .and_then(|lid| layer_idx_by_id.get(lid).copied())
                        .is_some_and(|fs_idx| {
                            max_placed_layer_idx.is_some_and(|max_placed| fs_idx > max_placed)
                        });
                    if is_pre_place
                        && fs.frame == frame
                        && matches!(
                            fs.script.language,
                            ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
                        )
                    {
                        let expanded = expand_swf_includes(
                            self.project,
                            fs.script.language,
                            "",
                            &fs.script.source,
                        )
                        .map_err(|err| {
                            ExportError::Options(format!(
                                "Frame {} AVM1 include failed: {err}",
                                frame + 1
                            ))
                        })?;
                        let actions = compile_avm1_actions(&expanded).map_err(|err| {
                            ExportError::Options(format!(
                                "Frame {} AVM1 script failed to compile: {err}",
                                frame + 1
                            ))
                        })?;
                        if actions.len() > 1 {
                            stream_tags.write_tag(12, &actions);
                        }
                    }
                }
            }

            // Emit PlaceObject2 / PlaceObject3 for new or changed slots
            for (&depth, slot) in &next_slots {
                match active_slots.get(&depth) {
                    None => {
                        write_place_object_tag(&mut stream_tags, depth, slot, None);
                    }
                    Some(prev) if !prev.same_state(slot) => {
                        let is_move = prev.character_id == slot.character_id
                            && prev.name == slot.name
                            && prev.clip_depth == slot.clip_depth
                            && prev.clip_actions == slot.clip_actions
                            && !(prev.ratio.is_some() && slot.ratio.is_none());
                        if is_move {
                            write_place_object_tag(&mut stream_tags, depth, slot, Some(prev));
                        } else {
                            stream_tags.write_tag(28, &depth.to_le_bytes());
                            write_place_object_tag(&mut stream_tags, depth, slot, None);
                        }
                    }
                    Some(_) => {}
                }
            }
            active_slots = next_slots;

            if let Some(head_payload) = stream_plan.head_by_frame.get(&frame) {
                stream_tags.write_tag(45, head_payload); // SoundStreamHead2
            }
            if let Some(block_payload) = stream_plan.block_by_frame.get(&frame) {
                stream_tags.write_tag(19, block_payload); // SoundStreamBlock
            }

            for snd in frame_sounds {
                if let Some(&sid) = self.sound_ids.get(&snd.resource) {
                    let duration_44 = self
                        .sound_durations_44
                        .get(&snd.resource)
                        .copied()
                        .unwrap_or(44_100);
                    let payload = build_start_sound_payload(sid, snd, duration_44);
                    stream_tags.write_tag(15, &payload);
                }
            }

            if !self.options.profile.is_avm2() && !self.options.scriptless_mode {
                for fs in &timeline.scripts {
                    let is_pre_place = fs
                        .layer
                        .as_deref()
                        .and_then(|lid| layer_idx_by_id.get(lid).copied())
                        .is_some_and(|fs_idx| {
                            max_placed_layer_idx.is_some_and(|max_placed| fs_idx > max_placed)
                        });
                    if !is_pre_place
                        && fs.frame == frame
                        && matches!(
                            fs.script.language,
                            ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
                        )
                    {
                        let expanded = expand_swf_includes(
                            self.project,
                            fs.script.language,
                            "",
                            &fs.script.source,
                        )
                        .map_err(|err| {
                            ExportError::Options(format!(
                                "Frame {} AVM1 include failed: {err}",
                                frame + 1
                            ))
                        })?;
                        let actions = compile_avm1_actions(&expanded).map_err(|err| {
                            ExportError::Options(format!(
                                "Frame {} AVM1 script failed to compile: {err}",
                                frame + 1
                            ))
                        })?;
                        if actions.len() > 1 {
                            stream_tags.write_tag(12, &actions);
                        }
                    }
                }
            }

            // Flush any newly defined root dictionary tags before this frame's stream tags + ShowFrame
            if is_root {
                self.root_tags.bytes(&stream_tags.buf);
                stream_tags.buf.clear();
                self.root_tags.write_tag(1, &[]); // ShowFrame
            } else {
                stream_tags.write_tag(1, &[]); // ShowFrame
            }
        }

        let frame_count = u16::try_from(total_frames)
            .map_err(|_| ExportError::Options("SWF timeline exceeds 65535 frames".to_owned()))?;
        if is_root {
            Ok((Vec::new(), frame_count))
        } else {
            stream_tags.write_tag(0, &[]); // End tag inside DefineSprite
            Ok((stream_tags.buf, frame_count))
        }
    }

    pub(super) fn compile_symbol_in_order(
        &mut self,
        sym_id: &str,
        visited: &mut BTreeSet<String>,
    ) -> Result<(), ExportError> {
        if visited.contains(sym_id) {
            return Ok(());
        }
        visited.insert(sym_id.to_owned());
        let Some(sym) = self.project.symbol(sym_id).cloned() else {
            return Ok(());
        };

        // Compile child symbol dependencies first so every DefineSprite / DefineButton2 is at root level before use!
        for layer in &sym.timeline.layers {
            for kf in &layer.keyframes {
                for el in &kf.elements {
                    if let ElementKind::Instance { symbol_id, .. } = &el.kind {
                        self.compile_symbol_in_order(symbol_id, visited)?;
                    }
                }
            }
        }

        let char_id = if sym.kind == SymbolKind::Button {
            let initial_scripts = self.initial_button_scripts_for_symbol(&sym);
            let bid = self.compile_button_symbol(&sym, &initial_scripts)?;
            let key = button_scripts_variant_key(&initial_scripts);
            self.button_variant_ids.insert((sym.id.clone(), key), bid);
            bid
        } else {
            let (sprite_tags, frame_count) = self.build_timeline_tags(&sym.timeline, false)?;
            // Separate placements still create separate runtime MovieClips. A compiled sprite
            // needs its own dictionary identity only when it has an external binding or metadata.
            let shareable = sym.linkage_id.is_none()
                && sym.class_name.is_none()
                && sym.scale9_grid().is_none()
                && sym.timeline.scripts.is_empty();
            let sprite_key = shareable.then(|| (frame_count, crate::sha256_bytes(&sprite_tags)));
            if let Some(existing) = sprite_key.and_then(|key| self.anonymous_sprites.get(&key)) {
                self.symbol_char_ids.insert(sym.id.clone(), *existing);
                return Ok(());
            }
            let sid = self.alloc_char_id();
            if let Some(key) = sprite_key {
                self.anonymous_sprites.insert(key, sid);
            }
            let mut w = SwfBitWriter::new();
            w.u16(sid);
            w.u16(frame_count);
            w.bytes(&sprite_tags);
            self.root_tags.write_tag(39, &w.buf); // DefineSprite
            if sym.kind == SymbolKind::MovieClip
                && let Some([gx, gy, gw, gh]) = sym.scale9_grid()
                && gw > 0.0
                && gh > 0.0
                && [gx, gy, gw, gh]
                    .iter()
                    .all(|v| v.is_finite() && v.abs() <= 1e6)
            {
                let xmin = (gx * 20.0).round() as i32;
                let ymin = (gy * 20.0).round() as i32;
                let xmax = ((gx + gw) * 20.0).round() as i32;
                let ymax = ((gy + gh) * 20.0).round() as i32;
                if xmax > xmin && ymax > ymin {
                    let mut gw_buf = SwfBitWriter::new();
                    gw_buf.u16(sid);
                    gw_buf.rect_twips([xmin, xmax, ymin, ymax]);
                    self.root_tags.write_tag(78, &gw_buf.buf); // DefineScalingGrid
                }
            }
            sid
        };

        self.symbol_char_ids.insert(sym.id.clone(), char_id);

        if !self.options.profile.is_avm2() {
            if let Some(linkage) = &sym.linkage_id
                && !linkage.is_empty()
            {
                self.avm1_exports.push((char_id, linkage.clone()));
            }
        } else if !self.options.scriptless_mode {
            let as3_scripts = collect_expanded_as3_frame_scripts(self.project, &sym.timeline)?;
            let explicit_cls = sym
                .class_name
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    sym.linkage_id
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                });
            if let Some(cls_name) = explicit_cls {
                self.avm2_symbol_classes
                    .push((char_id, cls_name.to_owned()));
                self.avm2_symbol_kinds.insert(cls_name.to_owned(), sym.kind);
                if !as3_scripts.is_empty() {
                    self.avm2_symbol_frame_scripts
                        .insert(cls_name.to_owned(), as3_scripts);
                }
            } else if !as3_scripts.is_empty() {
                self.avm2_anon_symbol_scripts.push((char_id, as3_scripts));
            }
        }
        Ok(())
    }
}
