//! Image Loader ownership and bounded AS2 content replacement. Encoded bytes
//! cross the host boundary once; decoding and premultiplied pixels stay in Rust.
use super::*;

#[derive(Clone, Debug)]
pub(super) struct LoaderState {
    pub(super) content: Option<ObjectRef>,
    as2: bool,
    pub(super) mounted_prefix: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoaderMutation {
    pub content_ref: Option<ObjectRef>,
    pub events: Vec<DisplayEvent>,
    pub retired_refs: Vec<ObjectRef>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MountedPublishResult {
    pub mutation: LoaderMutation,
    pub prefix: String,
    pub root_symbol_id: String,
    pub symbol_id_map: BTreeMap<String, String>,
    pub resource_id_map: BTreeMap<String, String>,
    #[serde(default)]
    pub resource_path_map: BTreeMap<String, String>,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}
impl RuntimeGraph {
    pub(super) fn loaded_as2(&self, reference: &ObjectRef) -> bool {
        self.loaders
            .get(reference)
            .is_some_and(|state| state.as2 && !self.namespaces.contains_key(reference))
    }
    pub(super) fn loaded_as2_smoothing(&self, reference: &ObjectRef) -> bool {
        let Some(parent) = self
            .dynamic
            .get(reference)
            .and_then(|node| node.parent.as_ref())
        else {
            return false;
        };
        self.loaders
            .get(parent)
            .is_some_and(|state| state.as2 && state.content.as_ref() == Some(reference))
            && self
                .as2_input_states
                .get(parent)
                .is_some_and(|state| state.force_smoothing == Some(true))
    }
    pub(super) fn validate_loader_mutation(
        &self,
        mutation: &DisplayMutation,
    ) -> Result<(), DisplayError> {
        let (parent, first, second) = match mutation {
            DisplayMutation::Add { parent, child, .. }
            | DisplayMutation::Remove { parent, child }
            | DisplayMutation::SetIndex { parent, child, .. } => (parent, child, None),
            DisplayMutation::Swap {
                parent,
                first,
                second,
            } => (parent, first, Some(second)),
        };
        if self
            .dynamic
            .get(parent)
            .is_some_and(|n| n.kind == DynamicKind::Loader)
            || self.loaders.iter().any(|(loader_ref, s)| {
                s.content
                    .as_ref()
                    .is_some_and(|id| id != loader_ref && (id == first || Some(id) == second))
            })
        {
            return Err(error(
                "Loader owns its content child; use load/unload instead of child-list mutation",
            ));
        }
        Ok(())
    }
    fn retire_objects(&mut self, retired: &BTreeSet<ObjectRef>) -> Vec<crate::SoundCommand> {
        let mut sound_commands = vec![];
        self.forget_hierarchy(retired);
        for reference in retired {
            if let Some(node) = self.dynamic.remove(reference) {
                self.graphics_cost -= node.graphics.cost();
            }
            if let Some(mut namespace) = self.namespaces.remove(reference) {
                sound_commands.extend(namespace.player.stop_and_drain_scoped_sounds(reference));
                if let Some(prefix) = namespace.mounted_prefix {
                    self.retired_published_prefixes.insert(prefix);
                }
            }
            self.prefix.remove(reference);
            self.suffix.remove(reference);
            self.loaders.remove(reference);
        }
        self.authored
            .retain(|_, node| !retired.contains(&node.reference));
        self.paths.retain(|id, _| !retired.contains(id));
        self.linked_authored
            .retain(|_, node| !retired.contains(&node.reference));
        self.linked_paths.retain(|id, _| !retired.contains(id));
        for children in self.prefix.values_mut().chain(self.suffix.values_mut()) {
            children.retain(|id| !retired.contains(id));
        }
        self.bitmap_states.retain(|id, _| !retired.contains(id));
        self.affines.retain(|id, _| !retired.contains(id));
        self.color_transforms.retain(|id, _| !retired.contains(id));
        self.retire_text(retired);
        self.retire_video(retired);
        self.retire_input(retired);
        self.retire_filters(retired);
        self.retire_clipping(retired);
        self.events
            .retain(|event| !retired.contains(&event.target_ref));
        sound_commands
    }
    pub(super) fn retire_loader_children(
        &mut self,
        retired: &BTreeSet<ObjectRef>,
    ) -> Vec<crate::SoundCommand> {
        let children: BTreeSet<_> = retired
            .iter()
            .filter_map(|id| self.loaders.get(id).and_then(|s| s.content.clone()))
            .flat_map(|id| self.subtree(&id))
            .collect();
        let sound_commands = self.retire_objects(&children);
        self.loaders.retain(|id, _| !retired.contains(id));
        sound_commands
    }
}
impl Player {
    pub fn pixel_decode(&mut self, encoded: &[u8]) -> Result<PixelRef, BitmapError> {
        let (w, h) = crate::resource_image::encoded_image_dimensions(encoded)
            .map_err(|e| BitmapError(e.to_string()))?;
        self.graph.pixels.reserve_import(w, h)?;
        let image = crate::resource_image::decode_encoded_image(encoded)
            .map_err(|e| BitmapError(e.to_string()))?;
        self.graph.pixels.import(&image)
    }
    /// For aborted, never-published loader payloads. Published BitmapData aliases
    /// must remain alive through AS3 unload and use explicit dispose/Restart.
    pub fn pixel_release(&mut self, reference: &PixelRef) -> Result<(), BitmapError> {
        if self
            .graph
            .bitmap_states
            .values()
            .any(|state| state.pixel_ref.as_ref() == Some(reference))
            || self
                .graph
                .resource_pixels
                .values()
                .any(|id| id == reference)
        {
            return Err(BitmapError(
                "cannot release pixels referenced by a display object".into(),
            ));
        }
        self.graph.pixels.release(reference)
    }
    pub fn loader_content(&self, target: &ObjectRef) -> Result<Option<ObjectRef>, DisplayError> {
        if !self.graph.alive(target) {
            return Err(error("Loader target is stale"));
        }
        Ok(self
            .graph
            .loaders
            .get(target)
            .and_then(|state| state.content.clone()))
    }
    pub fn loader_replace(
        &mut self,
        project: &Project,
        target: &ObjectRef,
        pixels: Option<PixelRef>,
        as2_replace: bool,
    ) -> Result<LoaderMutation, DisplayError> {
        self.loader_replace_with_payload(
            project,
            target,
            pixels.map(LoaderPayload::Bitmap),
            as2_replace,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn loader_replace_swf(
        &mut self,
        project: &Project,
        target: &ObjectRef,
        as_version: u32,
        width: u32,
        height: u32,
        bg_color: Option<u32>,
        as2_replace: bool,
    ) -> Result<LoaderMutation, DisplayError> {
        self.loader_replace_with_payload(
            project,
            target,
            Some(LoaderPayload::Swf {
                as_version,
                width,
                height,
                bg_color,
            }),
            as2_replace,
        )
    }

    fn loader_replace_with_payload(
        &mut self,
        project: &Project,
        target: &ObjectRef,
        payload: Option<LoaderPayload>,
        as2_replace: bool,
    ) -> Result<LoaderMutation, DisplayError> {
        if !self.graph.loaders.contains_key(target) && self.graph.loaders.len() >= 256 {
            return Err(error(
                "Loader exceeds 256 tracked targets; Restart releases the session",
            ));
        }
        let snapshot = self.runtime_snapshot(project);
        let node = snapshot
            .objects
            .iter()
            .find(|node| &node.object_ref == target)
            .ok_or_else(|| error("Loader target is stale"))?;
        if as2_replace {
            if node.display.owner != "movie_clip"
                || self.graph.paths.get(target).is_some_and(Vec::is_empty)
            {
                return Err(error(
                    "MovieClipLoader requires a non-root MovieClip target",
                ));
            }
            if self
                .graph
                .subtree(target)
                .iter()
                .any(|id| self.graph.parent_overrides.contains_key(id))
            {
                return Err(error(
                    "AS2 image replacement cannot retire reparented authored content; restore its source hierarchy before loading",
                ));
            }
        } else if self
            .graph
            .dynamic
            .get(target)
            .is_none_or(|node| node.kind != DynamicKind::Loader)
        {
            return Err(error("AS3 loading requires a runtime Loader container"));
        }
        if self
            .graph
            .route(target)
            .iter()
            .any(|id| self.graph.masks.values().any(|source| source == id))
        {
            return Err(error(
                "Loaded image content cannot replace a vector-mask source",
            ));
        }
        if let Some(LoaderPayload::Bitmap(reference)) = &payload {
            let info = self
                .graph
                .pixels
                .info(reference)
                .map_err(|e| error(&e.to_string()))?;
            if info.disposed {
                return Err(error("Loader image pixels are disposed"));
            }
        }
        let old = if as2_replace {
            self.graph.children(target)
        } else {
            self.graph
                .loaders
                .get(target)
                .and_then(|s| s.content.clone())
                .into_iter()
                .collect()
        };
        let descendants: Vec<_> = old.iter().flat_map(|id| self.graph.subtree(id)).collect();
        let retired: BTreeSet<_> = if as2_replace {
            descendants.iter().cloned().collect()
        } else {
            BTreeSet::new()
        };
        let released = retired
            .iter()
            .filter(|id| self.graph.dynamic.contains_key(*id))
            .count();
        if payload.is_some()
            && self.graph.dynamic.len().saturating_sub(released) >= MAX_DYNAMIC_OBJECTS
        {
            return Err(error("Loader exceeds dynamic display object budget"));
        }
        if payload.is_some() && self.graph.route(target).len() >= MAX_GRAPH_DEPTH {
            return Err(error("Loader content exceeds display depth budget"));
        }
        let attached = node.on_stage;
        let event_count = old.len()
            + if attached { descendants.len() } else { 0 }
            + if payload.is_some() {
                1 + usize::from(attached)
            } else {
                0
            };
        if event_count > MAX_DISPLAY_EVENTS
            || self.graph.events.len().saturating_add(event_count) > MAX_DISPLAY_EVENTS
        {
            return Err(error("Loader replacement exceeds lifecycle event budget"));
        }
        let swf_graphics = if let Some(LoaderPayload::Swf {
            width,
            height,
            bg_color,
            ..
        }) = &payload
        {
            let mut graphics = GraphicsState::default();
            let color = bg_color.unwrap_or(0x336699) & 0x00ff_ffff;
            graphics.apply(&crate::GraphicsOp::BeginFill { color, alpha: 1.0 });
            graphics.apply(&crate::GraphicsOp::DrawRect {
                x: 0.0,
                y: 0.0,
                width: ((*width).max(1)) as f64,
                height: ((*height).max(1)) as f64,
            });
            graphics.apply(&crate::GraphicsOp::EndFill);
            let released_graphics = retired
                .iter()
                .filter_map(|id| self.graph.dynamic.get(id).map(|n| n.graphics.cost()))
                .sum::<usize>()
                + if as2_replace {
                    self.graph
                        .dynamic
                        .get(target)
                        .map_or(0, |n| n.graphics.cost())
                } else {
                    0
                };
            let next_graphics_cost = self
                .graph
                .graphics_cost
                .saturating_sub(released_graphics)
                .checked_add(graphics.cost())
                .ok_or_else(|| error("Runtime Graphics storage exceeds 100000 commands/runs"))?;
            if graphics.cost() > MAX_GRAPHICS_OPS || next_graphics_cost > MAX_GRAPHICS_COMMANDS {
                return Err(error(
                    "Runtime Graphics storage exceeds 100000 commands/runs",
                ));
            }
            Some(graphics)
        } else {
            None
        };
        let mut events = vec![];
        for child in &old {
            events.push(DisplayEvent {
                kind: "removed".into(),
                target_ref: child.clone(),
                route: self.graph.route(child),
            });
        }
        if attached {
            for id in &descendants {
                events.push(DisplayEvent {
                    kind: "removed_from_stage".into(),
                    target_ref: id.clone(),
                    route: self.graph.route(id),
                });
            }
        }
        // All fallible checks precede mutation. Stop the original target before
        // suppressing its authored prefix; outer placement identity is retained.
        if as2_replace && !self.graph.loaded_as2(target) {
            self.object_control(project, target, PlaybackCommand::Stop);
        }
        if as2_replace {
            let retired_sound_commands = self.graph.retire_objects(&retired);
            self.sound.extend(retired_sound_commands);
            if let Some(mut namespace) = self.graph.namespaces.remove(target) {
                self.sound
                    .extend(namespace.player.stop_and_drain_scoped_sounds(target));
                self.sync_linked(project);
            }
            if let Some(node) = self.graph.dynamic.get_mut(target) {
                self.graph.graphics_cost -= node.graphics.cost();
                node.graphics = GraphicsState::default();
                node.playing = false;
            }
            self.graph.prefix.remove(target);
            self.graph.suffix.remove(target);
            self.graph.child_order.remove(target);
            if let Some(address) = self.graph.source_address(target) {
                self.graph
                    .namespaces
                    .get_mut(&address.namespace)
                    .expect("live namespace")
                    .player
                    .suppress_loaded_content(project, &address.path);
                self.sync_linked(project);
            } else if let Some(path) = self.graph.paths.get(target).cloned() {
                self.suppress_loaded_content(project, &path);
            }
        } else {
            for child in &old {
                if let Some(node) = self.graph.dynamic.get_mut(child) {
                    node.parent = None;
                }
            }
            self.graph.suffix.remove(target);
            self.graph.child_order.remove(target);
        }
        self.runtime_scripts.retain(|item| {
            !retired.contains(&item.target_ref) && (!as2_replace || &item.target_ref != target)
        });
        let content = match payload {
            Some(LoaderPayload::Bitmap(ref pixel_ref)) => {
                let id = self.graph.allocate();
                self.graph.dynamic.insert(
                    id.clone(),
                    DynamicObject {
                        kind: DynamicKind::Bitmap,
                        parent: Some(target.clone()),
                        transform: Transform::default(),
                        visible: true,
                        name: "content".into(),
                        playing: false,
                        graphics: GraphicsState::default(),
                    },
                );
                self.graph.bitmap_states.insert(
                    id.clone(),
                    bitmap::BitmapState {
                        pixel_ref: Some(pixel_ref.clone()),
                        smoothing: false,
                        pixel_snapping: PixelSnapping::Auto,
                    },
                );
                self.graph.suffix.insert(target.clone(), vec![id.clone()]);
                events.push(DisplayEvent {
                    kind: "added".into(),
                    target_ref: id.clone(),
                    route: self.graph.route(&id),
                });
                if attached {
                    events.push(DisplayEvent {
                        kind: "added_to_stage".into(),
                        target_ref: id.clone(),
                        route: self.graph.route(&id),
                    });
                }
                Some(id)
            }
            Some(LoaderPayload::Swf { as_version, .. }) => {
                let id = self.graph.allocate();
                let kind = if as_version < 3 {
                    DynamicKind::Avm1Movie
                } else {
                    DynamicKind::MovieClip
                };
                let graphics = swf_graphics.expect("validated SWF graphics");
                self.graph.graphics_cost += graphics.cost();
                self.graph.dynamic.insert(
                    id.clone(),
                    DynamicObject {
                        kind,
                        parent: Some(target.clone()),
                        transform: Transform::default(),
                        visible: true,
                        name: "content".into(),
                        playing: as_version >= 3,
                        graphics,
                    },
                );
                self.graph.suffix.insert(target.clone(), vec![id.clone()]);
                events.push(DisplayEvent {
                    kind: "added".into(),
                    target_ref: id.clone(),
                    route: self.graph.route(&id),
                });
                if attached {
                    events.push(DisplayEvent {
                        kind: "added_to_stage".into(),
                        target_ref: id.clone(),
                        route: self.graph.route(&id),
                    });
                }
                Some(id)
            }
            Some(LoaderPayload::PublishedClip {
                ref prefix,
                ref root_symbol_id,
                nodes,
                depth,
            }) => {
                let is_playing = self.is_playing();
                let scoped = Player::with_source_root_playing(
                    project,
                    Some(root_symbol_id.clone()),
                    is_playing,
                );
                if as2_replace {
                    if let Some(node) = self.graph.dynamic.get_mut(target) {
                        node.playing = is_playing;
                    }
                    self.graph.namespaces.insert(
                        target.clone(),
                        LinkedNamespace {
                            player: Box::new(scoped),
                            nodes,
                            depth,
                            mounted_prefix: Some(prefix.clone()),
                        },
                    );
                    self.sync_linked(project);
                    self.collect_linked_scripts(target);
                    Some(target.clone())
                } else {
                    let id = self.graph.allocate();
                    self.graph.dynamic.insert(
                        id.clone(),
                        DynamicObject {
                            kind: DynamicKind::MovieClip,
                            parent: Some(target.clone()),
                            transform: Transform::default(),
                            visible: true,
                            name: "content".into(),
                            playing: self.is_playing(),
                            graphics: GraphicsState::default(),
                        },
                    );
                    self.graph.namespaces.insert(
                        id.clone(),
                        LinkedNamespace {
                            player: Box::new(scoped),
                            nodes,
                            depth,
                            mounted_prefix: Some(prefix.clone()),
                        },
                    );
                    self.graph.suffix.insert(target.clone(), vec![id.clone()]);
                    events.push(DisplayEvent {
                        kind: "added".into(),
                        target_ref: id.clone(),
                        route: self.graph.route(&id),
                    });
                    if attached {
                        events.push(DisplayEvent {
                            kind: "added_to_stage".into(),
                            target_ref: id.clone(),
                            route: self.graph.route(&id),
                        });
                    }
                    self.sync_linked(project);
                    self.collect_linked_scripts(&id);
                    Some(id)
                }
            }
            None => None,
        };
        let mounted_prefix = match payload {
            Some(LoaderPayload::PublishedClip { prefix, .. }) => Some(prefix),
            _ => None,
        };
        self.graph.loaders.insert(
            target.clone(),
            LoaderState {
                content: content.clone(),
                as2: as2_replace,
                mounted_prefix,
            },
        );
        Ok(LoaderMutation {
            content_ref: content,
            events,
            retired_refs: retired.into_iter().collect(),
        })
    }

    /// Unload a loader target (`unloadMovie`, `MovieClipLoader.unloadClip`, `Loader.unload`,
    /// `Loader.unloadAndStop`), stopping any mounted published namespace and pruning its isolated
    /// symbols and resources from `project` (`T-248`).
    pub fn loader_unload_published(
        &mut self,
        project: &mut Project,
        target: &ObjectRef,
        as2_replace: bool,
        stop_detached: bool,
    ) -> Result<LoaderMutation, DisplayError> {
        let old_state = self.graph.loaders.get(target).cloned();
        if stop_detached
            && !as2_replace
            && let Some(content_id) = old_state.as_ref().and_then(|s| s.content.as_ref())
        {
            let subtree: BTreeSet<_> = self.graph.subtree(content_id).into_iter().collect();
            let stopped_sounds = self.graph.retire_objects(&subtree);
            self.sound.extend(stopped_sounds);
        }
        let mutation = self.loader_replace_with_payload(project, target, None, as2_replace)?;
        if let Some(prefix) = old_state.and_then(|s| s.mounted_prefix) {
            self.prune_unused_published_prefix(project, &prefix);
        }
        self.drain_retired_published_prefixes(project);
        Ok(mutation)
    }

    /// Prune any `__nfpub_<seq>_` namespaces whose owning display objects were retired by
    /// timeline navigation or `removeMovieClip` / `removeChild`.
    pub fn drain_retired_published_prefixes(&mut self, project: &mut Project) {
        let prefixes: Vec<String> = std::mem::take(&mut self.graph.retired_published_prefixes)
            .into_iter()
            .collect();
        for prefix in prefixes {
            self.prune_unused_published_prefix(project, &prefix);
        }
    }

    /// Hydrate decoded RGBA pixels for an externalized/sprite-sheet child bitmap resource (`T-248`).
    pub fn hydrate_mounted_resource_rgba(
        &mut self,
        resource_id: &str,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    ) -> Result<(), BitmapError> {
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|px| px.checked_mul(4))
            .ok_or_else(|| BitmapError("mounted bitmap dimensions overflow".into()))?;
        if rgba.len() != expected {
            return Err(BitmapError("mounted bitmap RGBA length mismatch".into()));
        }
        let image = crate::resource_image::DecodedImage {
            width,
            height,
            rgba: rgba.into(),
        };
        let pixel_ref = self.graph.pixels.import(&image)?;
        if let Some(old_ref) = self
            .graph
            .resource_pixels
            .insert(resource_id.to_owned(), pixel_ref)
        {
            let _ = self.graph.pixels.release(&old_ref);
        }
        self.graph
            .hydrated_bitmaps
            .insert(resource_id.to_owned(), image);
        Ok(())
    }

    fn prune_unused_published_prefix(&mut self, project: &mut Project, prefix: &str) {
        let still_mounted = self
            .graph
            .loaders
            .values()
            .any(|s| s.mounted_prefix.as_deref() == Some(prefix))
            || self
                .graph
                .namespaces
                .values()
                .any(|ns| ns.mounted_prefix.as_deref() == Some(prefix));
        if !still_mounted {
            project.symbols.retain(|s| !s.id.starts_with(prefix));
            project.resources.retain(|r| !r.id.starts_with(prefix));
            let stale_res_ids: Vec<Id> = self
                .graph
                .resource_pixels
                .keys()
                .filter(|id| id.starts_with(prefix))
                .cloned()
                .collect();
            for res_id in stale_res_ids {
                if let Some(pixel_ref) = self.graph.resource_pixels.remove(&res_id) {
                    let _ = self.graph.pixels.release(&pixel_ref);
                }
                self.graph.hydrated_bitmaps.remove(&res_id);
            }
        }
    }

    /// Dynamically mount another published NeoFlash `Project` inside `target` (`T-248` `loadPublish`).
    ///
    /// Flattens multi-scene child projects, allocates an isolated `__nfpub_<seq>_` namespace prefix,
    /// remaps all symbol/resource/layer/element IDs without mutating child instance `name`s, appends
    /// the isolated closure to `project`, and starts a nested `LinkedNamespace` timeline.
    pub fn loader_mount_project(
        &mut self,
        project: &mut Project,
        target: &ObjectRef,
        child_project: Project,
        as2_replace: bool,
    ) -> Result<MountedPublishResult, DisplayError> {
        self.drain_retired_published_prefixes(project);
        child_project
            .validate_published_bundle()
            .map_err(|e| error(&format!("Invalid published NeoFlash bundle: {e}")))?;
        let (flat_child, _) = crate::scenes::flatten_scenes_with_ranges(&child_project);

        let mount_seq = self.graph.next_id;
        self.graph.next_id += 1;
        let prefix = format!("__nfpub_{mount_seq}_");

        let mut symbol_id_map = BTreeMap::new();
        for (i, sym) in flat_child.symbols.iter().enumerate() {
            symbol_id_map.insert(sym.id.clone(), format!("{prefix}s{i}"));
        }
        let mut resource_id_map = BTreeMap::new();
        for (i, res) in flat_child.resources.iter().enumerate() {
            resource_id_map.insert(res.id.clone(), format!("{prefix}r{i}"));
        }

        if project.resources.len() + flat_child.resources.len() > crate::MAX_RESOURCES {
            return Err(error(
                "Mounting published bundle would exceed 1024 project resources",
            ));
        }

        let root_symbol_id = symbol_id_map
            .get(&flat_child.root)
            .cloned()
            .ok_or_else(|| error("Published bundle is missing its root symbol"))?;

        let mut resource_path_map = BTreeMap::new();
        let mut remapped_resources = Vec::with_capacity(flat_child.resources.len());
        for mut res in flat_child.resources {
            let new_id = resource_id_map[&res.id].clone();
            let orig_path = res.path.clone();
            let mounted_path = format!("__nfpub/{mount_seq}/{orig_path}");
            resource_path_map.insert(orig_path, mounted_path.clone());
            res.id = new_id;
            res.path = mounted_path;
            remapped_resources.push(res);
        }

        let mut remapped_symbols = Vec::with_capacity(flat_child.symbols.len());
        for (si, mut sym) in flat_child.symbols.into_iter().enumerate() {
            let is_child_root = sym.id == flat_child.root;
            sym.id = symbol_id_map[&sym.id].clone();
            if is_child_root {
                sym.kind = SymbolKind::MovieClip;
                sym.linkage_id = Some(format!("{prefix}root"));
                sym.class_name = None;
            } else if let Some(lid) = &sym.linkage_id {
                sym.linkage_id = Some(format!("{prefix}link_{lid}"));
            }

            let mut lid_seq = 0usize;
            let mut eid_seq = 0usize;
            sym.timeline.remap_internal_ids(
                |tag| {
                    if tag == "lyr" {
                        let next = format!("{prefix}s{si}_l{lid_seq}");
                        lid_seq += 1;
                        next
                    } else {
                        let next = format!("{prefix}s{si}_e{eid_seq}");
                        eid_seq += 1;
                        next
                    }
                },
                Some(&symbol_id_map),
                Some(&resource_id_map),
            );
            for layer in &mut sym.timeline.layers {
                layer.properties.groups.clear();
            }
            remapped_symbols.push(sym);
        }

        let referenced_child_bitmaps: BTreeSet<String> = remapped_symbols
            .iter()
            .flat_map(|s| &s.timeline.layers)
            .flat_map(|l| &l.keyframes)
            .flat_map(|k| &k.elements)
            .filter_map(|e| {
                if let ElementKind::Bitmap { resource_id, .. } = &e.kind {
                    Some(resource_id.clone())
                } else {
                    None
                }
            })
            .collect();
        let mut imported_child_pixels: Vec<(String, PixelRef)> = Vec::new();
        for res_id in &referenced_child_bitmaps {
            if let Some(res) = remapped_resources.iter().find(|r| &r.id == res_id)
                && res.is_image()
                && !res.data_base64.is_empty()
            {
                let decoded = match res.decode_image() {
                    Ok(img) => img,
                    Err(e) => {
                        for (_, pref) in imported_child_pixels {
                            let _ = self.graph.pixels.release(&pref);
                        }
                        return Err(error(&format!("Invalid mounted bitmap resource: {e}")));
                    }
                };
                match self.graph.pixels.import(&decoded) {
                    Ok(pref) => imported_child_pixels.push((res_id.clone(), pref)),
                    Err(e) => {
                        for (_, pref) in imported_child_pixels {
                            let _ = self.graph.pixels.release(&pref);
                        }
                        return Err(error(&e.to_string()));
                    }
                }
            }
        }
        for (res_id, pref) in &imported_child_pixels {
            self.graph
                .resource_pixels
                .insert(res_id.clone(), pref.clone());
        }

        let old_prefix = self
            .graph
            .loaders
            .get(target)
            .and_then(|s| s.mounted_prefix.clone());

        let prev_format_version = project.format_version;
        let prev_sym_len = project.symbols.len();
        let prev_res_len = project.resources.len();
        project.format_version = project.format_version.max(flat_child.format_version).max(2);
        project.symbols.extend(remapped_symbols);
        project.resources.extend(remapped_resources);

        let budget_res = super::linked::symbol_budget(
            project,
            &root_symbol_id,
            &mut BTreeSet::new(),
            &mut HashMap::new(),
        );
        let (nodes, depth) = match budget_res {
            Ok(v) => v,
            Err(err) => {
                for (res_id, pref) in imported_child_pixels {
                    self.graph.resource_pixels.remove(&res_id);
                    let _ = self.graph.pixels.release(&pref);
                }
                project.format_version = prev_format_version;
                project.symbols.truncate(prev_sym_len);
                project.resources.truncate(prev_res_len);
                return Err(err);
            }
        };
        if nodes
            + self
                .graph
                .namespaces
                .values()
                .map(|ns| ns.nodes)
                .sum::<usize>()
            > 100_000
        {
            for (res_id, pref) in imported_child_pixels {
                self.graph.resource_pixels.remove(&res_id);
                let _ = self.graph.pixels.release(&pref);
            }
            project.format_version = prev_format_version;
            project.symbols.truncate(prev_sym_len);
            project.resources.truncate(prev_res_len);
            return Err(error(
                "Mounted published timelines exceed 100000 reserved objects",
            ));
        }

        let mutation = match self.loader_replace_with_payload(
            project,
            target,
            Some(LoaderPayload::PublishedClip {
                prefix: prefix.clone(),
                root_symbol_id: root_symbol_id.clone(),
                nodes,
                depth,
            }),
            as2_replace,
        ) {
            Ok(m) => m,
            Err(err) => {
                for (res_id, pref) in imported_child_pixels {
                    self.graph.resource_pixels.remove(&res_id);
                    let _ = self.graph.pixels.release(&pref);
                }
                project.format_version = prev_format_version;
                project.symbols.truncate(prev_sym_len);
                project.resources.truncate(prev_res_len);
                return Err(err);
            }
        };

        if let Some(old_p) = old_prefix {
            self.prune_unused_published_prefix(project, &old_p);
        }
        self.drain_retired_published_prefixes(project);

        Ok(MountedPublishResult {
            mutation,
            prefix,
            root_symbol_id,
            symbol_id_map,
            resource_id_map,
            resource_path_map,
            width: flat_child.width,
            height: flat_child.height,
            fps: flat_child.fps,
        })
    }
}

enum LoaderPayload {
    Bitmap(PixelRef),
    Swf {
        as_version: u32,
        width: u32,
        height: u32,
        bg_color: Option<u32>,
    },
    PublishedClip {
        prefix: String,
        root_symbol_id: String,
        nodes: usize,
        depth: usize,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linked_stream_project() -> Project {
        let mut project = Project::blank();
        project.fps = 10;
        project.format_version = 6;
        project.resources.push(
            ResourceUnit::from_bytes("song", "audio/song.mp3", "audio/mpeg", b"ID3").unwrap(),
        );
        project.symbols.push(Symbol {
            id: "linked-sound".into(),
            name: "Linked sound".into(),
            folder: String::new(),
            kind: SymbolKind::MovieClip,
            timeline: Timeline {
                frame_count: 4,
                layers: vec![Layer {
                    id: "music".into(),
                    name: "music".into(),
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe {
                        sound: Some(FrameSound {
                            sync: SoundSync::Stream,
                            ..FrameSound::new("song")
                        }),
                        ..Keyframe::default()
                    }],
                    properties: LayerProperties::default(),
                }],
                scripts: vec![],
                guides: vec![],
            },
            class_name: None,
            linkage_id: Some("LinkedSound".into()),
        });
        project.validate().unwrap();
        project
    }

    fn stream_start_channels(commands: &[SoundCommand]) -> Vec<String> {
        let mut channels: Vec<_> = commands
            .iter()
            .filter_map(|command| match command {
                SoundCommand::StreamStart { channel, .. } => Some(channel.clone()),
                _ => None,
            })
            .collect();
        channels.sort();
        channels
    }

    fn stream_stop_channels(commands: &[SoundCommand]) -> Vec<String> {
        let mut channels: Vec<_> = commands
            .iter()
            .filter_map(|command| match command {
                SoundCommand::StreamStop { channel } => Some(channel.clone()),
                _ => None,
            })
            .collect();
        channels.sort();
        channels
    }

    #[test]
    fn loader_replace_swf_enforces_dynamic_depth_graphics_and_event_budgets() {
        let project = Project::blank();
        let mut player = Player::new(&project);
        let loader = player.create_display(DynamicKind::Loader).unwrap();

        // 1. Graphics budget check
        player.graph.graphics_cost = MAX_GRAPHICS_COMMANDS;
        let err = player
            .loader_replace_swf(&project, &loader, 3, 100, 100, None, false)
            .unwrap_err();
        assert!(err.0.contains("100000"));
        assert!(player.loader_content(&loader).unwrap().is_none());
        player.graph.graphics_cost = 0;

        // 2. Pending display events budget check
        player.graph.events = vec![
            DisplayEvent {
                kind: "added".into(),
                target_ref: loader.clone(),
                route: vec![loader.clone()],
            };
            MAX_DISPLAY_EVENTS
        ];
        let err = player
            .loader_replace_swf(&project, &loader, 3, 100, 100, None, false)
            .unwrap_err();
        assert!(err.0.contains("lifecycle event budget"));
        player.graph.events.clear();

        // 3. Dynamic display object budget check
        while player.graph.dynamic.len() < MAX_DYNAMIC_OBJECTS {
            player.create_display(DynamicKind::Sprite).unwrap();
        }
        let err = player
            .loader_replace_swf(&project, &loader, 3, 100, 100, None, false)
            .unwrap_err();
        assert!(err.0.contains("dynamic display object budget"));
    }

    #[test]
    fn loader_replacement_recursively_stops_nested_linked_streams() {
        let project = linked_stream_project();
        let mut player = Player::new(&project);
        let target = player.create_display(DynamicKind::MovieClip).unwrap();
        let outer = player.create_linked(&project, "LinkedSound").unwrap();
        player
            .graph
            .namespaces
            .get_mut(&outer)
            .unwrap()
            .player
            .create_linked(&project, "LinkedSound")
            .unwrap();
        player
            .display_mutate(
                &project,
                DisplayMutation::Add {
                    parent: target.clone(),
                    child: outer.clone(),
                    index: None,
                },
            )
            .unwrap();

        let starts = stream_start_channels(&player.drain_sound_commands());
        assert_eq!(starts.len(), 2);
        assert_ne!(starts[0], starts[1]);

        player
            .loader_replace(&project, &target, None, true)
            .unwrap();
        let stops = stream_stop_channels(&player.drain_sound_commands());
        assert_eq!(stops, starts);
        assert!(!player.object_alive(&outer));
    }

    #[test]
    fn loader_mount_project_isolates_ids_steps_timeline_and_prunes_on_unload() {
        let mut host_project = Project::blank();
        let initial_syms = host_project.symbols.len();
        let initial_res = host_project.resources.len();

        let child_project = linked_stream_project();
        let mut player = Player::new(&host_project);
        let holder = player.create_display(DynamicKind::MovieClip).unwrap();

        let mounted = player
            .loader_mount_project(&mut host_project, &holder, child_project, true)
            .expect("mount published child project");
        assert!(mounted.prefix.starts_with("__nfpub_"));
        assert!(host_project.symbols.len() > initial_syms);
        assert!(host_project.resources.len() > initial_res);
        host_project
            .validate()
            .expect("merged host project validates cleanly");

        let content_ref = mounted.mutation.content_ref.expect("mounted content clip");
        assert_eq!(content_ref, holder);
        assert!(player.object_alive(&holder));

        player
            .loader_unload_published(&mut host_project, &holder, true, true)
            .expect("unload published child project");
        assert!(player.object_alive(&holder));
        assert_eq!(player.object_frame_count(&host_project, &holder), Some(1));
        assert_eq!(host_project.symbols.len(), initial_syms);
        assert_eq!(host_project.resources.len(), initial_res);

        // Also verify AS3 Loader (`as2_replace = false`) detaches & retires `content_ref`
        let loader_obj = player.create_display(DynamicKind::Loader).unwrap();
        let mounted_as3 = player
            .loader_mount_project(
                &mut host_project,
                &loader_obj,
                linked_stream_project(),
                false,
            )
            .expect("mount published child project into AS3 Loader");
        let as3_content = mounted_as3
            .mutation
            .content_ref
            .expect("AS3 Loader content child");
        assert_ne!(as3_content, loader_obj);
        assert!(player.object_alive(&as3_content));
        player
            .loader_unload_published(&mut host_project, &loader_obj, false, true)
            .expect("unloadAndStop AS3 Loader");
        assert!(!player.object_alive(&as3_content));
        assert_eq!(host_project.symbols.len(), initial_syms);
        assert_eq!(host_project.resources.len(), initial_res);
    }
}
