use super::*;

impl Context<'_> {
    pub(super) fn ensure_placeholder_symbol(&mut self) {
        if !self.generated.iter().any(|g| g.id == PLACEHOLDER) {
            self.generated.push(group_symbol(
                PLACEHOLDER,
                "Unsupported source object (see import report)".into(),
                vec![],
            ));
        }
    }
    pub(super) fn placeholder_element(&mut self, id: String, transform: Transform) -> Element {
        self.ensure_placeholder_symbol();
        Element {
            id,
            name: String::new(),
            transform,
            kind: ElementKind::Instance {
                symbol_id: PLACEHOLDER.into(),
                playback: InstancePlayback::SingleFrame(0),
            },
            scripts: vec![],
            accessibility: None,
            ..Element::default()
        }
    }
    pub(super) fn frame_sound(
        &mut self,
        s: &BinaryTimelineStream,
        f: &TimelineFrame,
        at: &str,
    ) -> Option<FrameSound> {
        let id = f.sound_media_id.unwrap_or(0);
        if id == 0 {
            return None;
        }
        let Some(asset) = self.sounds.get(&id).cloned() else {
            self.report.warning(
                "fla_frame_sound",
                at,
                format!(
                    "Timeline sound references media #{id}, which could not be imported; the sound and its sync settings remain in the embedded source."
                ),
            );
            return None;
        };
        let sync = match f.sound_sync.unwrap_or(0) {
            0 => SoundSync::Event,
            1 => SoundSync::Start,
            2 => SoundSync::Stop,
            3 => SoundSync::Stream,
            other => {
                self.report.warning(
                    "fla_frame_sound",
                    at,
                    format!("Unknown frame-sound sync mode {other}; Event sync is used."),
                );
                SoundSync::Event
            }
        };
        let raw_loop = f.sound_loop_count.unwrap_or(1);
        let (loops, loop_forever) = if raw_loop == 0x7fff {
            (1, true)
        } else {
            let n = u32::from(raw_loop).max(1);
            if n > FrameSound::MAX_LOOPS {
                self.report.warning(
                    "fla_frame_sound",
                    at,
                    format!(
                        "Frame-sound repeat count {n} exceeds {}; clamped to {}.",
                        FrameSound::MAX_LOOPS,
                        FrameSound::MAX_LOOPS
                    ),
                );
                (FrameSound::MAX_LOOPS, false)
            } else {
                (n, false)
            }
        };
        let envelope_points = || -> Option<Vec<(u32, u16, u16)>> {
            let count = usize::from(f.sound_envelope_count.unwrap_or(0));
            if count == 0 {
                return Some(vec![]);
            }
            let span = f.sound_envelope?;
            let raw = s.original_bytes.get(span.start..span.end)?;
            if raw.len() != count * 8 {
                return None;
            }
            Some(
                raw.as_chunks::<8>()
                    .0
                    .iter()
                    .map(|c| {
                        (
                            u32::from_le_bytes([c[0], c[1], c[2], c[3]]),
                            u16::from_le_bytes([c[4], c[5]]),
                            u16::from_le_bytes([c[6], c[7]]),
                        )
                    })
                    .collect(),
            )
        };
        let full_span_fade = |p0: u32, p1: u32| -> bool {
            let dur = u64::from(asset.duration_44);
            p0 <= 64
                && if dur > 0 {
                    u64::from(p1).abs_diff(dur) <= 64
                } else {
                    p1 > 0
                }
        };
        let classify_envelope = |pts: &[(u32, u16, u16)]| -> Option<SoundEffect> {
            if pts.is_empty() || pts.iter().all(|&(_, l, r)| l == 0x8000 && r == 0x8000) {
                Some(SoundEffect::None)
            } else if pts.iter().all(|&(_, l, r)| l == 0x8000 && r == 0) {
                Some(SoundEffect::LeftChannel)
            } else if pts.iter().all(|&(_, l, r)| l == 0 && r == 0x8000) {
                Some(SoundEffect::RightChannel)
            } else if let [(p0, 0, 0), (p1, 0x8000, 0x8000)] = *pts
                && full_span_fade(p0, p1)
            {
                Some(SoundEffect::FadeIn)
            } else if let [(p0, 0x8000, 0x8000), (p1, 0, 0)] = *pts
                && full_span_fade(p0, p1)
            {
                Some(SoundEffect::FadeOut)
            } else if let [(p0, 0x8000, 0), (p1, 0, 0x8000)] = *pts
                && full_span_fade(p0, p1)
            {
                Some(SoundEffect::FadeLeftToRight)
            } else if let [(p0, 0, 0x8000), (p1, 0x8000, 0)] = *pts
                && full_span_fade(p0, p1)
            {
                Some(SoundEffect::FadeRightToLeft)
            } else {
                None
            }
        };
        let (effect, envelope) = match f.sound_effect {
            Some(1) => (SoundEffect::LeftChannel, None),
            Some(2) => (SoundEffect::RightChannel, None),
            Some(3) => (SoundEffect::FadeLeftToRight, None),
            Some(4) => (SoundEffect::FadeRightToLeft, None),
            Some(5) => (SoundEffect::FadeIn, None),
            Some(6) => (SoundEffect::FadeOut, None),
            Some(0) | Some(7) | None => match envelope_points() {
                Some(mut pts) => match classify_envelope(&pts) {
                    Some(preset) => (preset, None),
                    None => {
                        pts.sort_by_key(|p| p.0);
                        if pts.len() > FrameSound::MAX_ENVELOPE_POINTS {
                            self.report.warning(
                                "fla_frame_sound",
                                at,
                                format!(
                                    "Custom frame-sound envelope has {} points; truncated to {} points and full envelope data remains in the embedded source.",
                                    pts.len(),
                                    FrameSound::MAX_ENVELOPE_POINTS
                                ),
                            );
                            pts.truncate(FrameSound::MAX_ENVELOPE_POINTS);
                        }
                        let max_mark = pts.iter().map(|p| u64::from(p.0)).max().unwrap_or(0);
                        let denom = u64::from(asset.duration_44).max(max_mark);
                        if pts.iter().any(|&(_, l, r)| l > 0x8000 || r > 0x8000) {
                            self.report.warning(
                                "fla_frame_sound",
                                at,
                                "Custom frame-sound envelope gain above 100% was clamped to 1.0; original envelope levels remain in the embedded source.",
                            );
                        }
                        let mut prev_pos = 0.0_f64;
                        let points = pts
                            .into_iter()
                            .map(|(mark44, l, r)| {
                                let pos = if denom == 0 {
                                    0.0
                                } else {
                                    (u64::from(mark44) as f64 / denom as f64).clamp(0.0, 1.0)
                                }
                                .max(prev_pos);
                                prev_pos = pos;
                                let left = (f64::from(l) / 32768.0).clamp(0.0, 1.0);
                                let right = (f64::from(r) / 32768.0).clamp(0.0, 1.0);
                                [pos, left, right]
                            })
                            .collect();
                        (SoundEffect::Custom, Some(points))
                    }
                },
                None => {
                    self.report.warning(
                        "fla_frame_sound",
                        at,
                        "Corrupt frame-sound envelope span could not be decoded; imported without gain automation and original bytes remain in the embedded source.",
                    );
                    (SoundEffect::None, None)
                }
            },
            Some(other) => {
                self.report.warning(
                    "fla_frame_sound",
                    at,
                    format!(
                        "Unknown frame-sound effect {other}; imported without an effect preset."
                    ),
                );
                (SoundEffect::None, None)
            }
        };
        let in_trimmed = f.sound_in_point.unwrap_or(0) > 0;
        let out_raw = f.sound_out_point.unwrap_or(0);
        let out_trimmed = out_raw > 0
            && out_raw < 0x3fff_ffff
            && asset.duration_44 > 0
            && out_raw.saturating_add(8) < asset.duration_44;
        let mut trim_invalid = false;
        let in_point = if in_trimmed {
            let sec = f64::from(f.sound_in_point.unwrap_or(0)) / 44_100.0;
            if sec.is_finite() && (0.0..=FrameSound::MAX_SOUND_TRIM_SECONDS).contains(&sec) {
                Some(sec)
            } else {
                trim_invalid = true;
                None
            }
        } else {
            None
        };
        let out_point = if out_trimmed {
            let sec = f64::from(out_raw) / 44_100.0;
            if sec.is_finite()
                && (0.0..=FrameSound::MAX_SOUND_TRIM_SECONDS).contains(&sec)
                && in_point.is_none_or(|inp| sec > inp)
            {
                Some(sec)
            } else {
                trim_invalid = true;
                None
            }
        } else {
            None
        };
        if trim_invalid {
            self.report.warning(
                "fla_frame_sound",
                at,
                "Frame-sound in/out trim points are outside the supported range and were omitted; trim offsets remain in the embedded source.",
            );
        }
        Some(FrameSound {
            resource: asset.resource_id,
            sync,
            loops,
            loop_forever,
            effect,
            envelope,
            in_point,
            out_point,
        })
    }
    pub(super) fn actions(
        &mut self,
        stream: &BinaryTimelineStream,
        actions: Option<&TimelineActions>,
    ) -> Result<Option<Script>, String> {
        let Some(a) = actions else { return Ok(None) };
        let loc = format!("stream #{} @0x{:x}", stream.index, a.encoded.start);
        match a.presence {
            TimelineActionPresence::Empty => Ok(None),
            TimelineActionPresence::Source if a.complete => Ok(Some(Script {
                language: self.language,
                source: read_action_source_string(
                    &stream.original_bytes,
                    a.revision,
                    a.source.as_ref().ok_or("Missing action source")?,
                )?,
            })),
            TimelineActionPresence::Source => {
                if let Some(span) = a.source.as_ref()
                    && let Ok(source) =
                        read_action_source_string(&stream.original_bytes, a.revision, span)
                {
                    self.report.warning(
                        "fla_actions_partial",
                        &loc,
                        "Source action record stopped after reading its script text; the recovered script is preserved.",
                    );
                    Ok((!source.trim().is_empty()).then_some(Script {
                        language: self.language,
                        source,
                    }))
                } else {
                    self.report.warning(
                        "fla_actions_partial",
                        &loc,
                        "Source action record stopped before its script string completed; the incomplete record remains in the embedded source.",
                    );
                    Ok(None)
                }
            }
            // Historical and compiled-mode command lists carry no source text;
            // the reader's ActionScript 1 translation stands in for it.
            TimelineActionPresence::Historical | TimelineActionPresence::Compiled if a.complete => {
                let decoded = a
                    .decoded
                    .as_ref()
                    .ok_or("Missing decoded action commands")?;
                if decoded.needs_review {
                    self.report.warning(
                        "fla_actions_translated",
                        loc,
                        format!(
                            "ActionScript 1 translation of legacy action commands needs review: {} of {} commands remain as marked comments (or a block was repaired); the original commands stay in the embedded FLA.",
                            decoded.untranslated.len(),
                            decoded.commands.len()
                        ),
                    );
                }
                Ok((!decoded.source.trim().is_empty()).then(|| Script {
                    language: ScriptLanguage::ActionScript1,
                    source: decoded.source.clone(),
                }))
            }
            TimelineActionPresence::Historical | TimelineActionPresence::Compiled => {
                if let Some(decoded) = a.decoded.as_ref()
                    && let Some((mut source, complete_count, untranslated_count)) =
                        crate::legacy::mfc::translate_partial_actions(
                            &stream.original_bytes,
                            decoded,
                        )
                {
                    source.push_str(&format!(
                        "// NeoFlash: action record stopped at @0x{:x} after {complete_count} complete command(s)\n",
                        a.encoded.end
                    ));
                    self.report.warning(
                        "fla_actions_partial",
                        &loc,
                        format!(
                            "Legacy action command list stopped before completion; {complete_count} complete command(s) before the stop were translated to ActionScript 1 ({untranslated_count} kept as comments)."
                        ),
                    );
                    Ok(Some(Script {
                        language: ScriptLanguage::ActionScript1,
                        source,
                    }))
                } else {
                    self.report.warning(
                        "fla_actions_partial",
                        &loc,
                        "Legacy action command list stopped before any complete statement could be translated; the raw bytes remain in the embedded source.",
                    );
                    Ok(None)
                }
            }
            TimelineActionPresence::Unknown => {
                self.report.warning(
                    "fla_actions_partial",
                    &loc,
                    "Action record stopped before its encoding mode was identified; the raw bytes remain in the embedded source.",
                );
                Ok(None)
            }
        }
    }
    pub(super) fn timeline(&mut self, s: &BinaryTimelineStream) -> Result<Timeline, String> {
        const MAX_TIMELINE_FRAMES: u32 = 1_000_000;
        let Some(page) = s.root.and_then(|r| object(s, r).ok()) else {
            if !self.stream_stops.contains(&s.index) {
                self.report.warning(
                    "fla_timeline_empty",
                    format!("stream #{}", s.index),
                    "Timeline stream contains no readable root page record; imported as an empty 1-frame timeline.",
                );
            }
            return Ok(empty_timeline(&format!("fla-layer-{}-0", s.index)));
        };
        let mut timeline = Timeline {
            guides: vec![],
            frame_count: 1,
            layers: vec![],
            scripts: vec![],
        };
        // Layer source IDs and their total authored length, for end markers.
        let mut sources = Vec::new();
        for id in &page.children {
            let o = object(s, *id)?;
            let default_layer = TimelineLayer::default();
            let (l, name) = match &o.payload {
                Some(TimelinePayload::Layer(l)) => {
                    (l, optional_text(&s.original_bytes, l.name.as_ref())?)
                }
                None if o.class == TimelineClass::Layer && !o.children.is_empty() => {
                    let fallback = format!("Layer {}", timeline.layers.len() + 1);
                    self.report.warning(
                        "fla_layer_partial",
                        format!("stream #{} object #{id}", s.index),
                        format!(
                            "Layer record stopped before its settings after reading frame records; recovered frames are imported on default normal layer '{fallback}'."
                        ),
                    );
                    (&default_layer, fallback)
                }
                _ => {
                    self.report.warning("fla_layer_skipped",format!("stream #{} object #{id}",s.index),if o.complete {"A timeline child is not a layer record; it remains in the embedded source."} else {"The layer record stops before its settings; it remains in the embedded source."});
                    continue;
                }
            };
            if !o.complete && o.children.is_empty() {
                self.report.warning(
                    "fla_layer_skipped",
                    format!("stream #{} layer {name}", s.index),
                    "The layer record stops before any frame; it remains in the embedded source.",
                );
                continue;
            }
            let layer_id = lid(s.index, *id);
            let state = l
                .state_flags
                .and_then(|v| s.original_bytes.get(v.start..v.end));
            let (editor_visible, locked) = match state {
                // Older CPicLayer revisions encode hidden, locked, normal and current in one byte.
                Some([legacy]) => (*legacy != 0, *legacy == 1),
                Some([_, hidden, locked]) => (*hidden == 0, *locked != 0),
                _ => (l.legacy_flag != Some(0), l.legacy_flag == Some(1)),
            };
            let layer_relation_target =
                |layer_obj_id: u32, relation: TimelineRelation| -> Option<u32> {
                    s.pointers
                        .iter()
                        .find(|p| p.source == Some(layer_obj_id) && p.relation == relation)
                        .and_then(|p| p.target)
                };
            let layer_primary_related = |layer_obj_id: u32| -> Option<u32> {
                layer_relation_target(layer_obj_id, TimelineRelation::RelatedLayer)
            };
            let is_pre_r9_legacy_mask = |layer_obj_id: u32, layer: &TimelineLayer| -> bool {
                let rev = layer.revision.unwrap_or(13);
                if rev >= 9
                    || !matches!(layer.layer_type, Some(0 | 1) | None)
                    || layer.mask_or_open.unwrap_or(0) == 0
                {
                    return false;
                }
                let points_to_parent = layer_primary_related(layer_obj_id).is_some_and(|t| {
                    t != layer_obj_id
                        && matches!(
                            object(s, t).map(|obj| &obj.payload),
                            Ok(Some(TimelinePayload::Layer(parent)))
                                if matches!(parent.layer_type, Some(1 | 3 | 4))
                        )
                });
                if points_to_parent {
                    return false;
                }
                page.children.iter().copied().any(|sib_id| {
                    sib_id != layer_obj_id
                        && (layer_primary_related(sib_id) == Some(layer_obj_id)
                            || layer_relation_target(sib_id, TimelineRelation::LegacyRelatedLayer)
                                == Some(layer_obj_id))
                        && matches!(
                            object(s, sib_id).map(|obj| &obj.payload),
                            Ok(Some(TimelinePayload::Layer(sib)))
                                if matches!(sib.layer_type, Some(0 | 2 | 5) | None)
                        )
                })
            };
            let effective_layer_type = |layer_obj_id: u32, layer: &TimelineLayer| -> Option<u8> {
                if is_pre_r9_legacy_mask(layer_obj_id, layer) {
                    Some(4)
                } else {
                    layer.layer_type
                }
            };
            let role = match effective_layer_type(*id, l).unwrap_or(0) {
                0 | 2 | 5 => LayerRole::Normal,
                1 => LayerRole::Guide,
                3 => LayerRole::Folder,
                4 => LayerRole::Mask,
                other => {
                    self.report.warning(
                        "fla_layer_type",
                        format!("stream #{} layer {name}", s.index),
                        format!("Unknown layer type {other}; imported as a normal layer."),
                    );
                    LayerRole::Normal
                }
            };
            let rev = l.revision.unwrap_or(13);
            let primary_related = layer_primary_related(*id);
            let legacy_related = s
                .pointers
                .iter()
                .find(|p| {
                    p.source == Some(*id) && p.relation == TimelineRelation::LegacyRelatedLayer
                })
                .and_then(|p| p.target);
            let is_legacy_downward_child_link = |target_id: u32| -> bool {
                target_id != *id
                    && page.children.contains(&target_id)
                    && matches!(
                        object(s, target_id).map(|obj| &obj.payload),
                        Ok(Some(TimelinePayload::Layer(child)))
                            if !is_pre_r9_legacy_mask(target_id, child)
                                && matches!(child.layer_type, Some(0 | 2) | None)
                    )
            };
            let related = if (7..=8).contains(&rev) && legacy_related.is_some() {
                legacy_related
            } else if rev < 9
                && (l.layer_type == Some(1) || is_pre_r9_legacy_mask(*id, l))
                && primary_related.is_some_and(is_legacy_downward_child_link)
            {
                // Pre-R9 Guide and Mask layers store a historical downward pointer to
                // their guided/masked child layer; the child's own RelatedLayer links back.
                None
            } else {
                primary_related
            };
            let mut properties = LayerProperties {
                role,
                outline: l.outline.unwrap_or(0) != 0,
                outline_color: l.outline_color_word.map(|v| {
                    let b = v.to_le_bytes();
                    [b[0], b[1], b[2]]
                }),
                collapsed: role == LayerRole::Folder && rev >= 9 && l.mask_or_open == Some(0),
                ..Default::default()
            };
            if let Some(parent) = related {
                let parent_type = match object(s, parent).map(|o| &o.payload) {
                    Ok(Some(TimelinePayload::Layer(p)))
                        if parent != *id && page.children.contains(&parent) =>
                    {
                        effective_layer_type(parent, p)
                    }
                    _ => None,
                };
                let parent_id = lid(s.index, parent);
                match parent_type {
                    Some(4) if role == LayerRole::Normal => properties.mask = Some(parent_id),
                    Some(3) => properties.parent = Some(parent_id),
                    Some(1) if role == LayerRole::Normal => {
                        properties.motion_guide = Some(parent_id)
                    }
                    _ => self.report.warning("fla_layer_relation",format!("stream #{} layer {name}",s.index),"The layer's mask/guide/folder relation cannot be resolved; the layer is imported without it."),
                }
            }
            if l.animation_type.unwrap_or(0) != 0 {
                self.report.warning(
                    "fla_layer_animation",
                    format!("stream #{} layer {name}", s.index),
                    "Original motion-layer or IK metadata remains in the source. Supported motion-object tracks are sampled at each authored frame.",
                );
            }
            self.provenance.push(serde_json::json!({"stream":s.index,"object":id,"target":layer_id,"editor_visible":editor_visible,"source":o.encoded}));
            let mut layer = Layer {
                id: layer_id,
                name,
                visible: editor_visible,
                locked,
                keyframes: vec![],
                properties,
            };
            let is_scene_stream = self.scene_stream_indices.contains(&s.index);
            let layer_has_tween = l.animation_type.unwrap_or(0) != 0
                || o.children
                    .iter()
                    .filter_map(|fid| object(s, *fid).ok())
                    .any(|fr| {
                        matches!(
                            &fr.payload,
                            Some(TimelinePayload::Frame(tf))
                                if (tf.key_mode.unwrap_or(0) & 255) != 0
                                    || tf.morph_shape.is_some()
                                    || tf.motion_xml.as_ref().is_some_and(|x| x.data.end > x.data.start)
                        )
                    });
            let mut cursor = 0u32;
            let mut motion_keys = Vec::new();
            let mut pending_morphs: Vec<(usize, u32)> = Vec::new();
            let mut pending_custom_eases: Vec<(usize, TimelineFrame, String)> = Vec::new();
            for frame_id in &o.children {
                if cursor >= MAX_TIMELINE_FRAMES {
                    self.report.warning(
                        "fla_frame_limit",
                        format!("stream #{} layer {}", s.index, layer.name),
                        format!(
                            "Layer exceeds the {MAX_TIMELINE_FRAMES}-frame timeline limit; trailing keyframes are omitted."
                        ),
                    );
                    break;
                }
                let frame = object(s, *frame_id)?;
                if frame.class != TimelineClass::Frame {
                    self.report.warning(
                        "fla_frame_skipped",
                        format!("stream #{} layer {}", s.index, layer.name),
                        "A layer child is not a frame record; it remains in the embedded source.",
                    );
                    continue;
                }
                let default_frame = TimelineFrame::default();
                let f = match &frame.payload {
                    Some(TimelinePayload::Frame(f)) => f.as_ref(),
                    _ => &default_frame,
                };
                if !frame.complete {
                    let has_recoverable = !frame.children.is_empty()
                        || frame.shape_index.is_some()
                        || f.duration.is_some()
                        || f.label.is_some()
                        || f.sound_media_id.unwrap_or(0) != 0
                        || f.actions
                            .as_ref()
                            .is_some_and(|a| a.presence != TimelineActionPresence::Empty);
                    if !has_recoverable {
                        break;
                    }
                    if frame.payload.is_none() || f.duration.is_none() {
                        self.report.warning(
                            "fla_frame_partial",
                            format!(
                                "stream #{} layer {} frame {}",
                                s.index,
                                layer.name,
                                cursor + 1
                            ),
                            "Keyframe stopped before all frame properties were read; decoded elements, shapes and scripts before the stop are preserved with a 1-frame fallback duration.",
                        );
                    }
                }
                if f.duration == Some(0) {
                    // Flash writes empty zero-length keys at the start of some
                    // layers; they occupy no frame, so the next key starts here too.
                    if !frame.complete
                        || !frame.children.is_empty()
                        || frame.shape_index.is_some_and(|index| {
                            s.shapes
                                .get(index)
                                .is_none_or(|shape| !shape.record.edges.is_empty())
                        })
                        || f.label
                            .as_ref()
                            .is_some_and(|label| label.data.start != label.data.end)
                        || f.sound_media_id.unwrap_or(0) != 0
                        || f.morph_shape.is_some()
                        || f.motion_xml
                            .as_ref()
                            .is_some_and(|xml| xml.data.start != xml.data.end)
                        || f.actions
                            .as_ref()
                            .is_some_and(|a| a.presence != TimelineActionPresence::Empty)
                    {
                        self.report.warning("fla_frame_duration",format!("stream #{} layer {} frame {}",s.index,layer.name,cursor+1),"A zero-length keyframe with content occupies no frame in Flash and is skipped; it remains in the embedded source.");
                    }
                    if !frame.complete {
                        break;
                    }
                    continue;
                }
                let at = format!(
                    "stream #{} layer {} frame {}",
                    s.index,
                    layer.name,
                    cursor + 1
                );
                let sound = self.frame_sound(s, f, &at);
                let mut frame_videos = Vec::new();
                for &cid in &frame.children {
                    let Ok(co) = object(s, cid) else { continue };
                    let Some(TimelinePayload::Video(v)) = &co.payload else {
                        continue;
                    };
                    let Some(mid) = v.media_id else { continue };
                    let Some(asset) = self.videos.get(&mid) else {
                        continue;
                    };
                    let Some(res_id) = asset.resource_id.as_ref() else {
                        continue;
                    };
                    frame_videos.push(TimelineVideoSpan {
                        resource: res_id.clone(),
                        sync: if asset.sync_video {
                            VideoSyncMode::Stream
                        } else {
                            VideoSyncMode::Event
                        },
                        muted: !asset.import_audio,
                        deployment: timeline_video::VideoDeploymentMode::Embedded,
                        ..Default::default()
                    });
                }
                if frame_videos.len() > 1 {
                    self.report.warning(
                        "fla_media_not_imported",
                        &at,
                        format!(
                            "Keyframe places {} embedded video streams simultaneously; the first stream is bound to the keyframe video track and {} additional stream placement(s) remain as stage viewports.",
                            frame_videos.len(),
                            frame_videos.len() - 1
                        ),
                    );
                }
                let video = frame_videos.into_iter().next();
                if let Some(ext) = &f.pose_extension
                    && (ext.matrix_words != [65536, 0, 0, 65536, 0, 0]
                        || ext.color_transform.multipliers != [256; 4]
                        || ext.color_transform.offsets != [0; 4]
                        || ext.color_transform.effect_type.unwrap_or(0) != 0)
                {
                    self.report.warning(
                        "fla_layer_animation",
                        &at,
                        "Historical frame pose extension is retained in the embedded source; the keyframe uses its stored pose.",
                    );
                }
                let mut elements = Vec::new();
                let mut bitmap_fills = Vec::new();
                let mut appearances = Vec::new();
                let mut vector_effects = Vec::new();
                let provenance_start = self.provenance.len();
                for (slot, child) in frame.children.iter().enumerate() {
                    elements.push(self.element_or_placeholder(
                        s,
                        *child,
                        &format!("fla-{}-{}-{}", s.index, id, child),
                        0,
                    ));
                    self.provenance.push(serde_json::json!({"stream":s.index,"object":child,"layer":layer.id,"frame":cursor,"slot":slot}));
                }
                if let Some(index) = frame.shape_index {
                    let frame_is_shape_tween = matches!(ease(f), Ok((Some(_), TweenKind::Shape)))
                        || layer.keyframes.last().is_some_and(|prev| {
                            prev.tween.is_some() && prev.tween_kind == TweenKind::Shape
                        });
                    let shape_id = format!("fla-{}-{}-shape-{frame_id}", s.index, id);
                    let shape_record = &s.shapes[index].record;
                    match shape::convert_shape_with_effects_opts(
                        shape_record,
                        &shape_id,
                        &self.bitmaps,
                        self.report,
                        &format!("stream #{} frame {cursor}", s.index),
                        !frame_is_shape_tween,
                        !is_scene_stream || layer_has_tween,
                    ) {
                        Ok(converted) => {
                            elements.extend(converted.elements);
                            bitmap_fills.extend(converted.bitmap_fills);
                            appearances.extend(converted.appearances);
                            vector_effects.extend(converted.vector_effects);
                        }
                        Err(e) => {
                            self.report.warning(
                                "fla_shape_skipped",
                                &at,
                                format!(
                                    "Frame artwork was not converted: {e}. A placeholder keeps its slot and the original shape remains in the embedded source."
                                ),
                            );
                            if !shape_record.edges.is_empty() {
                                elements
                                    .push(self.placeholder_element(shape_id, Transform::default()));
                            }
                        }
                    }
                }
                let (tween, tween_kind, defer_multi_segment_ease) = match ease_with_split_flag(f) {
                    Ok(value) => value,
                    Err(e) => {
                        self.report.warning(
                            "fla_tween_easing",
                            &at,
                            format!("{e}; the tween uses its classic acceleration instead."),
                        );
                        (
                            Some(Easing::Flash {
                                acceleration: f.acceleration.unwrap_or(0) as i16 as f64,
                            }),
                            TweenKind::Classic,
                            false,
                        )
                    }
                };
                let is_classic_tween = tween.is_some() && tween_kind == TweenKind::Classic;
                let mode = f.key_mode.unwrap_or(0);
                let orient_to_path = is_classic_tween && (mode & 0x100) != 0;
                let sync_symbols = is_classic_tween && (mode & 0x800) != 0;
                let snap_to_guide = is_classic_tween && (mode & 0x1000) != 0;
                let raw_rot_mode = f.rotation_mode.unwrap_or(0);
                let raw_rot_turns = f.rotation_count.unwrap_or(0);
                let (classic_rotate_dir, classic_rotate_turns) = if is_classic_tween {
                    if raw_rot_mode > 2 || raw_rot_turns > Keyframe::MAX_ROTATE_TURNS {
                        self.report.warning(
                            "fla_tween_rotation",
                            &at,
                            "Directed tween rotation exceeds supported mode/turn bounds and was clamped.",
                        );
                    }
                    let dir = match raw_rot_mode {
                        1 => ClassicRotateDirection::Cw,
                        2 => ClassicRotateDirection::Ccw,
                        _ => ClassicRotateDirection::Auto,
                    };
                    (dir, raw_rot_turns.min(Keyframe::MAX_ROTATE_TURNS))
                } else {
                    (ClassicRotateDirection::Auto, 0)
                };
                if let Some(previous) = layer.keyframes.last_mut() {
                    // An explicit classic tween may pair library symbols (MovieClip, Graphic,
                    // Button, including playback/first-frame transitions and symbol swaps),
                    // generated group symbols (with member placement transforms hoisted onto
                    // the outer group instance), or primitive/bitmap/text/video elements.
                    // Empty, multi-element, or unresolved placeholder endpoints hold their
                    // keyframe poses and emit `fla_tween_dropped` (unless the span is 1 frame
                    // wide with zero in-between frames before/from a blank keyframe).
                    if previous.tween.is_some()
                        && previous.tween_kind == TweenKind::Classic
                        && cursor > previous.frame + 1
                    {
                        try_wrap_tweened_frame_shapes_into_group(
                            &mut self.generated,
                            previous,
                            &mut elements,
                            &mut bitmap_fills,
                            &mut appearances,
                            &mut vector_effects,
                            s.index,
                            *id,
                            *frame_id,
                        );
                    }
                    if previous.tween.is_some()
                        && previous.tween_kind == TweenKind::Classic
                        && previous.elements.len() == 1
                        && elements.len() == 1
                    {
                        hoist_generated_group_transform(
                            &mut self.generated,
                            &mut previous.elements[0],
                        );
                        hoist_generated_group_transform(&mut self.generated, &mut elements[0]);
                        deduplicate_tweened_group_endpoint(
                            &mut self.generated,
                            &mut self.provenance,
                            &previous.elements[0],
                            &mut elements[0],
                        );
                    }
                    let is_tweenable_symbol = |sym_id: &str| {
                        self.library.values().any(|id| id == sym_id)
                            || ((sym_id.ends_with("-group")
                                || sym_id.starts_with("fla-compiled-clip-"))
                                && self.generated.iter().any(|s| s.id == sym_id))
                    };
                    let classic_tween_pair = previous.tween.is_some()
                        && previous.tween_kind == TweenKind::Classic
                        && previous.elements.len() == 1
                        && elements.len() == 1
                        && !placement_matches(&previous.elements[0], &elements[0])
                        && match (&previous.elements[0].kind, &elements[0].kind) {
                            (
                                ElementKind::Instance { symbol_id: a, .. },
                                ElementKind::Instance { symbol_id: b, .. },
                            ) => is_tweenable_symbol(a) && is_tweenable_symbol(b),
                            (ElementKind::Rectangle { .. }, ElementKind::Rectangle { .. })
                            | (ElementKind::Ellipse { .. }, ElementKind::Ellipse { .. })
                            | (ElementKind::Bitmap { .. }, ElementKind::Bitmap { .. })
                            | (ElementKind::TextBox { .. }, ElementKind::TextBox { .. })
                            | (ElementKind::Video { .. }, ElementKind::Video { .. }) => true,
                            _ => false,
                        };
                    for (a, b) in previous.elements.iter().zip(elements.iter_mut()) {
                        if placement_matches(a, b) || classic_tween_pair {
                            let source_id = std::mem::replace(&mut b.id, a.id.clone());
                            for mapping in &mut self.provenance[provenance_start..] {
                                if mapping["target"].as_str() == Some(source_id.as_str()) {
                                    mapping["target"] = serde_json::json!(b.id);
                                }
                            }
                        }
                    }
                    if previous.tween.is_some()
                        && previous.tween_kind == TweenKind::Classic
                        && (previous.elements.len() != 1
                            || elements.len() != 1
                            || !elements.iter().any(|e| e.id == previous.elements[0].id))
                    {
                        let span_frames = cursor.saturating_sub(previous.frame);
                        let valid_single_or_blank = |els: &[Element]| match els {
                            [] => true,
                            [el] => match &el.kind {
                                ElementKind::Instance { symbol_id, .. } => {
                                    is_tweenable_symbol(symbol_id)
                                }
                                ElementKind::Rectangle { .. }
                                | ElementKind::Ellipse { .. }
                                | ElementKind::Bitmap { .. }
                                | ElementKind::TextBox { .. }
                                | ElementKind::Video { .. } => true,
                                ElementKind::PaintedPath { commands, .. }
                                | ElementKind::Path { commands, .. } => !commands.is_empty(),
                                _ => false,
                            },
                            _ => false,
                        };
                        let noop_single_frame_boundary = span_frames <= 1
                            && (previous.elements.is_empty() || elements.is_empty())
                            && valid_single_or_blank(&previous.elements)
                            && valid_single_or_blank(&elements);
                        previous.tween = None;
                        previous.orient_to_path = false;
                        previous.sync_symbols = false;
                        previous.snap_to_guide = false;
                        previous.classic_rotate_dir = ClassicRotateDirection::Auto;
                        previous.classic_rotate_turns = 0;
                        if !noop_single_frame_boundary {
                            self.report.warning("fla_tween_dropped",&at,"This classic motion tween has empty, multiple or unsupported placement endpoints, so the frames hold their keyframe poses.");
                        }
                    }
                }
                if let Some(script) = self.actions(s, f.actions.as_ref())? {
                    timeline.scripts.push(FrameScript {
                        layer: None,
                        frame: cursor,
                        script,
                    })
                }
                if defer_multi_segment_ease {
                    pending_custom_eases.push((layer.keyframes.len(), f.clone(), at.clone()));
                }
                if let Some(morph_id) = f.morph_shape {
                    pending_morphs.push((layer.keyframes.len(), morph_id));
                }
                layer.keyframes.push(Keyframe {
                    frame: cursor,
                    elements,
                    tween,
                    tween_kind,
                    orient_to_path,
                    sync_symbols,
                    snap_to_guide,
                    classic_rotate_dir,
                    classic_rotate_turns,
                    sound,
                    video,
                    bitmap_fills,
                    appearances,
                    vector_effects,
                    label: optional_text(&s.original_bytes, f.label.as_ref())?,
                    ..Default::default()
                });
                let duration = u32::from(f.duration.unwrap_or(1)).max(1);
                if let Some(span) = f.motion_xml.as_ref().filter(|v| v.data.end > v.data.start) {
                    let key = layer.keyframes.last().unwrap();
                    let sampled = (|| {
                        if span.data.end - span.data.start > crate::motion_xml::MAX_XML_BYTES {
                            return Err("motion XML exceeds the sampling limit");
                        }
                        let xml = read_string(&s.original_bytes, span)
                            .map_err(|_| "invalid motion XML text")?;
                        let doc = roxmltree::Document::parse_with_options(
                            &xml,
                            roxmltree::ParsingOptions {
                                nodes_limit: crate::motion_xml::MAX_XML_NODES,
                                ..Default::default()
                            },
                        )
                        .map_err(|_| "invalid or oversized motion XML")?;
                        let pivot = frame
                            .children
                            .iter()
                            .filter_map(|id| object(s, *id).ok())
                            .find_map(|placed| {
                                let Some(TimelinePayload::Instance(instance)) = &placed.payload
                                else {
                                    return None;
                                };
                                let world = placed
                                    .base
                                    .as_ref()
                                    .and_then(|base| base.registration_words)
                                    .filter(|xy| !xy.contains(&i32::MIN))
                                    .or_else(|| {
                                        instance
                                            .center_point_3d
                                            .map(|[x, y, _]| [x, y])
                                            .filter(|p| !p.contains(&i32::MIN))
                                    })?;
                                let inv = invert_affine_matrix(transform_matrix(
                                    key.elements.first()?.transform,
                                ))?;
                                let [x, y] = world.map(|v| f64::from(v) / 20.);
                                Some([
                                    inv[0] * x + inv[2] * y + inv[4],
                                    inv[1] * x + inv[3] * y + inv[5],
                                ])
                            });
                        if duration > MAX_TIMELINE_FRAMES - cursor {
                            return Err("motion span exceeds timeline limit");
                        }
                        let ctx_3d = crate::motion_xml::MotionContext {
                            pivot,
                            stage_size: self.motion_3d.0,
                            view_angle_3d: self.motion_3d.1,
                            vanishing_point_3d: self.motion_3d.2,
                        };
                        crate::motion_xml::sample_poses(
                            doc.root_element(),
                            key,
                            duration,
                            ctx_3d,
                            &mut self.motion_pose_budget,
                        )
                    })();
                    match sampled {
                        Ok(res) => {
                            if let Some(el0) = res.start_element
                                && let Some(last_kf) = layer.keyframes.last_mut()
                                && !last_kf.elements.is_empty()
                            {
                                last_kf.elements[0] = el0;
                            }
                            if res.used_3d {
                                self.report.warning(
                                    "fla_3d_transform",
                                    &at,
                                    "Projected 3D motion-object rotation and depth tracks to 2D perspective affine poses.",
                                );
                            }
                            if res.filter_approximated {
                                self.report.warning(
                                    "fla_filter_approximated",
                                    &at,
                                    "Unsupported motion-object filter curves were approximated on sampled poses.",
                                );
                            }
                            motion_keys.extend(res.poses);
                            self.report.warning(
                                "fla_motion_xml",
                                &at,
                                "Sampled 2D motion-object tracks into editable per-frame poses.",
                            );
                        }
                        Err(reason) => self.report.warning(
                            "fla_motion_xml",
                            &at,
                            format!("Motion-object tracks use the stored pose: {reason}."),
                        ),
                    }
                } else if let Some(pm_span) = f.pose_matrices {
                    let key = layer.keyframes.last().unwrap();
                    let sampled_ik = sample_fla_ik_pose_matrices(
                        &s.original_bytes,
                        pm_span,
                        key,
                        duration,
                        cursor,
                        &mut self.motion_pose_budget,
                    );
                    if let Some(ik_poses) = sampled_ik {
                        motion_keys.extend(ik_poses);
                        if l.animation_type.unwrap_or(0) == 0 {
                            self.report.warning(
                                "fla_layer_animation",
                                &at,
                                "Sampled binary FLA IK pose matrices into editable per-frame keyframes; runtime IK bone constraints are baked to frame poses.",
                            );
                        }
                    } else if l.animation_type.unwrap_or(0) == 0 {
                        self.report.warning(
                            "fla_layer_animation",
                            &at,
                            "IK pose matrices are retained in the embedded source; the keyframe uses its stored pose.",
                        );
                    }
                }
                let next_cursor = u64::from(cursor) + u64::from(duration);
                if next_cursor > u64::from(MAX_TIMELINE_FRAMES) {
                    self.report.warning(
                        "fla_frame_limit",
                        &at,
                        format!(
                            "Keyframe duration extends beyond the {MAX_TIMELINE_FRAMES}-frame timeline limit and was clamped."
                        ),
                    );
                    cursor = MAX_TIMELINE_FRAMES;
                } else {
                    cursor = next_cursor as u32;
                }
                if !frame.complete {
                    break;
                }
            }
            if let Some(last) = layer.keyframes.last_mut()
                && last.tween.is_some()
            {
                // Flash shows an unterminated tween as a held pose.
                last.tween = None;
                last.tween_kind = TweenKind::Classic;
                last.orient_to_path = false;
                last.sync_symbols = false;
                last.snap_to_guide = false;
                last.classic_rotate_dir = ClassicRotateDirection::Auto;
                last.classic_rotate_turns = 0;
            }
            for (key_idx, morph_id) in pending_morphs {
                if key_idx + 1 < layer.keyframes.len()
                    && layer.keyframes[key_idx].tween.is_some()
                    && layer.keyframes[key_idx].tween_kind == TweenKind::Shape
                {
                    let (hints, had_unconverted_hints) = resolve_fla_morph_hints(
                        s,
                        morph_id,
                        &layer.keyframes[key_idx].elements,
                        &layer.keyframes[key_idx + 1].elements,
                    );
                    if had_unconverted_hints {
                        self.report.warning(
                            "fla_morph_hints_approximated",
                            format!(
                                "stream #{} layer {} frame {}",
                                s.index,
                                if layer.name.is_empty() {
                                    "<unnamed>"
                                } else {
                                    &layer.name
                                },
                                layer.keyframes[key_idx].frame
                            ),
                            "One or more binary FLA shape-hint records were incomplete or unsupported; compatible hints were retained and the original morph records remain embedded.",
                        );
                    }
                    if !hints.is_empty() {
                        layer.keyframes[key_idx].shape_hints = hints;
                    }
                }
            }
            for (key_idx, saved_frame, saved_at) in pending_custom_eases.into_iter().rev() {
                if key_idx < layer.keyframes.len()
                    && layer.keyframes[key_idx].tween.is_some()
                    && let Err(reason) =
                        try_split_multi_segment_custom_ease(&mut layer, key_idx, &saved_frame)
                {
                    self.report.warning(
                        "fla_tween_easing",
                        &saved_at,
                        format!("{reason}; the tween uses its classic acceleration instead."),
                    );
                }
            }
            layer.keyframes.extend(motion_keys);
            layer.keyframes.sort_by_key(|key| key.frame);
            if layer.keyframes.is_empty() && layer.properties.role != LayerRole::Folder {
                if o.complete && o.children.is_empty() {
                    layer.properties.end_frame = Some(0);
                } else {
                    self.report.warning("fla_layer_skipped",format!("stream #{} layer {}",s.index,layer.name),"No complete frame could be read for this layer; it remains in the embedded source.");
                    continue;
                }
            }
            if !editor_visible && self.include_hidden {
                layer.properties.publish_when_hidden = match layer.properties.role {
                    LayerRole::Guide | LayerRole::Folder => false,
                    LayerRole::Mask => true,
                    LayerRole::Normal => layer.keyframes.iter().any(|key| {
                        !key.elements.is_empty()
                            || !key.label.is_empty()
                            || key.sound.is_some()
                            || key.video.is_some()
                    }),
                };
                if layer.properties.publish_when_hidden {
                    self.report.warning(
                        "fla_publish_hidden_layer",
                        format!("stream #{} layer {}", s.index, layer.name),
                        "The layer stays hidden while editing and is included in playback by the Flash publish profile.",
                    );
                }
            }
            timeline.frame_count = timeline.frame_count.max(cursor);
            sources.push(cursor);
            timeline.layers.push(layer);
        }
        // Parent/mask/guide links to layers that were not imported are dropped.
        let ids: Vec<_> = timeline.layers.iter().map(|l| l.id.clone()).collect();
        for layer in &mut timeline.layers {
            let p = &mut layer.properties;
            for link in [&mut p.parent, &mut p.mask, &mut p.motion_guide] {
                if link.as_ref().is_some_and(|id| !ids.contains(id)) {
                    *link = None;
                }
            }
        }
        for (layer, end) in timeline.layers.iter_mut().zip(sources) {
            if end < timeline.frame_count
                && layer.properties.role != LayerRole::Folder
                && layer.properties.end_frame.is_none()
            {
                layer.keyframes.push(Keyframe {
                    frame: end,
                    elements: vec![],
                    tween: None,
                    label: String::new(),
                    ..Default::default()
                });
            }
        }
        Ok(timeline)
    }
}
