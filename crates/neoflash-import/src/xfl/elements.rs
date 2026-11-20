use super::*;

impl Parser<'_> {
    pub(super) fn element_with_depth(
        &mut self,
        node: Node<'_, '_>,
        base_id: Id,
        frame: u32,
        depth: usize,
        enclosing_kind: SymbolKind,
        file: &str,
    ) -> Vec<Element> {
        let tag = node.tag_name().name();
        if tag == "DOMShape" {
            return self.shape(node, base_id, file).unwrap_or_default();
        }
        let location = self.location(node, file);
        if tag == "DOMGroup" {
            self.audit(
                node,
                file,
                &["name", "selected", "locked"],
                &["matrix", "transformationPoint", "members"],
            );
            let _ = self.transform(node, file);
            if depth >= 32 {
                self.report.error(
                    "xfl_group_depth",
                    &location,
                    "Nested DOMGroup depth exceeds 32 levels.",
                );
                return vec![];
            }
            let mut grouped = Vec::new();
            if let Some(members) = child(node, "members") {
                self.audit(
                    members,
                    file,
                    &[],
                    &[
                        "DOMSymbolInstance",
                        "DOMBitmapInstance",
                        "DOMStaticText",
                        "DOMDynamicText",
                        "DOMInputText",
                        "DOMTLFText",
                        "DOMGroup",
                        "DOMVideoInstance",
                        "DOMCompiledClipInstance",
                        "DOMComponentInstance",
                        "DOMRectangleObject",
                        "DOMOvalObject",
                        "DOMShape",
                    ],
                );
                for (mi, member) in elements(members).enumerate() {
                    let member_id = format!("{base_id}-g{mi}");
                    grouped.extend(self.element_with_depth(
                        member,
                        member_id,
                        frame,
                        depth + 1,
                        enclosing_kind,
                        file,
                    ));
                }
            }
            return grouped;
        }
        let mut id = base_id;
        // A later authored instance span never inherits an earlier
        // XML slot's state unless explicitly connected by a classic tween.
        if matches!(
            tag,
            "DOMSymbolInstance" | "DOMCompiledClipInstance" | "DOMComponentInstance"
        ) && frame != 0
        {
            id.push_str(&format!("-placement-{frame}"));
        }
        let mut transform = self.transform(node, file);
        let mut name: String = node.attribute("name").unwrap_or(tag).into();
        let mut scripts = Vec::new();
        let mut visible = true;
        let mut cache_as_bitmap = false;
        let mut blend_mode = BlendMode::Normal;
        let mut accessibility = None;
        let mut filters = Vec::new();
        let mut instance_behavior = None;
        let kind = match tag {
            "DOMSymbolInstance" => {
                self.audit(
                    node,
                    file,
                    &[
                        "name",
                        "libraryItemName",
                        "symbolType",
                        "firstFrame",
                        "loop",
                        "trackAsMenu",
                        "selected",
                        "locked",
                        "referenceID",
                        "uniqueIdentifier",
                        "centerPoint3DX",
                        "centerPoint3DY",
                        "blendMode",
                        "cacheAsBitmap",
                        "exportAsBitmap",
                        "isVisible",
                        "bits32",
                        "matrix3D",
                        "rotationX",
                        "rotationY",
                        "rotationZ",
                        "hasAccessibleData",
                        "accName",
                        "description",
                        "shortcut",
                        "tabIndex",
                        "silent",
                        "forceSimple",
                    ],
                    &[
                        "matrix",
                        "transformationPoint",
                        "color",
                        "filters",
                        "Actionscript",
                        "MatteColor",
                        "persistentData",
                    ],
                );
                if node.attribute("centerPoint3DX").is_some()
                    || node.attribute("centerPoint3DY").is_some()
                {
                    self.number(node, "centerPoint3DX", 0., file);
                    self.number(node, "centerPoint3DY", 0., file);
                    self.report.warning("transform_pivot", &location, "Placement matrix retained; Flash's 2D authoring centerPoint3DX/Y pivot is not retained. Actual 3D transforms remain unsupported.");
                }
                cache_as_bitmap = self.boolean(node, "cacheAsBitmap", false, file);
                let _ = self.boolean(node, "exportAsBitmap", false, file);
                visible = self.boolean(node, "isVisible", true, file);
                let has_accessible_data = self.boolean(node, "hasAccessibleData", false, file);
                let silent = self.boolean(node, "silent", false, file);
                let _ = self.boolean(node, "forceSimple", false, file);
                let tab_index = self.integer(node, "tabIndex", 0, file).clamp(0, 10_000);
                if let Some(bm) = node.attribute("blendMode")
                    && !bm.is_empty()
                {
                    if let Some(parsed_bm) = BlendMode::from_name(bm) {
                        blend_mode = parsed_bm;
                    } else {
                        self.report.warning(
                            "xfl_blend_mode_approximated",
                            &location,
                            format!(
                                "XFL instance blendMode={bm:?} is rendered with normal alpha compositing."
                            ),
                        );
                    }
                }
                if has_accessible_data
                    || node
                        .attribute("accName")
                        .is_some_and(|s| !s.trim().is_empty())
                {
                    let raw_acc_name = node
                        .attribute("accName")
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .or_else(|| {
                            node.attribute("name")
                                .map(str::trim)
                                .filter(|s| !s.is_empty())
                        })
                        .or_else(|| {
                            node.attribute("libraryItemName")
                                .map(str::trim)
                                .filter(|s| !s.is_empty())
                        })
                        .unwrap_or("Instance");
                    let acc_name: String = raw_acc_name.chars().take(256).collect();
                    let acc_desc: String = node
                        .attribute("description")
                        .unwrap_or("")
                        .chars()
                        .take(4096)
                        .collect();
                    accessibility = Some(Accessibility {
                        name: acc_name,
                        description: acc_desc,
                        role: AccessibleRole::Button,
                        tab_order: tab_index,
                        disabled: silent,
                    });
                }
                if node.attribute("matrix3D").is_some()
                    || node.attribute("rotationX").is_some()
                    || node.attribute("rotationY").is_some()
                    || node.attribute("rotationZ").is_some()
                {
                    if let Some(m3d_str) = node.attribute("matrix3D") {
                        let vals: Vec<f64> = m3d_str
                            .split(|c: char| c.is_ascii_whitespace() || c == ',')
                            .filter(|s| !s.is_empty())
                            .filter_map(|s| s.parse::<f64>().ok())
                            .collect();
                        if vals.len() == 16 && vals.iter().all(|v| v.is_finite()) {
                            let persp = (1.0 / (1.0 + vals[14] / 1000.0)).clamp(0.1, 10.0);
                            let mat2d = [
                                vals[0] * persp,
                                vals[1] * persp,
                                vals[4] * persp,
                                vals[5] * persp,
                                vals[12],
                                vals[13],
                            ];
                            if let Some(mut projected) = Transform::from_matrix(mat2d) {
                                projected.alpha = transform.alpha;
                                projected.color_transform = transform.color_transform;
                                transform = projected;
                            }
                        }
                    }
                    if node.attribute("rotationZ").is_some() {
                        let rz = self.number(node, "rotationZ", 0.0, file);
                        if rz.is_finite() {
                            transform.rotation = (transform.rotation + rz).clamp(-1e6, 1e6);
                        }
                    }
                    if node.attribute("rotationX").is_some() {
                        let rx = self.number(node, "rotationX", 0.0, file);
                        if rx.is_finite() {
                            let cos_x = rx.to_radians().cos();
                            if cos_x.abs() >= 0.05 {
                                transform.scale_y *= cos_x;
                            }
                        }
                    }
                    if node.attribute("rotationY").is_some() {
                        let ry = self.number(node, "rotationY", 0.0, file);
                        if ry.is_finite() {
                            let cos_y = ry.to_radians().cos();
                            if cos_y.abs() >= 0.05 {
                                transform.scale_x *= cos_y;
                            }
                        }
                    }
                    self.report.warning(
                        "xfl_3d_approximated",
                        &location,
                        "XFL 3D instance transform attributes (matrix3D/rotationX/Y/Z) are projected to 2D placement.",
                    );
                }
                if let Some(filters_node) = child(node, "filters") {
                    if enclosing_kind == SymbolKind::Button {
                        self.audit_filters(filters_node, file, &location);
                    } else {
                        filters = self.parse_instance_filters(filters_node, file, &location);
                    }
                }
                if let Some(matte) = child(node, "MatteColor") {
                    self.audit(matte, file, &["color", "alpha"], &[]);
                }
                if let Some(pd_node) = child(node, "persistentData") {
                    self.audit_persistent_data(pd_node, file);
                }
                if let Some(actions) = child(node, "Actionscript") {
                    self.audit(actions, file, &["breakpoints"], &["script"]);
                    if let Some(script_node) = child(actions, "script") {
                        self.audit(script_node, file, &[], &[]);
                        let source = immediate_text(script_node);
                        if !source.trim().is_empty() {
                            let script_loc = self.location(script_node, file);
                            let unwrapped_neoflash = parse_neoflash_object_scripts(&source);
                            if !unwrapped_neoflash.is_empty() {
                                scripts.extend(unwrapped_neoflash);
                            } else if let Some(language) =
                                self.resolve_script_language(&source, &script_loc)
                            {
                                scripts.push(ObjectScript {
                                    event: ObjectEvent::Load,
                                    script: Script { language, source },
                                });
                                self.report.warning(
                                    "script_preserved",
                                    script_loc,
                                    "Object ActionScript handler source retained unchanged.",
                                );
                            }
                        }
                    }
                }
                let target = node.attribute("libraryItemName").unwrap_or("");
                let Some((symbol_id, target_kind)) = self.symbols.get(target).cloned() else {
                    if matches!(target, "__Camera__" | "___camera___instance") {
                        self.report.warning(
                            "xfl_camera_layer_ignored",
                            &location,
                            "Animate CC virtual camera symbol instance is omitted.",
                        );
                        return vec![];
                    }
                    self.report.error(
                        "missing_symbol",
                        &location,
                        format!("Unknown symbol {target}"),
                    );
                    return vec![];
                };
                let first = self.integer(node, "firstFrame", 0, file);
                let track_as_menu = self.boolean(node, "trackAsMenu", false, file);
                let default_type = match target_kind {
                    SymbolKind::Button => "button",
                    _ => "movie clip",
                };
                let (parsed_kind, playback) = match node
                    .attribute("symbolType")
                    .unwrap_or(default_type)
                {
                    "button" => {
                        if track_as_menu {
                            self.report.warning(
                                    "xfl_button_track_as_menu",
                                    &location,
                                    "Button trackAsMenu behavior is approximated with standard button pointer tracking.",
                                );
                        }
                        if first != 0 || node.attribute("loop").is_some() {
                            self.report.warning(
                                "clip_start_frame",
                                &location,
                                "Button instances ignore graphic loop and firstFrame attributes.",
                            );
                        }
                        (SymbolKind::Button, InstancePlayback::Independent)
                    }
                    "movie clip" => {
                        if track_as_menu {
                            self.report.warning(
                                "symbol_type_mismatch",
                                &location,
                                "trackAsMenu on a MovieClip instance is ignored.",
                            );
                        }
                        if first != 0 {
                            self.report.warning(
                                    "clip_start_frame",
                                    &location,
                                    "MovieClip firstFrame authoring attribute is ignored; MovieClips always start at frame 0.",
                                );
                        }
                        (SymbolKind::MovieClip, InstancePlayback::Independent)
                    }
                    "graphic" => {
                        if track_as_menu {
                            self.report.warning(
                                "symbol_type_mismatch",
                                &location,
                                "trackAsMenu on a graphic instance is ignored.",
                            );
                        }
                        let pb = match node.attribute("loop").unwrap_or("loop") {
                            "single frame" => InstancePlayback::SingleFrame(first),
                            "loop" => {
                                if first == 0 {
                                    InstancePlayback::Loop
                                } else {
                                    InstancePlayback::LoopFrom(first)
                                }
                            }
                            "play once" => {
                                if first == 0 {
                                    InstancePlayback::PlayOnce
                                } else {
                                    InstancePlayback::PlayOnceFrom(first)
                                }
                            }
                            other => {
                                self.report.error(
                                    "graphic_loop",
                                    &location,
                                    format!("Unsupported playback mode {other}"),
                                );
                                InstancePlayback::Loop
                            }
                        };
                        (SymbolKind::Graphic, pb)
                    }
                    other => {
                        self.report.error(
                            "instance_type",
                            &location,
                            format!("Unsupported instance type {other}"),
                        );
                        (target_kind, InstancePlayback::Independent)
                    }
                };
                if parsed_kind != target_kind {
                    instance_behavior = Some(parsed_kind);
                }
                ElementKind::Instance {
                    symbol_id,
                    playback,
                }
            }
            "DOMCompiledClipInstance" | "DOMComponentInstance" => {
                self.audit(
                    node,
                    file,
                    &[
                        "libraryItemName",
                        "name",
                        "selected",
                        "locked",
                        "referenceID",
                        "uniqueIdentifier",
                        "uniqueID",
                        "centerPoint3DX",
                        "centerPoint3DY",
                        "blendMode",
                        "cacheAsBitmap",
                        "isVisible",
                        "hasAccessibleData",
                        "accName",
                        "description",
                        "shortcut",
                        "tabIndex",
                        "silent",
                        "forceSimple",
                    ],
                    &[
                        "matrix",
                        "transformationPoint",
                        "color",
                        "filters",
                        "dataBindingXML",
                        "parametersAsXML",
                        "persistentData",
                    ],
                );
                if node.attribute("centerPoint3DX").is_some()
                    || node.attribute("centerPoint3DY").is_some()
                {
                    self.number(node, "centerPoint3DX", 0., file);
                    self.number(node, "centerPoint3DY", 0., file);
                }
                if let Some(filters_node) = child(node, "filters") {
                    self.audit_filters(filters_node, file, &location);
                }
                if let Some(pd_node) = child(node, "persistentData") {
                    self.audit_persistent_data(pd_node, file);
                }
                for meta_tag in ["dataBindingXML", "parametersAsXML"] {
                    if let Some(sub) = child(node, meta_tag) {
                        self.audit_unmodeled_xfl_subtree(sub, file);
                    }
                }
                let target = node.attribute("libraryItemName").unwrap_or("");
                let symbol_id = if let Some((sid, _)) = self.symbols.get(target).cloned() {
                    sid
                } else if !target.is_empty() {
                    let placeholder_idx = self.extra_symbols.len() + 1;
                    let sid = format!("xfl-component-placeholder-{placeholder_idx}");
                    let lid = format!("xfl-component-placeholder-layer-{placeholder_idx}");
                    let sym_name = target.rsplit_once('/').map_or(target, |(_, b)| b);
                    self.extra_symbols.push(Symbol {
                        folder: String::new(),
                        class_name: None,
                        linkage_id: None,
                        id: sid.clone(),
                        name: sym_name.to_owned(),
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
                                    elements: vec![],
                                    ..Default::default()
                                }],
                            }],
                            scripts: vec![],
                        },
                    });
                    self.symbols
                        .insert(target.to_owned(), (sid.clone(), SymbolKind::MovieClip));
                    sid
                } else {
                    self.report.error(
                        "missing_symbol",
                        &location,
                        "Component instance is missing libraryItemName.",
                    );
                    return vec![];
                };
                self.report.warning(
                    "xfl_component_approximated",
                    &location,
                    format!("{tag} is approximated as a MovieClip instance."),
                );
                ElementKind::Instance {
                    symbol_id,
                    playback: InstancePlayback::Independent,
                }
            }
            "DOMVideoInstance" => {
                self.audit(
                    node,
                    file,
                    &[
                        "libraryItemName",
                        "name",
                        "selected",
                        "locked",
                        "referenceID",
                        "uniqueIdentifier",
                        "frameLeft",
                        "frameRight",
                        "frameTop",
                        "frameBottom",
                        "centerPoint3DX",
                        "centerPoint3DY",
                    ],
                    &["matrix", "transformationPoint", "color", "filters"],
                );
                if let Some(filters_node) = child(node, "filters") {
                    self.audit_filters(filters_node, file, &location);
                }
                let target = node.attribute("libraryItemName").unwrap_or("");
                let (default_w, default_h) =
                    self.videos.get(target).copied().unwrap_or((320.0, 240.0));
                let fl = self.number(node, "frameLeft", 0.0, file);
                let fr = self.number(node, "frameRight", 0.0, file);
                let ft = self.number(node, "frameTop", 0.0, file);
                let fb = self.number(node, "frameBottom", 0.0, file);
                let width = if fr > fl {
                    ((fr - fl) / 20.0).clamp(1.0, 8192.0)
                } else {
                    default_w.clamp(1.0, 8192.0)
                };
                let height = if fb > ft {
                    ((fb - ft) / 20.0).clamp(1.0, 8192.0)
                } else {
                    default_h.clamp(1.0, 8192.0)
                };
                self.report.warning(
                    "xfl_video_approximated",
                    &location,
                    "DOMVideoInstance is imported as a Video viewport; embedded FLV/video stream data is not bundled.",
                );
                ElementKind::Video { width, height }
            }
            "DOMBitmapInstance" => {
                self.audit(
                    node,
                    file,
                    &[
                        "libraryItemName",
                        "name",
                        "selected",
                        "locked",
                        "referenceID",
                        "uniqueIdentifier",
                        "centerPoint3DX",
                        "centerPoint3DY",
                        "frameLeft",
                        "frameRight",
                        "frameTop",
                        "frameBottom",
                    ],
                    &["matrix", "transformationPoint", "color", "filters"],
                );
                if let Some(filters_node) = child(node, "filters") {
                    self.audit_filters(filters_node, file, &location);
                }
                if node.attribute("centerPoint3DX").is_some()
                    || node.attribute("centerPoint3DY").is_some()
                {
                    self.number(node, "centerPoint3DX", 0., file);
                    self.number(node, "centerPoint3DY", 0., file);
                    self.report.warning(
                        "transform_pivot",
                        &location,
                        "Placement matrix retained; Flash's 2D authoring centerPoint3DX/Y pivot is not retained. Actual 3D transforms remain unsupported.",
                    );
                }
                if node.attribute("name").is_some_and(|n| !n.is_empty()) {
                    self.report.warning(
                        "xfl_bitmap_instance_name",
                        &location,
                        "DOMBitmapInstance name is not addressable as a scriptable display object; wrap in a MovieClip symbol if script access is needed.",
                    );
                }
                let target = node.attribute("libraryItemName").unwrap_or("");
                let Some(asset) = self.bitmaps.get(target).cloned() else {
                    self.report.error(
                        "missing_bitmap",
                        &location,
                        format!("Unknown bitmap library item {target:?}."),
                    );
                    return vec![];
                };
                transform.scale_x *= asset.source_scale[0];
                transform.scale_y *= asset.source_scale[1];
                name = String::new();
                ElementKind::Bitmap {
                    resource_id: asset.resource_id,
                    width: asset.width,
                    height: asset.height,
                    smoothing: asset.smoothing,
                }
            }
            "DOMStaticText" | "DOMDynamicText" | "DOMInputText" => {
                self.audit(
                    node,
                    file,
                    &[
                        "name",
                        "width",
                        "height",
                        "left",
                        "top",
                        "selected",
                        "locked",
                        "isSelectable",
                        "fontRenderingMode",
                        "antiAliasSharpness",
                        "antiAliasThickness",
                        "referenceID",
                        "uniqueIdentifier",
                        "autoExpand",
                        "renderAsHTML",
                        "lineType",
                        "includeOutlines",
                        "orientation",
                        "border",
                        "variableName",
                        "scrollable",
                        "maxCharacters",
                        "blendMode",
                        "cacheAsBitmap",
                        "isVisible",
                        "centerPoint3DX",
                        "centerPoint3DY",
                        "hasAccessibleData",
                        "accName",
                        "description",
                        "shortcut",
                        "tabIndex",
                        "silent",
                        "forceSimple",
                    ],
                    &[
                        "matrix",
                        "transformationPoint",
                        "color",
                        "filters",
                        "textRuns",
                        "persistentData",
                    ],
                );
                if let Some(filters_node) = child(node, "filters") {
                    self.audit_filters(filters_node, file, &location);
                }
                if let Some(pd_node) = child(node, "persistentData") {
                    self.audit_persistent_data(pd_node, file);
                }
                let is_selectable =
                    self.boolean(node, "isSelectable", tag != "DOMStaticText", file);
                let auto_expand = self.boolean(node, "autoExpand", false, file);
                let render_as_html = self.boolean(node, "renderAsHTML", false, file);
                let include_outlines = self.boolean(node, "includeOutlines", false, file);
                let border = self.boolean(node, "border", false, file);
                let _ = self.boolean(node, "scrollable", false, file);
                let max_characters = self.u32_attr(node, "maxCharacters", 0, file);
                let orientation = node.attribute("orientation").unwrap_or("horizontal");
                let line_type = node.attribute("lineType").unwrap_or("single line");
                let font_rendering_mode = node.attribute("fontRenderingMode").unwrap_or("normal");
                let aa_sharpness = self
                    .number(node, "antiAliasSharpness", 0.0, file)
                    .clamp(-400.0, 400.0);
                let aa_thickness = self
                    .number(node, "antiAliasThickness", 0.0, file)
                    .clamp(-200.0, 200.0);
                let clean_var: String = node
                    .attribute("variableName")
                    .unwrap_or("")
                    .trim()
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(256)
                    .collect();
                let Some(runs_wrapper) = child(node, "textRuns") else {
                    if tag == "DOMStaticText" {
                        self.report.error(
                            "missing_text_runs",
                            &location,
                            "Static text has no textRuns.",
                        );
                        return vec![];
                    }
                    let left = self.number(node, "left", 0., file);
                    let top = self.number(node, "top", 0., file);
                    let matrix = transform_matrix(transform);
                    transform.x += matrix[0] * left + matrix[2] * top;
                    transform.y += matrix[1] * left + matrix[3] * top;
                    let width = self
                        .number(node, "width", 100., file)
                        .clamp(0.0, 1_000_000.0);
                    let height = self
                        .number(node, "height", 20., file)
                        .clamp(0.0, 1_000_000.0);
                    let password = line_type == "password";
                    let multiline =
                        !password && matches!(line_type, "multiline" | "multiline no wrap");
                    let word_wrap = !password && line_type == "multiline";
                    let mut empty_fmt = TextFormat::default();
                    let empty_opts = AuthoredTextFieldOptions {
                        field_type: if tag == "DOMInputText" {
                            TextFieldType::Input
                        } else {
                            TextFieldType::Dynamic
                        },
                        selectable: is_selectable,
                        display_as_password: password,
                        max_chars: max_characters.min(MAX_TEXT_BYTES as u32),
                        variable: (!clean_var.is_empty()).then_some(clean_var),
                        auto_size: if auto_expand {
                            "left".into()
                        } else {
                            "none".into()
                        },
                        border,
                        background: border,
                        html: render_as_html,
                        embed_fonts: include_outlines,
                        orientation: if orientation.starts_with("vertical") {
                            "vertical".into()
                        } else {
                            "horizontal".into()
                        },
                        anti_alias_type: if matches!(
                            font_rendering_mode,
                            "advanced" | "customThicknessSharpness"
                        ) {
                            "advanced".into()
                        } else {
                            "normal".into()
                        },
                        sharpness: if font_rendering_mode == "customThicknessSharpness" {
                            aa_sharpness
                        } else {
                            0.0
                        },
                        thickness: if font_rendering_mode == "customThicknessSharpness" {
                            aa_thickness
                        } else {
                            0.0
                        },
                        ..AuthoredTextFieldOptions::default()
                    };
                    if !empty_opts.is_default() {
                        empty_fmt.set_field_options(Some(empty_opts));
                    }
                    return vec![Element {
                        id,
                        accessibility: None,
                        name,
                        transform,
                        kind: ElementKind::TextBox {
                            text: String::new(),
                            width,
                            height,
                            format: empty_fmt,
                            word_wrap,
                            multiline,
                        },
                        scripts,
                        ..Element::default()
                    }];
                };
                self.audit(runs_wrapper, file, &[], &["DOMTextRun"]);
                let run_nodes: Vec<_> = elements(runs_wrapper)
                    .filter(|n| n.has_tag_name("DOMTextRun"))
                    .collect();
                let mut combined_text = String::new();
                let mut parsed_runs: Vec<(String, TextFormat, u8, String)> =
                    Vec::with_capacity(run_nodes.len());
                let mut first_color_rgba: Color = [0, 0, 0, 255];
                let mut first_raw_face: String = "Arial".into();
                let mut first_raw_size: f64 = 12.0;
                let mut any_nondefault_layout = false;
                let mut any_nonleft_align = false;
                let mut any_rotated_glyphs = false;
                let mut any_char_position = false;
                for (ri, run_node) in run_nodes.iter().copied().enumerate() {
                    self.audit(run_node, file, &[], &["characters", "textAttrs"]);
                    let Some(characters) = child(run_node, "characters") else {
                        self.report.error(
                            "missing_text",
                            &location,
                            "Text run has no characters node.",
                        );
                        return vec![];
                    };
                    self.audit(characters, file, &[], &[]);
                    let run_text = immediate_text(characters);
                    combined_text.push_str(&run_text);
                    let Some(attrs_wrapper) = child(run_node, "textAttrs") else {
                        self.report.error(
                            "missing_text_style",
                            &location,
                            "Text run has no textAttrs.",
                        );
                        return vec![];
                    };
                    self.audit(attrs_wrapper, file, &[], &["DOMTextAttrs"]);
                    let Some(attrs) = child(attrs_wrapper, "DOMTextAttrs") else {
                        self.report
                            .error("missing_text_style", &location, "No DOMTextAttrs.");
                        return vec![];
                    };
                    self.audit(
                        attrs,
                        file,
                        &[
                            "face",
                            "size",
                            "fillColor",
                            "alpha",
                            "bitmapSize",
                            "aliasText",
                            "autoKern",
                            "alignment",
                            "letterSpacing",
                            "lineSpacing",
                            "lineHeight",
                            "leftMargin",
                            "rightMargin",
                            "indent",
                            "rotation",
                            "characterPosition",
                            "useScreenSpacing",
                            "bold",
                            "italic",
                            "underline",
                            "url",
                            "target",
                        ],
                        &[],
                    );
                    let _ = self.boolean(attrs, "aliasText", false, file);
                    let auto_kern = self.boolean(attrs, "autoKern", false, file);
                    let rotation = self.boolean(attrs, "rotation", false, file);
                    if rotation {
                        any_rotated_glyphs = true;
                        any_nondefault_layout = true;
                    }
                    let _ = self.boolean(attrs, "useScreenSpacing", false, file);
                    let bold = self.boolean(attrs, "bold", false, file);
                    let italic = self.boolean(attrs, "italic", false, file);
                    let underline = self.boolean(attrs, "underline", false, file);
                    let letter_spacing = self.number(attrs, "letterSpacing", 0., file);
                    let left_margin = self.number(attrs, "leftMargin", 0., file);
                    let right_margin = self.number(attrs, "rightMargin", 0., file);
                    let indent = self.number(attrs, "indent", 0., file);
                    let line_spacing = self.number(attrs, "lineSpacing", 0., file);
                    let _ = self.number(attrs, "lineHeight", 0., file);
                    let char_pos = attrs
                        .attribute("characterPosition")
                        .unwrap_or("normal")
                        .to_owned();
                    if char_pos != "normal" {
                        any_char_position = true;
                        any_nondefault_layout = true;
                    }
                    if letter_spacing != 0.0
                        || line_spacing != 0.0
                        || left_margin != 0.0
                        || right_margin != 0.0
                        || indent != 0.0
                        || auto_kern
                        || bold
                        || italic
                        || underline
                        || attrs.attribute("url").is_some_and(|s| !s.is_empty())
                        || attrs.attribute("target").is_some_and(|s| !s.is_empty())
                    {
                        any_nondefault_layout = true;
                    }
                    let align_str = attrs.attribute("alignment").unwrap_or("left");
                    if align_str != "left" {
                        any_nonleft_align = true;
                    }
                    let (align, align_mode) = match align_str {
                        "center" => (TextAlign::Center, None),
                        "right" => (TextAlign::Right, None),
                        "justify" => (TextAlign::Left, Some("justify")),
                        _ => (TextAlign::Left, None),
                    };
                    let alpha = self.number(attrs, "alpha", 1., file);
                    let color = self.color(
                        attrs.attribute("fillColor").unwrap_or("#000000"),
                        alpha,
                        &location,
                    );
                    let raw_face = attrs.attribute("face").unwrap_or("Arial");
                    let mapped_face = self
                        .font_items
                        .get(raw_face)
                        .map(String::as_str)
                        .unwrap_or(raw_face);
                    let clean_face: String = mapped_face
                        .chars()
                        .filter(|c| !c.is_control())
                        .take(256)
                        .collect();
                    let valid_face = if clean_face.trim().is_empty() {
                        "Arial".to_owned()
                    } else {
                        clean_face
                    };
                    let raw_size = self.number(attrs, "size", 12., file);
                    let valid_size = raw_size.clamp(0.5, 10_000.0);
                    let min_leading = -(valid_size * 1.2) + 0.5;
                    let valid_leading = line_spacing.max(min_leading).clamp(-10_000.0, 10_000.0);
                    let clean_url: String = attrs
                        .attribute("url")
                        .unwrap_or("")
                        .chars()
                        .filter(|c| !c.is_control())
                        .take(1024)
                        .collect();
                    let clean_target: String = attrs
                        .attribute("target")
                        .unwrap_or("")
                        .chars()
                        .filter(|c| !c.is_control())
                        .take(128)
                        .collect();
                    let mut run_fmt = TextFormat {
                        font: valid_face.clone(),
                        size: valid_size,
                        color: (u32::from(color[0]) << 16)
                            | (u32::from(color[1]) << 8)
                            | u32::from(color[2]),
                        bold,
                        italic,
                        leading: valid_leading,
                        align,
                        left_margin: left_margin.clamp(0.0, 10_000.0),
                        right_margin: right_margin.clamp(0.0, 10_000.0),
                        indent: indent.clamp(-10_000.0, 10_000.0),
                        block_indent: 0.0,
                        underline,
                        letter_spacing: letter_spacing.clamp(-1_000.0, 1_000.0),
                        kerning: auto_kern.then_some(true),
                        bullet: false,
                        tab_stops: Vec::new(),
                        url: clean_url,
                        target: clean_target,
                    };
                    if let Some(mode) = align_mode {
                        run_fmt.set_align_mode(Some(mode.to_owned()));
                    }
                    if ri == 0 {
                        first_color_rgba = color;
                        first_raw_face = valid_face;
                        first_raw_size = valid_size;
                    }
                    parsed_runs.push((run_text, run_fmt, color[3], char_pos));
                }
                let left = self.number(node, "left", 0., file);
                let top = self.number(node, "top", 0., file);
                let matrix = transform_matrix(transform);
                transform.x += matrix[0] * left + matrix[2] * top;
                transform.y += matrix[1] * left + matrix[3] * top;
                if !self.text_warning {
                    self.report.warning("text_metrics",&location,"Font names and text are retained; line metrics/antialiasing may differ from Flash. Verify imported typography.");
                    self.text_warning = true;
                }
                let has_newlines = combined_text.contains(['\n', '\r']);
                let has_custom_aa =
                    matches!(font_rendering_mode, "advanced" | "customThicknessSharpness");
                if tag == "DOMStaticText"
                    && run_nodes.len() == 1
                    && !has_newlines
                    && !any_nondefault_layout
                    && !any_nonleft_align
                    && !any_char_position
                    && !has_custom_aa
                    && orientation == "horizontal"
                    && !render_as_html
                {
                    ElementKind::Text {
                        text: combined_text,
                        font_family: first_raw_face,
                        font_size: first_raw_size,
                        color: first_color_rgba,
                    }
                } else {
                    if tag == "DOMStaticText" && has_newlines {
                        self.report.warning(
                            "multiline_text",
                            &location,
                            "Multiline static text is imported as a bounded TextBox.",
                        );
                    }

                    let max_chars_budget = MAX_TEXT_BYTES / 4;
                    let mut normalized = String::new();
                    let mut format_runs: Vec<TextFormatRun> = Vec::new();
                    let mut utf16_cursor = 0u32;
                    let mut chars_used = 0usize;
                    let mut prev_ended_cr = false;
                    let mut nonempty_positions: Vec<&str> = Vec::new();
                    let mut nonempty_alphas: Vec<u8> = Vec::new();
                    let fallback_format = parsed_runs
                        .first()
                        .map(|(_, f, _, _)| f.clone())
                        .unwrap_or_default();

                    for (raw_text, run_fmt, run_alpha, char_pos) in &parsed_runs {
                        if chars_used >= max_chars_budget {
                            break;
                        }
                        let trimmed_cr = if prev_ended_cr {
                            raw_text.strip_prefix('\n').unwrap_or(raw_text.as_str())
                        } else {
                            raw_text.as_str()
                        };
                        prev_ended_cr = raw_text.ends_with('\r');
                        let norm_piece: String = normalize_text(trimmed_cr)
                            .chars()
                            .take(max_chars_budget - chars_used)
                            .collect();
                        let piece_chars = norm_piece.chars().count();
                        let piece_units = norm_piece.encode_utf16().count() as u32;
                        if piece_units > 0 {
                            if nonempty_alphas.is_empty() {
                                first_color_rgba[3] = *run_alpha;
                            }
                            nonempty_positions.push(char_pos.as_str());
                            nonempty_alphas.push(*run_alpha);
                            if let Some(last) = format_runs.last_mut()
                                && last.end == utf16_cursor
                                && &last.format == run_fmt
                            {
                                last.end = utf16_cursor + piece_units;
                            } else {
                                format_runs.push(TextFormatRun {
                                    start: utf16_cursor,
                                    end: utf16_cursor + piece_units,
                                    format: run_fmt.clone(),
                                });
                            }
                            utf16_cursor += piece_units;
                            chars_used += piece_chars;
                            normalized.push_str(&norm_piece);
                        }
                    }

                    let mut base_format = if format_runs.len() > 1 {
                        format_runs[0].format.clone()
                    } else if let Some(single) = format_runs.pop() {
                        single.format
                    } else {
                        fallback_format
                    };

                    let first_pos = nonempty_positions.first().copied().unwrap_or("normal");
                    let mixed_pos = nonempty_positions.iter().any(|&p| p != first_pos);
                    let mixed_alpha = nonempty_alphas
                        .first()
                        .is_some_and(|&a0| nonempty_alphas.iter().any(|&a| a != a0));
                    if any_rotated_glyphs || mixed_pos || mixed_alpha {
                        self.report.warning(
                            "xfl_text_approximated",
                            &location,
                            "Per-run alpha, mixed superscript/subscript, or rotated glyphs are approximated in a TextBox.",
                        );
                    }

                    let (superscript, subscript) = if !mixed_pos {
                        match first_pos {
                            "superscript" => (true, false),
                            "subscript" => (false, true),
                            _ => (false, false),
                        }
                    } else {
                        (false, false)
                    };

                    let password = tag != "DOMStaticText" && line_type == "password";
                    let opts = AuthoredTextFieldOptions {
                        field_type: if tag == "DOMInputText" {
                            TextFieldType::Input
                        } else {
                            TextFieldType::Dynamic
                        },
                        selectable: if tag == "DOMStaticText" {
                            true
                        } else {
                            is_selectable
                        },
                        display_as_password: password,
                        max_chars: if tag != "DOMStaticText" {
                            max_characters.min(MAX_TEXT_BYTES as u32)
                        } else {
                            0
                        },
                        variable: if tag != "DOMStaticText" && !clean_var.is_empty() {
                            Some(clean_var)
                        } else {
                            None
                        },
                        auto_size: if tag != "DOMStaticText" && auto_expand {
                            if base_format.align_mode() == Some("justify") {
                                "left".into()
                            } else {
                                match base_format.align {
                                    TextAlign::Left => "left".into(),
                                    TextAlign::Center => "center".into(),
                                    TextAlign::Right => "right".into(),
                                }
                            }
                        } else {
                            "none".into()
                        },
                        border: tag != "DOMStaticText" && border,
                        background: tag != "DOMStaticText" && border,
                        html: render_as_html,
                        embed_fonts: tag != "DOMStaticText" && include_outlines,
                        orientation: if orientation.starts_with("vertical") {
                            "vertical".into()
                        } else {
                            "horizontal".into()
                        },
                        superscript,
                        subscript,
                        anti_alias_type: if has_custom_aa {
                            "advanced".into()
                        } else {
                            "normal".into()
                        },
                        sharpness: if font_rendering_mode == "customThicknessSharpness" {
                            aa_sharpness
                        } else {
                            0.0
                        },
                        thickness: if font_rendering_mode == "customThicknessSharpness" {
                            aa_thickness
                        } else {
                            0.0
                        },
                        format_runs,
                        ..AuthoredTextFieldOptions::default()
                    };
                    if !opts.is_default() {
                        base_format.set_field_options(Some(opts));
                    }

                    let width = self
                        .number(node, "width", 100., file)
                        .clamp(0.0, 1_000_000.0);
                    let height = self
                        .number(node, "height", 20., file)
                        .clamp(0.0, 1_000_000.0);
                    let multiline = !password
                        && (has_newlines || matches!(line_type, "multiline" | "multiline no wrap"));
                    let word_wrap = !password
                        && (line_type == "multiline"
                            || (tag == "DOMStaticText"
                                && has_newlines
                                && line_type != "multiline no wrap"));
                    if first_color_rgba[3] < 255 {
                        transform.alpha = (transform.alpha
                            * (f64::from(first_color_rgba[3]) / 255.0))
                            .clamp(0.0, 1.0);
                    }
                    ElementKind::TextBox {
                        text: normalized,
                        width,
                        height,
                        format: base_format,
                        word_wrap,
                        multiline,
                    }
                }
            }
            "DOMTLFText" => {
                self.audit(
                    node,
                    file,
                    &[
                        "name",
                        "selected",
                        "locked",
                        "left",
                        "top",
                        "right",
                        "bottom",
                        "width",
                        "height",
                        "referenceID",
                        "uniqueIdentifier",
                        "blendMode",
                        "cacheAsBitmap",
                        "isVisible",
                    ],
                    &[
                        "matrix",
                        "transformationPoint",
                        "color",
                        "filters",
                        "tlfFonts",
                        "markup",
                        "tlfTextObject",
                    ],
                );
                if let Some(filters_node) = child(node, "filters") {
                    self.audit_filters(filters_node, file, &location);
                }
                if let Some(tlf_fonts) = child(node, "tlfFonts") {
                    self.audit(tlf_fonts, file, &[], &["TLFFont"]);
                    for tf in elements(tlf_fonts).filter(|n| n.has_tag_name("TLFFont")) {
                        self.audit(tf, file, &["platformName", "psName"], &[]);
                    }
                }
                let mut paragraphs = Vec::new();
                let mut format = TextFormat::default();
                if let Some(markup) = child(node, "markup").or_else(|| child(node, "tlfTextObject"))
                {
                    self.audit_tlf_subtree(markup, file);
                    for p_node in markup
                        .descendants()
                        .filter(|n| n.is_element() && n.tag_name().name() == "p")
                    {
                        let mut para = String::new();
                        for desc in p_node.descendants() {
                            if desc.is_text()
                                && let Some(t) = desc.text()
                            {
                                para.push_str(t);
                            } else if desc.is_element() && desc.tag_name().name() == "br" {
                                para.push('\n');
                            }
                        }
                        paragraphs.push(para);
                    }
                    if let Some(span) = markup
                        .descendants()
                        .find(|n| n.is_element() && n.tag_name().name() == "span")
                    {
                        if let Some(ff) = span
                            .attribute("fontFamily")
                            .map(str::trim)
                            .filter(|s| !s.is_empty() && !s.chars().any(char::is_control))
                        {
                            format.font = ff.chars().take(256).collect();
                        }
                        if let Some(fs) = span
                            .attribute("fontSize")
                            .and_then(|s| s.parse::<f64>().ok())
                            .filter(|v| v.is_finite())
                        {
                            format.size = fs.clamp(0.5, 10_000.0);
                        }
                        if let Some(col_str) = span.attribute("color") {
                            let c = self.color(col_str, 1.0, &location);
                            format.color =
                                (u32::from(c[0]) << 16) | (u32::from(c[1]) << 8) | u32::from(c[2]);
                        }
                    }
                }
                let left = self.number(node, "left", 0., file);
                let top = self.number(node, "top", 0., file);
                let right = self.number(node, "right", left + 100., file);
                let bottom = self.number(node, "bottom", top + 20., file);
                let matrix = transform_matrix(transform);
                transform.x += matrix[0] * left + matrix[2] * top;
                transform.y += matrix[1] * left + matrix[3] * top;
                let width = self
                    .number(node, "width", (right - left).max(100.0), file)
                    .clamp(0.0, 1_000_000.0);
                let height = self
                    .number(node, "height", (bottom - top).max(20.0), file)
                    .clamp(0.0, 1_000_000.0);
                let text: String = normalize_text(&paragraphs.join("\n"))
                    .chars()
                    .take(MAX_TEXT_BYTES / 4)
                    .collect();
                let multiline = text.contains('\n');
                self.report.warning(
                    "xfl_tlf_text_approximated",
                    &location,
                    "DOMTLFText TextFlow markup is imported as a plain TextBox.",
                );
                ElementKind::TextBox {
                    text,
                    width,
                    height,
                    format,
                    word_wrap: true,
                    multiline,
                }
            }
            "DOMRectangleObject" | "DOMOvalObject" => {
                self.audit(
                    node,
                    file,
                    &[
                        "name",
                        "objectWidth",
                        "objectHeight",
                        "x",
                        "y",
                        "selected",
                        "locked",
                        "referenceID",
                        "uniqueIdentifier",
                        "lockFlag",
                        "topLeftRadius",
                        "topRightRadius",
                        "bottomLeftRadius",
                        "bottomRightRadius",
                        "startAngle",
                        "endAngle",
                        "innerRadius",
                        "closePath",
                    ],
                    &[
                        "matrix",
                        "transformationPoint",
                        "color",
                        "filters",
                        "fill",
                        "stroke",
                    ],
                );
                if let Some(filters_node) = child(node, "filters") {
                    self.audit_filters(filters_node, file, &location);
                }
                let fill = if let Some(fill_wrapper) = child(node, "fill") {
                    self.audit(
                        fill_wrapper,
                        file,
                        &[],
                        &[
                            "SolidColor",
                            "LinearGradient",
                            "RadialGradient",
                            "BitmapFill",
                        ],
                    );
                    if let Some(solid) = child(fill_wrapper, "SolidColor") {
                        self.audit(solid, file, &["color", "alpha"], &[]);
                        let alpha = self.number(solid, "alpha", 1., file);
                        self.color(
                            solid.attribute("color").unwrap_or("#000000"),
                            alpha,
                            &location,
                        )
                    } else if let Some(grad) = child(fill_wrapper, "LinearGradient")
                        .or_else(|| child(fill_wrapper, "RadialGradient"))
                    {
                        self.audit_unmodeled_xfl_subtree(grad, file);
                        self.report.warning(
                            "xfl_primitive_approximated",
                            &location,
                            "Gradient fill on primitive object is approximated using its first stop color.",
                        );
                        if let Some(entry) =
                            elements(grad).find(|n| n.has_tag_name("GradientEntry"))
                        {
                            let alpha = self.number(entry, "alpha", 1.0, file);
                            self.color(
                                entry.attribute("color").unwrap_or("#000000"),
                                alpha,
                                &location,
                            )
                        } else {
                            [0, 0, 0, 255]
                        }
                    } else if let Some(bf) = child(fill_wrapper, "BitmapFill") {
                        self.audit_unmodeled_xfl_subtree(bf, file);
                        self.report.warning(
                            "xfl_primitive_approximated",
                            &location,
                            "BitmapFill on primitive object is approximated as solid gray.",
                        );
                        [128, 128, 128, 255]
                    } else {
                        [0, 0, 0, 0]
                    }
                } else {
                    [0, 0, 0, 0]
                };
                let (stroke, stroke_width) = if let Some(stroke_wrapper) = child(node, "stroke") {
                    self.audit(
                        stroke_wrapper,
                        file,
                        &[],
                        &[
                            "SolidStroke",
                            "DashedStroke",
                            "DottedStroke",
                            "RaggedStroke",
                            "StippleStroke",
                            "HatchedStroke",
                        ],
                    );
                    if let Some(stroke_node) = elements(stroke_wrapper).next() {
                        self.audit_unmodeled_xfl_subtree(stroke_node, file);
                        if stroke_node.tag_name().name() != "SolidStroke" {
                            self.report.warning(
                                "primitive_stroke",
                                &location,
                                "Patterned stroke on primitive object is approximated as a solid stroke.",
                            );
                        }
                        let w = self
                            .number(stroke_node, "weight", 1.0, file)
                            .clamp(0.0, 1e9);
                        let scolor = if let Some(sfill) = child(stroke_node, "fill")
                            && let Some(solid) = child(sfill, "SolidColor")
                        {
                            let alpha = self.number(solid, "alpha", 1.0, file);
                            self.color(
                                solid.attribute("color").unwrap_or("#000000"),
                                alpha,
                                &location,
                            )
                        } else {
                            [0, 0, 0, 255]
                        };
                        (scolor, w)
                    } else {
                        ([0, 0, 0, 0], 0.0)
                    }
                } else {
                    ([0, 0, 0, 0], 0.0)
                };
                let x = self.number(node, "x", 0., file);
                let y = self.number(node, "y", 0., file);
                let matrix = transform_matrix(transform);
                transform.x += matrix[0] * x + matrix[2] * y;
                transform.y += matrix[1] * x + matrix[3] * y;
                let width = self.number(node, "objectWidth", 0., file).clamp(0.0, 1e9);
                let height = self.number(node, "objectHeight", 0., file).clamp(0.0, 1e9);
                if tag == "DOMRectangleObject" {
                    let locked = self.boolean(node, "lockFlag", true, file);
                    let raw_radii = [
                        self.number(node, "topLeftRadius", 0., file),
                        self.number(node, "topRightRadius", 0., file),
                        self.number(node, "bottomLeftRadius", 0., file),
                        self.number(node, "bottomRightRadius", 0., file),
                    ];
                    if raw_radii.iter().any(|&r| r < 0.0) {
                        self.report.warning(
                            "xfl_primitive_approximated",
                            &location,
                            "Negative (inverted) rectangle corner radii are clamped to 0.",
                        );
                    }
                    let corner_radii = raw_radii.map(|r| r.clamp(0.0, 1e9));
                    let opts = RectangleOptions {
                        corner_radii,
                        corners_locked: locked,
                    };
                    if !opts.is_default() {
                        transform.primitive = Some(PrimitiveOptions::Rectangle(opts));
                    }
                    ElementKind::Rectangle {
                        width,
                        height,
                        fill,
                        stroke,
                        stroke_width,
                    }
                } else {
                    let norm_angle = |a: f64| -> f64 {
                        if (a - 360.0).abs() <= 1e-6 {
                            360.0
                        } else {
                            a.rem_euclid(360.0)
                        }
                    };
                    let start_angle = norm_angle(self.number(node, "startAngle", 0., file));
                    let end_angle = norm_angle(self.number(node, "endAngle", 0., file));
                    let inner_radius = self.number(node, "innerRadius", 0., file).clamp(0.0, 99.0);
                    let close_path = self.boolean(node, "closePath", true, file);
                    let opts = OvalOptions {
                        start_angle,
                        end_angle,
                        inner_radius,
                        close_path,
                    };
                    if !opts.is_default() {
                        transform.primitive = Some(PrimitiveOptions::Oval(opts));
                    }
                    ElementKind::Ellipse {
                        width,
                        height,
                        fill,
                        stroke,
                        stroke_width,
                    }
                }
            }
            _ => return vec![],
        };
        vec![Element {
            id,
            accessibility,
            name,
            transform,
            kind,
            scripts,
            visible,
            cache_as_bitmap,
            blend_mode,
            filters,
            instance_behavior,
            ..Element::default()
        }]
    }

    pub(super) fn transform(&mut self, node: Node<'_, '_>, file: &str) -> Transform {
        let mut t = Transform::default();
        if let Some(wrapper) = child(node, "matrix") {
            self.audit(wrapper, file, &[], &["Matrix"]);
            if let Some(matrix) = child(wrapper, "Matrix") {
                self.audit(matrix, file, &["a", "b", "c", "d", "tx", "ty"], &[]);
                let a = self.number(matrix, "a", 1., file);
                let b = self.number(matrix, "b", 0., file);
                let c = self.number(matrix, "c", 0., file);
                let d = self.number(matrix, "d", 1., file);
                let tx = self.number(matrix, "tx", 0., file);
                let ty = self.number(matrix, "ty", 0., file);
                let sx = a.hypot(b);
                let sy = c.hypot(d);
                let det = a * d - b * c;
                if sx < 1e-12 || sy < 1e-12 || det.abs() <= 1e-12 * sx * sy {
                    self.report.error(
                        "matrix_singular",
                        self.location(matrix, file),
                        "Singular or zero-scale placement matrices cannot be represented as an editable 2D transform.",
                    );
                } else if (a * c + b * d).abs() <= 1e-7 * (1. + sx * sy) {
                    t.scale_x = sx;
                    t.scale_y = det / sx;
                    t.rotation = b.atan2(a).to_degrees();
                    t.x = tx;
                    t.y = ty;
                } else if let Some(skewed) = Transform::from_matrix([a, b, c, d, tx, ty]) {
                    t.x = skewed.x;
                    t.y = skewed.y;
                    t.scale_x = skewed.scale_x;
                    t.scale_y = skewed.scale_y;
                    t.rotation = skewed.rotation;
                    t.skew = skewed.skew;
                } else {
                    self.report.error(
                        "matrix_singular",
                        self.location(matrix, file),
                        "Singular or out-of-range placement matrix cannot be represented.",
                    );
                }
            }
        }
        if let Some(wrapper) = child(node, "color") {
            self.audit(wrapper, file, &[], &["Color"]);
            if let Some(color) = child(wrapper, "Color") {
                self.audit(
                    color,
                    file,
                    &[
                        "alphaMultiplier",
                        "redMultiplier",
                        "greenMultiplier",
                        "blueMultiplier",
                        "alphaOffset",
                        "redOffset",
                        "greenOffset",
                        "blueOffset",
                        "tintColor",
                        "tintMultiplier",
                        "brightness",
                    ],
                    &[],
                );
                let has_only_alpha = color.attributes().all(|a| a.name() == "alphaMultiplier");
                if has_only_alpha {
                    t.alpha = self.number(color, "alphaMultiplier", 1., file);
                } else {
                    let loc = self.location(color, file);
                    let mut rm = self.number(color, "redMultiplier", 1., file);
                    let mut gm = self.number(color, "greenMultiplier", 1., file);
                    let mut bm = self.number(color, "blueMultiplier", 1., file);
                    let am = self.number(color, "alphaMultiplier", 1., file);
                    let mut ro = self.number(color, "redOffset", 0., file);
                    let mut go = self.number(color, "greenOffset", 0., file);
                    let mut bo = self.number(color, "blueOffset", 0., file);
                    let ao = self.number(color, "alphaOffset", 0., file);
                    if color.attribute("tintColor").is_some()
                        || color.attribute("tintMultiplier").is_some()
                    {
                        let tm = self
                            .number(color, "tintMultiplier", 0., file)
                            .clamp(0.0, 1.0);
                        let tc = self.color(
                            color.attribute("tintColor").unwrap_or("#000000"),
                            1.0,
                            &loc,
                        );
                        rm = 1.0 - tm;
                        gm = 1.0 - tm;
                        bm = 1.0 - tm;
                        ro = f64::from(tc[0]) * tm;
                        go = f64::from(tc[1]) * tm;
                        bo = f64::from(tc[2]) * tm;
                    } else if color.attribute("brightness").is_some() {
                        let br = self.number(color, "brightness", 0., file).clamp(-1.0, 1.0);
                        if br < 0.0 {
                            let m = 1.0 + br;
                            rm = m;
                            gm = m;
                            bm = m;
                        } else {
                            let m = 1.0 - br;
                            let off = 255.0 * br;
                            rm = m;
                            gm = m;
                            bm = m;
                            ro = off;
                            go = off;
                            bo = off;
                        }
                    }
                    t.alpha = am.clamp(0.0, 1.0);
                    t.color_transform = ColorTransform {
                        multipliers: [
                            rm.clamp(-10_000.0, 10_000.0),
                            gm.clamp(-10_000.0, 10_000.0),
                            bm.clamp(-10_000.0, 10_000.0),
                            1.0,
                        ],
                        offsets: [
                            ro.clamp(-255.0, 255.0),
                            go.clamp(-255.0, 255.0),
                            bo.clamp(-255.0, 255.0),
                            ao.clamp(-255.0, 255.0),
                        ],
                    };
                }
            }
        }
        if let Some(point) = child(node, "transformationPoint") {
            self.audit(point, file, &[], &["Point"]);
            if let Some(point) = child(point, "Point") {
                self.audit(point, file, &["x", "y"], &[]);
                if self.number(point, "x", 0., file) != 0.
                    || self.number(point, "y", 0., file) != 0.
                {
                    self.report.warning(
                        "transform_pivot",
                        self.location(point, file),
                        "Placement matrix retained; authoring transform pivot is not retained.",
                    );
                }
            }
        }
        t
    }
}
