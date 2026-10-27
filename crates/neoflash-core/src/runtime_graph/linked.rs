//! Scoped authored timelines inside runtime-created, explicitly exported clips.
use super::*;

const MAX_LINKED_NODES: usize = 100_000;
const SCRIPT_LIMIT: usize = 4096;

fn symbol_budget(
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
            .ok_or_else(|| error("No MovieClip is exported with this exact linkage identifier"))?;
        if matches.next().is_some()
            || symbol.kind != SymbolKind::MovieClip
            || symbol.id == project.root
        {
            return Err(error(
                "Library linkage must identify one exported non-root MovieClip",
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
        if self.graph.dynamic.len() >= MAX_DYNAMIC_OBJECTS {
            return Err(error("Dynamic display object limit reached"));
        }
        let scoped = Player::with_source_root(project, Some(symbol.id.clone()));
        let reference = self.create_display(DynamicKind::MovieClip)?;
        self.graph.namespaces.insert(
            reference.clone(),
            LinkedNamespace {
                player: Box::new(scoped),
                nodes,
                depth,
            },
        );
        self.sync_linked(project);
        self.collect_linked_scripts(&reference);
        Ok(reference)
    }

    pub(super) fn sync_linked(&mut self, project: &Project) {
        let placements: Vec<_> = self
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
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        let active: HashMap<_, _> = placements
            .iter()
            .map(|(address, lifetime)| (address, *lifetime))
            .collect();
        let retired: BTreeSet<_> = self
            .graph
            .linked_authored
            .iter()
            .filter(|(address, node)| active.get(address) != Some(&node.lifetime))
            .map(|(_, node)| node.reference.clone())
            .collect();
        self.graph.retire_loader_children(&retired);
        for parent in &retired {
            for child in self.graph.suffix.get(parent).cloned().unwrap_or_default() {
                let mut events = vec![DisplayEvent {
                    kind: "removed".into(),
                    target_ref: child.clone(),
                    route: self.graph.route(&child),
                }];
                if self.graph.on_stage(&child) {
                    for id in self.graph.subtree(&child) {
                        events.push(DisplayEvent {
                            kind: "removed_from_stage".into(),
                            target_ref: id.clone(),
                            route: self.graph.route(&id),
                        });
                    }
                }
                self.graph
                    .dynamic
                    .get_mut(&child)
                    .expect("dynamic child")
                    .parent = None;
                if self.graph.events.len() + events.len() > MAX_DISPLAY_EVENTS {
                    self.graph.events_overflowed = true;
                } else {
                    self.graph.events.extend(events);
                }
            }
            self.graph.suffix.remove(parent);
            self.graph.prefix.remove(parent);
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
        self.graph.retire_filters(&retired);
        self.graph.retire_clipping(&retired);
        self.graph
            .affines
            .retain(|reference, _| !retired.contains(reference));
        self.graph
            .color_transforms
            .retain(|reference, _| !retired.contains(reference));
        for (address, lifetime) in &placements {
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
            }
        }
        self.graph.prefix.retain(|reference, _| {
            !self.graph.linked_paths.contains_key(reference)
                && !self.graph.namespaces.contains_key(reference)
        });
        for (address, _) in placements {
            let parent = self
                .graph
                .linked_ref(&address.namespace, &address.path[..address.path.len() - 1])
                .expect("namespace parent");
            self.graph
                .prefix
                .entry(parent)
                .or_default()
                .push(self.graph.linked_authored[&address].reference.clone());
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

    pub(crate) fn tick_linked(&mut self, project: &Project) {
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
