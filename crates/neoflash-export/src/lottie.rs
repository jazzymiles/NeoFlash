//! Bodymovin / Lottie (`v: "5.7.14"`) JSON animation exporter.
//!
//! Converts NeoFlash authoring timelines, classic/motion/shape tweens, ease curves, motion guides,
//! nested symbols (`Graphic`, `MovieClip`, `Button`), timeline-synchronized layer crops (`T-038`),
//! instance `First frame` offsets, layer/element blend modes (`T-036`), masks, gradients, bitmaps,
//! and text into a self-contained Lottie JSON document.
//!
//! Features that Lottie players cannot execute (such as ActionScript / JavaScript code, timeline
//! audio, video viewports, or Flash-only filter/blend modes) produce actionable diagnostics in
//! [`LottieExportReport::warnings`] instead of failing or silently dropping artwork.

use crate::ExportError;
use neoflash_core::{
    BlendMode, Color, ColorTransform, Easing, Element, ElementKind, FillRule, GradientKind,
    GradientPaint, InstancePlayback, Layer, LayerRole, PathCommand, Project, Symbol, Timeline,
    Transform, TweenKind, TweenProperty, effective_property_ease, flatten_scenes_with_ranges,
    layer_effective_blend_mode, layer_effectively_synced, layer_effectively_visible, motion_span,
    placement_start, sample_layer, sample_layer_in,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

/// Bodymovin schema version targeted by the exporter.
pub const LOTTIE_VERSION: &str = "5.7.14";

/// Options controlling Lottie JSON generation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LottieOptions {
    /// Export a specific symbol by ID or name instead of the flattened root scene timeline.
    pub symbol: Option<String>,
    /// Emit a bottom solid layer (`ty: 1`) matching `project.background` when its alpha is > 0.
    pub include_background: bool,
    /// Include layers whose authoring visibility is turned off.
    pub include_hidden_layers: bool,
    /// Format the JSON output with indentation (`serde_json::to_string_pretty`).
    pub pretty: bool,
}

impl Default for LottieOptions {
    fn default() -> Self {
        Self {
            symbol: None,
            include_background: true,
            include_hidden_layers: false,
            pretty: false,
        }
    }
}

/// Summary and diagnostics produced by a Lottie export.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LottieExportReport {
    pub path: Option<PathBuf>,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub frame_count: u32,
    pub layer_count: usize,
    pub asset_count: usize,
    pub bytes: usize,
    pub warnings: Vec<String>,
}

struct LottieBuilder<'a> {
    project: &'a Project,
    options: &'a LottieOptions,
    fps: f64,
    comp_w: u32,
    comp_h: u32,
    assets: Vec<Value>,
    exported_precomps: BTreeSet<String>,
    exported_images: BTreeSet<String>,
    warnings: Vec<String>,
    warning_set: BTreeSet<String>,
}

impl<'a> LottieBuilder<'a> {
    fn warn(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        if self.warning_set.insert(msg.clone()) {
            self.warnings.push(msg);
        }
    }

    fn map_blend_mode(&mut self, mode: BlendMode, context: &str) -> u32 {
        match mode {
            BlendMode::Normal | BlendMode::Layer => 0,
            BlendMode::Multiply => 1,
            BlendMode::Screen => 2,
            BlendMode::Overlay => 3,
            BlendMode::Darken => 4,
            BlendMode::Lighten => 5,
            BlendMode::Hardlight => 8,
            BlendMode::Difference => 10,
            BlendMode::Add => 12,
            BlendMode::Subtract | BlendMode::Invert | BlendMode::Alpha | BlendMode::Erase => {
                self.warn(format!(
                    "{context}: blend mode '{}' is not supported by Lottie v{LOTTIE_VERSION}; exported as 'normal'.",
                    mode.as_str()
                ));
                0
            }
        }
    }

    fn ensure_image_asset(
        &mut self,
        resource_id: &str,
        fallback_w: u32,
        fallback_h: u32,
    ) -> String {
        let asset_id = format!("img_{}", sanitize_id(resource_id));
        if !self.exported_images.insert(resource_id.to_owned()) {
            return asset_id;
        }
        if let Some(unit) = self.project.resources.iter().find(|r| r.id == resource_id) {
            let (w, h) = unit
                .decode_image()
                .map(|img| (img.width, img.height))
                .unwrap_or((fallback_w.max(1), fallback_h.max(1)));
            let data_uri = format!("data:{};base64,{}", unit.media_type, unit.data_base64);
            self.assets.push(json!({
                "id": asset_id,
                "w": w,
                "h": h,
                "u": "",
                "p": data_uri,
                "e": 1
            }));
        } else {
            self.warn(format!(
                "Bitmap resource '{resource_id}' is missing from project resources."
            ));
        }
        asset_id
    }

    fn ensure_symbol_precomp(
        &mut self,
        symbol_id: &str,
        stack: &mut Vec<String>,
    ) -> Option<String> {
        let asset_id = format!("comp_{}", sanitize_id(symbol_id));
        if self.exported_precomps.contains(symbol_id) {
            return Some(asset_id);
        }
        if stack.iter().any(|s| s == symbol_id) {
            return None;
        }
        let symbol = self.project.symbol(symbol_id)?;
        self.exported_precomps.insert(symbol_id.to_owned());
        stack.push(symbol_id.to_owned());

        let precomp_w = self.comp_w.max(1) * 2;
        let precomp_h = self.comp_h.max(1) * 2;
        let origin = [self.comp_w as f64, self.comp_h as f64];
        let layers =
            self.build_timeline_layers(&symbol.name, &symbol.timeline, origin, false, stack);
        stack.pop();

        self.assets.push(json!({
            "id": asset_id,
            "nm": symbol.name,
            "fr": round4(self.fps),
            "w": precomp_w,
            "h": precomp_h,
            "layers": layers
        }));
        Some(asset_id)
    }

    fn build_timeline_layers(
        &mut self,
        symbol_name: &str,
        timeline: &Timeline,
        origin_offset: [f64; 2],
        include_bg: bool,
        stack: &mut Vec<String>,
    ) -> Vec<Value> {
        if !timeline.scripts.is_empty() {
            self.warn(format!(
                "Timeline '{symbol_name}': {} frame script(s) are not executed by Lottie players; timeline animation was exported.",
                timeline.scripts.len()
            ));
        }

        let total_frames = timeline.frame_count.max(1);
        let mut lottie_layers = Vec::new();
        let mut next_ind: u32 = 1;

        // Collect mask layers by ID so masked layers can attach `masksProperties`.
        let mut mask_layers: BTreeMap<&str, &Layer> = BTreeMap::new();
        for layer in &timeline.layers {
            if layer.properties.role == LayerRole::Mask {
                mask_layers.insert(layer.id.as_str(), layer);
            }
        }

        // In NeoFlash, `timeline.layers[0]` is the bottom layer and `layers.last()` is the top
        // layer. In Lottie, `layers[0]` is rendered on top, so iterate in reverse order.
        for layer in timeline.layers.iter().rev() {
            for kf in &layer.keyframes {
                if kf.sound.is_some() {
                    self.warn(format!(
                        "Layer '{}' (frame {}): timeline sound is not supported in Lottie v{LOTTIE_VERSION} export.",
                        layer.name,
                        kf.frame + 1
                    ));
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
            );
            let synced = layer_effectively_synced(timeline, layer);
            let mask_layer = layer
                .properties
                .mask
                .as_deref()
                .and_then(|id| mask_layers.get(id).copied());

            let spans = collect_layer_spans(layer, layer_end);
            for span in spans.into_iter().rev() {
                let emitted = self.emit_span_layers(
                    timeline,
                    layer,
                    &span,
                    total_frames,
                    layer_bm,
                    synced,
                    mask_layer,
                    origin_offset,
                    stack,
                    &mut next_ind,
                );
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

        lottie_layers
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_span_layers(
        &mut self,
        timeline: &Timeline,
        layer: &Layer,
        span: &LayerSpan,
        total_frames: u32,
        layer_bm: u32,
        layer_synced: bool,
        mask_layer: Option<&Layer>,
        origin_offset: [f64; 2],
        stack: &mut Vec<String>,
        next_ind: &mut u32,
    ) -> Vec<Value> {
        let first_elements = sample_layer_in(timeline, layer, span.start_frame);
        if first_elements.is_empty() {
            return Vec::new();
        }

        // Check if this contiguous non-empty span tracks a stable set of element IDs or is a
        // shape tween so we can emit clean per-track or morphed Lottie layers.
        let has_shape_tween = layer.keyframes.iter().any(|k| {
            k.frame >= span.start_frame
                && k.frame < span.end_frame
                && k.tween.is_some()
                && k.tween_kind == TweenKind::Shape
        });

        if has_shape_tween {
            return self.emit_sampled_shape_span(
                timeline,
                layer,
                span,
                layer_bm,
                mask_layer,
                origin_offset,
                next_ind,
            );
        }

        // Group elements by their ID across the span, rendered top-to-bottom (reverse of
        // `elements` slice order, since `elements.last()` is on top in NeoFlash).
        let mut tracks: Vec<String> = Vec::new();
        for f in span.start_frame..span.end_frame {
            for el in sample_layer_in(timeline, layer, f) {
                if !tracks.contains(&el.id) {
                    tracks.push(el.id.clone());
                }
            }
        }

        let mut out = Vec::new();
        for element_id in tracks.into_iter().rev() {
            if let Some(val) = self.emit_element_track(
                timeline,
                layer,
                span,
                &element_id,
                total_frames,
                layer_bm,
                layer_synced,
                mask_layer,
                origin_offset,
                stack,
                next_ind,
            ) {
                out.push(val);
            }
        }
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_sampled_shape_span(
        &mut self,
        timeline: &Timeline,
        layer: &Layer,
        span: &LayerSpan,
        layer_bm: u32,
        mask_layer: Option<&Layer>,
        origin_offset: [f64; 2],
        next_ind: &mut u32,
    ) -> Vec<Value> {
        // Check if every frame in the span has a single morphing shape with identical contour
        // count and vertex count per contour; if so, emit a single animated Lottie shape layer!
        let mut frame_shapes: Vec<(u32, Element)> = Vec::new();
        let mut single_track = true;
        for f in span.start_frame..span.end_frame {
            let mut els = sample_layer_in(timeline, layer, f);
            if els.len() == 1 && els[0].to_path_commands().is_some() {
                frame_shapes.push((f, els.pop().unwrap()));
            } else {
                single_track = false;
                break;
            }
        }

        if single_track
            && !frame_shapes.is_empty()
            && let Some(layer_val) = self.try_build_morphed_shape_layer(
                timeline,
                layer,
                span,
                &frame_shapes,
                layer_bm,
                mask_layer,
                origin_offset,
                next_ind,
            )
        {
            return vec![layer_val];
        }

        // Fallback: emit 1-frame slices for arbitrary multi-shape topology changes.
        let mut out = Vec::new();
        for f in (span.start_frame..span.end_frame).rev() {
            let els = sample_layer_in(timeline, layer, f);
            for el in els.into_iter().rev() {
                if let Some(shape_group) = self.element_to_shape_group(&el, [0.0, 0.0], true) {
                    let ind = *next_ind;
                    *next_ind += 1;
                    let bm = if el.blend_mode.is_normal() {
                        layer_bm
                    } else {
                        self.map_blend_mode(el.blend_mode, &format!("Element '{}'", el.id))
                    };
                    let mut obj = json!({
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
                    self.attach_masks(&mut obj, timeline, mask_layer, f, f + 1, el.transform);
                    out.push(obj);
                }
            }
        }
        out
    }

    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    fn try_build_morphed_shape_layer(
        &mut self,
        timeline: &Timeline,
        layer: &Layer,
        span: &LayerSpan,
        frame_shapes: &[(u32, Element)],
        layer_bm: u32,
        mask_layer: Option<&Layer>,
        origin_offset: [f64; 2],
        next_ind: &mut u32,
    ) -> Option<Value> {
        // Transform every frame's path commands into layer space (with `origin_offset`) so
        // morphing vertices, translations, and paint changes are all unified smoothly.
        let mut contours_per_frame: Vec<(u32, Vec<LottieBezier>, [u8; 4], [u8; 4], f64, FillRule)> =
            Vec::with_capacity(frame_shapes.len());
        for (f, el) in frame_shapes {
            let (cmds, fill_rule) = el.to_path_commands()?;
            let m = neoflash_core::transform_matrix(Transform {
                x: el.transform.x + origin_offset[0],
                y: el.transform.y + origin_offset[1],
                ..el.transform
            });
            let tx_cmds = transform_path_commands(&cmds, m);
            let contours = path_commands_to_lottie_beziers(&tx_cmds);
            if contours.is_empty() {
                return None;
            }
            let (fill, stroke, stroke_width) = extract_element_paint(el);
            contours_per_frame.push((*f, contours, fill, stroke, stroke_width, fill_rule));
        }

        let contour_count = contours_per_frame[0].1.len();
        let vertex_counts: Vec<usize> = contours_per_frame[0].1.iter().map(|c| c.v.len()).collect();
        if contours_per_frame.iter().any(|(_, cs, _, _, _, _)| {
            cs.len() != contour_count
                || cs
                    .iter()
                    .zip(&vertex_counts)
                    .any(|(c, &expected)| c.v.len() != expected)
        }) {
            return None;
        }

        let mut items: Vec<Value> = Vec::new();
        for c_idx in 0..contour_count {
            let mut kfs = Vec::new();
            for (idx, (f, cs, _, _, _, _)) in contours_per_frame.iter().enumerate() {
                let bez = cs[c_idx].to_json();
                if idx + 1 < contours_per_frame.len() {
                    kfs.push(json!({
                        "t": *f,
                        "s": [bez],
                        "i": {"x": [1.0], "y": [1.0]},
                        "o": {"x": [0.0], "y": [0.0]}
                    }));
                } else {
                    kfs.push(json!({
                        "t": *f,
                        "s": [bez]
                    }));
                }
            }
            items.push(json!({
                "ty": "sh",
                "d": 1,
                "nm": format!("Path {}", c_idx + 1),
                "ks": {
                    "a": if kfs.len() > 1 { 1 } else { 0 },
                    "k": if kfs.len() > 1 { Value::Array(kfs) } else { contours_per_frame[0].1[c_idx].to_json() }
                }
            }));
        }

        let (_, _, fill0, stroke0, sw0, fill_rule0) = &contours_per_frame[0];
        if fill0[3] > 0 {
            let [r, g, b, a] = *fill0;
            items.push(json!({
                "ty": "fl",
                "nm": "Fill",
                "c": {"a": 0, "k": [round4(f64::from(r) / 255.0), round4(f64::from(g) / 255.0), round4(f64::from(b) / 255.0), 1.0]},
                "o": {"a": 0, "k": round4(f64::from(a) / 255.0 * 100.0)},
                "r": if *fill_rule0 == FillRule::EvenOdd { 2 } else { 1 },
                "bm": 0
            }));
        }
        if stroke0[3] > 0 && *sw0 > 0.0 {
            let [r, g, b, a] = *stroke0;
            items.push(json!({
                "ty": "st",
                "nm": "Stroke",
                "c": {"a": 0, "k": [round4(f64::from(r) / 255.0), round4(f64::from(g) / 255.0), round4(f64::from(b) / 255.0), 1.0]},
                "o": {"a": 0, "k": round4(f64::from(a) / 255.0 * 100.0)},
                "w": {"a": 0, "k": round4(*sw0)},
                "lc": 2,
                "lj": 2,
                "bm": 0
            }));
        }
        items.push(identity_group_tr());

        let ind = *next_ind;
        *next_ind += 1;
        let first_el = &frame_shapes[0].1;
        let mut obj = json!({
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
        self.attach_masks(
            &mut obj,
            timeline,
            mask_layer,
            span.start_frame,
            span.end_frame,
            Transform::default(),
        );
        Some(obj)
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_element_track(
        &mut self,
        timeline: &Timeline,
        layer: &Layer,
        span: &LayerSpan,
        element_id: &str,
        _total_frames: u32,
        layer_bm: u32,
        layer_synced: bool,
        mask_layer: Option<&Layer>,
        origin_offset: [f64; 2],
        stack: &mut Vec<String>,
        next_ind: &mut u32,
    ) -> Option<Value> {
        let mut samples: Vec<(u32, Element)> = Vec::new();
        for f in span.start_frame..span.end_frame {
            if let Some(el) = sample_layer_in(timeline, layer, f)
                .into_iter()
                .find(|e| e.id == element_id && e.visible)
            {
                samples.push((f, el));
            }
        }
        if samples.is_empty() {
            return None;
        }
        let first_frame = samples.first().unwrap().0;
        let last_frame = samples.last().unwrap().0 + 1;
        let representative = &samples[0].1;

        if !representative.scripts.is_empty() {
            self.warn(format!(
                "Layer '{}' element '{}': object script(s) are not executed by Lottie players.",
                layer.name,
                layer_element_name(layer, representative)
            ));
        }
        if !representative.filters.is_empty() {
            self.warn(format!(
                "Layer '{}' element '{}': display filter(s) are not exported to Lottie v{LOTTIE_VERSION}.",
                layer.name,
                layer_element_name(layer, representative)
            ));
        }

        let bm = if representative.blend_mode.is_normal() {
            layer_bm
        } else {
            self.map_blend_mode(
                representative.blend_mode,
                &format!("Layer '{}' element '{}'", layer.name, representative.id),
            )
        };

        let orient_auto = layer
            .keyframes
            .iter()
            .any(|k| k.frame >= first_frame && k.frame < last_frame && k.orient_to_path);

        match &representative.kind {
            ElementKind::Instance {
                symbol_id,
                playback,
            } => {
                let ref_id = self.ensure_symbol_precomp(symbol_id, stack)?;
                let precomp_anchor = [self.comp_w as f64, self.comp_h as f64];
                let ks = self.build_animated_ks(
                    timeline,
                    layer,
                    &samples,
                    precomp_anchor,
                    origin_offset,
                );
                let ind = *next_ind;
                *next_ind += 1;
                let mut obj = json!({
                    "ddd": 0,
                    "ind": ind,
                    "ty": 0,
                    "nm": layer_element_name(layer, representative),
                    "refId": ref_id,
                    "sr": 1,
                    "ks": ks,
                    "ao": if orient_auto { 1 } else { 0 },
                    "w": self.comp_w.max(1) * 2,
                    "h": self.comp_h.max(1) * 2,
                    "ip": first_frame,
                    "op": last_frame,
                    "st": first_frame,
                    "bm": bm
                });
                if let Some(tm) = self.build_instance_time_remap(
                    timeline,
                    layer,
                    symbol_id,
                    *playback,
                    layer_synced,
                    first_frame,
                    last_frame,
                ) {
                    obj.as_object_mut().unwrap().insert("tm".into(), tm);
                }
                self.attach_masks(
                    &mut obj,
                    timeline,
                    mask_layer,
                    first_frame,
                    last_frame,
                    representative.transform,
                );
                Some(obj)
            }
            ElementKind::Bitmap {
                resource_id,
                width,
                height,
                ..
            } => {
                let ref_id = self.ensure_image_asset(resource_id, *width, *height);
                let ks =
                    self.build_animated_ks(timeline, layer, &samples, [0.0, 0.0], origin_offset);
                let ind = *next_ind;
                *next_ind += 1;
                let mut obj = json!({
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
                self.attach_masks(
                    &mut obj,
                    timeline,
                    mask_layer,
                    first_frame,
                    last_frame,
                    representative.transform,
                );
                Some(obj)
            }
            ElementKind::Text {
                text,
                font_family,
                font_size,
                color,
            } => {
                let ks =
                    self.build_animated_ks(timeline, layer, &samples, [0.0, 0.0], origin_offset);
                let ind = *next_ind;
                *next_ind += 1;
                let [r, g, b, _] = representative.transform.color_transform.apply(*color);
                let mut obj = json!({
                    "ddd": 0,
                    "ind": ind,
                    "ty": 5,
                    "nm": layer_element_name(layer, representative),
                    "sr": 1,
                    "ks": ks,
                    "ao": 0,
                    "t": {
                        "d": {
                            "k": [{
                                "t": 0,
                                "s": {
                                    "s": round4(*font_size),
                                    "f": font_family,
                                    "t": text.replace('\n', "\r"),
                                    "j": 0,
                                    "tr": 0,
                                    "lh": round4(*font_size * 1.2),
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
                self.attach_masks(
                    &mut obj,
                    timeline,
                    mask_layer,
                    first_frame,
                    last_frame,
                    representative.transform,
                );
                Some(obj)
            }
            ElementKind::TextBox {
                text,
                width,
                height,
                format,
                ..
            } => {
                let ks =
                    self.build_animated_ks(timeline, layer, &samples, [0.0, 0.0], origin_offset);
                let ind = *next_ind;
                *next_ind += 1;
                let fmt_rgba = [
                    ((format.color >> 16) & 0xFF) as u8,
                    ((format.color >> 8) & 0xFF) as u8,
                    (format.color & 0xFF) as u8,
                    255,
                ];
                let [r, g, b, _] = representative.transform.color_transform.apply(fmt_rgba);
                let justify = match format.align {
                    neoflash_core::TextAlign::Left => 0,
                    neoflash_core::TextAlign::Right => 1,
                    neoflash_core::TextAlign::Center => 2,
                };
                let mut obj = json!({
                    "ddd": 0,
                    "ind": ind,
                    "ty": 5,
                    "nm": layer_element_name(layer, representative),
                    "sr": 1,
                    "ks": ks,
                    "ao": 0,
                    "t": {
                        "d": {
                            "k": [{
                                "t": 0,
                                "s": {
                                    "s": round4(format.size),
                                    "f": format.font,
                                    "t": text.replace('\n', "\r"),
                                    "j": justify,
                                    "tr": 0,
                                    "lh": round4(format.size * 1.2 + format.leading),
                                    "ls": 0,
                                    "fc": [round4(f64::from(r) / 255.0), round4(f64::from(g) / 255.0), round4(f64::from(b) / 255.0)],
                                    "sz": [round4(*width), round4(*height)],
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
                self.attach_masks(
                    &mut obj,
                    timeline,
                    mask_layer,
                    first_frame,
                    last_frame,
                    representative.transform,
                );
                Some(obj)
            }
            ElementKind::Video { .. } => {
                self.warn(format!(
                    "Layer '{}' element '{}': video elements are not supported by Lottie v{LOTTIE_VERSION}.",
                    layer.name,
                    layer_element_name(layer, representative)
                ));
                None
            }
            ElementKind::Rectangle { .. }
            | ElementKind::Ellipse { .. }
            | ElementKind::Path { .. }
            | ElementKind::PaintedPath { .. } => {
                let shape_group = self.element_to_shape_group(representative, [0.0, 0.0], false)?;
                let ks =
                    self.build_animated_ks(timeline, layer, &samples, [0.0, 0.0], origin_offset);
                let ind = *next_ind;
                *next_ind += 1;
                let mut obj = json!({
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
                self.attach_masks(
                    &mut obj,
                    timeline,
                    mask_layer,
                    first_frame,
                    last_frame,
                    representative.transform,
                );
                Some(obj)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn build_instance_time_remap(
        &self,
        timeline: &Timeline,
        layer: &Layer,
        symbol_id: &str,
        playback: InstancePlayback,
        layer_synced: bool,
        first_frame: u32,
        last_frame: u32,
    ) -> Option<Value> {
        let child_sym = self.project.symbol(symbol_id)?;
        let child_len = child_sym.timeline.frame_count.max(1);
        if child_len <= 1 {
            return None;
        }
        let span_start = placement_start(layer, first_frame, symbol_id);
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

        // Check if default linear precomp playback (`st == first_frame` and no loop wrap or offset)
        // already matches every frame in `[first_frame..last_frame)`.
        let _ = timeline;
        let matches_default =
            (first_frame..last_frame).all(|f| child_frame_at(f) == f.saturating_sub(first_frame));
        if matches_default {
            return None;
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

    fn build_animated_ks(
        &self,
        timeline: &Timeline,
        layer: &Layer,
        samples: &[(u32, Element)],
        anchor: [f64; 2],
        origin_offset: [f64; 2],
    ) -> Value {
        let first_t = samples[0].1.transform;
        let is_static = samples
            .iter()
            .all(|(_, e)| transforms_approx_eq(e.transform, first_t));
        if is_static {
            return static_ks(first_t, anchor, origin_offset);
        }

        // Check if this layer uses a motion guide, custom spatial motion path, orient_to_path,
        // or bounce/elastic easing; if so, emit exact per-frame keyframes from `samples`.
        let dense = layer.properties.motion_guide.is_some()
            || layer.keyframes.iter().any(|k| {
                k.motion_path.is_some()
                    || k.orient_to_path
                    || matches!(k.tween, Some(Easing::Bounce | Easing::Elastic))
                    || k.property_eases
                        .iter()
                        .any(|(_, e)| matches!(e, Easing::Bounce | Easing::Elastic))
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
            collect_transform_keyframes(timeline, layer, samples)
        };

        let p_kfs = build_vec3_keyframes(&key_frames, |t, f| {
            let ease = ease_for_prop(layer, f, TweenProperty::X).or(t.2);
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
            let ease = ease_for_prop(layer, f, TweenProperty::ScaleX).or(t.2);
            (
                [
                    round4(t.1.scale_x * 100.0),
                    round4(t.1.scale_y * 100.0),
                    100.0,
                ],
                ease,
            )
        });
        let r_kfs = build_scalar_keyframes(&key_frames, |t, f| {
            let ease = ease_for_prop(layer, f, TweenProperty::Rotation).or(t.2);
            (round4(t.1.rotation), ease)
        });
        let o_kfs = build_scalar_keyframes(&key_frames, |t, f| {
            let ease = ease_for_prop(layer, f, TweenProperty::Alpha).or(t.2);
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
                let ease = ease_for_prop(layer, f, TweenProperty::Skew).or(t.2);
                (round4(-t.1.skew), ease)
            });
            ks.insert("sk".into(), sk_kfs);
            ks.insert("sa".into(), json!({"a": 0, "k": 0.0}));
        }
        Value::Object(ks)
    }

    fn element_to_shape_group(
        &self,
        element: &Element,
        offset: [f64; 2],
        apply_color_tx_alpha: bool,
    ) -> Option<Value> {
        let (mut commands, fill_rule) = element.to_path_commands()?;
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
                json!({
                    "ty": "sh",
                    "d": 1,
                    "nm": format!("Path {}", i + 1),
                    "ks": {
                        "a": 0,
                        "k": bez.to_json()
                    }
                })
            })
            .collect();

        let (raw_fill, raw_stroke, stroke_width) = extract_element_paint(element);
        let ct = element.transform.color_transform;
        let fill = apply_rgb_only(ct, raw_fill, apply_color_tx_alpha);
        let stroke = apply_rgb_only(ct, raw_stroke, apply_color_tx_alpha);

        let (fill_grad, stroke_grad) = match &element.kind {
            ElementKind::PaintedPath {
                fill_gradient,
                stroke_gradient,
                ..
            } => (fill_gradient.as_ref(), stroke_gradient.as_ref()),
            _ => (None, None),
        };

        if let Some(grad) = fill_grad {
            items.push(gradient_fill_item(grad, fill_rule));
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
            if let Some(grad) = stroke_grad {
                items.push(gradient_stroke_item(grad, stroke_width));
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

    fn attach_masks(
        &self,
        layer_obj: &mut Value,
        timeline: &Timeline,
        mask_layer: Option<&Layer>,
        first_frame: u32,
        last_frame: u32,
        target_transform: Transform,
    ) {
        let Some(mask_layer) = mask_layer else {
            return;
        };
        if !layer_effectively_visible(timeline, mask_layer) {
            return;
        }
        let inv_target = invert_affine(neoflash_core::transform_matrix(target_transform));
        let mask_els = sample_layer(mask_layer, first_frame);
        let mut masks_props = Vec::new();
        for el in mask_els {
            let Some((cmds, _)) = el.to_path_commands() else {
                continue;
            };
            let mask_m = neoflash_core::transform_matrix(el.transform);
            let combined = match inv_target {
                Some(inv) => neoflash_core::affine_multiply(inv, mask_m),
                None => mask_m,
            };
            let local_cmds = transform_path_commands(&cmds, combined);
            for bez in path_commands_to_lottie_beziers(&local_cmds) {
                masks_props.push(json!({
                    "inv": false,
                    "mode": "a",
                    "pt": {"a": 0, "k": bez.to_json()},
                    "o": {"a": 0, "k": 100},
                    "x": {"a": 0, "k": 0},
                    "nm": mask_layer.name
                }));
            }
        }
        let _ = last_frame;
        if !masks_props.is_empty()
            && let Some(map) = layer_obj.as_object_mut()
        {
            map.insert("hasMask".into(), Value::Bool(true));
            map.insert("masksProperties".into(), Value::Array(masks_props));
        }
    }
}

#[derive(Clone, Debug)]
struct LayerSpan {
    start_frame: u32,
    end_frame: u32,
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
) -> Vec<(u32, Transform, Option<Easing>)> {
    let first_f = samples.first().unwrap().0;
    let last_f = samples.last().unwrap().0;
    let sample_map: BTreeMap<u32, Transform> =
        samples.iter().map(|(f, e)| (*f, e.transform)).collect();

    let mut key_times: BTreeSet<u32> = BTreeSet::new();
    key_times.insert(first_f);
    key_times.insert(last_f);
    for kf in &layer.keyframes {
        if kf.frame >= first_f && kf.frame <= last_f && sample_map.contains_key(&kf.frame) {
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
        let ease = layer_ease_at(layer, f);
        out.push((f, t, ease));
    }
    out
}

fn layer_ease_at(layer: &Layer, frame: u32) -> Option<Easing> {
    if let Some((start, _, _)) = motion_span(layer, frame) {
        return start.tween;
    }
    layer.key_at(frame).and_then(|k| k.tween)
}

fn ease_for_prop(layer: &Layer, frame: u32, prop: TweenProperty) -> Option<Easing> {
    if let Some((start, _, _)) = motion_span(layer, frame) {
        return Some(effective_property_ease(start, prop));
    }
    layer.key_at(frame).and_then(|k| k.tween)
}

fn easing_to_lottie_tangents(ease: Easing) -> ([f64; 2], [f64; 2]) {
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

fn static_ks(t: Transform, anchor: [f64; 2], origin_offset: [f64; 2]) -> Value {
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
        json!({"a": 0, "k": [round4(t.scale_x * 100.0), round4(t.scale_y * 100.0), 100.0]}),
    );
    if t.skew.abs() > 1e-4 {
        ks.insert("sk".into(), json!({"a": 0, "k": round4(-t.skew)}));
        ks.insert("sa".into(), json!({"a": 0, "k": 0.0}));
    }
    Value::Object(ks)
}

fn identity_group_tr() -> Value {
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

fn effective_opacity_percent(t: Transform) -> f64 {
    let ct_alpha =
        (t.color_transform.multipliers[3] + t.color_transform.offsets[3] / 255.0).clamp(0.0, 1.0);
    (t.alpha.clamp(0.0, 1.0) * ct_alpha * 100.0).clamp(0.0, 100.0)
}

fn apply_rgb_only(ct: ColorTransform, color: Color, include_alpha: bool) -> Color {
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

fn gradient_stops_array(grad: &GradientPaint) -> Vec<f64> {
    let mut arr = Vec::with_capacity(grad.stops.len() * 6);
    for stop in &grad.stops {
        arr.push(round4(f64::from(stop.ratio) / 255.0));
        arr.push(round4(f64::from(stop.color[0]) / 255.0));
        arr.push(round4(f64::from(stop.color[1]) / 255.0));
        arr.push(round4(f64::from(stop.color[2]) / 255.0));
    }
    if grad.stops.iter().any(|s| s.color[3] < 255) {
        for stop in &grad.stops {
            arr.push(round4(f64::from(stop.ratio) / 255.0));
            arr.push(round4(f64::from(stop.color[3]) / 255.0));
        }
    }
    arr
}

fn gradient_fill_item(grad: &GradientPaint, fill_rule: FillRule) -> Value {
    let (s, e, t) = gradient_endpoints(grad);
    let k = gradient_stops_array(grad);
    json!({
        "ty": "gf",
        "nm": "Gradient Fill",
        "o": {"a": 0, "k": 100},
        "r": if fill_rule == FillRule::EvenOdd { 2 } else { 1 },
        "bm": 0,
        "g": {
            "p": grad.stops.len(),
            "k": {"a": 0, "k": k}
        },
        "s": {"a": 0, "k": s},
        "e": {"a": 0, "k": e},
        "t": t
    })
}

fn gradient_stroke_item(grad: &GradientPaint, stroke_width: f64) -> Value {
    let (s, e, t) = gradient_endpoints(grad);
    let k = gradient_stops_array(grad);
    json!({
        "ty": "gs",
        "nm": "Gradient Stroke",
        "o": {"a": 0, "k": 100},
        "w": {"a": 0, "k": round4(stroke_width)},
        "lc": 2,
        "lj": 2,
        "bm": 0,
        "g": {
            "p": grad.stops.len(),
            "k": {"a": 0, "k": k}
        },
        "s": {"a": 0, "k": s},
        "e": {"a": 0, "k": e},
        "t": t
    })
}

#[derive(Clone, Debug)]
struct LottieBezier {
    c: bool,
    v: Vec<[f64; 2]>,
    i: Vec<[f64; 2]>,
    o: Vec<[f64; 2]>,
}

impl LottieBezier {
    fn to_json(&self) -> Value {
        json!({
            "c": self.c,
            "v": self.v.iter().map(|p| [round4(p[0]), round4(p[1])]).collect::<Vec<_>>(),
            "i": self.i.iter().map(|p| [round4(p[0]), round4(p[1])]).collect::<Vec<_>>(),
            "o": self.o.iter().map(|p| [round4(p[0]), round4(p[1])]).collect::<Vec<_>>()
        })
    }
}

fn path_commands_to_lottie_beziers(commands: &[PathCommand]) -> Vec<LottieBezier> {
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

fn invert_affine(m: [f64; 6]) -> Option<[f64; 6]> {
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

fn transforms_approx_eq(a: Transform, b: Transform) -> bool {
    (a.x - b.x).abs() < 1e-4
        && (a.y - b.y).abs() < 1e-4
        && (a.scale_x - b.scale_x).abs() < 1e-4
        && (a.scale_y - b.scale_y).abs() < 1e-4
        && (a.rotation - b.rotation).abs() < 1e-4
        && (a.alpha - b.alpha).abs() < 1e-4
        && (a.skew - b.skew).abs() < 1e-4
        && a.color_transform == b.color_transform
}

fn layer_element_name(layer: &Layer, el: &Element) -> String {
    if el.name.is_empty() {
        layer.name.clone()
    } else {
        format!("{} · {}", layer.name, el.name)
    }
}

fn sanitize_id(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn round4(v: f64) -> f64 {
    if !v.is_finite() {
        return 0.0;
    }
    (v * 10_000.0).round() / 10_000.0
}

fn resolve_target_symbol<'a>(project: &'a Project, query: &str) -> Option<&'a Symbol> {
    project
        .symbol(query)
        .or_else(|| project.symbols.iter().find(|s| s.name == query))
}

fn build_markers(timeline: &Timeline, scene_ranges: &[neoflash_core::SceneRange]) -> Vec<Value> {
    let mut markers = Vec::new();
    if scene_ranges.len() > 1 {
        for scene in scene_ranges {
            markers.push(json!({
                "tm": scene.start,
                "dr": scene.frames,
                "cm": scene.name
            }));
        }
    }
    for layer in &timeline.layers {
        for (idx, kf) in layer.keyframes.iter().enumerate() {
            if !kf.label.trim().is_empty() {
                let next_f = layer
                    .keyframes
                    .get(idx + 1)
                    .map(|n| n.frame)
                    .unwrap_or(timeline.frame_count);
                markers.push(json!({
                    "tm": kf.frame,
                    "dr": next_f.saturating_sub(kf.frame),
                    "cm": kf.label
                }));
            }
        }
    }
    markers
}

/// Build a Lottie (`v: "5.7.14"`) JSON [`Value`] and [`LottieExportReport`] from `project`.
pub fn export_lottie_value(
    project: &Project,
    options: &LottieOptions,
) -> Result<(Value, LottieExportReport), ExportError> {
    project
        .validate()
        .map_err(|e| ExportError::InvalidProject(e.to_string()))?;

    let (working_project, scene_ranges, target_symbol_id, comp_name, include_bg) =
        if let Some(sym_query) = options.symbol.as_deref() {
            let sym = resolve_target_symbol(project, sym_query).ok_or_else(|| {
                ExportError::Options(format!(
                    "Symbol '{sym_query}' was not found in the project."
                ))
            })?;
            let is_scene = project.is_scene(&sym.id) || sym.id == project.root;
            (
                project.clone(),
                Vec::new(),
                sym.id.clone(),
                sym.name.clone(),
                options.include_background && is_scene,
            )
        } else {
            let (flattened, ranges) = flatten_scenes_with_ranges(project);
            let root_id = flattened.root.clone();
            (
                flattened,
                ranges,
                root_id,
                project.name.clone(),
                options.include_background,
            )
        };

    let target_symbol = working_project.symbol(&target_symbol_id).ok_or_else(|| {
        ExportError::InvalidProject(format!("Target symbol '{target_symbol_id}' is missing."))
    })?;

    let mut builder = LottieBuilder {
        project: &working_project,
        options,
        fps: f64::from(working_project.fps.clamp(1, 240)),
        comp_w: working_project.width.max(1),
        comp_h: working_project.height.max(1),
        assets: Vec::new(),
        exported_precomps: BTreeSet::new(),
        exported_images: BTreeSet::new(),
        warnings: Vec::new(),
        warning_set: BTreeSet::new(),
    };

    if working_project.has_program() {
        builder.warn(
            "Project scripts/classes (ActionScript/JavaScript) are not executed by Lottie players; timeline animation was exported."
                .to_owned(),
        );
    }

    // For a non-scene symbol whose registration point is `(0, 0)`, center the root Lottie
    // composition at `(width / 2, height / 2)` only if its artwork crosses negative coordinates.
    let is_scene = project.is_scene(&target_symbol.id) || target_symbol.id == working_project.root;
    let root_offset = if is_scene {
        [0.0, 0.0]
    } else {
        [
            working_project.width as f64 / 2.0,
            working_project.height as f64 / 2.0,
        ]
    };

    let mut stack = vec![target_symbol_id.clone()];
    let layers = builder.build_timeline_layers(
        &comp_name,
        &target_symbol.timeline,
        root_offset,
        include_bg,
        &mut stack,
    );
    let markers = build_markers(&target_symbol.timeline, &scene_ranges);
    let frame_count = target_symbol.timeline.frame_count.max(1);

    let root = json!({
        "v": LOTTIE_VERSION,
        "fr": round4(builder.fps),
        "ip": 0,
        "op": frame_count,
        "w": builder.comp_w,
        "h": builder.comp_h,
        "nm": comp_name,
        "ddd": 0,
        "assets": builder.assets,
        "layers": layers,
        "markers": markers
    });

    let report = LottieExportReport {
        path: None,
        name: comp_name,
        width: builder.comp_w,
        height: builder.comp_h,
        frame_count,
        layer_count: root["layers"].as_array().map_or(0, Vec::len),
        asset_count: root["assets"].as_array().map_or(0, Vec::len),
        bytes: 0,
        warnings: builder.warnings,
    };

    Ok((root, report))
}

/// Serialize `project` into a Lottie JSON string along with [`LottieExportReport`].
pub fn export_lottie_json(
    project: &Project,
    options: &LottieOptions,
) -> Result<(String, LottieExportReport), ExportError> {
    let (value, mut report) = export_lottie_value(project, options)?;
    let json = if options.pretty {
        serde_json::to_string_pretty(&value)?
    } else {
        serde_json::to_string(&value)?
    };
    report.bytes = json.len();
    Ok((json, report))
}

/// Write a Lottie `.json` file to `output_file` and return the resulting [`LottieExportReport`].
pub fn export_project_lottie(
    project: &Project,
    output_file: &Path,
    options: &LottieOptions,
) -> Result<LottieExportReport, ExportError> {
    let (json, mut report) = export_lottie_json(project, options)?;
    if let Some(parent) = output_file.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    fs::write(output_file, json.as_bytes())?;
    report.path = Some(output_file.to_path_buf());
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use neoflash_core::{
        FrameScript, GradientStop, Keyframe, LayerProperties, OvalOptions, PrimitiveOptions,
        RectangleOptions, Script, ScriptLanguage, SymbolKind,
    };

    #[test]
    fn demo_and_tween_features_export_valid_bodymovin_lottie_json() {
        let mut project = Project::blank();
        project.name = "Hero Banner".into();
        project.width = 640;
        project.height = 360;
        project.fps = 30;
        project.background = [24, 28, 36, 255];
        project.format_version = 8;

        // Add a nested Graphic symbol ("Star") with 4 frames so we can test Precomps + T-038
        // `sync_timeline` and `LoopFrom` time remapping!
        let star_id = "symbol-star".to_owned();
        project.symbols.push(Symbol {
            id: star_id.clone(),
            name: "Star".into(),
            folder: String::new(),
            kind: SymbolKind::Graphic,
            class_name: None,
            linkage_id: None,
            timeline: Timeline {
                guides: vec![],
                frame_count: 4,
                scripts: vec![],
                layers: vec![Layer {
                    id: "star-layer".into(),
                    name: "Star Shape".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: (0..4)
                        .map(|f| Keyframe {
                            frame: f,
                            elements: vec![Element {
                                id: "star-rect".into(),
                                name: "Box".into(),
                                transform: Transform {
                                    x: f64::from(f * 10),
                                    y: 0.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Rectangle {
                                    width: 40.0,
                                    height: 40.0,
                                    fill: [255, 200, 40, 255],
                                    stroke: [255, 255, 255, 255],
                                    stroke_width: 2.0,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        })
                        .collect(),
                }],
            },
        });

        let root_id = project.root.clone();
        let root = project.symbol_mut(&root_id).unwrap();
        root.timeline.frame_count = 10;
        // Layer 0 (bottom): Classic-tweened rounded rectangle with Screen blend mode + label
        root.timeline.layers[0] = Layer {
            id: "layer-bg-card".into(),
            name: "Card".into(),
            visible: true,
            locked: false,
            properties: LayerProperties {
                blend_mode: BlendMode::Screen,
                ..LayerProperties::default()
            },
            keyframes: vec![
                Keyframe {
                    frame: 0,
                    label: "intro".into(),
                    tween: Some(Easing::EaseInOut),
                    tween_kind: TweenKind::Classic,
                    elements: vec![Element {
                        id: "card-el".into(),
                        name: "Card".into(),
                        transform: Transform {
                            x: 40.0,
                            y: 50.0,
                            primitive: Some(PrimitiveOptions::Rectangle(RectangleOptions {
                                corner_radii: [8.0; 4],
                                corners_locked: true,
                            })),
                            ..Transform::default()
                        },
                        kind: ElementKind::Rectangle {
                            width: 120.0,
                            height: 80.0,
                            fill: [60, 140, 255, 255],
                            stroke: [0; 4],
                            stroke_width: 0.0,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                },
                Keyframe {
                    frame: 9,
                    elements: vec![Element {
                        id: "card-el".into(),
                        name: "Card".into(),
                        transform: Transform {
                            x: 240.0,
                            y: 110.0,
                            alpha: 0.5,
                            primitive: Some(PrimitiveOptions::Rectangle(RectangleOptions {
                                corner_radii: [8.0; 4],
                                corners_locked: true,
                            })),
                            ..Transform::default()
                        },
                        kind: ElementKind::Rectangle {
                            width: 120.0,
                            height: 80.0,
                            fill: [60, 140, 255, 255],
                            stroke: [0; 4],
                            stroke_width: 0.0,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                },
            ],
        };

        // Layer 1 (top): Timeline-synced layer (`sync_timeline = true`) with frames 0..6 empty
        // and the `Star` instance appearing at frame 6 (1-based frame 7).
        root.timeline.layers.push(Layer {
            id: "layer-synced-star".into(),
            name: "Synced Star".into(),
            visible: true,
            locked: false,
            properties: LayerProperties {
                sync_timeline: true,
                ..LayerProperties::default()
            },
            keyframes: vec![
                Keyframe {
                    frame: 0,
                    elements: vec![],
                    ..Keyframe::default()
                },
                Keyframe {
                    frame: 6,
                    elements: vec![Element {
                        id: "star-inst".into(),
                        name: "StarInstance".into(),
                        transform: Transform {
                            x: 320.0,
                            y: 180.0,
                            ..Transform::default()
                        },
                        kind: ElementKind::Instance {
                            symbol_id: star_id.clone(),
                            playback: InstancePlayback::Loop,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                },
            ],
        });

        let (lottie, report) = export_lottie_value(&project, &LottieOptions::default()).unwrap();
        assert_eq!(lottie["v"], LOTTIE_VERSION);
        assert_eq!(lottie["w"], 640);
        assert_eq!(lottie["h"], 360);
        assert_eq!(lottie["fr"], 30.0);
        assert_eq!(lottie["ip"], 0);
        assert_eq!(lottie["op"], 10);
        assert_eq!(report.asset_count, 1, "Star precomp asset");
        assert_eq!(
            report.layer_count, 3,
            "Synced Star precomp + Card shape + Stage Background solid"
        );
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);

        // Top layer in Lottie (`layers[0]`) is "Synced Star · StarInstance" (`ty: 0`, `ip: 6`, `op: 10`)
        // with time remapping `tm` starting at frame 6 (`6 % 4 == 2`, so `2.0 / 30.0` seconds).
        let top = &lottie["layers"][0];
        assert_eq!(top["ty"], 0);
        assert_eq!(top["ip"], 6);
        assert_eq!(top["op"], 10);
        assert_eq!(top["tm"]["a"], 1);
        assert_eq!(top["tm"]["k"][0]["t"], 6);
        assert_eq!(top["tm"]["k"][0]["s"][0], round4(2.0 / 30.0));

        // Second layer (`layers[1]`) is "Card · Card" (`ty: 4`, Screen blend mode `bm: 2`, animated `p` and `o`).
        let card = &lottie["layers"][1];
        assert_eq!(card["ty"], 4);
        assert_eq!(card["bm"], 2);
        assert_eq!(card["ks"]["p"]["a"], 1);
        assert_eq!(card["ks"]["o"]["a"], 1);

        // Marker "intro" exported at frame 0.
        assert_eq!(lottie["markers"][0]["cm"], "intro");
        assert_eq!(lottie["markers"][0]["tm"], 0);

        // Also test symbol-only export (`options.symbol = Some("Star")`).
        let (star_json, star_report) = export_lottie_json(
            &project,
            &LottieOptions {
                symbol: Some("Star".into()),
                include_background: false,
                include_hidden_layers: false,
                pretty: true,
            },
        )
        .unwrap();
        assert_eq!(star_report.name, "Star");
        assert_eq!(star_report.frame_count, 4);
        assert!(star_json.contains("\"nm\": \"Star\""));
    }

    #[test]
    fn shape_tween_gradients_oval_and_script_warnings_export_cleanly() {
        let mut project = Project::blank();
        project.format_version = 8;
        let root_id = project.root.clone();
        let root = project.symbol_mut(&root_id).unwrap();
        root.timeline.frame_count = 5;
        root.timeline.scripts.push(FrameScript {
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript3,
                source: "stop();".into(),
            },
        });
        root.timeline.layers[0] = Layer {
            id: "morph-layer".into(),
            name: "Morph".into(),
            visible: true,
            locked: false,
            properties: LayerProperties {
                blend_mode: BlendMode::Invert,
                ..LayerProperties::default()
            },
            keyframes: vec![
                Keyframe {
                    frame: 0,
                    tween: Some(Easing::Linear),
                    tween_kind: TweenKind::Shape,
                    elements: vec![Element {
                        id: "shape-a".into(),
                        name: "Oval".into(),
                        transform: Transform {
                            primitive: Some(PrimitiveOptions::Oval(OvalOptions {
                                start_angle: 0.0,
                                end_angle: 270.0,
                                inner_radius: 25.0,
                                close_path: true,
                            })),
                            ..Transform::default()
                        },
                        kind: ElementKind::Ellipse {
                            width: 100.0,
                            height: 100.0,
                            fill: [255, 80, 80, 255],
                            stroke: [255, 255, 255, 255],
                            stroke_width: 3.0,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                },
                Keyframe {
                    frame: 4,
                    elements: vec![Element {
                        id: "shape-b".into(),
                        name: "GradientPath".into(),
                        transform: Transform::default(),
                        kind: ElementKind::PaintedPath {
                            commands: vec![
                                PathCommand::MoveTo { x: 0.0, y: 0.0 },
                                PathCommand::LineTo { x: 120.0, y: 0.0 },
                                PathCommand::QuadraticTo {
                                    cx: 120.0,
                                    cy: 120.0,
                                    x: 0.0,
                                    y: 120.0,
                                },
                                PathCommand::Close,
                            ],
                            fill_rule: FillRule::EvenOdd,
                            fill: [0, 200, 120, 255],
                            stroke: [0; 4],
                            fill_gradient: Some(GradientPaint {
                                kind: GradientKind::Linear,
                                matrix: [0.1, 0.0, 0.0, 0.1, 60.0, 60.0],
                                stops: vec![
                                    GradientStop {
                                        ratio: 0,
                                        color: [255, 0, 128, 255],
                                    },
                                    GradientStop {
                                        ratio: 255,
                                        color: [0, 200, 255, 180],
                                    },
                                ],
                                focal_point_ratio: 0.0,
                            }),
                            stroke_gradient: None,
                            stroke_width: 0.0,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                },
            ],
        };

        let (lottie, report) = export_lottie_value(&project, &LottieOptions::default()).unwrap();
        assert_eq!(lottie["v"], LOTTIE_VERSION);
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("frame script(s) are not executed by Lottie players")),
            "expected script diagnostic: {:?}",
            report.warnings
        );
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("blend mode 'invert' is not supported by Lottie")),
            "expected blend mode diagnostic: {:?}",
            report.warnings
        );
    }
}
