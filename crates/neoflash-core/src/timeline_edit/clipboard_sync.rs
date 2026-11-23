use super::motion_keys::*;
use super::*;

pub fn copy_frames(
    project: &Project,
    symbol: &str,
    range: &FrameRange,
    include_scripts: bool,
) -> Result<FrameClipboard, String> {
    let fps = f64::from(project.fps);
    let timeline = &project
        .symbol(symbol)
        .ok_or("The edited symbol no longer exists.")?
        .timeline;
    let end = range_end(timeline, range)?;
    let indices = layer_indices(timeline, &range.layers)?;
    let mut budget = 0;
    let mut layers = Vec::with_capacity(indices.len());
    for index in indices {
        let mut layer = timeline.layers[index].clone();
        let effective_end = exposure(&layer, timeline.frame_count);
        let original = layer.clone();
        let (_, head_video) = video_cut_on_layer(&original, effective_end, range.start, fps)?;
        if end < effective_end {
            let _ = video_cut_on_layer(&original, effective_end, end, fps)?;
        }
        let clip_end = effective_end.min(end).saturating_sub(range.start);
        let preserve_curves = original.properties.motion_guide.is_none()
            && clip_end >= 2
            && original.keyframes.iter().any(has_object_motion_path)
            && !original
                .keyframes
                .iter()
                .any(|k| k.property_expressions.iter().any(|e| e.enabled));
        let keyframes = if preserve_curves {
            isolate_range(
                timeline,
                &mut layer,
                range.start,
                range.start,
                &mut budget,
                fps,
            )?;
            if end < effective_end {
                isolate_range(timeline, &mut layer, end, end, &mut budget, fps)?;
            }
            split(timeline, &mut layer, range.start, fps);
            let mut extracted: Vec<Keyframe> = layer
                .keyframes
                .into_iter()
                .filter(|key| key.frame >= range.start && key.frame < end)
                .map(|mut key| {
                    if key.frame == range.start {
                        if head_video.is_some() {
                            key.video = head_video.clone();
                        }
                    } else if key.keyed_properties.is_none()
                        && let Some(orig_key) = original
                            .keyframes
                            .iter()
                            .find(|k| k.frame == key.frame && k.keyed_properties.is_none())
                        && orig_key.video.is_some()
                    {
                        key.video = orig_key.video.clone();
                    }
                    key.frame -= range.start;
                    key
                })
                .collect();
            let mut temp = Layer {
                id: layer.id.clone(),
                name: layer.name.clone(),
                visible: true,
                locked: false,
                keyframes: std::mem::take(&mut extracted),
                properties: LayerProperties {
                    end_frame: Some(clip_end),
                    ..Default::default()
                },
            };
            temp.normalize_motion_keys();
            temp.keyframes
        } else {
            layer.properties.end_frame = Some(effective_end);
            bake_touched(timeline, &mut layer, range.start, end, &mut budget, fps)?;
            split(timeline, &mut layer, range.start, fps);
            layer
                .keyframes
                .into_iter()
                .filter(|key| key.frame >= range.start && key.frame < end)
                .map(|mut key| {
                    if key.frame == range.start {
                        if head_video.is_some() {
                            key.video = head_video.clone();
                        }
                    } else if let Some(orig_key) = original
                        .keyframes
                        .iter()
                        .find(|k| k.frame == key.frame && k.keyed_properties.is_none())
                        && orig_key.video.is_some()
                    {
                        key.video = orig_key.video.clone();
                    }
                    key.frame -= range.start;
                    key.tween = None;
                    key
                })
                .collect()
        };
        layers.push(FrameLayerClip {
            source_layer: layer.id,
            keyframes,
            end_frame: clip_end,
        });
    }
    let scripts = if include_scripts {
        timeline
            .scripts
            .iter()
            .filter(|script| {
                script.frame >= range.start
                    && script.frame < end
                    && script_follows_layers(script, &range.layers)
            })
            .cloned()
            .map(|mut script| {
                script.frame -= range.start;
                script
            })
            .collect()
    } else {
        vec![]
    };
    Ok(FrameClipboard {
        frame_count: end - range.start,
        layers,
        scripts,
        includes_scripts: include_scripts,
    })
}
pub(super) fn clear_rows(
    timeline: &mut Timeline,
    range: &FrameRange,
    budget: &mut usize,
    fps: f64,
) -> Result<(), String> {
    let end = range_end(timeline, range)?;
    let fallback_count = timeline.frame_count;
    for index in layer_indices(timeline, &range.layers)? {
        let mut layer = timeline.layers[index].clone();
        let effective_end = exposure(&layer, fallback_count);
        let original = layer.clone();
        let (left_split, _) = video_cut_on_layer(&original, effective_end, range.start, fps)?;
        let (_, tail_video) = video_cut_on_layer(&original, effective_end, end, fps)?;
        isolate_range(timeline, &mut layer, range.start, end, budget, fps)?;
        split(timeline, &mut layer, end, fps);
        if let Some((left_frame, left_video)) = left_split
            && let Some(left_key) = layer
                .keyframes
                .iter_mut()
                .find(|k| k.frame == left_frame && k.keyed_properties.is_none())
        {
            left_key.video = Some(left_video);
        }
        if let Some(right_video) = tail_video
            && let Some(right_key) = layer
                .keyframes
                .iter_mut()
                .find(|k| k.frame == end && k.keyed_properties.is_none())
        {
            right_key.video = Some(right_video);
        }
        layer
            .keyframes
            .retain(|key| key.frame < range.start || key.frame >= end);
        if range.start < effective_end {
            layer.keyframes.push(blank(range.start));
        }
        layer.keyframes.sort_by_key(|key| key.frame);
        timeline.layers[index] = layer;
    }
    Ok(())
}
pub(super) fn remove_scripts(timeline: &mut Timeline, range: &FrameRange) {
    timeline.scripts.retain(|script| {
        script.frame < range.start
            || script.frame > range.end
            || !script_follows_layers(script, &range.layers)
    });
}
pub(super) fn paste_rows(
    project: &mut Project,
    symbol: &str,
    clip: &FrameClipboard,
    layers: &[Id],
    at: u32,
    include_scripts: bool,
    budget: &mut usize,
) -> Result<(), String> {
    let fps = f64::from(project.fps);
    let end = at
        .checked_add(clip.frame_count)
        .filter(|end| *end <= MAX_FRAME)
        .ok_or("Pasted frames exceed the timeline limit.")?;
    if clip.frame_count == 0 || clip.layers.len() != layers.len() {
        return Err(
            "Paste requires the same number of destination artwork layers as copied rows.".into(),
        );
    }
    if clip
        .scripts
        .iter()
        .any(|script| script.frame >= clip.frame_count)
    {
        return Err("Frame clipboard contains a script outside its copied range.".into());
    }
    if include_scripts && !clip.includes_scripts {
        return Err("This clipboard was copied without frame scripts. Copy again with Include frame scripts enabled.".into());
    }
    let timeline = &project
        .symbol(symbol)
        .ok_or("The edited symbol no longer exists.")?
        .timeline;
    let indices = layer_indices(timeline, layers)?;
    let script_layer_map: HashMap<Id, Id> = clip
        .layers
        .iter()
        .zip(&indices)
        .map(|(source, &index)| {
            (
                source.source_layer.clone(),
                timeline.layers[index].id.clone(),
            )
        })
        .collect();
    if include_scripts
        && clip.scripts.iter().any(|script| {
            script
                .layer
                .as_ref()
                .is_some_and(|owner| !script_layer_map.contains_key(owner))
        })
    {
        return Err(
            "Frame clipboard contains a script whose owning layer was not copied; no frames or code were pasted."
                .into(),
        );
    }
    if include_scripts
        && clip.scripts.iter().any(|incoming| {
            let incoming_layer = incoming
                .layer
                .as_ref()
                .and_then(|layer| script_layer_map.get(layer));
            timeline.scripts.iter().any(|existing| {
                existing.frame == at + incoming.frame
                    && match (existing.layer.as_ref(), incoming_layer) {
                        (Some(existing), Some(incoming)) => existing == incoming,
                        // A legacy layerless record has no safe independent destination.
                        _ => true,
                    }
            })
        })
    {
        return Err("A destination layer/frame already has code, or legacy layerless code makes the destination global. Move it first or paste without frame scripts; no code was replaced.".into());
    }
    let mut owners: HashMap<Id, Id> = project
        .symbols
        .iter()
        .flat_map(|s| &s.timeline.layers)
        .flat_map(|layer| {
            layer.keyframes.iter().flat_map(move |key| {
                key.elements
                    .iter()
                    .map(move |e| (e.id.clone(), layer.id.clone()))
            })
        })
        .collect();
    let mut all_ids: HashSet<Id> = owners.keys().cloned().collect();
    for source in &clip.layers {
        all_ids.extend(
            source
                .keyframes
                .iter()
                .flat_map(|key| key.elements.iter().map(|e| e.id.clone())),
        );
    }
    let timeline = &mut project.symbol_mut(symbol).unwrap().timeline;
    let prev_count = timeline.frame_count;
    timeline.frame_count = timeline.frame_count.max(end);
    for (source, index) in clip.layers.iter().zip(indices) {
        if source.end_frame > clip.frame_count
            || source
                .keyframes
                .iter()
                .any(|key| key.frame >= source.end_frame)
            || source
                .keyframes
                .windows(2)
                .any(|keys| keys[0].frame >= keys[1].frame)
        {
            return Err(
                "Frame clipboard has invalid exposure bounds or duplicate/unordered keys.".into(),
            );
        }
        let mut layer = timeline.layers[index].clone();
        let old_end = exposure(&layer, prev_count);
        let original = layer.clone();
        let (left_split, _) = video_cut_on_layer(&original, old_end, at, fps)?;
        let (_, tail_video) = video_cut_on_layer(&original, old_end, end, fps)?;
        isolate_range(timeline, &mut layer, at, end, budget, fps)?;
        split(timeline, &mut layer, end, fps);
        if let Some((left_frame, left_video)) = left_split
            && let Some(left_key) = layer
                .keyframes
                .iter_mut()
                .find(|k| k.frame == left_frame && k.keyed_properties.is_none())
        {
            left_key.video = Some(left_video);
        }
        if let Some(right_video) = tail_video
            && let Some(right_key) = layer
                .keyframes
                .iter_mut()
                .find(|k| k.frame == end && k.keyed_properties.is_none())
        {
            right_key.video = Some(right_video);
        }
        layer
            .keyframes
            .retain(|key| key.frame < at || key.frame >= end);
        // A paste beyond an existing end must leave the intervening gap empty.
        if at > old_end {
            layer.keyframes.push(blank(old_end));
        }
        let mut remap = HashMap::new();
        for element in source.keyframes.iter().flat_map(|key| &key.elements) {
            if owners
                .get(&element.id)
                .is_some_and(|owner| owner != &layer.id)
            {
                remap.entry(element.id.clone()).or_insert_with(|| {
                    let mut id = new_id("object");
                    while all_ids.contains(&id) {
                        id = new_id("object");
                    }
                    all_ids.insert(id.clone());
                    owners.insert(id.clone(), layer.id.clone());
                    id
                });
            } else {
                owners.insert(element.id.clone(), layer.id.clone());
            }
        }
        for key in &source.keyframes {
            let mut key = key.clone();
            key.frame += at;
            key.remap_all_ids(&remap, None, None);
            layer.keyframes.push(key);
        }
        let pasted_end = at + source.end_frame;
        let new_end = old_end.max(pasted_end);
        // Empty copied rows must clear their whole rectangle, not keep the old
        // preceding exposure alive. Keep later destination material untouched.
        if at < new_end && source.keyframes.first().is_none_or(|key| key.frame != 0) {
            layer.keyframes.push(blank(at));
        }
        if pasted_end < end && pasted_end < new_end {
            layer.keyframes.push(blank(pasted_end));
        }
        layer.properties.end_frame = Some(new_end);
        layer.keyframes.retain(|key| key.frame < new_end);
        layer.keyframes.sort_by_key(|key| key.frame);
        layer.keyframes.dedup_by_key(|key| key.frame);
        timeline.layers[index] = layer;
    }
    if include_scripts {
        timeline
            .scripts
            .extend(clip.scripts.iter().cloned().map(|mut script| {
                script.frame += at;
                if let Some(source_owner) = script.layer.take() {
                    // Preflight above guarantees a destination owner. Preserve the explicit
                    // source ID even if that invariant is changed later; never degrade an owned
                    // script into the legacy layerless representation.
                    script.layer = Some(
                        script_layer_map
                            .get(&source_owner)
                            .cloned()
                            .unwrap_or(source_owner),
                    );
                }
                script
            }));
        timeline.scripts.sort_by_key(|script| script.frame);
    }
    Ok(())
}
/// Applies a complete operation to a private candidate. Errors never leave a
/// partial document, changed IDs, deleted code, or an incomplete multi-row edit.
/// Playback first frame continuing `previous` (placed at `previous_start`) at `frame`, for an
/// instance placed at `start`; `None` when the playback mode has no first frame to adjust.
pub(super) fn continued_playback(
    previous: InstancePlayback,
    previous_start: u32,
    current: InstancePlayback,
    start: u32,
    frame: u32,
    count: u32,
) -> Option<InstancePlayback> {
    let first = match previous {
        InstancePlayback::Loop | InstancePlayback::PlayOnce => 0,
        InstancePlayback::LoopFrom(first) | InstancePlayback::PlayOnceFrom(first) => first,
        InstancePlayback::Independent | InstancePlayback::SingleFrame(_) => return None,
    };
    // Unwrapped nested frame shown by the previous placement at `frame`, minus the age the
    // current placement already has there.
    let phase = u64::from(first) + u64::from(frame.saturating_sub(previous_start));
    let age = u64::from(frame.saturating_sub(start));
    let count = u64::from(count.max(1));
    let continued = match current {
        InstancePlayback::Loop | InstancePlayback::LoopFrom(_) => {
            (phase + count - age % count) % count
        }
        InstancePlayback::PlayOnce | InstancePlayback::PlayOnceFrom(_) => {
            phase.saturating_sub(age).min(count - 1)
        }
        InstancePlayback::Independent | InstancePlayback::SingleFrame(_) => return None,
    } as u32;
    Some(match current {
        InstancePlayback::Loop | InstancePlayback::LoopFrom(_) if continued == 0 => {
            InstancePlayback::Loop
        }
        InstancePlayback::Loop | InstancePlayback::LoopFrom(_) => {
            InstancePlayback::LoopFrom(continued)
        }
        _ if continued == 0 => InstancePlayback::PlayOnce,
        _ => InstancePlayback::PlayOnceFrom(continued),
    })
}

/// Synchronize graphic symbol playback on `layer` across `start..=end` without requiring a change.
pub fn sync_layer_symbols(project: &Project, layer: &mut Layer, start: u32, end: u32) -> usize {
    let symbols: HashMap<Id, (SymbolKind, u32)> = project
        .symbols
        .iter()
        .map(|s| (s.id.clone(), (s.kind, s.timeline.frame_count.max(1))))
        .collect();
    sync_symbols(layer, start, end, &symbols, None)
}

/// Synchronize one graphic symbol placement without changing sibling instances on the layer.
pub fn sync_layer_symbol(
    project: &Project,
    layer: &mut Layer,
    start: u32,
    end: u32,
    element_id: &str,
) -> usize {
    let symbols: HashMap<Id, (SymbolKind, u32)> = project
        .symbols
        .iter()
        .map(|symbol| {
            (
                symbol.id.clone(),
                (symbol.kind, symbol.timeline.frame_count.max(1)),
            )
        })
        .collect();
    sync_symbols(layer, start, end, &symbols, Some(element_id))
}

/// Synchronise symbols on one layer: keyframes in `start..=end` (in frame order) continue the
/// graphic instances of the keyframe before each of them. Returns the number of changed instances.
pub(super) fn sync_symbols(
    layer: &mut Layer,
    start: u32,
    end: u32,
    symbols: &HashMap<Id, (SymbolKind, u32)>,
    target_element: Option<&str>,
) -> usize {
    layer.keyframes.sort_by_key(|key| key.frame);
    let mut changed = 0;
    for index in 1..layer.keyframes.len() {
        let frame = layer.keyframes[index].frame;
        if frame < start || frame > end {
            continue;
        }
        let previous = layer.keyframes[index - 1].clone();
        for element_index in 0..layer.keyframes[index].elements.len() {
            let element = &layer.keyframes[index].elements[element_index];
            if target_element.is_some_and(|target| element.id != target) {
                continue;
            }
            let ElementKind::Instance {
                symbol_id,
                playback,
            } = &element.kind
            else {
                continue;
            };
            let Some(&(sym_kind, count)) = symbols.get(symbol_id) else {
                continue;
            };
            if element.instance_behavior.unwrap_or(sym_kind) != SymbolKind::Graphic {
                continue;
            }
            let is_graphic_of_symbol = |candidate: &Element| {
                matches!(&candidate.kind, ElementKind::Instance { symbol_id: s, .. } if s == symbol_id)
                    && candidate.instance_behavior.unwrap_or(sym_kind) == SymbolKind::Graphic
            };
            let same_id_on_previous = previous
                .elements
                .iter()
                .find(|candidate| candidate.id == element.id);
            let Some(before) = (if let Some(exact) = same_id_on_previous {
                is_graphic_of_symbol(exact).then_some(exact)
            } else if target_element.is_some() {
                let ordinal = layer.keyframes[index].elements[..=element_index]
                    .iter()
                    .filter(|candidate| is_graphic_of_symbol(candidate))
                    .count()
                    .saturating_sub(1);
                previous
                    .elements
                    .iter()
                    .filter(|candidate| is_graphic_of_symbol(candidate))
                    .nth(ordinal)
            } else {
                previous
                    .elements
                    .iter()
                    .find(|candidate| is_graphic_of_symbol(candidate))
            }) else {
                continue;
            };
            let ElementKind::Instance {
                playback: before_playback,
                ..
            } = &before.kind
            else {
                continue;
            };
            let previous_start =
                crate::evaluate::placement_start(layer, previous.frame, &before.id);
            let start = crate::evaluate::placement_start(layer, frame, &element.id);
            let Some(next) = continued_playback(
                *before_playback,
                previous_start,
                *playback,
                start,
                frame,
                count,
            ) else {
                continue;
            };
            if next != *playback {
                if let ElementKind::Instance { playback, .. } =
                    &mut layer.keyframes[index].elements[element_index].kind
                {
                    *playback = next;
                }
                changed += 1;
            }
        }
    }
    changed
}

/// Converts frames in `start..=end` on `layer` to keyframes at every `step` frames
/// (`start, start + step, start + 2*step, ...` up to `end`), sampling the evaluated poses
/// via `sample_layer_in_at_fps`, removing intermediate keyframes inside each `step` interval,
/// and either stripping tween/motion metadata (`strip_tween = true`, ToonMonkey `Tween2Keys`)
/// or preserving tween easing (`strip_tween = false`, Convert to Keyframes Advanced).
#[allow(dead_code)]
pub fn convert_layer_to_keyframes_stepped(
    timeline: &Timeline,
    layer: &mut Layer,
    start: u32,
    end: u32,
    step: u32,
    strip_tween: bool,
    fps: f64,
) -> usize {
    let step = step.max(1);
    if start > end || end >= MAX_FRAME {
        return 0;
    }
    layer.keyframes.sort_by_key(|key| key.frame);
    let original = layer.clone();
    let effective_end = original
        .properties
        .end_frame
        .unwrap_or(timeline.frame_count)
        .max(end + 1);
    if layer.properties.end_frame.is_some() {
        layer.properties.end_frame = Some(effective_end);
    }

    let mut stepped_frames: Vec<u32> = Vec::new();
    let mut f = start;
    while f <= end {
        stepped_frames.push(f);
        match f.checked_add(step) {
            Some(next) if next > f => f = next,
            _ => break,
        }
    }
    if stepped_frames.last().copied() != Some(end)
        && original
            .keyframes
            .iter()
            .any(|k| k.frame == end && k.keyed_properties.is_none())
    {
        stepped_frames.push(end);
    }

    let active_ease_at = |frame: u32| -> Option<(Easing, TweenKind)> {
        if let Some((span_start, _, span_end)) = motion_span(&original, frame) {
            let end_limit = span_end.unwrap_or(effective_end);
            if frame < end_limit {
                return span_start.tween.map(|ease| (ease, TweenKind::Classic));
            }
        }
        let idx = original.keyframes.partition_point(|k| k.frame <= frame);
        let key = idx.checked_sub(1).and_then(|i| original.keyframes.get(i))?;
        let next_frame = original
            .keyframes
            .get(idx)
            .map_or(effective_end, |next| next.frame);
        if frame < next_frame {
            key.tween.map(|ease| {
                let kind = if key.tween_kind == TweenKind::Shape {
                    TweenKind::Shape
                } else {
                    TweenKind::Classic
                };
                (ease, kind)
            })
        } else {
            None
        }
    };

    let mut replacement = Vec::with_capacity(stepped_frames.len());
    for (idx, &frame) in stepped_frames.iter().enumerate() {
        let mut key = sampled_key(timeline, &original, frame, fps);
        if let Some(old) = original.keyframes.iter().find(|k| k.frame == frame) {
            key.label = old.label.clone();
            key.sound = old.sound.clone();
        }
        if strip_tween {
            key.tween = None;
            key.tween_kind = TweenKind::Classic;
            key.keyed_properties = None;
            key.motion_keys = None;
            key.motion_tracks.clear();
            key.motion_path = None;
            key.property_eases.clear();
            key.shape_hints.clear();
        } else {
            let is_last_in_range = idx + 1 == stepped_frames.len() && frame == end;
            if is_last_in_range {
                if let Some(old_end) = original
                    .keyframes
                    .iter()
                    .find(|k| k.frame == frame && k.keyed_properties.is_none())
                {
                    key.tween = old_end.tween;
                    key.tween_kind =
                        if key.tween.is_some() && old_end.tween_kind == TweenKind::Shape {
                            TweenKind::Shape
                        } else {
                            TweenKind::Classic
                        };
                } else if let Some((ease, kind)) = active_ease_at(frame) {
                    key.tween = Some(ease);
                    key.tween_kind = kind;
                }
            } else if let Some((ease, kind)) = active_ease_at(frame) {
                key.tween = Some(ease);
                key.tween_kind = kind;
            }
            key.keyed_properties = None;
            key.motion_keys = None;
            key.motion_tracks.clear();
            key.motion_path = None;
            key.property_eases.clear();
        }
        replacement.push(key);
    }

    // Preserve pose immediately after `end` if the original layer was mid-tween across `end + 1`
    // and we stripped tweens inside `start..=end`.
    if strip_tween
        && end + 1 < effective_end
        && !original.keyframes.iter().any(|k| k.frame == end + 1)
        && active_ease_at(end).is_some()
    {
        let mut after_key = sampled_key(timeline, &original, end + 1, fps);
        if let Some((ease, kind)) = active_ease_at(end + 1) {
            after_key.tween = Some(ease);
            after_key.tween_kind = kind;
        }
        replacement.push(after_key);
    }

    layer
        .keyframes
        .retain(|key| key.frame < start || key.frame > end);
    layer.keyframes.extend(replacement);
    layer.keyframes.sort_by_key(|key| key.frame);
    layer.keyframes.dedup_by_key(|key| key.frame);
    layer.normalize_motion_keys();
    stepped_frames.len()
}
