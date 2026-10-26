use crate::app::Editor;
use neoflash_core::*;
use std::collections::HashSet;

/// Authoring selection addresses placements in one symbol, independently of layer order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Selection {
    pub symbol: Id,
    pub targets: Vec<SelectionTarget>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct SelectionTarget {
    pub layer: Id,
    pub element: Id,
}

impl Selection {
    pub fn len(&self) -> usize {
        self.targets.len()
    }
    pub fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }
    pub fn single(&self) -> Option<&SelectionTarget> {
        (self.targets.len() == 1).then(|| &self.targets[0])
    }
}

impl Editor {
    pub(crate) fn clear_selection(&mut self) {
        self.selection = Selection {
            symbol: self.symbol_id().to_owned(),
            targets: vec![],
        };
    }
    pub(crate) fn target_for_id(&self, id: &str) -> Option<SelectionTarget> {
        self.symbol().timeline.layers.iter().find_map(|layer| {
            (layer.visible
                && !layer.locked
                && layer
                    .key_at(self.frame)
                    .is_some_and(|key| key.elements.iter().any(|element| element.id == id)))
            .then(|| SelectionTarget {
                layer: layer.id.clone(),
                element: id.to_owned(),
            })
        })
    }
    pub(crate) fn target_layer_index(&self, target: &SelectionTarget) -> Option<usize> {
        self.symbol()
            .timeline
            .layers
            .iter()
            .position(|layer| layer.id == target.layer)
    }
    pub(crate) fn target_editable(&self, target: &SelectionTarget) -> bool {
        self.target_layer_index(target).is_some_and(|index| {
            let layer = &self.symbol().timeline.layers[index];
            layer.visible
                && !layer.locked
                && layer.key_at(self.frame).is_some_and(|key| {
                    key.elements
                        .iter()
                        .any(|element| element.id == target.element)
                })
        })
    }
    pub(crate) fn editable_targets(&self) -> Vec<SelectionTarget> {
        self.symbol()
            .timeline
            .layers
            .iter()
            .filter(|layer| layer.visible && !layer.locked)
            .flat_map(|layer| {
                layer.key_at(self.frame).into_iter().flat_map(move |key| {
                    key.elements.iter().map(move |element| SelectionTarget {
                        layer: layer.id.clone(),
                        element: element.id.clone(),
                    })
                })
            })
            .collect()
    }
    pub(crate) fn set_selection(&mut self, targets: Vec<SelectionTarget>) {
        if targets.is_empty() {
            self.clear_selection();
            return;
        }
        let allowed: HashSet<_> = self.editable_targets().into_iter().collect();
        let mut seen = HashSet::new();
        let unique: Vec<_> = targets
            .into_iter()
            .filter(|target| allowed.contains(target) && seen.insert(target.clone()))
            .collect();
        if let Some(primary) = unique.last()
            && let Some(index) = self.target_layer_index(primary)
        {
            self.layer = index;
        }
        self.selection = Selection {
            symbol: self.symbol_id().to_owned(),
            targets: unique,
        };
    }
    pub(crate) fn select_single_id(&mut self, id: Id) {
        self.set_selection(self.target_for_id(&id).into_iter().collect());
    }
    pub(crate) fn select_all(&mut self) {
        self.finish_edits();
        self.set_selection(self.editable_targets());
    }
    pub(crate) fn normalize_targets(&mut self) {
        if self.selection.symbol != self.symbol_id() {
            self.clear_selection();
            return;
        }
        if self.selection.is_empty() {
            return;
        }
        let allowed: HashSet<_> = self.editable_targets().into_iter().collect();
        // Active timeline layer remains an insertion target; normalization must not move it.
        self.selection
            .targets
            .retain(|target| allowed.contains(target));
    }
    pub(crate) fn selected_editable(&self) -> bool {
        self.selection.symbol == self.symbol_id()
            && self
                .selection
                .single()
                .is_some_and(|target| self.target_editable(target))
    }
    pub fn selected_element(&self) -> Option<&Element> {
        if self.selection.symbol != self.symbol_id() {
            return None;
        }
        let target = self.selection.single()?;
        self.symbol()
            .timeline
            .layers
            .get(self.target_layer_index(target)?)?
            .key_at(self.frame)?
            .elements
            .iter()
            .find(|element| element.id == target.element)
    }
    pub fn selected_sampled(&self) -> Option<Element> {
        if self.selection.symbol != self.symbol_id() {
            return None;
        }
        let target = self.selection.single()?;
        sample_layer(
            &self.symbol().timeline.layers[self.target_layer_index(target)?],
            self.frame,
        )
        .into_iter()
        .find(|element| element.id == target.element)
    }
    pub fn selected_element_mut(&mut self) -> Option<&mut Element> {
        if self.selection.symbol != self.symbol_id() {
            return None;
        }
        let target = self.selection.single()?.clone();
        let index = self.target_layer_index(&target)?;
        let frame = self.frame;
        self.symbol_mut().timeline.layers[index]
            .key_at_mut(frame)?
            .elements
            .iter_mut()
            .find(|element| element.id == target.element)
    }
    pub(crate) fn sampled_targets(&self) -> Vec<(SelectionTarget, Element)> {
        if self.selection.symbol != self.symbol_id() || self.selection.is_empty() {
            return vec![];
        }
        let selected: HashSet<_> = self.selection.targets.iter().collect();
        let mut sampled = Vec::new();
        for layer in self
            .symbol()
            .timeline
            .layers
            .iter()
            .filter(|layer| layer.visible && !layer.locked)
        {
            for element in sample_layer(layer, self.frame) {
                let target = SelectionTarget {
                    layer: layer.id.clone(),
                    element: element.id.clone(),
                };
                if selected.contains(&target) {
                    sampled.push((target, element));
                }
            }
        }
        sampled
    }
    pub(crate) fn ensure_key_for(&mut self, layer_id: &str, blank: bool) {
        let frame = self.frame;
        let Some(layer) = self
            .symbol_mut()
            .timeline
            .layers
            .iter_mut()
            .find(|layer| layer.id == layer_id)
        else {
            return;
        };
        if let Some(key) = layer.keyframes.iter_mut().find(|key| key.frame == frame) {
            if blank {
                key.elements.clear();
                key.tween = None;
            }
            return;
        }
        let sampled = sample_layer(layer, frame);
        let mut key = if blank {
            Keyframe {
                frame,
                elements: vec![],
                tween: None,
                label: String::new(),
            }
        } else {
            layer.key_at(frame).cloned().unwrap_or(Keyframe {
                frame,
                elements: vec![],
                tween: None,
                label: String::new(),
            })
        };
        if !blank {
            key.elements = sampled;
        }
        key.frame = frame;
        key.label.clear();
        layer.keyframes.push(key);
        layer.keyframes.sort_by_key(|key| key.frame);
    }
    pub(crate) fn ensure_selected_key(&mut self) {
        if let Some(target) = self.selection.single().cloned() {
            self.ensure_key_for(&target.layer, false);
        }
    }
    pub fn delete_selected(&mut self) {
        self.finish_edits();
        let targets = self.sampled_targets();
        if targets.is_empty() {
            return;
        }
        self.playing = false;
        self.transact(|editor| {
            for (target, _) in &targets {
                editor.ensure_key_for(&target.layer, false);
                let index = editor.target_layer_index(target).unwrap();
                let frame = editor.frame;
                editor.symbol_mut().timeline.layers[index]
                    .key_at_mut(frame)
                    .unwrap()
                    .elements
                    .retain(|element| element.id != target.element);
            }
            editor.clear_selection();
        });
    }
    pub fn duplicate(&mut self) {
        self.finish_edits();
        let targets = self.sampled_targets();
        if targets.is_empty() {
            return;
        }
        self.playing = false;
        self.transact(|editor| {
            let mut copies = Vec::new();
            for (target, mut element) in targets {
                editor.ensure_key_for(&target.layer, false);
                element.id = new_id("object");
                element.name = format!(
                    "{}_copy_{}",
                    element.name,
                    element.id.rsplit('-').next().unwrap_or("1")
                );
                element.transform.x += 16.;
                element.transform.y += 16.;
                copies.push(SelectionTarget {
                    layer: target.layer.clone(),
                    element: element.id.clone(),
                });
                let index = editor.target_layer_index(&target).unwrap();
                let frame = editor.frame;
                editor.symbol_mut().timeline.layers[index]
                    .key_at_mut(frame)
                    .unwrap()
                    .elements
                    .push(element);
            }
            editor.set_selection(copies);
        });
    }
}
