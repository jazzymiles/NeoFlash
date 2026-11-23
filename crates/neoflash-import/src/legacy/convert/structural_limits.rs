use super::*;

pub(super) fn empty_timeline(layer_id: &str) -> Timeline {
    Timeline {
        guides: vec![],
        frame_count: 1,
        layers: vec![Layer {
            id: layer_id.into(),
            name: "Layer 1".into(),
            visible: true,
            locked: false,
            keyframes: vec![Keyframe {
                frame: 0,
                elements: vec![],
                tween: None,
                label: String::new(),
                ..Default::default()
            }],
            properties: LayerProperties::default(),
        }],
        scripts: vec![],
    }
}

pub(super) fn is_valid_qualified_class_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 1024
        && name.split('.').all(|part| {
            !part.is_empty()
                && part.chars().enumerate().all(|(i, ch)| {
                    ch == '_' || ch == '$' || ch.is_alphabetic() || (i > 0 && ch.is_numeric())
                })
        })
}

pub(super) fn leaf_placeholder_element(id: String, transform: Transform) -> Element {
    Element {
        id,
        name: String::new(),
        transform,
        kind: ElementKind::Path {
            commands: vec![],
            fill_rule: FillRule::EvenOdd,
            fill: [0; 4],
            stroke: [0; 4],
            stroke_width: 0.0,
        },
        scripts: vec![],
        accessibility: None,
        ..Element::default()
    }
}

pub(super) fn prune_keyframe_side_tables(key: &mut Keyframe) {
    let kept: std::collections::HashSet<&str> =
        key.elements.iter().map(|e| e.id.as_str()).collect();
    key.bitmap_fills
        .retain(|bf| kept.contains(bf.element_id.as_str()));
    key.appearances
        .retain(|app| kept.contains(app.element_id.as_str()));
    key.vector_effects
        .retain(|fx| kept.contains(fx.element_id.as_str()));
    key.asset_warps
        .retain(|w| kept.contains(w.element_id.as_str()));
    key.element_parents
        .retain(|ep| kept.contains(ep.child_id.as_str()) && kept.contains(ep.parent_id.as_str()));
}

pub(super) fn enforce_project_structural_limits(
    project: &mut Project,
    provenance: &mut [serde_json::Value],
    report: &mut ImportReport,
) {
    const MAX_SYMBOLS: usize = 10_000;
    const MAX_NESTING_DEPTH: usize = 64;
    const MAX_PLACED_BITMAPS: usize = 256;
    const MAX_AUTHORED_ELEMENTS: usize = 100_000;
    const MAX_EXPANDED_NODES: usize = 100_000;

    // 1. Break symbol cycles and nesting deeper than 64 levels first so later
    //    bottom-up traversals operate on a bounded DAG.
    loop {
        let index_by_id: std::collections::HashMap<String, usize> = project
            .symbols
            .iter()
            .enumerate()
            .map(|(idx, s)| (s.id.clone(), idx))
            .collect();
        let mut active = std::collections::HashSet::<String>::new();
        let mut good_max_depth = std::collections::HashMap::<String, usize>::new();
        let mut bad_edge: Option<(usize, usize, usize, usize, String)> = None;

        fn find_bad_instance(
            sym_idx: usize,
            depth: usize,
            symbols: &[Symbol],
            index_by_id: &std::collections::HashMap<String, usize>,
            active: &mut std::collections::HashSet<String>,
            good_max_depth: &mut std::collections::HashMap<String, usize>,
        ) -> Result<usize, (usize, usize, usize, usize, String)> {
            let sym = &symbols[sym_idx];
            if let Some(&sub_height) = good_max_depth.get(&sym.id)
                && depth + sub_height <= MAX_NESTING_DEPTH + 1
            {
                return Ok(sub_height);
            }
            active.insert(sym.id.clone());
            let mut max_sub = 1usize;
            for (li, layer) in sym.timeline.layers.iter().enumerate() {
                for (ki, key) in layer.keyframes.iter().enumerate() {
                    for (ei, elem) in key.elements.iter().enumerate() {
                        if let ElementKind::Instance { symbol_id, .. } = &elem.kind {
                            if active.contains(symbol_id) {
                                active.remove(&sym.id);
                                return Err((
                                    sym_idx,
                                    li,
                                    ki,
                                    ei,
                                    "cyclic symbol reference".into(),
                                ));
                            }
                            if depth >= MAX_NESTING_DEPTH {
                                active.remove(&sym.id);
                                return Err((
                                    sym_idx,
                                    li,
                                    ki,
                                    ei,
                                    format!("symbol nesting exceeds {MAX_NESTING_DEPTH} levels"),
                                ));
                            }
                            if let Some(&child_idx) = index_by_id.get(symbol_id) {
                                let child_h = find_bad_instance(
                                    child_idx,
                                    depth + 1,
                                    symbols,
                                    index_by_id,
                                    active,
                                    good_max_depth,
                                )?;
                                max_sub = max_sub.max(1 + child_h);
                            }
                        }
                    }
                }
            }
            active.remove(&sym.id);
            good_max_depth.insert(sym.id.clone(), max_sub);
            Ok(max_sub)
        }

        for idx in 0..project.symbols.len() {
            if let Err(found) = find_bad_instance(
                idx,
                1,
                &project.symbols,
                &index_by_id,
                &mut active,
                &mut good_max_depth,
            ) {
                bad_edge = Some(found);
                break;
            }
        }
        let Some((si, li, ki, ei, reason)) = bad_edge else {
            break;
        };
        let sym_name = project.symbols[si].name.clone();
        let layer_name = project.symbols[si].timeline.layers[li].name.clone();
        let frame_no = project.symbols[si].timeline.layers[li].keyframes[ki].frame + 1;
        let elem = &mut project.symbols[si].timeline.layers[li].keyframes[ki].elements[ei];
        *elem = leaf_placeholder_element(elem.id.clone(), elem.transform);
        report.warning(
            "fla_symbol_nesting_limit",
            format!("{sym_name} layer {layer_name} frame {frame_no}"),
            format!(
                "Symbol placement replaced by a placeholder ({reason}); original symbol remains in the library."
            ),
        );
    }

    // 2. Enforce runtime placed-bitmap budgets (256 distinct bitmap buffers and
    //    MAX_BITMAP_BYTES = 128 MiB of decoded RGBA pixels). First proportionally
    //    downscale the largest placed bitmaps so visible Stage bitmaps stay within
    //    the 128 MiB pixel budget before falling back to placeholders.
    for _ in 0..512 {
        let mut seen = std::collections::HashSet::<String>::new();
        let mut total_rgba = 0usize;
        let mut largest: Option<(usize, String)> = None;
        for sym in &project.symbols {
            for layer in &sym.timeline.layers {
                for key in &layer.keyframes {
                    for elem in &key.elements {
                        if let ElementKind::Bitmap {
                            resource_id,
                            width,
                            height,
                            ..
                        } = &elem.kind
                        {
                            if seen.len() >= MAX_PLACED_BITMAPS && !seen.contains(resource_id) {
                                continue;
                            }
                            if seen.insert(resource_id.clone()) {
                                let rgba = (*width as usize)
                                    .saturating_mul(*height as usize)
                                    .saturating_mul(4);
                                total_rgba = total_rgba.saturating_add(rgba);
                                if (*width).max(*height) > 64
                                    && resource_id.starts_with("legacy-")
                                    && largest.as_ref().is_none_or(|(best, _)| rgba > *best)
                                {
                                    largest = Some((rgba, resource_id.clone()));
                                }
                            }
                        }
                    }
                }
            }
        }
        if total_rgba <= MAX_BITMAP_BYTES {
            break;
        }
        let Some((_, res_id)) = largest else {
            break;
        };
        if downscale_one_project_bitmap_resource(
            project,
            &res_id,
            64,
            false,
            report,
            "128 MiB runtime pixel budget",
        )
        .is_none()
        {
            break;
        }
    }

    let mut kept_placed_bitmaps = std::collections::HashSet::<String>::new();
    let mut placed_rgba_bytes = 0usize;
    let mut degraded_bitmap_placements = Vec::<(usize, usize, usize, usize, String)>::new();
    for (si, sym) in project.symbols.iter().enumerate() {
        for (li, layer) in sym.timeline.layers.iter().enumerate() {
            for (ki, key) in layer.keyframes.iter().enumerate() {
                for (ei, elem) in key.elements.iter().enumerate() {
                    if let ElementKind::Bitmap {
                        resource_id,
                        width,
                        height,
                        ..
                    } = &elem.kind
                    {
                        if kept_placed_bitmaps.contains(resource_id) {
                            continue;
                        }
                        let rgba = (*width as usize)
                            .saturating_mul(*height as usize)
                            .saturating_mul(4);
                        if kept_placed_bitmaps.len() < MAX_PLACED_BITMAPS
                            && placed_rgba_bytes
                                .checked_add(rgba)
                                .is_some_and(|n| n <= MAX_BITMAP_BYTES)
                        {
                            kept_placed_bitmaps.insert(resource_id.clone());
                            placed_rgba_bytes += rgba;
                        } else {
                            degraded_bitmap_placements.push((si, li, ki, ei, resource_id.clone()));
                        }
                    }
                }
            }
        }
    }
    if !degraded_bitmap_placements.is_empty() {
        if !project.symbols.iter().any(|s| s.id == PLACEHOLDER)
            && project.symbols.len() < MAX_SYMBOLS
        {
            project.symbols.push(group_symbol(
                PLACEHOLDER,
                "Unsupported source object (see import report)".into(),
                vec![],
            ));
        }
        let has_placeholder = project.symbols.iter().any(|s| s.id == PLACEHOLDER);
        let allowed_leaf = symbols_allowing_leaf_instance(&project.symbols);
        for (si, li, ki, ei, res_id) in degraded_bitmap_placements {
            let sym_id = project.symbols[si].id.clone();
            let sym_name = project.symbols[si].name.clone();
            let layer_name = project.symbols[si].timeline.layers[li].name.clone();
            let frame_no = project.symbols[si].timeline.layers[li].keyframes[ki].frame + 1;
            let use_inst = has_placeholder && allowed_leaf.contains(&sym_id);
            let elem = &mut project.symbols[si].timeline.layers[li].keyframes[ki].elements[ei];
            elem.name = format!("Budget-limited bitmap ({res_id})");
            elem.kind = if use_inst {
                ElementKind::Instance {
                    symbol_id: PLACEHOLDER.into(),
                    playback: InstancePlayback::SingleFrame(0),
                }
            } else {
                ElementKind::Path {
                    commands: vec![],
                    fill_rule: FillRule::EvenOdd,
                    fill: [0; 4],
                    stroke: [0; 4],
                    stroke_width: 0.0,
                }
            };
            report.warning(
                "fla_bitmap_skipped",
                format!("{sym_name} layer {layer_name} frame {frame_no}"),
                "Placed bitmap exceeds the 256-bitmap or 128 MiB runtime pixel budget; a placeholder preserves its stage transform.",
            );
        }
    }

    // 3. Deduplicate groups/media first whenever symbol, element or expanded-node
    //    counts approach document limits.
    let authored_count = |proj: &Project| -> usize {
        proj.symbols
            .iter()
            .flat_map(|s| &s.timeline.layers)
            .flat_map(|l| &l.keyframes)
            .map(|k| k.elements.len())
            .sum()
    };
    let symbol_expanded_bound =
        |sym: &Symbol, child_bounds: &std::collections::HashMap<String, usize>| -> usize {
            let cap = MAX_EXPANDED_NODES + 1;
            let mut count = 1usize;
            for layer in &sym.timeline.layers {
                let mut max_key = 0usize;
                for key in &layer.keyframes {
                    let mut key_count = 0usize;
                    for elem in &key.elements {
                        let nodes = match &elem.kind {
                            ElementKind::Instance { symbol_id, .. } => {
                                child_bounds.get(symbol_id).copied().unwrap_or(1)
                            }
                            _ => 1,
                        };
                        key_count = key_count.saturating_add(nodes).min(cap);
                    }
                    max_key = max_key.max(key_count);
                }
                count = count.saturating_add(max_key).min(cap);
            }
            count
        };
    let compute_expanded_bounds = |symbols: &[Symbol]| -> std::collections::HashMap<String, usize> {
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
            let cap = MAX_EXPANDED_NODES + 1;
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
                        key_count = key_count.saturating_add(nodes).min(cap);
                    }
                    max_key = max_key.max(key_count);
                }
                count = count.saturating_add(max_key).min(cap);
            }
            active.remove(id);
            memo.insert(id.to_owned(), count);
            count
        }
        for s in symbols {
            eval(&s.id, &by_id, &mut active, &mut memo);
        }
        memo
    };

    if project.symbols.len() > MAX_SYMBOLS
        || authored_count(project) > MAX_AUTHORED_ELEMENTS
        || compute_expanded_bounds(&project.symbols)
            .values()
            .any(|&n| n > MAX_EXPANDED_NODES)
    {
        deduplicate_project_for_budget(project, provenance);
        if authored_count(project) > MAX_AUTHORED_ELEMENTS {
            deduplicate_repeated_vector_paths_for_budget(project, provenance, false);
        }
    }

    // 3a. Cap symbol count at 10,000.
    if project.symbols.len() > MAX_SYMBOLS {
        let protected: std::collections::HashSet<String> = std::iter::once(project.root.clone())
            .chain(project.scenes.iter().cloned())
            .chain(std::iter::once(PLACEHOLDER.to_owned()))
            .collect();
        let protected_present = project
            .symbols
            .iter()
            .filter(|sym| protected.contains(&sym.id))
            .count();
        let reserve_placeholder = usize::from(
            !project
                .symbols
                .iter()
                .any(|sym| sym.id.as_str() == PLACEHOLDER),
        );
        let max_unprotected = MAX_SYMBOLS.saturating_sub(protected_present + reserve_placeholder);
        let mut unprotected_kept = 0usize;
        let mut kept = Vec::with_capacity(MAX_SYMBOLS);
        let mut dropped = std::collections::HashSet::<String>::new();
        for sym in project.symbols.drain(..) {
            if protected.contains(&sym.id) {
                kept.push(sym);
            } else if unprotected_kept < max_unprotected {
                unprotected_kept += 1;
                kept.push(sym);
            } else {
                report.warning(
                    "fla_symbol_limit",
                    &sym.name,
                    format!(
                        "Symbol omitted because the document exceeds the {MAX_SYMBOLS}-symbol limit; placements become placeholders."
                    ),
                );
                dropped.insert(sym.id);
            }
        }
        project.symbols = kept;
        for sym in &mut project.symbols {
            for layer in &mut sym.timeline.layers {
                for key in &mut layer.keyframes {
                    for elem in &mut key.elements {
                        if let ElementKind::Instance { symbol_id, .. } = &elem.kind
                            && dropped.contains(symbol_id)
                        {
                            *elem = leaf_placeholder_element(elem.id.clone(), elem.transform);
                        }
                    }
                }
            }
        }
    }

    // 3b. Cap total authored elements at 100,000.
    let mut total_elements = authored_count(project);
    if total_elements > MAX_AUTHORED_ELEMENTS {
        for sym in project.symbols.iter_mut().rev() {
            if total_elements <= MAX_AUTHORED_ELEMENTS {
                break;
            }
            for layer in sym.timeline.layers.iter_mut().rev() {
                if total_elements <= MAX_AUTHORED_ELEMENTS {
                    break;
                }
                for key in layer.keyframes.iter_mut().rev() {
                    if total_elements <= MAX_AUTHORED_ELEMENTS {
                        break;
                    }
                    if key.elements.len() <= 1 {
                        continue;
                    }
                    let removed = key.elements.len() - 1;
                    let first = &key.elements[0];
                    key.elements =
                        vec![leaf_placeholder_element(first.id.clone(), first.transform)];
                    prune_keyframe_side_tables(key);
                    total_elements = total_elements.saturating_sub(removed);
                    report.warning(
                        "fla_element_limit",
                        format!("{} layer {} frame {}", sym.name, layer.name, key.frame + 1),
                        format!(
                            "Keyframe elements collapsed into a placeholder to fit the {MAX_AUTHORED_ELEMENTS}-element project limit."
                        ),
                    );
                }
            }
        }
        if total_elements > MAX_AUTHORED_ELEMENTS {
            for sym in project.symbols.iter_mut().rev() {
                for layer in sym.timeline.layers.iter_mut().rev() {
                    for key in layer.keyframes.iter_mut().rev() {
                        if total_elements <= MAX_AUTHORED_ELEMENTS {
                            break;
                        }
                        if !key.elements.is_empty() {
                            total_elements = total_elements.saturating_sub(key.elements.len());
                            key.elements.clear();
                            prune_keyframe_side_tables(key);
                            report.warning(
                                "fla_element_limit",
                                format!("{} layer {} frame {}", sym.name, layer.name, key.frame + 1),
                                format!(
                                    "Trailing keyframe cleared to fit the {MAX_AUTHORED_ELEMENTS}-element project limit."
                                ),
                            );
                        }
                    }
                }
            }
        }
    }

    // 3c. Cap expanded scene/symbol node bound at 100,000 (bottom-up over the DAG).
    loop {
        let bounds = compute_expanded_bounds(&project.symbols);
        let Some(target_idx) = (0..project.symbols.len()).find(|&idx| {
            let sym = &project.symbols[idx];
            bounds.get(&sym.id).copied().unwrap_or(0) > MAX_EXPANDED_NODES
                && sym
                    .timeline
                    .layers
                    .iter()
                    .flat_map(|l| &l.keyframes)
                    .flat_map(|k| &k.elements)
                    .all(|e| match &e.kind {
                        ElementKind::Instance { symbol_id, .. } => {
                            bounds.get(symbol_id).copied().unwrap_or(0) <= MAX_EXPANDED_NODES
                        }
                        _ => true,
                    })
        }) else {
            break;
        };
        let sym_name = project.symbols[target_idx].name.clone();
        // First degrade heavy child instances in keyframes whose per-keyframe bound contributes
        // to the over-budget layer maximum, recomputing the exact symbol bound after each step.
        while symbol_expanded_bound(&project.symbols[target_idx], &bounds) > MAX_EXPANDED_NODES {
            // Find the layer with the largest `max_key`, and within it the last keyframe
            // holding a child instance with `child_cost > 1`.
            let mut best: Option<(usize, usize, usize, usize)> = None; // (layer_max, li, ki, ei)
            for (li, layer) in project.symbols[target_idx]
                .timeline
                .layers
                .iter()
                .enumerate()
            {
                for (ki, key) in layer.keyframes.iter().enumerate() {
                    let mut key_cost = 0usize;
                    let mut last_heavy_elem = None;
                    for (ei, elem) in key.elements.iter().enumerate() {
                        let nodes = match &elem.kind {
                            ElementKind::Instance { symbol_id, .. } => {
                                let c = bounds.get(symbol_id).copied().unwrap_or(1);
                                if c > 1 {
                                    last_heavy_elem = Some(ei);
                                }
                                c
                            }
                            _ => 1,
                        };
                        key_cost = key_cost.saturating_add(nodes);
                    }
                    if let Some(ei) = last_heavy_elem
                        && best.is_none_or(|(b_cost, ..)| key_cost >= b_cost)
                    {
                        best = Some((key_cost, li, ki, ei));
                    }
                }
            }
            let Some((_, li, ki, ei)) = best else {
                break;
            };
            let layer_name = project.symbols[target_idx].timeline.layers[li].name.clone();
            let frame_no = project.symbols[target_idx].timeline.layers[li].keyframes[ki].frame + 1;
            let elem =
                &mut project.symbols[target_idx].timeline.layers[li].keyframes[ki].elements[ei];
            *elem = leaf_placeholder_element(elem.id.clone(), elem.transform);
            report.warning(
                "fla_expanded_node_limit",
                format!("{sym_name} layer {layer_name} frame {frame_no}"),
                format!(
                    "Symbol instance replaced by a placeholder so expanded scene/symbol nodes stay within {MAX_EXPANDED_NODES}."
                ),
            );
        }
        // If leaf elements alone still exceed 100,000, collapse the heaviest keyframes.
        while symbol_expanded_bound(&project.symbols[target_idx], &bounds) > MAX_EXPANDED_NODES {
            let mut best_kf: Option<(usize, usize, usize)> = None; // (len, li, ki)
            for (li, layer) in project.symbols[target_idx]
                .timeline
                .layers
                .iter()
                .enumerate()
            {
                for (ki, key) in layer.keyframes.iter().enumerate() {
                    if !key.elements.is_empty()
                        && best_kf.is_none_or(|(b_len, ..)| key.elements.len() >= b_len)
                    {
                        best_kf = Some((key.elements.len(), li, ki));
                    }
                }
            }
            let Some((len, li, ki)) = best_kf else {
                break;
            };
            let layer_name = project.symbols[target_idx].timeline.layers[li].name.clone();
            let key = &mut project.symbols[target_idx].timeline.layers[li].keyframes[ki];
            if len > 1 {
                let first = &key.elements[0];
                key.elements = vec![leaf_placeholder_element(first.id.clone(), first.transform)];
                prune_keyframe_side_tables(key);
                report.warning(
                    "fla_expanded_node_limit",
                    format!("{sym_name} layer {layer_name} frame {}", key.frame + 1),
                    format!(
                        "Keyframe collapsed to a placeholder so expanded nodes stay within {MAX_EXPANDED_NODES}."
                    ),
                );
            } else {
                key.elements.clear();
                prune_keyframe_side_tables(key);
            }
        }
    }

    // 4. Sanitize Folder layers carrying stage artwork and broken/cyclic layer relations.
    for sym in &mut project.symbols {
        let sym_name = sym.name.clone();
        let layers = &mut sym.timeline.layers;
        for li in 0..layers.len() {
            if layers[li].properties.role == LayerRole::Folder
                && layers[li].keyframes.iter().any(|k| !k.elements.is_empty())
            {
                let folder_id = layers[li].id.clone();
                let has_children = layers.iter().enumerate().any(|(j, other)| {
                    j != li && other.properties.parent.as_deref() == Some(folder_id.as_str())
                });
                let loc = format!("{sym_name} layer {}", layers[li].name);
                if !has_children {
                    layers[li].properties.role = LayerRole::Normal;
                    layers[li].properties.collapsed = false;
                    report.warning(
                        "fla_layer_type",
                        loc,
                        "Folder layer contains stage elements and has no child layers; imported as a normal layer.",
                    );
                } else {
                    for key in &mut layers[li].keyframes {
                        key.elements.clear();
                        key.tween = None;
                        key.tween_kind = TweenKind::Classic;
                        key.keyed_properties = None;
                        key.orient_to_path = false;
                        key.sync_symbols = false;
                        key.snap_to_guide = false;
                        key.classic_rotate_dir = ClassicRotateDirection::Auto;
                        key.classic_rotate_turns = 0;
                        key.shape_hints.clear();
                        prune_keyframe_side_tables(key);
                    }
                    report.warning(
                        "fla_layer_type",
                        loc,
                        "Folder layer cannot contain stage artwork; elements on the folder row were omitted.",
                    );
                }
            }
        }
        for li in 0..layers.len() {
            if layers[li].properties.role != LayerRole::Mask {
                layers[li].properties.mask_mode = MaskMode::Geometry;
                layers[li].properties.mask_invert = false;
            }
            if let Some(mask_id) = layers[li].properties.mask.clone() {
                let valid = layers[li].properties.role == LayerRole::Normal
                    && mask_id != layers[li].id
                    && layers
                        .iter()
                        .any(|l| l.id == mask_id && l.properties.role == LayerRole::Mask);
                if !valid {
                    layers[li].properties.mask = None;
                    report.warning(
                        "fla_layer_relation",
                        format!("{sym_name} layer {}", layers[li].name),
                        "The layer's mask relation is invalid and was cleared.",
                    );
                }
            }
            if let Some(guide_id) = layers[li].properties.motion_guide.clone() {
                let valid = layers[li].properties.role == LayerRole::Normal
                    && guide_id != layers[li].id
                    && layers
                        .iter()
                        .any(|l| l.id == guide_id && l.properties.role == LayerRole::Guide);
                if !valid {
                    layers[li].properties.motion_guide = None;
                    report.warning(
                        "fla_layer_relation",
                        format!("{sym_name} layer {}", layers[li].name),
                        "The layer's motion-guide relation is invalid and was cleared.",
                    );
                }
            }
            if layers[li].properties.parent.is_some() {
                let mut seen = std::collections::HashSet::from([layers[li].id.clone()]);
                let mut next = layers[li].properties.parent.clone();
                let mut valid = true;
                while let Some(pid) = next {
                    if !seen.insert(pid.clone()) {
                        valid = false;
                        break;
                    }
                    let Some(parent_layer) = layers
                        .iter()
                        .find(|l| l.id == pid && l.properties.role == LayerRole::Folder)
                    else {
                        valid = false;
                        break;
                    };
                    next = parent_layer.properties.parent.clone();
                }
                if !valid {
                    layers[li].properties.parent = None;
                    report.warning(
                        "fla_layer_relation",
                        format!("{sym_name} layer {}", layers[li].name),
                        "The layer's folder ancestry is missing or cyclic and was moved to the timeline root.",
                    );
                }
            }
        }
    }

    // 5. Validate mask layers to a fixpoint mirroring `authored_layers::validate_layers`:
    //    upgrade bitmap-containing Geometry masks to Alpha, or demote non-maskable
    //    mask layers to Normal (resetting mask_mode, mask_invert, and sibling mask links).
    fn mask_source_ok<'a>(
        kind: &'a ElementKind,
        mode: MaskMode,
        by_id: &std::collections::HashMap<&'a str, &'a Symbol>,
        decodable_bitmaps: &std::collections::HashSet<String>,
        visiting: &mut std::collections::HashSet<&'a str>,
    ) -> bool {
        match kind {
            ElementKind::Instance { symbol_id, .. } => {
                let Some(&sym) = by_id.get(symbol_id.as_str()) else {
                    return false;
                };
                if !visiting.insert(sym.id.as_str()) {
                    return false;
                }
                let valid = sym
                    .timeline
                    .layers
                    .iter()
                    .filter(|l| l.properties.role != LayerRole::Guide)
                    .all(|layer| {
                        let child_mode = if layer.properties.role == LayerRole::Mask {
                            layer.properties.mask_mode
                        } else {
                            mode
                        };
                        layer.keyframes.iter().flat_map(|k| &k.elements).all(|e| {
                            mask_source_ok(&e.kind, child_mode, by_id, decodable_bitmaps, visiting)
                        })
                    });
                visiting.remove(sym.id.as_str());
                valid
            }
            ElementKind::Bitmap { resource_id, .. } if mode.uses_paint_coverage() => {
                decodable_bitmaps.contains(resource_id.as_str())
            }
            ElementKind::Rectangle { width, height, .. }
            | ElementKind::Ellipse { width, height, .. } => *width > 0.0 && *height > 0.0,
            ElementKind::Path { .. } | ElementKind::PaintedPath { .. } => true,
            _ => false,
        }
    }

    let decodable_bitmaps: std::collections::HashSet<String> = project
        .resources
        .iter()
        .filter(|r| r.decode_image().is_ok())
        .map(|r| r.id.clone())
        .collect();
    loop {
        let mut mask_fixes = Vec::<(usize, usize, bool)>::new();
        {
            let by_id: std::collections::HashMap<&str, &Symbol> =
                project.symbols.iter().map(|s| (s.id.as_str(), s)).collect();
            for (si, sym) in project.symbols.iter().enumerate() {
                for (li, layer) in sym.timeline.layers.iter().enumerate() {
                    if layer.properties.role != LayerRole::Mask {
                        continue;
                    }
                    let current_ok = layer
                        .keyframes
                        .iter()
                        .flat_map(|k| &k.elements)
                        .all(|elem| {
                            let mut visiting = std::collections::HashSet::new();
                            mask_source_ok(
                                &elem.kind,
                                layer.properties.mask_mode,
                                &by_id,
                                &decodable_bitmaps,
                                &mut visiting,
                            )
                        });
                    if current_ok {
                        continue;
                    }
                    if layer.properties.mask_mode.is_geometry() {
                        let alpha_ok =
                            layer
                                .keyframes
                                .iter()
                                .flat_map(|k| &k.elements)
                                .all(|elem| {
                                    let mut visiting = std::collections::HashSet::new();
                                    mask_source_ok(
                                        &elem.kind,
                                        MaskMode::Alpha,
                                        &by_id,
                                        &decodable_bitmaps,
                                        &mut visiting,
                                    )
                                });
                        mask_fixes.push((si, li, alpha_ok));
                    } else {
                        mask_fixes.push((si, li, false));
                    }
                }
            }
        }
        if mask_fixes.is_empty() {
            break;
        }
        for (si, li, promote_to_alpha) in mask_fixes {
            let sym = &mut project.symbols[si];
            let mask_id = sym.timeline.layers[li].id.clone();
            let loc = format!("{} layer {}", sym.name, sym.timeline.layers[li].name);
            if promote_to_alpha {
                sym.timeline.layers[li].properties.mask_mode = MaskMode::Alpha;
                report.warning(
                    "fla_mask_layer_approximated",
                    loc,
                    "Mask layer contains bitmap elements and was promoted to alpha masking.",
                );
            } else {
                sym.timeline.layers[li].properties.role = LayerRole::Normal;
                sym.timeline.layers[li].properties.mask_mode = MaskMode::Geometry;
                sym.timeline.layers[li].properties.mask_invert = false;
                for sibling in &mut sym.timeline.layers {
                    if sibling.properties.mask.as_deref() == Some(mask_id.as_str()) {
                        sibling.properties.mask = None;
                    }
                }
                report.warning(
                    "fla_mask_layer_approximated",
                    loc,
                    "Mask layer contains text, video or unresolved elements not supported as a mask and is imported as a normal layer.",
                );
            }
        }
    }
}
