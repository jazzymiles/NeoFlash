//! AS2 sparse depths and target paths, separate from AS3 child-index operations.
//! Semantics adapted from Ruffle's MIT AVM1 movie_clip.rs at
//! 89a7049965f3090e706265670df309a5128233ac; see docs/AS2-DISPLAY-BATCH.md.
use super::*;
const MAX_DEPTH: i32 = 1_048_575;

fn depth_value(value: i32) -> Result<(), DisplayError> {
    if !(0..=MAX_DEPTH).contains(&value) {
        return Err(error(
            "AS2 runtime depth requires an integer from 0 through 1048575; authored negative depths are read-only",
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
    /// them. Authored negative-depth placements remain immutable.
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
        if !self.graph.alive(reference) {
            return Err(error("DepthManager target is stale or foreign"));
        }
        let planning = op == "plan";
        let parent = if planning {
            reference.clone()
        } else {
            if !self.graph.dynamic.contains_key(reference) {
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
        if depth < 0 {
            return Ok(usize::try_from(depth + 16384)
                .ok()
                .and_then(|index| self.graph.prefix.get(parent)?.get(index).cloned())
                .filter(|child| self.graph.parent(child).as_ref() == Some(parent)));
        }
        Ok(self
            .graph
            .as2_depths(parent)
            .into_iter()
            .find_map(|(child, value)| (value == depth).then_some(child)))
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
            let removed = self.display_mutate(
                project,
                DisplayMutation::Remove {
                    parent: parent.clone(),
                    child: old,
                },
            )?;
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
    pub fn as2_swap_depth(
        &mut self,
        reference: &ObjectRef,
        depth: i32,
    ) -> Result<(), DisplayError> {
        depth_value(depth)?;
        let parent = self
            .graph
            .parent(reference)
            .ok_or_else(|| error("swapDepths requires an attached runtime MovieClip"))?;
        if !self.graph.dynamic.contains_key(reference) {
            return Err(error(
                "swapDepths on authored timeline placements is outside the runtime depth profile",
            ));
        }
        let previous = self.as2_get_depth(reference)?;
        let other = self.as2_child_at_depth(&parent, depth)?;
        self.graph.as2_freeze_depths(&parent);
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
        project: &Project,
        target: &ObjectRef,
        slash: bool,
    ) -> Result<String, DisplayError> {
        if !self.graph.on_stage(target) {
            return Err(error("AS2 target paths require an attached display object"));
        }
        let snapshot = self.runtime_snapshot(project);
        let mut names = vec![];
        for reference in self.graph.route(target).into_iter().rev().skip(1) {
            let node = snapshot
                .objects
                .iter()
                .find(|node| node.object_ref == reference)
                .ok_or_else(|| error("AS2 path target is stale"))?;
            if node.display.name.is_empty() || node.display.name.contains(['/', '.', ':']) {
                return Err(error(
                    "AS2 string paths require nonempty instance names without slash, dot or colon; pass a display object instead",
                ));
            }
            names.push(node.display.name.as_str());
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
        if path.len() > 4096 || !self.graph.alive(relative) {
            return Err(error(
                "AS2 target path is oversized or its relative target is stale",
            ));
        }
        let root = self
            .object_ref(&[])
            .ok_or_else(|| error("Publication root is unavailable"))?;
        let mut current = relative.clone();
        let mut tail = path;
        for prefix in ["_root", "_level0"] {
            if path == prefix {
                return Ok(Some(root));
            }
            if let Some(rest) = path
                .strip_prefix(prefix)
                .filter(|rest| rest.starts_with(['.', '/']))
            {
                current = root.clone();
                tail = &rest[1..];
                break;
            }
        }
        if let Some(rest) = tail.strip_prefix('/') {
            current = root;
            tail = rest;
        }
        let snapshot = self.runtime_snapshot(project);
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
            if part.is_empty() || part == "this" {
                continue;
            }
            if part == ".." || part == "_parent" {
                let Some(parent) = self.graph.parent(&current) else {
                    return Ok(None);
                };
                current = parent;
                continue;
            }
            let children = self.graph.children(&current);
            let Some(child) = snapshot
                .objects
                .iter()
                .find(|node| children.contains(&node.object_ref) && node.display.name == part)
            else {
                return Ok(None);
            };
            current = child.object_ref.clone();
        }
        Ok(Some(current))
    }
}
