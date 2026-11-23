use super::*;

impl Context<'_> {
    /// Convert one display record, or keep its slot with an empty placeholder
    /// instance and an explicit warning when it cannot be represented yet.
    pub(super) fn element_or_placeholder(
        &mut self,
        s: &BinaryTimelineStream,
        object_id: u32,
        id: &str,
        depth: usize,
    ) -> Element {
        match self.element(s, object_id, id, depth) {
            Ok(element) => element,
            Err(message) => {
                self.report.warning(
                    "fla_object_placeholder",
                    format!("stream #{} object #{object_id}", s.index),
                    format!("{message}. An empty placeholder keeps its place; the original object remains in the embedded source."),
                );
                if !self.generated.iter().any(|g| g.id == PLACEHOLDER) {
                    self.generated.push(group_symbol(
                        PLACEHOLDER,
                        "Unsupported source object (see import report)".into(),
                        vec![],
                    ));
                }
                Element {
                    id: id.into(),
                    name: String::new(),
                    transform: Transform::default(),
                    kind: ElementKind::Instance {
                        symbol_id: PLACEHOLDER.into(),
                        playback: InstancePlayback::SingleFrame(0),
                    },
                    scripts: vec![],
                    accessibility: None,
                    ..Element::default()
                }
            }
        }
    }
    pub(super) fn element(
        &mut self,
        s: &BinaryTimelineStream,
        object_id: u32,
        id: &str,
        depth: usize,
    ) -> Result<Element, String> {
        if depth > 48 {
            return Err("Nested source group limit exceeded".into());
        }
        let o = object(s, object_id)?;
        let location = format!(
            "stream #{} object #{object_id} @0x{:x}",
            s.index, o.encoded.start
        );
        let mut element = Element {
            id: id.into(),
            name: String::new(),
            transform: Transform::default(),
            kind: ElementKind::Rectangle {
                width: 0.,
                height: 0.,
                fill: [0; 4],
                stroke: [0; 4],
                stroke_width: 0.,
            },
            scripts: vec![],
            accessibility: None,
            ..Element::default()
        };
        match &o.payload {
            Some(TimelinePayload::Instance(i)) => {
                element.filters = convert_filters(&i.filters, &location, self.report);
                element.blend_mode = convert_blend_mode(i.blend_mode, &location, self.report);
                if i.is_visible == Some(0) {
                    element.visible = false;
                }
                let target_supports_avm1_load_script = o.class == TimelineClass::Sprite
                    && i.library_id.is_some_and(|lid| {
                        self.library_kinds.get(&lid) == Some(&SymbolKind::MovieClip)
                    })
                    && i.library_id
                        .and_then(|lid| self.library.get(&lid))
                        .is_some_and(|sid| self.implemented_symbols.contains(sid));
                let (prop_script, prop_lossy) = instance_props::translate_component_properties(
                    &s.original_bytes,
                    i.properties.as_ref(),
                    i.property_count,
                    self.language,
                    target_supports_avm1_load_script,
                );
                if instance_props::has_nonredundant_animation_settings(
                    &i.animation_frame_settings,
                    i.loop_mode,
                    i.first_frame,
                ) || prop_lossy
                {
                    self.report.warning("fla_instance_settings",&location,"Component parameters or per-instance animation settings are not imported; they remain in the embedded source.");
                }
                if let Some(span) = &i.component_xml {
                    let xml = read_string(&s.original_bytes, span)?;
                    if !xml.is_empty() {
                        let document = roxmltree::Document::parse(&xml).ok();
                        if document.as_ref().is_none_or(|document| {
                            let root = document.root_element();
                            root.tag_name().name() != "component"
                                || root.children().any(|n| {
                                    n.is_element()
                                        || n.is_text() && !n.text().unwrap_or("").trim().is_empty()
                                })
                                || root.attributes().any(|a| {
                                    !matches!(
                                        a.name(),
                                        "metaDataFetched"
                                            | "schemaUrl"
                                            | "schemaOperation"
                                            | "sceneRootLabel"
                                            | "oldCopiedComponentPath"
                                    ) || matches!(a.name(), "schemaUrl" | "schemaOperation")
                                        && !a.value().is_empty()
                                })
                        }) {
                            self.report.warning("fla_component_metadata",&location,"Component data-binding metadata is not imported; it remains in the embedded source.");
                        }
                    }
                }
                element.name = i
                    .historical_wide_name
                    .clone()
                    .filter(|s| !s.is_empty())
                    .or_else(|| {
                        optional_text(
                            &s.original_bytes,
                            i.instance_name.as_ref().or(i.legacy_name.as_ref()),
                        )
                        .ok()
                    })
                    .unwrap_or_default();
                element.transform = Transform::from_matrix(shape::matrix(
                    i.matrix_words.ok_or("Missing instance matrix")?,
                ))
                .ok_or("Invalid instance matrix")?;
                if let Some(m) = i.matrix_3d
                    && m != [
                        1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
                    ]
                {
                    self.report.warning("fla_3d_transform",&location,"3D transforms are not rendered; the instance uses its 2D placement matrix.");
                    let m64: Vec<f64> = m.iter().map(|&v| f64::from(v)).collect();
                    if m64.iter().all(|v| v.is_finite()) {
                        let persp = (1.0 / (1.0 + m64[14] / 1000.0)).clamp(0.1, 10.0);
                        let mat2d = [
                            m64[0] * persp,
                            m64[1] * persp,
                            m64[4] * persp,
                            m64[5] * persp,
                            if m64[12] != 0.0 {
                                m64[12] / 20.0
                            } else {
                                element.transform.x
                            },
                            if m64[13] != 0.0 {
                                m64[13] / 20.0
                            } else {
                                element.transform.y
                            },
                        ];
                        if let Some(projected) = Transform::from_matrix(mat2d) {
                            element.transform = projected;
                        }
                    }
                } else if let Some([rx, ry, rz]) = i.rotation_3d
                    && [rx, ry, rz].iter().all(|v| v.is_finite())
                    && (rx.abs() > 1e-6 || ry.abs() > 1e-6 || rz.abs() > 1e-6)
                {
                    self.report.warning("fla_3d_transform",&location,"3D transforms are not rendered; the instance uses its 2D placement matrix.");
                    let sx = element.transform.scale_x * ry.to_radians().cos();
                    let sy = element.transform.scale_y * rx.to_radians().cos();
                    let rot = element.transform.rotation + rz;
                    if sx.is_finite()
                        && sy.is_finite()
                        && rot.is_finite()
                        && sx.abs() <= 1e4
                        && sy.abs() <= 1e4
                        && rot.abs() <= 1e9
                    {
                        element.transform.scale_x = sx;
                        element.transform.scale_y = sy;
                        element.transform.rotation = rot;
                    }
                }
                if let Some(c) = &i.color_transform {
                    let mut value = color_transform(c)?;
                    if value.validate().is_err() {
                        let original = value;
                        for v in &mut value.multipliers {
                            *v = v.clamp(-1e4, 1e4);
                        }
                        for v in &mut value.offsets {
                            *v = v.clamp(-255., 255.);
                        }
                        // ±256 offsets saturate exactly like ±255 for nonnegative multipliers.
                        if original
                            .multipliers
                            .iter()
                            .any(|m| !(0.0..=1e4).contains(m))
                            || original.offsets.iter().any(|o| o.abs() > 256.)
                        {
                            self.report.warning("fla_color_transform",&location,format!("Color effect {:?}/{:?} is outside the renderable range and was clamped.",original.multipliers,original.offsets));
                        }
                    }
                    element.transform.color_transform = value;
                }
                let lib_id = i.library_id.ok_or("Missing library identity")?;
                let symbol_id = self
                    .library
                    .get(&lib_id)
                    .ok_or_else(|| format!("{location}: unresolved library reference"))?
                    .clone();
                let inst_kind = match o.class {
                    TimelineClass::Sprite => SymbolKind::MovieClip,
                    TimelineClass::Button => SymbolKind::Button,
                    _ => SymbolKind::Graphic,
                };
                if let Some(&lib_kind) = self.library_kinds.get(&lib_id)
                    && inst_kind != lib_kind
                {
                    element.instance_behavior = Some(inst_kind);
                }
                let first = u32::from(i.first_frame.unwrap_or(0));
                let playback =
                    if o.class == TimelineClass::Sprite || o.class == TimelineClass::Button {
                        InstancePlayback::Independent
                    } else {
                        match i.loop_mode.unwrap_or(0) {
                            0 if first == 0 => InstancePlayback::Loop,
                            0 => InstancePlayback::LoopFrom(first),
                            1 if first == 0 => InstancePlayback::PlayOnce,
                            1 => InstancePlayback::PlayOnceFrom(first),
                            2 => InstancePlayback::SingleFrame(first),
                            other => {
                                self.report.warning(
                                    "fla_graphic_playback",
                                    &location,
                                    format!(
                                        "Unknown graphic playback mode {other}; the graphic loops."
                                    ),
                                );
                                InstancePlayback::LoopFrom(first)
                            }
                        }
                    };
                element.kind = ElementKind::Instance {
                    symbol_id,
                    playback,
                };
                if let Some(ps) = prop_script {
                    element.scripts.push(ps);
                }
                if let Some(script) = self.actions(s, i.actions.as_ref())? {
                    if self.language == ScriptLanguage::ActionScript3
                        || o.class == TimelineClass::Symbol
                    {
                        self.report.warning(
                            "fla_actions_partial",
                            &location,
                            "Object-attached instance scripts are not supported on ActionScript 3 or Graphic instances and remain in the embedded source.",
                        );
                    } else {
                        element.scripts.push(ObjectScript {
                            event: ObjectEvent::Load,
                            script,
                        });
                    }
                }
                if let Some(render_mode) = i.bitmap_render_mode {
                    element.cache_as_bitmap = render_mode == 1;
                }
                let acc_role = if o.class == TimelineClass::Button {
                    AccessibleRole::Button
                } else {
                    AccessibleRole::Image
                };
                element.accessibility =
                    convert_accessibility(s, i.accessibility.as_ref(), &element.name, acc_role)?;
            }
            Some(TimelinePayload::Text(t)) => {
                element = convert_timeline_text(s, t, id, "", self.contents_revision, self.report)?;
            }
            Some(TimelinePayload::Bitmap(b)) => {
                let raw_mat = shape::matrix(b.matrix_words.ok_or("Missing bitmap matrix")?);
                if let Some(media) = b.media_id {
                    if let Some(asset) = self.bitmaps.get(&media) {
                        let mut scaled_mat = raw_mat;
                        let ox_shift = raw_mat[0] * asset.origin_x + raw_mat[2] * asset.origin_y;
                        let oy_shift = raw_mat[1] * asset.origin_x + raw_mat[3] * asset.origin_y;
                        if ox_shift.is_finite() && oy_shift.is_finite() {
                            scaled_mat[4] += ox_shift;
                            scaled_mat[5] += oy_shift;
                        }
                        let sx = asset.scale_x * asset.bounds_scale_x;
                        let sy = asset.scale_y * asset.bounds_scale_y;
                        scaled_mat[0] *= sx;
                        scaled_mat[1] *= sx;
                        scaled_mat[2] *= sy;
                        scaled_mat[3] *= sy;
                        element.transform = Transform::from_matrix(scaled_mat)
                            .or_else(|| Transform::from_matrix(raw_mat))
                            .ok_or("Invalid bitmap matrix")?;
                        element.kind = ElementKind::Bitmap {
                            resource_id: asset.resource_id.clone(),
                            width: asset.width as u32,
                            height: asset.height as u32,
                            smoothing: asset.smoothing,
                        };
                    } else {
                        element.transform =
                            Transform::from_matrix(raw_mat).ok_or("Invalid bitmap matrix")?;
                        let symbol_id = if let Some(id) = self.missing_bitmaps.get(&media) {
                            id.clone()
                        } else {
                            let id = format!("fla-missing-bitmap-{media}");
                            self.generated.push(group_symbol(
                                &id,
                                format!("Unresolved bitmap #{media}"),
                                vec![],
                            ));
                            self.missing_bitmaps.insert(media, id.clone());
                            id
                        };
                        element.kind = ElementKind::Instance {
                            symbol_id,
                            playback: InstancePlayback::SingleFrame(0),
                        };
                        element.name = format!("Unresolved source bitmap #{media}");
                        if self.skipped_bitmaps.contains(&media) {
                            self.report.warning(
                                "fla_bitmap_skipped",
                                &location,
                                format!(
                                    "Bitmap #{media} could not be decoded or exceeded project resource limits; an unresolved placeholder preserves its stage transform."
                                ),
                            );
                        } else {
                            self.report.warning("fla_source_bitmap_missing",&location,format!("Bitmap #{media} is already absent from the source archive. The placement and unresolved reference are retained; no image was invented."));
                        }
                    }
                } else if let Some(span) = b.inline_raster
                    && let Some(raw) = s.original_bytes.get(span.start..span.end)
                    && let Ok(mut decoded) = crate::legacy::media::decode_binary_bitmap(raw, 0)
                {
                    let (orig_w, orig_h) = (decoded.width, decoded.height);
                    if decoded.width > MAX_BITMAP_DIMENSION || decoded.height > MAX_BITMAP_DIMENSION
                    {
                        let Some((nw, nh, resized)) =
                            crate::legacy::media::resize_rgba_proportional(
                                &decoded.rgba,
                                decoded.width,
                                decoded.height,
                                MAX_BITMAP_DIMENSION,
                            )
                        else {
                            return Err("Inline bitmap exceeds 4096 px per-axis limit".into());
                        };
                        decoded.width = nw;
                        decoded.height = nh;
                        decoded.rgba = resized;
                        self.report.warning(
                            "fla_bitmap_downscaled",
                            &location,
                            format!(
                                "Inline bitmap was downscaled from {orig_w}x{orig_h} px to {nw}x{nh} px (preserving Stage dimensions via transform scale) to fit project bitmap limits."
                            ),
                        );
                    }
                    let Ok(png) = decoded.png_bytes() else {
                        return Err("Inline bitmap PNG encoding failed".into());
                    };
                    if png.len() > MAX_RESOURCE_BYTES {
                        return Err("Inline bitmap resource exceeded limits".into());
                    }
                    let [xmin, xmax, ymin, ymax] = decoded.bounds_words;
                    let mut scaled_mat = raw_mat;
                    let mut sx = f64::from(orig_w) / f64::from(decoded.width);
                    let mut sy = f64::from(orig_h) / f64::from(decoded.height);
                    if xmin != i32::MIN
                        && i64::from(xmax) > i64::from(xmin)
                        && i64::from(ymax) > i64::from(ymin)
                    {
                        let ox = f64::from(xmin) / 20.0;
                        let oy = f64::from(ymin) / 20.0;
                        let bw = (f64::from(xmax) - f64::from(xmin)) / 20.0;
                        let bh = (f64::from(ymax) - f64::from(ymin)) / 20.0;
                        let ox_shift = raw_mat[0] * ox + raw_mat[2] * oy;
                        let oy_shift = raw_mat[1] * ox + raw_mat[3] * oy;
                        if ox_shift.is_finite() && oy_shift.is_finite() {
                            scaled_mat[4] += ox_shift;
                            scaled_mat[5] += oy_shift;
                        }
                        sx *= (bw / f64::from(orig_w)).clamp(1e-4, 1e4);
                        sy *= (bh / f64::from(orig_h)).clamp(1e-4, 1e4);
                    }
                    scaled_mat[0] *= sx;
                    scaled_mat[1] *= sx;
                    scaled_mat[2] *= sy;
                    scaled_mat[3] *= sy;
                    element.transform = Transform::from_matrix(scaled_mat)
                        .or_else(|| Transform::from_matrix(raw_mat))
                        .ok_or("Invalid bitmap matrix")?;
                    let res_id = format!("legacy-inline-bitmap-{}-{object_id}", s.index);
                    let res_path = format!("legacy/inline-bitmaps/{}-{object_id}.png", s.index);
                    if let Ok(unit) = ResourceUnit::from_bytes(&res_id, res_path, "image/png", &png)
                    {
                        self.inline_resources.push(unit);
                        element.kind = ElementKind::Bitmap {
                            resource_id: res_id,
                            width: decoded.width,
                            height: decoded.height,
                            smoothing: false,
                        };
                    } else {
                        return Err("Inline bitmap resource exceeded limits".into());
                    }
                } else {
                    return Err("Missing bitmap identity".into());
                }
            }
            Some(TimelinePayload::Shape) => {
                if !o.complete && o.children.is_empty() && o.shape_index.is_none() {
                    return Err("Shape record stopped before completion".into());
                }
                let mut members = Vec::new();
                let mut bitmap_fills = Vec::new();
                let mut appearances = Vec::new();
                let mut vector_effects = Vec::new();
                for (slot, child) in o.children.iter().enumerate() {
                    members.push(self.element_or_placeholder(
                        s,
                        *child,
                        &format!("{id}-member-{slot}"),
                        depth + 1,
                    ))
                }
                if let Some(index) = o.shape_index {
                    let paint_id = format!("{id}-paint");
                    let shape_record = &s.shapes[index].record;
                    match shape::convert_shape_with_effects(
                        shape_record,
                        &paint_id,
                        &self.bitmaps,
                        self.report,
                        &location,
                    ) {
                        Ok(converted) => {
                            members.extend(converted.elements);
                            bitmap_fills.extend(converted.bitmap_fills);
                            appearances.extend(converted.appearances);
                            vector_effects.extend(converted.vector_effects);
                        }
                        Err(e) => {
                            self.report.warning(
                                "fla_shape_skipped",
                                &location,
                                format!(
                                    "Group artwork was not converted: {e}. A placeholder keeps its slot and the original shape remains in the embedded source."
                                ),
                            );
                            if !shape_record.edges.is_empty() {
                                members
                                    .push(self.placeholder_element(paint_id, Transform::default()));
                            }
                        }
                    }
                }
                // A one-frame graphic keeps group membership and vector paint order editable.
                let symbol_id = format!("{id}-group");
                self.generated.push(group_symbol_with_effects(
                    &symbol_id,
                    format!("Source group {object_id}"),
                    members,
                    bitmap_fills,
                    appearances,
                    vector_effects,
                ));
                element.kind = ElementKind::Instance {
                    symbol_id,
                    playback: InstancePlayback::SingleFrame(0),
                };
            }
            Some(TimelinePayload::Primitive(prim)) => {
                if o.children.is_empty()
                    && let Some(index) = o.shape_index
                    && let Some(shape_rec) = s.shapes.get(index)
                    && let Some((transform, _origin, kind)) =
                        shape::try_convert_primitive(o.class, prim, &shape_rec.record)
                {
                    element.transform = transform;
                    element.kind = kind;
                } else {
                    let mut members = Vec::new();
                    let mut bitmap_fills = Vec::new();
                    let mut appearances = Vec::new();
                    let mut vector_effects = Vec::new();
                    for (slot, child) in o.children.iter().enumerate() {
                        members.push(self.element_or_placeholder(
                            s,
                            *child,
                            &format!("{id}-member-{slot}"),
                            depth + 1,
                        ));
                    }
                    if let Some(index) = o.shape_index
                        && let Some(shape_rec) = s.shapes.get(index)
                    {
                        let paint_id = format!("{id}-paint");
                        match shape::convert_shape_with_effects(
                            &shape_rec.record,
                            &paint_id,
                            &self.bitmaps,
                            self.report,
                            &location,
                        ) {
                            Ok(converted) => {
                                members.extend(converted.elements);
                                bitmap_fills.extend(converted.bitmap_fills);
                                appearances.extend(converted.appearances);
                                vector_effects.extend(converted.vector_effects);
                            }
                            Err(e) => {
                                self.report.warning(
                                    "fla_shape_skipped",
                                    &location,
                                    format!(
                                        "Primitive artwork was not converted: {e}. A placeholder keeps its slot and the original shape remains in the embedded source."
                                    ),
                                );
                                if !shape_rec.record.edges.is_empty() {
                                    members.push(
                                        self.placeholder_element(paint_id, Transform::default()),
                                    );
                                }
                            }
                        }
                    }
                    let symbol_id = format!("{id}-group");
                    self.generated.push(group_symbol_with_effects(
                        &symbol_id,
                        format!("Source primitive {object_id}"),
                        members,
                        bitmap_fills,
                        appearances,
                        vector_effects,
                    ));
                    element.kind = ElementKind::Instance {
                        symbol_id,
                        playback: InstancePlayback::SingleFrame(0),
                    };
                }
            }
            Some(TimelinePayload::Video(v)) => {
                element.name = optional_text(&s.original_bytes, v.instance_name.as_ref())?;
                let matrix = shape::matrix(v.matrix_words.ok_or("Missing video matrix")?);
                element.transform = Transform::from_matrix(matrix).ok_or("Invalid video matrix")?;
                let asset = v.media_id.and_then(|mid| self.videos.get(&mid));
                let mut width = asset.map_or(160.0, |a| a.width);
                let mut height = asset.map_or(120.0, |a| a.height);
                if let Some([left, right, top, bottom]) = v.bounds_words
                    && left != i32::MIN
                {
                    let x = f64::from(left) / 20.0;
                    let y = f64::from(top) / 20.0;
                    let w = (f64::from(right) - f64::from(left)) / 20.0;
                    let h = (f64::from(bottom) - f64::from(top)) / 20.0;
                    let tx = element.transform.x + matrix[0] * x + matrix[2] * y;
                    let ty = element.transform.y + matrix[1] * x + matrix[3] * y;
                    if tx.is_finite() && ty.is_finite() && tx.abs() <= 1e12 && ty.abs() <= 1e12 {
                        element.transform.x = tx;
                        element.transform.y = ty;
                    }
                    if w.is_finite() && w > 0.0 {
                        width = w.clamp(1.0, 8192.0);
                    }
                    if h.is_finite() && h > 0.0 {
                        height = h.clamp(1.0, 8192.0);
                    }
                }
                element.kind = ElementKind::Video { width, height };
            }
            Some(TimelinePayload::CompiledClip(c)) => {
                element.name = optional_text(&s.original_bytes, c.instance_name.as_ref())?;
                element.transform = Transform::from_matrix(shape::matrix(
                    c.matrix_words.ok_or("Missing compiled clip matrix")?,
                ))
                .ok_or("Invalid compiled clip matrix")?;
                let cc_supports_avm1_props = c
                    .media_id
                    .and_then(|mid| self.compiled_clips.get(&mid))
                    .is_some_and(|cc| cc.has_avm1_scripts);
                let (prop_script, prop_lossy) = instance_props::translate_component_properties(
                    &s.original_bytes,
                    c.properties.as_ref(),
                    c.property_count,
                    self.language,
                    cc_supports_avm1_props,
                );
                if prop_lossy {
                    self.report.warning(
                        "fla_instance_settings",
                        &location,
                        "Component parameters or per-instance animation settings are not imported; they remain in the embedded source.",
                    );
                }
                if let Some(ps) = prop_script {
                    element.scripts.push(ps);
                }
                if let Some(script) = self.actions(s, c.actions.as_ref())? {
                    if self.language == ScriptLanguage::ActionScript3 {
                        self.report.warning(
                            "fla_actions_partial",
                            &location,
                            "Object-attached scripts on compiled clips are not supported in ActionScript 3 documents and remain in the embedded source.",
                        );
                    } else {
                        element.scripts.push(ObjectScript {
                            event: ObjectEvent::Load,
                            script,
                        });
                    }
                }
                element.accessibility = convert_accessibility(
                    s,
                    c.accessibility.as_ref(),
                    &element.name,
                    AccessibleRole::Button,
                )?;
                let symbol_id = if let Some(media) = c.media_id {
                    let sym_id = format!("fla-compiled-clip-{media}");
                    if !self.generated.iter().any(|g| g.id == sym_id) {
                        if let Some(rec) = self
                            .compiled_clips
                            .get_mut(&media)
                            .and_then(|cc| cc.reconstructed.take())
                        {
                            self.generated.extend(rec.helper_symbols);
                            self.generated.push(rec.main_symbol);
                            self.compiled_clip_sources.extend(rec.sources);
                        } else {
                            let members = if let Some(cc_asset) = self.compiled_clips.get(&media) {
                                if let Some(icon) = &cc_asset.icon_bitmap {
                                    let mut tf = Transform::default();
                                    if icon.width > 0.0 && icon.height > 0.0 {
                                        tf.scale_x =
                                            (cc_asset.width / icon.width).clamp(0.01, 1000.0);
                                        tf.scale_y =
                                            (cc_asset.height / icon.height).clamp(0.01, 1000.0);
                                    }
                                    vec![Element {
                                        id: format!("{sym_id}-el"),
                                        name: String::new(),
                                        transform: tf,
                                        kind: ElementKind::Bitmap {
                                            resource_id: icon.resource_id.clone(),
                                            width: icon.width as u32,
                                            height: icon.height as u32,
                                            smoothing: icon.smoothing,
                                        },
                                        ..Element::default()
                                    }]
                                } else {
                                    vec![Element {
                                        id: format!("{sym_id}-el"),
                                        name: String::new(),
                                        transform: Transform::default(),
                                        kind: ElementKind::Rectangle {
                                            width: cc_asset.width,
                                            height: cc_asset.height,
                                            fill: [180, 180, 180, 64],
                                            stroke: [120, 120, 120, 160],
                                            stroke_width: 1.0,
                                        },
                                        ..Element::default()
                                    }]
                                }
                            } else {
                                self.report.warning(
                                    "fla_media_not_imported",
                                    &location,
                                    format!(
                                        "Compiled clip placement references unresolved media #{media}; a placeholder symbol keeps its placement."
                                    ),
                                );
                                vec![]
                            };
                            let mut sym =
                                group_symbol(&sym_id, format!("Compiled clip #{media}"), members);
                            sym.kind = SymbolKind::MovieClip;
                            self.generated.push(sym);
                        }
                    }
                    sym_id
                } else {
                    self.report.warning(
                        "fla_media_not_imported",
                        &location,
                        "Compiled clip placement has no media identity; a placeholder symbol keeps its placement.",
                    );
                    if !self.generated.iter().any(|g| g.id == PLACEHOLDER) {
                        self.generated.push(group_symbol(
                            PLACEHOLDER,
                            "Unsupported source object (see import report)".into(),
                            vec![],
                        ));
                    }
                    PLACEHOLDER.to_owned()
                };
                element.kind = ElementKind::Instance {
                    symbol_id,
                    playback: InstancePlayback::Independent,
                };
            }
            None if matches!(
                o.class,
                TimelineClass::Shape | TimelineClass::Rect | TimelineClass::Oval
            ) && (!o.children.is_empty() || o.shape_index.is_some()) =>
            {
                self.report.warning(
                    "fla_group_partial",
                    &location,
                    "Group or primitive record stopped before completion; decoded child records and artwork before the stop are preserved.",
                );
                let mut members = Vec::new();
                let mut bitmap_fills = Vec::new();
                let mut appearances = Vec::new();
                let mut vector_effects = Vec::new();
                for (slot, child) in o.children.iter().enumerate() {
                    members.push(self.element_or_placeholder(
                        s,
                        *child,
                        &format!("{id}-member-{slot}"),
                        depth + 1,
                    ));
                }
                if let Some(index) = o.shape_index
                    && let Some(shape_rec) = s.shapes.get(index)
                {
                    let paint_id = format!("{id}-paint");
                    match shape::convert_shape_with_effects(
                        &shape_rec.record,
                        &paint_id,
                        &self.bitmaps,
                        self.report,
                        &location,
                    ) {
                        Ok(converted) => {
                            members.extend(converted.elements);
                            bitmap_fills.extend(converted.bitmap_fills);
                            appearances.extend(converted.appearances);
                            vector_effects.extend(converted.vector_effects);
                        }
                        Err(e) => {
                            self.report.warning(
                                "fla_shape_skipped",
                                &location,
                                format!(
                                    "Group artwork was not converted: {e}. A placeholder keeps its slot and the original shape remains in the embedded source."
                                ),
                            );
                            if !shape_rec.record.edges.is_empty() {
                                members
                                    .push(self.placeholder_element(paint_id, Transform::default()));
                            }
                        }
                    }
                }
                let symbol_id = format!("{id}-group");
                self.generated.push(group_symbol_with_effects(
                    &symbol_id,
                    format!("Source group {object_id}"),
                    members,
                    bitmap_fills,
                    appearances,
                    vector_effects,
                ));
                element.kind = ElementKind::Instance {
                    symbol_id,
                    playback: InstancePlayback::SingleFrame(0),
                };
            }
            Some(TimelinePayload::Auxiliary(aux)) => {
                let tf = aux
                    .matrix_words
                    .and_then(|m| Transform::from_matrix(shape::matrix(m)))
                    .unwrap_or_default();
                if o.class == TimelineClass::TlfText {
                    self.report.warning(
                        "fla_text_approximated",
                        &location,
                        "TLF text record is retained in the embedded source; an empty placeholder preserves its placement.",
                    );
                } else {
                    self.report.warning(
                        "fla_object_placeholder",
                        &location,
                        format!(
                            "Auxiliary {:?} record is retained in the embedded source; an empty placeholder keeps its place.",
                            o.class
                        ),
                    );
                }
                element = self.placeholder_element(id.into(), tf);
            }
            _ => {
                return Err(format!(
                    "{location}: unsupported display record {:?}",
                    o.class
                ));
            }
        }
        if !matches!(
            &o.payload,
            Some(TimelinePayload::Instance(i)) if i.bitmap_render_mode.is_some()
        ) && o
            .base
            .as_ref()
            .is_some_and(|b| b.cache_as_bitmap.unwrap_or(0) != 0)
        {
            element.cache_as_bitmap = true;
        }
        self.provenance.push(
            serde_json::json!({"stream":s.index,"object":object_id,"target":id,"source":o.encoded}),
        );
        Ok(element)
    }
}
/// Graphic first-frame indices beyond the referenced symbol's length: Flash
/// wraps looping graphics and shows the last frame otherwise.
pub(super) fn normalize_playback(symbols: &mut [Symbol], report: &mut ImportReport) {
    let lengths: BTreeMap<_, _> = symbols
        .iter()
        .map(|s| (s.id.clone(), s.timeline.frame_count.max(1)))
        .collect();
    for symbol in symbols.iter_mut() {
        for element in symbol
            .timeline
            .layers
            .iter_mut()
            .flat_map(|l| &mut l.keyframes)
            .flat_map(|k| &mut k.elements)
        {
            let ElementKind::Instance {
                symbol_id,
                playback,
            } = &mut element.kind
            else {
                continue;
            };
            let Some(&len) = lengths.get(symbol_id) else {
                continue;
            };
            if len == 1 {
                *playback = match *playback {
                    InstancePlayback::SingleFrame(_) => InstancePlayback::SingleFrame(0),
                    InstancePlayback::PlayOnceFrom(_) => InstancePlayback::PlayOnce,
                    InstancePlayback::LoopFrom(_) => InstancePlayback::Loop,
                    other => other,
                };
                continue;
            }
            if let InstancePlayback::LoopFrom(f) = *playback
                && f >= len
            {
                // Looping graphic instances advance as `(first_frame + elapsed) % len`
                // in Flash (including Sync classic tweens where cumulative `first_frame`
                // exceeds `len`), so wrapping modulo `len` is exact and lossless.
                *playback = if f % len == 0 {
                    InstancePlayback::Loop
                } else {
                    InstancePlayback::LoopFrom(f % len)
                };
                continue;
            }
            let fixed = match *playback {
                InstancePlayback::SingleFrame(f) if f >= len => {
                    InstancePlayback::SingleFrame(len - 1)
                }
                InstancePlayback::PlayOnceFrom(f) if f >= len => {
                    InstancePlayback::PlayOnceFrom(len - 1)
                }
                _ => continue,
            };
            report.warning("fla_graphic_first_frame",&element.id,format!("The graphic's first frame lies beyond the symbol's {len} frames; it is mapped to {fixed:?}."));
            *playback = fixed;
        }
    }
}
pub(super) fn stream_has_executable_actions(stream: &BinaryTimelineStream) -> bool {
    stream.objects.iter().any(|o| match &o.payload {
        Some(TimelinePayload::Frame(f)) => f
            .actions
            .as_ref()
            .is_some_and(|a| a.presence != TimelineActionPresence::Empty),
        Some(TimelinePayload::Instance(i)) => i
            .actions
            .as_ref()
            .is_some_and(|a| a.presence != TimelineActionPresence::Empty),
        Some(TimelinePayload::CompiledClip(cc)) => cc
            .actions
            .as_ref()
            .is_some_and(|a| a.presence != TimelineActionPresence::Empty),
        _ => false,
    })
}
pub(super) fn resolve_include_logical_path(parent: &str, requested: &str) -> Option<String> {
    if requested.is_empty() || requested.starts_with('/') || requested.contains(['\\', ':', '\0']) {
        return None;
    }
    let mut parts: Vec<_> = parent
        .rsplit_once('/')
        .map_or("", |(base, _)| base)
        .split('/')
        .filter(|p| !p.is_empty())
        .collect();
    for part in requested.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            value => parts.push(value),
        }
    }
    let result = parts.join("/");
    let lower = result.to_ascii_lowercase();
    (lower.ends_with(".as") || lower.ends_with(".asi")).then_some(result)
}
pub(super) fn scan_include_directives(source: &str, parent_logical: &str, out: &mut Vec<String>) {
    for line in source.split(['\r', '\n']) {
        let trimmed = line.trim();
        let raw_arg = if let Some(rest) = trimmed.strip_prefix("#include")
            && (rest.is_empty()
                || rest.starts_with(|c: char| c.is_whitespace() || c == '"' || c == '\''))
        {
            Some(rest.trim())
        } else if let Some(rest) = trimmed.strip_prefix("include")
            && rest.starts_with(|c: char| c.is_whitespace() || c == '"' || c == '\'')
        {
            Some(rest.trim())
        } else {
            None
        };
        let Some(raw_arg) = raw_arg else {
            continue;
        };
        let no_comment = raw_arg.split("//").next().unwrap_or(raw_arg).trim();
        let argument = no_comment.strip_suffix(';').map_or(no_comment, str::trim);
        let Some(requested) = argument
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| {
                argument
                    .strip_prefix('\'')
                    .and_then(|v| v.strip_suffix('\''))
            })
            .filter(|v| !v.contains(['"', '\'', '\\']))
        else {
            continue;
        };
        if let Some(logical) = resolve_include_logical_path(parent_logical, requested) {
            out.push(logical);
        }
    }
}
pub(super) const MAX_COMPANION_INCLUDE_FILE_BYTES: usize = 1 << 20;
pub(super) const MAX_COMPANION_INCLUDE_TOTAL_BYTES: usize = 4 << 20;

pub(super) fn resolve_safe_companion_include_path(
    canon_base: &std::path::Path,
    logical: &str,
) -> Option<std::path::PathBuf> {
    let segments: Vec<&str> = logical.split('/').collect();
    if segments.is_empty() {
        return None;
    }
    let mut cur = canon_base.to_path_buf();
    for (idx, seg) in segments.iter().enumerate() {
        if seg.is_empty() || matches!(*seg, "." | "..") || seg.contains(['/', '\\', ':', '\0']) {
            return None;
        }
        let exact = cur.join(seg);
        let next_path = if std::fs::symlink_metadata(&exact).is_ok() {
            exact
        } else {
            let entry = std::fs::read_dir(&cur).ok()?.flatten().find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(seg)
            })?;
            entry.path()
        };
        let meta = std::fs::symlink_metadata(&next_path).ok()?;
        if meta.file_type().is_symlink() {
            return None;
        }
        let is_last = idx + 1 == segments.len();
        if is_last {
            if !meta.file_type().is_file() || meta.len() > MAX_COMPANION_INCLUDE_FILE_BYTES as u64 {
                return None;
            }
            let valid_ext = next_path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("as") || e.eq_ignore_ascii_case("asi"));
            if !valid_ext {
                return None;
            }
        } else if !meta.file_type().is_dir() {
            return None;
        }
        cur = next_path;
    }
    let canon = std::fs::canonicalize(&cur).ok()?;
    if !canon.starts_with(canon_base) || canon == *canon_base {
        return None;
    }
    let valid_canon_ext = canon
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("as") || e.eq_ignore_ascii_case("asi"));
    valid_canon_ext.then_some(canon)
}

pub(super) fn attach_companion_includes(
    source_path: &str,
    symbols: &[Symbol],
    resources: &mut Vec<ResourceUnit>,
) {
    let Some(base_dir) = std::path::Path::new(source_path)
        .parent()
        .filter(|p| !p.as_os_str().is_empty() && p.is_dir())
    else {
        return;
    };
    let Ok(canon_base) = std::fs::canonicalize(base_dir) else {
        return;
    };
    if !canon_base.is_dir() {
        return;
    }
    let mut queue = Vec::new();
    for symbol in symbols {
        for frame in &symbol.timeline.scripts {
            scan_include_directives(&frame.script.source, "", &mut queue);
        }
        for layer in &symbol.timeline.layers {
            for key in &layer.keyframes {
                for element in &key.elements {
                    for handler in &element.scripts {
                        scan_include_directives(&handler.script.source, "", &mut queue);
                    }
                }
            }
        }
    }
    if queue.is_empty() {
        return;
    }
    let mut seen_lower = std::collections::BTreeSet::new();
    for existing in resources.iter() {
        seen_lower.insert(existing.path.to_ascii_lowercase());
    }
    let mut used_bytes: usize = resources.iter().map(ResourceUnit::byte_len).sum();
    let mut companion_bytes = 0usize;
    let mut include_idx = 0usize;
    let mut cursor = 0usize;
    while cursor < queue.len() && include_idx < 64 {
        let logical = queue[cursor].clone();
        cursor += 1;
        let lower = logical.to_ascii_lowercase();
        if !seen_lower.insert(lower) || resources.len() >= MAX_RESOURCES {
            continue;
        }
        let Some(safe_path) = resolve_safe_companion_include_path(&canon_base, &logical) else {
            continue;
        };
        if std::fs::symlink_metadata(&safe_path)
            .ok()
            .is_none_or(|m| m.file_type().is_symlink() || !m.file_type().is_file())
        {
            continue;
        }
        let Ok(raw) = std::fs::read(&safe_path) else {
            continue;
        };
        if raw.len() > MAX_COMPANION_INCLUDE_FILE_BYTES {
            continue;
        }
        let without_bom = raw.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&raw);
        let decoded = match std::str::from_utf8(without_bom) {
            Ok(s) => s.to_owned(),
            Err(_) => encoding_rs::WINDOWS_1252
                .decode_without_bom_handling(without_bom)
                .0
                .into_owned(),
        };
        let normalized = decoded.replace("\r\n", "\n").replace('\r', "\n");
        let bytes = normalized.as_bytes();
        if bytes.len() > MAX_COMPANION_INCLUDE_FILE_BYTES
            || companion_bytes.saturating_add(bytes.len()) > MAX_COMPANION_INCLUDE_TOTAL_BYTES
            || used_bytes.saturating_add(bytes.len()) > MAX_RESOURCE_TOTAL_BYTES
        {
            continue;
        }
        include_idx += 1;
        if let Ok(unit) = ResourceUnit::from_bytes(
            format!("fla-include-{include_idx}"),
            logical.clone(),
            "text/x-actionscript",
            bytes,
        ) {
            used_bytes = used_bytes.saturating_add(bytes.len());
            companion_bytes = companion_bytes.saturating_add(bytes.len());
            resources.push(unit);
            scan_include_directives(&normalized, &logical, &mut queue);
        }
    }
}
