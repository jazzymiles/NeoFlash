//! Vector shape, morph, paint, gradient, dash, mask/matte, and text layer emitters for Lottie.

use super::{
    LottieBuilder, LottieNoteKind, LottieTargetPlayer, round4,
    tracks::{
        LayerSpan, easing_to_lottie_tangents, invert_affine, layer_ease_at, layer_element_name,
        static_ks, transforms_approx_eq,
    },
};
use crate::ExportError;
use neoflash_core::{
    AppearancePaintEntry, Color, ColorTransform, Easing, Element, ElementKind, FillRule,
    GradientKind, GradientPaint, GradientSpread, Keyframe, Layer, MaskMode, PathCommand,
    PatternBrushKind, TextAlign, TextFieldType, Timeline, Transform, TweenKind,
    camera_layer_matrix, layer_effectively_visible, sample_layer_in_at_fps, sample_timeline_camera,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

impl<'a> LottieBuilder<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn emit_sampled_shape_span(
        &mut self,
        symbol_id: &str,
        timeline: &Timeline,
        timeline_has_camera: bool,
        layer: &Layer,
        span: &LayerSpan,
        layer_bm: u32,
        mask_layer: Option<&Layer>,
        origin_offset: [f64; 2],
        stack: &mut Vec<String>,
        next_ind: &mut u32,
    ) -> Result<Vec<Value>, ExportError> {
        let stage_center = [self.comp_w as f64 * 0.5, self.comp_h as f64 * 0.5];
        let apply_cam = |mut els: Vec<Element>, f: u32| -> Vec<Element> {
            let controllers = layer
                .key_at(f)
                .map(|k| k.controller_elements.as_slice())
                .unwrap_or(&[]);
            els.retain(|e| e.visible && !controllers.iter().any(|cid| cid == &e.id));
            if timeline_has_camera
                && !layer.properties.attach_to_camera
                && let Some(cam) = sample_timeline_camera(timeline, f)
            {
                let cam_m = camera_layer_matrix(&cam, layer.properties.z_depth, stage_center);
                for el in &mut els {
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
            }
            els
        };

        let mut frame_shapes: Vec<(u32, Element)> = Vec::new();
        let mut single_track = true;
        for f in span.start_frame..span.end_frame {
            self.check_cancelled()?;
            let mut els = apply_cam(
                sample_shape_tween_frame(timeline, layer, f, self.source_fps),
                f,
            );
            if els.len() == 1 && els[0].to_path_commands().is_some() {
                frame_shapes.push((f, els.pop().unwrap()));
            } else {
                single_track = false;
                break;
            }
        }

        if single_track
            && !frame_shapes.is_empty()
            && let Some(layers) = self.try_build_morphed_shape_layer(
                symbol_id,
                timeline,
                layer,
                span,
                &frame_shapes,
                layer_bm,
                mask_layer,
                origin_offset,
                stack,
                next_ind,
            )?
        {
            return Ok(layers);
        }

        // Fallback: emit 1-frame slices for arbitrary multi-shape topology changes.
        let mut out = Vec::new();
        for f in (span.start_frame..span.end_frame).rev() {
            self.check_cancelled()?;
            let els = apply_cam(
                sample_layer_in_at_fps(timeline, layer, f, self.source_fps),
                f,
            );
            for el in els.into_iter().rev() {
                if let Some(shape_group) = self.element_to_shape_group(
                    &el,
                    [0.0, 0.0],
                    true,
                    None,
                    Some(symbol_id),
                    Some(&layer.id),
                    Some(f),
                ) {
                    let ind = *next_ind;
                    *next_ind += 1;
                    let bm = if el.blend_mode.is_normal() {
                        layer_bm
                    } else {
                        self.map_blend_mode(
                            el.blend_mode,
                            &format!("Element '{}'", el.id),
                            Some(symbol_id),
                            Some(&layer.id),
                            Some(f),
                        )
                    };
                    let obj = json!({
                        "ddd": 0,
                        "ind": ind,
                        "ty": 4,
                        "nm": layer_element_name(layer, &el),
                        "sr": 1,
                        "ks": static_ks(el.transform, [0.0, 0.0], origin_offset),
                        "ao": 0,
                        "shapes": [shape_group],
                        "ip": f,
                        "op": f + 1,
                        "st": 0,
                        "bm": bm
                    });
                    let world_mask_samples = [(f, el.transform)];
                    let wrapped = self.attach_mask_or_matte(
                        symbol_id,
                        timeline,
                        mask_layer,
                        obj,
                        f,
                        f + 1,
                        &world_mask_samples,
                        origin_offset,
                        stack,
                        next_ind,
                    )?;
                    out.extend(wrapped);
                }
            }
        }
        Ok(out)
    }

    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    fn try_build_morphed_shape_layer(
        &mut self,
        symbol_id: &str,
        timeline: &Timeline,
        layer: &Layer,
        span: &LayerSpan,
        frame_shapes: &[(u32, Element)],
        layer_bm: u32,
        mask_layer: Option<&Layer>,
        origin_offset: [f64; 2],
        stack: &mut Vec<String>,
        next_ind: &mut u32,
    ) -> Result<Option<Vec<Value>>, ExportError> {
        struct MorphFrameSample {
            frame: u32,
            contours: Vec<LottieBezier>,
            fill: Color,
            stroke: Color,
            fill_grad: Option<GradientPaint>,
            stroke_grad: Option<GradientPaint>,
            stroke_width: f64,
            fill_rule: FillRule,
        }

        let mut samples_per_frame: Vec<MorphFrameSample> = Vec::with_capacity(frame_shapes.len());
        for (f, el) in frame_shapes {
            self.check_cancelled()?;
            let resolved_el = resolve_parametric_stroke_element(el);
            let target_el = resolved_el.as_ref().unwrap_or(el);
            let Some((cmds, fill_rule)) = target_el.to_path_commands() else {
                return Ok(None);
            };
            let m = neoflash_core::transform_matrix(Transform {
                x: el.transform.x + origin_offset[0],
                y: el.transform.y + origin_offset[1],
                ..el.transform
            });
            let tx_cmds = transform_path_commands(&cmds, m);
            let contours = path_commands_to_lottie_beziers(&tx_cmds);
            if contours.is_empty() {
                return Ok(None);
            }
            let (raw_fill, raw_stroke, stroke_width) = extract_element_paint(target_el);
            let ct = el.transform.color_transform;
            let fill = ct.apply(raw_fill);
            let stroke = ct.apply(raw_stroke);
            let (fill_grad, stroke_grad) = match &target_el.kind {
                ElementKind::PaintedPath {
                    fill_gradient,
                    stroke_gradient,
                    ..
                } => (
                    fill_gradient
                        .as_ref()
                        .map(|g| transform_gradient_paint(g, m, ct, true)),
                    stroke_gradient
                        .as_ref()
                        .map(|g| transform_gradient_paint(g, m, ct, true)),
                ),
                _ => (None, None),
            };
            samples_per_frame.push(MorphFrameSample {
                frame: *f,
                contours,
                fill,
                stroke,
                fill_grad,
                stroke_grad,
                stroke_width,
                fill_rule,
            });
        }

        let contour_count = samples_per_frame[0].contours.len();
        let vertex_counts: Vec<usize> = samples_per_frame[0]
            .contours
            .iter()
            .map(|c| c.v.len())
            .collect();
        if samples_per_frame.iter().any(|s| {
            s.contours.len() != contour_count
                || s.contours
                    .iter()
                    .zip(&vertex_counts)
                    .any(|(c, &expected)| c.v.len() != expected)
        }) {
            return Ok(None);
        }

        let any_fill_grad = samples_per_frame.iter().any(|s| s.fill_grad.is_some());
        let any_stroke_grad = samples_per_frame.iter().any(|s| s.stroke_grad.is_some());
        let normalized_fill_grads: Option<Vec<(u32, GradientPaint)>> = if any_fill_grad {
            let Some(template) = samples_per_frame.iter().find_map(|s| s.fill_grad.as_ref()) else {
                return Ok(None);
            };
            let kind = template.kind;
            let stop_len = template.stops.len();
            if stop_len == 0 {
                return Ok(None);
            }
            let mut grads = Vec::with_capacity(samples_per_frame.len());
            for s in &samples_per_frame {
                if let Some(g) = &s.fill_grad {
                    if g.kind != kind || g.stops.len() != stop_len {
                        return Ok(None);
                    }
                    grads.push((s.frame, g.clone()));
                } else {
                    grads.push((s.frame, solid_as_matching_gradient(s.fill, template)));
                }
            }
            Some(grads)
        } else {
            None
        };

        let normalized_stroke_grads: Option<Vec<(u32, GradientPaint)>> = if any_stroke_grad {
            let Some(template) = samples_per_frame
                .iter()
                .find_map(|s| s.stroke_grad.as_ref())
            else {
                return Ok(None);
            };
            let kind = template.kind;
            let stop_len = template.stops.len();
            if stop_len == 0 {
                return Ok(None);
            }
            let mut grads = Vec::with_capacity(samples_per_frame.len());
            for s in &samples_per_frame {
                if let Some(g) = &s.stroke_grad {
                    if g.kind != kind || g.stops.len() != stop_len {
                        return Ok(None);
                    }
                    grads.push((s.frame, g.clone()));
                } else {
                    grads.push((s.frame, solid_as_matching_gradient(s.stroke, template)));
                }
            }
            Some(grads)
        } else {
            None
        };

        let mut items: Vec<Value> = Vec::new();
        for c_idx in 0..contour_count {
            self.check_cancelled()?;
            let mut kfs = Vec::new();
            for (idx, s) in samples_per_frame.iter().enumerate() {
                if idx & 15 == 0 {
                    self.check_cancelled()?;
                }
                let bez = s.contours[c_idx].to_json();
                if idx + 1 < samples_per_frame.len() {
                    kfs.push(json!({
                        "t": s.frame,
                        "s": [bez],
                        "i": {"x": [1.0], "y": [1.0]},
                        "o": {"x": [0.0], "y": [0.0]}
                    }));
                } else {
                    kfs.push(json!({
                        "t": s.frame,
                        "s": [bez]
                    }));
                }
            }
            let dir = samples_per_frame[0].contours[c_idx].lottie_direction();
            items.push(json!({
                "ty": "sh",
                "d": dir,
                "nm": format!("Path {}", c_idx + 1),
                "ks": {
                    "a": if kfs.len() > 1 { 1 } else { 0 },
                    "k": if kfs.len() > 1 { Value::Array(kfs) } else { samples_per_frame[0].contours[c_idx].to_json() }
                }
            }));
        }

        let fill_rule0 = samples_per_frame[0].fill_rule;
        let fill0 = samples_per_frame[0].fill;
        let stroke0 = samples_per_frame[0].stroke;
        let sw0 = samples_per_frame[0].stroke_width;

        if let Some(grads) = &normalized_fill_grads {
            self.note_gradient_approximations(&grads[0].1, symbol_id, &layer.id, span.start_frame);
            items.push(animated_gradient_fill_item(grads, fill_rule0));
        } else if samples_per_frame.iter().any(|s| s.fill[3] > 0) {
            let fill_rgb_varies = samples_per_frame
                .iter()
                .any(|s| s.fill[0..3] != fill0[0..3]);
            let fill_alpha_varies = samples_per_frame.iter().any(|s| s.fill[3] != fill0[3]);
            let c_val = if fill_rgb_varies && samples_per_frame.len() > 1 {
                let kfs: Vec<Value> = samples_per_frame
                    .iter()
                    .enumerate()
                    .map(|(idx, s)| {
                        let [r, g, b, _] = s.fill;
                        let c = [
                            round4(f64::from(r) / 255.0),
                            round4(f64::from(g) / 255.0),
                            round4(f64::from(b) / 255.0),
                            1.0,
                        ];
                        if idx + 1 < samples_per_frame.len() {
                            json!({"t": s.frame, "s": c, "i": {"x": [1.0], "y": [1.0]}, "o": {"x": [0.0], "y": [0.0]}})
                        } else {
                            json!({"t": s.frame, "s": c})
                        }
                    })
                    .collect();
                json!({"a": 1, "k": kfs})
            } else {
                let [r, g, b, _] = fill0;
                json!({"a": 0, "k": [round4(f64::from(r) / 255.0), round4(f64::from(g) / 255.0), round4(f64::from(b) / 255.0), 1.0]})
            };
            let o_val = if fill_alpha_varies && samples_per_frame.len() > 1 {
                let kfs: Vec<Value> = samples_per_frame
                    .iter()
                    .enumerate()
                    .map(|(idx, s)| {
                        let o = round4(f64::from(s.fill[3]) / 255.0 * 100.0);
                        if idx + 1 < samples_per_frame.len() {
                            json!({"t": s.frame, "s": [o], "i": {"x": [1.0], "y": [1.0]}, "o": {"x": [0.0], "y": [0.0]}})
                        } else {
                            json!({"t": s.frame, "s": [o]})
                        }
                    })
                    .collect();
                json!({"a": 1, "k": kfs})
            } else {
                json!({"a": 0, "k": round4(f64::from(fill0[3]) / 255.0 * 100.0)})
            };
            items.push(json!({
                "ty": "fl",
                "nm": "Fill",
                "c": c_val,
                "o": o_val,
                "r": if fill_rule0 == FillRule::EvenOdd { 2 } else { 1 },
                "bm": 0
            }));
        }

        if let Some(grads) = &normalized_stroke_grads
            && samples_per_frame.iter().any(|s| s.stroke_width > 0.0)
        {
            self.note_gradient_approximations(&grads[0].1, symbol_id, &layer.id, span.start_frame);
            let widths: Vec<(u32, f64)> = samples_per_frame
                .iter()
                .map(|s| (s.frame, s.stroke_width))
                .collect();
            items.push(animated_gradient_stroke_item(grads, &widths));
        } else if samples_per_frame
            .iter()
            .any(|s| s.stroke[3] > 0 && s.stroke_width > 0.0)
        {
            let stroke_rgb_varies = samples_per_frame
                .iter()
                .any(|s| s.stroke[0..3] != stroke0[0..3]);
            let stroke_alpha_varies = samples_per_frame.iter().any(|s| s.stroke[3] != stroke0[3]);
            let sw_varies = samples_per_frame
                .iter()
                .any(|s| (s.stroke_width - sw0).abs() > 1e-4);
            let c_val = if stroke_rgb_varies && samples_per_frame.len() > 1 {
                let kfs: Vec<Value> = samples_per_frame
                    .iter()
                    .enumerate()
                    .map(|(idx, s)| {
                        let [r, g, b, _] = s.stroke;
                        let c = [
                            round4(f64::from(r) / 255.0),
                            round4(f64::from(g) / 255.0),
                            round4(f64::from(b) / 255.0),
                            1.0,
                        ];
                        if idx + 1 < samples_per_frame.len() {
                            json!({"t": s.frame, "s": c, "i": {"x": [1.0], "y": [1.0]}, "o": {"x": [0.0], "y": [0.0]}})
                        } else {
                            json!({"t": s.frame, "s": c})
                        }
                    })
                    .collect();
                json!({"a": 1, "k": kfs})
            } else {
                let [r, g, b, _] = stroke0;
                json!({"a": 0, "k": [round4(f64::from(r) / 255.0), round4(f64::from(g) / 255.0), round4(f64::from(b) / 255.0), 1.0]})
            };
            let o_val = if stroke_alpha_varies && samples_per_frame.len() > 1 {
                let kfs: Vec<Value> = samples_per_frame
                    .iter()
                    .enumerate()
                    .map(|(idx, s)| {
                        let o = round4(f64::from(s.stroke[3]) / 255.0 * 100.0);
                        if idx + 1 < samples_per_frame.len() {
                            json!({"t": s.frame, "s": [o], "i": {"x": [1.0], "y": [1.0]}, "o": {"x": [0.0], "y": [0.0]}})
                        } else {
                            json!({"t": s.frame, "s": [o]})
                        }
                    })
                    .collect();
                json!({"a": 1, "k": kfs})
            } else {
                json!({"a": 0, "k": round4(f64::from(stroke0[3]) / 255.0 * 100.0)})
            };
            let w_val = if sw_varies && samples_per_frame.len() > 1 {
                let kfs: Vec<Value> = samples_per_frame
                    .iter()
                    .enumerate()
                    .map(|(idx, s)| {
                        let w = round4(s.stroke_width);
                        if idx + 1 < samples_per_frame.len() {
                            json!({"t": s.frame, "s": [w], "i": {"x": [1.0], "y": [1.0]}, "o": {"x": [0.0], "y": [0.0]}})
                        } else {
                            json!({"t": s.frame, "s": [w]})
                        }
                    })
                    .collect();
                json!({"a": 1, "k": kfs})
            } else {
                json!({"a": 0, "k": round4(sw0)})
            };
            items.push(json!({
                "ty": "st",
                "nm": "Stroke",
                "c": c_val,
                "o": o_val,
                "w": w_val,
                "lc": 2,
                "lj": 2,
                "ml": 4,
                "bm": 0
            }));
        }
        items.push(identity_group_tr());

        let ind = *next_ind;
        *next_ind += 1;
        let first_el = &frame_shapes[0].1;
        let obj = json!({
            "ddd": 0,
            "ind": ind,
            "ty": 4,
            "nm": layer_element_name(layer, first_el),
            "sr": 1,
            "ks": static_ks(Transform::default(), [0.0, 0.0], [0.0, 0.0]),
            "ao": 0,
            "shapes": [{
                "ty": "gr",
                "nm": "Shape Group",
                "it": items,
                "bm": 0
            }],
            "ip": span.start_frame,
            "op": span.end_frame,
            "st": 0,
            "bm": layer_bm
        });
        let identity_mask_samples: Vec<(u32, Transform)> = frame_shapes
            .iter()
            .map(|(f, _)| {
                (
                    *f,
                    Transform {
                        x: -origin_offset[0],
                        y: -origin_offset[1],
                        ..Transform::default()
                    },
                )
            })
            .collect();
        let wrapped = self.attach_mask_or_matte(
            symbol_id,
            timeline,
            mask_layer,
            obj,
            span.start_frame,
            span.end_frame,
            &identity_mask_samples,
            origin_offset,
            stack,
            next_ind,
        )?;
        self.exported_count += 1;
        Ok(Some(wrapped))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn emit_text_element_layer(
        &mut self,
        symbol_id: &str,
        timeline: &Timeline,
        layer: &Layer,
        samples: &[(u32, Element)],
        representative: &Element,
        text: &str,
        font_family: &str,
        font_size: f64,
        color: Color,
        first_frame: u32,
        last_frame: u32,
        bm: u32,
        orient_auto: bool,
        force_dense_ks: bool,
        mask_layer: Option<&Layer>,
        origin_offset: [f64; 2],
        comp_origin_offset: [f64; 2],
        world_samples_for_mask: &[(u32, Transform)],
        stack: &mut Vec<String>,
        next_ind: &mut u32,
    ) -> Result<Vec<Value>, ExportError> {
        if let Some(outlined) = self.try_emit_outlined_text_layer(
            symbol_id,
            timeline,
            layer,
            samples,
            representative,
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
        )? {
            return Ok(outlined);
        }
        self.emitted_text_layer = true;
        let f_name = self.register_font(font_family, false, false);
        self.record_glyph_chars_for_text(text, font_family, font_size, false, false);
        let ks = self.build_animated_ks(
            timeline,
            layer,
            samples,
            [0.0, 0.0],
            origin_offset,
            force_dense_ks,
            orient_auto,
        )?;
        let ind = *next_ind;
        *next_ind += 1;
        let [r, g, b, _] = representative.transform.color_transform.apply(color);
        let obj = json!({
            "ddd": 0,
            "ind": ind,
            "ty": 5,
            "nm": layer_element_name(layer, representative),
            "sr": 1,
            "ks": ks,
            "ao": if orient_auto { 1 } else { 0 },
            "t": {
                "d": {
                    "k": [{
                        "t": 0,
                        "s": {
                            "s": round4(font_size),
                            "f": f_name,
                            "t": text.replace('\n', "\r"),
                            "j": 0,
                            "tr": 0,
                            "lh": round4(font_size * 1.2),
                            "ls": 0,
                            "fc": [round4(f64::from(r) / 255.0), round4(f64::from(g) / 255.0), round4(f64::from(b) / 255.0)]
                        }
                    }]
                },
                "p": {},
                "m": {"g": 1, "a": {"a": 0, "k": [0, 0]}},
                "a": []
            },
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

    #[allow(clippy::too_many_arguments)]
    pub(super) fn emit_textbox_element_layer(
        &mut self,
        symbol_id: &str,
        timeline: &Timeline,
        layer: &Layer,
        samples: &[(u32, Element)],
        representative: &Element,
        text: &str,
        width: f64,
        height: f64,
        format: &neoflash_core::TextFormat,
        first_frame: u32,
        last_frame: u32,
        bm: u32,
        orient_auto: bool,
        force_dense_ks: bool,
        mask_layer: Option<&Layer>,
        origin_offset: [f64; 2],
        comp_origin_offset: [f64; 2],
        world_samples_for_mask: &[(u32, Transform)],
        stack: &mut Vec<String>,
        next_ind: &mut u32,
    ) -> Result<Vec<Value>, ExportError> {
        if let Some(outlined) = self.try_emit_outlined_text_layer(
            symbol_id,
            timeline,
            layer,
            samples,
            representative,
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
        )? {
            return Ok(outlined);
        }
        self.emitted_text_layer = true;
        if let Some(tf_state) = neoflash_core::TextFieldState::from_kind(&representative.kind) {
            let kind_label = match tf_state.field_type {
                TextFieldType::Input => "Input",
                TextFieldType::Dynamic => "Dynamic",
            };
            self.add_note_coded(
                LottieNoteKind::Approximated,
                Some("LOTTIE_DYNAMIC_INPUT_TEXT"),
                format!(
                    "Layer '{}' · {}",
                    layer.name,
                    layer_element_name(layer, representative)
                ),
                format!(
                    "{kind_label} text field interactivity/runtime binding is not supported by Lottie players; static text content was exported."
                ),
                Some(symbol_id.to_owned()),
                Some(layer.id.clone()),
                Some(first_frame),
            );
        }
        let f_name = self.register_font(&format.font, format.bold, format.italic);
        self.record_glyph_chars_for_text(
            text,
            &format.font,
            format.size,
            format.bold,
            format.italic,
        );
        let ks = self.build_animated_ks(
            timeline,
            layer,
            samples,
            [0.0, 0.0],
            origin_offset,
            force_dense_ks,
            orient_auto,
        )?;
        let ind = *next_ind;
        *next_ind += 1;
        let fmt_rgba = [
            ((format.color >> 16) & 0xFF) as u8,
            ((format.color >> 8) & 0xFF) as u8,
            (format.color & 0xFF) as u8,
            255,
        ];
        let [r, g, b, _] = representative.transform.color_transform.apply(fmt_rgba);
        let justify = match format.effective_align_mode() {
            "right" | "end" => 1,
            "center" => 2,
            "justify" | "justify_all" => 3,
            _ => match format.align {
                TextAlign::Left => 0,
                TextAlign::Right => 1,
                TextAlign::Center => 2,
            },
        };
        let tracking = if format.size > 1e-3 {
            round4((format.letter_spacing / format.size) * 1000.0)
        } else {
            0.0
        };
        let obj = json!({
            "ddd": 0,
            "ind": ind,
            "ty": 5,
            "nm": layer_element_name(layer, representative),
            "sr": 1,
            "ks": ks,
            "ao": if orient_auto { 1 } else { 0 },
            "t": {
                "d": {
                    "k": [{
                        "t": 0,
                        "s": {
                            "s": round4(format.size),
                            "f": f_name,
                            "t": text.replace('\n', "\r"),
                            "j": justify,
                            "tr": tracking,
                            "lh": round4(format.size * 1.2 + format.leading),
                            "ls": 0,
                            "fc": [round4(f64::from(r) / 255.0), round4(f64::from(g) / 255.0), round4(f64::from(b) / 255.0)],
                            "sz": [round4(width), round4(height)],
                            "ps": [0.0, 0.0]
                        }
                    }]
                },
                "p": {},
                "m": {"g": 1, "a": {"a": 0, "k": [0, 0]}},
                "a": []
            },
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

    fn note_gradient_approximations(
        &mut self,
        grad: &GradientPaint,
        symbol_id: &str,
        layer_id: &str,
        frame: u32,
    ) {
        if grad.spread != GradientSpread::Extend {
            self.add_note_coded(
                LottieNoteKind::Approximated,
                Some("LOTTIE_GRADIENT_SPREAD"),
                format!("Layer '{layer_id}' (frame {})", frame + 1),
                format!(
                    "Gradient spread mode '{:?}' is approximated as pad/extend in Lottie.",
                    grad.spread
                ),
                Some(symbol_id.to_owned()),
                Some(layer_id.to_owned()),
                Some(frame),
            );
        }
        if grad.linear_rgb {
            self.add_note_coded(
                LottieNoteKind::Approximated,
                Some("LOTTIE_GRADIENT_LINEAR_RGB"),
                format!("Layer '{layer_id}' (frame {})", frame + 1),
                "Linear-RGB gradient interpolation is approximated in standard sRGB in Lottie.",
                Some(symbol_id.to_owned()),
                Some(layer_id.to_owned()),
                Some(frame),
            );
        }
    }

    pub(super) fn build_shape_group_for_samples(
        &mut self,
        layer: &Layer,
        samples: &[(u32, Element)],
        force_dense: bool,
        dash_override: Option<&NativeDashStroke>,
        symbol_id: &str,
    ) -> Result<Option<Value>, ExportError> {
        self.check_cancelled()?;
        let first_el = &samples[0].1;
        let first_f = samples[0].0;
        let shape_static = dash_override.is_some()
            || samples.iter().all(|(_, e)| {
                e.kind == first_el.kind
                    && e.transform.primitive == first_el.transform.primitive
                    && apply_rgb_only(e.transform.color_transform, [255, 255, 255, 255], false)
                        == apply_rgb_only(
                            first_el.transform.color_transform,
                            [255, 255, 255, 255],
                            false,
                        )
            });

        if shape_static {
            return Ok(self.element_to_shape_group(
                first_el,
                [0.0, 0.0],
                false,
                dash_override,
                Some(symbol_id),
                Some(&layer.id),
                Some(first_f),
            ));
        }

        let key_samples: Vec<(u32, &Element, Option<Easing>)> = if force_dense {
            samples
                .iter()
                .enumerate()
                .map(|(idx, (f, el))| {
                    let ease = (idx + 1 < samples.len()).then_some(Easing::Linear);
                    (*f, el, ease)
                })
                .collect()
        } else {
            let sample_map: BTreeMap<u32, &Element> =
                samples.iter().map(|(f, e)| (*f, e)).collect();
            let mut key_times = BTreeSet::new();
            key_times.insert(samples[0].0);
            key_times.insert(samples.last().unwrap().0);
            for kf in &layer.keyframes {
                if kf.frame >= samples[0].0
                    && kf.frame <= samples.last().unwrap().0
                    && sample_map.contains_key(&kf.frame)
                {
                    key_times.insert(kf.frame);
                }
            }
            key_times
                .into_iter()
                .map(|f| (f, sample_map[&f], layer_ease_at(layer, f, &first_el.id)))
                .collect()
        };

        let mut per_key = Vec::with_capacity(key_samples.len());
        for (f, el, ease) in &key_samples {
            self.check_cancelled()?;
            let resolved = resolve_parametric_stroke_element(el);
            let target = resolved.as_ref().unwrap_or(el);
            if matches!(
                target.kind,
                ElementKind::PaintedPath {
                    fill_gradient: Some(_),
                    ..
                } | ElementKind::PaintedPath {
                    stroke_gradient: Some(_),
                    ..
                }
            ) {
                return Ok(self.element_to_shape_group(
                    first_el,
                    [0.0, 0.0],
                    false,
                    None,
                    Some(symbol_id),
                    Some(&layer.id),
                    Some(first_f),
                ));
            }
            let Some((cmds, fill_rule)) = target.to_path_commands() else {
                return Ok(None);
            };
            let contours = path_commands_to_lottie_beziers(&cmds);
            if contours.is_empty() {
                return Ok(None);
            }
            let (raw_fill, raw_stroke, sw) = extract_element_paint(target);
            let ct = el.transform.color_transform;
            let fill = apply_rgb_only(ct, raw_fill, false);
            let stroke = apply_rgb_only(ct, raw_stroke, false);
            per_key.push((*f, *ease, contours, fill, stroke, sw, fill_rule));
        }

        let c_count = per_key[0].2.len();
        let v_counts: Vec<usize> = per_key[0].2.iter().map(|c| c.v.len()).collect();
        let compatible = per_key.iter().all(|(_, _, cs, _, _, _, _)| {
            cs.len() == c_count
                && cs
                    .iter()
                    .zip(&v_counts)
                    .all(|(c, &expected)| c.v.len() == expected)
        });
        if !compatible {
            return Ok(self.element_to_shape_group(
                first_el,
                [0.0, 0.0],
                false,
                None,
                Some(symbol_id),
                Some(&layer.id),
                Some(first_f),
            ));
        }

        let mut items: Vec<Value> = Vec::new();
        for c_idx in 0..c_count {
            self.check_cancelled()?;
            let first_json = per_key[0].2[c_idx].to_json();
            let dir = per_key[0].2[c_idx].lottie_direction();
            let geom_varies = per_key
                .iter()
                .any(|(_, _, cs, _, _, _, _)| cs[c_idx].to_json() != first_json);
            if geom_varies && per_key.len() > 1 {
                let mut kfs = Vec::with_capacity(per_key.len());
                for (idx, (f, ease, cs, _, _, _, _)) in per_key.iter().enumerate() {
                    if idx & 15 == 0 {
                        self.check_cancelled()?;
                    }
                    let bez = cs[c_idx].to_json();
                    if idx + 1 < per_key.len() {
                        if let Some(e) = ease {
                            let (o, i) = easing_to_lottie_tangents(*e);
                            kfs.push(json!({
                                "t": *f,
                                "s": [bez],
                                "o": {"x": [o[0]], "y": [o[1]]},
                                "i": {"x": [i[0]], "y": [i[1]]}
                            }));
                        } else {
                            kfs.push(json!({"t": *f, "s": [bez], "h": 1}));
                        }
                    } else {
                        kfs.push(json!({"t": *f, "s": [bez]}));
                    }
                }
                items.push(json!({
                    "ty": "sh",
                    "d": dir,
                    "nm": format!("Path {}", c_idx + 1),
                    "ks": {"a": 1, "k": kfs}
                }));
            } else {
                items.push(json!({
                    "ty": "sh",
                    "d": dir,
                    "nm": format!("Path {}", c_idx + 1),
                    "ks": {"a": 0, "k": first_json}
                }));
            }
        }

        let fill0 = per_key[0].3;
        let stroke0 = per_key[0].4;
        let sw0 = per_key[0].5;
        let fill_rule0 = per_key[0].6;

        if per_key.iter().any(|(_, _, _, f, _, _, _)| f[3] > 0) {
            let rgb_varies = per_key
                .iter()
                .any(|(_, _, _, f, _, _, _)| f[0..3] != fill0[0..3]);
            let a_varies = per_key.iter().any(|(_, _, _, f, _, _, _)| f[3] != fill0[3]);
            let c_val = if rgb_varies && per_key.len() > 1 {
                let kfs: Vec<Value> = per_key
                    .iter()
                    .enumerate()
                    .map(|(idx, (f, ease, _, [r, g, b, _], _, _, _))| {
                        let c = [
                            round4(f64::from(*r) / 255.0),
                            round4(f64::from(*g) / 255.0),
                            round4(f64::from(*b) / 255.0),
                            1.0,
                        ];
                        if idx + 1 < per_key.len() {
                            if let Some(e) = ease {
                                let (o, i) = easing_to_lottie_tangents(*e);
                                json!({"t": *f, "s": c, "o": {"x": [o[0]], "y": [o[1]]}, "i": {"x": [i[0]], "y": [i[1]]}})
                            } else {
                                json!({"t": *f, "s": c, "h": 1})
                            }
                        } else {
                            json!({"t": *f, "s": c})
                        }
                    })
                    .collect();
                json!({"a": 1, "k": kfs})
            } else {
                let [r, g, b, _] = fill0;
                json!({"a": 0, "k": [round4(f64::from(r) / 255.0), round4(f64::from(g) / 255.0), round4(f64::from(b) / 255.0), 1.0]})
            };
            let o_val = if a_varies && per_key.len() > 1 {
                let kfs: Vec<Value> = per_key
                    .iter()
                    .enumerate()
                    .map(|(idx, (f, ease, _, [_, _, _, a], _, _, _))| {
                        let o_pct = round4(f64::from(*a) / 255.0 * 100.0);
                        if idx + 1 < per_key.len() {
                            if let Some(e) = ease {
                                let (o, i) = easing_to_lottie_tangents(*e);
                                json!({"t": *f, "s": [o_pct], "o": {"x": [o[0]], "y": [o[1]]}, "i": {"x": [i[0]], "y": [i[1]]}})
                            } else {
                                json!({"t": *f, "s": [o_pct], "h": 1})
                            }
                        } else {
                            json!({"t": *f, "s": [o_pct]})
                        }
                    })
                    .collect();
                json!({"a": 1, "k": kfs})
            } else {
                json!({"a": 0, "k": round4(f64::from(fill0[3]) / 255.0 * 100.0)})
            };
            items.push(json!({
                "ty": "fl",
                "nm": "Fill",
                "c": c_val,
                "o": o_val,
                "r": if fill_rule0 == FillRule::EvenOdd { 2 } else { 1 },
                "bm": 0
            }));
        }

        if per_key
            .iter()
            .any(|(_, _, _, _, s, sw, _)| s[3] > 0 && *sw > 0.0)
        {
            let rgb_varies = per_key
                .iter()
                .any(|(_, _, _, _, s, _, _)| s[0..3] != stroke0[0..3]);
            let a_varies = per_key
                .iter()
                .any(|(_, _, _, _, s, _, _)| s[3] != stroke0[3]);
            let sw_varies = per_key
                .iter()
                .any(|(_, _, _, _, _, sw, _)| (*sw - sw0).abs() > 1e-4);
            let c_val = if rgb_varies && per_key.len() > 1 {
                let kfs: Vec<Value> = per_key
                    .iter()
                    .enumerate()
                    .map(|(idx, (f, ease, _, _, [r, g, b, _], _, _))| {
                        let c = [
                            round4(f64::from(*r) / 255.0),
                            round4(f64::from(*g) / 255.0),
                            round4(f64::from(*b) / 255.0),
                            1.0,
                        ];
                        if idx + 1 < per_key.len() {
                            if let Some(e) = ease {
                                let (o, i) = easing_to_lottie_tangents(*e);
                                json!({"t": *f, "s": c, "o": {"x": [o[0]], "y": [o[1]]}, "i": {"x": [i[0]], "y": [i[1]]}})
                            } else {
                                json!({"t": *f, "s": c, "h": 1})
                            }
                        } else {
                            json!({"t": *f, "s": c})
                        }
                    })
                    .collect();
                json!({"a": 1, "k": kfs})
            } else {
                let [r, g, b, _] = stroke0;
                json!({"a": 0, "k": [round4(f64::from(r) / 255.0), round4(f64::from(g) / 255.0), round4(f64::from(b) / 255.0), 1.0]})
            };
            let o_val = if a_varies && per_key.len() > 1 {
                let kfs: Vec<Value> = per_key
                    .iter()
                    .enumerate()
                    .map(|(idx, (f, ease, _, _, [_, _, _, a], _, _))| {
                        let o_pct = round4(f64::from(*a) / 255.0 * 100.0);
                        if idx + 1 < per_key.len() {
                            if let Some(e) = ease {
                                let (o, i) = easing_to_lottie_tangents(*e);
                                json!({"t": *f, "s": [o_pct], "o": {"x": [o[0]], "y": [o[1]]}, "i": {"x": [i[0]], "y": [i[1]]}})
                            } else {
                                json!({"t": *f, "s": [o_pct], "h": 1})
                            }
                        } else {
                            json!({"t": *f, "s": [o_pct]})
                        }
                    })
                    .collect();
                json!({"a": 1, "k": kfs})
            } else {
                json!({"a": 0, "k": round4(f64::from(stroke0[3]) / 255.0 * 100.0)})
            };
            let w_val = if sw_varies && per_key.len() > 1 {
                let kfs: Vec<Value> = per_key
                    .iter()
                    .enumerate()
                    .map(|(idx, (f, ease, _, _, _, sw, _))| {
                        let w = round4(*sw);
                        if idx + 1 < per_key.len() {
                            if let Some(e) = ease {
                                let (o, i) = easing_to_lottie_tangents(*e);
                                json!({"t": *f, "s": [w], "o": {"x": [o[0]], "y": [o[1]]}, "i": {"x": [i[0]], "y": [i[1]]}})
                            } else {
                                json!({"t": *f, "s": [w], "h": 1})
                            }
                        } else {
                            json!({"t": *f, "s": [w]})
                        }
                    })
                    .collect();
                json!({"a": 1, "k": kfs})
            } else {
                json!({"a": 0, "k": round4(sw0)})
            };
            items.push(json!({
                "ty": "st",
                "nm": "Stroke",
                "c": c_val,
                "o": o_val,
                "w": w_val,
                "lc": 2,
                "lj": 2,
                "ml": 4,
                "bm": 0
            }));
        }

        items.push(identity_group_tr());
        Ok(Some(json!({
            "ty": "gr",
            "nm": if first_el.name.is_empty() { "Vector Shape" } else { &first_el.name },
            "it": items,
            "bm": 0
        })))
    }

    #[allow(clippy::too_many_arguments)]
    fn element_to_shape_group(
        &mut self,
        element: &Element,
        offset: [f64; 2],
        apply_color_tx_alpha: bool,
        dash_override: Option<&NativeDashStroke>,
        symbol_id: Option<&str>,
        layer_id: Option<&str>,
        frame: Option<u32>,
    ) -> Option<Value> {
        let resolved = if dash_override.is_none() {
            resolve_parametric_stroke_element(element)
        } else {
            None
        };
        let target = resolved.as_ref().unwrap_or(element);

        let (mut commands, fill_rule) = if let Some(d_ov) = dash_override {
            (d_ov.commands.clone(), FillRule::NonZero)
        } else {
            target.to_path_commands()?
        };
        if offset != [0.0, 0.0] {
            commands =
                transform_path_commands(&commands, [1.0, 0.0, 0.0, 1.0, offset[0], offset[1]]);
        }
        let contours = path_commands_to_lottie_beziers(&commands);
        if contours.is_empty() {
            return None;
        }
        let mut items: Vec<Value> = contours
            .into_iter()
            .enumerate()
            .map(|(i, bez)| {
                let dir = bez.lottie_direction();
                json!({
                    "ty": "sh",
                    "d": dir,
                    "nm": format!("Path {}", i + 1),
                    "ks": {
                        "a": 0,
                        "k": bez.to_json()
                    }
                })
            })
            .collect();

        let ct = element.transform.color_transform;
        if let Some(d_ov) = dash_override {
            let stroke = apply_rgb_only(ct, d_ov.color, apply_color_tx_alpha);
            if d_ov.width > 0.0 {
                let dash_json = build_lottie_dash_array(d_ov.dash, d_ov.gap);
                if let Some(grad) = &d_ov.gradient
                    && !grad.stops.is_empty()
                {
                    if let (Some(sid), Some(lid), Some(fr)) = (symbol_id, layer_id, frame) {
                        self.note_gradient_approximations(grad, sid, lid, fr);
                    }
                    let tinted = tint_gradient_paint(grad, ct, apply_color_tx_alpha);
                    let mut gs = gradient_stroke_item(&tinted, d_ov.width);
                    gs.as_object_mut()
                        .unwrap()
                        .insert("d".into(), Value::Array(dash_json));
                    items.push(gs);
                } else if stroke[3] > 0 {
                    let [r, g, b, a] = stroke;
                    items.push(json!({
                        "ty": "st",
                        "nm": "Stroke",
                        "c": {"a": 0, "k": [round4(f64::from(r) / 255.0), round4(f64::from(g) / 255.0), round4(f64::from(b) / 255.0), 1.0]},
                        "o": {"a": 0, "k": round4(f64::from(a) / 255.0 * 100.0)},
                        "w": {"a": 0, "k": round4(d_ov.width)},
                        "lc": 2,
                        "lj": 2,
                        "ml": 4,
                        "d": dash_json,
                        "bm": 0
                    }));
                }
            }
            items.push(identity_group_tr());
            return Some(json!({
                "ty": "gr",
                "nm": if element.name.is_empty() { "Vector Shape" } else { &element.name },
                "it": items,
                "bm": 0
            }));
        }

        let (raw_fill, raw_stroke, stroke_width) = extract_element_paint(target);
        let fill = apply_rgb_only(ct, raw_fill, apply_color_tx_alpha);
        let stroke = apply_rgb_only(ct, raw_stroke, apply_color_tx_alpha);

        let (fill_grad, stroke_grad) = match &target.kind {
            ElementKind::PaintedPath {
                fill_gradient,
                stroke_gradient,
                ..
            } => (fill_gradient.as_ref(), stroke_gradient.as_ref()),
            _ => (None, None),
        };

        if let Some(grad) = fill_grad
            && !grad.stops.is_empty()
        {
            if let (Some(sid), Some(lid), Some(fr)) = (symbol_id, layer_id, frame) {
                self.note_gradient_approximations(grad, sid, lid, fr);
            }
            let tinted = tint_gradient_paint(grad, ct, apply_color_tx_alpha);
            items.push(gradient_fill_item(&tinted, fill_rule));
        } else if fill[3] > 0 {
            let [r, g, b, a] = fill;
            items.push(json!({
                "ty": "fl",
                "nm": "Fill",
                "c": {"a": 0, "k": [round4(f64::from(r) / 255.0), round4(f64::from(g) / 255.0), round4(f64::from(b) / 255.0), 1.0]},
                "o": {"a": 0, "k": round4(f64::from(a) / 255.0 * 100.0)},
                "r": if fill_rule == FillRule::EvenOdd { 2 } else { 1 },
                "bm": 0
            }));
        }

        if stroke_width > 0.0 {
            if let Some(grad) = stroke_grad
                && !grad.stops.is_empty()
            {
                if let (Some(sid), Some(lid), Some(fr)) = (symbol_id, layer_id, frame) {
                    self.note_gradient_approximations(grad, sid, lid, fr);
                }
                let tinted = tint_gradient_paint(grad, ct, apply_color_tx_alpha);
                items.push(gradient_stroke_item(&tinted, stroke_width));
            } else if stroke[3] > 0 {
                let [r, g, b, a] = stroke;
                items.push(json!({
                    "ty": "st",
                    "nm": "Stroke",
                    "c": {"a": 0, "k": [round4(f64::from(r) / 255.0), round4(f64::from(g) / 255.0), round4(f64::from(b) / 255.0), 1.0]},
                    "o": {"a": 0, "k": round4(f64::from(a) / 255.0 * 100.0)},
                    "w": {"a": 0, "k": round4(stroke_width)},
                    "lc": 2,
                    "lj": 2,
                    "ml": 4,
                    "bm": 0
                }));
            }
        }

        items.push(identity_group_tr());
        Some(json!({
            "ty": "gr",
            "nm": if element.name.is_empty() { "Vector Shape" } else { &element.name },
            "it": items,
            "bm": 0
        }))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn attach_mask_or_matte(
        &mut self,
        symbol_id: &str,
        timeline: &Timeline,
        mask_layer: Option<&Layer>,
        mut layer_obj: Value,
        first_frame: u32,
        last_frame: u32,
        world_samples: &[(u32, Transform)],
        comp_origin_offset: [f64; 2],
        stack: &mut Vec<String>,
        next_ind: &mut u32,
    ) -> Result<Vec<Value>, ExportError> {
        let Some(mask_layer) = mask_layer else {
            return Ok(vec![layer_obj]);
        };
        if !layer_effectively_visible(timeline, mask_layer) {
            return Ok(vec![layer_obj]);
        }

        let mask_has_non_vector = (first_frame..last_frame).any(|f| {
            sample_layer_in_at_fps(timeline, mask_layer, f, self.source_fps)
                .iter()
                .any(|el| el.visible && el.to_path_commands().is_none())
        });

        let use_track_matte =
            mask_layer.properties.mask_mode != MaskMode::Geometry || mask_has_non_vector;

        if !use_track_matte
            && self.try_attach_vector_masks(
                &mut layer_obj,
                timeline,
                mask_layer,
                first_frame,
                last_frame,
                world_samples,
            )
        {
            return Ok(vec![layer_obj]);
        }

        let Some(matte_ref_id) = self.ensure_mask_matte_precomp(
            symbol_id,
            timeline,
            mask_layer,
            comp_origin_offset,
            stack,
        )?
        else {
            return Ok(vec![layer_obj]);
        };

        let tt_code = match (
            mask_layer.properties.mask_mode,
            mask_layer.properties.mask_invert,
        ) {
            (MaskMode::Luminance, false) => 3,
            (MaskMode::Luminance, true) => 4,
            (_, false) => 1,
            (_, true) => 2,
        };

        if self.options.player != LottieTargetPlayer::LottieWeb && matches!(tt_code, 3 | 4) {
            self.add_note_coded(
                LottieNoteKind::Approximated,
                Some("LOTTIE_LUMA_MATTE_PLAYER"),
                format!("Mask layer '{}'", mask_layer.name),
                format!(
                    "Luminance track matte (`tt: {tt_code}`) may have limited support on {} player.",
                    self.options.player.label()
                ),
                Some(symbol_id.to_owned()),
                Some(mask_layer.id.clone()),
                Some(first_frame),
            );
        }

        let matte_ind = *next_ind;
        *next_ind += 1;
        let matte_layer_json = json!({
            "ddd": 0,
            "ind": matte_ind,
            "ty": 0,
            "nm": format!("Matte · {}", mask_layer.name),
            "refId": matte_ref_id,
            "td": 1,
            "sr": 1,
            "ks": static_ks(Transform::default(), [0.0, 0.0], [0.0, 0.0]),
            "ao": 0,
            "w": self.comp_w.max(1),
            "h": self.comp_h.max(1),
            "ip": first_frame,
            "op": last_frame,
            "st": 0,
            "bm": 0
        });
        if let Some(map) = layer_obj.as_object_mut() {
            map.insert("tt".into(), json!(tt_code));
        }
        Ok(vec![matte_layer_json, layer_obj])
    }

    fn try_attach_vector_masks(
        &mut self,
        layer_obj: &mut Value,
        timeline: &Timeline,
        mask_layer: &Layer,
        first_frame: u32,
        last_frame: u32,
        world_samples: &[(u32, Transform)],
    ) -> bool {
        let fps = self.source_fps;
        let fallback_target = world_samples.first().map(|(_, t)| *t).unwrap_or_default();
        let target_at = |frame: u32| -> Transform {
            match world_samples.binary_search_by_key(&frame, |(f, _)| *f) {
                Ok(i) => world_samples[i].1,
                Err(0) => fallback_target,
                Err(i) if i >= world_samples.len() => world_samples.last().unwrap().1,
                Err(i) => {
                    let (f0, t0) = world_samples[i - 1];
                    let (f1, t1) = world_samples[i];
                    let u = if f1 > f0 {
                        (frame - f0) as f64 / (f1 - f0) as f64
                    } else {
                        0.0
                    };
                    Transform {
                        x: t0.x + (t1.x - t0.x) * u,
                        y: t0.y + (t1.y - t0.y) * u,
                        scale_x: t0.scale_x + (t1.scale_x - t0.scale_x) * u,
                        scale_y: t0.scale_y + (t1.scale_y - t0.scale_y) * u,
                        rotation: t0.rotation + (t1.rotation - t0.rotation) * u,
                        skew: t0.skew + (t1.skew - t0.skew) * u,
                        ..t0
                    }
                }
            }
        };

        // A precomp draws its origin at the layer anchor. Vector masks live in that same padded coordinate space.
        let anchor = &layer_obj["ks"]["a"]["k"];
        let anchor = [
            anchor[0].as_f64().unwrap_or_default(),
            anchor[1].as_f64().unwrap_or_default(),
        ];
        let mask_contours_at = |frame: u32| -> Vec<LottieBezier> {
            let inv_target = invert_affine(neoflash_core::transform_matrix(target_at(frame)));
            let mut out = Vec::new();
            for el in sample_layer_in_at_fps(timeline, mask_layer, frame, fps) {
                if !el.visible {
                    continue;
                }
                let Some((cmds, _)) = el.to_path_commands() else {
                    continue;
                };
                let mask_m = neoflash_core::transform_matrix(el.transform);
                let combined = match inv_target {
                    Some(inv) => neoflash_core::affine_multiply(inv, mask_m),
                    None => mask_m,
                };
                let mut combined = combined;
                combined[4] += anchor[0];
                combined[5] += anchor[1];
                let local_cmds = transform_path_commands(&cmds, combined);
                out.extend(path_commands_to_lottie_beziers(&local_cmds));
            }
            out
        };

        let first_contours = mask_contours_at(first_frame);
        if first_contours.is_empty() {
            return true;
        }

        let target_moves = world_samples
            .iter()
            .any(|(_, t)| !transforms_approx_eq(*t, fallback_target));
        let mask_animated = last_frame > first_frame + 1
            && (target_moves
                || mask_layer.keyframes.len() > 1
                || mask_layer
                    .keyframes
                    .iter()
                    .any(|k| k.tween.is_some() || !k.property_expressions.is_empty()));
        let mask_invert = mask_layer.properties.mask_invert;
        let mask_mode_for_index = |idx: usize| -> (bool, &'static str) {
            if mask_invert {
                if idx == 0 { (true, "a") } else { (false, "s") }
            } else {
                (false, "a")
            }
        };
        let mut masks_props = Vec::new();
        if mask_animated {
            let per_frame: Vec<(u32, Vec<LottieBezier>)> = (first_frame..last_frame)
                .map(|f| (f, mask_contours_at(f)))
                .collect();
            let count = first_contours.len();
            let v_counts: Vec<usize> = first_contours.iter().map(|c| c.v.len()).collect();
            let compatible = per_frame.iter().all(|(_, cs)| {
                cs.len() == count
                    && cs
                        .iter()
                        .zip(&v_counts)
                        .all(|(c, &expected)| c.v.len() == expected)
            });
            if !compatible {
                return false;
            }
            for c_idx in 0..count {
                let (inv, mode) = mask_mode_for_index(c_idx);
                let kfs: Vec<Value> = per_frame
                    .iter()
                    .enumerate()
                    .map(|(idx, (f, cs))| {
                        let bez = cs[c_idx].to_json();
                        if idx + 1 < per_frame.len() {
                            json!({
                                "t": *f,
                                "s": [bez],
                                "i": {"x": [1.0], "y": [1.0]},
                                "o": {"x": [0.0], "y": [0.0]}
                            })
                        } else {
                            json!({"t": *f, "s": [bez]})
                        }
                    })
                    .collect();
                masks_props.push(json!({
                    "inv": inv,
                    "mode": mode,
                    "pt": {"a": 1, "k": kfs},
                    "o": {"a": 0, "k": 100},
                    "x": {"a": 0, "k": 0},
                    "nm": mask_layer.name
                }));
            }
        } else {
            for (c_idx, bez) in first_contours.into_iter().enumerate() {
                let (inv, mode) = mask_mode_for_index(c_idx);
                masks_props.push(json!({
                    "inv": inv,
                    "mode": mode,
                    "pt": {"a": 0, "k": bez.to_json()},
                    "o": {"a": 0, "k": 100},
                    "x": {"a": 0, "k": 0},
                    "nm": mask_layer.name
                }));
            }
        }
        if !masks_props.is_empty()
            && let Some(map) = layer_obj.as_object_mut()
        {
            map.insert("hasMask".into(), Value::Bool(true));
            map.insert("masksProperties".into(), Value::Array(masks_props));
        }
        true
    }
}

#[derive(Clone, Debug)]
pub(super) struct NativeDashStroke {
    commands: Vec<PathCommand>,
    color: Color,
    gradient: Option<GradientPaint>,
    width: f64,
    dash: f64,
    gap: f64,
}

pub(super) fn resolve_native_dash_stroke(
    layer: &Layer,
    frame: u32,
    element_id: &str,
) -> Option<NativeDashStroke> {
    let kf = layer.key_at(frame)?;
    let (base_id, entry_idx) = if kf.appearance(element_id).is_some() {
        (element_id, 0usize)
    } else if let Some((prefix, idx_str)) = element_id.rsplit_once("-app-")
        && let Ok(idx) = idx_str.parse::<usize>()
        && kf.appearance(prefix).is_some()
    {
        (prefix, idx)
    } else {
        return None;
    };
    let app = kf.appearance(base_id)?;
    if !app.modifiers.is_empty() || app.text_binding.is_some() {
        return None;
    }
    let entry = app.entries.get(entry_idx)?;
    let AppearancePaintEntry::Stroke {
        color,
        gradient,
        width,
        offset_distance,
        pattern_brush: Some(PatternBrushKind::DashPattern { dash, gap }),
        ..
    } = entry
    else {
        return None;
    };
    if *width <= 0.0 || *dash <= 0.0 || *gap < 0.0 || offset_distance.abs() > 1e-6 {
        return None;
    }
    let base_el = kf.elements.iter().find(|e| e.id == base_id)?;
    let (commands, _) = base_el.to_path_commands()?;
    Some(NativeDashStroke {
        commands,
        color: *color,
        gradient: gradient.clone(),
        width: *width,
        dash: *dash,
        gap: *gap,
    })
}

fn build_lottie_dash_array(dash: f64, gap: f64) -> Vec<Value> {
    vec![
        json!({"n": "d", "nm": "dash", "v": {"a": 0, "k": round4(dash.max(0.01))}}),
        json!({"n": "g", "nm": "gap", "v": {"a": 0, "k": round4(gap.max(0.0))}}),
        json!({"n": "o", "nm": "offset", "v": {"a": 0, "k": 0.0}}),
    ]
}

fn resolve_parametric_stroke_element(element: &Element) -> Option<Element> {
    if !element.stroke_width_profile().is_default()
        && let Some(kind) = element.parametric_path_kind()
    {
        let mut expanded = element.clone();
        expanded.kind = kind;
        expanded.transform.primitive = None;
        return Some(expanded);
    }
    None
}

pub(super) fn sample_shape_tween_frame(
    timeline: &Timeline,
    layer: &Layer,
    frame: u32,
    fps: f64,
) -> Vec<Element> {
    fn has_post_shape_evaluation(key: &Keyframe) -> bool {
        !key.property_expressions.is_empty()
            || !key.ik_armatures.is_empty()
            || !key.element_parents.is_empty()
            || !key.asset_warps.is_empty()
            || !key.vector_effects.is_empty()
            || !key.appearances.is_empty()
    }

    // Plain endpoint keys keep the shape tween's resampled topology. That lets Lottie encode a
    // single animated path even when the authored endpoint primitives have different command
    // counts. Advanced keys must go through the shared evaluator so expressions, rigs, effects,
    // and appearances are not silently dropped at exact keyframes.
    if let Some((idx, key)) = layer
        .keyframes
        .iter()
        .enumerate()
        .find(|(_, key)| key.frame == frame)
    {
        if key.tween.is_some()
            && key.tween_kind == TweenKind::Shape
            && let Some(next) = layer.keyframes.get(idx + 1)
            && !has_post_shape_evaluation(key)
            && !has_post_shape_evaluation(next)
        {
            return neoflash_core::shape_tween_elements_with_hints(
                &key.elements,
                &next.elements,
                &key.shape_hints,
                0.0,
            );
        }
        if idx > 0 {
            let previous = &layer.keyframes[idx - 1];
            if previous.tween.is_some()
                && previous.tween_kind == TweenKind::Shape
                && !has_post_shape_evaluation(previous)
                && !has_post_shape_evaluation(key)
            {
                return neoflash_core::shape_tween_elements_with_hints(
                    &previous.elements,
                    &key.elements,
                    &previous.shape_hints,
                    1.0,
                );
            }
        }
    }
    sample_layer_in_at_fps(timeline, layer, frame, fps)
}

pub(super) fn identity_group_tr() -> Value {
    json!({
        "ty": "tr",
        "p": {"a": 0, "k": [0, 0]},
        "a": {"a": 0, "k": [0, 0]},
        "s": {"a": 0, "k": [100, 100]},
        "r": {"a": 0, "k": 0},
        "o": {"a": 0, "k": 100},
        "sk": {"a": 0, "k": 0},
        "sa": {"a": 0, "k": 0},
        "nm": "Transform"
    })
}

pub(super) fn apply_rgb_only(ct: ColorTransform, color: Color, include_alpha: bool) -> Color {
    let mut out = ct.apply(color);
    if !include_alpha {
        out[3] = color[3];
    }
    out
}

fn extract_element_paint(element: &Element) -> (Color, Color, f64) {
    match &element.kind {
        ElementKind::Rectangle {
            fill,
            stroke,
            stroke_width,
            ..
        } => (*fill, *stroke, *stroke_width),
        ElementKind::Ellipse {
            fill,
            stroke,
            stroke_width,
            ..
        } => {
            let fill = if element.oval_options().close_path {
                *fill
            } else {
                [0, 0, 0, 0]
            };
            (fill, *stroke, *stroke_width)
        }
        ElementKind::Path {
            fill,
            stroke,
            stroke_width,
            ..
        }
        | ElementKind::PaintedPath {
            fill,
            stroke,
            stroke_width,
            ..
        } => (*fill, *stroke, *stroke_width),
        _ => ([0; 4], [0; 4], 0.0),
    }
}

fn tint_gradient_paint(
    grad: &GradientPaint,
    ct: ColorTransform,
    include_alpha: bool,
) -> GradientPaint {
    let mut out = grad.clone();
    for stop in &mut out.stops {
        stop.color = apply_rgb_only(ct, stop.color, include_alpha);
    }
    out
}

fn transform_gradient_paint(
    grad: &GradientPaint,
    m: [f64; 6],
    ct: ColorTransform,
    include_alpha: bool,
) -> GradientPaint {
    let mut out = tint_gradient_paint(grad, ct, include_alpha);
    out.matrix = neoflash_core::affine_multiply(m, grad.matrix);
    out
}

fn solid_as_matching_gradient(color: Color, template: &GradientPaint) -> GradientPaint {
    let mut out = template.clone();
    for stop in &mut out.stops {
        stop.color = color;
    }
    out
}

fn gradient_endpoints(grad: &GradientPaint) -> ([f64; 2], [f64; 2], u8) {
    let [a, b, _c, _d, tx, ty] = grad.matrix;
    match grad.kind {
        GradientKind::Linear => (
            [round4(tx - 819.2 * a), round4(ty - 819.2 * b)],
            [round4(tx + 819.2 * a), round4(ty + 819.2 * b)],
            1,
        ),
        GradientKind::Radial => (
            [round4(tx), round4(ty)],
            [round4(tx + 819.2 * a), round4(ty + 819.2 * b)],
            2,
        ),
    }
}

fn gradient_radial_highlight(grad: &GradientPaint) -> Option<(f64, f64)> {
    if grad.kind != GradientKind::Radial
        || !grad.focal_point_ratio.is_finite()
        || grad.focal_point_ratio.abs() <= 1e-4
    {
        return None;
    }
    let r = grad.focal_point_ratio.clamp(-0.999, 0.999);
    let h = round4(r.abs() * 100.0);
    let a = if r < 0.0 { 180.0 } else { 0.0 };
    Some((h, a))
}

fn normalized_lottie_stops(
    grad: &GradientPaint,
) -> std::borrow::Cow<'_, [neoflash_core::GradientStop]> {
    if grad.stops.is_empty() {
        std::borrow::Cow::Owned(vec![
            neoflash_core::GradientStop {
                ratio: 0,
                color: [0, 0, 0, 0],
            },
            neoflash_core::GradientStop {
                ratio: 255,
                color: [0, 0, 0, 0],
            },
        ])
    } else if grad.stops.len() == 1 {
        let c = grad.stops[0].color;
        std::borrow::Cow::Owned(vec![
            neoflash_core::GradientStop { ratio: 0, color: c },
            neoflash_core::GradientStop {
                ratio: 255,
                color: c,
            },
        ])
    } else {
        std::borrow::Cow::Borrowed(&grad.stops)
    }
}

fn gradient_stops_array_with_opacity(grad: &GradientPaint, include_opacity: bool) -> Vec<f64> {
    let stops = normalized_lottie_stops(grad);
    let mut arr = Vec::with_capacity(stops.len() * 6);
    for stop in stops.iter() {
        arr.push(round4(f64::from(stop.ratio) / 255.0));
        arr.push(round4(f64::from(stop.color[0]) / 255.0));
        arr.push(round4(f64::from(stop.color[1]) / 255.0));
        arr.push(round4(f64::from(stop.color[2]) / 255.0));
    }
    if include_opacity {
        for stop in stops.iter() {
            arr.push(round4(f64::from(stop.ratio) / 255.0));
            arr.push(round4(f64::from(stop.color[3]) / 255.0));
        }
    }
    arr
}

fn gradient_stops_array(grad: &GradientPaint) -> Vec<f64> {
    let stops = normalized_lottie_stops(grad);
    let has_alpha = stops.iter().any(|s| s.color[3] < 255);
    gradient_stops_array_with_opacity(grad, has_alpha)
}

fn gradient_fill_item(grad: &GradientPaint, fill_rule: FillRule) -> Value {
    let (s, e, t) = gradient_endpoints(grad);
    let p_count = normalized_lottie_stops(grad).len();
    let k = gradient_stops_array(grad);
    let mut obj = json!({
        "ty": "gf",
        "nm": "Gradient Fill",
        "o": {"a": 0, "k": 100},
        "r": if fill_rule == FillRule::EvenOdd { 2 } else { 1 },
        "bm": 0,
        "g": {
            "p": p_count,
            "k": {"a": 0, "k": k}
        },
        "s": {"a": 0, "k": s},
        "e": {"a": 0, "k": e},
        "t": t
    });
    if let Some((h, a)) = gradient_radial_highlight(grad)
        && let Some(map) = obj.as_object_mut()
    {
        map.insert("h".into(), json!({"a": 0, "k": h}));
        map.insert("a".into(), json!({"a": 0, "k": a}));
    }
    obj
}

fn gradient_stroke_item(grad: &GradientPaint, stroke_width: f64) -> Value {
    let (s, e, t) = gradient_endpoints(grad);
    let p_count = normalized_lottie_stops(grad).len();
    let k = gradient_stops_array(grad);
    let mut obj = json!({
        "ty": "gs",
        "nm": "Gradient Stroke",
        "o": {"a": 0, "k": 100},
        "w": {"a": 0, "k": round4(stroke_width)},
        "lc": 2,
        "lj": 2,
        "ml": 4,
        "bm": 0,
        "g": {
            "p": p_count,
            "k": {"a": 0, "k": k}
        },
        "s": {"a": 0, "k": s},
        "e": {"a": 0, "k": e},
        "t": t
    });
    if let Some((h, a)) = gradient_radial_highlight(grad)
        && let Some(map) = obj.as_object_mut()
    {
        map.insert("h".into(), json!({"a": 0, "k": h}));
        map.insert("a".into(), json!({"a": 0, "k": a}));
    }
    obj
}

fn build_linear_vec_prop<const N: usize>(samples: &[(u32, [f64; N])]) -> Value {
    let first = samples[0].1;
    if samples.iter().all(|(_, v)| *v == first) {
        return json!({"a": 0, "k": first.as_slice()});
    }
    let kfs: Vec<Value> = samples
        .iter()
        .enumerate()
        .map(|(idx, (f, v))| {
            if idx + 1 < samples.len() {
                json!({"t": *f, "s": v.as_slice(), "i": {"x": [1.0], "y": [1.0]}, "o": {"x": [0.0], "y": [0.0]}})
            } else {
                json!({"t": *f, "s": v.as_slice()})
            }
        })
        .collect();
    json!({"a": 1, "k": kfs})
}

fn build_linear_f64_slice_prop(samples: &[(u32, Vec<f64>)]) -> Value {
    let first = &samples[0].1;
    if samples.iter().all(|(_, v)| v == first) {
        return json!({"a": 0, "k": first});
    }
    let kfs: Vec<Value> = samples
        .iter()
        .enumerate()
        .map(|(idx, (f, v))| {
            if idx + 1 < samples.len() {
                json!({"t": *f, "s": v, "i": {"x": [1.0], "y": [1.0]}, "o": {"x": [0.0], "y": [0.0]}})
            } else {
                json!({"t": *f, "s": v})
            }
        })
        .collect();
    json!({"a": 1, "k": kfs})
}

fn animated_gradient_common(
    grads: &[(u32, GradientPaint)],
) -> (usize, u8, Value, Value, Value, Option<(Value, Value)>) {
    let p_count = normalized_lottie_stops(&grads[0].1).len();
    let (_, _, t_code) = gradient_endpoints(&grads[0].1);
    let include_opacity = grads
        .iter()
        .any(|(_, g)| normalized_lottie_stops(g).iter().any(|s| s.color[3] < 255));
    let k_samples: Vec<(u32, Vec<f64>)> = grads
        .iter()
        .map(|(f, g)| (*f, gradient_stops_array_with_opacity(g, include_opacity)))
        .collect();
    let s_samples: Vec<(u32, [f64; 2])> = grads
        .iter()
        .map(|(f, g)| (*f, gradient_endpoints(g).0))
        .collect();
    let e_samples: Vec<(u32, [f64; 2])> = grads
        .iter()
        .map(|(f, g)| (*f, gradient_endpoints(g).1))
        .collect();
    let highlight = if t_code == 2
        && grads
            .iter()
            .any(|(_, g)| g.focal_point_ratio.is_finite() && g.focal_point_ratio.abs() > 1e-4)
    {
        let h_samples: Vec<(u32, [f64; 1])> = grads
            .iter()
            .map(|(f, g)| {
                let r = if g.focal_point_ratio.is_finite() {
                    g.focal_point_ratio.clamp(-0.999, 0.999)
                } else {
                    0.0
                };
                (*f, [round4(r.abs() * 100.0)])
            })
            .collect();
        let a_samples: Vec<(u32, [f64; 1])> = grads
            .iter()
            .map(|(f, g)| {
                let neg = g.focal_point_ratio.is_finite() && g.focal_point_ratio < -1e-4;
                (*f, [if neg { 180.0 } else { 0.0 }])
            })
            .collect();
        let h_val = if h_samples.iter().all(|(_, v)| v[0] == h_samples[0].1[0]) {
            json!({"a": 0, "k": h_samples[0].1[0]})
        } else {
            build_linear_vec_prop(&h_samples)
        };
        let a_val = if a_samples.iter().all(|(_, v)| v[0] == a_samples[0].1[0]) {
            json!({"a": 0, "k": a_samples[0].1[0]})
        } else {
            build_linear_vec_prop(&a_samples)
        };
        Some((h_val, a_val))
    } else {
        None
    };
    (
        p_count,
        t_code,
        build_linear_f64_slice_prop(&k_samples),
        build_linear_vec_prop(&s_samples),
        build_linear_vec_prop(&e_samples),
        highlight,
    )
}

fn animated_gradient_fill_item(grads: &[(u32, GradientPaint)], fill_rule: FillRule) -> Value {
    let (p_count, t_code, k_val, s_val, e_val, highlight) = animated_gradient_common(grads);
    let mut obj = json!({
        "ty": "gf",
        "nm": "Gradient Fill",
        "o": {"a": 0, "k": 100},
        "r": if fill_rule == FillRule::EvenOdd { 2 } else { 1 },
        "bm": 0,
        "g": {
            "p": p_count,
            "k": k_val
        },
        "s": s_val,
        "e": e_val,
        "t": t_code
    });
    if let Some((h_val, a_val)) = highlight
        && let Some(map) = obj.as_object_mut()
    {
        map.insert("h".into(), h_val);
        map.insert("a".into(), a_val);
    }
    obj
}

fn animated_gradient_stroke_item(grads: &[(u32, GradientPaint)], widths: &[(u32, f64)]) -> Value {
    let (p_count, t_code, k_val, s_val, e_val, highlight) = animated_gradient_common(grads);
    let w_samples: Vec<(u32, [f64; 1])> = widths.iter().map(|(f, w)| (*f, [round4(*w)])).collect();
    let w_val = if w_samples
        .iter()
        .all(|(_, v)| (v[0] - w_samples[0].1[0]).abs() < 1e-4)
    {
        json!({"a": 0, "k": w_samples[0].1[0]})
    } else {
        build_linear_vec_prop(&w_samples)
    };
    let mut obj = json!({
        "ty": "gs",
        "nm": "Gradient Stroke",
        "o": {"a": 0, "k": 100},
        "w": w_val,
        "lc": 2,
        "lj": 2,
        "ml": 4,
        "bm": 0,
        "g": {
            "p": p_count,
            "k": k_val
        },
        "s": s_val,
        "e": e_val,
        "t": t_code
    });
    if let Some((h_val, a_val)) = highlight
        && let Some(map) = obj.as_object_mut()
    {
        map.insert("h".into(), h_val);
        map.insert("a".into(), a_val);
    }
    obj
}

#[derive(Clone, Debug)]
pub(super) struct LottieBezier {
    pub(super) c: bool,
    pub(super) v: Vec<[f64; 2]>,
    pub(super) i: Vec<[f64; 2]>,
    pub(super) o: Vec<[f64; 2]>,
}

impl LottieBezier {
    pub(super) fn to_json(&self) -> Value {
        json!({
            "c": self.c,
            "v": self.v.iter().map(|p| [round4(p[0]), round4(p[1])]).collect::<Vec<_>>(),
            "i": self.i.iter().map(|p| [round4(p[0]), round4(p[1])]).collect::<Vec<_>>(),
            "o": self.o.iter().map(|p| [round4(p[0]), round4(p[1])]).collect::<Vec<_>>()
        })
    }

    /// Screen-space (Y-down) contour winding direction for Bodymovin `"ty": "sh"` (`1` = CW, `3` = CCW).
    pub(super) fn lottie_direction(&self) -> u8 {
        if !self.c || self.v.len() < 2 {
            return 1;
        }
        let n = self.v.len();
        let mut pts = Vec::with_capacity(n * 4);
        for idx in 0..n {
            let next = (idx + 1) % n;
            let p0 = self.v[idx];
            let p1 = [p0[0] + self.o[idx][0], p0[1] + self.o[idx][1]];
            let p3 = self.v[next];
            let p2 = [p3[0] + self.i[next][0], p3[1] + self.i[next][1]];
            for step in 0..4 {
                let u = f64::from(step) / 4.0;
                let mu = 1.0 - u;
                let x = mu * mu * mu * p0[0]
                    + 3.0 * mu * mu * u * p1[0]
                    + 3.0 * mu * u * u * p2[0]
                    + u * u * u * p3[0];
                let y = mu * mu * mu * p0[1]
                    + 3.0 * mu * mu * u * p1[1]
                    + 3.0 * mu * u * u * p2[1]
                    + u * u * u * p3[1];
                pts.push([x, y]);
            }
        }
        let mut area2 = 0.0;
        for idx in 0..pts.len() {
            let a = pts[idx];
            let b = pts[(idx + 1) % pts.len()];
            area2 += a[0] * b[1] - b[0] * a[1];
        }
        if area2 < -1e-6 { 3 } else { 1 }
    }
}

pub(super) fn path_commands_to_lottie_beziers(commands: &[PathCommand]) -> Vec<LottieBezier> {
    let mut contours = Vec::new();
    let mut current: Option<LottieBezier> = None;

    let flush = |current: &mut Option<LottieBezier>, contours: &mut Vec<LottieBezier>| {
        if let Some(mut bez) = current.take() {
            if bez.c && bez.v.len() > 1 {
                let first = bez.v[0];
                let last = *bez.v.last().unwrap();
                if (first[0] - last[0]).hypot(first[1] - last[1]) < 1e-4 {
                    let last_in = bez.i.pop().unwrap_or([0.0, 0.0]);
                    bez.v.pop();
                    bez.o.pop();
                    bez.i[0] = last_in;
                }
            }
            if bez.v.len() >= 2 {
                contours.push(bez);
            }
        }
    };

    for cmd in commands {
        match *cmd {
            PathCommand::MoveTo { x, y } => {
                flush(&mut current, &mut contours);
                current = Some(LottieBezier {
                    c: false,
                    v: vec![[x, y]],
                    i: vec![[0.0, 0.0]],
                    o: vec![[0.0, 0.0]],
                });
            }
            PathCommand::LineTo { x, y } => {
                if let Some(bez) = current.as_mut() {
                    bez.v.push([x, y]);
                    bez.i.push([0.0, 0.0]);
                    bez.o.push([0.0, 0.0]);
                }
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                if let Some(bez) = current.as_mut()
                    && let Some(&[x0, y0]) = bez.v.last()
                {
                    // Exact degree elevation from quadratic to cubic Bézier:
                    // cp1 = p0 + 2/3 * (q - p0), cp2 = p2 + 2/3 * (q - p2)
                    if let Some(last_o) = bez.o.last_mut() {
                        *last_o = [(2.0 / 3.0) * (cx - x0), (2.0 / 3.0) * (cy - y0)];
                    }
                    let in_tan = [(2.0 / 3.0) * (cx - x), (2.0 / 3.0) * (cy - y)];
                    bez.v.push([x, y]);
                    bez.i.push(in_tan);
                    bez.o.push([0.0, 0.0]);
                }
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                if let Some(bez) = current.as_mut()
                    && let Some(&[x0, y0]) = bez.v.last()
                {
                    if let Some(last_o) = bez.o.last_mut() {
                        *last_o = [cx1 - x0, cy1 - y0];
                    }
                    let in_tan = [cx2 - x, cy2 - y];
                    bez.v.push([x, y]);
                    bez.i.push(in_tan);
                    bez.o.push([0.0, 0.0]);
                }
            }
            PathCommand::Close => {
                if let Some(bez) = current.as_mut() {
                    bez.c = true;
                }
                flush(&mut current, &mut contours);
            }
        }
    }
    flush(&mut current, &mut contours);
    contours
}

fn transform_path_commands(commands: &[PathCommand], m: [f64; 6]) -> Vec<PathCommand> {
    let pt =
        |x: f64, y: f64| -> (f64, f64) { (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]) };
    commands
        .iter()
        .map(|cmd| match *cmd {
            PathCommand::MoveTo { x, y } => {
                let (x, y) = pt(x, y);
                PathCommand::MoveTo { x, y }
            }
            PathCommand::LineTo { x, y } => {
                let (x, y) = pt(x, y);
                PathCommand::LineTo { x, y }
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                let (cx, cy) = pt(cx, cy);
                let (x, y) = pt(x, y);
                PathCommand::QuadraticTo { cx, cy, x, y }
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                let (cx1, cy1) = pt(cx1, cy1);
                let (cx2, cy2) = pt(cx2, cy2);
                let (x, y) = pt(x, y);
                PathCommand::CubicTo {
                    cx1,
                    cy1,
                    cx2,
                    cy2,
                    x,
                    y,
                }
            }
            PathCommand::Close => PathCommand::Close,
        })
        .collect()
}
