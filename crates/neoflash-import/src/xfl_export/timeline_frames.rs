//! XFL timeline, layer, motion-span expansion, and frame XML serialization.
use super::*;

pub(super) struct PreparedFrame {
    index: u32,
    duration: u32,
    label: String,
    label_type: FrameLabelType,
    tween: Option<(Easing, TweenKind, bool)>,
    sound: Option<FrameSound>,
    script: Option<String>,
    elements_xml: Vec<String>,
}

pub(super) struct PreparedLayer {
    original_id: Option<Id>,
    is_primary_for_id: bool,
    name: String,
    visible: bool,
    locked: bool,
    properties: LayerProperties,
    frames: Vec<PreparedFrame>,
}

pub(super) fn compute_timeline_frames(timeline: &Timeline) -> u32 {
    let mut frames = timeline.frame_count.max(1);
    for layer in &timeline.layers {
        if layer.properties.role == LayerRole::Folder {
            continue;
        }
        if let Some(end) = layer.properties.end_frame {
            frames = frames.max(end);
        }
        for key in &layer.keyframes {
            frames = frames.max(key.frame.saturating_add(1));
        }
    }
    for fs in &timeline.scripts {
        if !fs.script.source.trim().is_empty() {
            frames = frames.max(fs.frame.saturating_add(1));
        }
    }
    frames
}

pub(super) fn layer_effective_end(layer: &Layer, timeline_frames: u32) -> u32 {
    layer
        .properties
        .end_frame
        .unwrap_or(timeline_frames)
        .min(timeline_frames)
}

pub(super) fn render_dom_timeline(
    xml: &mut String,
    indent: &str,
    name: &str,
    timeline: &Timeline,
    ctx: &ExportContext,
) {
    writeln!(
        xml,
        "{indent}<DOMTimeline name=\"{}\">",
        escape_xml_attr(name)
    )
    .unwrap();
    writeln!(xml, "{indent}  <layers>").unwrap();

    let timeline_frames = compute_timeline_frames(timeline);

    // Prepare layers in NeoFlash painter order (bottom-to-top), splitting multi-element
    // continuous symbol-instance layers when needed to preserve 1-element instance continuity.
    let mut painter_layers: Vec<PreparedLayer> = Vec::new();
    if timeline.layers.is_empty() {
        painter_layers.push(PreparedLayer {
            original_id: None,
            is_primary_for_id: false,
            name: "Layer 1".to_owned(),
            visible: true,
            locked: false,
            properties: LayerProperties::default(),
            frames: vec![PreparedFrame {
                index: 0,
                duration: timeline_frames,
                label: String::new(),
                label_type: FrameLabelType::Name,
                tween: None,
                sound: None,
                script: None,
                elements_xml: Vec::new(),
            }],
        });
    } else {
        for layer in &timeline.layers {
            for sub in prepare_layer(timeline, layer, timeline_frames, ctx) {
                painter_layers.push(sub);
            }
        }
    }

    // XFL stores topmost layers first.
    painter_layers.reverse();

    // Explicitly owned frame scripts stay on their stable authoring layer. This preserves two
    // independent scripts at the same timeline frame instead of merging them onto the first row.
    // A legacy layerless script retains the previous deterministic topmost-boundary fallback.
    let append_source = |scripts: &mut BTreeMap<u32, String>, frame: u32, source: String| {
        scripts
            .entry(frame)
            .and_modify(|existing| {
                existing.push('\n');
                existing.push_str(&source);
            })
            .or_insert(source);
    };
    let mut unplaced_scripts: BTreeMap<u32, String> = BTreeMap::new();
    for fs in timeline
        .scripts
        .iter()
        .filter(|script| script.layer.is_some() && !script.script.source.trim().is_empty())
    {
        let owner = fs.layer.as_ref().expect("filtered owned frame script");
        let encoded = encode_script_with_language_tag(&fs.script);
        let placed = painter_layers
            .iter_mut()
            .find(|layer| {
                layer.is_primary_for_id
                    && layer.original_id.as_ref() == Some(owner)
                    && layer.properties.role != LayerRole::Folder
            })
            .and_then(|layer| {
                layer
                    .frames
                    .iter_mut()
                    .find(|frame| frame.index == fs.frame)
            })
            .is_some_and(|slot| {
                if let Some(existing) = &mut slot.script {
                    existing.push('\n');
                    existing.push_str(&encoded);
                } else {
                    slot.script = Some(encoded.clone());
                }
                true
            });
        if !placed {
            append_source(&mut unplaced_scripts, fs.frame, encoded);
        }
    }

    let mut legacy_scripts: BTreeMap<u32, String> = BTreeMap::new();
    for fs in timeline
        .scripts
        .iter()
        .filter(|script| script.layer.is_none() && !script.script.source.trim().is_empty())
    {
        append_source(
            &mut legacy_scripts,
            fs.frame,
            encode_script_with_language_tag(&fs.script),
        );
    }
    for (frame_idx, source_code) in legacy_scripts {
        let mut placed = false;
        for layer in &mut painter_layers {
            if layer.properties.role == LayerRole::Folder {
                continue;
            }
            if !layer.visible && !ctx.include_hidden_layers {
                continue;
            }
            if let Some(slot) = layer
                .frames
                .iter_mut()
                .find(|f| f.index == frame_idx && f.script.is_none())
            {
                slot.script = Some(source_code.clone());
                placed = true;
                break;
            }
        }
        if !placed {
            append_source(&mut unplaced_scripts, frame_idx, source_code);
        }
    }

    if !unplaced_scripts.is_empty() {
        let unplaced_scripts: Vec<_> = unplaced_scripts.into_iter().collect();
        painter_layers.insert(
            0,
            build_synthetic_actions_layer(&unplaced_scripts, timeline_frames),
        );
    }

    // Map original layer IDs to their XFL layer index for `parentLayerIndex`.
    let mut id_to_xfl_index: HashMap<&str, (usize, LayerRole)> = HashMap::new();
    for (xfl_idx, layer) in painter_layers.iter().enumerate() {
        if layer.is_primary_for_id
            && let Some(orig_id) = layer.original_id.as_deref()
        {
            id_to_xfl_index.insert(orig_id, (xfl_idx, layer.properties.role));
        }
    }

    let layer_indent = format!("{indent}    ");
    for (xfl_idx, layer) in painter_layers.iter().enumerate() {
        let parent_idx = resolve_parent_layer_index(xfl_idx, layer, &id_to_xfl_index);
        render_dom_layer(xml, &layer_indent, layer, parent_idx, ctx);
    }

    writeln!(xml, "{indent}  </layers>").unwrap();
    writeln!(xml, "{indent}</DOMTimeline>").unwrap();
}

pub(super) fn build_synthetic_actions_layer(
    scripts: &[(u32, String)],
    timeline_frames: u32,
) -> PreparedLayer {
    let mut frames = Vec::new();
    let mut cursor = 0u32;
    for (idx, (script_frame, source_code)) in scripts.iter().enumerate() {
        if *script_frame > cursor {
            frames.push(PreparedFrame {
                index: cursor,
                duration: *script_frame - cursor,
                label: String::new(),
                label_type: FrameLabelType::Name,
                tween: None,
                sound: None,
                script: None,
                elements_xml: Vec::new(),
            });
        }
        let next_boundary = scripts
            .get(idx + 1)
            .map_or(timeline_frames, |(nf, _)| *nf)
            .max(script_frame.saturating_add(1));
        frames.push(PreparedFrame {
            index: *script_frame,
            duration: next_boundary - *script_frame,
            label: String::new(),
            label_type: FrameLabelType::Name,
            tween: None,
            sound: None,
            script: Some(source_code.clone()),
            elements_xml: Vec::new(),
        });
        cursor = next_boundary;
    }
    PreparedLayer {
        original_id: None,
        is_primary_for_id: false,
        name: "Actions".to_owned(),
        visible: true,
        locked: false,
        properties: LayerProperties::default(),
        frames,
    }
}

pub(super) fn resolve_parent_layer_index(
    xfl_idx: usize,
    layer: &PreparedLayer,
    id_to_xfl_index: &HashMap<&str, (usize, LayerRole)>,
) -> Option<usize> {
    let lookup = |target: Option<&Id>, expected_role: LayerRole| -> Option<usize> {
        let target_id = target?;
        let &(idx, role) = id_to_xfl_index.get(target_id.as_str())?;
        (role == expected_role && idx != xfl_idx).then_some(idx)
    };
    if layer.properties.role == LayerRole::Normal {
        lookup(layer.properties.mask.as_ref(), LayerRole::Mask)
            .or_else(|| lookup(layer.properties.motion_guide.as_ref(), LayerRole::Guide))
            .or_else(|| lookup(layer.properties.parent.as_ref(), LayerRole::Folder))
    } else {
        lookup(layer.properties.parent.as_ref(), LayerRole::Folder)
    }
}

pub(super) fn prepare_layer(
    timeline: &Timeline,
    layer: &Layer,
    timeline_frames: u32,
    ctx: &ExportContext,
) -> Vec<PreparedLayer> {
    if layer.properties.role == LayerRole::Folder {
        return vec![PreparedLayer {
            original_id: Some(layer.id.clone()),
            is_primary_for_id: true,
            name: if layer.name.is_empty() {
                "Folder".to_owned()
            } else {
                layer.name.clone()
            },
            visible: layer.visible,
            locked: layer.locked,
            properties: layer.properties.clone(),
            frames: Vec::new(),
        }];
    }

    let baked_keys = expand_motion_spans(timeline, layer, timeline_frames, ctx.fps);
    let split_key_sets = split_multi_element_continuous_instances(&baked_keys, ctx);
    let total_splits = split_key_sets.len();
    let mut out = Vec::with_capacity(total_splits);
    for (sub_idx, sub_keys) in split_key_sets.into_iter().enumerate() {
        let is_primary = sub_idx == 0;
        let frames = build_prepared_frames(layer, &sub_keys, timeline_frames, ctx);
        out.push(PreparedLayer {
            original_id: Some(layer.id.clone()),
            is_primary_for_id: is_primary,
            name: if layer.name.is_empty() {
                "Layer".to_owned()
            } else if total_splits == 1 || is_primary {
                layer.name.clone()
            } else {
                format!("{} ({})", layer.name, sub_idx + 1)
            },
            visible: layer.visible,
            locked: layer.locked,
            properties: layer.properties.clone(),
            frames,
        });
    }
    out
}

/// Evaluates `TweenKind::Motion` property-track spans into standard pose keyframes so
/// XFL 2.2 receives classic tweens without unsupported `MotionObjectXML` payloads.
pub(super) fn expand_motion_spans(
    timeline: &Timeline,
    layer: &Layer,
    timeline_frames: u32,
    fps: f64,
) -> Vec<Keyframe> {
    let mut sorted = layer.keyframes.clone();
    sorted.sort_by_key(|k| k.frame);
    // Deduplicate same-frame keys if any exist by keeping the last entry at each frame.
    let mut deduped: Vec<Keyframe> = Vec::with_capacity(sorted.len());
    for k in sorted {
        if let Some(last) = deduped.last_mut()
            && last.frame == k.frame
        {
            *last = k;
        } else {
            deduped.push(k);
        }
    }

    let layer_end = layer_effective_end(layer, timeline_frames);
    let mut out: Vec<Keyframe> = Vec::with_capacity(deduped.len());
    let mut idx = 0usize;
    while idx < deduped.len() {
        let start = &deduped[idx];
        if start.keyed_properties.is_none()
            && start.tween.is_some()
            && start.tween_kind == TweenKind::Motion
        {
            let mut prop_end_idx = idx + 1;
            while prop_end_idx < deduped.len() && deduped[prop_end_idx].keyed_properties.is_some() {
                prop_end_idx += 1;
            }
            let last_prop_frame = if prop_end_idx > idx + 1 {
                deduped[prop_end_idx - 1].frame
            } else {
                start.frame
            };
            let next_ord_frame = deduped.get(prop_end_idx).map(|k| k.frame);
            let raw_span_end = next_ord_frame
                .unwrap_or(last_prop_frame)
                .max(last_prop_frame)
                .max(start.frame.saturating_add(1));
            let bake_end = raw_span_end.min(layer_end.max(start.frame.saturating_add(1)));

            let mut meta_by_frame: HashMap<u32, (String, FrameLabelType, Option<FrameSound>)> =
                HashMap::new();
            for k in &deduped[idx..prop_end_idx] {
                meta_by_frame.insert(k.frame, (k.label.clone(), k.label_type, k.sound.clone()));
            }

            for f in start.frame..bake_end {
                let (label, label_type, sound) = meta_by_frame.remove(&f).unwrap_or_default();
                let has_following_frame = f + 1 < bake_end || next_ord_frame == Some(bake_end);
                out.push(Keyframe {
                    frame: f,
                    elements: sample_layer_in_at_fps(timeline, layer, f, fps),
                    tween: has_following_frame.then_some(Easing::Linear),
                    tween_kind: TweenKind::Classic,
                    label,
                    label_type,
                    sound,
                    ..Default::default()
                });
            }
            if next_ord_frame.is_none() && bake_end < layer_end {
                let (label, label_type, sound) =
                    meta_by_frame.remove(&bake_end).unwrap_or_default();
                out.push(Keyframe {
                    frame: bake_end,
                    elements: sample_layer_in_at_fps(timeline, layer, bake_end, fps),
                    tween: None,
                    tween_kind: TweenKind::Classic,
                    label,
                    label_type,
                    sound,
                    ..Default::default()
                });
            }
            idx = prop_end_idx;
        } else {
            let mut clean = start.clone();
            clean.keyed_properties = None;
            out.push(clean);
            idx += 1;
        }
    }
    out
}

pub(super) struct ElementTrack {
    elem_id: Id,
    symbol_and_name: Option<(Id, String)>,
    by_key: Vec<Option<Element>>,
}

/// When a multi-element layer places the same library symbol across adjacent keyframes,
/// `xfl.rs` cannot connect multi-element keyframes via `classic_tween_matches` and would
/// reject the boundary as `ambiguous_instance_continuity`. Splitting into ordered single-track
/// sub-layers preserves exact painter order while giving each continuous symbol instance its
/// own 1-element layer.
pub(super) fn split_multi_element_continuous_instances(
    keys: &[Keyframe],
    ctx: &ExportContext,
) -> Vec<Vec<Keyframe>> {
    let has_multi = keys.iter().any(|k| k.elements.len() > 1);
    if !has_multi || keys.len() < 2 {
        return vec![keys.to_vec()];
    }
    let has_adjacent_same_symbol = keys.windows(2).any(|pair| {
        if pair[0].elements.len() <= 1 && pair[1].elements.len() <= 1 {
            return false;
        }
        pair[0].elements.iter().any(|ea| {
            let ElementKind::Instance { symbol_id: sa, .. } = &ea.kind else {
                return false;
            };
            ctx.symbols.contains_key(sa)
                && pair[1].elements.iter().any(|eb| {
                    matches!(&eb.kind, ElementKind::Instance { symbol_id: sb, .. } if sb == sa)
                })
        })
    });
    if !has_adjacent_same_symbol {
        return vec![keys.to_vec()];
    }

    let mut tracks: Vec<ElementTrack> = Vec::new();
    for (ki, key) in keys.iter().enumerate() {
        let mut min_track_idx = 0usize;
        for elem in &key.elements {
            let sym_key = match &elem.kind {
                ElementKind::Instance { symbol_id, .. } => {
                    Some((symbol_id.clone(), elem.name.clone()))
                }
                _ => None,
            };
            let matched = tracks[min_track_idx..]
                .iter()
                .position(|tr| {
                    tr.by_key[ki].is_none()
                        && (tr.elem_id == elem.id
                            || (sym_key.is_some() && tr.symbol_and_name == sym_key))
                })
                .map(|rel| min_track_idx + rel);

            if let Some(t_idx) = matched {
                tracks[t_idx].by_key[ki] = Some(elem.clone());
                min_track_idx = t_idx + 1;
            } else {
                let mut by_key = vec![None; keys.len()];
                by_key[ki] = Some(elem.clone());
                tracks.insert(
                    min_track_idx,
                    ElementTrack {
                        elem_id: elem.id.clone(),
                        symbol_and_name: sym_key,
                        by_key,
                    },
                );
                min_track_idx += 1;
            }
        }
    }

    if tracks.is_empty() {
        return vec![keys.to_vec()];
    }

    tracks
        .into_iter()
        .enumerate()
        .map(|(ti, tr)| {
            keys.iter()
                .enumerate()
                .map(|(ki, orig_k)| Keyframe {
                    frame: orig_k.frame,
                    elements: tr.by_key[ki].iter().cloned().collect(),
                    tween: orig_k.tween,
                    tween_kind: orig_k.tween_kind,
                    orient_to_path: orig_k.orient_to_path,
                    label: if ti == 0 {
                        orig_k.label.clone()
                    } else {
                        String::new()
                    },
                    label_type: if ti == 0 {
                        orig_k.label_type
                    } else {
                        FrameLabelType::Name
                    },
                    sound: if ti == 0 { orig_k.sound.clone() } else { None },
                    ..Default::default()
                })
                .collect()
        })
        .collect()
}

pub(super) struct RenderedKeyElements {
    xml_items: Vec<String>,
    all_shapes: bool,
}

pub(super) fn render_keyframe_elements(
    elements: &[Element],
    bitmap_fills: &[AuthoredBitmapFill],
    ctx: &ExportContext,
) -> RenderedKeyElements {
    let mut xml_items = Vec::with_capacity(elements.len());
    let mut all_shapes = true;
    for elem in elements {
        let (rendered, is_shape) = render_element(elem, bitmap_fills, ctx);
        if !rendered.is_empty() {
            if !is_shape {
                all_shapes = false;
            }
            xml_items.extend(rendered);
        }
    }
    if xml_items.is_empty() {
        all_shapes = false;
    }
    RenderedKeyElements {
        xml_items,
        all_shapes,
    }
}

pub(super) fn classic_tween_elements_compatible(
    a: &Element,
    b: &Element,
    a_has_bmp_fill: bool,
    b_has_bmp_fill: bool,
    a_xml_count: usize,
    b_xml_count: usize,
    ctx: &ExportContext,
) -> bool {
    if a_xml_count != 1 || b_xml_count != 1 || a.name != b.name {
        return false;
    }
    // Check that neither element will decompose into separate fill+stroke Elements on XFL re-import.
    if shape_splits_on_reimport(a, a_has_bmp_fill) || shape_splits_on_reimport(b, b_has_bmp_fill) {
        return false;
    }
    match (&a.kind, &b.kind) {
        (
            ElementKind::Instance { symbol_id: sa, .. },
            ElementKind::Instance { symbol_id: sb, .. },
        ) => sa == sb && ctx.symbols.contains_key(sa),
        (
            ElementKind::Bitmap {
                resource_id: ra, ..
            },
            ElementKind::Bitmap {
                resource_id: rb, ..
            },
        ) => ra == rb && ctx.bitmaps.contains_key(ra),
        (ElementKind::Rectangle { .. }, ElementKind::Rectangle { .. })
        | (ElementKind::Ellipse { .. }, ElementKind::Ellipse { .. }) => {
            let a_plain = !a.has_parametric_options() && !element_has_visible_stroke(a);
            let b_plain = !b.has_parametric_options() && !element_has_visible_stroke(b);
            a_plain == b_plain
        }
        (ElementKind::Path { .. }, ElementKind::Path { .. })
        | (ElementKind::PaintedPath { .. }, ElementKind::PaintedPath { .. })
        | (ElementKind::Text { .. }, ElementKind::Text { .. })
        | (ElementKind::TextBox { .. }, ElementKind::TextBox { .. }) => true,
        _ => false,
    }
}

pub(super) fn element_has_visible_stroke(elem: &Element) -> bool {
    let ct = elem
        .transform
        .color_transform
        .with_alpha(elem.transform.alpha);
    match &elem.kind {
        ElementKind::Rectangle {
            stroke,
            stroke_width,
            ..
        }
        | ElementKind::Ellipse {
            stroke,
            stroke_width,
            ..
        }
        | ElementKind::Path {
            stroke,
            stroke_width,
            ..
        } => *stroke_width > 0.0 && ct.apply(*stroke)[3] > 0,
        ElementKind::PaintedPath {
            stroke,
            stroke_gradient,
            stroke_width,
            ..
        } => {
            *stroke_width > 0.0
                && (stroke_gradient
                    .as_ref()
                    .is_some_and(|g| g.stops.iter().any(|s| ct.apply(s.color)[3] > 0))
                    || ct.apply(*stroke)[3] > 0)
        }
        _ => false,
    }
}

/// Returns true when a `<DOMShape>` has both fill and stroke and uses curves/gradients/multiple
/// contours, causing `xfl/shape.rs::general_shape` to emit 2 `PaintedPath` elements on import.
pub(super) fn shape_splits_on_reimport(elem: &Element, has_bitmap_fill: bool) -> bool {
    let ct = elem
        .transform
        .color_transform
        .with_alpha(elem.transform.alpha);
    match &elem.kind {
        ElementKind::PaintedPath {
            commands,
            fill,
            stroke,
            fill_gradient,
            stroke_gradient,
            stroke_width,
            ..
        } => {
            let has_fill = has_bitmap_fill
                || fill_gradient
                    .as_ref()
                    .is_some_and(|g| g.stops.iter().any(|s| ct.apply(s.color)[3] > 0))
                || ct.apply(*fill)[3] > 0;
            let has_stroke = *stroke_width > 0.0
                && (stroke_gradient
                    .as_ref()
                    .is_some_and(|g| g.stops.iter().any(|s| ct.apply(s.color)[3] > 0))
                    || ct.apply(*stroke)[3] > 0);
            if !(has_fill && has_stroke) {
                return false;
            }
            has_bitmap_fill
                || fill_gradient.is_some()
                || stroke_gradient.is_some()
                || commands.iter().any(|c| {
                    matches!(
                        c,
                        PathCommand::QuadraticTo { .. } | PathCommand::CubicTo { .. }
                    )
                })
        }
        ElementKind::Path {
            commands,
            fill,
            stroke,
            stroke_width,
            ..
        } => {
            let has_fill = has_bitmap_fill || ct.apply(*fill)[3] > 0;
            let has_stroke = *stroke_width > 0.0 && ct.apply(*stroke)[3] > 0;
            has_fill
                && has_stroke
                && (has_bitmap_fill
                    || commands.iter().any(|c| {
                        matches!(
                            c,
                            PathCommand::QuadraticTo { .. } | PathCommand::CubicTo { .. }
                        )
                    }))
        }
        ElementKind::Ellipse { fill, .. } => {
            let has_fill = elem.oval_options().close_path && ct.apply(*fill)[3] > 0;
            has_fill && element_has_visible_stroke(elem)
        }
        ElementKind::Rectangle { fill, .. } => {
            ct.apply(*fill)[3] > 0
                && element_has_visible_stroke(elem)
                && elem.has_parametric_options()
        }
        _ => false,
    }
}

pub(super) fn build_prepared_frames(
    layer: &Layer,
    keys: &[Keyframe],
    timeline_frames: u32,
    ctx: &ExportContext,
) -> Vec<PreparedFrame> {
    if layer.properties.end_frame == Some(0) {
        return Vec::new();
    }
    let layer_end = layer_effective_end(layer, timeline_frames);
    if keys.is_empty() {
        return vec![PreparedFrame {
            index: 0,
            duration: layer_end.max(1),
            label: String::new(),
            label_type: FrameLabelType::Name,
            tween: None,
            sound: None,
            script: None,
            elements_xml: Vec::new(),
        }];
    }

    let rendered: Vec<RenderedKeyElements> = keys
        .iter()
        .map(|k| render_keyframe_elements(&k.elements, &k.bitmap_fills, ctx))
        .collect();

    let mut frames = Vec::with_capacity(keys.len() + 2);
    if keys[0].frame > 0 {
        frames.push(PreparedFrame {
            index: 0,
            duration: keys[0].frame,
            label: String::new(),
            label_type: FrameLabelType::Name,
            tween: None,
            sound: None,
            script: None,
            elements_xml: Vec::new(),
        });
    }

    let has_motion_guide = layer.properties.motion_guide.is_some();

    for (i, key) in keys.iter().enumerate() {
        let f0 = key.frame;
        let f1 = if let Some(next_k) = keys.get(i + 1) {
            next_k.frame.max(f0.saturating_add(1))
        } else {
            layer_end.max(f0.saturating_add(1))
        };
        let duration = f1 - f0;
        let cur_rendered = &rendered[i];
        let next_pair = keys.get(i + 1).zip(rendered.get(i + 1));

        let authored_tween = key.tween.and_then(|ease| {
            let (next_k, next_r) = next_pair?;
            match key.tween_kind {
                TweenKind::Shape => (cur_rendered.all_shapes && next_r.all_shapes).then_some((
                    ease,
                    TweenKind::Shape,
                    false,
                )),
                TweenKind::Classic | TweenKind::Motion => (key.elements.len() == 1
                    && next_k.elements.len() == 1
                    && classic_tween_elements_compatible(
                        &key.elements[0],
                        &next_k.elements[0],
                        key.bitmap_fills
                            .iter()
                            .any(|bf| bf.element_id == key.elements[0].id),
                        next_k
                            .bitmap_fills
                            .iter()
                            .any(|bf| bf.element_id == next_k.elements[0].id),
                        cur_rendered.xml_items.len(),
                        next_r.xml_items.len(),
                        ctx,
                    ))
                .then_some((ease, TweenKind::Classic, key.orient_to_path)),
            }
        });

        // Bridge untweened continuous symbol-instance keyframes on non-guided layers so
        // `xfl.rs` preserves continuous `element.id` without `ambiguous_instance_continuity`.
        let needs_instance_bridge = key.tween.is_none()
            && !has_motion_guide
            && next_pair.is_some_and(|(next_k, next_r)| {
                key.elements.len() == 1
                    && next_k.elements.len() == 1
                    && cur_rendered.xml_items.len() == 1
                    && next_r.xml_items.len() == 1
                    && key.elements[0].name == next_k.elements[0].name
                    && matches!(
                        (&key.elements[0].kind, &next_k.elements[0].kind),
                        (
                            ElementKind::Instance { symbol_id: sa, .. },
                            ElementKind::Instance { symbol_id: sb, .. }
                        ) if sa == sb && ctx.symbols.contains_key(sa)
                    )
            });

        if needs_instance_bridge {
            let linear_classic = Some((Easing::Linear, TweenKind::Classic, false));
            if duration == 1 {
                frames.push(PreparedFrame {
                    index: f0,
                    duration: 1,
                    label: key.label.clone(),
                    label_type: key.label_type,
                    tween: linear_classic,
                    sound: key.sound.clone(),
                    script: None,
                    elements_xml: cur_rendered.xml_items.clone(),
                });
            } else {
                frames.push(PreparedFrame {
                    index: f0,
                    duration: duration - 1,
                    label: key.label.clone(),
                    label_type: key.label_type,
                    tween: linear_classic,
                    sound: key.sound.clone(),
                    script: None,
                    elements_xml: cur_rendered.xml_items.clone(),
                });
                frames.push(PreparedFrame {
                    index: f1 - 1,
                    duration: 1,
                    label: String::new(),
                    label_type: FrameLabelType::Name,
                    tween: linear_classic,
                    sound: None,
                    script: None,
                    elements_xml: cur_rendered.xml_items.clone(),
                });
            }
        } else {
            frames.push(PreparedFrame {
                index: f0,
                duration,
                label: key.label.clone(),
                label_type: key.label_type,
                tween: authored_tween,
                sound: key.sound.clone(),
                script: None,
                elements_xml: cur_rendered.xml_items.clone(),
            });
        }
    }

    frames
}

pub(super) fn render_dom_layer(
    xml: &mut String,
    indent: &str,
    layer: &PreparedLayer,
    parent_layer_index: Option<usize>,
    ctx: &ExportContext,
) {
    write!(
        xml,
        "{indent}<DOMLayer name=\"{}\"",
        escape_xml_attr(&layer.name)
    )
    .unwrap();
    if let Some(rgb) = layer.properties.outline_color {
        write!(xml, " color=\"{}\"", fmt_hex_rgb(rgb)).unwrap();
    }
    match layer.properties.role {
        LayerRole::Normal => {}
        LayerRole::Guide => xml.push_str(" layerType=\"guide\""),
        LayerRole::Mask => xml.push_str(" layerType=\"mask\""),
        LayerRole::Folder => xml.push_str(" layerType=\"folder\""),
    }
    if let Some(p_idx) = parent_layer_index {
        write!(xml, " parentLayerIndex=\"{p_idx}\"").unwrap();
    }
    if !layer.visible {
        xml.push_str(" visible=\"false\"");
    }
    if layer.locked {
        xml.push_str(" locked=\"true\"");
    }
    if layer.properties.outline {
        xml.push_str(" outline=\"true\"");
    }
    if layer.properties.role == LayerRole::Folder && layer.properties.collapsed {
        xml.push_str(" open=\"false\"");
    }
    xml.push_str(">\n");

    if layer.frames.is_empty() {
        writeln!(xml, "{indent}  <frames/>").unwrap();
    } else {
        writeln!(xml, "{indent}  <frames>").unwrap();
        let frame_indent = format!("{indent}    ");
        for frame in &layer.frames {
            render_dom_frame(xml, &frame_indent, frame, ctx);
        }
        writeln!(xml, "{indent}  </frames>").unwrap();
    }
    writeln!(xml, "{indent}</DOMLayer>").unwrap();
}

pub(super) fn render_dom_frame(
    xml: &mut String,
    indent: &str,
    frame: &PreparedFrame,
    ctx: &ExportContext,
) {
    write!(xml, "{indent}<DOMFrame index=\"{}\"", frame.index).unwrap();
    if frame.duration > 1 {
        write!(xml, " duration=\"{}\"", frame.duration).unwrap();
    }
    let label_type_str = match frame.label_type {
        FrameLabelType::Name => "name",
        FrameLabelType::Comment => "comment",
        FrameLabelType::Anchor => "anchor",
    };
    if !frame.label.is_empty() {
        write!(
            xml,
            " name=\"{}\" labelType=\"{label_type_str}\"",
            escape_xml_attr(&frame.label)
        )
        .unwrap();
    } else if !frame.label_type.is_default() {
        write!(xml, " labelType=\"{label_type_str}\"").unwrap();
    }

    let mut custom_ease_bezier: Option<[f64; 4]> = None;
    if let Some((ease, kind, orient_to_path)) = frame.tween {
        let tween_type = match kind {
            TweenKind::Shape => "shape",
            TweenKind::Classic | TweenKind::Motion => "motion",
        };
        write!(xml, " tweenType=\"{tween_type}\"").unwrap();
        match ease {
            Easing::Linear => {}
            Easing::Flash { acceleration } => {
                let clamped = acceleration.clamp(-100.0, 100.0);
                if clamped.abs() > 1e-9 {
                    write!(xml, " acceleration=\"{}\"", fmt_num(clamped)).unwrap();
                }
            }
            Easing::EaseIn => {
                xml.push_str(" acceleration=\"-100\"");
            }
            Easing::EaseOut => {
                xml.push_str(" acceleration=\"100\"");
            }
            Easing::EaseInOut => {
                xml.push_str(" hasCustomEase=\"true\" useSingleEaseCurve=\"true\"");
                custom_ease_bezier = Some([0.42, 0.0, 0.58, 1.0]);
            }
            Easing::Bounce | Easing::Elastic => {
                xml.push_str(" hasCustomEase=\"true\" useSingleEaseCurve=\"true\"");
                custom_ease_bezier = Some([0.34, 1.56, 0.64, 1.0]);
            }
            Easing::CubicBezier { x1, y1, x2, y2 } => {
                xml.push_str(" hasCustomEase=\"true\" useSingleEaseCurve=\"true\"");
                custom_ease_bezier = Some([
                    x1.clamp(0.0, 1.0),
                    y1.clamp(-100.0, 100.0),
                    x2.clamp(0.0, 1.0),
                    y2.clamp(-100.0, 100.0),
                ]);
            }
        }
        if orient_to_path && tween_type == "motion" {
            xml.push_str(" motionTweenOrientToPath=\"true\"");
        }
    }

    let mut sound_envelope_points: Option<Vec<(u64, u32, u32)>> = None;
    if let Some(snd) = &frame.sound
        && let Some(snd_asset) = ctx.sounds.get(&snd.resource)
    {
        write!(
            xml,
            " soundName=\"{}\"",
            escape_xml_attr(&snd_asset.item_name)
        )
        .unwrap();
        let sync_str = match snd.sync {
            SoundSync::Event => "event",
            SoundSync::Start => "start",
            SoundSync::Stop => "stop",
            SoundSync::Stream => "stream",
        };
        write!(xml, " soundSync=\"{sync_str}\"").unwrap();
        if snd.loop_forever {
            xml.push_str(" soundLoopMode=\"loop\"");
        } else if snd.loops > 1 {
            write!(
                xml,
                " soundLoopMode=\"repeat\" soundLoop=\"{}\"",
                snd.loops.min(FrameSound::MAX_LOOPS)
            )
            .unwrap();
        }
        if let Some(in_sec) = snd.in_point
            && in_sec.is_finite()
            && in_sec > 0.0
        {
            let in_44 =
                (in_sec.clamp(0.0, FrameSound::MAX_SOUND_TRIM_SECONDS) * 44_100.0).round() as u64;
            if in_44 > 0 {
                write!(xml, " inPoint44=\"{in_44}\"").unwrap();
            }
        }
        if let Some(out_sec) = snd.out_point
            && out_sec.is_finite()
            && out_sec > 0.0
        {
            let out_44 =
                (out_sec.clamp(0.0, FrameSound::MAX_SOUND_TRIM_SECONDS) * 44_100.0).round() as u64;
            if out_44 > 0 {
                write!(xml, " outPoint44=\"{out_44}\"").unwrap();
            }
        }
        let has_custom_env = snd.effect == SoundEffect::Custom
            || snd.envelope.as_ref().is_some_and(|v| !v.is_empty());
        let effect_str = if has_custom_env {
            "custom"
        } else {
            match snd.effect {
                SoundEffect::None => "none",
                SoundEffect::LeftChannel => "left channel",
                SoundEffect::RightChannel => "right channel",
                SoundEffect::FadeLeftToRight => "fade left to right",
                SoundEffect::FadeRightToLeft => "fade right to left",
                SoundEffect::FadeIn => "fade in",
                SoundEffect::FadeOut => "fade out",
                SoundEffect::Custom => "custom",
            }
        };
        if effect_str != "none" {
            write!(xml, " soundEffect=\"{effect_str}\"").unwrap();
        }
        if has_custom_env {
            let pts = encode_sound_envelope_points(snd, snd_asset.duration_44);
            if !pts.is_empty() {
                sound_envelope_points = Some(pts);
            }
        }
    }

    if custom_ease_bezier.is_none()
        && sound_envelope_points.is_none()
        && frame.script.is_none()
        && frame.elements_xml.is_empty()
    {
        xml.push_str("/>\n");
        return;
    }

    xml.push_str(">\n");
    if let Some([x1, y1, x2, y2]) = custom_ease_bezier {
        writeln!(xml, "{indent}  <tweens>").unwrap();
        writeln!(xml, "{indent}    <CustomEase target=\"all\">").unwrap();
        writeln!(xml, "{indent}      <Point x=\"0\" y=\"0\"/>").unwrap();
        writeln!(
            xml,
            "{indent}      <Point x=\"{}\" y=\"{}\"/>",
            fmt_num(x1),
            fmt_num(y1)
        )
        .unwrap();
        writeln!(
            xml,
            "{indent}      <Point x=\"{}\" y=\"{}\"/>",
            fmt_num(x2),
            fmt_num(y2)
        )
        .unwrap();
        writeln!(xml, "{indent}      <Point x=\"1\" y=\"1\"/>").unwrap();
        writeln!(xml, "{indent}    </CustomEase>").unwrap();
        writeln!(xml, "{indent}  </tweens>").unwrap();
    }

    if let Some(env_pts) = sound_envelope_points {
        writeln!(xml, "{indent}  <SoundEnvelope>").unwrap();
        for (mark44, level0, level1) in env_pts {
            writeln!(
                xml,
                "{indent}    <SoundEnvelopePoint mark44=\"{mark44}\" level0=\"{level0}\" level1=\"{level1}\"/>"
            )
            .unwrap();
        }
        writeln!(xml, "{indent}  </SoundEnvelope>").unwrap();
    }

    if let Some(script_src) = &frame.script {
        writeln!(xml, "{indent}  <Actionscript>").unwrap();
        writeln!(
            xml,
            "{indent}    <script>{}</script>",
            cdata_wrap(script_src)
        )
        .unwrap();
        writeln!(xml, "{indent}  </Actionscript>").unwrap();
    }

    if !frame.elements_xml.is_empty() {
        writeln!(xml, "{indent}  <elements>").unwrap();
        for item_xml in &frame.elements_xml {
            let mut in_cdata = false;
            for line in item_xml.lines() {
                if in_cdata {
                    writeln!(xml, "{line}").unwrap();
                } else {
                    writeln!(xml, "{indent}    {line}").unwrap();
                }
                if let Some(open_pos) = line.rfind("<![CDATA[") {
                    in_cdata = !line[open_pos + 9..].contains("]]>");
                } else if in_cdata && line.contains("]]>") {
                    in_cdata = false;
                }
            }
        }
        writeln!(xml, "{indent}  </elements>").unwrap();
    }
    writeln!(xml, "{indent}</DOMFrame>").unwrap();
}

pub(super) fn encode_sound_envelope_points(
    snd: &FrameSound,
    duration_44: u64,
) -> Vec<(u64, u32, u32)> {
    let raw = snd.envelope_points();
    let denom = if duration_44 > 0 { duration_44 } else { 44_100 };
    let mut encoded: Vec<(u64, u32, u32)> = raw
        .into_iter()
        .take(FrameSound::MAX_ENVELOPE_POINTS)
        .map(|[pos, left, right]| {
            let p = if pos.is_finite() {
                pos.clamp(0.0, 1.0)
            } else {
                0.0
            };
            let l = if left.is_finite() {
                left.clamp(0.0, 1.0)
            } else {
                1.0
            };
            let r = if right.is_finite() {
                right.clamp(0.0, 1.0)
            } else {
                1.0
            };
            let mark44 = (p * denom as f64).round() as u64;
            let level0 = (l * 32_768.0).round() as u32;
            let level1 = (r * 32_768.0).round() as u32;
            (mark44, level0, level1)
        })
        .collect();
    encoded.sort_by_key(|pt| pt.0);
    encoded
}
