use super::*;

pub(super) fn shed_video_resources(
    project: &mut Project,
    report: &mut ImportReport,
    excess: usize,
) -> bool {
    let mut candidates: Vec<(usize, u16, String)> = project
        .resources
        .iter()
        .filter_map(|r| {
            let suffix = r.id.strip_prefix("legacy-video-")?;
            let media_id = suffix.parse::<u16>().ok()?;
            Some((r.data_base64.len(), media_id, r.id.clone()))
        })
        .collect();
    if candidates.is_empty() {
        return false;
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    let mut shed = Vec::new();
    let mut saved = 0usize;
    for (b64_len, media_id, res_id) in candidates {
        shed.push((res_id, media_id));
        saved = saved.saturating_add(b64_len.saturating_add(80));
        if saved >= excess {
            break;
        }
    }
    let shed_ids: std::collections::HashSet<&str> =
        shed.iter().map(|(id, _)| id.as_str()).collect();
    project
        .resources
        .retain(|r| !shed_ids.contains(r.id.as_str()));
    for (res_id, media_id) in &shed {
        report.warning(
            "fla_media_not_imported",
            format!("media {media_id}"),
            "Decoded video stream exceeds the project's 128 MiB encoded JSON budget alongside converted timelines; this video remains in the original FLA.",
        );
        for symbol in &mut project.symbols {
            for layer in &mut symbol.timeline.layers {
                for key in &mut layer.keyframes {
                    if key
                        .video
                        .as_ref()
                        .is_some_and(|video| &video.resource == res_id)
                    {
                        key.video = None;
                    }
                }
            }
        }
    }
    true
}

pub(super) fn shed_audio_resources(
    project: &mut Project,
    report: &mut ImportReport,
    excess: usize,
) -> bool {
    let mut candidates: Vec<(usize, u16, String)> = project
        .resources
        .iter()
        .filter_map(|r| {
            let suffix = r.id.strip_prefix("legacy-sound-").or_else(|| {
                r.path
                    .strip_prefix("legacy/sounds/")
                    .and_then(|s| s.split('/').next())
            })?;
            let media_id = suffix.parse::<u16>().ok()?;
            Some((r.data_base64.len(), media_id, r.id.clone()))
        })
        .collect();
    if candidates.is_empty() {
        return false;
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    let mut shed = Vec::new();
    let mut saved = 0usize;
    for (b64_len, media_id, res_id) in candidates {
        shed.push((res_id, media_id));
        saved = saved.saturating_add(b64_len.saturating_add(80));
        if saved >= excess {
            break;
        }
    }
    let shed_ids: std::collections::HashSet<&str> =
        shed.iter().map(|(id, _)| id.as_str()).collect();
    project
        .resources
        .retain(|r| !shed_ids.contains(r.id.as_str()));
    for (res_id, media_id) in &shed {
        report.warning(
            "fla_media_not_imported",
            format!("media {media_id}"),
            "Decoded sound exceeds the project's 128 MiB encoded JSON budget alongside converted timelines; this sound remains in the original FLA.",
        );
        for symbol in &mut project.symbols {
            for layer in &mut symbol.timeline.layers {
                for key in &mut layer.keyframes {
                    if key
                        .sound
                        .as_ref()
                        .is_some_and(|sound| &sound.resource == res_id)
                    {
                        key.sound = None;
                        report.warning(
                            "fla_frame_sound",
                            format!("{} layer {} frame {}", symbol.name, layer.name, key.frame + 1),
                            format!(
                                "Timeline sound references media #{media_id}, which was omitted to fit the 128 MiB project JSON budget."
                            ),
                        );
                    }
                }
            }
        }
    }
    true
}

pub(super) fn downscale_one_project_bitmap_resource(
    project: &mut Project,
    res_id: &str,
    min_side: u32,
    require_b64_shrink: bool,
    report: &mut ImportReport,
    reason_budget: &str,
) -> Option<usize> {
    use image::{
        ExtendedColorType, ImageEncoder,
        codecs::jpeg::JpegEncoder,
        codecs::png::{CompressionType, FilterType, PngEncoder},
    };
    let res_idx = project.resources.iter().position(|r| r.id == res_id)?;
    let old_b64_len = project.resources[res_idx].data_base64.len();
    let old_media_type = project.resources[res_idx].media_type.clone();
    let old_path = project.resources[res_idx].path.clone();
    let decoded = project.resources[res_idx].decode_image().ok()?;
    let cur_max = decoded.width.max(decoded.height);
    if cur_max <= min_side {
        return None;
    }
    let target_max = (cur_max / 2).max(min_side);
    let (new_w, new_h, resized) = crate::legacy::media::resize_rgba_proportional(
        &decoded.rgba,
        decoded.width,
        decoded.height,
        target_max,
    )?;
    let sx = f64::from(decoded.width) / f64::from(new_w);
    let sy = f64::from(decoded.height) / f64::from(new_h);

    // Preflight all ElementKind::Bitmap and AuthoredBitmapFill bounds before mutating.
    for symbol in &project.symbols {
        for layer in &symbol.timeline.layers {
            for key in &layer.keyframes {
                for bf in &key.bitmap_fills {
                    if bf.resource_id == res_id {
                        let m = [
                            bf.matrix[0] * sx,
                            bf.matrix[1] * sx,
                            bf.matrix[2] * sy,
                            bf.matrix[3] * sy,
                        ];
                        if !m.iter().all(|v| v.is_finite() && v.abs() <= 1e9) {
                            return None;
                        }
                    }
                }
                for elem in &key.elements {
                    if matches!(
                        &elem.kind,
                        ElementKind::Bitmap { resource_id, .. } if resource_id == res_id
                    ) {
                        if let Some([px, py]) = elem.transform.pivot {
                            let npx = px / sx;
                            let npy = py / sy;
                            if !npx.is_finite()
                                || !npy.is_finite()
                                || npx.abs() > 1e9
                                || npy.abs() > 1e9
                            {
                                return None;
                            }
                        }
                        let geom = Transform {
                            x: elem.transform.x,
                            y: elem.transform.y,
                            scale_x: elem.transform.scale_x,
                            scale_y: elem.transform.scale_y,
                            rotation: elem.transform.rotation,
                            skew: elem.transform.skew,
                            ..Transform::default()
                        };
                        let mut mat = neoflash_core::transform_matrix(geom);
                        mat[0] *= sx;
                        mat[1] *= sx;
                        mat[2] *= sy;
                        mat[3] *= sy;
                        Transform::from_matrix(mat)?;
                    }
                }
            }
        }
    }

    let is_opaque = resized.as_chunks::<4>().0.iter().all(|px| px[3] == 255);
    let use_jpeg = old_media_type == "image/jpeg" || (require_b64_shrink && is_opaque);
    let (encoded_bytes, new_mime, new_path) = if use_jpeg {
        let mut rgb = Vec::with_capacity((new_w as usize) * (new_h as usize) * 3);
        for px in resized.as_chunks::<4>().0 {
            rgb.extend_from_slice(&px[..3]);
        }
        let mut jpg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpg, 85)
            .encode(&rgb, new_w, new_h, ExtendedColorType::Rgb8)
            .ok()?;
        let path = if old_media_type == "image/jpeg" {
            old_path
        } else {
            let used_lower: std::collections::BTreeSet<String> = project
                .resources
                .iter()
                .enumerate()
                .filter(|&(i, _)| i != res_idx)
                .map(|(_, r)| r.path.to_ascii_lowercase())
                .collect();
            let stem = old_path
                .strip_suffix(".png")
                .or_else(|| old_path.strip_suffix(".PNG"))
                .unwrap_or(&old_path);
            let mut seq = 1usize;
            loop {
                let cand = if seq == 1 {
                    format!("{stem}.jpg")
                } else {
                    format!("{stem}-{seq}.jpg")
                };
                if !used_lower.contains(&cand.to_ascii_lowercase()) {
                    break cand;
                }
                seq += 1;
            }
        };
        (jpg, "image/jpeg", path)
    } else {
        let mut png = Vec::new();
        PngEncoder::new_with_quality(&mut png, CompressionType::Best, FilterType::Adaptive)
            .write_image(&resized, new_w, new_h, ExtendedColorType::Rgba8)
            .ok()?;
        (png, "image/png", old_path)
    };

    let updated = ResourceUnit::from_bytes(res_id, new_path, new_mime, &encoded_bytes).ok()?;
    let new_b64_len = updated.data_base64.len();
    if require_b64_shrink && new_b64_len >= old_b64_len {
        return None;
    }

    project.resources[res_idx] = updated;
    for symbol in &mut project.symbols {
        for layer in &mut symbol.timeline.layers {
            for key in &mut layer.keyframes {
                for bf in &mut key.bitmap_fills {
                    if bf.resource_id == res_id {
                        bf.matrix[0] *= sx;
                        bf.matrix[1] *= sx;
                        bf.matrix[2] *= sy;
                        bf.matrix[3] *= sy;
                    }
                }
                for elem in &mut key.elements {
                    if let ElementKind::Bitmap {
                        resource_id,
                        width,
                        height,
                        ..
                    } = &mut elem.kind
                        && resource_id == res_id
                    {
                        *width = new_w;
                        *height = new_h;
                        if let Some(pivot) = &mut elem.transform.pivot {
                            pivot[0] /= sx;
                            pivot[1] /= sy;
                        }
                        let geom = Transform {
                            x: elem.transform.x,
                            y: elem.transform.y,
                            scale_x: elem.transform.scale_x,
                            scale_y: elem.transform.scale_y,
                            rotation: elem.transform.rotation,
                            skew: elem.transform.skew,
                            ..Transform::default()
                        };
                        let mut mat = neoflash_core::transform_matrix(geom);
                        mat[0] *= sx;
                        mat[1] *= sx;
                        mat[2] *= sy;
                        mat[3] *= sy;
                        if let Some(new_tf) = Transform::from_matrix(mat) {
                            elem.transform.x = new_tf.x;
                            elem.transform.y = new_tf.y;
                            elem.transform.scale_x = new_tf.scale_x;
                            elem.transform.scale_y = new_tf.scale_y;
                            elem.transform.rotation = new_tf.rotation;
                            elem.transform.skew = new_tf.skew;
                        }
                    }
                }
            }
        }
    }

    let loc = res_id
        .strip_prefix("legacy-bitmap-")
        .map(|mid| format!("media {mid}"))
        .unwrap_or_else(|| res_id.to_owned());
    report.warning(
        "fla_bitmap_downscaled",
        loc,
        format!(
            "Bitmap was downscaled from {}x{} px to {new_w}x{new_h} px (preserving Stage dimensions via transform scale) to fit the {reason_budget}; full-resolution bytes remain in the original FLA.",
            decoded.width, decoded.height
        ),
    );
    Some(old_b64_len.saturating_sub(new_b64_len))
}

pub(super) fn downscale_bitmap_resources_for_json(
    project: &mut Project,
    report: &mut ImportReport,
    excess: usize,
) -> bool {
    let mut saved = 0usize;
    for _ in 0..512 {
        let mut candidates: Vec<(usize, u16, String)> = project
            .resources
            .iter()
            .filter_map(|r| {
                let suffix = r.id.strip_prefix("legacy-bitmap-")?;
                let media_id = suffix.parse::<u16>().ok()?;
                if r.data_base64.len() <= 16 * 1024 {
                    return None;
                }
                let raw = r.decoded_bytes().ok()?;
                let (w, h) = image::ImageReader::new(std::io::Cursor::new(&raw))
                    .with_guessed_format()
                    .ok()?
                    .into_dimensions()
                    .ok()?;
                if w.max(h) <= 64 {
                    return None;
                }
                Some((r.data_base64.len(), media_id, r.id.clone()))
            })
            .collect();
        if candidates.is_empty() {
            break;
        }
        candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
        let mut progressed = false;
        for (_, _, res_id) in candidates {
            if let Some(delta) = downscale_one_project_bitmap_resource(
                project,
                &res_id,
                64,
                true,
                report,
                "128 MiB project JSON budget",
            ) && delta > 0
            {
                saved = saved.saturating_add(delta);
                progressed = true;
                break;
            }
        }
        if !progressed || saved >= excess {
            break;
        }
    }
    saved > 0
}

pub(super) fn shed_bitmap_resources(
    project: &mut Project,
    report: &mut ImportReport,
    excess: usize,
) -> bool {
    let mut placement_counts = BTreeMap::<String, usize>::new();
    for key in project
        .symbols
        .iter()
        .flat_map(|s| &s.timeline.layers)
        .flat_map(|l| &l.keyframes)
    {
        for element in &key.elements {
            if let ElementKind::Bitmap { resource_id, .. } = &element.kind {
                *placement_counts.entry(resource_id.clone()).or_default() += 1;
            }
        }
        for bf in &key.bitmap_fills {
            *placement_counts.entry(bf.resource_id.clone()).or_default() += 1;
        }
    }
    let mut candidates: Vec<(usize, u16, String)> = project
        .resources
        .iter()
        .filter_map(|r| {
            let suffix = r.id.strip_prefix("legacy-bitmap-")?;
            let media_id = suffix.parse::<u16>().ok()?;
            Some((r.data_base64.len(), media_id, r.id.clone()))
        })
        .collect();
    if candidates.is_empty() {
        return false;
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    let mut shed = Vec::new();
    let mut saved = 0usize;
    for (b64_len, media_id, res_id) in candidates {
        let uses = placement_counts.get(&res_id).copied().unwrap_or(0);
        let net = b64_len.saturating_sub(600usize.saturating_add(uses.saturating_mul(40)));
        shed.push((res_id, media_id));
        saved = saved.saturating_add(net.max(1));
        if saved >= excess {
            break;
        }
    }
    let shed_ids: std::collections::HashSet<&str> =
        shed.iter().map(|(id, _)| id.as_str()).collect();
    project
        .resources
        .retain(|r| !shed_ids.contains(r.id.as_str()));
    let allowed_leaf = symbols_allowing_leaf_instance(&project.symbols);
    for (res_id, media_id) in &shed {
        report.warning(
            "fla_bitmap_skipped",
            format!("media {media_id}"),
            "Decoded bitmap exceeds the project's 128 MiB encoded JSON budget alongside converted timelines; this bitmap becomes an unresolved placeholder.",
        );
        let missing_id = format!("fla-missing-bitmap-{media_id}");
        let placeholder_symbol = if project.symbols.iter().any(|s| s.id == missing_id) {
            Some(missing_id)
        } else if project.symbols.len() < 10_000 {
            project.symbols.push(group_symbol(
                &missing_id,
                format!("Unresolved bitmap #{media_id}"),
                vec![],
            ));
            Some(missing_id)
        } else if project.symbols.iter().any(|s| s.id == PLACEHOLDER) {
            Some(PLACEHOLDER.to_owned())
        } else {
            None
        };
        for symbol in &mut project.symbols {
            let can_nest = allowed_leaf.contains(&symbol.id);
            for layer in &mut symbol.timeline.layers {
                for key in &mut layer.keyframes {
                    key.bitmap_fills.retain(|bf| &bf.resource_id != res_id);
                    for element in &mut key.elements {
                        if matches!(
                            &element.kind,
                            ElementKind::Bitmap { resource_id, .. } if resource_id == res_id
                        ) {
                            element.name = format!("Unresolved source bitmap #{media_id}");
                            element.kind = match (&placeholder_symbol, can_nest) {
                                (Some(sid), true) => ElementKind::Instance {
                                    symbol_id: sid.clone(),
                                    playback: InstancePlayback::SingleFrame(0),
                                },
                                _ => ElementKind::Path {
                                    commands: vec![],
                                    fill_rule: FillRule::EvenOdd,
                                    fill: [0; 4],
                                    stroke: [0; 4],
                                    stroke_width: 0.0,
                                },
                            };
                        }
                    }
                }
            }
        }
    }
    true
}

pub(super) fn is_vector_path(element: &Element) -> bool {
    matches!(
        element.kind,
        ElementKind::PaintedPath { .. } | ElementKind::Path { .. }
    )
}

pub(super) fn encoded_element_bytes(element: &Element) -> usize {
    serde_json::to_string(element).map_or(512, |text| text.len() + 1)
}

pub(super) fn symbols_allowing_leaf_instance(
    symbols: &[Symbol],
) -> std::collections::HashSet<String> {
    let mut parents: std::collections::HashMap<&str, Vec<&str>> = std::collections::HashMap::new();
    for symbol in symbols {
        for element in symbol
            .timeline
            .layers
            .iter()
            .flat_map(|l| &l.keyframes)
            .flat_map(|k| &k.elements)
        {
            if let ElementKind::Instance { symbol_id, .. } = &element.kind {
                parents
                    .entry(symbol_id.as_str())
                    .or_default()
                    .push(symbol.id.as_str());
            }
        }
    }
    fn ancestor_depth<'a>(
        id: &'a str,
        parents: &std::collections::HashMap<&'a str, Vec<&'a str>>,
        active: &mut std::collections::HashSet<&'a str>,
        memo: &mut std::collections::HashMap<&'a str, usize>,
    ) -> usize {
        if let Some(&d) = memo.get(id) {
            return d;
        }
        if !active.insert(id) || active.len() > 64 {
            active.remove(id);
            return 65;
        }
        let mut best = 1usize;
        if let Some(ps) = parents.get(id) {
            for &p in ps {
                best = best.max(1 + ancestor_depth(p, parents, active, memo));
                if best >= 65 {
                    break;
                }
            }
        }
        active.remove(id);
        memo.insert(id, best);
        best
    }
    let mut active = std::collections::HashSet::new();
    let mut memo = std::collections::HashMap::new();
    symbols
        .iter()
        .filter(|s| ancestor_depth(&s.id, &parents, &mut active, &mut memo) < 64)
        .map(|s| s.id.clone())
        .collect()
}

pub(super) fn degrade_excess_vector_shapes(
    project: &mut Project,
    report: &mut ImportReport,
    target_savings: usize,
) -> bool {
    if !project.symbols.iter().any(|s| s.id == PLACEHOLDER) && project.symbols.len() < 10_000 {
        project.symbols.push(group_symbol(
            PLACEHOLDER,
            "Unsupported source object (see import report)".into(),
            vec![],
        ));
    }
    let has_placeholder = project.symbols.iter().any(|s| s.id == PLACEHOLDER);
    let allowed_leaf = symbols_allowing_leaf_instance(&project.symbols);
    let mut saved = 0usize;

    for preserve_special_layers in [true, false] {
        for symbol in project.symbols.iter_mut().rev() {
            if symbol.id == PLACEHOLDER || symbol.id.starts_with("fla-missing-bitmap-") {
                continue;
            }
            let is_group_symbol = symbol.id.ends_with("-group");
            let use_placeholder = has_placeholder && allowed_leaf.contains(&symbol.id);
            for layer in symbol.timeline.layers.iter_mut().rev() {
                if preserve_special_layers
                    && matches!(layer.properties.role, LayerRole::Guide | LayerRole::Mask)
                {
                    continue;
                }
                for key in layer.keyframes.iter_mut().rev() {
                    if !key.elements.iter().any(is_vector_path) {
                        continue;
                    }
                    let path_bytes: usize = key
                        .elements
                        .iter()
                        .filter(|e| is_vector_path(e))
                        .map(encoded_element_bytes)
                        .sum();
                    if is_group_symbol {
                        key.elements.retain(|e| !is_vector_path(e));
                        let kept_ids: std::collections::HashSet<&str> = key
                            .elements
                            .iter()
                            .filter(|e| is_vector_path(e))
                            .map(|e| e.id.as_str())
                            .collect();
                        key.bitmap_fills
                            .retain(|bf| kept_ids.contains(bf.element_id.as_str()));
                        key.appearances
                            .retain(|app| kept_ids.contains(app.element_id.as_str()));
                        key.vector_effects
                            .retain(|fx| kept_ids.contains(fx.element_id.as_str()));
                        report.warning(
                            "fla_shape_approximated",
                            &symbol.name,
                            "Vector path geometry was omitted as a placeholder to fit the project's 128 MiB encoded JSON limit; original artwork remains in the source FLA.",
                        );
                        saved = saved.saturating_add(path_bytes);
                    } else {
                        let first_id = key
                            .elements
                            .iter()
                            .find(|e| is_vector_path(e))
                            .unwrap()
                            .id
                            .clone();
                        let replacement = Element {
                            id: first_id,
                            name: String::new(),
                            transform: Transform::default(),
                            kind: if use_placeholder {
                                ElementKind::Instance {
                                    symbol_id: PLACEHOLDER.into(),
                                    playback: InstancePlayback::SingleFrame(0),
                                }
                            } else {
                                ElementKind::Path {
                                    commands: vec![],
                                    fill_rule: FillRule::EvenOdd,
                                    fill: [0; 4],
                                    stroke: [0; 4],
                                    stroke_width: 0.0,
                                }
                            },
                            scripts: vec![],
                            accessibility: None,
                            ..Element::default()
                        };
                        let repl_bytes = encoded_element_bytes(&replacement);
                        if path_bytes <= repl_bytes {
                            continue;
                        }
                        let mut replaced = false;
                        key.elements.retain_mut(|element| {
                            if !is_vector_path(element) {
                                return true;
                            }
                            if !replaced {
                                *element = replacement.clone();
                                replaced = true;
                                true
                            } else {
                                false
                            }
                        });
                        let kept_ids: std::collections::HashSet<&str> = key
                            .elements
                            .iter()
                            .filter(|e| is_vector_path(e))
                            .map(|e| e.id.as_str())
                            .collect();
                        key.bitmap_fills
                            .retain(|bf| kept_ids.contains(bf.element_id.as_str()));
                        key.appearances
                            .retain(|app| kept_ids.contains(app.element_id.as_str()));
                        key.vector_effects
                            .retain(|fx| kept_ids.contains(fx.element_id.as_str()));
                        report.warning(
                            "fla_shape_approximated",
                            format!("{} layer {} frame {}", symbol.name, layer.name, key.frame + 1),
                            "Vector path geometry was replaced by a placeholder to fit the project's 128 MiB encoded JSON limit; original artwork remains in the source FLA.",
                        );
                        saved = saved.saturating_add(path_bytes - repl_bytes);
                    }
                    if saved >= target_savings {
                        return true;
                    }
                }
            }
        }
    }
    saved > 0
}
