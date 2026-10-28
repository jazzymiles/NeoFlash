//! Image Loader ownership and bounded AS2 content replacement. Encoded bytes
//! cross the host boundary once; decoding and premultiplied pixels stay in Rust.
use super::*;

#[derive(Clone, Debug)]
pub(super) struct LoaderState {
    content: Option<ObjectRef>,
    as2: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoaderMutation {
    pub content_ref: Option<ObjectRef>,
    pub events: Vec<DisplayEvent>,
    pub retired_refs: Vec<ObjectRef>,
}
impl RuntimeGraph {
    pub(super) fn loaded_as2(&self, reference: &ObjectRef) -> bool {
        self.loaders.get(reference).is_some_and(|state| state.as2)
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
            || self.loaders.values().any(|s| {
                s.content
                    .as_ref()
                    .is_some_and(|id| id == first || Some(id) == second)
            })
        {
            return Err(error(
                "Loader owns its content child; use load/unload instead of child-list mutation",
            ));
        }
        Ok(())
    }
    fn retire_objects(&mut self, retired: &BTreeSet<ObjectRef>) {
        for reference in retired {
            if let Some(node) = self.dynamic.remove(reference) {
                self.graphics_cost -= node.graphics.cost();
            }
            self.namespaces.remove(reference);
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
    }
    pub(super) fn retire_loader_children(&mut self, retired: &BTreeSet<ObjectRef>) {
        let children: BTreeSet<_> = retired
            .iter()
            .filter_map(|id| self.loaders.get(id).and_then(|s| s.content.clone()))
            .flat_map(|id| self.subtree(&id))
            .collect();
        self.retire_objects(&children);
        self.loaders.retain(|id, _| !retired.contains(id));
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
        if let Some(reference) = &pixels {
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
        if pixels.is_some() && self.graph.dynamic.len() - released >= MAX_DYNAMIC_OBJECTS {
            return Err(error("Loader exceeds dynamic display object budget"));
        }
        if pixels.is_some() && self.graph.route(target).len() >= MAX_GRAPH_DEPTH {
            return Err(error("Loader content exceeds display depth budget"));
        }
        let attached = node.on_stage;
        let event_count = old.len()
            + if attached { descendants.len() } else { 0 }
            + if pixels.is_some() {
                1 + usize::from(attached)
            } else {
                0
            };
        if event_count > MAX_DISPLAY_EVENTS {
            return Err(error("Loader replacement exceeds lifecycle event budget"));
        }
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
            self.graph.retire_objects(&retired);
            if let Some(node) = self.graph.dynamic.get_mut(target) {
                self.graph.graphics_cost -= node.graphics.cost();
                node.graphics = GraphicsState::default();
                node.playing = false;
            }
            self.graph.prefix.remove(target);
            self.graph.suffix.remove(target);
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
        }
        let content = pixels.map(|pixel_ref| {
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
                    pixel_ref: Some(pixel_ref),
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
            id
        });
        self.graph.loaders.insert(
            target.clone(),
            LoaderState {
                content: content.clone(),
                as2: as2_replace,
            },
        );
        self.runtime_scripts.retain(|item| {
            !retired.contains(&item.target_ref) && (!as2_replace || &item.target_ref != target)
        });
        Ok(LoaderMutation {
            content_ref: content,
            events,
            retired_refs: retired.into_iter().collect(),
        })
    }
}
