//! Timeline layer commands commit a complete, valid document in one undo step.
use crate::app::Editor;
use eframe::egui;
use neoflash_core::*;
use std::collections::{HashMap, HashSet};

#[derive(Clone)]
struct LayerClipboard {
    layers: Vec<Layer>,
    frames: u32,
}
fn blank_layer(name: String, role: LayerRole, parent: Option<Id>) -> Layer {
    Layer {
        id: new_id("layer"),
        name,
        visible: true,
        locked: false,
        properties: LayerProperties {
            role,
            parent,
            ..Default::default()
        },
        keyframes: if role == LayerRole::Folder {
            vec![]
        } else {
            vec![Keyframe {
                frame: 0,
                elements: vec![],
                tween: None,
                label: String::new(),
            }]
        },
    }
}
fn group_indices(layers: &[Layer], selected: usize) -> Vec<usize> {
    let mut ids = HashSet::from([layers[selected].id.clone()]);
    loop {
        let before = ids.len();
        for layer in layers {
            if layer
                .properties
                .parent
                .as_ref()
                .is_some_and(|id| ids.contains(id))
                || layer
                    .properties
                    .mask
                    .as_ref()
                    .is_some_and(|id| ids.contains(id))
            {
                ids.insert(layer.id.clone());
            }
        }
        if ids.len() == before {
            break;
        }
    }
    layers
        .iter()
        .enumerate()
        .filter_map(|(index, layer)| ids.contains(&layer.id).then_some(index))
        .collect()
}
pub(crate) fn visible_layer_indices(timeline: &Timeline) -> Vec<usize> {
    timeline
        .layers
        .iter()
        .enumerate()
        .filter_map(|(index, layer)| {
            let mut parent = layer.properties.parent.as_ref();
            for _ in 0..timeline.layers.len() {
                let Some(id) = parent else {
                    return Some(index);
                };
                let Some(folder) = timeline.layers.iter().find(|l| &l.id == id) else {
                    return Some(index);
                };
                if folder.properties.collapsed {
                    return None;
                }
                parent = folder.properties.parent.as_ref();
            }
            None
        })
        .collect()
}
pub(crate) fn layer_label(layer: &Layer) -> String {
    let prefix = match layer.properties.role {
        LayerRole::Normal => {
            if layer.properties.mask.is_some() {
                "↳ "
            } else {
                ""
            }
        }
        LayerRole::Guide => "◇ ",
        LayerRole::Mask => "◉ ",
        LayerRole::Folder => {
            if layer.properties.collapsed {
                "▸ "
            } else {
                "▾ "
            }
        }
    };
    format!(
        "{}{}{}",
        if layer.properties.parent.is_some() {
            "   "
        } else {
            ""
        },
        prefix,
        layer.name
    )
}
impl Editor {
    fn layer_clipboard_id(&self) -> egui::Id {
        egui::Id::new(("layer-clipboard", &self.clipboard.session))
    }
    fn layer_change(&mut self, change: impl FnOnce(&mut Project, &str)) {
        self.finish_edits();
        self.timeline_key_drag = None;
        let symbol = self.symbol_id().to_owned();
        let mut candidate = self.project.clone();
        change(&mut candidate, &symbol);
        if candidate
            .symbols
            .iter()
            .flat_map(|s| &s.timeline.layers)
            .any(|l| !l.properties.is_default())
        {
            candidate.format_version = candidate.format_version.max(4);
        }
        if let Err(error) = candidate.validate() {
            self.message(error.to_string());
            return;
        }
        if candidate != self.project {
            self.transact(move |editor| {
                editor.project = candidate;
                editor.normalize_selection();
                editor.clear_selection();
            });
        }
    }
    pub(crate) fn delete_timeline_layer(&mut self, index: usize) {
        self.layer_change(|project, symbol| {
            let timeline = &mut project.symbol_mut(symbol).unwrap().timeline;
            let indices = group_indices(&timeline.layers, index);
            let removed: HashSet<_> = indices
                .iter()
                .map(|i| timeline.layers[*i].id.clone())
                .collect();
            let parent = timeline.layers[index].properties.parent.clone();
            timeline.layers.retain(|l| !removed.contains(&l.id));
            for layer in &mut timeline.layers {
                if layer
                    .properties
                    .parent
                    .as_ref()
                    .is_some_and(|id| removed.contains(id))
                {
                    layer.properties.parent = parent.clone();
                }
                if layer
                    .properties
                    .mask
                    .as_ref()
                    .is_some_and(|id| removed.contains(id))
                {
                    layer.properties.mask = None;
                }
            }
            if timeline.layers.is_empty() {
                timeline
                    .layers
                    .push(blank_layer("Layer 1".into(), LayerRole::Normal, None));
            }
        });
    }
    fn copy_timeline_layers(&self, ctx: &egui::Context, index: usize) {
        let timeline = &self.symbol().timeline;
        let indices = group_indices(&timeline.layers, index);
        let layers = indices
            .into_iter()
            .map(|i| timeline.layers[i].clone())
            .collect();
        ctx.data_mut(|data| {
            data.insert_temp(
                self.layer_clipboard_id(),
                LayerClipboard {
                    layers,
                    frames: timeline.frame_count,
                },
            )
        });
    }
    fn paste_timeline_layers(&mut self, clipboard: LayerClipboard, index: usize) {
        self.layer_change(move |project, symbol| {
            let timeline = &mut project.symbol_mut(symbol).unwrap().timeline;
            let mut layers = clipboard.layers;
            let mut mapping = HashMap::new();
            for layer in &layers {
                mapping.insert(layer.id.clone(), new_id("layer"));
                for element in layer.keyframes.iter().flat_map(|k| &k.elements) {
                    mapping
                        .entry(element.id.clone())
                        .or_insert_with(|| new_id("object"));
                }
            }
            for layer in &mut layers {
                layer.id = mapping[&layer.id].clone();
                layer.name = format!("{} copy", layer.name);
                layer.properties.parent = layer
                    .properties
                    .parent
                    .as_ref()
                    .and_then(|id| mapping.get(id).cloned());
                layer.properties.mask = layer
                    .properties
                    .mask
                    .as_ref()
                    .and_then(|id| mapping.get(id).cloned());
                for element in layer.keyframes.iter_mut().flat_map(|k| &mut k.elements) {
                    element.id = mapping[&element.id].clone();
                }
            }
            timeline.frame_count = timeline.frame_count.max(clipboard.frames);
            timeline.layers.splice(index + 1..index + 1, layers);
        });
    }
    fn set_layer_role(&mut self, index: usize, role: LayerRole) {
        self.layer_change(|project, symbol| {
            let timeline = &mut project.symbol_mut(symbol).unwrap().timeline;
            let id = timeline.layers[index].id.clone();
            timeline.layers[index].properties.role = role;
            timeline.layers[index].properties.mask = None;
            if role != LayerRole::Mask {
                for layer in &mut timeline.layers {
                    if layer.properties.mask.as_ref() == Some(&id) {
                        layer.properties.mask = None;
                    }
                }
            } else if let Some(target) = timeline
                .layers
                .iter_mut()
                .skip(index + 1)
                .find(|l| l.properties.role == LayerRole::Normal)
            {
                target.properties.mask = Some(id);
            }
        });
    }
    /// Returns true when the caller should enter its inline name editor.
    pub(crate) fn timeline_layer_menu(&mut self, ui: &mut egui::Ui, index: usize) -> bool {
        let mut rename = false;
        let layer = self.symbol().timeline.layers[index].clone();
        let ctx = ui.ctx().clone();
        macro_rules! action {
            ($label:expr,$body:expr) => {
                if ui.button($label).clicked() {
                    $body;
                    ui.close();
                }
            };
        }
        action!(
            "Show All",
            self.layer_change(
                |p, id| for l in &mut p.symbol_mut(id).unwrap().timeline.layers {
                    l.visible = true;
                }
            )
        );
        action!(
            "Unlock All",
            self.layer_change(
                |p, id| for l in &mut p.symbol_mut(id).unwrap().timeline.layers {
                    l.locked = false;
                }
            )
        );
        action!(
            "Lock Others",
            self.layer_change(|p, id| {
                for (i, l) in p
                    .symbol_mut(id)
                    .unwrap()
                    .timeline
                    .layers
                    .iter_mut()
                    .enumerate()
                {
                    l.locked = i != index && l.properties.role != LayerRole::Folder;
                }
            })
        );
        action!(
            "Hide Others",
            self.layer_change(|p, id| {
                for (i, l) in p
                    .symbol_mut(id)
                    .unwrap()
                    .timeline
                    .layers
                    .iter_mut()
                    .enumerate()
                {
                    l.visible = i == index
                        || l.properties.role == LayerRole::Folder
                        || Some(&l.id) == layer.properties.mask.as_ref();
                }
            })
        );
        action!(
            "Others Transparent",
            self.layer_change(|p, id| {
                for (i, l) in p
                    .symbol_mut(id)
                    .unwrap()
                    .timeline
                    .layers
                    .iter_mut()
                    .enumerate()
                {
                    l.properties.outline = i != index;
                }
            })
        );
        action!(
            "Show All as Solid",
            self.layer_change(
                |p, id| for l in &mut p.symbol_mut(id).unwrap().timeline.layers {
                    l.properties.outline = false;
                }
            )
        );
        ui.separator();
        action!(
            "Insert Layer",
            self.layer_change(|p, id| {
                let t = &mut p.symbol_mut(id).unwrap().timeline;
                let parent = if layer.properties.role == LayerRole::Folder {
                    Some(layer.id.clone())
                } else {
                    layer.properties.parent.clone()
                };
                t.layers.insert(
                    index + 1,
                    blank_layer(
                        format!("Layer {}", t.layers.len() + 1),
                        LayerRole::Normal,
                        parent,
                    ),
                );
            })
        );
        action!("Delete Layer", self.delete_timeline_layer(index));
        action!("Rename layer", rename = true);
        ui.separator();
        action!("Cut Layers", {
            self.copy_timeline_layers(&ctx, index);
            self.delete_timeline_layer(index);
        });
        action!("Copy Layers", self.copy_timeline_layers(&ctx, index));
        let clipboard = ctx.data(|data| data.get_temp::<LayerClipboard>(self.layer_clipboard_id()));
        if ui
            .add_enabled(clipboard.is_some(), egui::Button::new("Paste Layers"))
            .clicked()
        {
            self.paste_timeline_layers(clipboard.unwrap(), index);
            ui.close();
        }
        action!("Duplicate Layers", {
            self.copy_timeline_layers(&ctx, index);
            let copy = ctx
                .data(|data| data.get_temp::<LayerClipboard>(self.layer_clipboard_id()))
                .unwrap();
            self.paste_timeline_layers(copy, index);
        });
        let can_merge = layer.properties.role == LayerRole::Normal
            && !layer.locked
            && self.symbol().timeline.frame_count <= 10000
            && self
                .symbol()
                .timeline
                .layers
                .get(index + 1)
                .is_some_and(|next| {
                    next.properties.role == LayerRole::Normal
                        && !next.locked
                        && next.visible == layer.visible
                        && next.properties.mask == layer.properties.mask
                        && next.properties.parent == layer.properties.parent
                        && next.keyframes.iter().all(|key| key.label.is_empty())
                });
        if ui
            .add_enabled(can_merge, egui::Button::new("Merge with Next Layer"))
            .on_hover_text("Preserve both layers’ animation; tweened frames are sampled into keys")
            .clicked()
        {
            self.layer_change(|p, id| {
                let t = &mut p.symbol_mut(id).unwrap().timeline;
                let next = t.layers.remove(index + 1);
                let first = t.layers[index].clone();
                let frames: Vec<u32> = if first
                    .keyframes
                    .iter()
                    .chain(&next.keyframes)
                    .any(|k| k.tween.is_some())
                {
                    (0..t.frame_count).collect()
                } else {
                    first
                        .keyframes
                        .iter()
                        .chain(&next.keyframes)
                        .map(|k| k.frame)
                        .collect::<std::collections::BTreeSet<_>>()
                        .into_iter()
                        .collect()
                };
                t.layers[index].keyframes = frames
                    .into_iter()
                    .map(|frame| {
                        let mut elements = sample_layer(&first, frame);
                        elements.extend(sample_layer(&next, frame));
                        Keyframe {
                            frame,
                            elements,
                            tween: None,
                            label: first
                                .key_at(frame)
                                .filter(|k| k.frame == frame)
                                .map(|k| k.label.clone())
                                .unwrap_or_default(),
                        }
                    })
                    .collect();
            });
            ui.close();
        }
        let group = group_indices(&self.symbol().timeline.layers, index);
        let contiguous = group.last().copied().unwrap_or(index) - index + 1 == group.len();
        ui.add_enabled_ui(contiguous, |ui| {
            ui.menu_button("Convert Layers to Symbol…", |ui| {
                let name_id = ui.id().with("layer-symbol-name");
                let mut name = ctx
                    .data(|data| data.get_temp::<String>(name_id))
                    .unwrap_or_else(|| format!("{} symbol", layer.name));
                ui.label("Movie Clip name");
                ui.text_edit_singleline(&mut name);
                ctx.data_mut(|data| data.insert_temp(name_id, name.clone()));
                if ui
                    .add_enabled(
                        !name.trim().is_empty(),
                        egui::Button::new("Create Movie Clip"),
                    )
                    .clicked()
                {
                    self.convert_timeline_layers(index, name.trim().to_owned());
                    ui.close();
                }
            })
            .response
            .on_hover_text("Convert a contiguous layer stack, preserving all authored frames");
        });
        ui.separator();
        if layer.properties.role != LayerRole::Folder {
            for (role, label) in [
                (LayerRole::Normal, "Normal"),
                (LayerRole::Guide, "Guide"),
                (LayerRole::Mask, "Mask"),
            ] {
                if ui
                    .selectable_label(layer.properties.role == role, label)
                    .clicked()
                {
                    self.set_layer_role(index, role);
                    ui.close();
                }
            }
        }
        let mut masking = self.show_layer_masking(&ctx);
        if ui.checkbox(&mut masking, "Show Masking").changed() {
            self.set_layer_masking(&ctx, masking);
        }
        ui.separator();
        action!(
            "Insert Folder",
            self.layer_change(|p, id| {
                let t = &mut p.symbol_mut(id).unwrap().timeline;
                let folder = blank_layer(
                    "Folder".into(),
                    LayerRole::Folder,
                    layer.properties.parent.clone(),
                );
                t.layers[index].properties.parent = Some(folder.id.clone());
                t.layers.insert(index, folder);
            })
        );
        if layer.properties.role == LayerRole::Folder {
            action!(
                if layer.properties.collapsed {
                    "Expand Folder"
                } else {
                    "Collapse Folder"
                },
                self.layer_change(|p, id| p.symbol_mut(id).unwrap().timeline.layers[index]
                    .properties
                    .collapsed = !layer.properties.collapsed)
            );
        }
        action!(
            "Expand All Folders",
            self.layer_change(
                |p, id| for l in &mut p.symbol_mut(id).unwrap().timeline.layers {
                    l.properties.collapsed = false;
                }
            )
        );
        action!(
            "Collapse All Folders",
            self.layer_change(
                |p, id| for l in &mut p.symbol_mut(id).unwrap().timeline.layers {
                    if l.properties.role == LayerRole::Folder {
                        l.properties.collapsed = true;
                    }
                }
            )
        );
        ui.menu_button("Properties…", |ui| {
            let mut draft = layer.clone();
            ui.label("Layer name");
            ui.text_edit_singleline(&mut draft.name);
            ui.checkbox(&mut draft.visible, "Visible");
            ui.checkbox(&mut draft.locked, "Locked");
            ui.checkbox(
                &mut draft.properties.outline,
                "Outline / transparent authoring view",
            );
            egui::ComboBox::from_id_salt("layer-folder")
                .selected_text(
                    draft
                        .properties
                        .parent
                        .as_ref()
                        .and_then(|id| self.symbol().timeline.layers.iter().find(|l| &l.id == id))
                        .map(|l| l.name.as_str())
                        .unwrap_or("No folder"),
                )
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut draft.properties.parent, None, "No folder");
                    for folder in self
                        .symbol()
                        .timeline
                        .layers
                        .iter()
                        .filter(|l| l.properties.role == LayerRole::Folder && l.id != layer.id)
                    {
                        ui.selectable_value(
                            &mut draft.properties.parent,
                            Some(folder.id.clone()),
                            &folder.name,
                        );
                    }
                });
            if layer.properties.role == LayerRole::Normal {
                egui::ComboBox::from_id_salt("layer-mask")
                    .selected_text(
                        draft
                            .properties
                            .mask
                            .as_ref()
                            .and_then(|id| {
                                self.symbol().timeline.layers.iter().find(|l| &l.id == id)
                            })
                            .map(|l| l.name.as_str())
                            .unwrap_or("No mask"),
                    )
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut draft.properties.mask, None, "No mask");
                        for mask in self
                            .symbol()
                            .timeline
                            .layers
                            .iter()
                            .filter(|l| l.properties.role == LayerRole::Mask)
                        {
                            ui.selectable_value(
                                &mut draft.properties.mask,
                                Some(mask.id.clone()),
                                &mask.name,
                            );
                        }
                    });
            }
            if draft != layer && !draft.name.trim().is_empty() {
                self.layer_change(|p, id| p.symbol_mut(id).unwrap().timeline.layers[index] = draft);
            }
        });
        rename
    }
    fn convert_timeline_layers(&mut self, index: usize, name: String) {
        self.layer_change(|project, id| {
            let timeline = &mut project.symbol_mut(id).unwrap().timeline;
            let indices = group_indices(&timeline.layers, index);
            if indices.last().copied().unwrap_or(index) - index + 1 != indices.len() {
                return;
            }
            let selected: HashSet<_> = indices
                .iter()
                .map(|i| timeline.layers[*i].id.clone())
                .collect();
            let mut layers: Vec<_> = indices
                .iter()
                .map(|i| timeline.layers[*i].clone())
                .collect();
            for layer in &mut layers {
                if !layer
                    .properties
                    .parent
                    .as_ref()
                    .is_some_and(|id| selected.contains(id))
                {
                    layer.properties.parent = None;
                }
                if !layer
                    .properties
                    .mask
                    .as_ref()
                    .is_some_and(|id| selected.contains(id))
                {
                    layer.properties.mask = None;
                }
            }
            let symbol_id = new_id("symbol");
            let frame_count = timeline.frame_count;
            let mut replacement = blank_layer(
                name.clone(),
                LayerRole::Normal,
                timeline.layers[index].properties.parent.clone(),
            );
            replacement.properties.mask = timeline.layers[index].properties.mask.clone();
            replacement.keyframes[0].elements.push(Element {
                id: new_id("instance"),
                name: name.clone(),
                transform: Transform::default(),
                kind: ElementKind::Instance {
                    symbol_id: symbol_id.clone(),
                    playback: InstancePlayback::Loop,
                },
                scripts: vec![],
                accessibility: None,
            });
            timeline.layers.retain(|l| !selected.contains(&l.id));
            timeline.layers.insert(index, replacement);
            project.symbols.push(Symbol {
                id: symbol_id,
                name,
                folder: String::new(),
                kind: SymbolKind::MovieClip,
                timeline: Timeline {
                    frame_count,
                    layers,
                    scripts: vec![],
                },
                class_name: None,
                linkage_id: None,
            });
        });
    }
}
