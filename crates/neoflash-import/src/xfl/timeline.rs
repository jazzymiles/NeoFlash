use super::*;

impl Parser<'_> {
    pub(super) fn symbol(
        &mut self,
        node: Node<'_, '_>,
        id: Id,
        kind: SymbolKind,
        file: &str,
    ) -> Symbol {
        self.audit(
            node,
            file,
            &[
                "name",
                "currentFrame",
                "guides",
                "autoNamed",
                "layerDepthEnabled",
            ],
            &["layers"],
        );
        if self.boolean(node, "layerDepthEnabled", false, file) {
            self.report.warning(
                "xfl_layer_depth_ignored",
                self.location(node, file),
                "Animate CC layerDepthEnabled Z-depth is approximated as flat 2D painter order.",
            );
        }
        let mut timeline = Timeline {
            guides: vec![],
            frame_count: 1,
            layers: vec![],
            scripts: vec![],
        };
        let Some(layers) = child(node, "layers") else {
            self.report.error(
                "missing_layers",
                self.location(node, file),
                "Timeline lacks layers.",
            );
            return Symbol {
                folder: String::new(),
                class_name: None,
                linkage_id: None,
                id,
                name: node.attribute("name").unwrap_or("Scene 1").into(),
                kind,
                timeline,
            };
        };
        self.audit(layers, file, &[], &["DOMLayer"]);
        let layer_nodes: Vec<_> = elements(layers)
            .filter(|n| n.has_tag_name("DOMLayer"))
            .collect();
        let mut parent_indices: Vec<Option<(usize, String)>> = vec![None; layer_nodes.len()];
        let mut spans = vec![];
        let mut instance_starts: BTreeMap<u32, Vec<(Id, String)>> = BTreeMap::new();
        let mut instance_ends: BTreeMap<u32, BTreeSet<Id>> = BTreeMap::new();
        let mut published_end = 0;
        let mut excluded_end = 0;
        for (li, layer_node) in layer_nodes.iter().copied().enumerate() {
            self.audit(
                layer_node,
                file,
                &[
                    "name",
                    "color",
                    "visible",
                    "locked",
                    "current",
                    "isSelected",
                    "autoNamed",
                    "layerType",
                    "parentLayerIndex",
                    "animationType",
                    "open",
                    "heightLiteral",
                    "heightMultiplier",
                    "outline",
                    "useOutlineView",
                    "highlighted",
                    "poseLocations",
                ],
                &["frames"],
            );
            for attr in ["heightLiteral", "heightMultiplier"] {
                if layer_node.attribute(attr).is_some() {
                    let h = self.integer(layer_node, attr, 1, file);
                    if h == 0 {
                        self.report.error(
                            "integer_required",
                            self.location(layer_node, file),
                            format!("{attr} must be a positive integer."),
                        );
                    }
                }
            }
            if let Some(anim) = layer_node.attribute("animationType")
                && !anim.is_empty()
                && anim != "none"
            {
                self.report.warning(
                    "motion_object_not_supported",
                    self.location(layer_node, file),
                    format!(
                        "XFL layer animationType={anim:?} is approximated using its authored keyframe poses."
                    ),
                );
            }
            let role = match layer_node.attribute("layerType").unwrap_or("normal") {
                "normal" => LayerRole::Normal,
                "guide" => LayerRole::Guide,
                "mask" => LayerRole::Mask,
                "folder" => LayerRole::Folder,
                "camera" => {
                    self.report.warning(
                        "xfl_camera_layer_approximated",
                        self.location(layer_node, file),
                        "Animate CC camera layer is imported as an ordinary layer; virtual camera pan/zoom is not evaluated.",
                    );
                    LayerRole::Normal
                }
                other => {
                    self.report.error(
                        "layer_type",
                        self.location(layer_node, file),
                        format!("Unsupported XFL layerType {other:?}."),
                    );
                    LayerRole::Normal
                }
            };
            let open = self.boolean(layer_node, "open", true, file);
            if layer_node.attribute("parentLayerIndex").is_some() {
                let p_idx = self.integer(layer_node, "parentLayerIndex", u32::MAX, file) as usize;
                if p_idx >= layer_nodes.len() || p_idx == li {
                    self.report.error(
                        "invalid_layer_parent",
                        self.location(layer_node, file),
                        "parentLayerIndex must reference a distinct layer in the same timeline.",
                    );
                } else {
                    parent_indices[li] = Some((p_idx, self.location(layer_node, file)));
                }
            }
            let authoring_visible = self.boolean(layer_node, "visible", true, file);
            let excluded = !authoring_visible && !self.include_hidden_layers;
            if !authoring_visible {
                self.report.warning(
                    "hidden_layer_publish_policy",
                    self.location(layer_node, file),
                    if excluded {
                        "InvisibleLayer disables publishing hidden layers. Artwork is retained on a hidden NeoFlash layer and excluded from playback; hidden-layer actions/labels require unsupported layer-owned script/label semantics."
                    } else {
                        "Flash includes hidden layers in publishing (the default). This layer is made visible in NeoFlash to preserve published artwork/playback; the original authoring eye-toggle state is not retained."
                    },
                );
            }
            let mut layer = Layer {
                properties: LayerProperties {
                    role,
                    collapsed: role == LayerRole::Folder && !open,
                    ..Default::default()
                },
                id: new_id("layer"),
                name: layer_node.attribute("name").unwrap_or("Layer").into(),
                visible: !excluded,
                locked: self.boolean(layer_node, "locked", false, file),
                keyframes: vec![],
            };
            type RawKeyframeEntry = (u32, u32, String, Vec<(Id, String)>, Keyframe);
            let mut ends = vec![];
            let mut raw_keys: Vec<RawKeyframeEntry> = vec![];
            let mut pending_morph_hints: HashMap<u32, Vec<RawXflMorphHint>> = HashMap::new();
            if let Some(frames) = child(layer_node, "frames") {
                self.audit(frames, file, &[], &["DOMFrame"]);
                for frame_node in elements(frames).filter(|n| n.has_tag_name("DOMFrame")) {
                    let has_motion_xml = child(frame_node, "MotionObjectXML").is_some()
                        || child(frame_node, "motionObjectXML").is_some();
                    self.audit(
                        frame_node,
                        file,
                        &[
                            "index",
                            "duration",
                            "name",
                            "labelType",
                            "keyMode",
                            "tweenType",
                            "acceleration",
                            "easeMethodName",
                            "isMotionObject",
                            "visibleAnimationKeyframes",
                            "bookmark",
                            "tweenInstanceName",
                            "isIKPose",
                            "motionTweenOrientToPath",
                            "motionTweenSnap",
                            "motionTweenRotate",
                            "motionTweenRotateTimes",
                            "motionTweenScale",
                            "motionTweenSync",
                            "shapeTweenBlend",
                            "hasCustomEase",
                            "useSingleEaseCurve",
                            "soundName",
                            "soundSync",
                            "soundEffect",
                            "soundLoopMode",
                            "soundLoop",
                            "inPoint44",
                            "outPoint44",
                            "soundZoomLevel",
                        ],
                        &[
                            "elements",
                            "Actionscript",
                            "tweens",
                            "SoundEnvelope",
                            "MotionObjectXML",
                            "motionObjectXML",
                            "MorphShape",
                            "morphHintsList",
                            "betweenFrameMatrixList",
                            "IKTree",
                        ],
                    );
                    let _ = self.boolean(frame_node, "isMotionObject", false, file);
                    let _ = self.boolean(frame_node, "bookmark", false, file);
                    let _ = self.boolean(frame_node, "isIKPose", false, file);
                    for motion_tag in ["MotionObjectXML", "motionObjectXML"] {
                        if let Some(sub) = child(frame_node, motion_tag) {
                            self.audit_unmodeled_xfl_subtree(sub, file);
                            self.report.warning(
                                "motion_object_not_supported",
                                self.location(frame_node, file),
                                "XFL motion-object property-track XML is approximated by holding the authored keyframe pose.",
                            );
                        }
                    }
                    let mut warn_morph_ignored = false;
                    for morph_tag in ["MorphShape", "morphHintsList", "betweenFrameMatrixList"] {
                        if let Some(sub) = child(frame_node, morph_tag) {
                            self.audit_unmodeled_xfl_subtree(sub, file);
                            if morph_tag == "betweenFrameMatrixList" {
                                warn_morph_ignored = true;
                            }
                        }
                    }
                    let (parsed_morph_hints, had_unconverted_morph_data) =
                        parse_xfl_frame_morph_hints(frame_node);
                    if warn_morph_ignored || had_unconverted_morph_data {
                        self.report.warning(
                            "xfl_morph_hints_ignored",
                            self.location(frame_node, file),
                            "Compatible XFL shape hints were retained; malformed, unsupported, or excess MorphShape data was ignored, and remaining contours are resampled automatically.",
                        );
                    }
                    if let Some(sub) = child(frame_node, "IKTree") {
                        self.audit_unmodeled_xfl_subtree(sub, file);
                        self.report.warning(
                            "xfl_ik_approximated",
                            self.location(frame_node, file),
                            "XFL inverse-kinematics IKTree pose constraints are approximated using authored element poses.",
                        );
                    }
                    let frame_loc = self.location(frame_node, file);
                    let (tween, tween_kind, orient_to_path) =
                        self.frame_tween(frame_node, has_motion_xml, file);
                    let sound = self.frame_sound(frame_node, file);
                    let frame_label: String = frame_node.attribute("name").unwrap_or("").into();
                    let label_type = match frame_node.attribute("labelType").unwrap_or("name") {
                        "name" => FrameLabelType::Name,
                        "comment" => FrameLabelType::Comment,
                        "anchor" => FrameLabelType::Anchor,
                        other => {
                            self.report.error(
                                "frame_label_type",
                                &frame_loc,
                                format!("Unsupported frame labelType {other:?}."),
                            );
                            FrameLabelType::Name
                        }
                    };
                    let frame = self.integer(frame_node, "index", 0, file);
                    let duration = self.integer(frame_node, "duration", 1, file);
                    if duration == 0 {
                        self.report.error(
                            "frame_duration",
                            &frame_loc,
                            "Frame duration must be positive.",
                        );
                    }
                    let end = frame.saturating_add(duration.max(1));
                    if role != LayerRole::Folder {
                        if excluded {
                            excluded_end = excluded_end.max(end);
                            if !frame_label.is_empty()
                                && label_type != FrameLabelType::Comment
                                && !frame_label.starts_with("//")
                            {
                                self.report.error("excluded_layer_label", &frame_loc, "A label on an excluded hidden layer cannot be retained with its publication ownership in the current model. Move the label to a published layer before importing.");
                            }
                        } else {
                            published_end = published_end.max(end);
                        }
                        timeline.frame_count = timeline.frame_count.max(end);
                        ends.push((frame, end));
                    } else if frame_node
                        .attribute("name")
                        .is_some_and(|name| !name.is_empty())
                        || tween.is_some()
                        || sound.is_some()
                    {
                        self.report.error(
                            "folder_layer_elements",
                            &frame_loc,
                            "Folder layers cannot own labels, tweens or sounds.",
                        );
                    }
                    let mut key = Keyframe {
                        frame,
                        elements: vec![],
                        tween,
                        tween_kind,
                        orient_to_path,
                        sound,
                        label: frame_label,
                        label_type,
                        ..Default::default()
                    };
                    let mut elem_locs = vec![];
                    if let Some(container) = child(frame_node, "elements") {
                        self.audit(
                            container,
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
                        for (ei, element_node) in elements(container).enumerate() {
                            if role == LayerRole::Folder {
                                self.report.error(
                                    "folder_layer_elements",
                                    self.location(element_node, file),
                                    "Folder layers must not contain display elements.",
                                );
                                continue;
                            }
                            let identity = format!("xfl-{id}-{li}-{ei}");
                            self.pending_bitmap_fills.clear();
                            self.pending_video_resource = None;
                            for element in self.element_with_depth(
                                element_node,
                                identity,
                                frame,
                                0,
                                kind,
                                file,
                            ) {
                                elem_locs
                                    .push((element.id.clone(), self.location(element_node, file)));
                                key.elements.push(element);
                            }
                            key.bitmap_fills.append(&mut self.pending_bitmap_fills);
                            if let Some((vid_res, sync_video, import_audio)) =
                                self.pending_video_resource.take()
                                && key.video.is_none()
                            {
                                key.video = Some(TimelineVideoSpan {
                                    resource: vid_res,
                                    sync: if sync_video {
                                        VideoSyncMode::Stream
                                    } else {
                                        VideoSyncMode::Event
                                    },
                                    muted: !import_audio,
                                    deployment: timeline_video::VideoDeploymentMode::Embedded,
                                    ..Default::default()
                                });
                            }
                        }
                    }
                    if let Some(actions) = child(frame_node, "Actionscript") {
                        self.audit(actions, file, &["breakpoints"], &["script"]);
                        if let Some(script) = child(actions, "script") {
                            self.audit(script, file, &[], &[]);
                            let source = immediate_text(script);
                            if !source.trim().is_empty() {
                                if role == LayerRole::Folder {
                                    self.report.error(
                                        "folder_layer_elements",
                                        self.location(script, file),
                                        "Folder layers cannot own frame scripts.",
                                    );
                                }
                                if excluded {
                                    self.report.error("excluded_layer_script", self.location(script, file), "A frame script on an excluded hidden layer cannot retain its publication ownership in the current model. Import is blocked rather than executing or discarding that source.");
                                }
                                let script_loc = self.location(script, file);
                                if let Some((language, clean_source)) =
                                    strip_neoflash_language_marker(&source)
                                {
                                    timeline.scripts.push(FrameScript {
                                        layer: Some(layer.id.clone()),
                                        frame,
                                        script: Script {
                                            language,
                                            source: clean_source,
                                        },
                                    });
                                } else if let Some(language) =
                                    self.resolve_script_language(&source, &script_loc)
                                {
                                    timeline.scripts.push(FrameScript {
                                        layer: Some(layer.id.clone()),
                                        frame,
                                        script: Script { language, source },
                                    });
                                    self.report.warning("script_preserved",script_loc,"ActionScript source retained unchanged. Execution compatibility is checked separately by the compiler/exporter.");
                                }
                            }
                        }
                    }
                    if role != LayerRole::Folder {
                        if !parsed_morph_hints.is_empty() {
                            pending_morph_hints.insert(frame, parsed_morph_hints);
                        }
                        raw_keys.push((frame, end, frame_loc, elem_locs, key));
                    }
                }
            }
            raw_keys.sort_by_key(|entry| entry.0);
            ends.sort_unstable();
            for pair in ends.windows(2) {
                if pair[0].1 > pair[1].0 {
                    self.report.error(
                        "overlapping_frames",
                        self.location(layer_node, file),
                        "XFL frame spans overlap.",
                    );
                }
            }
            // Resolve classic and shape tween continuity across consecutive keyframes.
            for idx in 0..raw_keys.len() {
                let (prev_tween, prev_kind, prev_end, prev_loc) = {
                    let prev = &raw_keys[idx];
                    (prev.4.tween, prev.4.tween_kind, prev.1, prev.2.clone())
                };
                if prev_tween.is_none() {
                    continue;
                }
                if idx + 1 >= raw_keys.len() || raw_keys[idx + 1].0 != prev_end {
                    raw_keys[idx].4.tween = None;
                    raw_keys[idx].4.tween_kind = TweenKind::Classic;
                    raw_keys[idx].4.orient_to_path = false;
                    self.report.warning(
                        "xfl_tween_dropped",
                        prev_loc,
                        "Unterminated or gap-separated tween has no adjacent end keyframe; its pose is held.",
                    );
                    continue;
                }
                match prev_kind {
                    TweenKind::Classic => {
                        let compatible = raw_keys[idx].4.elements.len() == 1
                            && raw_keys[idx + 1].4.elements.len() == 1
                            && classic_tween_matches(
                                &raw_keys[idx].4.elements[0],
                                &raw_keys[idx + 1].4.elements[0],
                            );
                        if compatible {
                            let continued_id = raw_keys[idx].4.elements[0].id.clone();
                            let prev_elem_id = std::mem::replace(
                                &mut raw_keys[idx + 1].4.elements[0].id,
                                continued_id.clone(),
                            );
                            for bf in &mut raw_keys[idx + 1].4.bitmap_fills {
                                if bf.element_id == prev_elem_id {
                                    bf.element_id = continued_id.clone();
                                }
                            }
                            if let Some(slot) = raw_keys[idx + 1].3.first_mut() {
                                slot.0 = continued_id;
                            }
                        } else if raw_keys[idx + 1].4.elements.is_empty() {
                            raw_keys[idx].4.tween = None;
                            raw_keys[idx].4.orient_to_path = false;
                            self.report.warning(
                                "xfl_tween_dropped",
                                prev_loc,
                                "Classic motion tween ends at a blank keyframe; its start pose is held.",
                            );
                        } else {
                            raw_keys[idx].4.tween = None;
                            raw_keys[idx].4.orient_to_path = false;
                            self.report.warning(
                                "xfl_tween_dropped",
                                prev_loc,
                                "Classic motion tween does not connect a single compatible element across keyframes; poses are held.",
                            );
                        }
                    }
                    TweenKind::Shape => {
                        let is_shape = |e: &Element| {
                            matches!(
                                e.kind,
                                ElementKind::Rectangle { .. }
                                    | ElementKind::Ellipse { .. }
                                    | ElementKind::Path { .. }
                                    | ElementKind::PaintedPath { .. }
                            )
                        };
                        let valid_from = !raw_keys[idx].4.elements.is_empty()
                            && raw_keys[idx].4.elements.iter().all(is_shape);
                        let valid_to = !raw_keys[idx + 1].4.elements.is_empty()
                            && raw_keys[idx + 1].4.elements.iter().all(is_shape);
                        if !valid_from || !valid_to {
                            raw_keys[idx].4.tween = None;
                            raw_keys[idx].4.tween_kind = TweenKind::Classic;
                            raw_keys[idx].4.orient_to_path = false;
                            self.report.warning(
                                "xfl_tween_dropped",
                                prev_loc,
                                "Shape tween requires vector shape elements on both start and end keyframes; tween dropped and poses held.",
                            );
                        } else if let Some(raw_hints) = pending_morph_hints.get(&raw_keys[idx].0) {
                            let hints = resolve_xfl_morph_hints(
                                raw_hints,
                                &raw_keys[idx].4.elements,
                                &raw_keys[idx + 1].4.elements,
                            );
                            if !hints.is_empty() {
                                raw_keys[idx].4.shape_hints = hints;
                            }
                        }
                    }
                    TweenKind::Motion => {}
                }
            }
            // Record true instance placement lifetimes (continuous classic-tween chains
            // share one element ID and do not end/restart at intermediate tween keys).
            let mut active_instances: HashMap<Id, (Id, u32)> = HashMap::new();
            for (frame, end, _, elem_locs, key) in &raw_keys {
                let current_ids: BTreeSet<&str> =
                    key.elements.iter().map(|e| e.id.as_str()).collect();
                active_instances.retain(|elem_id, (sym_id, prev_end)| {
                    if *prev_end == *frame && current_ids.contains(elem_id.as_str()) {
                        true
                    } else {
                        instance_ends
                            .entry(*prev_end)
                            .or_default()
                            .insert(sym_id.clone());
                        false
                    }
                });
                for element in &key.elements {
                    if let ElementKind::Instance { symbol_id, .. } = &element.kind {
                        if let Some(entry) = active_instances.get_mut(&element.id) {
                            entry.1 = *end;
                        } else {
                            let loc = elem_locs
                                .iter()
                                .find(|(eid, _)| eid == &element.id)
                                .map(|(_, l)| l.clone())
                                .unwrap_or_default();
                            instance_starts
                                .entry(*frame)
                                .or_default()
                                .push((symbol_id.clone(), loc));
                            active_instances.insert(element.id.clone(), (symbol_id.clone(), *end));
                        }
                    }
                }
            }
            for (_, (sym_id, last_end)) in active_instances {
                instance_ends.entry(last_end).or_default().insert(sym_id);
            }
            layer.keyframes = raw_keys.into_iter().map(|entry| entry.4).collect();
            spans.push(ends);
            timeline.layers.push(layer);
        }
        let roles_and_ids: Vec<(LayerRole, Id)> = timeline
            .layers
            .iter()
            .map(|l| (l.properties.role, l.id.clone()))
            .collect();
        for (li, parent_entry) in parent_indices.into_iter().enumerate() {
            let Some((p_idx, loc)) = parent_entry else {
                continue;
            };
            let (parent_role, ref parent_id) = roles_and_ids[p_idx];
            let child_role = roles_and_ids[li].0;
            match parent_role {
                LayerRole::Folder => {
                    timeline.layers[li].properties.parent = Some(parent_id.clone());
                }
                LayerRole::Mask if child_role == LayerRole::Normal => {
                    timeline.layers[li].properties.mask = Some(parent_id.clone());
                }
                LayerRole::Guide if child_role == LayerRole::Normal => {
                    timeline.layers[li].properties.motion_guide = Some(parent_id.clone());
                }
                _ => {
                    self.report.error(
                        "invalid_layer_parent",
                        loc,
                        "parentLayerIndex must target a folder layer, or a mask/guide layer from a normal child layer.",
                    );
                }
            }
        }
        for (frame, starts) in instance_starts {
            if let Some(ends) = instance_ends.get(&frame) {
                for (symbol, location) in starts {
                    if ends.contains(&symbol) {
                        self.report.warning(
                            "ambiguous_instance_continuity",
                            location,
                            "An adjacent key ends and places the same library symbol without a classic tween; the importer treats the second keyframe as a new placement span.",
                        );
                    }
                }
            }
        }
        if excluded_end > published_end {
            self.report.error("excluded_layer_duration", self.location(node, file), "An excluded hidden layer defines a longer timeline than the published layers. Published duration cannot yet be established without dropping authored frame structure; align the visible timeline extent before importing.");
        }
        for (layer, ends) in timeline.layers.iter_mut().zip(spans) {
            if layer.properties.role == LayerRole::Folder {
                continue;
            }
            for (_, end) in ends {
                if end < timeline.frame_count && !layer.keyframes.iter().any(|k| k.frame == end) {
                    layer.keyframes.push(Keyframe {
                        frame: end,
                        elements: vec![],
                        tween: None,
                        label: String::new(),
                        ..Default::default()
                    });
                }
            }
            layer.keyframes.sort_by_key(|k| k.frame);
        }
        // XFL lists topmost layers first; NeoFlash stores painter order.
        timeline.layers.reverse();
        Symbol {
            folder: String::new(),
            class_name: None,
            linkage_id: None,
            id,
            name: node.attribute("name").unwrap_or("Scene 1").into(),
            kind,
            timeline,
        }
    }

    pub(super) fn frame_tween(
        &mut self,
        frame_node: Node<'_, '_>,
        has_motion_xml: bool,
        file: &str,
    ) -> (Option<Easing>, TweenKind, bool) {
        let loc = self.location(frame_node, file);
        let accel = self.number(frame_node, "acceleration", 0., file);
        if !(-100.0..=100.0).contains(&accel) {
            self.report.error(
                "invalid_acceleration",
                &loc,
                "Tween acceleration must be within [-100, 100].",
            );
        }
        let has_custom_ease = self.boolean(frame_node, "hasCustomEase", false, file);
        let single_ease = self.boolean(frame_node, "useSingleEaseCurve", true, file);
        let mut custom_bezier = None;
        let mut named_ease_method: Option<String> =
            frame_node.attribute("easeMethodName").map(str::to_owned);
        if let Some(tweens) = child(frame_node, "tweens") {
            self.audit(tweens, file, &[], &["CustomEase", "Ease"]);
            for ease_preset in elements(tweens).filter(|n| n.has_tag_name("Ease")) {
                self.audit(ease_preset, file, &["target", "method", "intensity"], &[]);
                if named_ease_method.is_none()
                    && let Some(m) = ease_preset.attribute("method")
                {
                    named_ease_method = Some(m.to_owned());
                }
            }
            let eases: Vec<_> = elements(tweens)
                .filter(|n| n.has_tag_name("CustomEase"))
                .collect();
            if has_custom_ease || !eases.is_empty() {
                let chosen = eases
                    .iter()
                    .copied()
                    .find(|n| n.attribute("target") == Some("all"))
                    .or_else(|| eases.first().copied());
                if !single_ease || eases.len() > 1 {
                    self.report.warning(
                        "custom_ease_not_supported",
                        self.location(tweens, file),
                        "Multiple per-property XFL CustomEase curves are approximated with the primary curve.",
                    );
                }
                for ease_node in &eases {
                    self.audit(*ease_node, file, &["target"], &["Point"]);
                    for pt in elements(*ease_node).filter(|n| n.has_tag_name("Point")) {
                        self.audit(pt, file, &["x", "y"], &[]);
                        let _ = self.number(pt, "x", 0., file);
                        let _ = self.number(pt, "y", 0., file);
                    }
                }
                if let Some(ease_node) = chosen {
                    if ease_node.attribute("target") != Some("all") {
                        self.report.warning(
                            "custom_ease_not_supported",
                            self.location(ease_node, file),
                            "Per-property XFL CustomEase target is applied to the whole tween span.",
                        );
                    }
                    let pts: Vec<_> = elements(ease_node)
                        .filter(|n| n.has_tag_name("Point"))
                        .map(|pt| {
                            [
                                self.number(pt, "x", 0., file),
                                self.number(pt, "y", 0., file),
                            ]
                        })
                        .collect();
                    if pts.len() == 4 && pts[0] == [0., 0.] && pts[3] == [1., 1.] {
                        if !(0.0..=1.0).contains(&pts[1][0])
                            || !(0.0..=1.0).contains(&pts[2][0])
                            || pts[1][1].abs() > 100.0
                            || pts[2][1].abs() > 100.0
                        {
                            self.report.error(
                                "custom_ease_not_supported",
                                self.location(ease_node, file),
                                "CustomEase Bézier controls must have x in [0, 1] and |y| <= 100.",
                            );
                        } else {
                            custom_bezier = Some(Easing::CubicBezier {
                                x1: pts[1][0],
                                y1: pts[1][1],
                                x2: pts[2][0],
                                y2: pts[2][1],
                            });
                        }
                    } else {
                        self.report.warning(
                            "custom_ease_not_supported",
                            self.location(ease_node, file),
                            "Multi-segment XFL CustomEase curve is approximated with standard tween easing.",
                        );
                    }
                } else {
                    self.report.warning(
                        "custom_ease_not_supported",
                        self.location(tweens, file),
                        "hasCustomEase is true, but no CustomEase curve was supplied.",
                    );
                }
            }
        } else if has_custom_ease {
            self.report.error(
                "custom_ease_not_supported",
                &loc,
                "hasCustomEase is true, but no <tweens> CustomEase curve was supplied.",
            );
        }
        let ease = custom_bezier.unwrap_or_else(|| {
            if let Some(ref method) = named_ease_method {
                let m = method.to_ascii_lowercase();
                if m.contains("bounce") {
                    return Easing::Bounce;
                }
                if m.contains("elastic") {
                    return Easing::Elastic;
                }
                if m.contains("inout") || m.contains("in_out") || m.contains("in-out") {
                    return Easing::EaseInOut;
                }
                if m.contains("in") {
                    return Easing::EaseIn;
                }
                if m.contains("out") {
                    return Easing::EaseOut;
                }
            }
            let clamped = accel.clamp(-100.0, 100.0);
            if clamped == 0.0 {
                Easing::Linear
            } else {
                Easing::Flash {
                    acceleration: clamped,
                }
            }
        });
        let orient = self.boolean(frame_node, "motionTweenOrientToPath", false, file);
        self.boolean(frame_node, "motionTweenSnap", false, file);
        self.boolean(frame_node, "motionTweenScale", true, file);
        self.boolean(frame_node, "motionTweenSync", false, file);
        let rotate_times = self.integer(frame_node, "motionTweenRotateTimes", 0, file);
        let rotate_mode = frame_node.attribute("motionTweenRotate").unwrap_or("auto");
        if !matches!(
            rotate_mode,
            "none" | "auto" | "clockwise" | "counter-clockwise"
        ) {
            self.report.error(
                "invalid_tween_rotate",
                &loc,
                format!("Unsupported motionTweenRotate value {rotate_mode:?}."),
            );
        }
        let shape_blend = frame_node
            .attribute("shapeTweenBlend")
            .unwrap_or("distributive");
        if !matches!(shape_blend, "distributive" | "angular") {
            self.report.error(
                "invalid_shape_tween_blend",
                &loc,
                format!("Unsupported shapeTweenBlend value {shape_blend:?}."),
            );
        }
        match frame_node.attribute("tweenType") {
            None | Some("") | Some("none") => (None, TweenKind::Classic, false),
            Some("motion") => {
                if matches!(rotate_mode, "clockwise" | "counter-clockwise") || rotate_times > 0 {
                    self.report.warning(
                        "tween_rotation",
                        &loc,
                        "Directed classic-tween rotation (clockwise/counter-clockwise turns) is interpolated along the shortest angle.",
                    );
                }
                (Some(ease), TweenKind::Classic, orient)
            }
            Some("shape") => {
                if shape_blend == "angular" {
                    self.report.warning(
                        "shape_tween_blend",
                        &loc,
                        "Angular shape-tween vertex blending is approximated with distributive arc-length resampling.",
                    );
                }
                (Some(ease), TweenKind::Shape, false)
            }
            Some("motion object") => {
                if !has_motion_xml {
                    self.report.warning(
                        "motion_object_not_supported",
                        &loc,
                        "XFL motion-object property-track tween is approximated by holding the authored keyframe pose.",
                    );
                }
                (None, TweenKind::Classic, false)
            }
            Some("IK pose") => {
                self.report.warning(
                    "xfl_ik_approximated",
                    &loc,
                    "XFL inverse-kinematics IK pose tween is approximated using authored keyframe poses.",
                );
                (None, TweenKind::Classic, false)
            }
            Some(other) => {
                self.report.error(
                    "tween_not_supported",
                    &loc,
                    format!("Unsupported XFL tweenType {other:?}."),
                );
                (None, TweenKind::Classic, false)
            }
        }
    }
}

pub(super) fn classic_tween_matches(a: &Element, b: &Element) -> bool {
    a.name == b.name
        && match (&a.kind, &b.kind) {
            (
                ElementKind::Instance { symbol_id: a, .. },
                ElementKind::Instance { symbol_id: b, .. },
            ) => a == b,
            (
                ElementKind::Bitmap { resource_id: a, .. },
                ElementKind::Bitmap { resource_id: b, .. },
            ) => a == b,
            (ElementKind::Rectangle { .. }, ElementKind::Rectangle { .. })
            | (ElementKind::Ellipse { .. }, ElementKind::Ellipse { .. })
            | (ElementKind::Path { .. }, ElementKind::Path { .. })
            | (ElementKind::PaintedPath { .. }, ElementKind::PaintedPath { .. })
            | (ElementKind::Text { .. }, ElementKind::Text { .. })
            | (ElementKind::TextBox { .. }, ElementKind::TextBox { .. })
            | (ElementKind::Video { .. }, ElementKind::Video { .. }) => true,
            _ => false,
        }
}

#[derive(Clone, Debug)]
pub(super) enum RawXflHintCoord {
    Uv([f64; 2]),
    PointCandidates(Vec<[f64; 2]>),
}

#[derive(Clone, Debug)]
pub(super) struct RawXflMorphHint {
    label: Option<char>,
    start: RawXflHintCoord,
    end: RawXflHintCoord,
    approximated: bool,
}

pub(super) fn parse_xfl_hex_24_8_token(token: &str) -> Option<i64> {
    let hex = token.trim().strip_prefix('#')?;
    let (whole_str, frac_opt) = match hex.split_once('.') {
        Some((w, f)) => (w, Some(f)),
        None => (hex, None),
    };
    if !(1..=6).contains(&whole_str.len()) || !whole_str.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let raw_whole = u32::from_str_radix(whole_str, 16).ok()?;
    let signed_whole = if raw_whole > 0x7f_ffff {
        i64::from(raw_whole) - 0x100_0000
    } else {
        i64::from(raw_whole)
    };
    let frac = if let Some(frac_str) = frac_opt {
        if !(1..=2).contains(&frac_str.len()) || !frac_str.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let raw_frac = u8::from_str_radix(frac_str, 16).ok()?;
        if frac_str.len() == 1 {
            raw_frac << 4
        } else {
            raw_frac
        }
    } else {
        0
    };
    Some(signed_whole * 256 + i64::from(frac))
}

pub(super) fn parse_xfl_point_candidates(raw: &str) -> Option<Vec<[f64; 2]>> {
    let trimmed = raw.trim();
    let (tx, ty) = if let Some((a, b)) = trimmed.split_once(',') {
        (a.trim(), b.trim())
    } else {
        let mut iter = trimmed.split_whitespace();
        let a = iter.next()?;
        let b = iter.next()?;
        if iter.next().is_some() {
            return None;
        }
        (a, b)
    };
    if tx.starts_with('#') || ty.starts_with('#') {
        let vx = parse_xfl_hex_24_8_token(tx)?;
        let vy = parse_xfl_hex_24_8_token(ty)?;
        return Some(vec![
            [vx as f64 / 20.0, vy as f64 / 20.0],
            [vx as f64 / 256.0, vy as f64 / 256.0],
            [vx as f64 / 5120.0, vy as f64 / 5120.0],
        ]);
    }
    let fx = tx.parse::<f64>().ok().filter(|v| v.is_finite())?;
    let fy = ty.parse::<f64>().ok().filter(|v| v.is_finite())?;
    Some(vec![
        [fx, fy],
        [fx / 20.0, fy / 20.0],
        [fx * 256.0 / 20.0, fy * 256.0 / 20.0],
    ])
}

pub(super) fn parse_xfl_hint_label(node: Node<'_, '_>) -> Option<char> {
    for attr in ["label", "key", "hintIndex", "index", "name"] {
        let Some(val) = node
            .attribute(attr)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        if val.len() == 1 {
            let ch = val.as_bytes()[0];
            if ch.is_ascii_lowercase() {
                return Some(ch as char);
            }
            if ch.is_ascii_uppercase() {
                return Some(ch.to_ascii_lowercase() as char);
            }
        }
        if let Ok(idx) = val.parse::<i64>()
            && (0..26).contains(&idx)
        {
            return Some((b'a' + idx as u8) as char);
        }
    }
    None
}

pub(super) fn parse_xfl_single_morph_hint(node: Node<'_, '_>) -> Option<RawXflMorphHint> {
    let label = parse_xfl_hint_label(node);
    let parse_f64 = |names: &[&str]| -> Option<f64> {
        for &name in names {
            if let Some(v) = node
                .attribute(name)
                .and_then(|s| s.trim().parse::<f64>().ok())
                .filter(|v| v.is_finite())
            {
                return Some(v);
            }
        }
        None
    };
    if let (Some(su), Some(sv), Some(eu), Some(ev)) = (
        parse_f64(&["startU", "start_u", "u"]),
        parse_f64(&["startV", "start_v", "v"]),
        parse_f64(&["endU", "end_u", "u"]),
        parse_f64(&["endV", "end_v", "v"]),
    ) {
        return Some(RawXflMorphHint {
            label,
            start: RawXflHintCoord::Uv([su.clamp(0.0, 1.0), sv.clamp(0.0, 1.0)]),
            end: RawXflHintCoord::Uv([eu.clamp(0.0, 1.0), ev.clamp(0.0, 1.0)]),
            approximated: [su, sv, eu, ev]
                .into_iter()
                .any(|value| !(0.0..=1.0).contains(&value)),
        });
    }
    let start_str = node
        .attribute("startPoint")
        .or_else(|| node.attribute("startPointA"))
        .or_else(|| node.attribute("pointA"))?;
    let end_str = node
        .attribute("endPoint")
        .or_else(|| node.attribute("endPointB"))
        .or_else(|| node.attribute("pointB"))?;
    let start_candidates = parse_xfl_point_candidates(start_str)?;
    let end_candidates = parse_xfl_point_candidates(end_str)?;
    Some(RawXflMorphHint {
        label,
        start: RawXflHintCoord::PointCandidates(start_candidates),
        end: RawXflHintCoord::PointCandidates(end_candidates),
        approximated: false,
    })
}

pub(super) fn retain_xfl_morph_hint(
    parsed: Option<RawXflMorphHint>,
    hints: &mut Vec<RawXflMorphHint>,
    had_unconverted: &mut bool,
) {
    let Some(parsed) = parsed else {
        *had_unconverted = true;
        return;
    };
    *had_unconverted |= parsed.approximated;
    if hints.len() < ShapeHint::MAX_HINTS {
        hints.push(parsed);
    } else {
        *had_unconverted = true;
    }
}

pub(super) fn inspect_xfl_hint_list(
    list_node: Node<'_, '_>,
    hints: &mut Vec<RawXflMorphHint>,
    had_unconverted: &mut bool,
) {
    for item in elements(list_node) {
        if item.tag_name().name().eq_ignore_ascii_case("MorphHint") {
            retain_xfl_morph_hint(parse_xfl_single_morph_hint(item), hints, had_unconverted);
        } else {
            *had_unconverted = true;
        }
    }
}

pub(super) fn parse_xfl_frame_morph_hints(
    frame_node: Node<'_, '_>,
) -> (Vec<RawXflMorphHint>, bool) {
    let mut hints = Vec::new();
    let mut had_unconverted = false;

    if let Some(morph_node) = child(frame_node, "MorphShape") {
        let mut saw_hint_container = false;
        for sub in elements(morph_node) {
            let tag = sub.tag_name().name();
            if tag.eq_ignore_ascii_case("morphHintsList") {
                saw_hint_container = true;
                inspect_xfl_hint_list(sub, &mut hints, &mut had_unconverted);
            } else if tag.eq_ignore_ascii_case("MorphHint") {
                saw_hint_container = true;
                retain_xfl_morph_hint(
                    parse_xfl_single_morph_hint(sub),
                    &mut hints,
                    &mut had_unconverted,
                );
            }
        }
        if hints.is_empty()
            && (!saw_hint_container
                || elements(morph_node).any(|n| n.has_tag_name("morphSegments")))
        {
            had_unconverted = true;
        }
    }
    if let Some(direct_list) = child(frame_node, "morphHintsList") {
        inspect_xfl_hint_list(direct_list, &mut hints, &mut had_unconverted);
    }

    (hints, had_unconverted)
}

pub(super) fn resolve_xfl_hint_coord(
    geom: Option<&ShapeHintGeometry>,
    coord: &RawXflHintCoord,
) -> [f64; 2] {
    match coord {
        RawXflHintCoord::Uv(uv) => [uv[0].clamp(0.0, 1.0), uv[1].clamp(0.0, 1.0)],
        RawXflHintCoord::PointCandidates(candidates) => {
            let mut best_pt = candidates.first().copied().unwrap_or([0.0, 0.0]);
            if let Some(g) = geom {
                let mut best_d2 = f64::INFINITY;
                for &cand in candidates {
                    let near = g.nearest_with_snap(cand, DEFAULT_HINT_VERTEX_SNAP_RADIUS);
                    let dx = cand[0] - near[0];
                    let dy = cand[1] - near[1];
                    let d2 = dx * dx + dy * dy;
                    if d2 + 1e-6 < best_d2 {
                        best_d2 = d2;
                        best_pt = cand;
                    }
                }
                let diag = (g.high[0] - g.low[0]).hypot(g.high[1] - g.low[1]);
                let snap_tol = (diag * 0.15).clamp(2.0, 24.0);
                let snapped = g.nearest_with_snap(best_pt, DEFAULT_HINT_VERTEX_SNAP_RADIUS);
                let dx = best_pt[0] - snapped[0];
                let dy = best_pt[1] - snapped[1];
                if dx * dx + dy * dy <= snap_tol * snap_tol {
                    return g.snapped_uv(best_pt);
                }
                return std::array::from_fn(|axis| {
                    let size = g.high[axis] - g.low[axis];
                    if size > 1e-9 {
                        ((best_pt[axis] - g.low[axis]) / size).clamp(0.0, 1.0)
                    } else {
                        0.5
                    }
                });
            }
            [
                (best_pt[0] / 550.0).clamp(0.0, 1.0),
                (best_pt[1] / 400.0).clamp(0.0, 1.0),
            ]
        }
    }
}

pub(super) fn resolve_xfl_morph_hints(
    raw_hints: &[RawXflMorphHint],
    start_elements: &[Element],
    end_elements: &[Element],
) -> Vec<ShapeHint> {
    if raw_hints.is_empty() {
        return Vec::new();
    }
    let start_geom = ShapeHintGeometry::from_elements(start_elements);
    let end_geom = ShapeHintGeometry::from_elements(end_elements);
    let mut seen_labels = HashSet::new();
    let mut out = Vec::new();
    for raw in raw_hints {
        if out.len() >= ShapeHint::MAX_HINTS {
            break;
        }
        let label = raw
            .label
            .filter(|c| c.is_ascii_lowercase() && !seen_labels.contains(c))
            .or_else(|| ('a'..='z').find(|c| !seen_labels.contains(c)));
        let Some(label) = label else {
            break;
        };
        seen_labels.insert(label);
        out.push(ShapeHint {
            label,
            start_uv: resolve_xfl_hint_coord(start_geom.as_ref(), &raw.start),
            end_uv: resolve_xfl_hint_coord(end_geom.as_ref(), &raw.end),
        });
    }
    out.sort_by_key(|h| h.label);
    out
}

pub(super) fn normalize_xfl_playback(symbols: &mut [Symbol], report: &mut ImportReport) {
    let frame_counts: HashMap<String, u32> = symbols
        .iter()
        .map(|s| (s.id.clone(), s.timeline.frame_count.max(1)))
        .collect();
    for sym in symbols.iter_mut() {
        let sym_name = sym.name.clone();
        for layer in &mut sym.timeline.layers {
            for key in &mut layer.keyframes {
                for el in &mut key.elements {
                    if let ElementKind::Instance {
                        symbol_id,
                        playback,
                    } = &mut el.kind
                        && let Some(&fc) = frame_counts.get(symbol_id)
                    {
                        let max_idx = fc.saturating_sub(1);
                        match playback {
                            InstancePlayback::SingleFrame(f)
                            | InstancePlayback::LoopFrom(f)
                            | InstancePlayback::PlayOnceFrom(f)
                                if *f >= fc =>
                            {
                                *f = max_idx;
                                report.warning(
                                    "xfl_playback_frame_clamped",
                                    format!("{sym_name}:frame {}", key.frame + 1),
                                    format!(
                                        "Graphic instance firstFrame clamped to last frame ({}) of target symbol.",
                                        max_idx + 1
                                    ),
                                );
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}

pub(super) fn is_pure_vector_mask_symbol(
    sym_id: &str,
    symbols: &HashMap<&str, &Symbol>,
    visiting: &mut HashSet<String>,
) -> bool {
    let Some(sym) = symbols.get(sym_id) else {
        return false;
    };
    if !visiting.insert(sym_id.to_owned()) {
        return false;
    }
    let ok = sym
        .timeline
        .layers
        .iter()
        .filter(|l| l.properties.role != LayerRole::Guide)
        .all(|l| {
            l.properties.role != LayerRole::Mask
                && l.keyframes.iter().flat_map(|k| &k.elements).all(|el| {
                    matches!(
                        el.kind,
                        ElementKind::Rectangle { .. }
                            | ElementKind::Ellipse { .. }
                            | ElementKind::Path { .. }
                            | ElementKind::PaintedPath { .. }
                    ) || matches!(
                        &el.kind,
                        ElementKind::Instance { symbol_id, .. }
                            if is_pure_vector_mask_symbol(symbol_id, symbols, visiting)
                    )
                })
        });
    visiting.remove(sym_id);
    ok
}

pub(super) fn normalize_xfl_masks(project: &mut Project, report: &mut ImportReport) {
    let sym_map: HashMap<&str, &Symbol> =
        project.symbols.iter().map(|s| (s.id.as_str(), s)).collect();
    let mut invalid_masks_per_symbol: Vec<HashSet<String>> =
        Vec::with_capacity(project.symbols.len());
    for sym in &project.symbols {
        let mut bad = HashSet::new();
        for layer in &sym.timeline.layers {
            if layer.properties.role != LayerRole::Mask {
                continue;
            }
            let valid = layer.keyframes.iter().flat_map(|k| &k.elements).all(|el| {
                matches!(
                    el.kind,
                    ElementKind::Rectangle { .. }
                        | ElementKind::Ellipse { .. }
                        | ElementKind::Path { .. }
                        | ElementKind::PaintedPath { .. }
                ) || matches!(
                    &el.kind,
                    ElementKind::Instance { symbol_id, .. }
                        if is_pure_vector_mask_symbol(symbol_id, &sym_map, &mut HashSet::new())
                )
            });
            if !valid {
                bad.insert(layer.id.clone());
            }
        }
        invalid_masks_per_symbol.push(bad);
    }
    for (sym, bad_ids) in project.symbols.iter_mut().zip(invalid_masks_per_symbol) {
        if bad_ids.is_empty() {
            continue;
        }
        for layer in &mut sym.timeline.layers {
            if bad_ids.contains(&layer.id) {
                layer.properties.role = LayerRole::Normal;
                report.warning(
                    "xfl_mask_approximated",
                    format!("{}/{}", sym.name, layer.name),
                    "XFL mask layer contains non-vector or nested-mask elements and was converted to a normal layer.",
                );
            }
            if layer
                .properties
                .mask
                .as_ref()
                .is_some_and(|m| bad_ids.contains(m))
            {
                layer.properties.mask = None;
            }
        }
    }
}
