use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn build_ae_comp(
    project: &Project,
    sym: &Symbol,
    width: u32,
    height: u32,
    origin: [f64; 2],
    fps: f64,
    background: [f64; 3],
    folder: String,
    options: &AepOptions,
    comp_origins: &BTreeMap<String, ([f64; 2], [u32; 2])>,
    symbol_time_remap_exprs: &BTreeMap<String, String>,
    resource_to_footage: &BTreeMap<String, AeFootageItem>,
    diagnostics: &mut Vec<AepDiagnostic>,
) -> AeComp {
    let duration_frames = sym.timeline.frame_count.max(1);
    let label_map = collect_timeline_labels(&sym.timeline);

    let (named_script_drivers, script_time_remap_expr, mut comp_markers) =
        if options.translate_scripts {
            analyze_symbol_scripts_for_ae(sym, &label_map, diagnostics)
        } else {
            (BTreeMap::new(), None, Vec::new())
        };

    // Frame labels & rich timeline markers -> AE Composition Markers
    for (lbl, frame) in &label_map {
        comp_markers.push(AeMarker {
            frame: *frame,
            duration_frames: 0,
            label: lbl.clone(),
            comment: "Frame label".into(),
        });
    }
    let mut work_area: Option<[u32; 2]> = None;
    for layer in &sym.timeline.layers {
        for kf in &layer.keyframes {
            if work_area.is_none()
                && let Some(wa) = &kf.work_area
                && wa.enabled
            {
                work_area = Some([wa.in_frame, wa.out_frame.max(wa.in_frame + 1)]);
            }
            for tm in &kf.markers {
                comp_markers.push(AeMarker {
                    frame: tm.frame,
                    duration_frames: tm.duration_frames,
                    label: tm.name.clone(),
                    comment: tm.comment.clone(),
                });
            }
        }
    }

    let guides: Vec<AeGuide> = sym
        .timeline
        .guides
        .iter()
        .map(|g| AeGuide {
            horizontal: g.axis == GuideAxis::Horizontal,
            position: g.position
                + if g.axis == GuideAxis::Horizontal {
                    origin[1]
                } else {
                    origin[0]
                },
        })
        .collect();

    // Map mask layers (`LayerRole::Mask`) so target layers (`layer.properties.mask == Some(mask_id)`)
    // get paired with an immediate After Effects Track Matte copy above them.
    let mask_layers: BTreeMap<String, &Layer> = sym
        .timeline
        .layers
        .iter()
        .filter(|l| l.properties.role == LayerRole::Mask)
        .map(|l| (l.id.clone(), l))
        .collect();

    let mut ae_layers: Vec<AeLayer> = Vec::new();
    let mut element_id_to_ae_layer_id: BTreeMap<String, String> = BTreeMap::new();
    let mut pending_parent_links: Vec<(String, String)> = Vec::new();
    let mut camera_keyframes: Vec<(u32, neoflash_core::CameraPose)> = Vec::new();

    for layer in &sym.timeline.layers {
        for kf in &layer.keyframes {
            if let Some(cam) = &kf.camera {
                camera_keyframes.push((kf.frame, *cam));
            }
            for link in &kf.element_parents {
                pending_parent_links.push((link.child_id.clone(), link.parent_id.clone()));
            }
        }

        if matches!(
            layer.properties.role,
            LayerRole::Guide | LayerRole::Folder | LayerRole::Mask
        ) {
            continue;
        }

        let layer_end = layer
            .properties
            .end_frame
            .unwrap_or(duration_frames)
            .min(duration_frames);
        if layer_end == 0 {
            continue;
        }

        // 1. Frame sounds on this layer -> AE Audio Footage layers
        for (k_idx, kf) in layer.keyframes.iter().enumerate() {
            if let Some(snd) = &kf.sound {
                if snd.sync == SoundSync::Stop {
                    continue;
                }
                if let Some(ft) = resource_to_footage.get(&snd.resource) {
                    let next_frame = layer
                        .keyframes
                        .get(k_idx + 1)
                        .map(|n| n.frame.min(layer_end))
                        .unwrap_or(layer_end);
                    let start_sec = snd.in_point.unwrap_or(0.0).max(0.0);
                    let audio_levels_db = build_sound_levels_keys(kf.frame, next_frame, snd);
                    ae_layers.push(AeLayer {
                        id: format!("snd-{}-{}", layer.id, kf.frame),
                        name: format!("{} · Audio", layer.name),
                        source_element_id: None,
                        in_frame: kf.frame,
                        out_frame: next_frame.max(kf.frame + 1),
                        visible: false,
                        locked: layer.locked,
                        solo: false,
                        is_track_matte_source: false,
                        track_matte: AeTrackMatte::None,
                        blend_mode: "NORMAL",
                        auto_orient_along_path: false,
                        three_d: false,
                        z_depth: 0.0,
                        parent_layer_id: None,
                        anchor_point: [0.0, 0.0],
                        position_keys: Vec::new(),
                        scale_keys: Vec::new(),
                        rotation_keys: Vec::new(),
                        opacity_keys: Vec::new(),
                        skew_keys: Vec::new(),
                        expressions: AeLayerExpressions::default(),
                        effects: Vec::new(),
                        markers: Vec::new(),
                        kind: AeLayerKind::Footage {
                            footage_id: ft.id.clone(),
                            is_audio_only: true,
                            is_video: false,
                            start_offset_sec: start_sec,
                            end_offset_sec: snd.out_point,
                            loop_count: if snd.loop_forever {
                                999
                            } else {
                                snd.loops.max(1)
                            },
                            muted: false,
                            audio_levels_db,
                        },
                    });
                }
            }
        }

        // 2. Shape Tween spans (`TweenKind::Shape`) -> Animated Bezier Shape Layer
        let mut handled_spans: BTreeSet<usize> = BTreeSet::new();
        for (k_idx, kf) in layer.keyframes.iter().enumerate() {
            if kf.tween.is_some() && kf.tween_kind == TweenKind::Shape {
                let end_frame = layer
                    .keyframes
                    .get(k_idx + 1)
                    .map(|n| n.frame.min(layer_end))
                    .unwrap_or(layer_end);
                if end_frame > kf.frame
                    && let Some(shape_layer) = build_shape_tween_ae_layer(
                        &sym.timeline,
                        layer,
                        kf,
                        end_frame,
                        origin,
                        fps as u32,
                        options.shape_morph_step.max(1),
                    )
                {
                    handled_spans.insert(k_idx);
                    push_layer_with_optional_track_matte(
                        &mut ae_layers,
                        shape_layer,
                        layer,
                        &mask_layers,
                        project,
                        &sym.timeline,
                        origin,
                        fps as u32,
                    );
                }
            }
        }

        // 3. Collect visual element tracks across non-shape-tween spans on this layer
        let tracks = collect_layer_element_tracks(
            project,
            sym,
            layer,
            layer_end,
            &handled_spans,
            origin,
            fps,
            options,
            &named_script_drivers,
            comp_origins,
            symbol_time_remap_exprs,
            resource_to_footage,
            diagnostics,
        );
        for built in tracks {
            if let Some(eid) = &built.source_element_id {
                element_id_to_ae_layer_id.insert(eid.clone(), built.id.clone());
            }
            push_layer_with_optional_track_matte(
                &mut ae_layers,
                built,
                layer,
                &mask_layers,
                project,
                &sym.timeline,
                origin,
                fps as u32,
            );
        }
    }

    // Wire element parenting (`layer.parent = parentLayer`)
    for (child_eid, parent_eid) in pending_parent_links {
        if let (Some(child_lid), Some(parent_lid)) = (
            element_id_to_ae_layer_id.get(&child_eid).cloned(),
            element_id_to_ae_layer_id.get(&parent_eid).cloned(),
        ) && child_lid != parent_lid
            && let Some(layer) = ae_layers.iter_mut().find(|l| l.id == child_lid)
        {
            layer.parent_layer_id = Some(parent_lid);
        }
    }

    // Optional Timeline Camera -> AE CameraLayer at the top of the composition
    if !camera_keyframes.is_empty() {
        camera_keyframes.sort_by_key(|(f, _)| *f);
        let first_focal = camera_keyframes[0].1.focal_length.max(50.0);
        let cx = f64::from(width) * 0.5;
        let cy = f64::from(height) * 0.5;
        let mut pos_keys = Vec::new();
        let mut rot_keys = Vec::new();
        let mut zoom_keys = Vec::new();
        for (frame, pose) in &camera_keyframes {
            pos_keys.push(AeKey {
                frame: *frame,
                value: [cx + pose.x, cy + pose.y],
                ease: AeEase::default(),
            });
            rot_keys.push(AeKey {
                frame: *frame,
                value: pose.rotation,
                ease: AeEase::default(),
            });
            zoom_keys.push(AeKey {
                frame: *frame,
                value: first_focal * pose.zoom.max(0.05),
                ease: AeEase::default(),
            });
        }
        ae_layers.push(AeLayer {
            id: format!("cam-{}", sym.id),
            name: "NeoFlash Camera".into(),
            source_element_id: None,
            in_frame: 0,
            out_frame: duration_frames,
            visible: true,
            locked: false,
            solo: false,
            is_track_matte_source: false,
            track_matte: AeTrackMatte::None,
            blend_mode: "NORMAL",
            auto_orient_along_path: false,
            three_d: true,
            z_depth: -first_focal,
            parent_layer_id: None,
            anchor_point: [cx, cy],
            position_keys: pos_keys,
            scale_keys: Vec::new(),
            rotation_keys: rot_keys,
            opacity_keys: Vec::new(),
            skew_keys: Vec::new(),
            expressions: AeLayerExpressions::default(),
            effects: Vec::new(),
            markers: Vec::new(),
            kind: AeLayerKind::Camera {
                focal_length: first_focal,
                zoom_keys,
            },
        });
    }

    AeComp {
        id: sym.id.clone(),
        name: if sym.name.trim().is_empty() {
            sym.id.clone()
        } else {
            sym.name.clone()
        },
        folder,
        width,
        height,
        fps,
        duration_frames,
        background,
        origin_offset: origin,
        work_area,
        guides,
        markers: comp_markers,
        layers: ae_layers,
        script_time_remap_expr,
    }
}

#[allow(clippy::too_many_arguments)]
fn push_layer_with_optional_track_matte(
    ae_layers: &mut Vec<AeLayer>,
    mut target_layer: AeLayer,
    authored_layer: &Layer,
    mask_layers: &BTreeMap<String, &Layer>,
    project: &Project,
    timeline: &Timeline,
    origin: [f64; 2],
    fps: u32,
) {
    if let Some(mask_id) = &authored_layer.properties.mask
        && let Some(mask_src) = mask_layers.get(mask_id).copied()
        && let Some(mut matte_layer) = build_mask_matte_layer(
            project,
            timeline,
            mask_src,
            target_layer.in_frame,
            target_layer.out_frame,
            origin,
            fps,
        )
    {
        let matte_mode = match (
            mask_src.properties.mask_mode,
            mask_src.properties.mask_invert,
        ) {
            (MaskMode::Luminance, false) => AeTrackMatte::Luma,
            (MaskMode::Luminance, true) => AeTrackMatte::LumaInverted,
            (_, false) => AeTrackMatte::Alpha,
            (_, true) => AeTrackMatte::AlphaInverted,
        };
        target_layer.track_matte = matte_mode;
        matte_layer.id = format!("matte-for-{}", target_layer.id);
        matte_layer.is_track_matte_source = true;
        matte_layer.visible = false;
        // In bottom-to-top order, `target_layer` is below `matte_layer` (so in AE's 1..N top-down
        // stack, `matte_layer` sits directly above `target_layer`, as required by AE Track Matte!).
        ae_layers.push(target_layer);
        ae_layers.push(matte_layer);
        return;
    }
    ae_layers.push(target_layer);
}

fn build_mask_matte_layer(
    project: &Project,
    timeline: &Timeline,
    mask_layer: &Layer,
    in_frame: u32,
    out_frame: u32,
    origin: [f64; 2],
    fps: u32,
) -> Option<AeLayer> {
    let sampled = sample_layer_in_at_fps(timeline, mask_layer, in_frame, f64::from(fps));
    let mut groups = Vec::new();
    for (idx, el) in sampled.iter().enumerate() {
        if let Some(grp) = build_static_shape_group_for_element(project, el, idx, true) {
            // Shift mask contour into element's local transform at `in_frame`
            groups.push(grp);
        }
    }
    if groups.is_empty() {
        return None;
    }
    let first_t = sampled.first().map(|e| e.transform).unwrap_or_default();
    Some(AeLayer {
        id: format!("mask-{}", mask_layer.id),
        name: format!("{} (Track Matte)", mask_layer.name),
        source_element_id: None,
        in_frame,
        out_frame: out_frame.max(in_frame + 1),
        visible: false,
        locked: true,
        solo: false,
        is_track_matte_source: true,
        track_matte: AeTrackMatte::None,
        blend_mode: "NORMAL",
        auto_orient_along_path: false,
        three_d: false,
        z_depth: 0.0,
        parent_layer_id: None,
        anchor_point: [0.0, 0.0],
        position_keys: vec![AeKey {
            frame: in_frame,
            value: [first_t.x + origin[0], first_t.y + origin[1]],
            ease: AeEase::default(),
        }],
        scale_keys: vec![AeKey {
            frame: in_frame,
            value: [first_t.scale_x * 100.0, first_t.scale_y * 100.0],
            ease: AeEase::default(),
        }],
        rotation_keys: vec![AeKey {
            frame: in_frame,
            value: first_t.rotation,
            ease: AeEase::default(),
        }],
        opacity_keys: vec![AeKey {
            frame: in_frame,
            value: (first_t.alpha * 100.0).clamp(0.0, 100.0),
            ease: AeEase::default(),
        }],
        skew_keys: Vec::new(),
        expressions: AeLayerExpressions::default(),
        effects: Vec::new(),
        markers: Vec::new(),
        kind: AeLayerKind::Shape { groups },
    })
}

fn build_shape_tween_ae_layer(
    timeline: &Timeline,
    layer: &Layer,
    start_kf: &Keyframe,
    end_frame: u32,
    origin: [f64; 2],
    fps: u32,
    morph_step: u32,
) -> Option<AeLayer> {
    let mut frames: Vec<u32> = (start_kf.frame..end_frame)
        .step_by(morph_step as usize)
        .collect();
    if frames.last().copied() != Some(end_frame.saturating_sub(1)) {
        frames.push(end_frame.saturating_sub(1));
    }

    let first_sample = sample_layer_in_at_fps(timeline, layer, start_kf.frame, f64::from(fps));
    let first_el = first_sample.first()?;
    let (_, fill_rule, fill, stroke, fill_grad, _, stroke_width) =
        element_vector_commands(first_el)?;

    let mut contour_tracks: Vec<Vec<AeKey<AeBezierContour>>> = Vec::new();
    for &f in &frames {
        let sampled = sample_layer_in_at_fps(timeline, layer, f, f64::from(fps));
        let mut all_contours = Vec::new();
        for el in &sampled {
            if let Some((cmds, _, _, _, _, _, _)) = element_vector_commands(el) {
                let c = path_commands_to_ae_contours(
                    &cmds,
                    [el.transform.x + origin[0], el.transform.y + origin[1]],
                );
                all_contours.extend(c);
            }
        }
        for (c_idx, contour) in all_contours.into_iter().enumerate() {
            if contour_tracks.len() <= c_idx {
                contour_tracks.push(Vec::new());
            }
            contour_tracks[c_idx].push(AeKey {
                frame: f,
                value: contour,
                ease: AeEase::default(),
            });
        }
    }
    if contour_tracks.is_empty() {
        return None;
    }

    let even_odd = fill_rule == FillRule::EvenOdd;
    let fill_paint = build_ae_fill(fill, fill_grad.as_ref(), even_odd);
    let mut strokes = Vec::new();
    if stroke_width > 0.0 && stroke[3] > 0 {
        strokes.push(AePaintStroke {
            color: color_rgb01(stroke),
            opacity: (f64::from(stroke[3]) / 255.0) * 100.0,
            width: stroke_width,
            dashes: None,
        });
    }

    Some(AeLayer {
        id: format!("morph-{}-{}", layer.id, start_kf.frame),
        name: format!("{} · Shape Morph", layer.name),
        source_element_id: Some(first_el.id.clone()),
        in_frame: start_kf.frame,
        out_frame: end_frame,
        visible: layer.visible,
        locked: layer.locked,
        solo: false,
        is_track_matte_source: false,
        track_matte: AeTrackMatte::None,
        blend_mode: ae_blend_mode(layer.properties.blend_mode),
        auto_orient_along_path: false,
        three_d: layer.properties.z_depth.abs() > 1e-6,
        z_depth: layer.properties.z_depth,
        parent_layer_id: None,
        anchor_point: [0.0, 0.0],
        position_keys: vec![AeKey {
            frame: start_kf.frame,
            value: [0.0, 0.0],
            ease: AeEase::default(),
        }],
        scale_keys: vec![AeKey {
            frame: start_kf.frame,
            value: [100.0, 100.0],
            ease: AeEase::default(),
        }],
        rotation_keys: vec![AeKey {
            frame: start_kf.frame,
            value: 0.0,
            ease: AeEase::default(),
        }],
        opacity_keys: vec![AeKey {
            frame: start_kf.frame,
            value: 100.0,
            ease: AeEase::default(),
        }],
        skew_keys: Vec::new(),
        expressions: AeLayerExpressions::default(),
        effects: convert_filters_to_ae(&layer.properties.filters, ColorTransform::default()),
        markers: Vec::new(),
        kind: AeLayerKind::Shape {
            groups: vec![AeShapeGroup {
                name: "Morph Group".into(),
                contour_tracks,
                fills: if matches!(fill_paint, AePaintFill::None) {
                    Vec::new()
                } else {
                    vec![fill_paint]
                },
                strokes,
                modifiers: Vec::new(),
            }],
        },
    })
}

#[allow(clippy::too_many_arguments)]
fn collect_layer_element_tracks(
    project: &Project,
    sym: &Symbol,
    layer: &Layer,
    layer_end: u32,
    handled_spans: &BTreeSet<usize>,
    origin: [f64; 2],
    fps: f64,
    options: &AepOptions,
    named_script_drivers: &BTreeMap<String, ScriptPropertyDrivers>,
    comp_origins: &BTreeMap<String, ([f64; 2], [u32; 2])>,
    symbol_time_remap_exprs: &BTreeMap<String, String>,
    resource_to_footage: &BTreeMap<String, AeFootageItem>,
    diagnostics: &mut Vec<AepDiagnostic>,
) -> Vec<AeLayer> {
    let mut ordered_ids: Vec<String> = Vec::new();
    let mut by_id: BTreeMap<String, Vec<SpanSample>> = BTreeMap::new();

    for (k_idx, kf) in layer.keyframes.iter().enumerate() {
        if handled_spans.contains(&k_idx) || kf.frame >= layer_end {
            continue;
        }
        let span_end = layer
            .keyframes
            .get(k_idx + 1)
            .map(|n| n.frame.min(layer_end))
            .unwrap_or(layer_end);
        if span_end <= kf.frame {
            continue;
        }

        let has_guide = layer.properties.motion_guide.is_some() || kf.motion_path.is_some();
        for el in &kf.elements {
            if !by_id.contains_key(&el.id) {
                ordered_ids.push(el.id.clone());
            }
            let appearance = kf
                .appearances
                .iter()
                .find(|a| a.element_id == el.id)
                .cloned();
            let is_controller = kf.controller_elements.iter().any(|cid| cid == &el.id);
            by_id.entry(el.id.clone()).or_default().push(SpanSample {
                frame: kf.frame,
                end_frame: span_end,
                element: el.clone(),
                easing: kf.tween,
                orient_to_path: kf.motion_orient_to_path_for(&el.id),
                curved_motion: has_guide
                    || !matches!(
                        kf.motion_path_for(&el.id),
                        neoflash_core::MotionPathSource::None
                    ),
                rotate_turns: kf.classic_rotate_turns,
                property_expressions: kf.property_expressions.clone(),
                video_span: kf.video.clone(),
                is_controller,
                appearance,
                kf_filters: kf.filters.clone(),
            });
        }
    }

    let mut out_layers = Vec::new();
    for eid in ordered_ids {
        let Some(samples) = by_id.remove(&eid) else {
            continue;
        };
        // Split into contiguous active intervals `[in_frame, out_frame)`
        let mut run_start = 0usize;
        while run_start < samples.len() {
            let mut run_end = run_start + 1;
            while run_end < samples.len()
                && samples[run_end].frame == samples[run_end - 1].end_frame
            {
                run_end += 1;
            }
            let slice = &samples[run_start..run_end];
            if let Some(ae_layer) = build_ae_layer_from_run(
                project,
                sym,
                layer,
                slice,
                origin,
                fps,
                options,
                named_script_drivers,
                comp_origins,
                symbol_time_remap_exprs,
                resource_to_footage,
                diagnostics,
            ) {
                out_layers.push(ae_layer);
            }
            run_start = run_end;
        }
    }
    out_layers
}

struct SpanSample {
    frame: u32,
    end_frame: u32,
    element: Element,
    easing: Option<Easing>,
    orient_to_path: bool,
    curved_motion: bool,
    rotate_turns: u32,
    property_expressions: Vec<PropertyExpression>,
    video_span: Option<neoflash_core::TimelineVideoSpan>,
    is_controller: bool,
    appearance: Option<neoflash_core::ElementAppearance>,
    kf_filters: Vec<FilterSpec>,
}

#[allow(clippy::too_many_arguments)]
fn build_ae_layer_from_run(
    project: &Project,
    sym: &Symbol,
    layer: &Layer,
    run: &[SpanSample],
    origin: [f64; 2],
    fps: f64,
    options: &AepOptions,
    named_script_drivers: &BTreeMap<String, ScriptPropertyDrivers>,
    comp_origins: &BTreeMap<String, ([f64; 2], [u32; 2])>,
    symbol_time_remap_exprs: &BTreeMap<String, String>,
    resource_to_footage: &BTreeMap<String, AeFootageItem>,
    diagnostics: &mut Vec<AepDiagnostic>,
) -> Option<AeLayer> {
    let first = &run[0];
    let last = &run[run.len() - 1];
    let in_frame = first.frame;
    let out_frame = last.end_frame.max(in_frame + 1);
    let el = &first.element;
    let pivot = el.transform.pivot.unwrap_or([0.0, 0.0]);

    // Determine pre-comp origin if this element is an Instance
    let precomp_origin = if let ElementKind::Instance { symbol_id, .. } = &el.kind {
        comp_origins
            .get(symbol_id)
            .map(|(o, _)| *o)
            .unwrap_or([0.0, 0.0])
    } else {
        [0.0, 0.0]
    };

    let anchor_point = [precomp_origin[0] + pivot[0], precomp_origin[1] + pivot[1]];

    let mut pos_keys = Vec::new();
    let mut scale_keys = Vec::new();
    let mut rot_keys = Vec::new();
    let mut opacity_keys = Vec::new();
    let mut skew_keys = Vec::new();
    let mut auto_orient = false;
    let mut accum_turns_deg = 0.0_f64;

    for (idx, item) in run.iter().enumerate() {
        auto_orient |= item.orient_to_path;
        let ease = easing_to_ae(item.easing);
        let next_frame = run.get(idx + 1).map(|n| n.frame);

        let needs_dense_sample =
            item.curved_motion || matches!(item.easing, Some(Easing::Bounce | Easing::Elastic));
        if needs_dense_sample
            && item.easing.is_some()
            && let Some(nf) = next_frame
            && nf > item.frame + 1
        {
            for sf in item.frame..nf {
                let sampled = sample_layer_in_at_fps(&sym.timeline, layer, sf, fps);
                if let Some(sel) = sampled.iter().find(|c| c.id == el.id) {
                    push_transform_keys(
                        sf,
                        sel.transform,
                        origin,
                        pivot,
                        accum_turns_deg,
                        AeEase::default(),
                        &mut pos_keys,
                        &mut scale_keys,
                        &mut rot_keys,
                        &mut opacity_keys,
                        &mut skew_keys,
                    );
                }
            }
        } else {
            push_transform_keys(
                item.frame,
                item.element.transform,
                origin,
                pivot,
                accum_turns_deg,
                ease,
                &mut pos_keys,
                &mut scale_keys,
                &mut rot_keys,
                &mut opacity_keys,
                &mut skew_keys,
            );
            if idx + 1 == run.len() && item.easing.is_some() && out_frame > item.frame + 1 {
                let end_f = out_frame - 1;
                let sampled = sample_layer_in_at_fps(&sym.timeline, layer, end_f, fps);
                if let Some(sel) = sampled.iter().find(|c| c.id == el.id) {
                    push_transform_keys(
                        end_f,
                        sel.transform,
                        origin,
                        pivot,
                        accum_turns_deg + f64::from(item.rotate_turns) * 360.0,
                        AeEase::default(),
                        &mut pos_keys,
                        &mut scale_keys,
                        &mut rot_keys,
                        &mut opacity_keys,
                        &mut skew_keys,
                    );
                }
            }
        }
        accum_turns_deg += f64::from(item.rotate_turns) * 360.0;
    }

    let mut expressions = AeLayerExpressions::default();
    let mut markers = Vec::new();

    if options.translate_scripts {
        let mut px_expr: Option<String> = None;
        let mut py_expr: Option<String> = None;
        let mut sx_expr: Option<String> = None;
        let mut sy_expr: Option<String> = None;

        for pe in &first.property_expressions {
            if !pe.enabled {
                continue;
            }
            match pe.property {
                TweenProperty::X => {
                    px_expr = Some(compile_property_expression_to_ae_js(pe, "value[0]"))
                }
                TweenProperty::Y => {
                    py_expr = Some(compile_property_expression_to_ae_js(pe, "value[1]"))
                }
                TweenProperty::ScaleX => {
                    sx_expr = Some(compile_property_expression_to_ae_js(pe, "value[0]"));
                }
                TweenProperty::ScaleY => {
                    sy_expr = Some(compile_property_expression_to_ae_js(pe, "value[1]"));
                }
                TweenProperty::Rotation => {
                    expressions.rotation = Some(compile_property_expression_to_ae_js(pe, "value"));
                }
                TweenProperty::Skew => {
                    expressions.skew = Some(compile_property_expression_to_ae_js(pe, "value"));
                }
                TweenProperty::Alpha => {
                    let compiled = compile_property_expression_to_ae_js(pe, "(value / 100.0)");
                    expressions.opacity = Some(format!("clamp(({compiled}) * 100.0, 0, 100)"));
                }
                TweenProperty::Color => {}
            }
        }
        if px_expr.is_some() || py_expr.is_some() {
            expressions.position = Some(format!(
                "[{}, {}]",
                px_expr.as_deref().unwrap_or("value[0]"),
                py_expr.as_deref().unwrap_or("value[1]")
            ));
        }
        if sx_expr.is_some() || sy_expr.is_some() {
            expressions.scale = Some(format!(
                "[{}, {}]",
                sx_expr.as_deref().unwrap_or("value[0]"),
                sy_expr.as_deref().unwrap_or("value[1]")
            ));
        }

        let (inst_drivers, inst_markers) =
            analyze_element_scripts_for_ae(&sym.name, in_frame, el, diagnostics);
        markers.extend(inst_markers);
        if !inst_drivers.is_empty() {
            inst_drivers.apply_to_expressions(&mut expressions, origin);
        }

        if !el.name.is_empty()
            && let Some(named_drv) = named_script_drivers.get(&el.name)
            && !named_drv.is_empty()
        {
            named_drv.apply_to_expressions(&mut expressions, origin);
        }
    }

    let kind = if first.is_controller {
        AeLayerKind::Null
    } else {
        match &el.kind {
            ElementKind::Rectangle { .. }
            | ElementKind::Ellipse { .. }
            | ElementKind::Path { .. }
            | ElementKind::PaintedPath { .. } => {
                let mut grp = build_static_shape_group_for_element(project, el, 0, false)?;
                if let Some(app) = &first.appearance {
                    apply_element_appearance_to_group(&mut grp, app);
                }
                AeLayerKind::Shape { groups: vec![grp] }
            }
            ElementKind::Text {
                text,
                font_family,
                font_size,
                color,
            } => AeLayerKind::Text {
                text: text.clone(),
                font: if font_family.trim().is_empty() {
                    "ArialMT".into()
                } else {
                    font_family.clone()
                },
                font_size: font_size.max(1.0),
                color: color_rgb01(*color),
                bold: false,
                italic: false,
                align: TextAlign::Left,
                leading: font_size * 1.2,
                tracking: 0.0,
                box_size: None,
            },
            ElementKind::TextBox {
                text,
                width,
                height,
                format,
                ..
            } => {
                let c = [
                    ((format.color >> 16) & 0xFF) as u8,
                    ((format.color >> 8) & 0xFF) as u8,
                    (format.color & 0xFF) as u8,
                    255,
                ];
                AeLayerKind::Text {
                    text: text.clone(),
                    font: if format.font.trim().is_empty() {
                        "ArialMT".into()
                    } else {
                        format.font.clone()
                    },
                    font_size: format.size.max(1.0),
                    color: color_rgb01(c),
                    bold: format.bold,
                    italic: format.italic,
                    align: format.align,
                    leading: (format.size + format.leading).max(1.0),
                    tracking: format.letter_spacing * 20.0,
                    box_size: Some([width.max(16.0), height.max(16.0)]),
                }
            }
            ElementKind::Bitmap { resource_id, .. } => {
                let ft = resource_to_footage.get(resource_id)?;
                AeLayerKind::Footage {
                    footage_id: ft.id.clone(),
                    is_audio_only: false,
                    is_video: false,
                    start_offset_sec: 0.0,
                    end_offset_sec: None,
                    loop_count: 1,
                    muted: true,
                    audio_levels_db: Vec::new(),
                }
            }
            ElementKind::Video { .. } => {
                let vs = first.video_span.as_ref()?;
                let ft_opt = resource_to_footage.get(&vs.resource).or_else(|| {
                    vs.external_path
                        .as_ref()
                        .and_then(|ext| resource_to_footage.get(&format!("__ext_video__:{ext}")))
                });
                let Some(ft) = ft_opt else {
                    diagnostics.push(AepDiagnostic {
                        code: "AE_EXTERNAL_VIDEO_UNRESOLVED",
                        kind: AepDiagnosticKind::Skipped,
                        location: format!("symbol '{}' layer '{}'", sym.name, layer.name),
                        message: format!(
                            "Skipped video layer because external/embedded video '{}' could not be resolved into Footage.",
                            vs.external_path.as_deref().unwrap_or(vs.resource.as_str())
                        ),
                    });
                    return None;
                };
                AeLayerKind::Footage {
                    footage_id: ft.id.clone(),
                    is_audio_only: false,
                    is_video: true,
                    start_offset_sec: f64::from(vs.start_offset_ms) / 1000.0,
                    end_offset_sec: vs.end_offset_ms.map(|ms| f64::from(ms) / 1000.0),
                    loop_count: if vs.loop_video { 999 } else { 1 },
                    muted: vs.muted,
                    audio_levels_db: Vec::new(),
                }
            }
            ElementKind::Instance {
                symbol_id,
                playback,
            } => {
                let child_frames = project
                    .symbol(symbol_id)
                    .map(|s| s.timeline.frame_count.max(1))
                    .unwrap_or(1);
                let (tr_keys, tr_expr) = build_instance_time_remap(
                    *playback,
                    layer.properties.sync_timeline,
                    in_frame,
                    out_frame,
                    child_frames,
                    fps,
                    options
                        .translate_scripts
                        .then(|| symbol_time_remap_exprs.get(symbol_id).cloned())
                        .flatten(),
                );
                if expressions.time_remap.is_none() {
                    expressions.time_remap = tr_expr;
                }
                AeLayerKind::PreComp {
                    comp_id: symbol_id.clone(),
                    comp_origin: precomp_origin,
                    time_remap_keys: tr_keys,
                }
            }
        }
    };

    let effective_blend = if el.blend_mode == BlendMode::Normal {
        layer.properties.blend_mode
    } else {
        el.blend_mode
    };
    let mut all_filters = layer.properties.filters.clone();
    all_filters.extend(first.kf_filters.iter().cloned());
    all_filters.extend(el.filters.iter().cloned());
    let effects = convert_filters_to_ae(&all_filters, el.transform.color_transform);

    let display_name = if !el.name.trim().is_empty() {
        format!("{} · {}", layer.name, el.name)
    } else {
        layer.name.clone()
    };

    Some(AeLayer {
        id: format!("lyr-{}-{}-{}", layer.id, el.id, in_frame),
        name: display_name,
        source_element_id: Some(el.id.clone()),
        in_frame,
        out_frame,
        visible: layer.visible && el.visible,
        locked: layer.locked,
        solo: false,
        is_track_matte_source: false,
        track_matte: AeTrackMatte::None,
        blend_mode: ae_blend_mode(effective_blend),
        auto_orient_along_path: auto_orient,
        three_d: layer.properties.z_depth.abs() > 1e-6,
        z_depth: layer.properties.z_depth,
        parent_layer_id: None,
        anchor_point,
        position_keys: dedup_constant_keys(pos_keys),
        scale_keys: dedup_constant_keys(scale_keys),
        rotation_keys: dedup_constant_keys(rot_keys),
        opacity_keys: dedup_constant_keys(opacity_keys),
        skew_keys: dedup_constant_zero_keys(skew_keys),
        expressions,
        effects,
        markers,
        kind,
    })
}

// ============================================================================
// Shape, Paint, Filter, Time-Remap & Footage Builders
// ============================================================================

fn convert_filters_to_ae(filters: &[FilterSpec], ct: ColorTransform) -> Vec<AeEffect> {
    let mut out = Vec::new();
    for f in filters {
        match f {
            FilterSpec::DropShadow {
                distance,
                angle,
                color,
                alpha,
                blur_x,
                blur_y,
                ..
            } => {
                out.push(AeEffect::DropShadow {
                    color: u32_rgb01(*color),
                    opacity_pct: (alpha * 100.0).clamp(0.0, 100.0),
                    direction_deg: (*angle + 90.0).rem_euclid(360.0),
                    distance: distance.max(0.0),
                    softness: ((blur_x + blur_y) * 0.5).max(0.0),
                });
            }
            FilterSpec::Glow {
                color,
                alpha,
                blur_x,
                blur_y,
                strength,
                ..
            } => {
                out.push(AeEffect::Glow {
                    color: u32_rgb01(*color),
                    radius: ((blur_x + blur_y) * 0.5).max(0.0),
                    intensity: (strength * alpha).clamp(0.0, 10.0),
                });
            }
            FilterSpec::Blur { blur_x, blur_y, .. } => {
                let dims = if (blur_x - blur_y).abs() <= 0.5 {
                    1 // Horizontal and Vertical
                } else if blur_x > blur_y {
                    2 // Horizontal
                } else {
                    3 // Vertical
                };
                out.push(AeEffect::GaussianBlur {
                    blurriness: blur_x.max(*blur_y).max(0.0),
                    dimensions: dims,
                });
            }
            FilterSpec::Bevel {
                distance,
                angle,
                strength,
                ..
            } => {
                out.push(AeEffect::BevelAlpha {
                    edge_thickness: distance.max(1.0),
                    light_angle: *angle,
                    light_intensity: strength.clamp(0.0, 2.0),
                });
            }
            FilterSpec::GradientGlow {
                colors,
                blur_x,
                blur_y,
                strength,
                ..
            } => {
                let col = colors.first().copied().unwrap_or(0xFFFFFF);
                out.push(AeEffect::Glow {
                    color: u32_rgb01(col),
                    radius: ((blur_x + blur_y) * 0.5).max(0.0),
                    intensity: strength.clamp(0.0, 10.0),
                });
            }
            FilterSpec::GradientBevel {
                distance,
                angle,
                strength,
                ..
            } => {
                out.push(AeEffect::BevelAlpha {
                    edge_thickness: distance.max(1.0),
                    light_angle: *angle,
                    light_intensity: strength.clamp(0.0, 2.0),
                });
            }
            FilterSpec::ColorMatrix { .. } | FilterSpec::Convolution { .. } => {}
        }
    }

    // Non-identity RGB tint -> AE Tint effect
    if !ct.is_identity() {
        let tint_strength = (1.0
            - (ct.multipliers[0] + ct.multipliers[1] + ct.multipliers[2]) / 3.0)
            .clamp(0.0, 1.0);
        if tint_strength > 0.02
            || ct.offsets[0].abs() > 2.0
            || ct.offsets[1].abs() > 2.0
            || ct.offsets[2].abs() > 2.0
        {
            let col = [
                (ct.offsets[0] / 255.0).clamp(0.0, 1.0),
                (ct.offsets[1] / 255.0).clamp(0.0, 1.0),
                (ct.offsets[2] / 255.0).clamp(0.0, 1.0),
            ];
            out.push(AeEffect::Tint {
                color: col,
                amount_pct: (tint_strength * 100.0).clamp(5.0, 100.0),
            });
        }
    }
    out
}

fn build_instance_time_remap(
    playback: InstancePlayback,
    sync_timeline: bool,
    in_frame: u32,
    out_frame: u32,
    child_frames: u32,
    fps: f64,
    script_expr: Option<String>,
) -> (Vec<AeKey<f64>>, Option<String>) {
    let cf = child_frames.max(1);
    if cf <= 1 && script_expr.is_none() {
        return (Vec::new(), None);
    }
    match playback {
        InstancePlayback::SingleFrame(f) => {
            let t = f64::from(f.min(cf - 1)) / fps;
            let ease = AeEase {
                hold: true,
                ..AeEase::default()
            };
            (
                vec![AeKey {
                    frame: in_frame,
                    value: t,
                    ease,
                }],
                None,
            )
        }
        InstancePlayback::PlayOnce | InstancePlayback::PlayOnceFrom(_) => {
            let start_f = match playback {
                InstancePlayback::PlayOnceFrom(s) => s.min(cf - 1),
                _ => 0,
            };
            let max_t = f64::from(cf - 1) / fps;
            let start_t = f64::from(start_f) / fps;
            let expr = format!("Math.min({max_t:.6}, {start_t:.6} + Math.max(0, time - inPoint))");
            (
                vec![AeKey {
                    frame: in_frame,
                    value: start_t,
                    ease: AeEase::default(),
                }],
                Some(expr),
            )
        }
        InstancePlayback::Loop | InstancePlayback::LoopFrom(_) => {
            let start_f = match playback {
                InstancePlayback::LoopFrom(s) => s % cf,
                _ => 0,
            };
            let base_ref = if sync_timeline {
                "time"
            } else {
                "(time - inPoint)"
            };
            let expr = format!(
                "(() => {{ const fps = 1.0 / thisComp.frameDuration; const f = ({start_f} + Math.max(0, timeToFrames({base_ref}, fps))) % {cf}; return f / fps; }})()"
            );
            (
                vec![AeKey {
                    frame: in_frame,
                    value: f64::from(start_f) / fps,
                    ease: AeEase::default(),
                }],
                Some(expr),
            )
        }
        InstancePlayback::Independent => {
            if let Some(se) = script_expr {
                return (
                    vec![AeKey {
                        frame: in_frame,
                        value: 0.0,
                        ease: AeEase::default(),
                    }],
                    Some(se),
                );
            }
            if out_frame - in_frame > cf && cf > 1 {
                // Independent MovieClips loop continuously in Flash unless stopped by script
                let expr = format!(
                    "(() => {{ const fps = 1.0 / thisComp.frameDuration; const f = Math.max(0, timeToFrames(time - inPoint, fps)) % {cf}; return f / fps; }})()"
                );
                (
                    vec![AeKey {
                        frame: in_frame,
                        value: 0.0,
                        ease: AeEase::default(),
                    }],
                    Some(expr),
                )
            } else {
                (Vec::new(), None)
            }
        }
    }
}

fn build_sound_levels_keys(
    start_frame: u32,
    end_frame: u32,
    snd: &neoflash_core::FrameSound,
) -> Vec<AeKey<[f64; 2]>> {
    let amp_to_db = |amp: f64| -> f64 {
        if amp <= 0.001 {
            -96.0
        } else {
            (20.0 * amp.log10()).clamp(-96.0, 12.0)
        }
    };
    let end_f = end_frame.max(start_frame + 1);
    if let Some(env) = &snd.envelope
        && !env.is_empty()
    {
        let span = f64::from(end_f - start_frame);
        return env
            .iter()
            .map(|&[pos, l, r]| AeKey {
                frame: start_frame + (pos.clamp(0.0, 1.0) * span).round() as u32,
                value: [amp_to_db(l), amp_to_db(r)],
                ease: AeEase::default(),
            })
            .collect();
    }
    match snd.effect {
        SoundEffect::None | SoundEffect::Custom => Vec::new(),
        SoundEffect::LeftChannel => vec![AeKey {
            frame: start_frame,
            value: [0.0, -96.0],
            ease: AeEase::default(),
        }],
        SoundEffect::RightChannel => vec![AeKey {
            frame: start_frame,
            value: [-96.0, 0.0],
            ease: AeEase::default(),
        }],
        SoundEffect::FadeIn => vec![
            AeKey {
                frame: start_frame,
                value: [-48.0, -48.0],
                ease: AeEase::default(),
            },
            AeKey {
                frame: end_f,
                value: [0.0, 0.0],
                ease: AeEase::default(),
            },
        ],
        SoundEffect::FadeOut => vec![
            AeKey {
                frame: start_frame,
                value: [0.0, 0.0],
                ease: AeEase::default(),
            },
            AeKey {
                frame: end_f,
                value: [-48.0, -48.0],
                ease: AeEase::default(),
            },
        ],
        SoundEffect::FadeLeftToRight => vec![
            AeKey {
                frame: start_frame,
                value: [0.0, -48.0],
                ease: AeEase::default(),
            },
            AeKey {
                frame: end_f,
                value: [-48.0, 0.0],
                ease: AeEase::default(),
            },
        ],
        SoundEffect::FadeRightToLeft => vec![
            AeKey {
                frame: start_frame,
                value: [-48.0, 0.0],
                ease: AeEase::default(),
            },
            AeKey {
                frame: end_f,
                value: [0.0, -48.0],
                ease: AeEase::default(),
            },
        ],
    }
}

#[allow(clippy::too_many_arguments)]
fn push_transform_keys(
    frame: u32,
    t: Transform,
    origin: [f64; 2],
    pivot: [f64; 2],
    extra_rot_deg: f64,
    ease: AeEase,
    pos_keys: &mut Vec<AeKey<[f64; 2]>>,
    scale_keys: &mut Vec<AeKey<[f64; 2]>>,
    rot_keys: &mut Vec<AeKey<f64>>,
    opacity_keys: &mut Vec<AeKey<f64>>,
    skew_keys: &mut Vec<AeKey<f64>>,
) {
    // Account for custom pivot when Anchor Point is shifted by `pivot`
    let rot = finite_or(t.rotation, 0.0);
    let skew = finite_or(t.skew, 0.0);
    let sx = finite_or(t.scale_x, 1.0);
    let sy_scale = finite_or(t.scale_y, 1.0);
    let (s, c) = rot.to_radians().sin_cos();
    let (sy, cy) = (rot + skew).to_radians().sin_cos();
    let px = finite_or(
        finite_or(t.x, 0.0) + origin[0] + c * sx * pivot[0] - sy * sy_scale * pivot[1],
        0.0,
    );
    let py = finite_or(
        finite_or(t.y, 0.0) + origin[1] + s * sx * pivot[0] + cy * sy_scale * pivot[1],
        0.0,
    );

    pos_keys.push(AeKey {
        frame,
        value: [px, py],
        ease,
    });
    scale_keys.push(AeKey {
        frame,
        value: [
            finite_or(sx * 100.0, 100.0),
            finite_or(sy_scale * 100.0, 100.0),
        ],
        ease,
    });
    rot_keys.push(AeKey {
        frame,
        value: finite_or(rot + extra_rot_deg, 0.0),
        ease,
    });
    let alpha_mul = finite_or(t.color_transform.multipliers[3], 1.0);
    let alpha_off = finite_or(t.color_transform.offsets[3], 0.0) / 255.0;
    let op = finite_or(
        ((finite_or(t.alpha, 1.0) * alpha_mul + alpha_off) * 100.0).clamp(0.0, 100.0),
        100.0,
    );
    opacity_keys.push(AeKey {
        frame,
        value: op,
        ease,
    });
    skew_keys.push(AeKey {
        frame,
        value: skew,
        ease,
    });
}

fn dedup_constant_keys<T: PartialEq>(keys: Vec<AeKey<T>>) -> Vec<AeKey<T>> {
    if keys.len() <= 1 {
        return keys;
    }
    let first_val = &keys[0].value;
    if keys.iter().all(|k| &k.value == first_val) {
        vec![keys.into_iter().next().unwrap()]
    } else {
        keys
    }
}

fn dedup_constant_zero_keys(keys: Vec<AeKey<f64>>) -> Vec<AeKey<f64>> {
    if keys.iter().all(|k| k.value.abs() <= 1e-6) {
        Vec::new()
    } else {
        dedup_constant_keys(keys)
    }
}

fn easing_to_ae(ease: Option<Easing>) -> AeEase {
    let Some(ease) = ease else {
        return AeEase {
            in_speed: 0.0,
            in_influence: 16.6667,
            out_speed: 0.0,
            out_influence: 16.6667,
            hold: true,
        };
    };
    match ease {
        Easing::Linear => AeEase::default(),
        Easing::EaseIn => AeEase {
            in_speed: 0.0,
            in_influence: 33.33,
            out_speed: 0.0,
            out_influence: 75.0,
            hold: false,
        },
        Easing::EaseOut => AeEase {
            in_speed: 0.0,
            in_influence: 75.0,
            out_speed: 0.0,
            out_influence: 33.33,
            hold: false,
        },
        Easing::EaseInOut => AeEase {
            in_speed: 0.0,
            in_influence: 66.67,
            out_speed: 0.0,
            out_influence: 66.67,
            hold: false,
        },
        Easing::Flash { acceleration } => {
            let amt = (acceleration / 100.0).clamp(-1.0, 1.0);
            if amt < 0.0 {
                // Ease In
                AeEase {
                    in_speed: 0.0,
                    in_influence: 20.0,
                    out_speed: 0.0,
                    out_influence: (16.67 + (-amt) * 75.0).clamp(16.67, 95.0),
                    hold: false,
                }
            } else {
                // Ease Out
                AeEase {
                    in_speed: 0.0,
                    in_influence: (16.67 + amt * 75.0).clamp(16.67, 95.0),
                    out_speed: 0.0,
                    out_influence: 20.0,
                    hold: false,
                }
            }
        }
        Easing::CubicBezier { x1, x2, .. } => AeEase {
            out_influence: (x1.clamp(0.05, 0.99) * 100.0),
            in_influence: ((1.0 - x2.clamp(0.01, 0.95)) * 100.0),
            in_speed: 0.0,
            out_speed: 0.0,
            hold: false,
        },
        Easing::Bounce | Easing::Elastic => AeEase::default(),
    }
}

fn ae_blend_mode(mode: BlendMode) -> &'static str {
    match mode {
        BlendMode::Normal | BlendMode::Layer => "NORMAL",
        BlendMode::Multiply => "MULTIPLY",
        BlendMode::Screen => "SCREEN",
        BlendMode::Lighten => "LIGHTEN",
        BlendMode::Darken => "DARKEN",
        BlendMode::Difference | BlendMode::Invert => "DIFFERENCE",
        BlendMode::Add => "ADD",
        BlendMode::Subtract => "SUBTRACT",
        BlendMode::Alpha => "STENCIL_ALPHA",
        BlendMode::Erase => "SILHOUETTE_ALPHA",
        BlendMode::Overlay => "OVERLAY",
        BlendMode::Hardlight => "HARD_LIGHT",
    }
}

pub(super) fn color_rgb01(c: Color) -> [f64; 3] {
    [
        f64::from(c[0]) / 255.0,
        f64::from(c[1]) / 255.0,
        f64::from(c[2]) / 255.0,
    ]
}

pub(super) fn u32_rgb01(rgb: u32) -> [f64; 3] {
    [
        f64::from((rgb >> 16) & 0xFF) / 255.0,
        f64::from((rgb >> 8) & 0xFF) / 255.0,
        f64::from(rgb & 0xFF) / 255.0,
    ]
}

fn collect_timeline_labels(timeline: &Timeline) -> BTreeMap<String, u32> {
    let mut map = BTreeMap::new();
    for layer in &timeline.layers {
        for kf in &layer.keyframes {
            let lbl = kf.label.trim();
            if !lbl.is_empty() && !map.contains_key(lbl) {
                map.insert(lbl.to_owned(), kf.frame);
            }
        }
    }
    map
}
