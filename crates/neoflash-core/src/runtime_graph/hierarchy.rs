//! Runtime hierarchy edits retain authored source addresses and lifetimes.
//! Source retirement still retires moved authored placements; no clock/world is
//! cloned. Unedited source order remains the default for newly appearing refs.
use super::*;

impl RuntimeGraph {
    pub(super) fn set_child_order(&mut self, parent: &ObjectRef, children: Vec<ObjectRef>) {
        self.suffix.insert(
            parent.clone(),
            children
                .iter()
                .filter(|child| self.dynamic.contains_key(*child))
                .cloned()
                .collect(),
        );
        self.child_order.insert(parent.clone(), children);
    }

    pub(super) fn detach_child(&mut self, child: &ObjectRef) {
        if let Some(parent) = self.parent(child) {
            let mut children = self.children(&parent);
            children.retain(|id| id != child);
            self.set_child_order(&parent, children);
            if let Some(suffix) = self.suffix.get_mut(&parent) {
                suffix.retain(|id| id != child);
            }
        }
        if let Some(node) = self.dynamic.get_mut(child) {
            node.parent = None;
        } else {
            self.parent_overrides.insert(child.clone(), None);
        }
    }

    pub(super) fn attach_child(&mut self, parent: &ObjectRef, child: &ObjectRef, index: usize) {
        self.detach_child(child);
        let mut children = self.children(parent);
        if let Some(node) = self.dynamic.get_mut(child) {
            node.parent = Some(parent.clone());
            self.suffix
                .entry(parent.clone())
                .or_default()
                .push(child.clone());
        } else if self.source_parent(child).as_ref() == Some(parent) {
            self.parent_overrides.remove(child);
        } else {
            self.parent_overrides
                .insert(child.clone(), Some(parent.clone()));
        }
        children.insert(index, child.clone());
        self.set_child_order(parent, children);
    }

    /// Clear dead source identities without allowing an effective parent to
    /// accidentally reconnect to a new placement at the same authored path.
    pub(super) fn forget_hierarchy(&mut self, retired: &BTreeSet<ObjectRef>) {
        self.parent_overrides.retain(|id, _| !retired.contains(id));
        for parent in self.parent_overrides.values_mut() {
            if parent.as_ref().is_some_and(|id| retired.contains(id)) {
                *parent = None;
            }
        }
        self.child_order.retain(|id, _| !retired.contains(id));
        for children in self.child_order.values_mut() {
            children.retain(|id| !retired.contains(id));
        }
    }

    /// An independently live child may have been moved under a retiring source
    /// placement. Detach it with its old event route before retiring the owner.
    pub(super) fn retire_hierarchy(&mut self, retired: &BTreeSet<ObjectRef>) {
        let children: Vec<_> = retired
            .iter()
            .flat_map(|parent| self.children(parent))
            .filter(|child| !retired.contains(child))
            .collect();
        for child in children {
            let route = self.route(&child);
            let attached = self.on_stage(&child);
            let mut events = vec![DisplayEvent {
                kind: "removed".into(),
                target_ref: child.clone(),
                route,
            }];
            if attached {
                for id in self.subtree(&child) {
                    events.push(DisplayEvent {
                        kind: "removed_from_stage".into(),
                        route: self.route(&id),
                        target_ref: id,
                    });
                }
            }
            self.detach_child(&child);
            if self.events.len().saturating_add(events.len()) <= MAX_DISPLAY_EVENTS {
                self.events.extend(events);
            } else {
                self.events_overflowed = true;
            }
        }
        for parent in retired {
            self.prefix.remove(parent);
            self.suffix.remove(parent);
        }
        self.forget_hierarchy(retired);
    }
}

impl Player {
    /// Layer masks belong to source-layer plans, not mutable display ancestry.
    /// Refuse affected authored edits until those dependencies can be migrated.
    pub(super) fn validate_authored_hierarchy_change(
        &self,
        project: &Project,
        child: &ObjectRef,
    ) -> Result<(), DisplayError> {
        if self.graph.dynamic.contains_key(child) {
            return Ok(());
        }
        let (player, path) = if let Some(address) = self.graph.linked_paths.get(child) {
            (
                &*self
                    .graph
                    .namespaces
                    .get(&address.namespace)
                    .expect("source namespace")
                    .player,
                &address.path,
            )
        } else if let Some(path) = self.graph.paths.get(child) {
            (self, path)
        } else {
            return Err(error(
                "Display reference is stale or belongs to another player",
            ));
        };
        if player.authored_snapshot(project).masks.iter().any(|mask| {
            mask.sources
                .iter()
                .chain(&mask.targets)
                .any(|member| member.starts_with(path) || path.starts_with(member))
        }) {
            return Err(error(
                "Moving or reordering authored layer-mask content is not supported; keep its source-layer hierarchy",
            ));
        }
        Ok(())
    }
}
