use super::*;

// ============================================================================
// 1. RIFX / Egg! Binary `.aep` Container Writer
// ============================================================================

fn sanitize_riff_marker_field(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '|' => '/',
            '\r' | '\n' => ' ',
            '\0' => ' ',
            _ => c,
        })
        .collect()
}

fn push_riff_chunk(buf: &mut Vec<u8>, fourcc: &[u8; 4], payload: &[u8]) -> Result<(), ExportError> {
    let len_u32 = u32::try_from(payload.len()).map_err(|_| {
        ExportError::Options("After Effects RIFX chunk exceeds the 32-bit container limit.".into())
    })?;
    buf.extend_from_slice(fourcc);
    buf.extend_from_slice(&len_u32.to_be_bytes());
    buf.extend_from_slice(payload);
    if payload.len() % 2 == 1 {
        buf.push(0);
    }
    Ok(())
}

fn push_riff_list(
    buf: &mut Vec<u8>,
    form_type: &[u8; 4],
    children: &[u8],
) -> Result<(), ExportError> {
    let total = children
        .len()
        .checked_add(4)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| {
            ExportError::Options(
                "After Effects RIFX LIST exceeds the 32-bit container limit.".into(),
            )
        })?;
    buf.extend_from_slice(b"LIST");
    buf.extend_from_slice(&total.to_be_bytes());
    buf.extend_from_slice(form_type);
    buf.extend_from_slice(children);
    if children.len() % 2 == 1 {
        buf.push(0);
    }
    Ok(())
}

pub(super) fn encode_rifx_aep_container(
    model: &AeProjectModel,
    jsx_source: &str,
) -> Result<Vec<u8>, ExportError> {
    let mut egg_body = Vec::with_capacity(8192);

    // `head` chunk: AE version header (CC 2024 / 24.0 compatible interchange descriptor)
    let mut head = Vec::with_capacity(20);
    head.extend_from_slice(&0x0058_0000_u32.to_be_bytes());
    head.extend_from_slice(&24_u32.to_be_bytes());
    head.extend_from_slice(&(model.compositions.len() as u32).to_be_bytes());
    head.extend_from_slice(&(model.footage.len() as u32).to_be_bytes());
    head.extend_from_slice(&8_u32.to_be_bytes()); // 8 bpc
    push_riff_chunk(&mut egg_body, b"head", &head)?;

    let mut fold_body = Vec::new();
    let next_id = &mut 1_u32;

    // Footage `Item` chunks (`Pin ` -> `Als2` relative path)
    for ft in &model.footage {
        let mut item_body = Vec::new();
        let mut idta = Vec::with_capacity(16);
        idta.extend_from_slice(&2_u16.to_be_bytes()); // type = Footage
        idta.extend_from_slice(&(*next_id).to_be_bytes());
        *next_id += 1;
        idta.extend_from_slice(&ft.width.unwrap_or(0).to_be_bytes());
        idta.extend_from_slice(&ft.height.unwrap_or(0).to_be_bytes());
        push_riff_chunk(&mut item_body, b"idta", &idta)?;
        push_riff_chunk(&mut item_body, b"Utf8", ft.name.as_bytes())?;

        let mut pin_body = Vec::new();
        push_riff_chunk(&mut pin_body, b"Als2", ft.relative_path.as_bytes())?;
        push_riff_list(&mut item_body, b"Pin ", &pin_body)?;
        push_riff_list(&mut fold_body, b"Item", &item_body)?;
    }

    // Composition `Item` chunks (`Cdta` header + top-to-bottom `Layr` lists with `shap` & `expr`)
    for comp in &model.compositions {
        let mut item_body = Vec::new();
        let mut idta = Vec::with_capacity(16);
        idta.extend_from_slice(&1_u16.to_be_bytes()); // type = Composition
        idta.extend_from_slice(&(*next_id).to_be_bytes());
        *next_id += 1;
        idta.extend_from_slice(&comp.width.to_be_bytes());
        idta.extend_from_slice(&comp.height.to_be_bytes());
        push_riff_chunk(&mut item_body, b"idta", &idta)?;
        push_riff_chunk(&mut item_body, b"Utf8", comp.name.as_bytes())?;

        // `Cdta` composition metrics
        let mut cdta = Vec::with_capacity(48);
        cdta.extend_from_slice(&comp.width.to_be_bytes());
        cdta.extend_from_slice(&comp.height.to_be_bytes());
        cdta.extend_from_slice(&comp.fps.to_be_bytes());
        cdta.extend_from_slice(&comp.duration_frames.to_be_bytes());
        cdta.push((comp.background[0].clamp(0.0, 1.0) * 255.0).round() as u8);
        cdta.push((comp.background[1].clamp(0.0, 1.0) * 255.0).round() as u8);
        cdta.push((comp.background[2].clamp(0.0, 1.0) * 255.0).round() as u8);
        cdta.push(255);
        push_riff_chunk(&mut item_body, b"Cdta", &cdta)?;

        // Comp markers
        for m in &comp.markers {
            let payload = format!(
                "{}|{}|{}|{}",
                m.frame,
                m.duration_frames,
                sanitize_riff_marker_field(&m.label),
                sanitize_riff_marker_field(&m.comment)
            );
            push_riff_chunk(&mut item_body, b"mrkr", payload.as_bytes())?;
        }

        // Layers in AE top-to-bottom order (reverse of NeoFlash bottom-to-top)
        for layer in comp.layers.iter().rev() {
            let mut layr_body = Vec::new();
            let mut ldta = Vec::with_capacity(32);
            ldta.extend_from_slice(&layer.in_frame.to_be_bytes());
            ldta.extend_from_slice(&layer.out_frame.to_be_bytes());
            let mut flags = 0_u16;
            if layer.visible {
                flags |= 0x0001;
            }
            if layer.locked {
                flags |= 0x0002;
            }
            if layer.three_d {
                flags |= 0x0004;
            }
            if layer.auto_orient_along_path {
                flags |= 0x0008;
            }
            if layer.is_track_matte_source {
                flags |= 0x0010;
            }
            ldta.extend_from_slice(&flags.to_be_bytes());
            ldta.extend_from_slice(&(finite_or(layer.anchor_point[0], 0.0) as f32).to_be_bytes());
            ldta.extend_from_slice(&(finite_or(layer.anchor_point[1], 0.0) as f32).to_be_bytes());
            ldta.extend_from_slice(&(finite_or(layer.z_depth, 0.0) as f32).to_be_bytes());
            push_riff_chunk(&mut layr_body, b"ldta", &ldta)?;
            push_riff_chunk(&mut layr_body, b"Utf8", layer.name.as_bytes())?;

            // Transform keyframes (`tdgp`)
            let tdgp_json = serde_json::to_vec(&serde_json::json!({
                "position": layer.position_keys,
                "scale": layer.scale_keys,
                "rotation": layer.rotation_keys,
                "opacity": layer.opacity_keys,
                "skew": layer.skew_keys,
                "blend": layer.blend_mode,
                "matte": layer.track_matte,
            }))?;
            push_riff_chunk(&mut layr_body, b"tdgp", &tdgp_json)?;

            // Vector Bezier contours (`shap`: count + closed flag + IEEE-754 float32 vertices/in/out)
            if let AeLayerKind::Shape { groups } = &layer.kind {
                for grp in groups {
                    for track in &grp.contour_tracks {
                        for key in track {
                            let c = &key.value;
                            let mut shap = Vec::with_capacity(12 + c.vertices.len() * 24);
                            shap.extend_from_slice(&key.frame.to_be_bytes());
                            shap.extend_from_slice(&(c.vertices.len() as u32).to_be_bytes());
                            shap.push(u8::from(c.closed));
                            shap.extend_from_slice(&[0, 0, 0]);
                            for idx in 0..c.vertices.len() {
                                let v = c.vertices[idx];
                                let tin = c.in_tangents.get(idx).copied().unwrap_or([0.0, 0.0]);
                                let tout = c.out_tangents.get(idx).copied().unwrap_or([0.0, 0.0]);
                                for val in [v[0], v[1], tin[0], tin[1], tout[0], tout[1]] {
                                    shap.extend_from_slice(
                                        &(finite_or(val, 0.0) as f32).to_be_bytes(),
                                    );
                                }
                            }
                            push_riff_chunk(&mut layr_body, b"shap", &shap)?;
                        }
                    }
                }
            }

            // AE JavaScript Expressions (`expr` UTF-8 chunks)
            for (prop_name, expr_opt) in [
                ("ADBE Position", &layer.expressions.position),
                ("ADBE Scale", &layer.expressions.scale),
                ("ADBE Rotate Z", &layer.expressions.rotation),
                ("ADBE Opacity", &layer.expressions.opacity),
                ("ADBE Geometry2-0006", &layer.expressions.skew),
                ("ADBE Time Remapping", &layer.expressions.time_remap),
                ("ADBE Text Document", &layer.expressions.source_text),
            ] {
                if let Some(code) = expr_opt {
                    let payload = format!("{prop_name}\0{code}");
                    push_riff_chunk(&mut layr_body, b"expr", payload.as_bytes())?;
                }
            }

            for m in &layer.markers {
                let payload = format!(
                    "{}|{}|{}|{}",
                    m.frame,
                    m.duration_frames,
                    sanitize_riff_marker_field(&m.label),
                    sanitize_riff_marker_field(&m.comment)
                );
                push_riff_chunk(&mut layr_body, b"mrkr", payload.as_bytes())?;
            }

            push_riff_list(&mut item_body, b"Layr", &layr_body)?;
        }

        push_riff_list(&mut fold_body, b"Item", &item_body)?;
    }

    push_riff_list(&mut egg_body, b"Fold", &fold_body)?;

    // Embed full structured interchange model + ExtendScript builder inside `NEX1` & `ESCR` chunks
    let model_json = serde_json::to_vec(model)?;
    push_riff_chunk(&mut egg_body, b"NEX1", &model_json)?;
    push_riff_chunk(&mut egg_body, b"ESCR", jsx_source.as_bytes())?;

    let total_riff = egg_body
        .len()
        .checked_add(4)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| {
            ExportError::Options(
                "After Effects RIFX container exceeds the 32-bit length limit.".into(),
            )
        })?;
    let mut rifx = Vec::with_capacity(12 + egg_body.len());
    rifx.extend_from_slice(b"RIFX");
    rifx.extend_from_slice(&total_riff.to_be_bytes());
    rifx.extend_from_slice(b"Egg!");
    rifx.extend_from_slice(&egg_body);
    Ok(rifx)
}
