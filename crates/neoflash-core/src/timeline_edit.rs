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
pub fn edit_frames(project: &mut Project, symbol: &str, edit: TimelineEdit) -> Result<(), String> {
    let mut candidate = project.clone();
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
    candidate.format_version = candidate.format_version.max(5);
    candidate.prune_authoring_groups();
    candidate.validate().map_err(|error| error.to_string())?;
    *project = candidate;
    Ok(())
}
