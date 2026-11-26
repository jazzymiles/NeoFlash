//! Scoped authored timelines inside runtime-created, explicitly exported clips.
use super::*;

const MAX_LINKED_NODES: usize = 100_000;
const SCRIPT_LIMIT: usize = 4096;

pub(super) fn symbol_budget(
    project: &Project,
    id: &str,
    seen: &mut BTreeSet<Id>,
    memo: &mut HashMap<Id, (usize, usize)>,
) -> Result<(usize, usize), DisplayError> {
    if let Some(value) = memo.get(id) {
        return Ok(*value);
    }
    if !seen.insert(id.into()) {
        return Err(error("Recursive linked symbol graph is unsupported"));
    }
    let symbol = project
        .symbol(id)
        .ok_or_else(|| error("Linked symbol reference is missing"))?;
    let mut nodes = 1usize;
    let mut depth = 1usize;
    for layer in &symbol.timeline.layers {
        let mut largest = 0;
        for key in &layer.keyframes {
            let mut count = 0usize;
            for element in &key.elements {
                let (n, d) = if let ElementKind::Instance { symbol_id, .. } = &element.kind {
                    symbol_budget(project, symbol_id, seen, memo)?
                } else {
                    (1, 1)
                };
                count = count.saturating_add(n);
                depth = depth.max(1 + d);
                if count > MAX_LINKED_NODES || depth > MAX_GRAPH_DEPTH {
                    return Err(error(
                        "Linked timeline exceeds its reserved node/depth budget",
                    ));
                }
            }
            largest = largest.max(count);
        }
        nodes = nodes.saturating_add(largest);
        if nodes > MAX_LINKED_NODES {
            return Err(error("Linked timeline exceeds its reserved node budget"));
        }
    }
    seen.remove(id);
    memo.insert(id.into(), (nodes, depth));
    Ok((nodes, depth))
}

impl RuntimeGraph {
    pub(super) fn linked_ref(&self, namespace: &ObjectRef, path: &[Id]) -> Option<ObjectRef> {
        if path.is_empty() {
            return self
                .namespaces
                .contains_key(namespace)
                .then(|| namespace.clone());
        }
        self.linked_authored
            .get(&SourceAddress {
                namespace: namespace.clone(),
                path: path.into(),
            })
            .map(|node| node.reference.clone())
    }
    pub(super) fn source_address(&self, reference: &ObjectRef) -> Option<SourceAddress> {
        if self.namespaces.contains_key(reference) {
            Some(SourceAddress {
                namespace: reference.clone(),
                path: vec![],
            })
        } else {
            self.linked_paths.get(reference).cloned()
        }
    }
}

impl Player {
    /// Whether a linked player (at any nesting depth) may have host audio that a
    /// parent reset must silence before retiring its namespaces.
    pub(crate) fn linked_sound_started(&self) -> bool {
        self.graph.namespaces.values().any(|namespace| {
            namespace.player.sound.has_started() || namespace.player.linked_sound_started()
        })
    }

    /// A Player transport pause applies to runtime-created linked timelines too.
    pub(crate) fn set_linked_playing(&mut self, playing: bool) {
        for namespace in self.graph.namespaces.values_mut() {
            namespace.player.set_playing(playing);
        }
    }

    /// The parent's StopAll command is host-global. Remove commands that would
    /// otherwise drain after it and reset every linked stream for later resync.
    pub(crate) fn clear_linked_sounds_for_global_stop(&mut self) {
        for namespace in self.graph.namespaces.values_mut() {
            namespace.player.clear_linked_sounds_for_global_stop();
            namespace.player.sound.clear_for_global_stop();
        }
    }

    fn stop_streams_recursive(&mut self) {
        self.sound.stop_streams();
        for namespace in self.graph.namespaces.values_mut() {
            namespace.player.stop_streams_recursive();
        }
    }

    /// Stop and drain one linked namespace before its graph entry is retired.
    /// Nested namespaces are already scoped by their owning linked player; applying
    /// this outer scope produces the same channel as the normal linked drain path.
    pub(crate) fn stop_and_drain_scoped_sounds(
        &mut self,
        reference: &ObjectRef,
    ) -> Vec<crate::SoundCommand> {
        self.stop_streams_recursive();
        let scope = serde_json::to_string(reference).unwrap_or_default();
        self.drain_sound_commands()
            .into_iter()
            .map(|command| command.scoped(&scope))
            .collect()
    }

    /// Seek retires linked namespaces. Preserve their already queued commands and
    /// explicit stream stops in the parent's queue before the graph is cleared.
    pub(crate) fn stop_and_drain_linked_streams(&mut self) -> Vec<crate::SoundCommand> {
        let mut commands = vec![];
        for (reference, namespace) in &mut self.graph.namespaces {
            commands.extend(namespace.player.stop_and_drain_scoped_sounds(reference));
        }
        commands
    }

    pub fn create_linked(
        &mut self,
        project: &Project,
        linkage_id: &str,
    ) -> Result<ObjectRef, DisplayError> {
        if linkage_id.is_empty() || linkage_id.len() > 512 {
            return Err(error("Invalid library linkage identifier"));
        }
        let mut matches = project
            .symbols
            .iter()
            .filter(|symbol| symbol.linkage_id.as_deref() == Some(linkage_id));
        let symbol = matches
            .next()
            .ok_or_else(|| error("No symbol is exported with this exact linkage identifier"))?;
        if matches.next().is_some()
            || !matches!(symbol.kind, SymbolKind::MovieClip | SymbolKind::Button)
            || symbol.id == project.root
        {
            return Err(error(
                "Library linkage must identify one exported non-root MovieClip or Button",
            ));
        }
        let (nodes, depth) = symbol_budget(
            project,
            &symbol.id,
            &mut BTreeSet::new(),
            &mut HashMap::new(),
        )?;
        if nodes
            + self
                .graph
                .namespaces
                .values()
                .map(|namespace| namespace.nodes)
                .sum::<usize>()
            > MAX_LINKED_NODES
        {
            return Err(error(
                "Linked timelines exceed 100000 reserved objects, including detached clips",
            ));
        }
        self.graph.evict_removed_shells_if_needed();
        if self.graph.dynamic.len() >= MAX_DYNAMIC_OBJECTS {
            return Err(error("Dynamic display object limit reached"));
        }
        let scoped =
            Player::with_source_root_playing(project, Some(symbol.id.clone()), self.is_playing());
        let reference = self.create_display(DynamicKind::MovieClip)?;
        self.graph.namespaces.insert(
            reference.clone(),
            LinkedNamespace {
                player: Box::new(scoped),
                nodes,
                depth,
                mounted_prefix: None,
            },
        );
        self.sync_linked(project);
        self.collect_linked_scripts(&reference);
        Ok(reference)
    }

    pub(crate) fn materialize_promoted_symbols(&mut self, project: &Project) {
        if self.graph.promoted_symbols.is_empty() {
            return;
        }
        let pending = std::mem::take(&mut self.graph.promoted_symbols);
        let playing = self.is_playing();
        for (reference, (symbol_id, frame)) in pending {
            if !self.graph.dynamic.contains_key(&reference)
                || self.graph.namespaces.contains_key(&reference)
            {
                continue;
            }
            let Ok((nodes, depth)) = symbol_budget(
                project,
                &symbol_id,
                &mut BTreeSet::new(),
                &mut HashMap::new(),
            ) else {
                continue;
            };
            let used_nodes: usize = self.graph.namespaces.values().map(|ns| ns.nodes).sum();
            if nodes + used_nodes > MAX_LINKED_NODES {
                continue;
            }
            let mut scoped = Player::with_source_root_playing(project, Some(symbol_id), playing);
            if frame > 0 {
                scoped.control(project, &[], PlaybackCommand::GotoAndPlay(frame));
            }
            self.graph.namespaces.insert(
                reference,
                LinkedNamespace {
                    player: Box::new(scoped),
                    nodes,
                    depth,
                    mounted_prefix: None,
                },
            );
        }
    }

    pub(super) fn sync_linked(&mut self, project: &Project) {
        self.materialize_promoted_symbols(project);
        let raw_placements: Vec<_> = self
            .graph
            .namespaces
            .iter()
            .flat_map(|(namespace, scoped)| {
                scoped
                    .player
                    .display_nodes(project)
                    .into_iter()
                    .filter(|node| !node.path.is_empty())
                    .map(|node| {
                        (
                            SourceAddress {
                                namespace: namespace.clone(),
                                path: node.path,
                            },
                            node.lifetime_id.parse::<u64>().expect("core lifetime"),
                            node.blend_mode,
                            node.cache_as_bitmap,
                            node.filters,
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        let raw_active: HashMap<SourceAddress, u64> = raw_placements
            .iter()
            .map(|(address, lifetime, _, _, _)| (address.clone(), *lifetime))
            .collect();
        self.graph.retired_promoted_slots.retain(|slot| {
            let Some(ref ns) = slot.namespace else {
                return true;
            };
            raw_active.get(&SourceAddress {
                namespace: ns.clone(),
                path: slot.path.clone(),
            }) == Some(&slot.lifetime)
        });
        let placements: Vec<_> = raw_placements
            .into_iter()
            .filter(|(address, _, _, _, _)| {
                !(1..=address.path.len()).any(|k| {
                    let prefix_addr = SourceAddress {
                        namespace: address.namespace.clone(),
                        path: address.path[..k].to_vec(),
                    };
                    raw_active.get(&prefix_addr).is_some_and(|lt| {
                        self.graph
                            .retired_promoted_slots
                            .contains(&RetiredAuthoredSlot {
                                namespace: Some(address.namespace.clone()),
                                path: prefix_addr.path,
                                lifetime: *lt,
                            })
                    })
                })
            })
            .collect();
        let active: HashMap<_, _> = placements
            .iter()
            .map(|(address, lifetime, _, _, _)| (address, *lifetime))
            .collect();

        let promoted_survivors: Vec<(SourceAddress, ObjectRef)> = self
            .graph
            .linked_authored
            .iter()
            .filter(|(address, node)| {
                active.get(address) != Some(&node.lifetime)
                    && self.graph.as2_promoted.contains_key(&node.reference)
                    && self
                        .graph
                        .as2_depths
                        .get(&node.reference)
                        .is_some_and(|(_, d)| *d >= 0)
                    && self.graph.parent(&node.reference).is_some()
            })
            .map(|(address, node)| (address.clone(), node.reference.clone()))
            .collect();
        for (address, reference) in promoted_survivors {
            let parent = self.graph.parent(&reference);
            let meta = self
                .graph
                .as2_promoted
                .remove(&reference)
                .expect("promoted metadata");
            self.graph.linked_authored.remove(&address);
            self.graph.linked_paths.remove(&reference);
            self.graph.parent_overrides.remove(&reference);
            if let Some(ref sym_id) = meta.symbol_id {
                self.graph
                    .promoted_symbols
                    .insert(reference.clone(), (sym_id.clone(), meta.frame));
            }
            self.graph.dynamic.insert(
                reference.clone(),
                DynamicObject {
                    kind: DynamicKind::MovieClip,
                    parent: parent.clone(),
                    transform: meta.transform,
                    visible: meta.visible,
                    name: meta.name,
                    playing: true,
                    graphics: GraphicsState::default(),
                    authored_kind: meta.kind,
                },
            );
            if let Some(parent_ref) = parent {
                let suffix = self.graph.suffix.entry(parent_ref).or_default();
                if !suffix.contains(&reference) {
                    suffix.push(reference);
                }
            }
        }
        if !self.graph.promoted_symbols.is_empty() {
            self.materialize_promoted_symbols(project);
        }
        let retired_info: Vec<_> = {
            let mut list: Vec<_> = self
                .graph
                .linked_authored
                .iter()
                .filter(|(address, node)| active.get(address) != Some(&node.lifetime))
                .map(|(address, node)| {
                    (
                        address.clone(),
                        node.reference.clone(),
                        self.graph.route(&node.reference),
                        self.graph.on_stage(&node.reference),
                    )
                })
                .collect();
            list.sort_by_key(|a| std::cmp::Reverse(a.0.path.len()));
            list
        };
        let retired: BTreeSet<_> = retired_info
            .iter()
            .map(|(_, reference, _, _)| reference.clone())
            .collect();
        let retired_sound_commands = self.graph.retire_loader_children(&retired);
        self.sound.extend(retired_sound_commands);
        self.graph.retire_hierarchy(&retired);
        for (_, reference, route, was_on_stage) in retired_info {
            self.graph.push_timeline_event(DisplayEvent {
                kind: "removed".into(),
                stage_route: was_on_stage,
                target_ref: reference.clone(),
                route: route.clone(),
            });
            if was_on_stage {
                self.graph.push_timeline_event(DisplayEvent {
                    kind: "removed_from_stage".into(),
                    stage_route: true,
                    target_ref: reference,
                    route,
                });
            }
        }
        self.graph
            .linked_authored
            .retain(|address, node| active.get(address) == Some(&node.lifetime));
        self.graph
            .linked_paths
            .retain(|reference, _| !retired.contains(reference));
        self.graph
            .bitmap_states
            .retain(|reference, _| !retired.contains(reference));
        self.graph.retire_text(&retired);
        self.graph.retire_video(&retired);
        self.graph.retire_input(&retired);
        self.graph.retire_filters(&retired);
        self.graph.retire_clipping(&retired);
        self.graph
            .affines
            .retain(|reference, _| !retired.contains(reference));
        self.graph
            .color_transforms
            .retain(|reference, _| !retired.contains(reference));
        let mut added_addresses = Vec::new();
        for (address, lifetime, blend_mode, cache_as_bitmap, filters) in &placements {
            if !self.graph.linked_authored.contains_key(address) {
                let reference = self.graph.allocate();
                self.graph
                    .linked_paths
                    .insert(reference.clone(), address.clone());
                self.graph.linked_authored.insert(
                    address.clone(),
                    Authored {
                        reference,
                        lifetime: *lifetime,
                    },
                );
                added_addresses.push(address.clone());
            }
            let reference = self.graph.linked_authored[address].reference.clone();
            self.graph
                .sync_authored_render(&reference, *blend_mode, *cache_as_bitmap, filters);
        }
        self.graph.prefix.retain(|reference, _| {
            !self.graph.linked_paths.contains_key(reference)
                && !self.graph.namespaces.contains_key(reference)
        });
        for (address, _, _, _, _) in placements {
            if let Some(parent) = self
                .graph
                .linked_ref(&address.namespace, &address.path[..address.path.len() - 1])
                && let Some(child_node) = self.graph.linked_authored.get(&address)
            {
                self.graph
                    .prefix
                    .entry(parent)
                    .or_default()
                    .push(child_node.reference.clone());
            }
        }
        for address in added_addresses {
            let Some(node) = self.graph.linked_authored.get(&address) else {
                continue;
            };
            let reference = node.reference.clone();
            let route = self.graph.route(&reference);
            let on_stage = self.graph.on_stage(&reference);
            self.graph.push_timeline_event(DisplayEvent {
                kind: "added".into(),
                stage_route: on_stage,
                target_ref: reference.clone(),
                route: route.clone(),
            });
            if on_stage {
                self.graph.push_timeline_event(DisplayEvent {
                    kind: "added_to_stage".into(),
                    stage_route: true,
                    target_ref: reference,
                    route,
                });
            }
        }
    }

    pub(super) fn collect_linked_scripts(&mut self, namespace: &ObjectRef) {
        let Some(scoped) = self.graph.namespaces.get_mut(namespace) else {
            return;
        };
        let overflow = scoped.player.script_overflowed();
        let scripts = scoped.player.drain_scripts();
        self.overflowed |= overflow;
        for dispatch in scripts {
            let Some(target_ref) = self.graph.linked_ref(namespace, &dispatch.target_path) else {
                continue;
            };
            if self.budget == 0 || self.runtime_scripts.len() >= SCRIPT_LIMIT {
                self.overflowed = true;
                break;
            }
            self.budget -= 1;
            self.runtime_scripts.push_back(RuntimeScriptDispatch {
                dispatch,
                target_ref,
                namespace_ref: Some(namespace.clone()),
            });
        }
    }

    /// Sound commands of linked clips' own timelines, with namespaced stream channels.
    pub(crate) fn drain_linked_sound_commands(&mut self) -> Vec<crate::SoundCommand> {
        let mut commands = vec![];
        for (reference, namespace) in &mut self.graph.namespaces {
            let scope = serde_json::to_string(reference).unwrap_or_default();
            commands.extend(
                namespace
                    .player
                    .drain_sound_commands()
                    .into_iter()
                    .map(|command| command.scoped(&scope)),
            );
        }
        commands
    }

    pub(crate) fn tick_linked(&mut self, project: &Project) {
        if !self.graph.promoted_symbols.is_empty() {
            self.sync_linked(project);
        }
        let namespaces: Vec<_> = self.graph.namespaces.keys().cloned().collect();
        let mut remaining = self.budget;
        for namespace in &namespaces {
            let scoped = &mut self
                .graph
                .namespaces
                .get_mut(namespace)
                .expect("namespace")
                .player;
            scoped.tick_budgeted(project, remaining);
            remaining = scoped.budget;
        }
        if !namespaces.is_empty() {
            self.sync_linked(project);
        }
        for namespace in namespaces {
            self.collect_linked_scripts(&namespace);
        }
    }

    pub fn drain_runtime_scripts(&mut self) -> Vec<RuntimeScriptDispatch> {
        let mut result: Vec<_> = self
            .drain_scripts()
            .into_iter()
            .filter_map(|dispatch| {
                self.object_ref(&dispatch.target_path)
                    .map(|target_ref| RuntimeScriptDispatch {
                        dispatch,
                        target_ref,
                        namespace_ref: None,
                    })
            })
            .collect();
        result.extend(self.runtime_scripts.drain(..));
        result.retain(|item| {
            self.graph.alive(&item.target_ref) && !self.graph.loaded_as2(&item.target_ref)
        });
        if result.len() > SCRIPT_LIMIT {
            result.truncate(SCRIPT_LIMIT);
            self.overflowed = true;
        }
        result
    }
    pub fn is_runtime_dispatch_current(
        &self,
        project: &Project,
        item: &RuntimeScriptDispatch,
    ) -> bool {
        if self.graph.loaded_as2(&item.target_ref) {
            return false;
        }
        if let Some(namespace) = &item.namespace_ref {
            self.graph
                .linked_ref(namespace, &item.dispatch.target_path)
                .as_ref()
                == Some(&item.target_ref)
                && self
                    .graph
                    .namespaces
                    .get(namespace)
                    .is_some_and(|scope| scope.player.is_dispatch_current(project, &item.dispatch))
        } else {
            self.object_ref(&item.dispatch.target_path).as_ref() == Some(&item.target_ref)
                && self.is_dispatch_current(project, &item.dispatch)
        }
    }

    fn source_accessibility(&self, reference: &ObjectRef) -> Option<&Accessibility> {
        if let Some(address) = self.graph.source_address(reference) {
            return self
                .graph
                .namespaces
                .get(&address.namespace)?
                .player
                .placement_accessibility(&address.path);
        }
        self.placement_accessibility(self.graph.paths.get(reference)?)
    }
    pub fn object_click(&mut self, project: &Project, reference: &ObjectRef) -> bool {
        let scene = self.runtime_snapshot(project);
        if !scene.objects.iter().any(|node| {
            &node.object_ref == reference && node.on_stage && node.display.effective_visible
        }) {
            return false;
        }
        let route = self.graph.route(reference);
        if route.iter().any(|id| {
            self.source_accessibility(id)
                .is_some_and(|a| a.role == AccessibleRole::Button && a.disabled)
        }) {
            return false;
        }
        self.budget = SCRIPT_LIMIT;
        self.overflowed = false;
        self.collect_main_scripts();
        for id in route {
            if self.graph.loaded_as2(&id) {
                continue;
            }
            if let Some(address) = self.graph.source_address(&id) {
                let scoped = &mut self
                    .graph
                    .namespaces
                    .get_mut(&address.namespace)
                    .expect("namespace")
                    .player;
                scoped.budget = self.budget;
                scoped.overflowed = false;
                scoped.queue_object_click(project, &address.path);
                self.collect_linked_scripts(&address.namespace);
            } else if let Some(path) = self.graph.paths.get(&id).cloned() {
                self.queue_object_click(project, &path);
                self.collect_main_scripts();
            }
        }
        true
    }
    fn collect_main_scripts(&mut self) {
        for dispatch in self.drain_scripts() {
            let Some(target_ref) = self.object_ref(&dispatch.target_path) else {
                continue;
            };
            if self.runtime_scripts.len() >= SCRIPT_LIMIT {
                self.overflowed = true;
                break;
            }
            self.runtime_scripts.push_back(RuntimeScriptDispatch {
                dispatch,
                target_ref,
                namespace_ref: None,
            });
        }
    }
    pub fn object_activate_accessible(&mut self, project: &Project, reference: &ObjectRef) -> bool {
        if !self.runtime_snapshot(project).semantics.iter().any(|node| {
            &node.owner_ref == reference
                && node.node.accessibility.role == AccessibleRole::Button
                && !node.node.accessibility.disabled
        }) {
            return false;
        }
        self.object_click(project, reference)
    }
}
