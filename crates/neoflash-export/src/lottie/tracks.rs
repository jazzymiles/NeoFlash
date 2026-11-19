//! Timeline layer spans, element tracks, keyframe interpolation, easing, and transform (`ks`) builders.

use super::{
    LOTTIE_VERSION, LottieBuilder, LottieMovieClipMode, LottieNoteKind, round4,
    shapes::resolve_native_dash_stroke,
};
use crate::ExportError;
use neoflash_core::{
    ClassicRotateDirection, ColorTransform, Easing, Element, ElementKind, InstancePlayback,
    Keyframe, Layer, LayerRole, MotionPathSource, SymbolKind, Timeline, Transform, TweenKind,
    TweenProperty, camera_layer_matrix, layer_effective_blend_mode, layer_effectively_synced,
    layer_effectively_visible, motion_span, placement_start, sample_layer_in_at_fps,
    sample_timeline_camera,
};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};

impl<'a> LottieBuilder<'a> {
    pub(super) fn build_timeline_layers(
        &mut self,
        symbol_id: &str,
        symbol_name: &str,
        timeline: &Timeline,
        origin_offset: [f64; 2],
        include_bg: bool,
        stack: &mut Vec<String>,
    ) -> Result<Vec<Value>, ExportError> {
        self.check_cancelled()?;
        if !timeline.scripts.is_empty() {
            let first_script_frame = timeline.scripts.first().map(|s| s.frame);
            self.add_note_coded(
                LottieNoteKind::NotExported,
                Some("LOTTIE_FRAME_SCRIPT"),
                format!("Timeline '{symbol_name}'"),
                format!(
                    "{} frame script(s) are not executed by Lottie players; timeline animation was exported.",
                    timeline.scripts.len()
                ),
                Some(symbol_id.to_owned()),
                None,
                first_script_frame,
            );
        }

        let total_frames = timeline.frame_count.max(1);
        let timeline_has_camera = timeline
            .layers
            .iter()
            .any(|l| l.keyframes.iter().any(|k| k.camera.is_some()));
        let mut lottie_layers = Vec::new();
        let mut next_ind: u32 = 1;

        // Collect mask layers by ID so masked layers can attach `masksProperties` or track mattes.
        let mut mask_layers: BTreeMap<&str, &Layer> = BTreeMap::new();
        for layer in &timeline.layers {
            if layer.properties.role == LayerRole::Mask {
                mask_layers.insert(layer.id.as_str(), layer);
            }
        }

        // In NeoFlash, `timeline.layers[0]` is the bottom layer and `layers.last()` is the top
        // layer. In Lottie, `layers[0]` is rendered on top, so iterate in reverse order.
        for layer in timeline.layers.iter().rev() {
            self.check_cancelled()?;
            for kf in &layer.keyframes {
                if let Some(audio) = self.stream_audio_layer(layer, kf, total_frames, next_ind) {
                    lottie_layers.push(audio);
                    next_ind += 1;
                    self.exported_count += 1;
                } else if kf.sound.is_some() {
                    self.add_note_coded(
                        LottieNoteKind::NotExported,
                        Some("LOTTIE_TIMELINE_SOUND"),
                        format!("Layer '{}' (frame {})", layer.name, kf.frame + 1),
                        "This playback mode, envelope or player profile cannot reproduce the authored sound. lottie-web can export a simple streamed WAV or MP3.",
                        Some(symbol_id.to_owned()),
                        Some(layer.id.clone()),
                        Some(kf.frame),
                    );
                }
                if kf.video.is_some() {
                    self.add_note_coded(
                        LottieNoteKind::NotExported,
                        Some("LOTTIE_TIMELINE_VIDEO"),
                        format!("Layer '{}' (frame {})", layer.name, kf.frame + 1),
                        format!(
                            "timeline video span is not supported in Lottie v{LOTTIE_VERSION} export."
                        ),
                        Some(symbol_id.to_owned()),
                        Some(layer.id.clone()),
                        Some(kf.frame),
                    );
                }
            }
            if matches!(
                layer.properties.role,
                LayerRole::Guide | LayerRole::Folder | LayerRole::Mask
            ) {
                continue;
            }
            if !self.options.include_hidden_layers && !layer_effectively_visible(timeline, layer) {
                continue;
            }

            if !layer.properties.filters.is_empty() {
                self.add_note_coded(
                    LottieNoteKind::NotExported,
                    Some("LOTTIE_DISPLAY_FILTER"),
                    format!("Layer '{}'", layer.name),
                    format!(
                        "Layer display filter(s) are not supported in core Lottie v{LOTTIE_VERSION}."
                    ),
                    Some(symbol_id.to_owned()),
                    Some(layer.id.clone()),
                    Some(0),
                );
            }
            if layer.properties.scale9_grid.is_some() {
                self.add_note_coded(
                    LottieNoteKind::Approximated,
                    Some("LOTTIE_SCALE9_GRID"),
                    format!("Layer '{}'", layer.name),
                    "9-slice scaling guides are flattened into standard 2D transforms in Lottie export.",
                    Some(symbol_id.to_owned()),
                    Some(layer.id.clone()),
                    Some(0),
                );
            }
            for kf in &layer.keyframes {
                if !kf.filters.is_empty() {
                    self.add_note_coded(
                        LottieNoteKind::NotExported,
                        Some("LOTTIE_DISPLAY_FILTER"),
                        format!("Layer '{}' (frame {})", layer.name, kf.frame + 1),
                        format!(
                            "Keyframe display filter(s) are not supported in core Lottie v{LOTTIE_VERSION}."
                        ),
                        Some(symbol_id.to_owned()),
                        Some(layer.id.clone()),
                        Some(kf.frame),
                    );
                }
                if !kf.bitmap_fills.is_empty() {
                    self.add_note_coded(
                        LottieNoteKind::NotExported,
                        Some("LOTTIE_BITMAP_FILL"),
                        format!("Layer '{}' (frame {})", layer.name, kf.frame + 1),
                        "Tiled/affine bitmap shape fills are not supported by core Lottie shapes; fallback solid/gradient paint exported.",
                        Some(symbol_id.to_owned()),
                        Some(layer.id.clone()),
                        Some(kf.frame),
                    );
                }
                if kf.perspective_grid.is_some() {
                    self.add_note_coded(
                        LottieNoteKind::NotExported,
                        Some("LOTTIE_PERSPECTIVE_3D"),
                        format!("Layer '{}' (frame {})", layer.name, kf.frame + 1),
                        "3D perspective grid planes are not supported in 2D Lottie export.",
                        Some(symbol_id.to_owned()),
                        Some(layer.id.clone()),
                        Some(kf.frame),
                    );
                }
                if !kf.instance_overrides.is_empty() {
                    self.add_note_coded(
                        LottieNoteKind::Approximated,
                        Some("LOTTIE_INSTANCE_OVERRIDE"),
                        format!("Layer '{}' (frame {})", layer.name, kf.frame + 1),
                        "Per-instance child property overrides share the base symbol precomp in Lottie export.",
                        Some(symbol_id.to_owned()),
                        Some(layer.id.clone()),
                        Some(kf.frame),
                    );
                }
                for vfx in &kf.vector_effects {
                    if vfx.freeform_gradient.is_some() || vfx.gradient_mesh.is_some() {
                        self.add_note_coded(
                            LottieNoteKind::NotExported,
                            Some("LOTTIE_VECTOR_MESH_GRADIENT"),
                            format!(
                                "Layer '{}' element '{}' (frame {})",
                                layer.name,
                                vfx.element_id,
                                kf.frame + 1
                            ),
                            "Freeform/mesh gradients cannot be represented as Lottie linear/radial gradients; base vector paint exported.",
                            Some(symbol_id.to_owned()),
                            Some(layer.id.clone()),
                            Some(kf.frame),
                        );
                    } else if !vfx.procedural_effects.is_empty()
                        || vfx.envelope_warp.is_some()
                        || vfx.vector_blend.is_some()
                    {
                        self.add_note_coded(
                            LottieNoteKind::Approximated,
                            Some("LOTTIE_VECTOR_EFFECT_BAKED"),
                            format!(
                                "Layer '{}' element '{}' (frame {})",
                                layer.name,
                                vfx.element_id,
                                kf.frame + 1
                            ),
                            "Procedural vector effects / envelope warps were baked into evaluated Bézier contours.",
                            Some(symbol_id.to_owned()),
                            Some(layer.id.clone()),
                            Some(kf.frame),
                        );
                    }
                }
            }

            let layer_end = layer
                .properties
                .end_frame
                .unwrap_or(total_frames)
                .min(total_frames);
            if layer_end == 0 || layer.keyframes.is_empty() {
                continue;
            }

            let layer_bm = self.map_blend_mode(
                layer_effective_blend_mode(timeline, layer),
                &format!("Layer '{}'", layer.name),
                Some(symbol_id),
                Some(&layer.id),
                Some(0),
            );
            let synced = layer_effectively_synced(timeline, layer);
            let mask_layer = layer
                .properties
                .mask
                .as_deref()
                .and_then(|id| mask_layers.get(id).copied());

            let spans = collect_layer_spans(layer, layer_end);
            for span in spans.into_iter().rev() {
                self.check_cancelled()?;
                let emitted = self.emit_span_layers(
                    symbol_id,
                    timeline,
                    timeline_has_camera,
                    layer,
                    &span,
                    total_frames,
                    layer_bm,
                    synced,
                    mask_layer,
                    origin_offset,
                    stack,
                    &mut next_ind,
                )?;
                lottie_layers.extend(emitted);
            }
        }

        if include_bg && self.project.background[3] > 0 {
            let [r, g, b, a] = self.project.background;
            let hex = format!("#{r:02x}{g:02x}{b:02x}");
            let mut bg_ks = static_ks(
                Transform {
                    alpha: f64::from(a) / 255.0,
                    ..Transform::default()
                },
                [0.0, 0.0],
                [0.0, 0.0],
            );
            if a < 255
                && let Some(obj) = bg_ks.as_object_mut()
            {
                obj.insert(
                    "o".into(),
                    json!({"a": 0, "k": round4(f64::from(a) / 255.0 * 100.0)}),
                );
            }
            lottie_layers.push(json!({
                "ddd": 0,
                "ind": next_ind,
                "ty": 1,
                "nm": "Stage Background",
                "sr": 1,
                "ks": bg_ks,
                "ao": 0,
                "sw": self.comp_w,
                "sh": self.comp_h,
                "sc": hex,
                "ip": 0,
                "op": total_frames,
                "st": 0,
                "bm": 0
            }));
        }

        Ok(lottie_layers)
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_span_layers(
        &mut self,
        symbol_id: &str,
        timeline: &Timeline,
        timeline_has_camera: bool,
        layer: &Layer,
        span: &LayerSpan,
        total_frames: u32,
        layer_bm: u32,
        layer_synced: bool,
        mask_layer: Option<&Layer>,
        origin_offset: [f64; 2],
        stack: &mut Vec<String>,
        next_ind: &mut u32,
    ) -> Result<Vec<Value>, ExportError> {
        self.check_cancelled()?;
        let stage_center = [self.comp_w as f64 * 0.5, self.comp_h as f64 * 0.5];
        let sample_filtered_frame = |f: u32| -> Vec<Element> {
            let controllers = layer
                .key_at(f)
                .map(|k| k.controller_elements.as_slice())
                .unwrap_or(&[]);
            let cam_opt = if timeline_has_camera && !layer.properties.attach_to_camera {
                sample_timeline_camera(timeline, f)
            } else {
                None
            };
            let mut out_els = Vec::new();
            for mut el in sample_layer_in_at_fps(timeline, layer, f, self.source_fps) {
                if !controllers.is_empty() && controllers.iter().any(|cid| cid == &el.id) {
                    continue;
                }
                if let Some(cam) = &cam_opt {
                    let cam_m = camera_layer_matrix(cam, layer.properties.z_depth, stage_center);
                    let world_m = neoflash_core::affine_multiply(
                        cam_m,
                        neoflash_core::transform_matrix(el.transform),
                    );
                    if let Some(cam_t) = Transform::from_matrix(world_m) {
                        el.transform.x = cam_t.x;
                        el.transform.y = cam_t.y;
                        el.transform.scale_x = cam_t.scale_x;
                        el.transform.scale_y = cam_t.scale_y;
                        el.transform.rotation = cam_t.rotation;
                        el.transform.skew = cam_t.skew;
                    }
                    el.transform.color_transform =
                        cam.color_transform.then(el.transform.color_transform);
                }
                out_els.push(el);
            }
            out_els
        };

        let first_elements = sample_filtered_frame(span.start_frame);
        if first_elements.is_empty() {
            return Ok(Vec::new());
        }

        // Check if this contiguous non-empty span is a shape tween so we can emit morphed Lottie layers.
        let has_shape_tween = layer.keyframes.iter().any(|k| {
            k.frame >= span.start_frame
                && k.frame < span.end_frame
                && k.tween.is_some()
                && k.tween_kind == TweenKind::Shape
        });

        if has_shape_tween {
            return self.emit_sampled_shape_span(
                symbol_id,
                timeline,
                timeline_has_camera,
                layer,
                span,
                layer_bm,
                mask_layer,
                origin_offset,
                stack,
                next_ind,
            );
        }

        // Group elements by their ID across the span, rendered top-to-bottom (reverse of
        // `elements` slice order, since `elements.last()` is on top in NeoFlash).
        let dense = timeline_has_camera
            || layer.properties.motion_guide.is_some()
            || layer.keyframes.iter().any(|k| {
                !k.property_expressions.is_empty()
                    || !k.ik_armatures.is_empty()
                    || !k.asset_warps.is_empty()
                    || k.motion_path.is_some()
                    || k.orient_to_path
                    || k.roving
                    || (k.frame > span.start_frame
                        && k.frame + 1 < span.end_frame
                        && (k.keyed_properties.is_some() || k.motion_keys.is_some()))
                    || has_asymmetric_pair_ease(&k.property_eases)
                    || matches!(k.tween, Some(Easing::Bounce | Easing::Elastic))
                    || k.property_eases
                        .iter()
                        .any(|(_, e)| matches!(e, Easing::Bounce | Easing::Elastic))
                    || k.motion_tracks.iter().any(|track| {
                        track.motion_path.is_some()
                            || track.orient_to_path == Some(true)
                            || matches!(track.easing, Some(Easing::Bounce | Easing::Elastic))
                            || track.property_eases.as_ref().is_some_and(|eases| {
                                has_asymmetric_pair_ease(eases)
                                    || eases.iter().any(|(_, ease)| {
                                        matches!(ease, Easing::Bounce | Easing::Elastic)
                                    })
                            })
                    })
                    || k.motion_keys
                        .as_ref()
                        .is_some_and(|keys| keys.iter().any(|key| key.roving))
            });
        let sample_frames: Vec<u32> = if dense {
            (span.start_frame..span.end_frame).collect()
        } else {
            let mut frames = BTreeSet::new();
            frames.insert(span.start_frame);
            frames.insert(span.end_frame.saturating_sub(1).max(span.start_frame));
            for kf in &layer.keyframes {
                if kf.frame >= span.start_frame && kf.frame < span.end_frame {
                    frames.insert(kf.frame);
                    frames.insert(kf.frame.saturating_sub(1).max(span.start_frame));
                }
            }
            frames.into_iter().collect()
        };

        let mut tracks: Vec<String> = Vec::new();
        let mut track_samples: BTreeMap<String, Vec<(u32, Element)>> = BTreeMap::new();
        for f in &sample_frames {
            self.check_cancelled()?;
            for el in sample_filtered_frame(*f) {
                if !track_samples.contains_key(&el.id) {
                    tracks.push(el.id.clone());
                    track_samples.insert(el.id.clone(), Vec::new());
                }
                if el.visible
                    && let Some(vec) = track_samples.get_mut(&el.id)
                {
                    vec.push((*f, el));
                }
            }
        }

        // Pre-detect consistent intra-layer `element_parents` links (`child_id -> parent_id`)
        // across this span when no camera warp is active, so children can emit native Lottie
        // `"parent": parent_ind` links.
        let mut stable_parent_of: BTreeMap<String, String> = BTreeMap::new();
        if !timeline_has_camera {
            for child_id in &tracks {
                let mut candidate: Option<String> = None;
                let mut consistent = true;
                for &f in &sample_frames {
                    let pid = layer
                        .key_at(f)
                        .and_then(|k| k.parent_of(child_id))
                        .map(str::to_owned);
                    match (&candidate, pid) {
                        (None, Some(p)) if f == sample_frames[0] => candidate = Some(p),
                        (Some(expected), Some(p)) if expected == &p => {}
                        (None, None) => {}
                        _ => {
                            consistent = false;
                            break;
                        }
                    }
                }
                if consistent
                    && let Some(pid) = candidate
                    && track_samples.contains_key(&pid)
                {
                    stable_parent_of.insert(child_id.clone(), pid);
                }
            }
        }

        let mut parent_world_tracks = BTreeMap::new();
        for pid in stable_parent_of.values() {
            if parent_world_tracks.contains_key(pid) {
                continue;
            }
            if let Some(psamples) = track_samples.get(pid)
                && !psamples.is_empty()
            {
                let anchor = if let ElementKind::Instance { symbol_id, .. } = &psamples[0].1.kind {
                    self.precomp_bounds
                        .get(symbol_id)
                        .map_or([0.0; 2], |bounds| bounds.0)
                } else {
                    [0.0, 0.0]
                };
                let mut map = BTreeMap::new();
                let mut invertible = true;
                for (f, pel) in psamples {
                    let m = neoflash_core::transform_matrix(pel.transform);
                    if invert_affine(m).is_none() {
                        invertible = false;
                        break;
                    }
                    map.insert(*f, m);
                }
                if invertible {
                    parent_world_tracks.insert(pid.clone(), (map, anchor));
                }
            }
        }

        let mut emitted_ind_by_element: BTreeMap<String, u32> = BTreeMap::new();
        let mut pending_parent_patch: Vec<(usize, String)> = Vec::new();
        let mut out = Vec::new();
        for element_id in tracks.into_iter().rev() {
            self.check_cancelled()?;
            let Some(mut samples) = track_samples.remove(&element_id) else {
                continue;
            };
            let mut effective_origin = origin_offset;
            let mut parent_target_id: Option<String> = None;
            let world_samples_for_mask: Vec<(u32, Transform)> =
                samples.iter().map(|(f, e)| (*f, e.transform)).collect();

            if let Some(pid) = stable_parent_of.get(&element_id)
                && let Some((pmap, panchor)) = parent_world_tracks.get(pid)
                && samples.iter().all(|(f, _)| pmap.contains_key(f))
            {
                let mut converted_ok = true;
                let mut local_samples = Vec::with_capacity(samples.len());
                for (f, mut el) in samples.iter().cloned() {
                    let pm = pmap[&f];
                    let Some(inv_pm) = invert_affine(pm) else {
                        converted_ok = false;
                        break;
                    };
                    let local_m = neoflash_core::affine_multiply(
                        inv_pm,
                        neoflash_core::transform_matrix(el.transform),
                    );
                    let Some(lt) = Transform::from_matrix(local_m) else {
                        converted_ok = false;
                        break;
                    };
                    el.transform.x = lt.x;
                    el.transform.y = lt.y;
                    el.transform.scale_x = lt.scale_x;
                    el.transform.scale_y = lt.scale_y;
                    el.transform.rotation = lt.rotation;
                    el.transform.skew = lt.skew;
                    local_samples.push((f, el));
                }
                if converted_ok {
                    samples = local_samples;
                    effective_origin = *panchor;
                    parent_target_id = Some(pid.clone());
                }
            }

            let emitted_layers = self.emit_element_track(
                symbol_id,
                timeline,
                layer,
                samples,
                &world_samples_for_mask,
                dense,
                total_frames,
                layer_bm,
                layer_synced,
                mask_layer,
                effective_origin,
                origin_offset,
                stack,
                next_ind,
            )?;
            if !emitted_layers.is_empty() {
                let content_idx_in_out = out.len() + emitted_layers.len() - 1;
                if let Some(ind_u64) = emitted_layers.last().and_then(|v| v["ind"].as_u64()) {
                    emitted_ind_by_element.insert(element_id.clone(), ind_u64 as u32);
                }
                if let Some(pid) = parent_target_id {
                    pending_parent_patch.push((content_idx_in_out, pid));
                }
                out.extend(emitted_layers);
            }
        }

        for (layer_idx, pid) in pending_parent_patch {
            if let Some(&p_ind) = emitted_ind_by_element.get(&pid)
                && let Some(obj) = out.get_mut(layer_idx).and_then(Value::as_object_mut)
            {
                obj.insert("parent".into(), json!(p_ind));
            }
        }

        Ok(out)
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_element_track(
        &mut self,
        symbol_id: &str,
        timeline: &Timeline,
        layer: &Layer,
        samples: Vec<(u32, Element)>,
        world_samples_for_mask: &[(u32, Transform)],
        force_dense_ks: bool,
        _total_frames: u32,
        layer_bm: u32,
        layer_synced: bool,
        mask_layer: Option<&Layer>,
        origin_offset: [f64; 2],
        comp_origin_offset: [f64; 2],
        stack: &mut Vec<String>,
        next_ind: &mut u32,
    ) -> Result<Vec<Value>, ExportError> {
        self.check_cancelled()?;
        if samples.is_empty() {
            return Ok(Vec::new());
        }
        let first_frame = samples.first().unwrap().0;
        let last_frame = samples.last().unwrap().0 + 1;
        let representative = &samples[0].1;

        if !representative.scripts.is_empty() {
            self.add_note_coded(
                LottieNoteKind::NotExported,
                Some("LOTTIE_OBJECT_SCRIPT"),
                format!(
                    "Layer '{}' element '{}'",
                    layer.name,
                    layer_element_name(layer, representative)
                ),
                "object script(s) are not executed by Lottie players.",
                Some(symbol_id.to_owned()),
                Some(layer.id.clone()),
                Some(first_frame),
            );
        }
        if samples.iter().any(|(_, e)| !e.filters.is_empty()) {
            self.add_note_coded(
                LottieNoteKind::NotExported,
                Some("LOTTIE_DISPLAY_FILTER"),
                format!(
                    "Layer '{}' element '{}'",
                    layer.name,
                    layer_element_name(layer, representative)
                ),
                format!("display filter(s) are not exported to Lottie v{LOTTIE_VERSION}."),
                Some(symbol_id.to_owned()),
                Some(layer.id.clone()),
                Some(first_frame),
            );
        }
        if representative.scale9_grid().is_some()
            && samples
                .iter()
                .any(|(_, e)| (e.transform.scale_x - e.transform.scale_y).abs() > 1e-3)
        {
            self.add_note_coded(
                LottieNoteKind::Approximated,
                Some("LOTTIE_SCALE9_GRID"),
                format!("Layer '{}' · {}", layer.name, representative.id),
                "Element 9-slice grid is approximated with uniform/affine layer scaling in Lottie.",
                Some(symbol_id.to_owned()),
                Some(layer.id.clone()),
                Some(first_frame),
            );
        }

        let bm = if representative.blend_mode.is_normal() {
            layer_bm
        } else {
            self.map_blend_mode(
                representative.blend_mode,
                &format!("Layer '{}' element '{}'", layer.name, representative.id),
                Some(symbol_id),
                Some(&layer.id),
                Some(first_frame),
            )
        };

        let orient_auto = layer.keyframes.iter().any(|k| {
            k.frame >= first_frame
                && k.frame < last_frame
                && k.motion_orient_to_path_for(&representative.id)
        });

        match &representative.kind {
            ElementKind::Instance {
                symbol_id: child_symbol_id,
                playback,
            } => {
                let Some(ref_id) = self.ensure_symbol_precomp(
                    child_symbol_id,
                    symbol_id,
                    Some(&layer.id),
                    Some(first_frame),
                    stack,
                )?
                else {
                    return Ok(Vec::new());
                };
                if samples
                    .iter()
                    .any(|(_, e)| has_non_alpha_color_transform(e.transform.color_transform))
                {
                    self.add_note_coded(
                        LottieNoteKind::Approximated,
                        Some("LOTTIE_COLOR_TRANSFORM_APPROX"),
                        format!(
                            "Layer '{}' · {}",
                            layer.name,
                            layer_element_name(layer, representative)
                        ),
                        "RGB tint/color-offset transform on symbol instance cannot be represented losslessly on a core Lottie precomp layer; alpha channel was preserved.",
                        Some(symbol_id.to_owned()),
                        Some(layer.id.clone()),
                        Some(first_frame),
                    );
                }
                let (precomp_anchor, [precomp_w, precomp_h]) = self.precomp_bounds[child_symbol_id];
                let ks = self.build_animated_ks(
                    timeline,
                    layer,
                    &samples,
                    precomp_anchor,
                    origin_offset,
                    force_dense_ks,
                    orient_auto,
                );
                let ind = *next_ind;
                *next_ind += 1;
                let target_sym = self.project.symbol(child_symbol_id);
                let is_button = target_sym.is_some_and(|s| s.kind == SymbolKind::Button);
                let effective_playback = if is_button {
                    InstancePlayback::SingleFrame(0)
                } else if self.options.movie_clip_mode == LottieMovieClipMode::FirstFrame
                    && target_sym.is_some_and(|s| s.kind == SymbolKind::MovieClip)
                {
                    if target_sym.is_some_and(|s| s.timeline.frame_count > 1) {
                        self.add_note_coded(
                            LottieNoteKind::Approximated,
                            Some("LOTTIE_MOVIECLIP_FIRST_FRAME"),
                            format!("Layer '{}' · {}", layer.name, representative.name),
                            format!(
                                "MovieClip '{}' exported as first frame only (Movie clips: First frame).",
                                child_symbol_id
                            ),
                            Some(child_symbol_id.clone()),
                            Some(layer.id.clone()),
                            Some(first_frame),
                        );
                    }
                    InstancePlayback::SingleFrame(0)
                } else {
                    *playback
                };
                let mut obj = json!({
                    "ddd": 0,
                    "ind": ind,
                    "ty": 0,
                    "nm": layer_element_name(layer, representative),
                    "refId": ref_id,
                    "sr": 1,
                    "ks": ks,
                    "ao": if orient_auto { 1 } else { 0 },
                    "w": precomp_w,
                    "h": precomp_h,
                    "ip": first_frame,
                    "op": last_frame,
                    "st": first_frame,
                    "bm": bm
                });
                if let Some(tm) = self.build_instance_time_remap(
                    timeline,
                    layer,
                    &representative.id,
                    child_symbol_id,
                    effective_playback,
                    is_button,
                    layer_synced,
                    first_frame,
                    last_frame,
                ) {
                    obj.as_object_mut().unwrap().insert("tm".into(), tm);
                }
                let wrapped = self.attach_mask_or_matte(
                    symbol_id,
                    timeline,
                    mask_layer,
                    obj,
                    first_frame,
                    last_frame,
                    world_samples_for_mask,
                    comp_origin_offset,
                    stack,
                    next_ind,
                )?;
                self.exported_count += 1;
                Ok(wrapped)
            }
            ElementKind::Bitmap {
                resource_id,
                width,
                height,
                ..
            } => {
                let Some(ref_id) = self.ensure_image_asset(
                    resource_id,
                    *width,
                    *height,
                    Some(symbol_id),
                    Some(&layer.id),
                    Some(first_frame),
                )?
                else {
                    return Ok(Vec::new());
                };
                if samples
                    .iter()
                    .any(|(_, e)| has_non_alpha_color_transform(e.transform.color_transform))
                {
                    self.add_note_coded(
                        LottieNoteKind::Approximated,
                        Some("LOTTIE_COLOR_TRANSFORM_APPROX"),
                        format!(
                            "Layer '{}' · {}",
                            layer.name,
                            layer_element_name(layer, representative)
                        ),
                        "RGB tint/color-offset transform on bitmap instance cannot be represented losslessly on a core Lottie image layer; alpha channel was preserved.",
                        Some(symbol_id.to_owned()),
                        Some(layer.id.clone()),
                        Some(first_frame),
                    );
                }
                let ks = self.build_animated_ks(
                    timeline,
                    layer,
                    &samples,
                    [0.0, 0.0],
                    origin_offset,
                    force_dense_ks,
                    orient_auto,
                );
                let ind = *next_ind;
                *next_ind += 1;
                let obj = json!({
                    "ddd": 0,
                    "ind": ind,
                    "ty": 2,
                    "nm": layer_element_name(layer, representative),
                    "refId": ref_id,
                    "sr": 1,
                    "ks": ks,
                    "ao": if orient_auto { 1 } else { 0 },
                    "ip": first_frame,
                    "op": last_frame,
                    "st": first_frame,
                    "bm": bm
                });
                let wrapped = self.attach_mask_or_matte(
                    symbol_id,
                    timeline,
                    mask_layer,
                    obj,
                    first_frame,
                    last_frame,
                    world_samples_for_mask,
                    comp_origin_offset,
                    stack,
                    next_ind,
                )?;
                self.exported_count += 1;
                Ok(wrapped)
            }
            ElementKind::Text {
                text,
                font_family,
                font_size,
                color,
            } => self.emit_text_element_layer(
                symbol_id,
                timeline,
                layer,
                &samples,
                representative,
                text,
                font_family,
                *font_size,
                *color,
                first_frame,
                last_frame,
                bm,
                orient_auto,
                force_dense_ks,
                mask_layer,
                origin_offset,
                comp_origin_offset,
                world_samples_for_mask,
                stack,
                next_ind,
            ),
            ElementKind::TextBox {
                text,
                width,
                height,
                format,
                ..
            } => self.emit_textbox_element_layer(
                symbol_id,
                timeline,
                layer,
                &samples,
                representative,
                text,
                *width,
                *height,
                format,
                first_frame,
                last_frame,
                bm,
                orient_auto,
                force_dense_ks,
                mask_layer,
                origin_offset,
                comp_origin_offset,
                world_samples_for_mask,
                stack,
                next_ind,
            ),
            ElementKind::Video { .. } => {
                self.add_note_coded(
                    LottieNoteKind::NotExported,
                    Some("LOTTIE_VIDEO_ELEMENT"),
                    format!(
                        "Layer '{}' element '{}'",
                        layer.name,
                        layer_element_name(layer, representative)
                    ),
                    format!("video elements are not supported by Lottie v{LOTTIE_VERSION}."),
                    Some(symbol_id.to_owned()),
                    Some(layer.id.clone()),
                    Some(first_frame),
                );
                Ok(Vec::new())
            }
            ElementKind::Rectangle { .. }
            | ElementKind::Ellipse { .. }
            | ElementKind::Path { .. }
            | ElementKind::PaintedPath { .. } => {
                let dash_override =
                    resolve_native_dash_stroke(layer, first_frame, &representative.id);
                let Some(shape_group) = self.build_shape_group_for_samples(
                    layer,
                    &samples,
                    force_dense_ks,
                    dash_override.as_ref(),
                    symbol_id,
                ) else {
                    return Ok(Vec::new());
                };
                let ks = self.build_animated_ks(
                    timeline,
                    layer,
                    &samples,
                    [0.0, 0.0],
                    origin_offset,
                    force_dense_ks,
                    orient_auto,
                );
                let ind = *next_ind;
                *next_ind += 1;
                let obj = json!({
                    "ddd": 0,
                    "ind": ind,
                    "ty": 4,
                    "nm": layer_element_name(layer, representative),
                    "sr": 1,
                    "ks": ks,
                    "ao": if orient_auto { 1 } else { 0 },
                    "shapes": [shape_group],
                    "ip": first_frame,
                    "op": last_frame,
                    "st": first_frame,
                    "bm": bm
                });
                let wrapped = self.attach_mask_or_matte(
                    symbol_id,
                    timeline,
                    mask_layer,
                    obj,
                    first_frame,
                    last_frame,
                    world_samples_for_mask,
                    comp_origin_offset,
                    stack,
                    next_ind,
                )?;
                self.exported_count += 1;
                Ok(wrapped)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn build_instance_time_remap(
        &self,
        timeline: &Timeline,
        layer: &Layer,
        element_id: &str,
        symbol_id: &str,
        playback: InstancePlayback,
        is_button: bool,
        layer_synced: bool,
        first_frame: u32,
        last_frame: u32,
    ) -> Option<Value> {
        let child_sym = self.project.symbol(symbol_id)?;
        let child_len = if is_button {
            1
        } else {
            child_sym.timeline.frame_count.max(1)
        };
        if child_len <= 1 {
            if last_frame > first_frame + 1 {
                return Some(json!({
                    "a": 0,
                    "k": 0.0
                }));
            }
            return None;
        }
        let span_start = placement_start(layer, first_frame, element_id);
        let child_frame_at = |parent_frame: u32| -> u32 {
            let age = if layer_synced {
                parent_frame
            } else {
                parent_frame.saturating_sub(span_start)
            };
            match playback {
                InstancePlayback::SingleFrame(f) => f.min(child_len - 1),
                InstancePlayback::PlayOnce => age.min(child_len - 1),
                InstancePlayback::PlayOnceFrom(start) => {
                    start.saturating_add(age).min(child_len - 1)
                }
                InstancePlayback::Loop => age % child_len,
                InstancePlayback::LoopFrom(start) => start.saturating_add(age) % child_len,
                InstancePlayback::Independent => {
                    if layer_synced {
                        parent_frame % child_len
                    } else {
                        age % child_len
                    }
                }
            }
        };

        if let InstancePlayback::SingleFrame(f) = playback {
            return Some(json!({
                "a": 0,
                "k": round4(f.min(child_len - 1) as f64 / self.fps)
            }));
        }

        let _ = timeline;
        let matches_default =
            (first_frame..last_frame).all(|f| child_frame_at(f) == f.saturating_sub(first_frame));
        if matches_default {
            return None;
        }

        if last_frame <= first_frame + 1 {
            return Some(json!({
                "a": 0,
                "k": round4(child_frame_at(first_frame) as f64 / self.fps)
            }));
        }

        let mut kfs = Vec::with_capacity((last_frame - first_frame) as usize);
        for f in first_frame..last_frame {
            let cf = child_frame_at(f);
            let sec = round4(cf as f64 / self.fps);
            kfs.push(json!({
                "t": f,
                "s": [sec],
                "h": 1
            }));
        }
        Some(json!({
            "a": 1,
            "k": kfs
        }))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_animated_ks(
        &self,
        timeline: &Timeline,
        layer: &Layer,
        samples: &[(u32, Element)],
        anchor: [f64; 2],
        origin_offset: [f64; 2],
        force_dense: bool,
        orient_auto: bool,
    ) -> Value {
        let first_t = samples[0].1.transform;
        let element_id = samples[0].1.id.as_str();
        let first_f = samples[0].0;
        let last_f = samples.last().unwrap().0;
        let has_classic_spin = layer.keyframes.iter().any(|k| {
            k.frame >= first_f
                && k.frame < last_f
                && k.tween.is_some()
                && k.tween_kind == TweenKind::Classic
                && (k.classic_rotate_turns > 0
                    || !matches!(k.classic_rotate_dir, ClassicRotateDirection::Auto))
        });

        let is_static = !has_classic_spin
            && !orient_auto
            && samples
                .iter()
                .all(|(_, e)| transforms_approx_eq(e.transform, first_t));
        if is_static {
            return static_ks(first_t, anchor, origin_offset);
        }

        let dense = force_dense
            || layer.properties.motion_guide.is_some()
            || layer.keyframes.iter().any(|k| {
                !k.property_expressions.is_empty()
                    || !matches!(k.motion_path_for(element_id), MotionPathSource::None)
                    || k.motion_orient_to_path_for(element_id)
                    || k.motion_key_roving_for(element_id)
                    || matches!(
                        k.motion_easing_for(element_id),
                        Easing::Bounce | Easing::Elastic
                    )
                    || TweenProperty::ALL.into_iter().any(|property| {
                        matches!(
                            k.motion_property_ease_for(element_id, property),
                            Easing::Bounce | Easing::Elastic
                        )
                    })
            });

        let key_frames: Vec<(u32, Transform, Option<Easing>)> = if dense {
            samples
                .iter()
                .enumerate()
                .map(|(idx, (f, el))| {
                    let ease = (idx + 1 < samples.len()).then_some(Easing::Linear);
                    (*f, el.transform, ease)
                })
                .collect()
        } else {
            collect_transform_keyframes(timeline, layer, samples, element_id)
        };

        let unwrapped_rotations = compute_unwrapped_rotations(layer, &key_frames, dense);
        let auto_orient_offsets = if orient_auto {
            compute_lottie_auto_orient_deg(&key_frames)
        } else {
            vec![0.0; key_frames.len()]
        };

        let p_kfs = build_vec3_keyframes(&key_frames, |t, f| {
            let ease = if dense {
                t.2
            } else {
                ease_for_prop(layer, f, element_id, TweenProperty::X).or(t.2)
            };
            (
                [
                    round4(t.1.x + origin_offset[0]),
                    round4(t.1.y + origin_offset[1]),
                    0.0,
                ],
                ease,
            )
        });
        let s_kfs = build_vec3_keyframes(&key_frames, |t, f| {
            let hold_scale = !dense
                && motion_span(layer, f).is_none()
                && layer.key_at(f).is_some_and(|k| {
                    k.tween.is_some() && k.tween_kind == TweenKind::Classic && !k.tween_scale
                });
            let ease = if hold_scale {
                None
            } else if dense {
                t.2
            } else {
                ease_for_prop(layer, f, element_id, TweenProperty::ScaleX).or(t.2)
            };
            let sk_cos = lottie_skew_cos(t.1.skew);
            (
                [
                    round4(t.1.scale_x * 100.0),
                    round4(t.1.scale_y * sk_cos * 100.0),
                    100.0,
                ],
                ease,
            )
        });
        let r_kfs = build_indexed_scalar_keyframes(&key_frames, |idx, t, f| {
            let (rot_deg, hold_rot) = unwrapped_rotations[idx];
            let compensated = rot_deg - auto_orient_offsets[idx];
            let ease = if hold_rot {
                None
            } else if dense {
                t.2
            } else {
                ease_for_prop(layer, f, element_id, TweenProperty::Rotation).or(t.2)
            };
            (round4(compensated), ease)
        });
        let o_kfs = build_scalar_keyframes(&key_frames, |t, f| {
            let ease = if dense {
                t.2
            } else {
                ease_for_prop(layer, f, element_id, TweenProperty::Alpha).or(t.2)
            };
            (round4(effective_opacity_percent(t.1)), ease)
        });

        let mut ks = Map::new();
        ks.insert("o".into(), o_kfs);
        ks.insert("r".into(), r_kfs);
        ks.insert("p".into(), p_kfs);
        ks.insert(
            "a".into(),
            json!({"a": 0, "k": [round4(anchor[0]), round4(anchor[1]), 0.0]}),
        );
        ks.insert("s".into(), s_kfs);
        if samples.iter().any(|(_, e)| e.transform.skew.abs() > 1e-4) {
            let sk_kfs = build_scalar_keyframes(&key_frames, |t, f| {
                let ease = if dense {
                    t.2
                } else {
                    ease_for_prop(layer, f, element_id, TweenProperty::Skew).or(t.2)
                };
                (round4(lottie_skew_deg(t.1.skew)), ease)
            });
            ks.insert("sk".into(), sk_kfs);
            ks.insert("sa".into(), json!({"a": 0, "k": 0.0}));
        }
        Value::Object(ks)
    }
}

fn has_asymmetric_pair_ease(eases: &[(TweenProperty, Easing)]) -> bool {
    let lookup = |prop: TweenProperty| eases.iter().find(|(p, _)| *p == prop).map(|(_, e)| *e);
    let x_ease = lookup(TweenProperty::X);
    let y_ease = lookup(TweenProperty::Y);
    if x_ease != y_ease && (x_ease.is_some() || y_ease.is_some()) {
        return true;
    }
    let sx_ease = lookup(TweenProperty::ScaleX);
    let sy_ease = lookup(TweenProperty::ScaleY);
    sx_ease != sy_ease && (sx_ease.is_some() || sy_ease.is_some())
}

fn has_non_alpha_color_transform(ct: ColorTransform) -> bool {
    (ct.multipliers[0] - 1.0).abs() > 1e-4
        || (ct.multipliers[1] - 1.0).abs() > 1e-4
        || (ct.multipliers[2] - 1.0).abs() > 1e-4
        || ct.offsets[0].abs() > 0.5
        || ct.offsets[1].abs() > 0.5
        || ct.offsets[2].abs() > 0.5
}

fn lottie_skew_deg(skew: f64) -> f64 {
    if !skew.is_finite() {
        return 0.0;
    }
    skew.clamp(-85.0, 85.0)
}

fn lottie_skew_cos(skew: f64) -> f64 {
    if skew.abs() <= 1e-6 || !skew.is_finite() {
        return 1.0;
    }
    skew.clamp(-85.0, 85.0).to_radians().cos()
}

fn classic_span_rotation_delta(kf: &Keyframe, a_rot: f64, b_rot: f64) -> (f64, bool) {
    let turns = (kf.classic_rotate_turns.min(Keyframe::MAX_ROTATE_TURNS) as f64) * 360.0;
    match kf.classic_rotate_dir {
        ClassicRotateDirection::Auto => {
            if turns == 0.0 {
                (b_rot - a_rot, false)
            } else {
                let delta = b_rot - a_rot;
                let sign = if delta < 0.0 { -1.0 } else { 1.0 };
                (delta + sign * turns, false)
            }
        }
        ClassicRotateDirection::None => (b_rot - a_rot, true),
        ClassicRotateDirection::Cw => {
            let cw_delta = (b_rot - a_rot).rem_euclid(360.0) + turns;
            (cw_delta, false)
        }
        ClassicRotateDirection::Ccw => {
            let ccw_delta = (a_rot - b_rot).rem_euclid(360.0) + turns;
            (-ccw_delta, false)
        }
    }
}

fn compute_lottie_auto_orient_deg(key_frames: &[(u32, Transform, Option<Easing>)]) -> Vec<f64> {
    let n = key_frames.len();
    if n < 2 {
        return vec![0.0; n];
    }
    let chord_deg = |a: usize, b: usize| -> Option<f64> {
        let dx = key_frames[b].1.x - key_frames[a].1.x;
        let dy = key_frames[b].1.y - key_frames[a].1.y;
        (dx.hypot(dy) > 1e-6).then(|| dy.atan2(dx).to_degrees())
    };
    let mut raw = vec![0.0; n];
    for (idx, slot) in raw.iter_mut().enumerate() {
        if let Some(deg) = (idx + 1 < n).then(|| chord_deg(idx, idx + 1)).flatten() {
            *slot = deg;
        } else if let Some(deg) = (idx > 0).then(|| chord_deg(idx - 1, idx)).flatten() {
            *slot = deg;
        } else {
            for dist in 1..n {
                if idx + dist < n
                    && let Some(deg) = chord_deg(idx, idx + dist)
                {
                    *slot = deg;
                    break;
                }
                if idx >= dist
                    && let Some(deg) = chord_deg(idx - dist, idx)
                {
                    *slot = deg;
                    break;
                }
            }
        }
    }
    raw
}

fn compute_unwrapped_rotations(
    layer: &Layer,
    keys: &[(u32, Transform, Option<Easing>)],
    dense: bool,
) -> Vec<(f64, bool)> {
    if keys.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(keys.len());
    if !dense {
        let mut turn_offset = 0.0f64;
        for (idx, (f, t, _)) in keys.iter().enumerate() {
            let curr_unwrapped = t.rotation + turn_offset;
            let mut hold = false;
            if idx + 1 < keys.len() {
                let next_f = keys[idx + 1].0;
                let next_raw_rot = keys[idx + 1].1.rotation;
                if motion_span(layer, *f).is_none()
                    && let Some(kf) = layer.key_at(*f)
                    && kf.frame == *f
                    && layer.keyframes.iter().any(|nk| nk.frame == next_f)
                {
                    if kf.tween.is_some() && kf.tween_kind == TweenKind::Classic {
                        let (delta, is_hold) =
                            classic_span_rotation_delta(kf, t.rotation, next_raw_rot);
                        hold = is_hold;
                        if !is_hold {
                            let target_end = curr_unwrapped + delta;
                            turn_offset = target_end - next_raw_rot;
                        }
                    } else if kf.tween.is_none() {
                        hold = true;
                    }
                }
            }
            out.push((curr_unwrapped, hold));
        }
    } else {
        let mut prev_unwrapped = keys[0].1.rotation;
        let first_hold = motion_span(layer, keys[0].0).is_none()
            && layer.key_at(keys[0].0).is_some_and(|k| {
                k.tween.is_none()
                    || (k.tween_kind == TweenKind::Classic
                        && k.classic_rotate_dir == ClassicRotateDirection::None)
            });
        out.push((prev_unwrapped, first_hold));
        for idx in 1..keys.len() {
            let (curr_f, curr_t, _) = keys[idx];
            let prev_f = keys[idx - 1].0;
            let prev_hold = out[idx - 1].1;
            let curr_unwrapped = if prev_hold {
                curr_t.rotation
            } else if motion_span(layer, prev_f).is_none()
                && layer.keyframes.iter().any(|k| k.frame == curr_f)
                && let Some(start_kf) = layer.key_at(prev_f)
                && start_kf.tween.is_some()
                && start_kf.tween_kind == TweenKind::Classic
            {
                // Exact end keyframe of a classic spin span: unwrap nearest to `prev_unwrapped`
                // while respecting multi-turn direction if step >= 180 deg.
                let k = ((prev_unwrapped - curr_t.rotation) / 360.0).round();
                curr_t.rotation + k * 360.0
            } else {
                let k = ((prev_unwrapped - curr_t.rotation) / 360.0).round();
                curr_t.rotation + k * 360.0
            };
            let hold = motion_span(layer, curr_f).is_none()
                && layer.key_at(curr_f).is_some_and(|k| {
                    k.tween.is_none()
                        || (k.tween_kind == TweenKind::Classic
                            && k.classic_rotate_dir == ClassicRotateDirection::None)
                });
            prev_unwrapped = curr_unwrapped;
            out.push((curr_unwrapped, hold));
        }
    }
    out
}

#[derive(Clone, Debug)]
pub(super) struct LayerSpan {
    pub(super) start_frame: u32,
    pub(super) end_frame: u32,
}

fn collect_layer_spans(layer: &Layer, layer_end: u32) -> Vec<LayerSpan> {
    let mut spans = Vec::new();
    let mut current_start: Option<u32> = None;
    for f in 0..layer_end {
        let non_empty = layer.key_at(f).is_some_and(|k| !k.elements.is_empty());
        match (current_start, non_empty) {
            (None, true) => current_start = Some(f),
            (Some(start), false) => {
                spans.push(LayerSpan {
                    start_frame: start,
                    end_frame: f,
                });
                current_start = None;
            }
            _ => {}
        }
    }
    if let Some(start) = current_start {
        spans.push(LayerSpan {
            start_frame: start,
            end_frame: layer_end,
        });
    }
    spans
}

fn collect_transform_keyframes(
    timeline: &Timeline,
    layer: &Layer,
    samples: &[(u32, Element)],
    element_id: &str,
) -> Vec<(u32, Transform, Option<Easing>)> {
    let first_f = samples.first().unwrap().0;
    let last_f = samples.last().unwrap().0;
    let sample_map: BTreeMap<u32, Transform> =
        samples.iter().map(|(f, e)| (*f, e.transform)).collect();

    let mut key_times: BTreeSet<u32> = BTreeSet::new();
    key_times.insert(first_f);
    key_times.insert(last_f);
    for kf in &layer.keyframes {
        let relevant =
            kf.keyed_properties.is_none() || !kf.motion_key_properties_for(element_id).is_empty();
        if relevant
            && kf.frame >= first_f
            && kf.frame <= last_f
            && sample_map.contains_key(&kf.frame)
        {
            key_times.insert(kf.frame);
            // If a hold keyframe transitions at `next_kf.frame`, include `next_kf.frame - 1` or
            // mark the keyframe as hold (`None`).
        }
    }
    let _ = timeline;

    let times: Vec<u32> = key_times.into_iter().collect();
    let mut out = Vec::with_capacity(times.len());
    for &f in &times {
        let t = sample_map[&f];
        let ease = layer_ease_at(layer, f, element_id);
        out.push((f, t, ease));
    }
    out
}

pub(super) fn layer_ease_at(layer: &Layer, frame: u32, element_id: &str) -> Option<Easing> {
    if let Some((start, _, _)) = motion_span(layer, frame) {
        return Some(start.motion_easing_for(element_id));
    }
    layer.key_at(frame).and_then(|k| k.tween)
}

fn ease_for_prop(
    layer: &Layer,
    frame: u32,
    element_id: &str,
    prop: TweenProperty,
) -> Option<Easing> {
    if let Some((start, _, _)) = motion_span(layer, frame) {
        return Some(start.motion_property_ease_for(element_id, prop));
    }
    layer.key_at(frame).and_then(|k| k.tween)
}

pub(super) fn easing_to_lottie_tangents(ease: Easing) -> ([f64; 2], [f64; 2]) {
    match ease {
        Easing::Linear => ([0.0, 0.0], [1.0, 1.0]),
        Easing::EaseIn => ([0.3333, 0.0], [0.6667, 0.3333]),
        Easing::EaseOut => ([0.3333, 0.6667], [0.6667, 1.0]),
        Easing::EaseInOut => ([0.42, 0.0], [0.58, 1.0]),
        Easing::CubicBezier { x1, y1, x2, y2 } => (
            [round4(x1.clamp(0.0, 1.0)), round4(y1)],
            [round4(x2.clamp(0.0, 1.0)), round4(y2)],
        ),
        Easing::Flash { acceleration } => {
            let a = (acceleration / 100.0).clamp(-1.0, 1.0);
            if a < 0.0 {
                let w = -a;
                (
                    [0.3333, round4(0.3333 * (1.0 - w))],
                    [0.6667, round4(0.6667 * (1.0 - w) + 0.3333 * w)],
                )
            } else {
                let w = a;
                (
                    [0.3333, round4(0.3333 * (1.0 - w) + 0.6667 * w)],
                    [0.6667, round4(0.6667 * (1.0 - w) + 1.0 * w)],
                )
            }
        }
        Easing::Bounce | Easing::Elastic => ([0.0, 0.0], [1.0, 1.0]),
    }
}

fn build_vec3_keyframes(
    keys: &[(u32, Transform, Option<Easing>)],
    extract: impl Fn(&(u32, Transform, Option<Easing>), u32) -> ([f64; 3], Option<Easing>),
) -> Value {
    let first = extract(&keys[0], keys[0].0).0;
    if keys.iter().all(|k| extract(k, k.0).0 == first) {
        return json!({"a": 0, "k": first});
    }
    let mut arr = Vec::with_capacity(keys.len());
    for (idx, k) in keys.iter().enumerate() {
        let (val, ease) = extract(k, k.0);
        if idx + 1 < keys.len() {
            if let Some(e) = ease {
                let (o, i) = easing_to_lottie_tangents(e);
                arr.push(json!({
                    "t": k.0,
                    "s": val,
                    "o": {"x": [o[0]], "y": [o[1]]},
                    "i": {"x": [i[0]], "y": [i[1]]}
                }));
            } else {
                arr.push(json!({
                    "t": k.0,
                    "s": val,
                    "h": 1
                }));
            }
        } else {
            arr.push(json!({
                "t": k.0,
                "s": val
            }));
        }
    }
    json!({"a": 1, "k": arr})
}

fn build_scalar_keyframes(
    keys: &[(u32, Transform, Option<Easing>)],
    extract: impl Fn(&(u32, Transform, Option<Easing>), u32) -> (f64, Option<Easing>),
) -> Value {
    let first = extract(&keys[0], keys[0].0).0;
    if keys
        .iter()
        .all(|k| (extract(k, k.0).0 - first).abs() < 1e-4)
    {
        return json!({"a": 0, "k": first});
    }
    let mut arr = Vec::with_capacity(keys.len());
    for (idx, k) in keys.iter().enumerate() {
        let (val, ease) = extract(k, k.0);
        if idx + 1 < keys.len() {
            if let Some(e) = ease {
                let (o, i) = easing_to_lottie_tangents(e);
                arr.push(json!({
                    "t": k.0,
                    "s": [val],
                    "o": {"x": [o[0]], "y": [o[1]]},
                    "i": {"x": [i[0]], "y": [i[1]]}
                }));
            } else {
                arr.push(json!({
                    "t": k.0,
                    "s": [val],
                    "h": 1
                }));
            }
        } else {
            arr.push(json!({
                "t": k.0,
                "s": [val]
            }));
        }
    }
    json!({"a": 1, "k": arr})
}

fn build_indexed_scalar_keyframes(
    keys: &[(u32, Transform, Option<Easing>)],
    extract: impl Fn(usize, &(u32, Transform, Option<Easing>), u32) -> (f64, Option<Easing>),
) -> Value {
    let first = extract(0, &keys[0], keys[0].0).0;
    if keys
        .iter()
        .enumerate()
        .all(|(idx, k)| (extract(idx, k, k.0).0 - first).abs() < 1e-4)
    {
        return json!({"a": 0, "k": first});
    }
    let mut arr = Vec::with_capacity(keys.len());
    for (idx, k) in keys.iter().enumerate() {
        let (val, ease) = extract(idx, k, k.0);
        if idx + 1 < keys.len() {
            if let Some(e) = ease {
                let (o, i) = easing_to_lottie_tangents(e);
                arr.push(json!({
                    "t": k.0,
                    "s": [val],
                    "o": {"x": [o[0]], "y": [o[1]]},
                    "i": {"x": [i[0]], "y": [i[1]]}
                }));
            } else {
                arr.push(json!({
                    "t": k.0,
                    "s": [val],
                    "h": 1
                }));
            }
        } else {
            arr.push(json!({
                "t": k.0,
                "s": [val]
            }));
        }
    }
    json!({"a": 1, "k": arr})
}

pub(super) fn static_ks(t: Transform, anchor: [f64; 2], origin_offset: [f64; 2]) -> Value {
    let sk_cos = lottie_skew_cos(t.skew);
    let mut ks = Map::new();
    ks.insert(
        "o".into(),
        json!({"a": 0, "k": round4(effective_opacity_percent(t))}),
    );
    ks.insert("r".into(), json!({"a": 0, "k": round4(t.rotation)}));
    ks.insert(
        "p".into(),
        json!({"a": 0, "k": [round4(t.x + origin_offset[0]), round4(t.y + origin_offset[1]), 0.0]}),
    );
    ks.insert(
        "a".into(),
        json!({"a": 0, "k": [round4(anchor[0]), round4(anchor[1]), 0.0]}),
    );
    ks.insert(
        "s".into(),
        json!({"a": 0, "k": [round4(t.scale_x * 100.0), round4(t.scale_y * sk_cos * 100.0), 100.0]}),
    );
    if t.skew.abs() > 1e-4 {
        ks.insert(
            "sk".into(),
            json!({"a": 0, "k": round4(lottie_skew_deg(t.skew))}),
        );
        ks.insert("sa".into(), json!({"a": 0, "k": 0.0}));
    }
    Value::Object(ks)
}

fn effective_opacity_percent(t: Transform) -> f64 {
    let ct_alpha =
        (t.color_transform.multipliers[3] + t.color_transform.offsets[3] / 255.0).clamp(0.0, 1.0);
    (t.alpha.clamp(0.0, 1.0) * ct_alpha * 100.0).clamp(0.0, 100.0)
}

pub(super) fn invert_affine(m: [f64; 6]) -> Option<[f64; 6]> {
    let det = m[0] * m[3] - m[1] * m[2];
    if det.abs() < 1e-12 {
        return None;
    }
    let inv_det = 1.0 / det;
    let a = m[3] * inv_det;
    let b = -m[1] * inv_det;
    let c = -m[2] * inv_det;
    let d = m[0] * inv_det;
    let tx = -(a * m[4] + c * m[5]);
    let ty = -(b * m[4] + d * m[5]);
    Some([a, b, c, d, tx, ty])
}

pub(super) fn transforms_approx_eq(a: Transform, b: Transform) -> bool {
    (a.x - b.x).abs() < 1e-4
        && (a.y - b.y).abs() < 1e-4
        && (a.scale_x - b.scale_x).abs() < 1e-4
        && (a.scale_y - b.scale_y).abs() < 1e-4
        && (a.rotation - b.rotation).abs() < 1e-4
        && (a.alpha - b.alpha).abs() < 1e-4
        && (a.skew - b.skew).abs() < 1e-4
        && a.color_transform == b.color_transform
}

pub(super) fn layer_element_name(layer: &Layer, el: &Element) -> String {
    if el.name.is_empty() {
        layer.name.clone()
    } else {
        format!("{} · {}", layer.name, el.name)
    }
}
