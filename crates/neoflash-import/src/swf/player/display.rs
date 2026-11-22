use super::*;

pub(super) fn live_slot_segment(depth: u16, _slot: &LiveSlot) -> String {
    format!("depth{depth}")
}

pub(super) fn join_target_path(parent: &str, segment: &str) -> String {
    if parent.is_empty() || parent == "_root" || parent == "/" {
        segment.to_owned()
    } else {
        format!("{}/{segment}", parent.trim_end_matches('/'))
    }
}

pub(super) fn target_segments(target: &str) -> Vec<&str> {
    target
        .trim()
        .trim_start_matches('/')
        .split(['/', '.'])
        .filter(|segment| !segment.is_empty() && *segment != "_root" && *segment != "_level0")
        .collect()
}

pub(super) fn slot_matches_segment(depth: u16, slot: &LiveSlot, segment: &str) -> bool {
    slot.name == segment || segment == format!("depth{depth}")
}

pub(super) fn resolve_target_identity_by_segments(
    display_list: &BTreeMap<u16, LiveSlot>,
    segments: &[&str],
    parent_identity: &str,
) -> Option<String> {
    let (segment, rest) = segments.split_first()?;
    let (depth, slot) = display_list
        .iter()
        .find(|(depth, slot)| slot_matches_segment(**depth, slot, segment))?;
    let identity = join_target_path(parent_identity, &format!("depth{depth}"));
    if rest.is_empty() {
        Some(identity)
    } else {
        resolve_target_identity_by_segments(&slot.children, rest, &identity)
    }
}

pub(super) fn resolve_target_identity(
    display_list: &BTreeMap<u16, LiveSlot>,
    target: &str,
) -> Option<String> {
    let segments = target_segments(target);
    resolve_target_identity_by_segments(display_list, &segments, "")
}

pub(super) fn identity_depth(segment: &str) -> Option<u16> {
    segment.strip_prefix("depth")?.parse().ok()
}

pub(super) fn find_slot_by_identity_segments<'a>(
    display_list: &'a BTreeMap<u16, LiveSlot>,
    segments: &[&str],
) -> Option<&'a LiveSlot> {
    let (segment, rest) = segments.split_first()?;
    let slot = display_list.get(&identity_depth(segment)?)?;
    if rest.is_empty() {
        Some(slot)
    } else {
        find_slot_by_identity_segments(&slot.children, rest)
    }
}

pub(super) fn find_slot_by_identity_segments_mut<'a>(
    display_list: &'a mut BTreeMap<u16, LiveSlot>,
    segments: &[&str],
) -> Option<&'a mut LiveSlot> {
    let (segment, rest) = segments.split_first()?;
    let slot = display_list.get_mut(&identity_depth(segment)?)?;
    if rest.is_empty() {
        Some(slot)
    } else {
        find_slot_by_identity_segments_mut(&mut slot.children, rest)
    }
}

pub(super) fn find_slot_by_identity<'a>(
    display_list: &'a BTreeMap<u16, LiveSlot>,
    target: &str,
) -> Option<&'a LiveSlot> {
    let segments = target_segments(target);
    find_slot_by_identity_segments(display_list, &segments)
}

pub(super) fn find_slot_by_identity_mut<'a>(
    display_list: &'a mut BTreeMap<u16, LiveSlot>,
    target: &str,
) -> Option<&'a mut LiveSlot> {
    let segments = target_segments(target);
    find_slot_by_identity_segments_mut(display_list, &segments)
}

pub(super) fn find_slot_in_detached<'a>(
    root_target: &str,
    root_slot: &'a LiveSlot,
    target: &str,
) -> Option<&'a LiveSlot> {
    if target == root_target {
        return Some(root_slot);
    }
    let suffix = target.strip_prefix(root_target)?.strip_prefix('/')?;
    find_slot_by_identity(&root_slot.children, suffix)
}

pub(super) fn find_slot_in_detached_mut<'a>(
    root_target: &str,
    root_slot: &'a mut LiveSlot,
    target: &str,
) -> Option<&'a mut LiveSlot> {
    if target == root_target {
        return Some(root_slot);
    }
    let suffix = target.strip_prefix(root_target)?.strip_prefix('/')?;
    find_slot_by_identity_mut(&mut root_slot.children, suffix)
}

pub(super) fn collect_pointer_actions_filtered(
    display_list: &BTreeMap<u16, LiveSlot>,
    parent_target: &str,
    buttons: &HashMap<u16, Vec<ButtonCondAction>>,
    clip_mask: u32,
    btn_cond_mask: u16,
    actions: &mut Vec<(String, Vec<u8>)>,
) {
    for (&depth, slot) in display_list {
        let target = join_target_path(parent_target, &live_slot_segment(depth, slot));
        if btn_cond_mask != 0
            && let Some(conditions) = buttons.get(&slot.character_id)
        {
            let btn_scope = if parent_target.is_empty() {
                "_root".to_owned()
            } else {
                parent_target.to_owned()
            };
            actions.extend(
                conditions
                    .iter()
                    .filter(|condition| (condition.conditions & btn_cond_mask) != 0)
                    .map(|condition| (btn_scope.clone(), condition.actions.clone())),
            );
        }
        if clip_mask != 0 {
            actions.extend(
                slot.clip_handlers
                    .iter()
                    .filter(|handler| (handler.event_flags & clip_mask) != 0)
                    .map(|handler| (target.clone(), handler.actions.clone())),
            );
        }
        collect_pointer_actions_filtered(
            &slot.children,
            &target,
            buttons,
            clip_mask,
            btn_cond_mask,
            actions,
        );
    }
}

pub(super) fn collect_key_actions_filtered(
    display_list: &BTreeMap<u16, LiveSlot>,
    parent_target: &str,
    buttons: &HashMap<u16, Vec<ButtonCondAction>>,
    clip_mask: u32,
    key_code: Option<u8>,
    actions: &mut Vec<(String, Vec<u8>)>,
) {
    for (&depth, slot) in display_list {
        let target = join_target_path(parent_target, &live_slot_segment(depth, slot));
        if clip_mask & CLIP_EVENT_KEY_PRESS != 0
            && let Some(kc) = key_code
            && kc != 0
            && let Some(conditions) = buttons.get(&slot.character_id)
        {
            let btn_scope = if parent_target.is_empty() {
                "_root".to_owned()
            } else {
                parent_target.to_owned()
            };
            actions.extend(
                conditions
                    .iter()
                    .filter(|condition| ((condition.conditions >> 9) as u8) == kc)
                    .map(|condition| (btn_scope.clone(), condition.actions.clone())),
            );
        }
        if clip_mask != 0 {
            actions.extend(
                slot.clip_handlers
                    .iter()
                    .filter(|handler| {
                        let events = handler.event_flags & clip_mask;
                        events & !CLIP_EVENT_KEY_PRESS != 0
                            || (events & CLIP_EVENT_KEY_PRESS != 0
                                && key_code.is_some()
                                && handler.key_code == key_code)
                    })
                    .map(|handler| (target.clone(), handler.actions.clone())),
            );
        }
        collect_key_actions_filtered(
            &slot.children,
            &target,
            buttons,
            clip_mask,
            key_code,
            actions,
        );
    }
}

pub(super) fn collect_preexisting_enter_frame_work(
    display_list: &BTreeMap<u16, LiveSlot>,
    parent_target: &str,
    work: &mut Vec<PendingAvm1Work>,
) {
    for (&depth, slot) in display_list {
        let target = join_target_path(parent_target, &live_slot_segment(depth, slot));
        collect_preexisting_enter_frame_work(&slot.children, &target, work);
        if !slot.placed_this_step {
            work.extend(
                slot.clip_handlers
                    .iter()
                    .filter(|handler| (handler.event_flags & CLIP_EVENT_ENTER_FRAME) != 0)
                    .map(|handler| pending_action(target.clone(), handler.actions.clone())),
            );
            if let Some(Avm1Value::Object(fn_obj_id)) = slot.props.get("onEnterFrame") {
                work.push(PendingAvm1Work::Callback {
                    target,
                    fn_obj_id: *fn_obj_id,
                });
            }
        }
    }
}

pub(super) fn clear_placed_this_step_in_map(display_list: &mut BTreeMap<u16, LiveSlot>) {
    for slot in display_list.values_mut() {
        slot.placed_this_step = false;
        clear_placed_this_step_in_map(&mut slot.children);
    }
}

pub(super) fn collect_unload_actions(
    slot: &LiveSlot,
    target: &str,
    actions: &mut Vec<DetachedAvm1Item>,
) {
    for (&depth, child) in &slot.children {
        let child_target = join_target_path(target, &live_slot_segment(depth, child));
        collect_unload_actions(child, &child_target, actions);
    }
    actions.extend(
        slot.clip_handlers
            .iter()
            .filter(|handler| (handler.event_flags & CLIP_EVENT_UNLOAD) != 0)
            .map(|handler| DetachedAvm1Item::Bytes(target.to_owned(), handler.actions.clone())),
    );
    if let Some(Avm1Value::Object(fn_id)) = slot.props.get("onUnload") {
        actions.push(DetachedAvm1Item::Callback(target.to_owned(), *fn_id));
    }
}

pub(super) fn detached_unload_work(slot: &LiveSlot, target: &str) -> Option<PendingAvm1Work> {
    let mut actions = Vec::new();
    collect_unload_actions(slot, target, &mut actions);
    (!actions.is_empty()).then(|| PendingAvm1Work::Detached {
        root_target: target.to_owned(),
        root_slot: Box::new(slot.clone()),
        actions,
    })
}

pub(super) fn pending_action(target: String, actions: Vec<u8>) -> PendingAvm1Work {
    PendingAvm1Work::Action { target, actions }
}

pub(super) fn prune_dangling_masks_in_map(
    display_list: &mut BTreeMap<u16, LiveSlot>,
    root_display_list: &BTreeMap<u16, LiveSlot>,
) {
    let depths: Vec<u16> = display_list.keys().copied().collect();
    for depth in depths {
        let clear_mask = display_list
            .get(&depth)
            .and_then(|s| s.masked_by.as_deref())
            .is_some_and(|mask_id| find_slot_by_identity(root_display_list, mask_id).is_none());
        if let Some(slot) = display_list.get_mut(&depth) {
            if clear_mask {
                slot.masked_by = None;
            }
            prune_dangling_masks_in_map(&mut slot.children, root_display_list);
        }
    }
}

pub(super) fn ecma_f64_to_i32(n: f64) -> i32 {
    if !n.is_finite() || n == 0.0 {
        return 0;
    }
    let int = n.trunc();
    let two32 = 4_294_967_296.0_f64;
    let mut m = int % two32;
    if m < 0.0 {
        m += two32;
    }
    (m as u32) as i32
}

pub(super) fn ecma_f64_to_u32(n: f64) -> u32 {
    ecma_f64_to_i32(n) as u32
}

pub(super) fn any_playing_descendant(display_list: &BTreeMap<u16, LiveSlot>) -> bool {
    display_list.values().any(|slot| {
        (slot.playing && slot.total_frames > 1) || any_playing_descendant(&slot.children)
    })
}

impl SwfBytecodePlayer {
    /// Build the transient runtime `Symbol` override table for `symbol_id` and any nested
    /// `SwfSprite` instances without cloning `project` (`T-230`).
    pub fn build_display_list_symbol_overrides(
        &self,
        project: &Project,
        symbol_id: &str,
        frame: u32,
    ) -> HashMap<String, Symbol> {
        let mut overrides = HashMap::new();
        let entries = self.display_list();
        let mut next_runtime_symbol = 0usize;
        let instance_symbols = self.materialize_runtime_symbol_overrides(
            project,
            &mut overrides,
            &entries,
            "",
            0,
            &mut next_runtime_symbol,
        );
        let offset = if symbol_id == project.root {
            self.stage_offset
        } else {
            (0.0, 0.0)
        };
        let root_drawn = if symbol_id == project.root {
            self.root_graphics.materialized_elements("swf-rt-root")
        } else {
            Vec::new()
        };
        if let Some(target_sym) = Self::build_overridden_symbol(
            project,
            &overrides,
            symbol_id,
            symbol_id,
            frame,
            &entries,
            "",
            offset,
            &instance_symbols,
            &root_drawn,
            true,
        ) {
            overrides.insert(symbol_id.to_owned(), target_sym);
        }
        overrides
    }

    /// Project the current runtime display-list state onto an ephemeral clone of `project`
    /// without mutating the authored `Project`.
    pub fn project_with_display_list_overlay(
        &self,
        project: &Project,
        symbol_id: &str,
        frame: u32,
    ) -> Project {
        let mut overlay = project.clone();
        let mut overrides = self.build_display_list_symbol_overrides(project, symbol_id, frame);
        if let Some(target_sym) = overrides.remove(symbol_id)
            && let Some(slot) = overlay.symbol_mut(symbol_id)
        {
            *slot = target_sym;
        }
        let mut extra: Vec<(String, Symbol)> = overrides.into_iter().collect();
        extra.sort_by(|a, b| a.0.cmp(&b.0));
        for (_, sym) in extra {
            overlay.symbols.push(sym);
        }
        overlay.upgrade_format_for_features();
        overlay
    }

    /// Evaluate the native Stage `AuthoredScene` for the current `SwfBytecodePlayer` display-list
    /// state directly against `project` + transient symbol overrides without cloning `project`
    /// (`T-230`).
    pub fn evaluate_stage_scene(
        &self,
        project: &Project,
        symbol_id: &str,
        frame: u32,
        show_masking: bool,
    ) -> AuthoredScene {
        let overrides = self.build_display_list_symbol_overrides(project, symbol_id, frame);
        evaluate_scene_with_symbol_overrides(
            project,
            &overrides,
            symbol_id,
            frame,
            true,
            show_masking,
        )
    }

    /// Synthesize transient runtime `Symbol` overrides per active `SwfSprite` instance without
    /// cloning `project` (`T-230`). Reusing the authored `swf-sym-*` symbol would make two
    /// instances of the same character overwrite each other's frame and descendants.
    pub(super) fn materialize_runtime_symbol_overrides(
        &self,
        project: &Project,
        overrides: &mut HashMap<String, Symbol>,
        entries: &[SwfDisplayEntry],
        parent_identity: &str,
        runtime_depth: usize,
        next_runtime_symbol: &mut usize,
    ) -> BTreeMap<u16, String> {
        let mut symbols = BTreeMap::new();
        for entry in entries {
            let entry_identity =
                join_target_path(parent_identity, &format!("depth{}", entry.depth));
            let slot_drawn = self
                .target_slot(&entry_identity)
                .filter(|s| !s.graphics.is_empty())
                .map(|s| {
                    s.graphics
                        .materialized_elements(&format!("swf-rt-{}", entry.depth))
                })
                .unwrap_or_default();
            if entry.character_id == 0 {
                if runtime_depth >= MAX_RUNTIME_SPRITE_DEPTH {
                    continue;
                }
                let runtime_symbol_id = loop {
                    *next_runtime_symbol = next_runtime_symbol.saturating_add(1);
                    let candidate = format!("swf-runtime-overlay-{}", *next_runtime_symbol);
                    if project.symbol(&candidate).is_none() && !overrides.contains_key(&candidate) {
                        break candidate;
                    }
                };
                let template_id = format!("{runtime_symbol_id}-base");
                overrides.insert(
                    template_id.clone(),
                    Symbol {
                        id: template_id.clone(),
                        name: if entry.name.is_empty() {
                            format!("DynamicClip {}", entry.depth)
                        } else {
                            entry.name.clone()
                        },
                        kind: SymbolKind::MovieClip,
                        folder: String::new(),
                        class_name: None,
                        linkage_id: None,
                        timeline: Timeline {
                            frame_count: 1,
                            layers: vec![Layer {
                                id: format!("{template_id}-empty"),
                                name: "Layer 1".into(),
                                visible: true,
                                locked: false,
                                keyframes: vec![Keyframe {
                                    frame: 0,
                                    ..Default::default()
                                }],
                                properties: LayerProperties::default(),
                            }],
                            scripts: Vec::new(),
                            guides: Vec::new(),
                        },
                    },
                );
                let child_symbols = self.materialize_runtime_symbol_overrides(
                    project,
                    overrides,
                    &entry.children,
                    &entry_identity,
                    runtime_depth + 1,
                    next_runtime_symbol,
                );
                let built = Self::build_overridden_symbol(
                    project,
                    overrides,
                    &template_id,
                    &runtime_symbol_id,
                    entry.current_frame,
                    &entry.children,
                    &entry_identity,
                    (0.0, 0.0),
                    &child_symbols,
                    &slot_drawn,
                    false,
                );
                overrides.remove(&template_id);
                if let Some(mut runtime_symbol) = built {
                    runtime_symbol.class_name = None;
                    runtime_symbol.linkage_id = None;
                    overrides.insert(runtime_symbol_id.clone(), runtime_symbol);
                    symbols.insert(entry.depth, runtime_symbol_id);
                }
                continue;
            }

            let base_symbol_id = format!("swf-sym-{}", entry.character_id);
            let has_dynamic_children = !entry.children.is_empty();
            let has_drawn = !slot_drawn.is_empty();
            if runtime_depth >= MAX_RUNTIME_SPRITE_DEPTH
                || (!self.sprites.contains_key(&entry.character_id)
                    && !has_dynamic_children
                    && !has_drawn)
            {
                symbols.insert(entry.depth, base_symbol_id);
                continue;
            }
            if project.symbol(&base_symbol_id).is_none() && !overrides.contains_key(&base_symbol_id)
            {
                continue;
            }
            let runtime_symbol_id = loop {
                *next_runtime_symbol = next_runtime_symbol.saturating_add(1);
                let candidate = format!("swf-runtime-overlay-{}", *next_runtime_symbol);
                if project.symbol(&candidate).is_none() && !overrides.contains_key(&candidate) {
                    break candidate;
                }
            };

            let child_symbols = self.materialize_runtime_symbol_overrides(
                project,
                overrides,
                &entry.children,
                &entry_identity,
                runtime_depth + 1,
                next_runtime_symbol,
            );
            if let Some(mut runtime_symbol) = Self::build_overridden_symbol(
                project,
                overrides,
                &base_symbol_id,
                &runtime_symbol_id,
                entry.current_frame,
                &entry.children,
                &entry_identity,
                (0.0, 0.0),
                &child_symbols,
                &slot_drawn,
                false,
            ) {
                runtime_symbol.class_name = None;
                runtime_symbol.linkage_id = None;
                overrides.insert(runtime_symbol_id.clone(), runtime_symbol);
                symbols.insert(entry.depth, runtime_symbol_id);
            }
        }
        symbols
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_overridden_symbol(
        project: &Project,
        overrides: &HashMap<String, Symbol>,
        source_symbol_id: &str,
        target_symbol_id: &str,
        frame: u32,
        entries: &[SwfDisplayEntry],
        parent_identity: &str,
        offset: (f64, f64),
        instance_symbols: &BTreeMap<u16, String>,
        drawn_elements: &[Element],
        match_existing_layer_elements: bool,
    ) -> Option<Symbol> {
        let base_sym = overrides
            .get(source_symbol_id)
            .or_else(|| project.symbol(source_symbol_id))?;
        let mut layers: Vec<Layer> =
            Vec::with_capacity(entries.len() + usize::from(!drawn_elements.is_empty()).max(1));
        if !drawn_elements.is_empty() {
            let mut shifted = Vec::with_capacity(drawn_elements.len());
            for (idx, raw_el) in drawn_elements.iter().enumerate() {
                let mut el = raw_el.clone();
                el.id = format!("{target_symbol_id}-rt-draw-{}", idx + 1);
                el.transform.x += offset.0;
                el.transform.y += offset.1;
                shifted.push(el);
            }
            let mut keyframes = Vec::with_capacity(2);
            if frame > 0 {
                keyframes.push(Keyframe {
                    frame: 0,
                    ..Default::default()
                });
            }
            keyframes.push(Keyframe {
                frame,
                elements: shifted,
                ..Default::default()
            });
            layers.push(Layer {
                id: format!("{target_symbol_id}-rt-graphics"),
                name: "Runtime Graphics".into(),
                visible: true,
                locked: false,
                keyframes,
                properties: LayerProperties::default(),
            });
        }
        let mut depth_to_layer_idx: BTreeMap<u16, usize> = BTreeMap::new();
        let mut identity_to_layer_idx: HashMap<String, usize> = HashMap::new();
        let mut layer_char_sym_ids: Vec<String> = vec![String::new(); layers.len()];
        let mut mask_requests: Vec<(u16, u16, usize)> = Vec::new();
        let mut dynamic_mask_requests: Vec<(usize, String)> = Vec::new();

        for entry in entries {
            let char_sym_id = instance_symbols
                .get(&entry.depth)
                .cloned()
                .unwrap_or_else(|| format!("swf-sym-{}", entry.character_id));
            let Some(child_sym) = overrides
                .get(&char_sym_id)
                .or_else(|| project.symbol(&char_sym_id))
            else {
                continue;
            };
            let playback = if let Some(ratio) = entry.ratio
                && child_sym.kind == SymbolKind::Graphic
                && child_sym.timeline.frame_count > 1
            {
                let max_idx = child_sym.timeline.frame_count.saturating_sub(1);
                let morph_frame = ((u32::from(ratio) * max_idx + 32767) / 65535).min(max_idx);
                InstancePlayback::SingleFrame(morph_frame)
            } else {
                let max_f = child_sym.timeline.frame_count.max(1);
                InstancePlayback::SingleFrame(entry.current_frame.min(max_f.saturating_sub(1)))
            };
            let depth_layer_id = format!("{target_symbol_id}-d{}", entry.depth);
            let existing_elem_id = if match_existing_layer_elements {
                base_sym
                    .timeline
                    .layers
                    .iter()
                    .find(|l| l.id == depth_layer_id)
                    .and_then(|layer| {
                        layer
                            .key_at(frame)
                            .and_then(|k| k.elements.first())
                            .or_else(|| layer.keyframes.iter().flat_map(|k| &k.elements).next())
                    })
                    .map(|e| e.id.clone())
            } else {
                None
            }
            .unwrap_or_else(|| format!("swf-el-{target_symbol_id}-d{}-s1", entry.depth));
            let el = Element {
                id: existing_elem_id,
                name: entry.name.clone(),
                visible: entry.visible,
                kind: ElementKind::Instance {
                    symbol_id: char_sym_id.clone(),
                    playback,
                },
                transform: Transform {
                    x: entry.x + offset.0,
                    y: entry.y + offset.1,
                    scale_x: entry.scale_x / 100.0,
                    scale_y: entry.scale_y / 100.0,
                    rotation: entry.rotation,
                    alpha: (entry.alpha / 100.0).clamp(0.0, 1.0),
                    skew: entry.skew,
                    color_transform: entry.color_transform,
                    pivot: None,
                    primitive: None,
                },
                filters: entry.filters.clone(),
                blend_mode: entry.blend_mode,
                cache_as_bitmap: entry.cache_as_bitmap,
                scripts: Vec::new(),
                accessibility: None,
                ..Element::default()
            };
            let mut keyframes = Vec::with_capacity(2);
            if frame > 0 {
                keyframes.push(Keyframe {
                    frame: 0,
                    ..Default::default()
                });
            }
            keyframes.push(Keyframe {
                frame,
                elements: vec![el],
                ..Default::default()
            });
            let layer_idx = layers.len();
            layers.push(Layer {
                id: depth_layer_id,
                name: format!("Layer {}", layer_idx + 1),
                visible: true,
                locked: false,
                keyframes,
                properties: LayerProperties::default(),
            });
            layer_char_sym_ids.push(char_sym_id);
            depth_to_layer_idx.insert(entry.depth, layer_idx);
            let entry_identity =
                join_target_path(parent_identity, &format!("depth{}", entry.depth));
            identity_to_layer_idx.insert(entry_identity, layer_idx);
            if let Some(clip_end) = entry.clip_depth
                && clip_end > entry.depth
            {
                mask_requests.push((entry.depth, clip_end, layer_idx));
            }
            if let Some(mask_id) = &entry.masked_by {
                dynamic_mask_requests.push((layer_idx, mask_id.clone()));
            }
        }

        for (mask_depth, clip_end, mask_lidx) in mask_requests {
            if layers[mask_lidx].properties.mask.is_some() {
                continue;
            }
            let Some(mask_mode) = runtime_symbol_mask_mode(
                project,
                overrides,
                &layer_char_sym_ids[mask_lidx],
                &mut std::collections::HashSet::new(),
            ) else {
                continue;
            };
            let masked_indices: Vec<usize> = depth_to_layer_idx
                .range((
                    std::ops::Bound::Excluded(mask_depth),
                    std::ops::Bound::Included(clip_end),
                ))
                .map(|(_, &idx)| idx)
                .filter(|&idx| {
                    layers[idx].properties.role == LayerRole::Normal
                        && layers[idx].properties.mask.is_none()
                })
                .collect();
            if masked_indices.is_empty() {
                continue;
            }
            let mask_id = layers[mask_lidx].id.clone();
            layers[mask_lidx].properties.role = LayerRole::Mask;
            layers[mask_lidx].properties.mask_mode = mask_mode;
            for idx in masked_indices {
                layers[idx].properties.mask = Some(mask_id.clone());
            }
        }

        for (maskee_lidx, mask_ident) in dynamic_mask_requests {
            let Some(&mask_lidx) = identity_to_layer_idx.get(&mask_ident) else {
                continue;
            };
            if mask_lidx == maskee_lidx
                || layers[maskee_lidx].properties.role != LayerRole::Normal
                || layers[maskee_lidx].properties.mask.is_some()
                || layers[mask_lidx].properties.mask.is_some()
            {
                continue;
            }
            let Some(mask_mode) = runtime_symbol_mask_mode(
                project,
                overrides,
                &layer_char_sym_ids[mask_lidx],
                &mut std::collections::HashSet::new(),
            ) else {
                continue;
            };
            let mask_id = layers[mask_lidx].id.clone();
            layers[mask_lidx].properties.role = LayerRole::Mask;
            if mask_mode == MaskMode::Alpha {
                layers[mask_lidx].properties.mask_mode = MaskMode::Alpha;
            }
            layers[maskee_lidx].properties.mask = Some(mask_id);
        }

        if layers.is_empty() {
            layers.push(Layer {
                id: format!("{target_symbol_id}-rt-empty"),
                name: "Layer 1".into(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    ..Default::default()
                }],
                properties: LayerProperties::default(),
            });
        }

        let mut scripts = base_sym.timeline.scripts.clone();
        for fs in &mut scripts {
            if fs
                .layer
                .as_deref()
                .is_some_and(|lid| !layers.iter().any(|l| l.id == lid))
            {
                fs.layer = None;
            }
        }

        Some(Symbol {
            id: target_symbol_id.to_owned(),
            name: base_sym.name.clone(),
            kind: base_sym.kind,
            folder: base_sym.folder.clone(),
            class_name: base_sym.class_name.clone(),
            linkage_id: base_sym.linkage_id.clone(),
            timeline: Timeline {
                frame_count: base_sym.timeline.frame_count.max(frame + 1),
                layers,
                scripts,
                guides: base_sym.timeline.guides.clone(),
            },
        })
    }

    pub(super) fn prepare_root_display_list_jump(
        &mut self,
        source_frame: u32,
        destination_frame: u32,
    ) {
        let rewinding = destination_frame < source_frame;
        let start_frame = if rewinding {
            let mut work = Vec::new();
            for (&depth, slot) in &self.display_list {
                if let Some(unload) = detached_unload_work(slot, &live_slot_segment(depth, slot)) {
                    work.push(unload);
                }
            }
            self.display_list.clear();
            self.removed_timeline_depths.clear();
            self.last_applied_frame = None;
            self.execute_avm1_work_list(work);
            0
        } else {
            source_frame.saturating_add(1)
        };
        // An Unload handler can redirect the root while the old display tree is detached. Honor
        // that final target and rebuild every intermediate frame exactly once before entering it.
        let destination_frame = self.current_frame;
        for frame in start_frame..destination_frame {
            let Some(tags) = self.root_timeline.frames.get(frame as usize).cloned() else {
                continue;
            };
            for tag in tags {
                match tag.code {
                    4 | 26 | 70 => {
                        let work = self.apply_place_tag(tag.code, &tag.payload);
                        self.execute_avm1_work_list(work);
                    }
                    5 | 28 => {
                        let work = self.apply_remove_tag(tag.code, &tag.payload);
                        self.execute_avm1_work_list(work);
                    }
                    _ => {}
                }
            }
            let inter_depths: Vec<u16> = self.display_list.keys().copied().collect();
            for d in inter_depths {
                self.step_sprite_frame_actions(d, frame > start_frame);
            }
        }
        clear_placed_this_step_in_map(&mut self.display_list);
        self.last_applied_frame = destination_frame.checked_sub(1);
    }

    pub(super) fn prune_dangling_masks(&mut self) {
        let snapshot = self.display_list.clone();
        prune_dangling_masks_in_map(&mut self.display_list, &snapshot);
    }

    pub(super) fn step_sprite_frame_actions(&mut self, depth: u16, advance_timeline: bool) {
        let mut pending_actions = Vec::new();
        let mut diagnostics = Vec::new();
        if let Some(slot) = self.display_list.get_mut(&depth) {
            let target_name = live_slot_segment(depth, slot);
            Self::sync_sprite_slot_recursive(
                slot,
                &self.sprites,
                self.swf_version,
                advance_timeline,
                0,
                &target_name,
                &mut pending_actions,
                &mut diagnostics,
            );
        }
        self.prune_dangling_masks();
        for diagnostic in diagnostics {
            self.record_unsupported_diagnostic(diagnostic);
        }
        self.execute_avm1_work_list(pending_actions);
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn sync_sprite_slot_recursive(
        slot: &mut LiveSlot,
        sprites: &HashMap<u16, TimelineDef>,
        swf_version: u8,
        advance_current: bool,
        runtime_depth: usize,
        target_path: &str,
        pending_actions: &mut Vec<PendingAvm1Work>,
        diagnostics: &mut Vec<String>,
    ) {
        if runtime_depth >= MAX_RUNTIME_SPRITE_DEPTH {
            diagnostics.push(format!(
                "SWF nested sprite depth exceeded the {MAX_RUNTIME_SPRITE_DEPTH}-level runtime limit"
            ));
            return;
        }

        if advance_current {
            pending_actions.extend(
                slot.clip_handlers
                    .iter()
                    .filter(|handler| (handler.event_flags & CLIP_EVENT_ENTER_FRAME) != 0)
                    .map(|handler| pending_action(target_path.to_owned(), handler.actions.clone())),
            );
            if let Some(Avm1Value::Object(fn_obj_id)) = slot.props.get("onEnterFrame") {
                pending_actions.push(PendingAvm1Work::Callback {
                    target: target_path.to_owned(),
                    fn_obj_id: *fn_obj_id,
                });
            }
        }

        if advance_current
            && slot.last_applied_frame.is_some()
            && slot.playing
            && slot.total_frames > 1
        {
            slot.current_frame = (slot.current_frame + 1) % slot.total_frames;
        }

        if let Some(def) = sprites.get(&slot.character_id).cloned() {
            let previous_frame = slot.last_applied_frame;
            let frame_idx = slot.current_frame;
            if previous_frame != Some(frame_idx) {
                slot.last_applied_frame = Some(frame_idx);
                let start_frame = match previous_frame {
                    Some(previous) if frame_idx > previous => previous + 1,
                    _ => {
                        for (&depth, child) in &slot.children {
                            let child_target =
                                join_target_path(target_path, &live_slot_segment(depth, child));
                            if let Some(work) = detached_unload_work(child, &child_target) {
                                pending_actions.push(work);
                            }
                        }
                        slot.children.clear();
                        slot.removed_timeline_depths.clear();
                        0
                    }
                };
                let max_frame = frame_idx.min(def.frames.len().saturating_sub(1) as u32);
                for frame in start_frame..=max_frame {
                    if let Some(tags) = def.frames.get(frame as usize) {
                        for tag in tags {
                            match tag.code {
                                4 | 26 | 70 => {
                                    let blocked = place_tag_blocked_by_tombstone(
                                        tag.code,
                                        &tag.payload,
                                        &slot.removed_timeline_depths,
                                        &slot.children,
                                    );
                                    if !blocked
                                        && let Some(depth) = place_tag_depth(tag.code, &tag.payload)
                                        && let Some(child) = slot.children.get(&depth)
                                        && place_tag_replaces_slot(tag.code, &tag.payload)
                                    {
                                        let replaced_target = join_target_path(
                                            target_path,
                                            &live_slot_segment(depth, child),
                                        );
                                        if let Some(work) =
                                            detached_unload_work(child, &replaced_target)
                                        {
                                            pending_actions.push(work);
                                        }
                                    }
                                    let (load_actions, unmodeled_filter) = apply_place_tag_to_map(
                                        &mut slot.children,
                                        &mut slot.removed_timeline_depths,
                                        sprites,
                                        swf_version,
                                        tag.code,
                                        &tag.payload,
                                    );
                                    if let Some(depth) = place_tag_depth(tag.code, &tag.payload)
                                        && let Some(child) = slot.children.get(&depth)
                                    {
                                        let child_path = join_target_path(
                                            target_path,
                                            &live_slot_segment(depth, child),
                                        );
                                        pending_actions.extend(load_actions.into_iter().map(
                                            |actions| pending_action(child_path.clone(), actions),
                                        ));
                                    }
                                    if unmodeled_filter {
                                        diagnostics.push(
                                            "SWF PlaceObject3 filter list contained an unmodeled filter"
                                                .to_owned(),
                                        );
                                    }
                                }
                                5 | 28 => {
                                    if let Some(depth) = remove_tag_depth(tag.code, &tag.payload)
                                        && let Some(child) = slot.children.get(&depth)
                                        && !child.is_dynamic
                                    {
                                        let removed_target = join_target_path(
                                            target_path,
                                            &live_slot_segment(depth, child),
                                        );
                                        if let Some(work) =
                                            detached_unload_work(child, &removed_target)
                                        {
                                            pending_actions.push(work);
                                        }
                                    }
                                    let _ = apply_remove_tag_to_map(
                                        &mut slot.children,
                                        &mut slot.removed_timeline_depths,
                                        tag.code,
                                        &tag.payload,
                                    );
                                }
                                _ => {}
                            }
                        }
                    }
                }
                if let Some(tags) = def.frames.get(frame_idx as usize) {
                    pending_actions.extend(
                        tags.iter()
                            .filter(|tag| tag.code == 12)
                            .map(|tag| pending_action(target_path.to_owned(), tag.payload.clone())),
                    );
                }
            }
        }

        for (&child_depth, child) in &mut slot.children {
            let child_path = join_target_path(target_path, &live_slot_segment(child_depth, child));
            Self::sync_sprite_slot_recursive(
                child,
                sprites,
                swf_version,
                advance_current,
                runtime_depth + 1,
                &child_path,
                pending_actions,
                diagnostics,
            );
        }
    }

    pub(super) fn apply_place_tag(
        &mut self,
        tag_code: u16,
        payload: &[u8],
    ) -> Vec<PendingAvm1Work> {
        let mut work = Vec::new();
        let blocked = place_tag_blocked_by_tombstone(
            tag_code,
            payload,
            &self.removed_timeline_depths,
            &self.display_list,
        );
        let replaced = !blocked && place_tag_replaces_slot(tag_code, payload);
        if let Some(depth) = place_tag_depth(tag_code, payload)
            && let Some(slot) = self.display_list.get(&depth)
            && replaced
            && let Some(unload) = detached_unload_work(slot, &live_slot_segment(depth, slot))
        {
            work.push(unload);
        }
        let (load_scripts, unmodeled_filter) = apply_place_tag_to_map(
            &mut self.display_list,
            &mut self.removed_timeline_depths,
            &self.sprites,
            self.swf_version,
            tag_code,
            payload,
        );
        if replaced {
            self.prune_dangling_masks();
        }
        if unmodeled_filter {
            self.record_unsupported_diagnostic(
                "SWF PlaceObject3 filter list contained an unmodeled filter".to_owned(),
            );
        }
        let Some(depth) = place_tag_depth(tag_code, payload) else {
            return work;
        };
        let Some(slot) = self.display_list.get(&depth) else {
            return work;
        };
        let target = live_slot_segment(depth, slot);
        work.extend(
            load_scripts
                .into_iter()
                .map(|actions| pending_action(target.clone(), actions)),
        );
        work
    }

    pub(super) fn apply_place_tag_avm1(
        &mut self,
        tag_code: u16,
        payload: &[u8],
        include_enter_frame_on_f0: bool,
    ) -> Vec<PendingAvm1Work> {
        let mut work = Vec::new();
        let blocked = place_tag_blocked_by_tombstone(
            tag_code,
            payload,
            &self.removed_timeline_depths,
            &self.display_list,
        );
        let Some(depth) = place_tag_depth(tag_code, payload) else {
            return work;
        };
        let is_move = tag_code != 4 && payload.first().is_some_and(|f| (f & 0x01) != 0);
        let has_char = place_tag_replaces_slot(tag_code, payload);
        let had_existing = self.display_list.contains_key(&depth);
        let will_create_new_slot = !blocked && has_char && (!is_move || had_existing);

        if will_create_new_slot
            && let Some(slot) = self.display_list.get(&depth)
            && let Some(unload) = detached_unload_work(slot, &live_slot_segment(depth, slot))
        {
            work.push(unload);
        }

        let (_legacy_load_scripts, unmodeled_filter) = apply_place_tag_to_map(
            &mut self.display_list,
            &mut self.removed_timeline_depths,
            &self.sprites,
            self.swf_version,
            tag_code,
            payload,
        );
        if will_create_new_slot && had_existing {
            self.prune_dangling_masks();
        }
        if unmodeled_filter {
            self.record_unsupported_diagnostic(
                "SWF PlaceObject3 filter list contained an unmodeled filter".to_owned(),
            );
        }
        if will_create_new_slot && let Some(slot) = self.display_list.get(&depth) {
            let target = live_slot_segment(depth, slot);
            work.extend(self.instantiate_avm1_placed_subtree(
                &target,
                0,
                include_enter_frame_on_f0,
            ));
        }
        work
    }

    pub(super) fn instantiate_avm1_placed_subtree(
        &mut self,
        root_target: &str,
        runtime_depth: usize,
        include_enter_frame: bool,
    ) -> Vec<PendingAvm1Work> {
        let mut diagnostics = Vec::new();
        let sprites = self.sprites.clone();
        let swf_version = self.swf_version;
        if let Some(root_slot) = self.target_slot_mut(root_target) {
            populate_placed_slot_frame0_recursive(
                root_slot,
                &sprites,
                swf_version,
                runtime_depth,
                &mut diagnostics,
            );
        }
        for d in diagnostics {
            self.record_unsupported_diagnostic(d);
        }

        let mut preorder = Vec::new();
        if let Some(root_slot) = self.target_slot(root_target) {
            collect_placed_subtree_preorder(root_slot, root_target, &mut preorder);
        }

        // 1. Immediate parent-first ClipEventInitialize
        for (target, _) in &preorder {
            let init_actions: Vec<Vec<u8>> = self
                .target_slot(target)
                .map(|s| {
                    s.clip_handlers
                        .iter()
                        .filter(|h| (h.event_flags & CLIP_EVENT_INITIALIZE) != 0)
                        .map(|h| h.actions.clone())
                        .collect()
                })
                .unwrap_or_default();
            for code in init_actions {
                self.execute_avm1_for_target(target, &code);
            }
        }

        // 2. Immediate parent-first ClipEventConstruct + Object.registerClass constructor
        for (target, cid) in &preorder {
            let construct_actions: Vec<Vec<u8>> = self
                .target_slot(target)
                .map(|s| {
                    s.clip_handlers
                        .iter()
                        .filter(|h| (h.event_flags & CLIP_EVENT_CONSTRUCT) != 0)
                        .map(|h| h.actions.clone())
                        .collect()
                })
                .unwrap_or_default();
            for code in construct_actions {
                self.execute_avm1_for_target(target, &code);
            }
            if let Some(ctor_id) = self.exports.iter().find_map(|(name, &exp_cid)| {
                (exp_cid == *cid)
                    .then(|| self.avm1_registered_classes.get(name).copied())
                    .flatten()
            }) {
                let proto = self.get_avm1_member(&Avm1Value::Object(ctor_id), "prototype");
                if let Some(slot) = self.target_slot_mut(target) {
                    slot.props.insert("__proto__".to_owned(), proto);
                }
                if let Some(func) = self
                    .avm1_objects
                    .get(ctor_id)
                    .and_then(|o| o.function.clone())
                {
                    let super_ctor = self
                        .avm1_objects
                        .get(ctor_id)
                        .and_then(|o| o.props.get("__super_ctor__"))
                        .and_then(|v| match v {
                            Avm1Value::Object(sid) => Some(*sid),
                            _ => None,
                        });
                    let mut steps = 0usize;
                    let _ = self.invoke_avm1_func_with_this(
                        &func,
                        Avm1Value::MovieClipIdentity(target.clone()),
                        super_ctor,
                        Vec::new(),
                        &mut steps,
                        0,
                    );
                }
            }
        }

        // 3. Deferred parent-first ClipEventLoad (+ Frame-0 EnterFrame if requested) + sprite Frame-0 DoAction
        let mut deferred = Vec::new();
        for (target, cid) in preorder {
            if let Some(slot) = self.target_slot(&target) {
                for h in &slot.clip_handlers {
                    if (h.event_flags & CLIP_EVENT_LOAD) != 0 {
                        deferred.push(pending_action(target.clone(), h.actions.clone()));
                    }
                }
                if let Some(Avm1Value::Object(fn_id)) = slot.props.get("onLoad") {
                    deferred.push(PendingAvm1Work::Callback {
                        target: target.clone(),
                        fn_obj_id: *fn_id,
                    });
                }
                if include_enter_frame {
                    for h in &slot.clip_handlers {
                        if (h.event_flags & CLIP_EVENT_ENTER_FRAME) != 0 {
                            deferred.push(pending_action(target.clone(), h.actions.clone()));
                        }
                    }
                    if let Some(Avm1Value::Object(fn_id)) = slot.props.get("onEnterFrame") {
                        deferred.push(PendingAvm1Work::Callback {
                            target: target.clone(),
                            fn_obj_id: *fn_id,
                        });
                    }
                }
            }
            if let Some(def) = self.sprites.get(&cid)
                && let Some(tags) = def.frames.first()
            {
                for tag in tags {
                    if tag.code == 12 {
                        deferred.push(pending_action(target.clone(), tag.payload.clone()));
                    }
                }
            }
        }
        deferred
    }

    pub(super) fn advance_preexisting_avm1_sprites(&mut self) -> Vec<PendingAvm1Work> {
        let mut targets = Vec::new();
        collect_preexisting_sprite_targets(&self.display_list, "", &mut targets);
        let sprites = self.sprites.clone();
        let swf_version = self.swf_version;
        let mut work = Vec::new();
        for (target, runtime_depth) in targets {
            if runtime_depth >= MAX_RUNTIME_SPRITE_DEPTH {
                continue;
            }
            let Some((cid, prev_frame, next_frame)) = self.target_slot_mut(&target).and_then(|s| {
                if s.placed_this_step {
                    return None;
                }
                if s.last_applied_frame.is_some() && s.playing && s.total_frames > 1 {
                    let prev = s.last_applied_frame;
                    s.current_frame = (s.current_frame + 1) % s.total_frames;
                    Some((s.character_id, prev, s.current_frame))
                } else {
                    None
                }
            }) else {
                continue;
            };
            if prev_frame == Some(next_frame) {
                continue;
            }
            let Some(def) = self.sprites.get(&cid).cloned() else {
                if let Some(s) = self.target_slot_mut(&target) {
                    s.last_applied_frame = Some(next_frame);
                }
                continue;
            };
            if let Some(s) = self.target_slot_mut(&target) {
                s.last_applied_frame = Some(next_frame);
            }
            let start_frame = match prev_frame {
                Some(prev) if next_frame > prev => prev + 1,
                _ => {
                    if let Some(s) = self.target_slot_mut(&target) {
                        for (&d, child) in &s.children {
                            let ct = join_target_path(&target, &live_slot_segment(d, child));
                            if let Some(u) = detached_unload_work(child, &ct) {
                                work.push(u);
                            }
                        }
                        s.children.clear();
                        s.removed_timeline_depths.clear();
                    }
                    0
                }
            };
            let max_frame = next_frame.min(def.frames.len().saturating_sub(1) as u32);
            for frame in start_frame..=max_frame {
                let Some(tags) = def.frames.get(frame as usize) else {
                    continue;
                };
                for tag in tags {
                    match tag.code {
                        4 | 26 | 70 => {
                            let Some(depth) = place_tag_depth(tag.code, &tag.payload) else {
                                continue;
                            };
                            let is_move = tag.code != 4
                                && tag.payload.first().is_some_and(|f| (f & 0x01) != 0);
                            let has_char = place_tag_replaces_slot(tag.code, &tag.payload);
                            let mut unmodeled_filter = false;
                            let mut created_child_target = None;
                            if let Some(pslot) = self.target_slot_mut(&target) {
                                let blocked = place_tag_blocked_by_tombstone(
                                    tag.code,
                                    &tag.payload,
                                    &pslot.removed_timeline_depths,
                                    &pslot.children,
                                );
                                let had_existing = pslot.children.contains_key(&depth);
                                let will_create =
                                    !blocked && has_char && (!is_move || had_existing);
                                if will_create && let Some(old_c) = pslot.children.get(&depth) {
                                    let ct =
                                        join_target_path(&target, &live_slot_segment(depth, old_c));
                                    if let Some(u) = detached_unload_work(old_c, &ct) {
                                        work.push(u);
                                    }
                                }
                                let (_, uf) = apply_place_tag_to_map(
                                    &mut pslot.children,
                                    &mut pslot.removed_timeline_depths,
                                    &sprites,
                                    swf_version,
                                    tag.code,
                                    &tag.payload,
                                );
                                unmodeled_filter = uf;
                                if will_create && let Some(new_c) = pslot.children.get(&depth) {
                                    created_child_target = Some(join_target_path(
                                        &target,
                                        &live_slot_segment(depth, new_c),
                                    ));
                                }
                            }
                            if unmodeled_filter {
                                self.record_unsupported_diagnostic(
                                    "SWF PlaceObject3 filter list contained an unmodeled filter"
                                        .to_owned(),
                                );
                            }
                            if let Some(ct) = created_child_target {
                                work.extend(self.instantiate_avm1_placed_subtree(
                                    &ct,
                                    runtime_depth + 1,
                                    false,
                                ));
                            }
                        }
                        5 | 28 => {
                            if let Some(pslot) = self.target_slot_mut(&target) {
                                if let Some(depth) = remove_tag_depth(tag.code, &tag.payload)
                                    && let Some(child) = pslot.children.get(&depth)
                                    && !child.is_dynamic
                                {
                                    let ct =
                                        join_target_path(&target, &live_slot_segment(depth, child));
                                    if let Some(u) = detached_unload_work(child, &ct) {
                                        work.push(u);
                                    }
                                }
                                let _ = apply_remove_tag_to_map(
                                    &mut pslot.children,
                                    &mut pslot.removed_timeline_depths,
                                    tag.code,
                                    &tag.payload,
                                );
                            }
                        }
                        _ => {}
                    }
                }
            }
            if let Some(tags) = def.frames.get(next_frame as usize) {
                for tag in tags {
                    if tag.code == 12 {
                        work.push(pending_action(target.clone(), tag.payload.clone()));
                    }
                }
            }
        }
        self.prune_dangling_masks();
        work
    }

    pub(super) fn apply_remove_tag(
        &mut self,
        tag_code: u16,
        payload: &[u8],
    ) -> Vec<PendingAvm1Work> {
        let mut work = Vec::new();
        if let Some(depth) = remove_tag_depth(tag_code, payload)
            && let Some(slot) = self.display_list.get(&depth)
            && !slot.is_dynamic
        {
            let target = live_slot_segment(depth, slot);
            if let Some(unload) = detached_unload_work(slot, &target) {
                work.push(unload);
            }
        }
        let _ = apply_remove_tag_to_map(
            &mut self.display_list,
            &mut self.removed_timeline_depths,
            tag_code,
            payload,
        );
        self.prune_dangling_masks();
        work
    }
}

pub(super) fn parse_timeline_tags(
    bytes: &[u8],
    swf_version: u8,
    declared_frames: u16,
) -> TimelineDef {
    let mut frames = Vec::new();
    let mut cur_tags = Vec::new();
    let mut labels = Vec::new();
    walk_swf_tags(bytes, |code, payload| {
        match code {
            0 => return false,
            43 => {
                let mut lr = SwfSlice::new(payload);
                if let Some((lbl, _)) = lr.c_string(swf_version) {
                    labels.push((lbl, frames.len() as u32));
                }
                cur_tags.push(RawFrameTag {
                    code,
                    payload: payload.to_vec(),
                });
            }
            1 => {
                frames.push(std::mem::take(&mut cur_tags));
            }
            4 | 5 | 12 | 15 | 18 | 19 | 26 | 28 | 45 | 70 => {
                cur_tags.push(RawFrameTag {
                    code,
                    payload: payload.to_vec(),
                });
            }
            _ => {}
        }
        true
    });
    if !cur_tags.is_empty() || frames.is_empty() {
        frames.push(cur_tags);
    }
    let total_frames = (frames.len() as u32).max(u32::from(declared_frames).max(1));
    TimelineDef {
        total_frames,
        frames,
        labels,
    }
}

fn populate_placed_slot_frame0_recursive(
    slot: &mut LiveSlot,
    sprites: &HashMap<u16, TimelineDef>,
    swf_version: u8,
    runtime_depth: usize,
    diagnostics: &mut Vec<String>,
) {
    slot.placed_this_step = true;
    if runtime_depth >= MAX_RUNTIME_SPRITE_DEPTH {
        diagnostics.push(format!(
            "SWF nested sprite depth exceeded the {MAX_RUNTIME_SPRITE_DEPTH}-level runtime limit"
        ));
        return;
    }
    let Some(def) = sprites.get(&slot.character_id) else {
        slot.last_applied_frame = Some(0);
        return;
    };
    slot.last_applied_frame = Some(0);
    if let Some(tags) = def.frames.first() {
        for tag in tags {
            match tag.code {
                4 | 26 | 70 => {
                    let (_, unmodeled_filter) = apply_place_tag_to_map(
                        &mut slot.children,
                        &mut slot.removed_timeline_depths,
                        sprites,
                        swf_version,
                        tag.code,
                        &tag.payload,
                    );
                    if unmodeled_filter {
                        diagnostics.push(
                            "SWF PlaceObject3 filter list contained an unmodeled filter".to_owned(),
                        );
                    }
                }
                5 | 28 => {
                    let _ = apply_remove_tag_to_map(
                        &mut slot.children,
                        &mut slot.removed_timeline_depths,
                        tag.code,
                        &tag.payload,
                    );
                }
                _ => {}
            }
        }
    }
    for child in slot.children.values_mut() {
        populate_placed_slot_frame0_recursive(
            child,
            sprites,
            swf_version,
            runtime_depth + 1,
            diagnostics,
        );
    }
}

fn collect_placed_subtree_preorder(slot: &LiveSlot, target: &str, out: &mut Vec<(String, u16)>) {
    out.push((target.to_owned(), slot.character_id));
    for (&depth, child) in &slot.children {
        let child_target = join_target_path(target, &live_slot_segment(depth, child));
        collect_placed_subtree_preorder(child, &child_target, out);
    }
}

fn collect_preexisting_sprite_targets(
    display_list: &BTreeMap<u16, LiveSlot>,
    parent_target: &str,
    out: &mut Vec<(String, usize)>,
) {
    collect_preexisting_sprite_targets_inner(display_list, parent_target, 0, out);
}

fn collect_preexisting_sprite_targets_inner(
    display_list: &BTreeMap<u16, LiveSlot>,
    parent_target: &str,
    depth_lvl: usize,
    out: &mut Vec<(String, usize)>,
) {
    for (&depth, slot) in display_list {
        if !slot.placed_this_step {
            let target = join_target_path(parent_target, &live_slot_segment(depth, slot));
            out.push((target.clone(), depth_lvl));
            collect_preexisting_sprite_targets_inner(&slot.children, &target, depth_lvl + 1, out);
        }
    }
}

pub(super) type ParsedButtonActions = (u16, Vec<(u8, u16, u16)>, Vec<ButtonCondAction>);

pub(super) fn parse_button_actions(tag_code: u16, payload: &[u8]) -> Option<ParsedButtonActions> {
    let mut r = SwfSlice::new(payload);
    let btn_id = r.u16()?;
    let mut records = Vec::new();
    if tag_code == 7 {
        for _ in 0..1024 {
            let flags = r.u8()?;
            if flags == 0 {
                break;
            }
            let cid = r.u16()?;
            let depth = r.u16()?;
            let _mat = r.matrix_px()?;
            records.push((flags & 0x0F, cid, depth));
        }
        let actions = r.rest().to_vec();
        return Some((
            btn_id,
            records,
            vec![ButtonCondAction {
                conditions: 0x0008, // DefineButton actions run on release.
                actions,
            }],
        ));
    }
    let track_as_menu = r.u8()? & 1 != 0;
    let off_pos = r.pos();
    let rel = usize::from(r.u16()?);
    let records_end = if rel == 0 {
        payload.len()
    } else {
        (off_pos + rel).min(payload.len())
    };
    for _ in 0..1024 {
        if r.pos() >= records_end {
            break;
        }
        let Some(rec_flags) = r.u8() else { break };
        if rec_flags == 0 {
            break;
        }
        let Some(cid) = r.u16() else { break };
        let Some(depth) = r.u16() else { break };
        if r.matrix_px().is_none() || r.cxform(true).is_none() {
            break;
        }
        if (rec_flags & 0x10) != 0 {
            let _ = parse_swf_filter_list(&mut r);
        }
        if (rec_flags & 0x20) != 0 {
            let _ = r.u8();
        }
        records.push((rec_flags & 0x0F, cid, depth));
    }
    if rel == 0 || off_pos + rel >= payload.len() {
        return Some((btn_id, records, Vec::new()));
    }
    let mut cond_slice = &payload[off_pos + rel..];
    let mut conds = Vec::new();
    while cond_slice.len() >= 4 {
        let size = usize::from(u16::from_le_bytes([cond_slice[0], cond_slice[1]]));
        // A push button captures drags; a menu button uses the two idle transitions.
        let tracking_flags = if track_as_menu { !0x0070 } else { !0x0180 };
        let conditions = u16::from_le_bytes([cond_slice[2], cond_slice[3]]) & tracking_flags;
        if size == 0 || size > cond_slice.len() {
            conds.push(ButtonCondAction {
                conditions,
                actions: cond_slice[4..].to_vec(),
            });
            break;
        }
        conds.push(ButtonCondAction {
            conditions,
            actions: cond_slice[4..size].to_vec(),
        });
        cond_slice = &cond_slice[size..];
    }
    Some((btn_id, records, conds))
}

pub(super) fn parse_raw_clip_handlers(
    r: &mut SwfSlice<'_>,
    swf_version: u8,
) -> Vec<ClipEventHandler> {
    let mut out = Vec::new();
    let _reserved = r.u16();
    let _all_flags = if swf_version >= 6 {
        r.u32()
    } else {
        r.u16().map(u32::from)
    };
    for _ in 0..256 {
        let Some(event_flags) = (if swf_version >= 6 {
            r.u32()
        } else {
            r.u16().map(u32::from)
        }) else {
            break;
        };
        if event_flags == 0 {
            break;
        }
        let Some(rec_size) = r.u32().map(|n| n as usize) else {
            break;
        };
        let has_key = (event_flags & CLIP_EVENT_KEY_PRESS) != 0;
        let key_code = if has_key && rec_size >= 1 {
            r.u8()
        } else {
            None
        };
        let action_len = if has_key {
            rec_size.saturating_sub(1)
        } else {
            rec_size
        };
        let Some(actions) = r.take(action_len) else {
            break;
        };
        out.push(ClipEventHandler {
            event_flags,
            key_code,
            actions: actions.to_vec(),
        });
    }
    out
}

pub(super) fn place_tag_depth(tag_code: u16, payload: &[u8]) -> Option<u16> {
    let offset = match tag_code {
        4 => 2,
        26 => 1,
        70 => 2,
        _ => return None,
    };
    payload
        .get(offset..offset + 2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
}

pub(super) fn remove_tag_depth(tag_code: u16, payload: &[u8]) -> Option<u16> {
    let offset = match tag_code {
        5 => 2,
        28 => 0,
        _ => return None,
    };
    payload
        .get(offset..offset + 2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
}

pub(super) fn place_tag_replaces_slot(tag_code: u16, payload: &[u8]) -> bool {
    if tag_code == 4 {
        return true;
    }
    payload.first().is_some_and(|flags| (flags & 0x02) != 0)
}

pub(super) fn place_tag_blocked_by_tombstone(
    tag_code: u16,
    payload: &[u8],
    removed_timeline_depths: &BTreeSet<u16>,
    display_list: &BTreeMap<u16, LiveSlot>,
) -> bool {
    if tag_code == 4 {
        return false;
    }
    let Some(&flags1) = payload.first() else {
        return false;
    };
    let is_move = (flags1 & 0x01) != 0;
    if !is_move {
        return false;
    }
    let Some(depth) = place_tag_depth(tag_code, payload) else {
        return false;
    };
    removed_timeline_depths.contains(&depth)
        || display_list.get(&depth).is_some_and(|s| s.is_dynamic)
}

pub(super) fn push_ordered_clip_lifecycle_scripts(
    handlers: &[ClipEventHandler],
    load_scripts: &mut Vec<Vec<u8>>,
) {
    for flag in [CLIP_EVENT_INITIALIZE, CLIP_EVENT_CONSTRUCT, CLIP_EVENT_LOAD] {
        for handler in handlers {
            if (handler.event_flags & flag) != 0 {
                load_scripts.push(handler.actions.clone());
            }
        }
    }
}

pub(super) fn apply_place_tag_to_map(
    display_list: &mut BTreeMap<u16, LiveSlot>,
    removed_timeline_depths: &mut BTreeSet<u16>,
    sprites: &HashMap<u16, TimelineDef>,
    swf_version: u8,
    tag_code: u16,
    payload: &[u8],
) -> (Vec<Vec<u8>>, bool) {
    let mut r = SwfSlice::new(payload);
    let mut load_scripts = Vec::new();
    let mut unmodeled_filter = false;

    if tag_code == 4 {
        let Some(character_id) = r.u16() else {
            return (load_scripts, false);
        };
        let Some(depth) = r.u16() else {
            return (load_scripts, false);
        };
        let Some(matrix) = r.matrix_px() else {
            return (load_scripts, false);
        };
        let (color_transform, alpha) = if r.remaining() > 0 {
            r.cxform(false)
                .unwrap_or_else(|| (ColorTransform::default(), 1.0))
        } else {
            (ColorTransform::default(), 1.0)
        };
        let tf = Transform::from_matrix(matrix).unwrap_or_default();
        let sprite_total = sprites
            .get(&character_id)
            .map_or(1, |s| s.total_frames.max(1));
        removed_timeline_depths.remove(&depth);
        display_list.insert(
            depth,
            LiveSlot {
                character_id,
                name: String::new(),
                matrix,
                x: tf.x,
                y: tf.y,
                scale_x: tf.scale_x * 100.0,
                scale_y: tf.scale_y * 100.0,
                rotation: tf.rotation,
                skew: tf.skew,
                alpha: alpha * 100.0,
                color_transform,
                visible: true,
                blend_mode: BlendMode::Normal,
                filters: Vec::new(),
                clip_depth: None,
                masked_by: None,
                is_dynamic: false,
                origin_timeline_depth: Some(depth),
                ratio: None,
                cache_as_bitmap: false,
                current_frame: 0,
                total_frames: sprite_total,
                playing: true,
                last_applied_frame: None,
                removed_timeline_depths: BTreeSet::new(),
                children: BTreeMap::new(),
                props: HashMap::new(),
                clip_handlers: Vec::new(),
                graphics: RuntimeGraphicsState::default(),
                placed_this_step: true,
                avm2_object_id: None,
            },
        );
        return (load_scripts, false);
    }

    let Some(flags1) = r.u8() else {
        return (load_scripts, false);
    };
    let flags2 = if tag_code == 70 {
        r.u8().unwrap_or(0)
    } else {
        0
    };
    let Some(depth) = r.u16() else {
        return (load_scripts, false);
    };

    let has_clip_actions = (flags1 & 0x80) != 0;
    let has_clip_depth = (flags1 & 0x40) != 0;
    let has_name = (flags1 & 0x20) != 0;
    let has_ratio = (flags1 & 0x10) != 0;
    let has_cxform = (flags1 & 0x08) != 0;
    let has_matrix = (flags1 & 0x04) != 0;
    let has_character = (flags1 & 0x02) != 0;
    let is_move = (flags1 & 0x01) != 0;

    if tag_code == 70 && ((flags2 & 0x08) != 0 || ((flags2 & 0x10) != 0 && has_character)) {
        let _ = r.c_string(swf_version);
    }
    let new_char = if has_character { r.u16() } else { None };
    let new_matrix = if has_matrix { r.matrix_px() } else { None };
    let new_cxform = if has_cxform { r.cxform(true) } else { None };
    let new_ratio = if has_ratio { r.u16() } else { None };
    let new_name = if has_name {
        r.c_string(swf_version).map(|(s, _)| s)
    } else {
        None
    };
    let new_clip_depth = if has_clip_depth { r.u16() } else { None };
    let mut new_visible = None;
    let mut new_blend = None;
    let mut new_filters = None;
    let mut new_cache_as_bitmap = None;
    if tag_code == 70 {
        if (flags2 & 0x01) != 0 {
            let (parsed, unmodeled) = parse_swf_filter_list(&mut r);
            new_filters = Some(parsed);
            unmodeled_filter |= unmodeled;
        }
        if (flags2 & 0x02) != 0 {
            new_blend = r.u8();
        }
        if (flags2 & 0x04) != 0 {
            new_cache_as_bitmap = r.u8().map(|v| v != 0);
        }
        if (flags2 & 0x20) != 0 {
            new_visible = r.u8().map(|v| v != 0);
        }
        if (flags2 & 0x40) != 0 {
            let _ = r.rgba();
        }
    }
    let new_handlers = if has_clip_actions {
        parse_raw_clip_handlers(&mut r, swf_version)
    } else {
        Vec::new()
    };

    if is_move {
        if removed_timeline_depths.contains(&depth)
            || display_list.get(&depth).is_some_and(|s| s.is_dynamic)
        {
            return (load_scripts, unmodeled_filter);
        }
    } else if has_character {
        removed_timeline_depths.remove(&depth);
    }

    if is_move && let Some(slot) = display_list.get_mut(&depth) {
        if let Some(character_id) = new_char {
            push_ordered_clip_lifecycle_scripts(&new_handlers, &mut load_scripts);
            let matrix = new_matrix.unwrap_or(slot.matrix);
            let transform = Transform::from_matrix(matrix).unwrap_or_default();
            let (color_transform, alpha) = new_cxform
                .map(|(color, alpha)| (color, alpha * 100.0))
                .unwrap_or((slot.color_transform, slot.alpha));
            *slot = LiveSlot {
                character_id,
                name: new_name.unwrap_or_default(),
                matrix,
                x: transform.x,
                y: transform.y,
                scale_x: transform.scale_x * 100.0,
                scale_y: transform.scale_y * 100.0,
                rotation: transform.rotation,
                skew: transform.skew,
                alpha,
                color_transform,
                visible: new_visible.unwrap_or(slot.visible),
                blend_mode: new_blend.and_then(map_blend).unwrap_or(slot.blend_mode),
                filters: new_filters.unwrap_or_else(|| slot.filters.clone()),
                clip_depth: new_clip_depth
                    .map(|value| (value > 0).then_some(value))
                    .unwrap_or(slot.clip_depth),
                masked_by: slot.masked_by.clone(),
                is_dynamic: false,
                origin_timeline_depth: Some(depth),
                ratio: new_ratio.or(slot.ratio),
                cache_as_bitmap: new_cache_as_bitmap.unwrap_or(slot.cache_as_bitmap),
                current_frame: 0,
                total_frames: sprites
                    .get(&character_id)
                    .map_or(1, |sprite| sprite.total_frames.max(1)),
                playing: true,
                last_applied_frame: None,
                removed_timeline_depths: BTreeSet::new(),
                children: BTreeMap::new(),
                props: HashMap::new(),
                clip_handlers: new_handlers,
                graphics: RuntimeGraphicsState::default(),
                placed_this_step: true,
                avm2_object_id: None,
            };
            return (load_scripts, unmodeled_filter);
        }
        if let Some(m) = new_matrix {
            slot.matrix = m;
            if let Some(tf) = Transform::from_matrix(m) {
                slot.x = tf.x;
                slot.y = tf.y;
                slot.scale_x = tf.scale_x * 100.0;
                slot.scale_y = tf.scale_y * 100.0;
                slot.rotation = tf.rotation;
                slot.skew = tf.skew;
            }
        }
        if let Some((ct, a)) = new_cxform {
            slot.color_transform = ct;
            slot.alpha = a * 100.0;
        }
        if let Some(rat) = new_ratio {
            slot.ratio = Some(rat);
        }
        if let Some(nm) = new_name {
            slot.name = nm;
        }
        if let Some(cd) = new_clip_depth {
            slot.clip_depth = (cd > 0).then_some(cd);
        }
        if let Some(vis) = new_visible {
            slot.visible = vis;
        }
        if let Some(cab) = new_cache_as_bitmap {
            slot.cache_as_bitmap = cab;
        }
        if let Some(b) = new_blend
            && let Some(bm) = map_blend(b)
        {
            slot.blend_mode = bm;
        }
        if let Some(fl) = new_filters {
            slot.filters = fl;
        }
        if has_clip_actions {
            slot.clip_handlers = new_handlers;
        }
        return (load_scripts, unmodeled_filter);
    }

    let Some(character_id) = new_char else {
        return (load_scripts, unmodeled_filter);
    };
    let matrix = new_matrix.unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    let tf = Transform::from_matrix(matrix).unwrap_or_default();
    let (color_transform, alpha) = new_cxform.map_or_else(
        || (ColorTransform::default(), 100.0),
        |(ct, a)| (ct, a * 100.0),
    );
    let sprite_total = sprites
        .get(&character_id)
        .map_or(1, |s| s.total_frames.max(1));

    push_ordered_clip_lifecycle_scripts(&new_handlers, &mut load_scripts);

    display_list.insert(
        depth,
        LiveSlot {
            character_id,
            name: new_name.unwrap_or_default(),
            matrix,
            x: tf.x,
            y: tf.y,
            scale_x: tf.scale_x * 100.0,
            scale_y: tf.scale_y * 100.0,
            rotation: tf.rotation,
            skew: tf.skew,
            alpha,
            color_transform,
            visible: new_visible.unwrap_or(true),
            blend_mode: new_blend.and_then(map_blend).unwrap_or(BlendMode::Normal),
            filters: new_filters.unwrap_or_default(),
            clip_depth: new_clip_depth.filter(|&cd| cd > 0),
            masked_by: None,
            is_dynamic: false,
            origin_timeline_depth: Some(depth),
            ratio: new_ratio,
            cache_as_bitmap: new_cache_as_bitmap.unwrap_or(false),
            current_frame: 0,
            total_frames: sprite_total,
            playing: true,
            last_applied_frame: None,
            removed_timeline_depths: BTreeSet::new(),
            children: BTreeMap::new(),
            props: HashMap::new(),
            clip_handlers: new_handlers,
            graphics: RuntimeGraphicsState::default(),
            placed_this_step: true,
            avm2_object_id: None,
        },
    );
    (load_scripts, unmodeled_filter)
}

pub(super) fn apply_remove_tag_to_map(
    display_list: &mut BTreeMap<u16, LiveSlot>,
    removed_timeline_depths: &mut BTreeSet<u16>,
    tag_code: u16,
    payload: &[u8],
) -> Vec<Vec<u8>> {
    let mut r = SwfSlice::new(payload);
    let depth = if tag_code == 5 {
        let _cid = r.u16();
        r.u16()
    } else {
        r.u16()
    };
    let mut unload_scripts = Vec::new();
    if let Some(d) = depth {
        removed_timeline_depths.remove(&d);
        if display_list.get(&d).is_some_and(|s| !s.is_dynamic)
            && let Some(removed) = display_list.remove(&d)
        {
            for h in removed.clip_handlers {
                if (h.event_flags & CLIP_EVENT_UNLOAD) != 0 {
                    unload_scripts.push(h.actions);
                }
            }
        }
    }
    unload_scripts
}

pub(super) fn map_blend(b: u8) -> Option<BlendMode> {
    match b {
        0 | 1 => Some(BlendMode::Normal),
        2 => Some(BlendMode::Layer),
        3 => Some(BlendMode::Multiply),
        4 => Some(BlendMode::Screen),
        5 => Some(BlendMode::Lighten),
        6 => Some(BlendMode::Darken),
        7 => Some(BlendMode::Difference),
        8 => Some(BlendMode::Add),
        9 => Some(BlendMode::Subtract),
        10 => Some(BlendMode::Invert),
        11 => Some(BlendMode::Alpha),
        12 => Some(BlendMode::Erase),
        13 => Some(BlendMode::Overlay),
        14 => Some(BlendMode::Hardlight),
        _ => None,
    }
}

pub(super) fn clear_mask_source_in_map(
    display_list: &mut BTreeMap<u16, LiveSlot>,
    mask_target: &str,
) {
    for slot in display_list.values_mut() {
        if slot.masked_by.as_deref() == Some(mask_target) {
            slot.masked_by = None;
        }
        clear_mask_source_in_map(&mut slot.children, mask_target);
    }
}

pub(super) fn script_depth_to_swf(script_depth: i32) -> u16 {
    (16384_i64 + i64::from(script_depth)).clamp(0, 65535) as u16
}

pub(super) fn runtime_element_supports_mask_mode(
    project: &Project,
    overrides: &HashMap<String, Symbol>,
    kind: &ElementKind,
    mode: MaskMode,
    visiting: &mut std::collections::HashSet<String>,
) -> bool {
    match kind {
        ElementKind::Path { .. } | ElementKind::PaintedPath { .. } => true,
        ElementKind::Rectangle { width, height, .. }
        | ElementKind::Ellipse { width, height, .. } => *width > 0.0 && *height > 0.0,
        ElementKind::Bitmap { resource_id, .. } if mode.uses_paint_coverage() => project
            .resources
            .iter()
            .find(|r| &r.id == resource_id)
            .is_some_and(|r| r.decode_image().is_ok()),
        ElementKind::Instance { symbol_id, .. } => {
            let Some(sym) = overrides
                .get(symbol_id)
                .or_else(|| project.symbol(symbol_id))
            else {
                return false;
            };
            if !visiting.insert(sym.id.clone()) {
                return false;
            }
            let valid = sym
                .timeline
                .layers
                .iter()
                .filter(|l| l.properties.role != LayerRole::Guide)
                .all(|layer| {
                    let effective_mode = if layer.properties.role == LayerRole::Mask {
                        layer.properties.mask_mode
                    } else {
                        mode
                    };
                    layer.keyframes.iter().flat_map(|k| &k.elements).all(|el| {
                        runtime_element_supports_mask_mode(
                            project,
                            overrides,
                            &el.kind,
                            effective_mode,
                            visiting,
                        )
                    })
                });
            visiting.remove(&sym.id);
            valid
        }
        _ => false,
    }
}

pub(super) fn runtime_symbol_mask_mode(
    project: &Project,
    overrides: &HashMap<String, Symbol>,
    sym_id: &str,
    visiting: &mut std::collections::HashSet<String>,
) -> Option<MaskMode> {
    let kind = ElementKind::Instance {
        symbol_id: sym_id.to_owned(),
        playback: InstancePlayback::Loop,
    };
    visiting.clear();
    if runtime_element_supports_mask_mode(project, overrides, &kind, MaskMode::Geometry, visiting) {
        return Some(MaskMode::Geometry);
    }
    visiting.clear();
    if runtime_element_supports_mask_mode(project, overrides, &kind, MaskMode::Alpha, visiting) {
        return Some(MaskMode::Alpha);
    }
    None
}
