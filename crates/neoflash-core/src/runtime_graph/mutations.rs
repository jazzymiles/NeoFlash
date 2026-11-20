//! Display-list hierarchy mutations, stage children mutations, graphics commands, and display properties.
use super::*;

impl Player {
    pub fn display_mutate(
        &mut self,
        project: &Project,
        mutation: DisplayMutation,
    ) -> Result<MutationResult, DisplayError> {
        let (parent, child) = match &mutation {
            DisplayMutation::Add { parent, child, .. }
            | DisplayMutation::Remove { parent, child }
            | DisplayMutation::SetIndex { parent, child, .. } => (parent, child),
            DisplayMutation::Swap { parent, first, .. } => (parent, first),
        };
        if !self.graph.alive(parent) || !self.graph.alive(child) {
            return Err(error(
                "Display reference is stale or belongs to another player",
            ));
        }
        self.graph.validate_loader_mutation(&mutation)?;
        let owner = self.object_owner(project, parent).expect("live parent");
        if !matches!(
            owner,
            "sprite" | "movie_clip" | "graphic" | "button" | "text_line"
        ) {
            return Err(error("This object cannot contain children"));
        }
        if self.graph.paths.get(child).is_some_and(Vec::is_empty) {
            return Err(error("The source root cannot be moved or removed"));
        }
        self.validate_authored_hierarchy_change(project, child)?;
        let current = self.graph.children(parent);
        let mut result = MutationResult::default();
        match mutation {
            DisplayMutation::Add {
                parent,
                child,
                index,
            } => {
                let old_parent = self.graph.parent(&child);
                let same = old_parent.as_ref() == Some(&parent);
                let length = current.len();
                let index = index.unwrap_or(if same {
                    length.saturating_sub(1)
                } else {
                    length
                });
                let maximum = if same {
                    length.saturating_sub(1)
                } else {
                    length
                };
                if index > maximum {
                    return Err(error("Child index is outside the container"));
                }
                let subtree = self.graph.subtree(&child);
                if subtree.contains(&parent) {
                    return Err(error("Display reparenting would create a cycle"));
                }
                let depth = self.graph.route(&parent).len();
                let height = subtree
                    .iter()
                    .map(|id| {
                        self.graph.route(id).len() - self.graph.route(&child).len()
                            + self.graph.namespaces.get(id).map_or(1, |scope| scope.depth)
                    })
                    .max()
                    .unwrap_or(1);
                if depth + height > MAX_GRAPH_DEPTH {
                    return Err(error("Display graph nesting exceeds 64 objects"));
                }
                if !self.graph.masks.is_empty() || !self.graph.scroll_rects.is_empty() {
                    let snapshot = self.runtime_snapshot(project);
                    self.graph.validate_clipping(
                        project,
                        &snapshot,
                        &self.graph.masks,
                        &self.graph.scroll_rects,
                        Some((&child, &parent)),
                    )?;
                }
                if same {
                    let mut siblings = current;
                    let old = siblings
                        .iter()
                        .position(|id| id == &child)
                        .expect("same parent");
                    siblings.remove(old);
                    siblings.insert(index, child);
                    self.graph.set_child_order(&parent, siblings);
                    return Ok(result);
                }
                let was_stage_child = self.graph.stage_order.contains(&child);
                let was_stage = self.graph.on_stage(&child);
                let will_stage = self.graph.on_stage(&parent);
                let count = usize::from(old_parent.is_some() || was_stage_child)
                    + 1
                    + if was_stage != will_stage {
                        subtree.len()
                    } else {
                        0
                    };
                if count > MAX_DISPLAY_EVENTS {
                    return Err(error("Display mutation exceeds its lifecycle event budget"));
                }
                if old_parent.is_some() || was_stage_child {
                    result.events.push(DisplayEvent {
                        kind: "removed".into(),
                        stage_route: was_stage,
                        target_ref: child.clone(),
                        route: self.graph.route(&child),
                    });
                }
                if was_stage && !will_stage {
                    for id in &subtree {
                        result.events.push(DisplayEvent {
                            kind: "removed_from_stage".into(),
                            stage_route: true,
                            target_ref: id.clone(),
                            route: self.graph.route(id),
                        });
                    }
                }
                self.graph.attach_child(&parent, &child, index);
                result.events.push(DisplayEvent {
                    kind: "added".into(),
                    stage_route: will_stage,
                    target_ref: child.clone(),
                    route: self.graph.route(&child),
                });
                if !was_stage && will_stage {
                    for id in subtree {
                        result.events.push(DisplayEvent {
                            kind: "added_to_stage".into(),
                            stage_route: true,
                            target_ref: id.clone(),
                            route: self.graph.route(&id),
                        });
                    }
                }
            }
            DisplayMutation::Remove { parent, child } => {
                if self.graph.parent(&child).as_ref() != Some(&parent) {
                    return Err(error("Object is not a child of this container"));
                }
                let subtree = self.graph.subtree(&child);
                let attached = self.graph.on_stage(&child);
                if 1 + if attached { subtree.len() } else { 0 } > MAX_DISPLAY_EVENTS {
                    return Err(error("Display mutation exceeds its lifecycle event budget"));
                }
                result.events.push(DisplayEvent {
                    kind: "removed".into(),
                    stage_route: attached,
                    target_ref: child.clone(),
                    route: self.graph.route(&child),
                });
                if attached {
                    for id in subtree {
                        result.events.push(DisplayEvent {
                            kind: "removed_from_stage".into(),
                            stage_route: true,
                            target_ref: id.clone(),
                            route: self.graph.route(&id),
                        });
                    }
                }
                self.graph.detach_child(&child);
            }
            DisplayMutation::SetIndex {
                parent,
                child,
                index,
            } => {
                let position = current
                    .iter()
                    .position(|id| id == &child)
                    .ok_or_else(|| error("Object is not a child of this container"))?;
                if index >= current.len() {
                    return Err(error("Child index is outside the container"));
                }
                let mut siblings = current;
                siblings.remove(position);
                siblings.insert(index, child);
                self.graph.set_child_order(&parent, siblings);
            }
            DisplayMutation::Swap {
                parent,
                first,
                second,
            } => {
                if !self.graph.alive(&second) {
                    return Err(error(
                        "Display reference is stale or belongs to another player",
                    ));
                }
                self.validate_authored_hierarchy_change(project, &second)?;
                let a = current
                    .iter()
                    .position(|id| id == &first)
                    .ok_or_else(|| error("First object is not a child"))?;
                let b = current
                    .iter()
                    .position(|id| id == &second)
                    .ok_or_else(|| error("Second object is not a child"))?;
                let mut siblings = current;
                siblings.swap(a, b);
                self.graph.set_child_order(&parent, siblings);
            }
        }
        Ok(result)
    }

    pub fn stage_children(&self) -> Vec<ObjectRef> {
        self.graph.stage_children()
    }

    pub fn stage_mutate(
        &mut self,
        project: &Project,
        mutation: StageMutation,
    ) -> Result<MutationResult, DisplayError> {
        let child = match &mutation {
            StageMutation::Add { child, .. }
            | StageMutation::Remove { child }
            | StageMutation::SetIndex { child, .. } => child,
            StageMutation::Swap { first, .. } => first,
        };
        if !self.graph.alive(child) {
            return Err(error(
                "Display reference is stale or belongs to another player",
            ));
        }
        if self
            .graph
            .loaders
            .values()
            .any(|s| s.content.as_ref() == Some(child))
        {
            return Err(error(
                "Loader owns its content child; use load/unload instead of child-list mutation",
            ));
        }
        if !self.graph.paths.get(child).is_some_and(Vec::is_empty) {
            self.validate_authored_hierarchy_change(project, child)?;
        }
        let current = self.graph.stage_children();
        let mut result = MutationResult::default();
        match mutation {
            StageMutation::Add { child, index } => {
                let same = current.contains(&child);
                let length = current.len();
                let maximum = if same {
                    length.saturating_sub(1)
                } else {
                    length
                };
                let index = index.unwrap_or(maximum);
                if index > maximum {
                    return Err(error("Child index is outside the container"));
                }
                if same {
                    let mut siblings = current;
                    let old = siblings
                        .iter()
                        .position(|id| id == &child)
                        .expect("same stage");
                    siblings.remove(old);
                    siblings.insert(index, child);
                    self.graph.stage_order = siblings;
                    return Ok(result);
                }
                let subtree = self.graph.subtree(&child);
                let height = subtree
                    .iter()
                    .map(|id| {
                        self.graph.route(id).len() - self.graph.route(&child).len()
                            + self.graph.namespaces.get(id).map_or(1, |scope| scope.depth)
                    })
                    .max()
                    .unwrap_or(1);
                if height > MAX_GRAPH_DEPTH {
                    return Err(error("Display graph nesting exceeds 64 objects"));
                }
                let old_parent = self.graph.parent(&child);
                let was_stage = self.graph.on_stage(&child);
                let count = usize::from(old_parent.is_some())
                    + 1
                    + if !was_stage { subtree.len() } else { 0 };
                if count > MAX_DISPLAY_EVENTS {
                    return Err(error("Display mutation exceeds its lifecycle event budget"));
                }
                if old_parent.is_some() {
                    result.events.push(DisplayEvent {
                        kind: "removed".into(),
                        stage_route: was_stage,
                        target_ref: child.clone(),
                        route: self.graph.route(&child),
                    });
                }
                self.graph.detach_child(&child);
                let mut siblings = self.graph.stage_children();
                siblings.insert(index, child.clone());
                self.graph.stage_order = siblings;
                result.events.push(DisplayEvent {
                    kind: "added".into(),
                    stage_route: true,
                    target_ref: child.clone(),
                    route: vec![child.clone()],
                });
                if !was_stage {
                    for id in subtree {
                        result.events.push(DisplayEvent {
                            kind: "added_to_stage".into(),
                            stage_route: true,
                            target_ref: id.clone(),
                            route: self.graph.route(&id),
                        });
                    }
                }
            }
            StageMutation::Remove { child } => {
                if self.graph.paths.get(&child).is_some_and(Vec::is_empty) {
                    return Err(error("The source root cannot be moved or removed"));
                }
                if !current.contains(&child) {
                    return Err(error("Object is not a child of this container"));
                }
                let subtree = self.graph.subtree(&child);
                if 1 + subtree.len() > MAX_DISPLAY_EVENTS {
                    return Err(error("Display mutation exceeds its lifecycle event budget"));
                }
                result.events.push(DisplayEvent {
                    kind: "removed".into(),
                    stage_route: true,
                    target_ref: child.clone(),
                    route: vec![child.clone()],
                });
                for id in subtree {
                    result.events.push(DisplayEvent {
                        kind: "removed_from_stage".into(),
                        stage_route: true,
                        target_ref: id.clone(),
                        route: self.graph.route(&id),
                    });
                }
                let mut siblings = current;
                siblings.retain(|id| id != &child);
                self.graph.stage_order = siblings;
                self.graph.detach_child(&child);
            }
            StageMutation::SetIndex { child, index } => {
                let position = current
                    .iter()
                    .position(|id| id == &child)
                    .ok_or_else(|| error("Object is not a child of this container"))?;
                if index >= current.len() {
                    return Err(error("Child index is outside the container"));
                }
                let mut siblings = current;
                siblings.remove(position);
                siblings.insert(index, child);
                self.graph.stage_order = siblings;
            }
            StageMutation::Swap { first, second } => {
                if !self.graph.alive(&second) {
                    return Err(error(
                        "Display reference is stale or belongs to another player",
                    ));
                }
                if self
                    .graph
                    .loaders
                    .values()
                    .any(|s| s.content.as_ref() == Some(&second))
                {
                    return Err(error(
                        "Loader owns its content child; use load/unload instead of child-list mutation",
                    ));
                }
                if !self.graph.paths.get(&second).is_some_and(Vec::is_empty) {
                    self.validate_authored_hierarchy_change(project, &second)?;
                }
                let a = current
                    .iter()
                    .position(|id| id == &first)
                    .ok_or_else(|| error("First object is not a child"))?;
                let b = current
                    .iter()
                    .position(|id| id == &second)
                    .ok_or_else(|| error("Second object is not a child"))?;
                let mut siblings = current;
                siblings.swap(a, b);
                self.graph.stage_order = siblings;
            }
        }
        Ok(result)
    }

    /// Detach an inclusive child range after validating the complete request.
    /// This bounded active-source-lifetime profile delivers lifecycle events after the
    /// batch; native reentrant per-child removal callbacks are not modeled.
    pub fn object_remove_children(
        &mut self,
        project: &Project,
        parent: &ObjectRef,
        begin: i32,
        end: i32,
    ) -> Result<MutationResult, DisplayError> {
        if !self.graph.alive(parent) {
            return Err(error(
                "Display reference is stale or belongs to another player",
            ));
        }
        let owner = self
            .object_owner(project, parent)
            .ok_or_else(|| error("Container is absent from the display graph"))?;
        if owner == "loader" {
            return Err(error(
                "Loader owns its content child; use load/unload instead of child-list mutation",
            ));
        }
        if !matches!(owner, "sprite" | "movie_clip" | "graphic" | "text_line") {
            return Err(error("removeChildren requires a DisplayObjectContainer"));
        }
        let children = self.graph.children(parent);
        if begin < 0
            || end < 0
            || begin > end
            || (end != i32::MAX
                && (begin as usize >= children.len() || end as usize >= children.len()))
        {
            return Err(error(
                "removeChildren index range is outside the child list",
            ));
        }
        // The default maximum means through the last child, including an empty
        // list or a begin position beyond its end (pinned Ruffle remove_children).
        let selected: Vec<_> = children
            .into_iter()
            .take(end as usize + 1)
            .skip(begin as usize)
            .collect();
        let mut result = MutationResult::default();
        for child in &selected {
            self.validate_authored_hierarchy_change(project, child)?;
            self.graph
                .validate_loader_mutation(&DisplayMutation::Remove {
                    parent: parent.clone(),
                    child: child.clone(),
                })?;
            let subtree = self.graph.subtree(child);
            let attached = self.graph.on_stage(child);
            let cost = 1 + if attached { subtree.len() } else { 0 };
            if result.events.len().saturating_add(cost) > MAX_DISPLAY_EVENTS {
                return Err(error("Display mutation exceeds its lifecycle event budget"));
            }
            result.events.push(DisplayEvent {
                kind: "removed".into(),
                stage_route: attached,
                target_ref: child.clone(),
                route: self.graph.route(child),
            });
            if attached {
                for reference in subtree {
                    result.events.push(DisplayEvent {
                        kind: "removed_from_stage".into(),
                        stage_route: true,
                        route: self.graph.route(&reference),
                        target_ref: reference,
                    });
                }
            }
        }
        for child in selected {
            self.graph.detach_child(&child);
        }
        Ok(result)
    }

    /// Stop this container's local MovieClip clocks and those in its current
    /// subtree, including detached linked subtrees. Media playback is separate.
    pub fn object_stop_all_movie_clips(
        &mut self,
        project: &Project,
        target: &ObjectRef,
    ) -> Result<(), DisplayError> {
        if !self.graph.alive(target) {
            return Err(error(
                "Display reference is stale or belongs to another player",
            ));
        }
        let owner = self
            .object_owner(project, target)
            .ok_or_else(|| error("Container is absent from the display graph"))?;
        if !matches!(owner, "sprite" | "movie_clip" | "graphic" | "loader") {
            return Err(error("stopAllMovieClips requires a DisplayObjectContainer"));
        }
        // The receiver is part of subtree(). Source MovieClips in synchronized
        // authoring modes have no independent clock; object_control leaves them
        // to their parent timeline instead of inventing playback state.
        for reference in self.graph.subtree(target) {
            if self.object_owner(project, &reference) == Some("movie_clip")
                && !self.graph.loaded_as2(&reference)
            {
                self.object_control(project, &reference, PlaybackCommand::Stop);
            }
        }
        Ok(())
    }

    pub fn graphics_apply(
        &mut self,
        reference: &ObjectRef,
        operations: &[GraphicsOp],
    ) -> Result<(), DisplayError> {
        let object = self.graph.dynamic.get(reference).ok_or_else(|| {
            error("Graphics currently requires a live runtime-created Sprite, Shape or MovieClip")
        })?;
        if matches!(
            object.kind,
            DynamicKind::Bitmap
                | DynamicKind::TextField
                | DynamicKind::Video
                | DynamicKind::Loader
                | DynamicKind::Button
                | DynamicKind::TextLine
                | DynamicKind::MorphShape
                | DynamicKind::Avm1Movie
        ) {
            return Err(error(
                "Bitmap, Video, TextField, Loader and SimpleButton do not own Graphics",
            ));
        }
        if operations.len() > MAX_GRAPHICS_OPS {
            return Err(error("Graphics batch exceeds 4096 operations"));
        }
        let old = object.graphics.cost();
        let mut reservation = old;
        let mut has_stroke = object.graphics.has_stroke();
        for op in operations {
            op.validate().map_err(error)?;
            match op {
                GraphicsOp::Clear => has_stroke = false,
                GraphicsOp::LineStyle { width, .. } => has_stroke = width.is_some(),
                GraphicsOp::BeginBitmapFill { bitmap } => {
                    let info = self
                        .graph
                        .pixels
                        .info(&bitmap.pixel_ref)
                        .map_err(|e| error(&e.to_string()))?;
                    if info.disposed {
                        return Err(error(
                            "beginBitmapFill requires a live non-disposed BitmapData",
                        ));
                    }
                }
                GraphicsOp::LineGradientStyle { .. } if !has_stroke => {
                    return Err(error(
                        "lineGradientStyle requires an active positive-width lineStyle",
                    ));
                }
                _ => {}
            }
            if matches!(op, GraphicsOp::Clear) {
                reservation = 0;
            } else {
                reservation = reservation.saturating_add(op.reservation());
            }
            if self.graph.graphics_cost - old + reservation > MAX_GRAPHICS_COMMANDS {
                return Err(error(
                    "Runtime Graphics storage exceeds 100000 commands/runs",
                ));
            }
        }
        let object = self
            .graph
            .dynamic
            .get_mut(reference)
            .expect("validated object");
        for op in operations {
            object.graphics.apply(op);
        }
        self.graph.graphics_cost = self.graph.graphics_cost - old + object.graphics.cost();
        Ok(())
    }

    pub fn set_morph_shape_ops(
        &mut self,
        reference: &ObjectRef,
        operations: &[GraphicsOp],
    ) -> Result<(), DisplayError> {
        let old = {
            let object = self
                .graph
                .dynamic
                .get(reference)
                .filter(|node| node.kind == DynamicKind::MorphShape)
                .ok_or_else(|| error("MorphShape operations require a live runtime MorphShape"))?;
            for op in operations {
                op.validate().map_err(error)?;
            }
            object.graphics.cost()
        };
        let object = self
            .graph
            .dynamic
            .get_mut(reference)
            .expect("validated MorphShape");
        object.graphics = GraphicsState::default();
        for op in operations {
            object.graphics.apply(op);
        }
        self.graph.graphics_cost = self.graph.graphics_cost - old + object.graphics.cost();
        Ok(())
    }

    /// Snapshot all Graphics state, including active styles and the pen. The
    /// destination subsequently evolves independently; display properties do not copy.
    pub fn graphics_copy_from(
        &mut self,
        target: &ObjectRef,
        source: &ObjectRef,
    ) -> Result<(), DisplayError> {
        let valid = |node: &DynamicObject| {
            !matches!(
                node.kind,
                DynamicKind::Bitmap
                    | DynamicKind::TextField
                    | DynamicKind::Video
                    | DynamicKind::Loader
                    | DynamicKind::Button
                    | DynamicKind::TextLine
                    | DynamicKind::MorphShape
                    | DynamicKind::Avm1Movie
            )
        };
        let destination = self
            .graph
            .dynamic
            .get(target)
            .filter(|n| valid(n))
            .ok_or_else(|| {
                error("Graphics copy destination must be a live runtime Sprite, Shape or MovieClip")
            })?;
        let source = self
            .graph
            .dynamic
            .get(source)
            .filter(|n| valid(n))
            .ok_or_else(|| {
                error("Graphics copy source must be a live runtime Sprite, Shape or MovieClip")
            })?;
        let old = destination.graphics.cost();
        let next = source.graphics.cost();
        if self.graph.graphics_cost - old + next > MAX_GRAPHICS_COMMANDS {
            return Err(error(
                "Graphics copy exceeds the publication storage budget",
            ));
        }
        let copy = source.graphics.clone();
        self.graph
            .dynamic
            .get_mut(target)
            .expect("validated destination")
            .graphics = copy;
        self.graph.graphics_cost = self.graph.graphics_cost - old + next;
        Ok(())
    }

    pub fn object_set_property(&mut self, reference: &ObjectRef, name: &str, value: f64) -> bool {
        let valid = value.is_finite()
            && match name {
                "x" | "y" => value.abs() <= 1e12,
                "scale_x" | "scale_y" => value.abs() <= 1e4,
                "rotation" => value.abs() <= 1e9,
                "alpha" => (0.0..=1.0).contains(&value),
                _ => false,
            };
        if !valid {
            return false;
        }
        if let Some(result) = self.set_transform_property_override(reference, name, value) {
            return result;
        }
        if let Some(address) = self.graph.linked_paths.get(reference).cloned() {
            return self
                .graph
                .namespaces
                .get_mut(&address.namespace)
                .expect("namespace")
                .player
                .set_property(&address.path, name, value);
        }
        if let Some(path) = self.graph.paths.get(reference).cloned() {
            return self.set_property(&path, name, value);
        }
        let Some(object) = self.graph.dynamic.get_mut(reference) else {
            return false;
        };
        if !value.is_finite() {
            return false;
        }
        match name {
            "x" if value.abs() <= 1e12 => object.transform.x = value,
            "y" if value.abs() <= 1e12 => object.transform.y = value,
            "scale_x" if value.abs() <= 1e4 => object.transform.scale_x = value,
            "scale_y" if value.abs() <= 1e4 => object.transform.scale_y = value,
            "rotation" if value.abs() <= 1e9 => object.transform.rotation = value,
            "alpha" if (0.0..=1.0).contains(&value) => object.transform.alpha = value,
            _ => return false,
        }
        true
    }
    pub fn object_property(
        &self,
        project: &Project,
        reference: &ObjectRef,
        name: &str,
    ) -> Option<f64> {
        if let Some(value) = self.transform_property_override(reference, name) {
            return Some(value);
        }
        if let Some(address) = self.graph.linked_paths.get(reference) {
            return self
                .graph
                .namespaces
                .get(&address.namespace)?
                .player
                .property(project, &address.path, name);
        }
        if let Some(path) = self.graph.paths.get(reference) {
            return self.property(project, path, name);
        }
        let transform = self.graph.dynamic.get(reference)?.transform;
        match name {
            "x" => Some(transform.x),
            "y" => Some(transform.y),
            "scale_x" => Some(transform.scale_x),
            "scale_y" => Some(transform.scale_y),
            "rotation" => Some(transform.rotation),
            "alpha" => Some(transform.alpha),
            _ => None,
        }
    }
    pub fn object_set_visible(&mut self, reference: &ObjectRef, value: bool) -> bool {
        if let Some(address) = self.graph.linked_paths.get(reference).cloned() {
            return self
                .graph
                .namespaces
                .get_mut(&address.namespace)
                .expect("namespace")
                .player
                .set_visible(&address.path, value);
        }
        if let Some(path) = self.graph.paths.get(reference).cloned() {
            return self.set_visible(&path, value);
        }
        let Some(object) = self.graph.dynamic.get_mut(reference) else {
            return false;
        };
        object.visible = value;
        true
    }
    pub fn object_visible(&self, project: &Project, reference: &ObjectRef) -> Option<bool> {
        if let Some(address) = self.graph.linked_paths.get(reference) {
            return self
                .graph
                .namespaces
                .get(&address.namespace)?
                .player
                .visible(project, &address.path);
        }
        if let Some(path) = self.graph.paths.get(reference) {
            return self.visible(project, path);
        }
        self.graph.dynamic.get(reference).map(|node| node.visible)
    }
    pub fn object_set_name(&mut self, reference: &ObjectRef, name: &str) -> bool {
        if let Some(address) = self.graph.linked_paths.get(reference).cloned() {
            return self
                .graph
                .namespaces
                .get_mut(&address.namespace)
                .expect("namespace")
                .player
                .set_name(&address.path, name);
        }
        if let Some(path) = self.graph.paths.get(reference).cloned() {
            return self.set_name(&path, name);
        }
        if name.len() > 4096 {
            return false;
        }
        let Some(object) = self.graph.dynamic.get_mut(reference) else {
            return false;
        };
        object.name = name.into();
        true
    }
}
