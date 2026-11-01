//! Atomic authoring edits over rectangular frame ranges. Frame scripts belong to
//! the symbol, so their inclusion is always explicit and never repeated per row.
use crate::*;
use std::collections::{HashMap, HashSet};

const MAX_FRAME: u32 = 1_000_000;
const MAX_BAKED_RECORDS: usize = 100_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameRange {
    pub layers: Vec<Id>,
    pub start: u32,
    /// Inclusive last selected frame.
    pub end: u32,
}
#[derive(Clone, Debug, PartialEq)]
pub struct FrameClipboard {
    pub frame_count: u32,
    pub layers: Vec<FrameLayerClip>,
    pub scripts: Vec<FrameScript>,
    pub includes_scripts: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub struct FrameLayerClip {
    pub source_layer: Id,
    /// Relative frame coordinates, starting with the sampled first frame.
    pub keyframes: Vec<Keyframe>,
    /// Exclusive end within the copied rectangle; zero means empty exposure.
    pub end_frame: u32,
}
#[derive(Clone, Debug)]
pub enum TimelineEdit {
    /// Clear contents without shifting later frames. Used by Cut after copying.
    Clear {
        range: FrameRange,
        include_scripts: bool,
    },
    /// Remove time from selected rows, shifting their later frames left.
    Delete {
        range: FrameRange,
        include_scripts: bool,
    },
    /// Insert held time before `at`, shifting later frames on selected rows right.
    Insert {
        layers: Vec<Id>,
        at: u32,
        count: u32,
        include_scripts: bool,
    },
    Paste {
        clip: FrameClipboard,
        layers: Vec<Id>,
        at: u32,
        include_scripts: bool,
    },
    Move {
        range: FrameRange,
        layers: Vec<Id>,
        at: u32,
        copy: bool,
        include_scripts: bool,
    },
    /// Explicitly trim or extend exposure; trimming discards keys at/after end.
    SetEnd { layers: Vec<Id>, end_frame: u32 },
    /// Mirror the selected frames in time (Modify ▸ Timeline ▸ Reverse frames).
    /// Content before and after the range is unchanged.
    Reverse { range: FrameRange },
    /// Make every selected frame a keyframe holding its visible pose, or a
    /// blank keyframe (Convert to keyframes / Convert to blank keyframes).
    ConvertToKeyframes { range: FrameRange, blank: bool },
    /// Synchronise symbols: every graphic instance on a selected keyframe continues the nested
    /// timeline of the same instance (or the same symbol) on the keyframe before it, so the
    /// nested animation stays in step across keyframe splits and tweens.
    SyncSymbols { range: FrameRange },
}
fn layer_indices(timeline: &Timeline, ids: &[Id]) -> Result<Vec<usize>, String> {
    if ids.is_empty() {
        return Err("Select at least one artwork layer.".into());
    }
    let mut seen = HashSet::new();
    ids.iter()
        .map(|id| {
            if !seen.insert(id) {
                return Err("A frame range cannot repeat a layer.".into());
            }
            let index = timeline
                .layers
                .iter()
                .position(|layer| &layer.id == id)
                .ok_or_else(|| "A selected layer no longer exists.".to_owned())?;
            let layer = &timeline.layers[index];
            if layer.properties.role == LayerRole::Folder {
                return Err("Select artwork rows, not a folder row.".into());
            }
            if !layer_effectively_visible(timeline, layer)
                || layer_effectively_locked(timeline, layer)
            {
                return Err(format!(
                    "Layer '{}' is hidden or locked. Show and unlock it before editing frames.",
                    layer.name
                ));
            }
            Ok(index)
        })
        .collect()
}
fn range_end(timeline: &Timeline, range: &FrameRange) -> Result<u32, String> {
    if range.start > range.end || range.end >= timeline.frame_count {
        return Err("The frame range is outside the timeline.".into());
    }
    Ok(range.end + 1)
}
fn exposure(layer: &Layer, count: u32) -> u32 {
    layer.properties.end_frame.unwrap_or(count)
}
fn blank(frame: u32) -> Keyframe {
    Keyframe {
        frame,
        elements: vec![],
        tween: None,
        label: String::new(),
        ..Default::default()
    }
}
fn sampled_key(layer: &Layer, frame: u32) -> Keyframe {
    let mut key = blank(frame);
    key.elements = sample_layer(layer, frame);
    if let Some(original) = layer.keyframes.iter().find(|key| key.frame == frame) {
        key.label = original.label.clone();
    }
    key
}
/// An edit through a tween must not alter the curve outside the edited span.
/// Bake only touched tween segments into exact integer-frame poses. This keeps
/// held spans sparse and avoids approximating a sliced nonlinear easing curve.
fn bake_touched(layer: &mut Layer, start: u32, end: u32, budget: &mut usize) -> Result<(), String> {
    layer.keyframes.sort_by_key(|key| key.frame);
    let original = layer.clone();
    let segments: Vec<_> = original
        .keyframes
        .windows(2)
        .filter(|pair| pair[0].tween.is_some() && pair[0].frame < end && pair[1].frame > start)
        .map(|pair| (pair[0].frame, pair[1].frame))
        .collect();
    for (first, last) in segments {
        let records = (last - first) as usize
            * original
                .key_at(first)
                .map_or(1, |key| key.elements.len().max(1));
        *budget = budget
            .checked_add(records)
            .ok_or("Frame edit is too large.")?;
        if *budget > MAX_BAKED_RECORDS {
            return Err("This edit would bake more than 100000 frame/object records. Split the tween into shorter sections first.".into());
        }
        layer
            .keyframes
            .retain(|key| key.frame < first || key.frame >= last);
        layer
            .keyframes
            .extend((first..last).map(|frame| sampled_key(&original, frame)));
    }
    layer.keyframes.sort_by_key(|key| key.frame);
    Ok(())
}
fn split(layer: &mut Layer, frame: u32) {
    if frame >= layer.properties.end_frame.unwrap_or(MAX_FRAME)
        || layer.keyframes.iter().any(|key| key.frame == frame)
    {
        return;
    }
    let key = sampled_key(layer, frame);
    layer.keyframes.push(key);
    layer.keyframes.sort_by_key(|key| key.frame);
}
pub fn copy_frames(
    project: &Project,
    symbol: &str,
    range: &FrameRange,
    include_scripts: bool,
) -> Result<FrameClipboard, String> {
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
        layer.properties.end_frame = Some(effective_end);
        bake_touched(&mut layer, range.start, end, &mut budget)?;
        split(&mut layer, range.start);
        let keyframes = layer
            .keyframes
            .into_iter()
            .filter(|key| key.frame >= range.start && key.frame < end)
            .map(|mut key| {
                key.frame -= range.start;
                key.tween = None;
                key
            })
            .collect();
        layers.push(FrameLayerClip {
            source_layer: layer.id,
            keyframes,
            end_frame: effective_end.min(end).saturating_sub(range.start),
        });
    }
    let scripts = if include_scripts {
        timeline
            .scripts
            .iter()
            .filter(|script| script.frame >= range.start && script.frame < end)
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
fn clear_rows(
    timeline: &mut Timeline,
    range: &FrameRange,
    budget: &mut usize,
) -> Result<(), String> {
    let end = range_end(timeline, range)?;
    for index in layer_indices(timeline, &range.layers)? {
        let layer = &mut timeline.layers[index];
        bake_touched(layer, range.start, end, budget)?;
        split(layer, end);
        layer
            .keyframes
            .retain(|key| key.frame < range.start || key.frame >= end);
        if range.start < layer.properties.end_frame.unwrap_or(0) {
            layer.keyframes.push(blank(range.start));
        }
        layer.keyframes.sort_by_key(|key| key.frame);
    }
    Ok(())
}
fn remove_scripts(timeline: &mut Timeline, range: &FrameRange) {
    timeline
        .scripts
        .retain(|script| script.frame < range.start || script.frame > range.end);
}
fn paste_rows(
    project: &mut Project,
    symbol: &str,
    clip: &FrameClipboard,
    layers: &[Id],
    at: u32,
    include_scripts: bool,
    budget: &mut usize,
) -> Result<(), String> {
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
    if include_scripts
        && clip.scripts.iter().any(|incoming| {
            timeline
                .scripts
                .iter()
                .any(|existing| existing.frame == at + incoming.frame)
        })
    {
        return Err("A destination frame already has global code. Move it first or paste without frame scripts; no code was replaced.".into());
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
        let layer = &mut timeline.layers[index];
        let old_end = layer.properties.end_frame.unwrap_or(0);
        bake_touched(layer, at, end, budget)?;
        split(layer, end);
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
            for element in &mut key.elements {
                if let Some(id) = remap.get(&element.id) {
                    element.id = id.clone();
                }
            }
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
    }
    if include_scripts {
        timeline
            .scripts
            .extend(clip.scripts.iter().cloned().map(|mut script| {
                script.frame += at;
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
fn continued_playback(
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

/// Synchronise symbols on one layer: keyframes in `start..=end` (in frame order) continue the
/// graphic instances of the keyframe before each of them. Returns the number of changed instances.
fn sync_symbols(layer: &mut Layer, start: u32, end: u32, graphics: &HashMap<Id, u32>) -> usize {
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
            let ElementKind::Instance {
                symbol_id,
                playback,
            } = &element.kind
            else {
                continue;
            };
            let Some(&count) = graphics.get(symbol_id) else {
                continue;
            };
            let instance = |e: &&Element| matches!(&e.kind, ElementKind::Instance { symbol_id: s, .. } if s == symbol_id);
            let Some(before) = previous
                .elements
                .iter()
                .filter(instance)
                .find(|e| e.id == element.id)
                .or_else(|| previous.elements.iter().find(instance))
            else {
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

pub fn edit_frames(project: &mut Project, symbol: &str, edit: TimelineEdit) -> Result<(), String> {
    let mut candidate = project.clone();
    // Graphic symbols and their frame counts (Synchronise symbols only).
    let graphics: HashMap<Id, u32> = match &edit {
        TimelineEdit::SyncSymbols { .. } => candidate
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Graphic)
            .map(|s| (s.id.clone(), s.timeline.frame_count.max(1)))
            .collect(),
        _ => HashMap::new(),
    };
    let timeline = &mut candidate
        .symbol_mut(symbol)
        .ok_or("The edited symbol no longer exists.")?
        .timeline;
    let old_count = timeline.frame_count;
    for layer in &mut timeline.layers {
        if layer.properties.end_frame.is_none() {
            layer.properties.end_frame = Some(old_count);
        }
    }
    let mut budget = 0;
    match edit {
        TimelineEdit::Clear {
            range,
            include_scripts,
        } => {
            clear_rows(timeline, &range, &mut budget)?;
            if include_scripts {
                remove_scripts(timeline, &range);
            }
        }
        TimelineEdit::Delete {
            range,
            include_scripts,
        } => {
            let end = range_end(timeline, &range)?;
            let count = end - range.start;
            for index in layer_indices(timeline, &range.layers)? {
                let layer = &mut timeline.layers[index];
                bake_touched(layer, range.start, end, &mut budget)?;
                split(layer, end);
                layer
                    .keyframes
                    .retain(|key| key.frame < range.start || key.frame >= end);
                for key in &mut layer.keyframes {
                    if key.frame >= end {
                        key.frame -= count;
                    }
                }
                let old_end = layer.properties.end_frame.unwrap_or(0);
                layer.properties.end_frame =
                    Some(old_end.saturating_sub(old_end.min(end).saturating_sub(range.start)));
            }
            if include_scripts {
                remove_scripts(timeline, &range);
                for script in &mut timeline.scripts {
                    if script.frame >= end {
                        script.frame -= count;
                    }
                }
            }
        }
        TimelineEdit::Insert {
            layers,
            at,
            count,
            include_scripts,
        } => {
            if count == 0
                || at > old_count
                || old_count
                    .checked_add(count)
                    .is_none_or(|end| end > MAX_FRAME)
            {
                return Err("Inserted time is outside the supported timeline range.".into());
            }
            for index in layer_indices(timeline, &layers)? {
                let layer = &mut timeline.layers[index];
                let old_end = layer.properties.end_frame.unwrap_or(0);
                bake_touched(
                    layer,
                    at.saturating_sub(1),
                    at.saturating_add(1),
                    &mut budget,
                )?;
                split(layer, at);
                let hold = if at > 0 && at <= old_end {
                    sampled_key(layer, at - 1).elements
                } else {
                    vec![]
                };
                for key in &mut layer.keyframes {
                    if key.frame >= at {
                        key.frame += count;
                    }
                }
                if at > old_end {
                    layer.keyframes.push(blank(old_end));
                }
                let mut key = blank(at);
                key.elements = hold;
                layer.keyframes.push(key);
                layer.properties.end_frame = Some(old_end.max(at) + count);
                layer.keyframes.sort_by_key(|key| key.frame);
            }
            if include_scripts {
                for script in &mut timeline.scripts {
                    if script.frame >= at {
                        script.frame += count;
                    }
                }
            }
        }
        TimelineEdit::Paste {
            clip,
            layers,
            at,
            include_scripts,
        } => paste_rows(
            &mut candidate,
            symbol,
            &clip,
            &layers,
            at,
            include_scripts,
            &mut budget,
        )?,
        TimelineEdit::Move {
            range,
            layers,
            at,
            copy,
            include_scripts,
        } => {
            if !copy && at == range.start && layers == range.layers {
                return Ok(());
            }
            let clip = copy_frames(&candidate, symbol, &range, include_scripts)?;
            if !copy {
                let timeline = &mut candidate.symbol_mut(symbol).unwrap().timeline;
                clear_rows(timeline, &range, &mut budget)?;
                if include_scripts {
                    remove_scripts(timeline, &range);
                }
            }
            paste_rows(
                &mut candidate,
                symbol,
                &clip,
                &layers,
                at,
                include_scripts,
                &mut budget,
            )?;
        }
        TimelineEdit::Reverse { range } => {
            let end = range_end(timeline, &range)?;
            for index in layer_indices(timeline, &range.layers)? {
                let layer = &mut timeline.layers[index];
                let last = end.min(layer.properties.end_frame.unwrap_or(0));
                if last <= range.start + 1 {
                    continue;
                }
                // Tweens crossing either boundary are baked; tweens inside are kept.
                bake_touched(layer, range.start, range.start, &mut budget)?;
                bake_touched(layer, last, last, &mut budget)?;
                let original = layer.clone();
                for frame in [range.start, last - 1, last] {
                    split(layer, frame);
                }
                let (first, final_frame) = (range.start, last - 1);
                let mut inside: Vec<Keyframe> = layer
                    .keyframes
                    .iter()
                    .filter(|key| (first..=final_frame).contains(&key.frame))
                    .cloned()
                    .collect();
                layer
                    .keyframes
                    .retain(|key| !(first..=final_frame).contains(&key.frame));
                let tweens: Vec<_> = inside.iter().map(|key| key.tween).collect();
                let count = inside.len();
                for (i, key) in inside.iter_mut().enumerate() {
                    // A segment a→b becomes b'→a' and keeps a's tween. Motion tween
                    // tracks do not survive reversal; their poses become classic keys.
                    if key.keyed_properties.take().is_some() || key.tween_kind == TweenKind::Motion
                    {
                        key.elements = sample_layer(&original, key.frame);
                        key.tween_kind = TweenKind::Classic;
                    }
                    key.tween = tweens[if i == 0 { count - 1 } else { i - 1 }];
                    key.frame = first + final_frame - key.frame;
                }
                layer.keyframes.extend(inside);
                layer.keyframes.sort_by_key(|key| key.frame);
            }
        }
        TimelineEdit::ConvertToKeyframes {
            range,
            blank: to_blank,
        } => {
            let end = range_end(timeline, &range)?;
            for index in layer_indices(timeline, &range.layers)? {
                let layer = &mut timeline.layers[index];
                let last = end.min(layer.properties.end_frame.unwrap_or(0));
                if last <= range.start {
                    continue;
                }
                let original = layer.clone();
                let records = (last - range.start) as usize
                    * original
                        .key_at(range.start)
                        .map_or(1, |key| key.elements.len().max(1));
                budget = budget
                    .checked_add(records)
                    .ok_or("Frame edit is too large.")?;
                if budget > MAX_BAKED_RECORDS {
                    return Err("This edit would create more than 100000 frame/object records. Select a shorter range.".into());
                }
                bake_touched(layer, last, last, &mut budget)?;
                split(layer, last);
                layer
                    .keyframes
                    .retain(|key| key.frame < range.start || key.frame >= last);
                layer.keyframes.extend((range.start..last).map(|frame| {
                    if to_blank {
                        let mut key = blank(frame);
                        if let Some(old) = original.keyframes.iter().find(|k| k.frame == frame) {
                            key.label = old.label.clone();
                        }
                        key
                    } else {
                        sampled_key(&original, frame)
                    }
                }));
                layer.keyframes.sort_by_key(|key| key.frame);
            }
        }
        TimelineEdit::SyncSymbols { range } => {
            range_end(timeline, &range)?;
            let mut changed = 0;
            for index in layer_indices(timeline, &range.layers)? {
                changed += sync_symbols(
                    &mut timeline.layers[index],
                    range.start,
                    range.end,
                    &graphics,
                );
            }
            if changed == 0 {
                return Err("The graphic symbols in the selected frames are already in step. Synchronise symbols adjusts graphic instances (Loop or Play once) on keyframes that follow another keyframe.".into());
            }
        }
        TimelineEdit::SetEnd { layers, end_frame } => {
            if end_frame > MAX_FRAME {
                return Err("Layer exposure exceeds 1000000 frames.".into());
            }
            for index in layer_indices(timeline, &layers)? {
                let layer = &mut timeline.layers[index];
                let old_end = layer.properties.end_frame.unwrap_or(0);
                if end_frame < old_end {
                    bake_touched(layer, end_frame.saturating_sub(1), end_frame, &mut budget)?;
                }
                layer.keyframes.retain(|key| key.frame < end_frame);
                layer.properties.end_frame = Some(end_frame);
            }
        }
    }
    let timeline = &mut candidate.symbol_mut(symbol).unwrap().timeline;
    timeline.frame_count = timeline
        .layers
        .iter()
        .filter(|layer| layer.properties.role != LayerRole::Folder)
        .filter_map(|layer| layer.properties.end_frame)
        .chain(timeline.scripts.iter().map(|script| script.frame + 1))
        .max()
        .unwrap_or(1)
        .max(1);
    for layer in &mut timeline.layers {
        if layer.properties.role == LayerRole::Folder {
            layer.properties.end_frame = Some(timeline.frame_count);
            layer
                .keyframes
                .retain(|key| key.frame < timeline.frame_count);
        }
    }
    for layer in &mut timeline.layers {
        layer.normalize_motion_keys();
    }
    candidate.format_version = candidate.format_version.max(5);
    candidate.upgrade_format_for_features();
    candidate.prune_authoring_groups();
    candidate.validate().map_err(|error| error.to_string())?;
    *project = candidate;
    Ok(())
}
