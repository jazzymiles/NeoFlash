use super::*;

/// The renderer rasterizes the group's complete subtree before applying its
/// filters. clip_ids are deferred until the filtered group is composited;
/// descendants' additional clips stay inside the source raster.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeFilterGroup {
    pub id: String,
    pub owner_ref: ObjectRef,
    pub parent_id: Option<String>,
    pub filters: Vec<FilterSpec>,
    pub clip_ids: Vec<String>,
}
impl Player {
    pub fn object_filters(&self, reference: &ObjectRef) -> Result<Vec<FilterSpec>, FilterError> {
        if !self.graph.alive(reference) {
            return Err(FilterError(
                "display reference is stale or belongs to another player".into(),
            ));
        }
        Ok(self
            .graph
            .filters
            .get(reference)
            .cloned()
            .unwrap_or_default())
    }
    pub fn object_set_filters(
        &mut self,
        reference: &ObjectRef,
        filters: &[FilterSpec],
    ) -> Result<(), FilterError> {
        if !self.graph.alive(reference) {
            return Err(FilterError(
                "display reference is stale or belongs to another player".into(),
            ));
        }
        validate_filters(filters)?;
        let old = self.graph.filters.get(reference).map_or(0, Vec::len);
        let groups =
            self.graph.filters.len() - usize::from(old > 0) + usize::from(!filters.is_empty());
        let total = self.graph.filters.values().map(Vec::len).sum::<usize>() - old + filters.len();
        if groups > 128 || total > 512 {
            return Err(FilterError(
                "runtime supports at most 128 filtered objects and 512 filter specifications"
                    .into(),
            ));
        }
        if filters.is_empty() {
            self.graph.filters.remove(reference);
        } else {
            self.graph
                .filters
                .insert(reference.clone(), filters.to_vec());
        }
        Ok(())
    }
}
impl RuntimeGraph {
    pub(super) fn retire_filters(&mut self, retired: &BTreeSet<ObjectRef>) {
        self.filters.retain(|id, _| !retired.contains(id));
    }
    pub(super) fn apply_filters(&self, snapshot: &mut RuntimeSnapshot) {
        // Objects are emitted parent-first. A single optional ID per object/draw
        // avoids cloning an inherited chain into every descendant primitive.
        let mut nearest: HashMap<ObjectRef, Option<String>> = HashMap::new();
        for object in &snapshot.objects {
            let parent = object
                .parent_ref
                .as_ref()
                .and_then(|id| nearest.get(id))
                .cloned()
                .flatten();
            let current = if object.filters.is_empty() {
                parent
            } else {
                let id = format!(
                    "filter-{}-{}",
                    object.object_ref.generation, object.object_ref.id
                );
                snapshot.filter_groups.push(RuntimeFilterGroup {
                    id: id.clone(),
                    owner_ref: object.object_ref.clone(),
                    parent_id: parent,
                    filters: object.filters.clone(),
                    clip_ids: object.clip_ids.clone(),
                });
                Some(id)
            };
            nearest.insert(object.object_ref.clone(), current);
        }
        for draw in &mut snapshot.draws {
            draw.filter_id = nearest.get(&draw.owner_ref).cloned().flatten();
        }
    }
}
