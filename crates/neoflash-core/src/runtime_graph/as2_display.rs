//! AS2 sparse depths and target paths, separate from AS3 child-index operations.
//! Semantics adapted from Ruffle's MIT AVM1 movie_clip.rs at
//! 89a7049965f3090e706265670df309a5128233ac; see docs/AS2-DISPLAY-BATCH.md.
use super::*;
const MAX_DEPTH: i32 = 1_048_575;
const MIN_SWAP_DEPTH: i32 = -16_384;

fn depth_value(value: i32) -> Result<(), DisplayError> {
    if !(0..=MAX_DEPTH).contains(&value) {
        return Err(error(
            "AS2 runtime depth requires an integer from 0 through 1048575; authored negative depths are read-only",
        ));
    }
    Ok(())
}

fn swap_depth_value(value: i32) -> Result<(), DisplayError> {
    if !(MIN_SWAP_DEPTH..=MAX_DEPTH).contains(&value) {
        return Err(error(
            "AS2 swapDepths requires an integer depth from -16384 through 1048575",
        ));
    }
    Ok(())
}
impl RuntimeGraph {
    fn as2_depths(&self, parent: &ObjectRef) -> BTreeMap<ObjectRef, i32> {
        let children = self.suffix.get(parent).cloned().unwrap_or_default();
        let mut result: BTreeMap<_, _> = children
            .iter()
            .filter_map(|child| {
                self.as2_depths
                    .get(child)
                    .filter(|(owner, _)| owner == parent)
                    .map(|(_, depth)| (child.clone(), *depth))
            })
            .collect();
        for (child, (owner, depth)) in &self.as2_depths {
            if owner == parent
                && self.alive(child)
                && self.parent(child).as_ref() == Some(parent)
                && !result.contains_key(child)
            {
                result.insert(child.clone(), *depth);
            }
        }
        let mut used: BTreeSet<i32> = result.values().copied().collect();
        let mut next = 0;
        for child in children {
            if result.contains_key(&child) {
                continue;
            }
            while used.contains(&next) {
                next += 1;
            }
            result.insert(child, next);
            used.insert(next);
        }
        result
    }
    fn as2_freeze_depths(&mut self, parent: &ObjectRef) {
        for (child, depth) in self.as2_depths(parent) {
            self.as2_depths.insert(child, (parent.clone(), depth));
        }
    }
    fn as2_sort_depths(&mut self, parent: &ObjectRef) {
        let depths = self.as2_depths(parent);
        if let Some(children) = self.suffix.get_mut(parent) {
            children.sort_by_key(|child| depths.get(child).copied().unwrap_or(0));
        }
        let has_promoted_child = depths
            .keys()
            .any(|id| self.as2_promoted.contains_key(id) || depths[id] < 0);
        if has_promoted_child {
            let prefix_positions: HashMap<ObjectRef, i32> = self
                .prefix
                .get(parent)
                .into_iter()
                .flatten()
                .enumerate()
                .map(|(pos, child)| (child.clone(), pos as i32 - 16_384))
                .collect();
            let current = self.children(parent);
            let mut decorated: Vec<(i32, usize, ObjectRef)> = current
                .into_iter()
                .enumerate()
                .map(|(ord, child)| {
                    let d = depths
                        .get(&child)
                        .copied()
                        .or_else(|| prefix_positions.get(&child).copied())
                        .unwrap_or(ord as i32);
                    (d, ord, child)
                })
                .collect();
            decorated.sort_by_key(|(d, ord, _)| (*d, *ord));
            let sorted: Vec<ObjectRef> = decorated.into_iter().map(|(_, _, c)| c).collect();
            self.set_child_order(parent, sorted);
            return;
        }
        // Keep authored slots in their explicit AS3 order, while depth changes
        // reorder the runtime siblings within the runtime slots.
        if let Some(order) = self.child_order.get_mut(parent) {
            let mut runtime: Vec<_> = order
                .iter()
                .filter(|id| depths.contains_key(*id))
                .cloned()
                .collect();
            runtime.sort_by_key(|id| depths[id]);
            let mut runtime = runtime.into_iter();
            for child in order.iter_mut() {
                if depths.contains_key(child) {
                    *child = runtime.next().expect("runtime slot");
                }
            }
        }
    }
}
impl Player {
    /// Relative AS2 component depth policy over the existing runtime suffix.
    /// Numeric tiers belong to this bounded profile; raw swapDepths may override
    /// them.
    pub fn as2_depth_manager(
        &mut self,
        reference: &ObjectRef,
        request: serde_json::Value,
    ) -> Result<serde_json::Value, DisplayError> {
        use serde_json::json;
        const TOPMOST: i32 = 500_000;
        let op = request["op"].as_str().unwrap_or("");
        if op == "definitions" {
            return Ok(
                json!({"kCursor":101,"kTooltip":102,"kTop":201,"kBottom":202,"kTopmost":203,"kNotopmost":204}),
            );
        }
        if op == "remove" {
            let (mutation, retired) = self.as2_remove_subtree(reference)?;
            return Ok(json!({
                "events": mutation.events,
                "retired": retired,
            }));
        }
        if !self.graph.alive(reference) {
            return Err(error("DepthManager target is stale or foreign"));
        }
        let planning = op == "plan";
        let parent = if planning {
            reference.clone()
        } else {
            if !self.graph.dynamic.contains_key(reference)
                && !self.graph.as2_promoted.contains_key(reference)
            {
                return Err(error(
                    "DepthManager currently reorders runtime MovieClips; authored negative-depth placements are unsupported",
                ));
            }
            self.graph
                .parent(reference)
                .ok_or_else(|| error("DepthManager requires an attached MovieClip"))?
        };
        let depths = self.graph.as2_depths(&parent);
        let mut order: Vec<(Option<ObjectRef>, bool)> = depths
            .iter()
            .filter(|(child, _)| planning || *child != reference)
            .map(|(child, depth)| (Some(child.clone()), *depth >= TOPMOST))
            .collect();
        order.sort_by_key(|(child, _)| depths[child.as_ref().unwrap()]);
        let (position, topmost) = if matches!(op, "plan" | "set") {
            let flag = request["flag"]
                .as_i64()
                .ok_or_else(|| error("DepthManager requires a documented integer depth flag"))?;
            let boundary = order
                .iter()
                .position(|(_, top)| *top)
                .unwrap_or(order.len());
            match flag {
                201 | 204 => (boundary, false),
                202 => (0, false),
                203 => (order.len(), true),
                _ => {
                    return Err(error(
                        "Relative depth requires kTop, kBottom, kTopmost or kNotopmost; reserved cursor/tooltip creation is unavailable",
                    ));
                }
            }
        } else if matches!(op, "above" | "below") {
            let other: ObjectRef = serde_json::from_value(request["other"].clone())
                .map_err(|_| error("DepthManager requires another live sibling MovieClip"))?;
            if other == *reference {
                return Ok(json!({"depth":self.as2_get_depth(reference)?}));
            }
            if self.graph.parent(&other).as_ref() != Some(&parent) {
                return Err(error("DepthManager relative movement requires siblings"));
            }
            let position = order
                .iter()
                .position(|(child, _)| child.as_ref() == Some(&other))
                .ok_or_else(|| {
                    error(
                        "DepthManager cannot anchor relative movement to authored negative depths",
                    )
                })?;
            (position + usize::from(op == "above"), order[position].1)
        } else {
            return Err(error("Unknown DepthManager operation"));
        };
        order.insert(position, ((!planning).then(|| reference.clone()), topmost));
        let mut normal = 0;
        let mut top = TOPMOST;
        let mut result = 0;
        let mut placements = Vec::with_capacity(order.len());
        for (child, high) in order {
            let depth = if high {
                top += 20;
                top
            } else {
                normal += 20;
                normal
            };
            if depth > MAX_DEPTH || (!high && depth >= TOPMOST) {
                return Err(error("DepthManager sibling depth tier is full"));
            }
            if child.is_none() || child.as_ref() == Some(reference) {
                result = depth;
            }
            if let Some(child) = child {
                placements.push((child, depth));
            }
        }
        for (child, depth) in placements {
            self.graph.as2_depths.insert(child, (parent.clone(), depth));
        }
        self.graph.as2_sort_depths(&parent);
        Ok(json!({"depth":result}))
    }
    pub fn as2_get_depth(&self, reference: &ObjectRef) -> Result<i32, DisplayError> {
        if !self.graph.alive(reference) {
            return Err(error("AS2 depth target is stale or foreign"));
        }
        let Some(parent) = self.graph.parent(reference) else {
            if self.graph.paths.get(reference).is_some_and(Vec::is_empty) {
                return Ok(-16384);
            }
            return Err(error("AS2 depth requires an attached display object"));
        };
        if let Some(depth) = self.graph.as2_depths(&parent).get(reference) {
            return Ok(*depth);
        }
        self.graph
            .prefix
            .get(&parent)
            .and_then(|children| children.iter().position(|child| child == reference))
            .map(|position| position as i32 - 16384)
            .ok_or_else(|| error("Display object has no AS2 depth"))
    }
    pub fn as2_next_depth(&self, parent: &ObjectRef) -> Result<i32, DisplayError> {
        if !self.graph.alive(parent) {
            return Err(error("AS2 depth container is stale or foreign"));
        }
        Ok(self
            .graph
            .as2_depths(parent)
            .values()
            .max()
            .map_or(0, |depth| depth + 1))
    }
    pub fn as2_child_at_depth(
        &self,
        parent: &ObjectRef,
        depth: i32,
    ) -> Result<Option<ObjectRef>, DisplayError> {
        if !self.graph.alive(parent) {
            return Err(error("AS2 depth container is stale or foreign"));
        }
        if let Some(found) = self
            .graph
            .as2_depths(parent)
            .into_iter()
            .find_map(|(child, value)| (value == depth).then_some(child))
        {
            return Ok(Some(found));
        }
        if depth < 0 {
            return Ok(usize::try_from(depth + 16384)
                .ok()
                .and_then(|index| self.graph.prefix.get(parent)?.get(index).cloned())
                .filter(|child| {
                    !self.graph.as2_promoted.contains_key(child)
                        && self.graph.parent(child).as_ref() == Some(parent)
                }));
        }
        Ok(None)
    }
    /// Replacement is confined to runtime children. Ordinary AS3 mutations keep
    /// their index semantics and never call this function or sort the list.
    pub fn as2_place_depth(
        &mut self,
        project: &Project,
        parent: &ObjectRef,
        child: &ObjectRef,
        depth: i32,
    ) -> Result<MutationResult, DisplayError> {
        depth_value(depth)?;
        let occupied = self
            .as2_child_at_depth(parent, depth)?
            .filter(|found| found != child);
        if let Some(old) = &occupied {
            if self.graph.subtree(old).contains(child) || self.graph.subtree(child).contains(old) {
                return Err(error(
                    "AS2 depth replacement cannot replace an ancestor or descendant of its source",
                ));
            }
            if self.graph.subtree(old).len() + 1 > MAX_DISPLAY_EVENTS {
                return Err(error(
                    "AS2 depth replacement exceeds its lifecycle event budget",
                ));
            }
            self.graph
                .validate_loader_mutation(&DisplayMutation::Remove {
                    parent: parent.clone(),
                    child: old.clone(),
                })?;
        }
        self.graph.as2_freeze_depths(parent);
        let mut result = self.display_mutate(
            project,
            DisplayMutation::Add {
                parent: parent.clone(),
                child: child.clone(),
                index: None,
            },
        )?;
        if let Some(old) = occupied {
            let (removed, _) = self.as2_remove_subtree(&old)?;
            let mut events = removed.events;
            events.append(&mut result.events);
            result.events = events;
        }
        self.graph
            .as2_depths
            .insert(child.clone(), (parent.clone(), depth));
        self.graph.as2_sort_depths(parent);
        Ok(result)
    }
    fn as2_promote_placement(&mut self, target: &ObjectRef) -> Result<(), DisplayError> {
        if self.graph.dynamic.contains_key(target) || self.graph.as2_promoted.contains_key(target) {
            return Ok(());
        }
        let (name, transform, visible, kind, symbol_id, frame) =
            if let Some(address) = self.graph.linked_paths.get(target).cloned() {
                let scoped = self
                    .graph
                    .namespaces
                    .get(&address.namespace)
                    .ok_or_else(|| error("swapDepths linked namespace is no longer active"))?;
                let node = scoped
                    .player
                    .previous
                    .get(&address.path)
                    .ok_or_else(|| error("swapDepths target placement is no longer active"))?;
                let mut t = node.transform;
                let mut vis = node.visible;
                let mut nm = node.name.clone();
                if let Some(patch) = scoped.player.overrides.get(&address.path) {
                    patch.apply(&mut t);
                    if let Some(v) = patch.visible {
                        vis = v;
                    }
                    if let Some(n) = &patch.name {
                        nm = n.clone();
                    }
                }
                (
                    nm,
                    t,
                    vis,
                    node.kind.clone(),
                    node.symbol_id.clone(),
                    node.frame,
                )
            } else if let Some(path) = self.graph.paths.get(target).cloned() {
                if path.is_empty() {
                    return Err(error("swapDepths cannot promote the root timeline"));
                }
                let node = self
                    .previous
                    .get(&path)
                    .ok_or_else(|| error("swapDepths target placement is no longer active"))?;
                let mut t = node.transform;
                let mut vis = node.visible;
                let mut nm = node.name.clone();
                if let Some(patch) = self.overrides.get(&path) {
                    patch.apply(&mut t);
                    if let Some(v) = patch.visible {
                        vis = v;
                    }
                    if let Some(n) = &patch.name {
                        nm = n.clone();
                    }
                }
                (
                    nm,
                    t,
                    vis,
                    node.kind.clone(),
                    node.symbol_id.clone(),
                    node.frame,
                )
            } else {
                return Err(error("swapDepths requires a live MovieClip"));
            };
        if !self.graph.affines.contains_key(target)
            && let Ok(affine) = transform::AffineState::new(transform_matrix(transform))
        {
            self.graph.affines.insert(target.clone(), affine);
        }
        self.graph
            .color_transforms
            .entry(target.clone())
            .or_insert_with(|| transform.color_transform.with_alpha(transform.alpha));
        self.graph.as2_promoted.insert(
            target.clone(),
            PromotedAuthoredMeta {
                name,
                transform,
                visible,
                kind,
                symbol_id,
                frame,
            },
        );
        if let Some(parent) = self.graph.parent(target) {
            let suffix = self.graph.suffix.entry(parent).or_default();
            if !suffix.contains(target) {
                suffix.push(target.clone());
            }
        }
        Ok(())
    }
    pub fn as2_remove_subtree(
        &mut self,
        reference: &ObjectRef,
    ) -> Result<(MutationResult, Vec<ObjectRef>), DisplayError> {
        if !self.graph.alive(reference) {
            return Ok((MutationResult::default(), Vec::new()));
        }
        let Some(parent) = self.graph.parent(reference) else {
            return Ok((MutationResult::default(), Vec::new()));
        };
        self.graph
            .validate_loader_mutation(&DisplayMutation::Remove {
                parent: parent.clone(),
                child: reference.clone(),
            })?;
        let subtree = self.graph.subtree(reference);
        if subtree.len() + 1 > MAX_DISPLAY_EVENTS {
            return Err(error("AS2 removal exceeds its lifecycle event budget"));
        }
        let attached = self.graph.on_stage(reference);
        let target_route = self.graph.route(reference);
        let mut events = vec![DisplayEvent {
            kind: "removed".into(),
            stage_route: attached,
            target_ref: reference.clone(),
            route: target_route,
        }];
        if attached {
            for id in &subtree {
                events.push(DisplayEvent {
                    kind: "removed_from_stage".into(),
                    stage_route: true,
                    target_ref: id.clone(),
                    route: self.graph.route(id),
                });
            }
        }

        self.graph.detach_child(reference);

        let mut descendants: BTreeSet<ObjectRef> = subtree.into_iter().skip(1).collect();
        // Also collect any authored descendants under `reference`'s path and any linked_authored
        // nodes belonging to namespaces inside the removed subtree.
        if let Some(root_path) = self.graph.paths.get(reference).cloned() {
            if let Some(auth) = self.graph.authored.get(&root_path) {
                self.graph
                    .retired_promoted_slots
                    .insert(RetiredAuthoredSlot {
                        namespace: None,
                        path: root_path.clone(),
                        lifetime: auth.lifetime,
                    });
            }
            let sub_paths: Vec<Vec<Id>> = self
                .graph
                .authored
                .keys()
                .filter(|p| p.len() > root_path.len() && p.starts_with(&root_path))
                .cloned()
                .collect();
            for sp in sub_paths {
                if let Some(auth) = self.graph.authored.remove(&sp) {
                    self.graph
                        .retired_promoted_slots
                        .insert(RetiredAuthoredSlot {
                            namespace: None,
                            path: sp,
                            lifetime: auth.lifetime,
                        });
                    self.graph.paths.remove(&auth.reference);
                    descendants.insert(auth.reference);
                }
            }
        } else if let Some(addr) = self.graph.linked_paths.get(reference).cloned() {
            if let Some(auth) = self.graph.linked_authored.get(&addr) {
                self.graph
                    .retired_promoted_slots
                    .insert(RetiredAuthoredSlot {
                        namespace: Some(addr.namespace.clone()),
                        path: addr.path.clone(),
                        lifetime: auth.lifetime,
                    });
            }
            let sub_addrs: Vec<SourceAddress> = self
                .graph
                .linked_authored
                .keys()
                .filter(|a| {
                    a.namespace == addr.namespace
                        && a.path.len() > addr.path.len()
                        && a.path.starts_with(&addr.path)
                })
                .cloned()
                .collect();
            for sa in sub_addrs {
                if let Some(auth) = self.graph.linked_authored.remove(&sa) {
                    self.graph
                        .retired_promoted_slots
                        .insert(RetiredAuthoredSlot {
                            namespace: Some(sa.namespace),
                            path: sa.path,
                            lifetime: auth.lifetime,
                        });
                    self.graph.linked_paths.remove(&auth.reference);
                    descendants.insert(auth.reference);
                }
            }
        }

        // Stop and remove any LinkedNamespaces owned by `reference` or its descendants.
        let mut ns_queue: Vec<ObjectRef> = std::iter::once(reference.clone())
            .chain(descendants.iter().cloned())
            .filter(|id| self.graph.namespaces.contains_key(id))
            .collect();
        while let Some(ns_id) = ns_queue.pop() {
            if let Some(mut ns) = self.graph.namespaces.remove(&ns_id) {
                let sounds = ns.player.stop_and_drain_scoped_sounds(&ns_id);
                self.sound.extend(sounds);
                if let Some(prefix) = ns.mounted_prefix {
                    self.graph.retired_published_prefixes.insert(prefix);
                }
            }
            let linked_keys: Vec<SourceAddress> = self
                .graph
                .linked_authored
                .keys()
                .filter(|a| a.namespace == ns_id)
                .cloned()
                .collect();
            for lk in linked_keys {
                if let Some(auth) = self.graph.linked_authored.remove(&lk) {
                    self.graph.linked_paths.remove(&auth.reference);
                    if descendants.insert(auth.reference.clone())
                        && self.graph.namespaces.contains_key(&auth.reference)
                    {
                        ns_queue.push(auth.reference);
                    }
                }
            }
        }

        let loader_sounds = self.graph.retire_loader_children(&descendants);
        self.sound.extend(loader_sounds);
        self.graph.retire_dynamic_descendants(&descendants);

        // Convert or clean `reference` into a lightweight detached shell so immediate
        // `.parent === null` queries succeed while zero graphics, children, text or bitmaps leak.
        let root_loader_sounds = self
            .graph
            .retire_loader_children(&BTreeSet::from([reference.clone()]));
        self.sound.extend(root_loader_sounds);
        self.graph.convert_removed_target_to_shell(reference);

        let retired_vec: Vec<ObjectRef> = descendants.into_iter().collect();
        Ok((MutationResult { events }, retired_vec))
    }
    pub fn as2_swap_depth(
        &mut self,
        reference: &ObjectRef,
        depth: i32,
    ) -> Result<(), DisplayError> {
        swap_depth_value(depth)?;
        let parent = self
            .graph
            .parent(reference)
            .ok_or_else(|| error("swapDepths requires an attached runtime MovieClip"))?;
        let previous = self.as2_get_depth(reference)?;
        let other = self.as2_child_at_depth(&parent, depth)?;
        self.graph.as2_freeze_depths(&parent);
        self.as2_promote_placement(reference)?;
        if let Some(ref other_ref) = other
            && other_ref != reference
        {
            self.as2_promote_placement(other_ref)?;
        }
        self.graph
            .as2_depths
            .insert(reference.clone(), (parent.clone(), depth));
        if let Some(other) = other
            && other != *reference
        {
            self.graph
                .as2_depths
                .insert(other, (parent.clone(), previous));
        }
        self.graph.as2_sort_depths(&parent);
        Ok(())
    }
    /// Flash duplicates drawing and transform, but not arbitrary Object fields
    /// or children dynamically added to the source. Linked/authored symbol
    /// reconstruction remains explicit rather than flattening timeline content.
    pub fn as2_duplicate(
        &mut self,
        project: &Project,
        source: &ObjectRef,
        name: &str,
        depth: i32,
    ) -> Result<(ObjectRef, MutationResult), DisplayError> {
        depth_value(depth)?;
        if name.len() > 4096 {
            return Err(error("duplicateMovieClip name exceeds 4096 UTF-8 bytes"));
        }
        let original = self.graph.dynamic.get(source).filter(|node| node.kind == DynamicKind::MovieClip)
            .ok_or_else(|| error("duplicateMovieClip currently requires a runtime-created MovieClip; authored timeline duplication is pending"))?;
        if self.graph.namespaces.contains_key(source) || self.graph.loaders.contains_key(source) {
            return Err(error(
                "duplicateMovieClip of linked or externally loaded timelines is pending; use attachMovie for a fresh linked instance",
            ));
        }
        let parent = original
            .parent
            .clone()
            .ok_or_else(|| error("duplicateMovieClip cannot duplicate a detached or root clip"))?;
        if self.graph.graphics_cost + original.graphics.cost() > MAX_GRAPHICS_COMMANDS {
            return Err(error(
                "duplicateMovieClip drawing exceeds the publication storage budget",
            ));
        }
        let graphics = original.graphics.clone();
        let matrix = self.object_matrix(project, source)?;
        let color = self.object_color_transform(project, source)?;
        let copy = self.create_display(DynamicKind::MovieClip)?;
        self.object_set_name(&copy, name);
        self.object_set_matrix(&copy, matrix)?;
        self.object_set_color_transform(&copy, color)?;
        self.graph.graphics_cost += graphics.cost();
        self.graph
            .dynamic
            .get_mut(&copy)
            .expect("new clip")
            .graphics = graphics;
        match self.as2_place_depth(project, &parent, &copy, depth) {
            Ok(events) => Ok((copy, events)),
            Err(failure) => {
                let node = self.graph.dynamic.remove(&copy).expect("new clip");
                self.graph.graphics_cost -= node.graphics.cost();
                self.graph.affines.remove(&copy);
                self.graph.color_transforms.remove(&copy);
                Err(failure)
            }
        }
    }
    pub fn as2_target_path(
        &self,
        _project: &Project,
        target: &ObjectRef,
        slash: bool,
    ) -> Result<String, DisplayError> {
        if !self.graph.on_stage(target) {
            return Err(error("AS2 target paths require an attached display object"));
        }
        let mut names = vec![];
        for reference in self.graph.route(target).into_iter().rev().skip(1) {
            let name = self
                .object_name(&reference)
                .ok_or_else(|| error("AS2 path target is stale"))?;
            if name.is_empty() || name.contains(['/', '.', ':']) {
                return Err(error(
                    "AS2 string paths require nonempty instance names without slash, dot or colon; pass a display object instead",
                ));
            }
            names.push(name);
        }
        Ok(if slash {
            format!("/{}", names.join("/"))
        } else if names.is_empty() {
            "_level0".into()
        } else {
            format!("_level0.{}", names.join("."))
        })
    }
    pub fn as2_resolve_target(
        &self,
        project: &Project,
        relative: &ObjectRef,
        path: &str,
    ) -> Result<Option<ObjectRef>, DisplayError> {
        self.as2_resolve_target_case(project, relative, path, false)
    }

    pub fn as2_resolve_target_case(
        &self,
        project: &Project,
        relative: &ObjectRef,
        path: &str,
        case_insensitive: bool,
    ) -> Result<Option<ObjectRef>, DisplayError> {
        if path.len() > 4096 || !self.graph.alive(relative) {
            return Err(error(
                "AS2 target path is oversized or its relative target is stale",
            ));
        }
        let eq_part = |left: &str, right: &str| {
            if case_insensitive {
                crate::as2_strings::legacy_case_fold(left)
                    == crate::as2_strings::legacy_case_fold(right)
            } else {
                left == right
            }
        };
        let root = self
            .object_ref(&[])
            .ok_or_else(|| error("Publication root is unavailable"))?;
        let locked_root = self.as2_root(relative)?;
        let mut current = relative.clone();
        let mut tail = path;
        for (prefix, base) in [("_root", &locked_root), ("_level0", &root)] {
            if eq_part(path, prefix) {
                return Ok(Some(base.clone()));
            }
            if path.len() > prefix.len()
                && path.is_char_boundary(prefix.len())
                && eq_part(&path[..prefix.len()], prefix)
                && path[prefix.len()..].starts_with(['.', '/'])
            {
                current = base.clone();
                tail = &path[prefix.len() + 1..];
                break;
            }
        }
        if path.len() >= 6 && path.is_char_boundary(6) && eq_part(&path[..6], "_level") {
            let rest = &path[6..];
            let digits_len = rest.bytes().take_while(u8::is_ascii_digit).count();
            if digits_len > 0 && &rest[..digits_len] != "0" {
                let after = &rest[digits_len..];
                if after.is_empty() || after.starts_with(['.', '/']) {
                    return Ok(None);
                }
            }
        }
        if let Some(rest) = tail.strip_prefix('/') {
            current = locked_root;
            tail = rest;
        }
        let _ = project;
        // Slash paths preserve '..'; dotted paths use _parent explicitly.
        let parts: Vec<_> = if tail.contains('/') || tail == ".." {
            tail.split('/').collect()
        } else {
            tail.split('.').collect()
        };
        if parts.len() > MAX_GRAPH_DEPTH {
            return Err(error("AS2 target path exceeds 64 segments"));
        }
        for part in parts {
            if part.is_empty() || eq_part(part, "this") {
                continue;
            }
            if part == ".." || eq_part(part, "_parent") {
                let Some(parent) = self.graph.parent(&current) else {
                    return Ok(None);
                };
                current = parent;
                continue;
            }
            let children = self.graph.children(&current);
            let child = children
                .iter()
                .find(|child_ref| self.object_name(child_ref).is_some_and(|name| name == part))
                .or_else(|| {
                    case_insensitive.then(|| {
                        children.iter().find(|child_ref| {
                            self.object_name(child_ref)
                                .is_some_and(|name| eq_part(&name, part))
                        })
                    })?
                });
            let Some(child) = child else {
                return Ok(None);
            };
            current = child.clone();
        }
        Ok(Some(current))
    }

    pub fn as2_call_frame_scripts(
        &self,
        project: &Project,
        relative: &ObjectRef,
        spec: &crate::GotoFrame,
    ) -> Result<Option<(ObjectRef, Vec<Script>)>, DisplayError> {
        if !self.graph.alive(relative) {
            return Err(error("AS2 call() relative target is stale"));
        }
        let (target, frame_spec) = match spec {
            crate::GotoFrame::Label(text) => {
                if text.len() > 4096 {
                    return Err(error("AS2 call() target specifier exceeds 4096 bytes"));
                }
                if let Some((target_path, frame_part)) = text.rsplit_once(':') {
                    let resolved = if target_path.is_empty() {
                        Some(relative.clone())
                    } else {
                        self.as2_resolve_target(project, relative, target_path)?
                    };
                    let Some(clip) = resolved else {
                        return Ok(None);
                    };
                    (clip, crate::GotoFrame::Label(frame_part.to_string()))
                } else {
                    (relative.clone(), crate::GotoFrame::Label(text.clone()))
                }
            }
            crate::GotoFrame::Number(num) => (relative.clone(), crate::GotoFrame::Number(*num)),
        };
        let request = crate::GotoRequest {
            frame: frame_spec,
            scene: None,
            as2: true,
        };
        let Ok(Some(frame)) = self.object_resolve_goto(project, &target, &request) else {
            return Ok(None);
        };
        Ok(Some((
            target.clone(),
            self.object_frame_scripts(project, &target, frame),
        )))
    }

    pub fn as2_is_frame_loaded(
        &self,
        project: &Project,
        relative: &ObjectRef,
        request: &crate::GotoRequest,
    ) -> Result<bool, DisplayError> {
        if !self.graph.alive(relative) {
            return Err(error("AS2 ifFrameLoaded target is stale"));
        }
        if self.object_frame(relative).is_none() {
            return Err(error("ifFrameLoaded requires a timeline MovieClip"));
        }
        let mut req = request.clone();
        req.as2 = true;
        Ok(matches!(
            self.object_resolve_goto(project, relative, &req),
            Ok(Some(_))
        ))
    }

    pub fn as2_accessibility_update(
        &mut self,
        entries: Vec<As2AccUpdateEntry>,
    ) -> Result<(), DisplayError> {
        if entries.len() > MAX_DYNAMIC_OBJECTS {
            return Err(error("AS2 accessibility update exceeds object budget"));
        }
        let mut next = BTreeMap::new();
        for entry in entries {
            if !self.graph.alive(&entry.object_ref) {
                continue;
            }
            if entry.props.name.as_ref().is_some_and(|s| s.len() > 4096)
                || entry
                    .props
                    .description
                    .as_ref()
                    .is_some_and(|s| s.len() > 4096)
            {
                return Err(error(
                    "AS2 _accProps name/description must be at most 4096 UTF-8 bytes",
                ));
            }
            next.insert(entry.object_ref, entry.props);
        }
        self.graph.as2_accessibility = next;
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct As2AccProps {
    #[serde(default)]
    pub silent: bool,
    #[serde(default)]
    pub force_simple: bool,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub interactive: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct As2AccUpdateEntry {
    pub object_ref: ObjectRef,
    pub props: As2AccProps,
}
