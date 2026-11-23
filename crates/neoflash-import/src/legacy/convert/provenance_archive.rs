use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ProvenanceDetail {
    Full,
    NoPlacements,
    Minimal,
}

pub(super) fn update_provenance_resource(
    project: &mut Project,
    detail: &mut ProvenanceDetail,
    warned: &mut bool,
    embedded: bool,
    placements: &[serde_json::Value],
    report: &mut ImportReport,
    make_provenance: &dyn Fn(bool, ProvenanceDetail, &[serde_json::Value]) -> serde_json::Value,
) -> Result<(), String> {
    let has_placements = !placements.is_empty();
    let other_decoded: usize = project
        .resources
        .iter()
        .filter(|r| r.id != "fla-provenance")
        .map(ResourceUnit::byte_len)
        .sum();
    let candidates: &[ProvenanceDetail] = match *detail {
        ProvenanceDetail::Full => &[
            ProvenanceDetail::Full,
            ProvenanceDetail::NoPlacements,
            ProvenanceDetail::Minimal,
        ],
        ProvenanceDetail::NoPlacements => {
            &[ProvenanceDetail::NoPlacements, ProvenanceDetail::Minimal]
        }
        ProvenanceDetail::Minimal => &[ProvenanceDetail::Minimal],
    };
    let mut chosen = None;
    for &candidate in candidates {
        let bytes = serde_json::to_vec(&make_provenance(embedded, candidate, placements))
            .map_err(|e| e.to_string())?;
        if bytes.len() <= MAX_RESOURCE_BYTES
            && other_decoded.saturating_add(bytes.len()) <= MAX_RESOURCE_TOTAL_BYTES
        {
            chosen = Some((candidate, bytes));
            break;
        }
    }
    let (chosen_detail, bytes) =
        chosen.ok_or_else(|| "Import provenance exceeds the project resource budget".to_owned())?;
    if chosen_detail != ProvenanceDetail::Full && !*warned && has_placements {
        *warned = true;
        report.warning(
            "fla_provenance_compacted",
            "legacy/provenance.json",
            "Per-placement source offsets were omitted from provenance to fit project size limits; source digest, stream descriptors and container metadata are retained.",
        );
    }
    *detail = chosen_detail;
    let unit = ResourceUnit::from_bytes(
        "fla-provenance",
        "legacy/provenance.json",
        "application/json",
        &bytes,
    )
    .map_err(|e| e.to_string())?;
    if let Some(slot) = project
        .resources
        .iter_mut()
        .find(|r| r.id == "fla-provenance")
    {
        *slot = unit;
    } else {
        project.resources.push(unit);
    }
    Ok(())
}

pub(super) fn replace_group_prefix(id: &mut String, old_prefix: &str) {
    if let Some(rest) = id.strip_prefix(old_prefix) {
        *id = format!("$G{rest}");
    }
}

pub(super) fn normalized_group_timeline_signature(
    timeline: &Timeline,
    prefix: &str,
) -> Option<String> {
    let mut clone = timeline.clone();
    for layer in &mut clone.layers {
        replace_group_prefix(&mut layer.id, prefix);
        for key in &mut layer.keyframes {
            for element in &mut key.elements {
                replace_group_prefix(&mut element.id, prefix);
            }
            for bf in &mut key.bitmap_fills {
                replace_group_prefix(&mut bf.element_id, prefix);
            }
            for app in &mut key.appearances {
                replace_group_prefix(&mut app.element_id, prefix);
            }
            for fx in &mut key.vector_effects {
                replace_group_prefix(&mut fx.element_id, prefix);
            }
        }
    }
    serde_json::to_string(&clone).ok()
}

pub(super) fn deduplicate_project_for_budget(
    project: &mut Project,
    provenance: &mut [serde_json::Value],
) -> bool {
    let mut seen_media: std::collections::HashMap<(String, std::sync::Arc<str>), String> =
        std::collections::HashMap::new();
    let mut resource_remap: BTreeMap<String, String> = BTreeMap::new();
    for res in &project.resources {
        if !(res.id.starts_with("legacy-sound-") || res.id.starts_with("legacy-bitmap-")) {
            continue;
        }
        let key = (res.media_type.clone(), res.data_base64.clone());
        if let Some(canon) = seen_media.get(&key) {
            resource_remap.insert(res.id.clone(), canon.clone());
        } else {
            seen_media.insert(key, res.id.clone());
        }
    }
    if !resource_remap.is_empty() {
        project
            .resources
            .retain(|r| !resource_remap.contains_key(&r.id));
        for symbol in &mut project.symbols {
            for layer in &mut symbol.timeline.layers {
                for key in &mut layer.keyframes {
                    if let Some(sound) = &mut key.sound
                        && let Some(canon) = resource_remap.get(&sound.resource)
                    {
                        sound.resource = canon.clone();
                    }
                    for bf in &mut key.bitmap_fills {
                        if let Some(canon) = resource_remap.get(&bf.resource_id) {
                            bf.resource_id = canon.clone();
                        }
                    }
                    for element in &mut key.elements {
                        if let ElementKind::Bitmap { resource_id, .. } = &mut element.kind
                            && let Some(canon) = resource_remap.get(resource_id)
                        {
                            *resource_id = canon.clone();
                        }
                    }
                }
            }
        }
    }

    let protected: std::collections::HashSet<String> = std::iter::once(project.root.clone())
        .chain(project.scenes.iter().cloned())
        .collect();
    let mut seen_groups: std::collections::HashMap<String, (String, String)> =
        std::collections::HashMap::new();
    let mut group_remap: BTreeMap<String, (String, String, String)> = BTreeMap::new();

    for symbol in &mut project.symbols {
        if !group_remap.is_empty() {
            for element in symbol
                .timeline
                .layers
                .iter_mut()
                .flat_map(|l| &mut l.keyframes)
                .flat_map(|k| &mut k.elements)
            {
                if let ElementKind::Instance { symbol_id, .. } = &mut element.kind
                    && let Some((canon_id, _, _)) = group_remap.get(symbol_id)
                {
                    *symbol_id = canon_id.clone();
                }
            }
        }
        if symbol.kind != SymbolKind::Graphic
            || symbol.class_name.is_some()
            || symbol.linkage_id.is_some()
            || protected.contains(&symbol.id)
            || symbol.id == PLACEHOLDER
            || symbol.id.starts_with("fla-missing-bitmap-")
        {
            continue;
        }
        let Some(prefix) = symbol.id.strip_suffix("-group").map(str::to_owned) else {
            continue;
        };
        let Some(sig) = normalized_group_timeline_signature(&symbol.timeline, &prefix) else {
            continue;
        };
        if let Some((canon_id, canon_prefix)) = seen_groups.get(&sig) {
            group_remap.insert(
                symbol.id.clone(),
                (canon_id.clone(), prefix, canon_prefix.clone()),
            );
        } else {
            seen_groups.insert(sig, (symbol.id.clone(), prefix));
        }
    }

    if !group_remap.is_empty() {
        project.symbols.retain(|s| !group_remap.contains_key(&s.id));
        for symbol in &mut project.symbols {
            for element in symbol
                .timeline
                .layers
                .iter_mut()
                .flat_map(|l| &mut l.keyframes)
                .flat_map(|k| &mut k.elements)
            {
                if let ElementKind::Instance { symbol_id, .. } = &mut element.kind
                    && let Some((canon_id, _, _)) = group_remap.get(symbol_id)
                {
                    *symbol_id = canon_id.clone();
                }
            }
        }
        let prefix_pairs: Vec<(String, String)> = group_remap
            .values()
            .map(|(_, old_p, new_p)| (format!("{old_p}-member-"), format!("{new_p}-member-")))
            .collect();
        for entry in provenance.iter_mut() {
            let Some(current) = entry.get("target").and_then(|v| v.as_str()) else {
                continue;
            };
            let mut rewritten = current.to_owned();
            let mut changed = false;
            for _ in 0..48 {
                let mut step = false;
                for (old_member, new_member) in &prefix_pairs {
                    if let Some(rest) = rewritten.strip_prefix(old_member) {
                        rewritten = format!("{new_member}{rest}");
                        changed = true;
                        step = true;
                        break;
                    }
                }
                if !step {
                    break;
                }
            }
            if changed {
                entry["target"] = serde_json::Value::String(rewritten);
            }
        }
    }

    !resource_remap.is_empty() || !group_remap.is_empty()
}

pub(super) fn deduplicate_repeated_vector_paths_for_budget(
    project: &mut Project,
    provenance: &mut [serde_json::Value],
    include_single_shapes: bool,
) -> bool {
    const MAX_SYMBOL_BUDGET: usize = 9_900;
    const MAX_AUTHORED_ELEMENTS: usize = 100_000;
    const MAX_EXPANDED_NODES: usize = 100_000;

    if project.symbols.len() >= MAX_SYMBOL_BUDGET {
        return false;
    }

    let mut changed = false;
    let mut existing_ids: std::collections::HashSet<String> =
        project.symbols.iter().map(|s| s.id.clone()).collect();
    let mut next_seq = 1usize;
    let mut alloc_sym_id = |prefix: &str| -> String {
        loop {
            let cand = format!("{prefix}-{next_seq}");
            next_seq += 1;
            if existing_ids.insert(cand.clone()) {
                return cand;
            }
        }
    };

    // Part A: Under authored-element pressure (> 100,000 elements) or JSON budget
    // pressure (`include_single_shapes`), collapse identical multi-element pure-vector
    // static keyframes into shared 1-frame Graphic symbols.
    let pre_max_expanded = if include_single_shapes {
        max_expanded_display_nodes(&project.symbols)
    } else {
        0
    };
    let saved_state = if include_single_shapes && pre_max_expanded > MAX_EXPANDED_NODES / 2 {
        Some((project.symbols.clone(), provenance.to_vec()))
    } else {
        None
    };
    let total_authored: usize = project
        .symbols
        .iter()
        .flat_map(|s| &s.timeline.layers)
        .flat_map(|l| &l.keyframes)
        .map(|k| k.elements.len())
        .sum();
    if total_authored > MAX_AUTHORED_ELEMENTS || include_single_shapes {
        let allowed_leaf = symbols_allowing_leaf_instance(&project.symbols);
        let mut kf_buckets: std::collections::HashMap<String, Vec<(usize, usize, usize)>> =
            std::collections::HashMap::new();
        let mut kf_order: Vec<String> = Vec::new();
        for (si, sym) in project.symbols.iter().enumerate() {
            if !allowed_leaf.contains(&sym.id)
                || sym.id == PLACEHOLDER
                || sym.id.starts_with("fla-missing-bitmap-")
                || sym.id.starts_with("fla-shared-")
            {
                continue;
            }
            for (li, layer) in sym.timeline.layers.iter().enumerate() {
                if layer.properties.role != LayerRole::Normal {
                    continue;
                }
                let max_key_len = layer
                    .keyframes
                    .iter()
                    .map(|k| k.elements.len())
                    .max()
                    .unwrap_or(0);
                if max_key_len + 2 >= MAX_EXPANDED_NODES {
                    continue;
                }
                for (ki, key) in layer.keyframes.iter().enumerate() {
                    let prev_tweened = ki > 0 && layer.keyframes[ki - 1].tween.is_some();
                    if key.elements.len() < 2
                        || key.tween.is_some()
                        || prev_tweened
                        || key.keyed_properties.is_some()
                        || !key.shape_hints.is_empty()
                        || !key.bitmap_fills.is_empty()
                        || !key.appearances.is_empty()
                        || !key.vector_effects.is_empty()
                        || !key.asset_warps.is_empty()
                        || !key.element_parents.is_empty()
                        || !key.elements.iter().all(|e| {
                            is_vector_path(e) && e.scripts.is_empty() && e.accessibility.is_none()
                        })
                    {
                        continue;
                    }
                    let mut normalized = key.elements.clone();
                    for el in &mut normalized {
                        el.id.clear();
                    }
                    let Ok(sig) = serde_json::to_string(&normalized) else {
                        continue;
                    };
                    let entry = kf_buckets.entry(sig.clone()).or_insert_with(|| {
                        kf_order.push(sig);
                        Vec::new()
                    });
                    entry.push((si, li, ki));
                }
            }
        }

        let mut target_remap: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let mut new_symbols = Vec::new();
        for sig in kf_order {
            if project.symbols.len() + new_symbols.len() >= MAX_SYMBOL_BUDGET {
                break;
            }
            let Some(locs) = kf_buckets.get(&sig) else {
                continue;
            };
            if locs.len() < 2 {
                continue;
            }
            let (si0, li0, ki0) = locs[0];
            let k_len = project.symbols[si0].timeline.layers[li0].keyframes[ki0]
                .elements
                .len();
            let saves_elements = locs.len() * k_len > k_len + locs.len() + 2;
            let saves_json =
                sig.len() > 260 && (locs.len() - 1) * (sig.len() - 220) > sig.len() + 600;
            if total_authored > MAX_AUTHORED_ELEMENTS {
                if !saves_elements {
                    continue;
                }
            } else if !saves_elements || !saves_json {
                continue;
            }
            let sym_id = alloc_sym_id("fla-shared-artwork");
            let mut members = project.symbols[si0].timeline.layers[li0].keyframes[ki0]
                .elements
                .clone();
            for (mi, member) in members.iter_mut().enumerate() {
                member.id = format!("{sym_id}-el-{mi}");
            }
            new_symbols.push(group_symbol(
                &sym_id,
                format!("Shared artwork {}", new_symbols.len() + 1),
                members,
            ));
            for &(si, li, ki) in locs {
                let kf = &mut project.symbols[si].timeline.layers[li].keyframes[ki];
                let outer_id = kf.elements[0].id.clone();
                for dropped in &kf.elements[1..] {
                    target_remap.insert(dropped.id.clone(), outer_id.clone());
                }
                kf.elements = vec![Element {
                    id: outer_id,
                    name: String::new(),
                    transform: Transform::default(),
                    kind: ElementKind::Instance {
                        symbol_id: sym_id.clone(),
                        playback: InstancePlayback::SingleFrame(0),
                    },
                    ..Element::default()
                }];
            }
            changed = true;
        }
        if !new_symbols.is_empty() {
            project.symbols.extend(new_symbols);
        }
        if !target_remap.is_empty() {
            for entry in provenance.iter_mut() {
                if let Some(cur) = entry.get("target").and_then(|v| v.as_str())
                    && let Some(mapped) = target_remap.get(cur)
                {
                    entry["target"] = serde_json::Value::String(mapped.clone());
                }
            }
        }
    }

    if !include_single_shapes {
        return changed;
    }

    let post_part_a_snapshot = if saved_state.is_some() {
        if max_expanded_display_nodes(&project.symbols) > MAX_EXPANDED_NODES {
            if let Some((orig_syms, orig_prov)) = saved_state {
                project.symbols = orig_syms;
                provenance.clone_from_slice(&orig_prov);
            }
            return false;
        }
        Some((project.symbols.clone(), changed))
    } else {
        None
    };

    // Part B: Deduplicate repeated complex PaintedPath / Path shapes (>= 6 commands)
    // across static normal-layer keyframes into shared 1-frame Graphic symbols.
    let allowed_leaf = symbols_allowing_leaf_instance(&project.symbols);
    let mut path_buckets: std::collections::HashMap<String, Vec<(usize, usize, usize, usize)>> =
        std::collections::HashMap::new();
    let mut path_order: Vec<String> = Vec::new();
    for (si, sym) in project.symbols.iter().enumerate() {
        if !allowed_leaf.contains(&sym.id)
            || sym.id == PLACEHOLDER
            || sym.id.starts_with("fla-missing-bitmap-")
            || sym.id.starts_with("fla-shared-")
        {
            continue;
        }
        for (li, layer) in sym.timeline.layers.iter().enumerate() {
            if layer.properties.role != LayerRole::Normal {
                continue;
            }
            for (ki, key) in layer.keyframes.iter().enumerate() {
                let prev_tweened = ki > 0 && layer.keyframes[ki - 1].tween.is_some();
                if key.tween.is_some()
                    || prev_tweened
                    || key.keyed_properties.is_some()
                    || !key.shape_hints.is_empty()
                {
                    continue;
                }
                let side_ids: std::collections::HashSet<&str> = key
                    .bitmap_fills
                    .iter()
                    .map(|bf| bf.element_id.as_str())
                    .chain(key.appearances.iter().map(|a| a.element_id.as_str()))
                    .chain(key.vector_effects.iter().map(|fx| fx.element_id.as_str()))
                    .chain(key.asset_warps.iter().map(|w| w.element_id.as_str()))
                    .chain(
                        key.element_parents
                            .iter()
                            .flat_map(|ep| [ep.child_id.as_str(), ep.parent_id.as_str()]),
                    )
                    .collect();
                for (ei, elem) in key.elements.iter().enumerate() {
                    let cmd_len = match &elem.kind {
                        ElementKind::PaintedPath { commands, .. }
                        | ElementKind::Path { commands, .. } => commands.len(),
                        _ => continue,
                    };
                    if cmd_len < 6
                        || !elem.scripts.is_empty()
                        || side_ids.contains(elem.id.as_str())
                    {
                        continue;
                    }
                    let Ok(sig) = serde_json::to_string(&elem.kind) else {
                        continue;
                    };
                    let entry = path_buckets.entry(sig.clone()).or_insert_with(|| {
                        path_order.push(sig);
                        Vec::new()
                    });
                    entry.push((si, li, ki, ei));
                }
            }
        }
    }

    let mut new_shape_symbols = Vec::new();
    for sig in path_order {
        if project.symbols.len() + new_shape_symbols.len() >= MAX_SYMBOL_BUDGET {
            break;
        }
        let Some(locs) = path_buckets.get(&sig) else {
            continue;
        };
        if locs.len() < 2 || (locs.len() - 1) * sig.len() <= sig.len() + 800 {
            continue;
        }
        let (si0, li0, ki0, ei0) = locs[0];
        let template_kind = project.symbols[si0].timeline.layers[li0].keyframes[ki0].elements[ei0]
            .kind
            .clone();
        let sym_id = alloc_sym_id("fla-shared-shape");
        let inner_el = Element {
            id: format!("{sym_id}-el"),
            name: String::new(),
            transform: Transform::default(),
            kind: template_kind,
            ..Element::default()
        };
        new_shape_symbols.push(group_symbol(
            &sym_id,
            format!("Shared shape {}", new_shape_symbols.len() + 1),
            vec![inner_el],
        ));
        for &(si, li, ki, ei) in locs {
            project.symbols[si].timeline.layers[li].keyframes[ki].elements[ei].kind =
                ElementKind::Instance {
                    symbol_id: sym_id.clone(),
                    playback: InstancePlayback::SingleFrame(0),
                };
        }
        changed = true;
    }
    if !new_shape_symbols.is_empty() {
        project.symbols.extend(new_shape_symbols);
    }
    if let Some((part_a_syms, part_a_changed)) = post_part_a_snapshot
        && max_expanded_display_nodes(&project.symbols) > MAX_EXPANDED_NODES
    {
        project.symbols = part_a_syms;
        return part_a_changed;
    }

    changed
}

pub(super) fn max_expanded_display_nodes(symbols: &[Symbol]) -> usize {
    const CAP: usize = 100_001;
    let by_id: std::collections::HashMap<&str, &Symbol> =
        symbols.iter().map(|s| (s.id.as_str(), s)).collect();
    let mut memo = std::collections::HashMap::<String, usize>::new();
    let mut active = std::collections::HashSet::<String>::new();
    fn eval(
        id: &str,
        by_id: &std::collections::HashMap<&str, &Symbol>,
        active: &mut std::collections::HashSet<String>,
        memo: &mut std::collections::HashMap<String, usize>,
    ) -> usize {
        if let Some(&v) = memo.get(id) {
            return v;
        }
        let Some(&sym) = by_id.get(id) else {
            return 1;
        };
        if !active.insert(id.to_owned()) {
            return 1;
        }
        let mut count = 1usize;
        for layer in &sym.timeline.layers {
            let mut max_key = 0usize;
            for key in &layer.keyframes {
                let mut key_count = 0usize;
                for elem in &key.elements {
                    let nodes = match &elem.kind {
                        ElementKind::Instance { symbol_id, .. } => {
                            eval(symbol_id, by_id, active, memo)
                        }
                        _ => 1,
                    };
                    key_count = key_count.saturating_add(nodes).min(CAP);
                }
                max_key = max_key.max(key_count);
            }
            count = count.saturating_add(max_key).min(CAP);
        }
        active.remove(id);
        memo.insert(id.to_owned(), count);
        count
    }
    symbols
        .iter()
        .map(|s| eval(&s.id, &by_id, &mut active, &mut memo))
        .max()
        .unwrap_or(0)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn fit_project_json_budget(
    project: &mut Project,
    json: &mut String,
    embedded: &mut bool,
    prov_detail: &mut ProvenanceDetail,
    prov_warned: &mut bool,
    original_len: usize,
    placements: &mut [serde_json::Value],
    report: &mut ImportReport,
    make_provenance: &dyn Fn(bool, ProvenanceDetail, &[serde_json::Value]) -> serde_json::Value,
) -> Result<(), String> {
    // 0a. Run lossless group/media and repeated-shape Graphic deduplication BEFORE
    //     dropping the embedded source FLA archive or shedding any media.
    let mut deduped = deduplicate_project_for_budget(project, placements);
    if deduplicate_repeated_vector_paths_for_budget(project, placements, true) {
        deduped = true;
    }
    if deduped {
        project.format_version = 5;
        project.upgrade_format_for_features();
        update_provenance_resource(
            project,
            prov_detail,
            prov_warned,
            *embedded,
            placements,
            report,
            make_provenance,
        )?;
        *json = serde_json::to_string(project).map_err(|e| e.to_string())?;
    }
    if json.len() <= MAX_PROJECT_JSON_BYTES {
        return Ok(());
    }

    if *embedded {
        project.resources.retain(|r| {
            r.media_type != "application/x-neoflash-fla-source"
                && !(r.media_type == "application/gzip"
                    && (r.path == "legacy/source.fla.gz"
                        || r.path.starts_with("legacy/source.fla.gz.part")))
        });
        *embedded = false;
        report.warning(
            "fla_source_not_embedded",
            "Original FLA",
            format!(
                "The original FLA ({} MiB) does not fit the project's limits next to the converted document (64 MiB of resources, 128 MiB in total), so it is not embedded. Keep the original file; provenance records its size, SHA-256 digest, and FNV-1a digest.",
                original_len / (1024 * 1024)
            ),
        );
        update_provenance_resource(
            project,
            prov_detail,
            prov_warned,
            false,
            placements,
            report,
            make_provenance,
        )?;
        *json = serde_json::to_string(project).map_err(|e| e.to_string())?;
    }

    // 0. Shed imported video resources (legacy-video-*, largest first) before audio/bitmaps/shapes.
    while json.len() > MAX_PROJECT_JSON_BYTES {
        let excess = json.len() - MAX_PROJECT_JSON_BYTES;
        if !shed_video_resources(project, report, excess) {
            break;
        }
        project.format_version = 5;
        project.upgrade_format_for_features();
        *json = serde_json::to_string(project).map_err(|e| e.to_string())?;
    }

    // 1. Shed imported audio resources (legacy-sound-*, largest first).
    while json.len() > MAX_PROJECT_JSON_BYTES {
        let excess = json.len() - MAX_PROJECT_JSON_BYTES;
        if !shed_audio_resources(project, report, excess) {
            break;
        }
        project.format_version = 5;
        project.upgrade_format_for_features();
        *json = serde_json::to_string(project).map_err(|e| e.to_string())?;
    }

    // 1b. Downscale/compress imported bitmap resources (legacy-bitmap-*, largest first)
    //     while preserving Stage placement size before shedding any bitmap to a placeholder.
    while json.len() > MAX_PROJECT_JSON_BYTES {
        let excess = json.len() - MAX_PROJECT_JSON_BYTES;
        if !downscale_bitmap_resources_for_json(project, report, excess) {
            break;
        }
        project.format_version = 5;
        project.upgrade_format_for_features();
        *json = serde_json::to_string(project).map_err(|e| e.to_string())?;
    }

    // 2. Shed imported bitmap resources (legacy-bitmap-*, largest first).
    while json.len() > MAX_PROJECT_JSON_BYTES {
        let excess = json.len() - MAX_PROJECT_JSON_BYTES;
        if !shed_bitmap_resources(project, report, excess) {
            break;
        }
        project.format_version = 5;
        project.upgrade_format_for_features();
        *json = serde_json::to_string(project).map_err(|e| e.to_string())?;
    }

    // Compact bulky provenance placement mappings before degrading vector artwork.
    if json.len() > MAX_PROJECT_JSON_BYTES && *prov_detail == ProvenanceDetail::Full {
        *prov_detail = ProvenanceDetail::NoPlacements;
        update_provenance_resource(
            project,
            prov_detail,
            prov_warned,
            *embedded,
            placements,
            report,
            make_provenance,
        )?;
        *json = serde_json::to_string(project).map_err(|e| e.to_string())?;
    }

    // 3. Degrade excess vector shape elements in later symbols/frames to placeholders.
    while json.len() > MAX_PROJECT_JSON_BYTES {
        let target = (json.len() - MAX_PROJECT_JSON_BYTES).saturating_add(65_536);
        if !degrade_excess_vector_shapes(project, report, target) {
            break;
        }
        project.format_version = 5;
        project.upgrade_format_for_features();
        *json = serde_json::to_string(project).map_err(|e| e.to_string())?;
    }

    if json.len() > MAX_PROJECT_JSON_BYTES && *prov_detail != ProvenanceDetail::Minimal {
        *prov_detail = ProvenanceDetail::Minimal;
        update_provenance_resource(
            project,
            prov_detail,
            prov_warned,
            *embedded,
            placements,
            report,
            make_provenance,
        )?;
        *json = serde_json::to_string(project).map_err(|e| e.to_string())?;
    }

    Ok(())
}
