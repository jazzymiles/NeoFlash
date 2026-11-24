use super::clipboard_sync::*;
use super::motion_keys::*;
use super::reverse_curves::*;
use super::*;

pub fn edit_frames(project: &mut Project, symbol: &str, edit: TimelineEdit) -> Result<(), String> {
    let mut candidate = project.clone();
    let fps = f64::from(candidate.fps);
    let is_set_end = matches!(edit, TimelineEdit::SetEnd { .. });
    // Symbols and their frame counts (Synchronise symbols and Razor / SplitClip).
    let symbols_for_sync: HashMap<Id, (SymbolKind, u32)> = match &edit {
        TimelineEdit::SyncSymbols { .. } | TimelineEdit::SplitClip { .. } => candidate
            .symbols
            .iter()
            .map(|s| (s.id.clone(), (s.kind, s.timeline.frame_count.max(1))))
            .collect(),
        _ => HashMap::new(),
    };
    let timeline = &mut candidate
        .symbol_mut(symbol)
        .ok_or("The edited symbol no longer exists.")?
        .timeline;
    let old_count = timeline.frame_count;
    let originally_open_end: HashSet<Id> = timeline
        .layers
        .iter()
        .filter(|l| l.properties.role != LayerRole::Folder && l.properties.end_frame.is_none())
        .map(|l| l.id.clone())
        .collect();
    let mut budget = 0;
    match edit {
        TimelineEdit::Clear {
            range,
            include_scripts,
        } => {
            clear_rows(timeline, &range, &mut budget, fps)?;
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
                let mut layer = timeline.layers[index].clone();
                let old_end = exposure(&layer, old_count);
                let original = layer.clone();
                let (left_split, _) = video_cut_on_layer(&original, old_end, range.start, fps)?;
                let (end_mid_split, tail_video) = video_cut_on_layer(&original, old_end, end, fps)?;
                if layer.keyframes.iter().any(has_object_motion_path) || end_mid_split.is_some() {
                    isolate_range(timeline, &mut layer, range.start, end, &mut budget, fps)?;
                    split(timeline, &mut layer, end, fps);
                } else if end < old_end
                    && !layer
                        .keyframes
                        .iter()
                        .any(|k| k.frame == end && k.keyed_properties.is_none())
                    && let Some(pos) = layer.keyframes.iter().rposition(|k| {
                        k.frame >= range.start && k.frame < end && k.keyed_properties.is_none()
                    })
                {
                    if layer.keyframes[pos].tween.is_some() {
                        let sampled = sample_layer_in_at_fps(timeline, &layer, end, fps);
                        layer.keyframes[pos].elements = sampled;
                    }
                    layer.keyframes[pos].frame = end;
                }
                if let Some((left_frame, left_video)) = left_split
                    && let Some(left_key) = layer
                        .keyframes
                        .iter_mut()
                        .find(|k| k.frame == left_frame && k.keyed_properties.is_none())
                {
                    left_key.video = Some(left_video);
                }
                if end_mid_split.is_some()
                    && let Some(right_video) = tail_video
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
                for key in &mut layer.keyframes {
                    if key.frame >= end {
                        key.frame -= count;
                    }
                }
                let new_end = old_end.saturating_sub(old_end.min(end).saturating_sub(range.start));
                if layer.keyframes.is_empty() && new_end > 0 {
                    layer.keyframes.push(blank(0));
                }
                layer.properties.end_frame = Some(new_end);
                timeline.layers[index] = layer;
            }
            if include_scripts {
                remove_scripts(timeline, &range);
                for script in &mut timeline.scripts {
                    if script.frame >= end && script_follows_layers(script, &range.layers) {
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
                || at > MAX_FRAME
                || old_count
                    .max(at)
                    .checked_add(count)
                    .is_none_or(|end| end > MAX_FRAME)
            {
                return Err("Inserted time is outside the supported timeline range.".into());
            }
            for index in layer_indices(timeline, &layers)? {
                let mut layer = timeline.layers[index].clone();
                let old_end = exposure(&layer, old_count);
                if at <= old_end {
                    let has_key_at_or_before = layer.keyframes.iter().any(|k| k.frame <= at);
                    for key in &mut layer.keyframes {
                        if if has_key_at_or_before {
                            key.frame > at
                        } else {
                            key.frame >= at
                        } {
                            key.frame += count;
                        }
                    }
                    if layer.keyframes.is_empty() {
                        layer.keyframes.push(blank(0));
                    }
                    layer.properties.end_frame = Some(old_end + count);
                } else {
                    if layer.keyframes.is_empty() {
                        layer.keyframes.push(blank(0));
                    }
                    layer.properties.end_frame = Some(at + count);
                }
                layer.keyframes.sort_by_key(|key| key.frame);
                timeline.layers[index] = layer;
            }
            if include_scripts {
                for script in &mut timeline.scripts {
                    if script.frame > at && script_follows_layers(script, &layers) {
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
                clear_rows(timeline, &range, &mut budget, fps)?;
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
                let mut layer = timeline.layers[index].clone();
                let layer_exposure = exposure(&layer, old_count);
                let last = end.min(layer_exposure);
                if last <= range.start + 1 {
                    continue;
                }
                let (first, final_frame) = (range.start, last - 1);
                isolate_range(timeline, &mut layer, first, first, &mut budget, fps)?;
                isolate_range(timeline, &mut layer, last, last, &mut budget, fps)?;

                let mut bake_ranges: Vec<(u32, u32)> = Vec::new();
                for (i, key) in layer.keyframes.iter().enumerate() {
                    if key.keyed_properties.is_some() || key.tween.is_none() {
                        continue;
                    }
                    let must_bake = has_noninvertible_motion_ease(key)
                        || key.property_expressions.iter().any(|e| e.enabled)
                        || (key.tween_kind == TweenKind::Motion
                            && layer.properties.motion_guide.is_some());
                    if must_bake {
                        let span_end = if key.tween_kind == TweenKind::Motion {
                            layer.keyframes[i + 1..]
                                .iter()
                                .find(|k| k.keyed_properties.is_none())
                                .map_or(last, |next| next.frame)
                        } else {
                            layer.keyframes.get(i + 1).map_or(last, |next| next.frame)
                        };
                        if key.frame < last && span_end > first {
                            bake_ranges.push((key.frame, span_end));
                        }
                    }
                }
                for (b_start, b_end) in bake_ranges {
                    bake_touched(timeline, &mut layer, b_start, b_end, &mut budget, fps)?;
                }

                // Discover remaining Motion spans inside `[first .. last)`.
                let mut motion_windows: Vec<(u32, u32)> = Vec::new();
                {
                    let mut idx = 0;
                    while idx < layer.keyframes.len() {
                        let key = &layer.keyframes[idx];
                        if key.frame >= first
                            && key.frame < last
                            && key.keyed_properties.is_none()
                            && key.tween.is_some()
                            && key.tween_kind == TweenKind::Motion
                        {
                            let mut next_idx = idx + 1;
                            while next_idx < layer.keyframes.len()
                                && layer.keyframes[next_idx].keyed_properties.is_some()
                            {
                                next_idx += 1;
                            }
                            let excl_end = layer
                                .keyframes
                                .get(next_idx)
                                .map_or(last, |k| k.frame.min(last));
                            motion_windows.push((key.frame, excl_end));
                            idx = next_idx;
                            continue;
                        }
                        idx += 1;
                    }
                }

                for &(m_start, m_excl_end) in &motion_windows {
                    isolate_range(timeline, &mut layer, m_start, m_start, &mut budget, fps)?;
                    isolate_range(
                        timeline,
                        &mut layer,
                        m_excl_end,
                        m_excl_end,
                        &mut budget,
                        fps,
                    )?;
                }

                let mut reversed_motion_blocks: Vec<Keyframe> = Vec::new();
                let mut preserved_motion_windows: Vec<(u32, u32)> = Vec::new();
                for (m_start, m_excl_end) in motion_windows {
                    if m_excl_end <= m_start + 1 {
                        bake_touched(timeline, &mut layer, m_start, m_excl_end, &mut budget, fps)?;
                        continue;
                    }
                    if let Some(rev_keys) = reverse_single_motion_span(
                        timeline,
                        &layer,
                        m_start,
                        m_excl_end,
                        first,
                        final_frame,
                        last,
                        layer_exposure,
                        fps,
                    ) {
                        if m_start > first {
                            split(timeline, &mut layer, m_start - 1, fps);
                        }
                        if m_excl_end < last {
                            split(timeline, &mut layer, m_excl_end, fps);
                        }
                        preserved_motion_windows.push((m_start, m_excl_end));
                        reversed_motion_blocks.extend(rev_keys);
                    } else {
                        bake_touched(timeline, &mut layer, m_start, m_excl_end, &mut budget, fps)?;
                    }
                }

                let in_preserved_motion = |f: u32| {
                    preserved_motion_windows
                        .iter()
                        .any(|&(ms, me)| f >= ms && f < me)
                };
                for boundary in [first, final_frame, last] {
                    if !in_preserved_motion(boundary) {
                        split(timeline, &mut layer, boundary, fps);
                    }
                }
                layer.keyframes.retain(|k| !in_preserved_motion(k.frame));

                // Partition `[first ..= final_frame]` into maximal non-Motion sub-intervals and
                // reverse Classic / Shape / Hold spans within each sub-interval.
                let guide_layer = layer
                    .properties
                    .motion_guide
                    .as_ref()
                    .and_then(|gid| timeline.layers.iter().find(|l| &l.id == gid))
                    .cloned();
                let mut cursor = first;
                let mut non_motion_intervals: Vec<(u32, u32)> = Vec::new();
                for &(ms, me) in &preserved_motion_windows {
                    if cursor < ms {
                        non_motion_intervals.push((cursor, ms - 1));
                    }
                    cursor = me;
                }
                if cursor <= final_frame {
                    non_motion_intervals.push((cursor, final_frame));
                }

                let mut reversed_non_motion: Vec<Keyframe> = Vec::new();
                for (sub_first, sub_final) in non_motion_intervals {
                    for b in [sub_first, sub_final] {
                        split(timeline, &mut layer, b, fps);
                    }
                    let mut sub_inside: Vec<Keyframe> = layer
                        .keyframes
                        .iter()
                        .filter(|k| (sub_first..=sub_final).contains(&k.frame))
                        .cloned()
                        .collect();
                    layer
                        .keyframes
                        .retain(|k| !(sub_first..=sub_final).contains(&k.frame));
                    reverse_classic_subinterval(
                        timeline,
                        guide_layer.as_ref(),
                        &mut sub_inside,
                        first,
                        final_frame,
                    );
                    reversed_non_motion.extend(sub_inside);
                }

                layer.keyframes.extend(reversed_non_motion);
                layer.keyframes.extend(reversed_motion_blocks);
                layer.keyframes.sort_by_key(|key| key.frame);
                timeline.layers[index] = layer;
            }
        }
        TimelineEdit::ConvertToKeyframes {
            range,
            blank: to_blank,
        } => {
            if range.start > range.end || range.end >= MAX_FRAME {
                return Err("The frame range is outside the timeline.".into());
            }
            let end = range.end + 1;
            for index in layer_indices(timeline, &range.layers)? {
                let mut layer = timeline.layers[index].clone();
                if layer.keyframes.is_empty() && end > 0 {
                    layer.keyframes.push(blank(0));
                }
                let old_end = exposure(&layer, old_count);
                let new_end = old_end.max(end);
                let original = layer.clone();
                let (left_split, _) = video_cut_on_layer(&original, new_end, range.start, fps)?;
                let (_, tail_video) =
                    video_cut_on_layer(&original, new_end, end.min(new_end), fps)?;
                let last = end.min(new_end);
                if last <= range.start {
                    layer.properties.end_frame = Some(new_end);
                    timeline.frame_count = timeline.frame_count.max(new_end);
                    timeline.layers[index] = layer;
                    continue;
                }
                let mut per_frame_videos: HashMap<u32, TimelineVideoSpan> = HashMap::new();
                if !to_blank {
                    for frame in range.start..last {
                        if let (_, Some(v)) = video_cut_on_layer(&original, new_end, frame, fps)? {
                            per_frame_videos.insert(frame, v);
                        }
                    }
                }
                let before_isolate = budget;
                isolate_range(timeline, &mut layer, range.start, last, &mut budget, fps)?;
                let isolate_charged = budget - before_isolate;
                let mut range_budget = 0;
                charge_bake_budget(&original, range.start, last, &mut range_budget)?;
                budget = before_isolate
                    .checked_add(isolate_charged.max(range_budget))
                    .ok_or("Frame edit is too large.")?;
                if budget > MAX_BAKED_RECORDS {
                    return Err("This edit would create more than 100000 frame/object records. Select a shorter range.".into());
                }
                layer.properties.end_frame = Some(new_end);
                timeline.frame_count = timeline.frame_count.max(new_end);
                split(timeline, &mut layer, last, fps);
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
                        .find(|k| k.frame == last && k.keyed_properties.is_none())
                {
                    right_key.video = Some(right_video);
                }
                layer
                    .keyframes
                    .retain(|key| key.frame < range.start || key.frame >= last);
                layer.keyframes.extend((range.start..last).map(|frame| {
                    if to_blank {
                        let mut key = blank(frame);
                        if let Some(old) = original.keyframes.iter().find(|k| k.frame == frame) {
                            key.label = old.label.clone();
                            key.label_type = old.label_type;
                        }
                        key
                    } else {
                        let mut key = sampled_key(timeline, &original, frame, fps);
                        if let Some(v) = per_frame_videos.remove(&frame) {
                            key.video = Some(v);
                        }
                        key
                    }
                }));
                layer.keyframes.sort_by_key(|key| key.frame);
                timeline.layers[index] = layer;
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
                    &symbols_for_sync,
                    None,
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
                let mut layer = timeline.layers[index].clone();
                let old_end = exposure(&layer, old_count);
                if end_frame < old_end {
                    let _ = video_cut_on_layer(&layer, old_end, end_frame, fps)?;
                    isolate_range(timeline, &mut layer, end_frame, old_end, &mut budget, fps)?;
                }
                layer.keyframes.retain(|key| key.frame < end_frame);
                if layer.keyframes.is_empty() && end_frame > 0 {
                    layer.keyframes.push(blank(0));
                }
                layer.properties.end_frame = Some(end_frame);
                timeline.layers[index] = layer;
            }
        }
        TimelineEdit::SplitClip { layers, at } => {
            if at == 0 || at >= MAX_FRAME {
                return Err("Place the playhead or Razor inside an active span to split.".into());
            }
            let indices = layer_indices(timeline, &layers)?;
            let mut split_count = 0usize;
            for index in indices {
                let mut layer = timeline.layers[index].clone();
                let layer_end = exposure(&layer, old_count);
                if at >= layer_end {
                    continue;
                }
                if layer
                    .keyframes
                    .iter()
                    .any(|k| k.frame == at && k.keyed_properties.is_none())
                {
                    continue;
                }
                let Some(start_pos) = layer
                    .keyframes
                    .iter()
                    .rposition(|k| k.frame < at && k.keyed_properties.is_none())
                else {
                    continue;
                };
                let span_start = layer.keyframes[start_pos].frame;
                let span_end = layer.keyframes[start_pos + 1..]
                    .iter()
                    .find(|k| k.keyed_properties.is_none())
                    .map_or(layer_end, |k| k.frame);
                if at >= span_end {
                    continue;
                }
                let video_split = if let Some(video) = &layer.keyframes[start_pos].video {
                    Some(split_video_span_at(video, span_start, at, fps)?)
                } else {
                    None
                };
                let sound_split = if let Some(sound) = &layer.keyframes[start_pos].sound {
                    if sound.sync == SoundSync::Stream {
                        Some(split_stream_sound_at(sound, span_start, at, fps))
                    } else if layer.keyframes[start_pos].elements.is_empty()
                        && video_split.is_none()
                    {
                        return Err(
                            "Cannot split an Event, Start, or Stop sound span mid-clip because asynchronous sound is not frame-locked. Switch Sync to Stream to split audio at a frame."
                                .into(),
                        );
                    } else {
                        None
                    }
                } else {
                    None
                };
                split_span_at(
                    timeline,
                    &mut layer,
                    start_pos,
                    span_start,
                    span_end,
                    at,
                    &mut budget,
                    fps,
                )?;
                if let Some((left_video, right_video)) = video_split {
                    if let Some(left_key) = layer
                        .keyframes
                        .iter_mut()
                        .find(|k| k.frame == span_start && k.keyed_properties.is_none())
                    {
                        left_key.video = Some(left_video);
                    }
                    if let Some(right_key) = layer
                        .keyframes
                        .iter_mut()
                        .find(|k| k.frame == at && k.keyed_properties.is_none())
                    {
                        right_key.video = Some(right_video);
                    }
                }
                if let Some((left_sound, right_sound)) = sound_split {
                    if let Some(left_key) = layer
                        .keyframes
                        .iter_mut()
                        .find(|k| k.frame == span_start && k.keyed_properties.is_none())
                    {
                        left_key.sound = Some(left_sound);
                    }
                    if let Some(right_key) = layer
                        .keyframes
                        .iter_mut()
                        .find(|k| k.frame == at && k.keyed_properties.is_none())
                    {
                        right_key.sound = Some(right_sound);
                    }
                }
                let _ = sync_symbols(&mut layer, at, at, &symbols_for_sync, None);
                timeline.layers[index] = layer;
                split_count += 1;
            }
            if split_count == 0 {
                return Err(
                    "Place the playhead or Razor inside an active span (between keyframes) to split."
                        .into(),
                );
            }
        }
    }
    let timeline = &mut candidate.symbol_mut(symbol).unwrap().timeline;
    for layer in &mut timeline.layers {
        if layer.properties.role != LayerRole::Folder && layer.properties.end_frame.is_none() {
            layer.properties.end_frame = Some(old_count);
        }
    }
    timeline.frame_count = timeline
        .layers
        .iter()
        .filter(|layer| layer.properties.role != LayerRole::Folder)
        .filter_map(|layer| layer.properties.end_frame)
        .chain(timeline.scripts.iter().map(|script| script.frame + 1))
        .max()
        .unwrap_or(1)
        .max(1);
    if !is_set_end {
        let new_count = timeline.frame_count;
        for layer in &mut timeline.layers {
            if layer.properties.role != LayerRole::Folder
                && originally_open_end.contains(&layer.id)
                && layer.properties.end_frame == Some(new_count)
            {
                layer.properties.end_frame = None;
            }
        }
    }
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
    candidate.prune_authoring_groups();
    candidate.prune_frame_script_layers();
    candidate.format_version = candidate.format_version.max(5);
    candidate.upgrade_format_for_features();
    candidate.validate().map_err(|error| error.to_string())?;
    *project = candidate;
    Ok(())
}

impl Timeline {
    /// Returns the indices of visible timeline rows in top-to-bottom display order
    /// (front-to-back sibling order, with folder/mask/guide headers above their children).
    pub fn visible_layer_indices(&self) -> Vec<usize> {
        let indices: std::collections::HashMap<_, _> = self
            .layers
            .iter()
            .enumerate()
            .map(|(index, layer)| (layer.id.as_str(), index))
            .collect();
        let mut children = vec![Vec::new(); self.layers.len()];
        let mut roots = Vec::new();
        for (index, layer) in self.layers.iter().enumerate() {
            let parent = layer
                .properties
                .mask
                .as_ref()
                .or(layer.properties.motion_guide.as_ref())
                .or(layer.properties.parent.as_ref())
                .and_then(|id| indices.get(id.as_str()).copied());
            if let Some(parent) = parent {
                children[parent].push(index);
            } else {
                roots.push(index);
            }
        }
        let mut visible = Vec::with_capacity(self.layers.len());
        let mut stack = roots;
        let mut visited = std::collections::HashSet::new();
        while let Some(index) = stack.pop() {
            if !visited.insert(index) {
                continue;
            }
            visible.push(index);
            let layer = &self.layers[index];
            if layer.properties.role != LayerRole::Folder || !layer.properties.collapsed {
                stack.extend(children[index].iter().copied());
            }
        }
        visible
    }

    /// Returns true when the timeline has a non-empty `FrameScript` at `frame`.
    pub fn has_frame_script(&self, frame: u32) -> bool {
        self.scripts
            .iter()
            .any(|script| script.frame == frame && !script.script.source.trim().is_empty())
    }

    /// Resolve the authoring row for one frame script. New documents store the stable layer ID;
    /// legacy scripts without it retain the old deterministic topmost-key fallback.
    pub fn action_layer_for_script(&self, script: &FrameScript) -> Option<usize> {
        if script.frame >= self.frame_count || script.script.source.trim().is_empty() {
            return None;
        }
        if let Some(layer_id) = &script.layer {
            return self.layers.iter().position(|layer| {
                &layer.id == layer_id
                    && layer.properties.role != LayerRole::Folder
                    && layer
                        .keyframes
                        .iter()
                        .any(|key| key.frame == script.frame && key.keyed_properties.is_none())
            });
        }
        self.inferred_action_layer_for_frame(script.frame)
    }

    pub(super) fn inferred_action_layer_for_frame(&self, frame: u32) -> Option<usize> {
        self.visible_layer_indices().into_iter().find(|&index| {
            let layer = &self.layers[index];
            if layer.properties.role == LayerRole::Folder {
                return false;
            }
            let layer_end = layer
                .properties
                .end_frame
                .unwrap_or(self.frame_count)
                .min(self.frame_count);
            frame < layer_end
                && layer
                    .keyframes
                    .iter()
                    .any(|key| key.frame == frame && key.keyed_properties.is_none())
        })
    }

    /// The script visible in Actions for one exact layer/frame context. Explicitly owned scripts
    /// win. A layerless legacy script uses its inferred marker row when one exists; if no exact
    /// ordinary key can host the marker, it retains its historical global authoring visibility
    /// from any non-folder row while the Timeline keeps the marker on the ruler.
    pub fn frame_script_for_layer(&self, layer_index: usize, frame: u32) -> Option<&FrameScript> {
        self.frame_script_index_for_layer(layer_index, frame)
            .and_then(|index| self.scripts.get(index))
    }

    /// Index form of [`Timeline::frame_script_for_layer`] for layer-aware mutation paths.
    pub fn frame_script_index_for_layer(&self, layer_index: usize, frame: u32) -> Option<usize> {
        let layer_id = &self.layers.get(layer_index)?.id;
        self.scripts
            .iter()
            .position(|script| script.frame == frame && script.layer.as_ref() == Some(layer_id))
            .or_else(|| {
                self.scripts.iter().position(|script| {
                    script.frame == frame
                        && !script.script.source.trim().is_empty()
                        && script.layer.is_none()
                        && self.action_layer_for_script(script).map_or_else(
                            || {
                                self.layers
                                    .get(layer_index)
                                    .is_some_and(|layer| layer.properties.role != LayerRole::Folder)
                            },
                            |owner| owner == layer_index,
                        )
                })
            })
    }

    /// Returns the layer index (`self.layers[index]`) whose keyframe at `frame` displays
    /// and owns the `a` marker for the timeline's `FrameScript` at `frame`, or `None` when
    /// there is no non-empty `FrameScript` at `frame` or no visible non-folder layer has an
    /// ordinary keyframe at `frame` (in which case the `a` marker is shown on the ruler).
    pub fn action_layer_for_frame(&self, frame: u32) -> Option<usize> {
        if frame >= self.frame_count || !self.has_frame_script(frame) {
            return None;
        }
        self.scripts
            .iter()
            .filter(|script| script.frame == frame)
            .find_map(|script| self.action_layer_for_script(script))
    }

    /// Returns true when at least one non-empty script at `frame` cannot be shown on a layer row.
    /// The Timeline paints one ruler marker in that case, even if another same-frame script does
    /// have a valid row owner.
    pub fn frame_has_unplaced_script(&self, frame: u32) -> bool {
        self.scripts.iter().any(|script| {
            script.frame == frame
                && !script.script.source.trim().is_empty()
                && self.action_layer_for_script(script).is_none()
        })
    }

    /// Returns true if the keyframe on `layer_index` at `frame` is the action-bearing
    /// keyframe that carries the timeline's `FrameScript` at `frame`.
    pub fn key_carries_frame_script(&self, layer_index: usize, frame: u32) -> bool {
        self.scripts.iter().any(|script| {
            script.frame == frame && self.action_layer_for_script(script) == Some(layer_index)
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyMoveError {
    MissingSymbol,
    MissingLayer,
    InvalidLayerOrBounds,
    MissingSourceKey,
    DestinationKeyConflict,
    DestinationScriptConflict,
}

impl Project {
    /// Moves or copies a single keyframe on `layer` from `from` to `to` within `symbol_id`.
    /// When the keyframe at `(layer, from)` is the action-bearing keyframe for the timeline's
    /// `FrameScript` at `from`, that `FrameScript` moves or copies atomically with the keyframe.
    pub fn move_keyframe(
        &mut self,
        symbol_id: &str,
        layer: usize,
        from: u32,
        to: u32,
        copy: bool,
        layer_editable: bool,
    ) -> Result<bool, KeyMoveError> {
        let symbol = self.symbol(symbol_id).ok_or(KeyMoveError::MissingSymbol)?;
        let is_button = symbol.kind == SymbolKind::Button;
        let timeline = &symbol.timeline;
        let l = timeline
            .layers
            .get(layer)
            .ok_or(KeyMoveError::MissingLayer)?;
        let max_frames = if is_button {
            timeline.frame_count
        } else {
            MAX_FRAME
        };
        if !layer_editable
            || l.properties.role == LayerRole::Folder
            || to >= max_frames
            || from == to
        {
            return Err(KeyMoveError::InvalidLayerOrBounds);
        }
        if l.keyframes.iter().any(|k| k.frame == to) {
            return Err(KeyMoveError::DestinationKeyConflict);
        }
        let mut key = l
            .keyframes
            .iter()
            .find(|k| k.frame == from)
            .cloned()
            .ok_or(KeyMoveError::MissingSourceKey)?;
        let layer_id = l.id.clone();
        let carries_script = timeline.key_carries_frame_script(layer, from);
        let script_index = carries_script
            .then(|| {
                timeline.scripts.iter().position(|script| {
                    script.frame == from && timeline.action_layer_for_script(script) == Some(layer)
                })
            })
            .flatten();
        if script_index.is_some() && timeline.frame_script_for_layer(layer, to).is_some() {
            return Err(KeyMoveError::DestinationScriptConflict);
        }

        let old_count = timeline.frame_count;
        let old_end = l.properties.end_frame.unwrap_or(old_count);
        let was_last = l.keyframes.last().is_some_and(|k| k.frame == from);
        let still_last = l
            .keyframes
            .iter()
            .filter(|k| k.frame != from)
            .all(|k| k.frame < to);
        let was_trailing_single =
            !copy && from > 0 && was_last && still_last && old_end == from + 1;
        let new_layer_end = if was_trailing_single {
            to + 1
        } else {
            old_end.max(to + 1)
        };

        let symbol_mut = self
            .symbol_mut(symbol_id)
            .ok_or(KeyMoveError::MissingSymbol)?;
        let l = &mut symbol_mut.timeline.layers[layer];
        if !copy {
            l.keyframes.retain(|k| k.frame != from);
        }
        key.frame = to;
        if copy {
            key.label.clear();
        }
        l.keyframes.push(key);
        l.keyframes.sort_by_key(|k| k.frame);
        l.normalize_motion_keys();

        let moved_script = script_index.is_some();
        if let Some(script_index) = script_index {
            let scripts = &mut symbol_mut.timeline.scripts;
            let mut script = if copy {
                scripts[script_index].clone()
            } else {
                scripts.remove(script_index)
            };
            script.frame = to;
            script.layer = Some(layer_id.clone());
            scripts.push(script);
            scripts.sort_by_key(|item| item.frame);
        }

        if new_layer_end != old_end {
            let timeline = &mut symbol_mut.timeline;
            for other in &mut timeline.layers {
                if other.id == layer_id {
                    other.properties.end_frame = Some(new_layer_end);
                } else if other.properties.role != LayerRole::Folder
                    && other.properties.end_frame.is_none()
                {
                    other.properties.end_frame = Some(old_count);
                }
            }
            let next_count = timeline
                .layers
                .iter()
                .filter(|other| other.properties.role != LayerRole::Folder)
                .map(|other| other.properties.end_frame.unwrap_or(old_count))
                .chain(timeline.scripts.iter().map(|sc| sc.frame + 1))
                .max()
                .unwrap_or(1)
                .max(1);
            timeline.frame_count = next_count;
            for other in &mut timeline.layers {
                if other.properties.role == LayerRole::Folder
                    && other.properties.end_frame.is_some()
                {
                    other.properties.end_frame = Some(next_count);
                }
            }
            self.format_version = self.format_version.max(5);
        } else if let Some(end) = &mut symbol_mut.timeline.layers[layer].properties.end_frame {
            *end = (*end).max(to + 1);
        }
        self.upgrade_format_for_features();
        Ok(moved_script)
    }
}
