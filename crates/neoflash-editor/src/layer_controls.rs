//! Timeline layer commands commit a complete, valid document in one undo step.
use crate::app::Editor;
use crate::theme::DesignUi as _;
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
    // Serialized layers paint back to front. Display siblings front to back,
    // while keeping folder/mask headers above their children.
    let indices: HashMap<_, _> = timeline
        .layers
        .iter()
        .enumerate()
        .map(|(index, layer)| (layer.id.as_str(), index))
        .collect();
    let mut children = vec![Vec::new(); timeline.layers.len()];
    let mut roots = Vec::new();
    for (index, layer) in timeline.layers.iter().enumerate() {
        let parent = layer
            .properties
            .mask
            .as_ref()
            .or(layer.properties.parent.as_ref())
            .and_then(|id| indices.get(id.as_str()).copied());
        if let Some(parent) = parent {
            children[parent].push(index);
        } else {
            roots.push(index);
        }
    }
    let mut visible = Vec::with_capacity(timeline.layers.len());
    let mut stack = roots;
    let mut visited = HashSet::new();
    while let Some(index) = stack.pop() {
        if !visited.insert(index) {
            continue;
        }
        visible.push(index);
        let layer = &timeline.layers[index];
        if layer.properties.role != LayerRole::Folder || !layer.properties.collapsed {
            stack.extend(children[index].iter().copied());
        }
    }
    visible
}
/// NF2 `TL Layer` type of a document layer. Folders follow their collapsed state; a normal layer
/// with a mask is Masked. The editor has no motion-guide, sound, video or armature layers yet.
pub(crate) fn layer_kind(layer: &Layer) -> neoflash_ui::components::timeline::LayerKind {
    use neoflash_ui::components::timeline::LayerKind;
    match layer.properties.role {
        LayerRole::Folder if layer.properties.collapsed => LayerKind::FolderClosed,
        LayerRole::Folder => LayerKind::FolderOpen,
        LayerRole::Guide => LayerKind::Guide,
        LayerRole::Mask => LayerKind::Mask,
        LayerRole::Normal if layer.properties.mask.is_some() => LayerKind::Masked,
        LayerRole::Normal => LayerKind::Layer,
    }
}
/// Nesting depth under folders and masks (`Indent 1`, `Indent 2`, …), matching
/// [`visible_layer_indices`]: a mask link wins over the folder parent.
pub(crate) fn layer_depth(timeline: &Timeline, layer: &Layer) -> usize {
    let mut depth = 0;
    let mut seen = HashSet::from([layer.id.as_str()]);
    let mut current = layer;
    while let Some(parent) = current
        .properties
        .mask
        .as_ref()
        .or(current.properties.parent.as_ref())
        .and_then(|id| timeline.layers.iter().find(|l| &l.id == id))
    {
        if !seen.insert(parent.id.as_str()) {
            break;
        }
        depth += 1;
        current = parent;
    }
    depth
}
/// Layer outline colour (document data) as an egui colour for the `TL Toggle` swatch.
pub(crate) fn layer_color(layer: &Layer) -> egui::Color32 {
    let color = layer_outline_color(layer);
    egui::Color32::from_rgb(color[0], color[1], color[2])
}

#[derive(Clone)]
struct LayerOutlineDraft {
    symbol: Id,
    layer: Id,
    name: String,
    color: egui::Color32,
}

impl Editor {
    /// A drop above a visual row belongs after that row's group in painter
    /// order. A drop below every visual row belongs at painter index zero.
    pub(crate) fn reorder_timeline_layer_visual(&mut self, layer_id: &str, before: Option<&str>) {
        let layers = &self.symbol().timeline.layers;
        let Some(index) = layers.iter().position(|layer| layer.id == layer_id) else {
            return;
        };
        let moved: HashSet<_> = group_indices(layers, index).into_iter().collect();
        let insertion = if let Some(target) = before {
            let Some(target) = layers.iter().position(|layer| layer.id == target) else {
                return;
            };
            if moved.contains(&target) {
                return;
            }
            let end = group_indices(layers, target)
                .into_iter()
                .filter(|i| !moved.contains(i))
                .max()
                .unwrap_or(target);
            layers
                .iter()
                .enumerate()
                .find(|(i, _)| *i > end && !moved.contains(i))
                .map(|(_, layer)| layer.id.clone())
        } else {
            layers
                .iter()
                .enumerate()
                .find(|(i, _)| !moved.contains(i))
                .map(|(_, layer)| layer.id.clone())
        };
        self.reorder_timeline_layer(layer_id, insertion.as_deref());
    }

    fn outline_draft_id(&self) -> egui::Id {
        egui::Id::new(("layer-outline-color", &self.clipboard.session))
    }
    fn begin_layer_outline_color(&self, ctx: &egui::Context, layer: &Layer) {
        let color = layer_outline_color(layer);
        ctx.data_mut(|data| {
            data.insert_temp(
                self.outline_draft_id(),
                LayerOutlineDraft {
                    symbol: self.symbol_id().to_owned(),
                    layer: layer.id.clone(),
                    name: layer.name.clone(),
                    color: egui::Color32::from_rgb(color[0], color[1], color[2]),
                },
            )
        });
    }
    pub(crate) fn toggle_layer_folder(&mut self, index: usize) {
        self.layer_change(|project, symbol| {
            let layer = &mut project.symbol_mut(symbol).unwrap().timeline.layers[index];
            if layer.properties.role == LayerRole::Folder {
                layer.properties.collapsed = !layer.properties.collapsed;
            }
        });
    }
    fn apply_layer_outline(
        &mut self,
        symbol: &str,
        layer_id: &str,
        outline: Option<bool>,
        color: Option<[u8; 3]>,
    ) {
        let Some(layer) = self.project.symbol(symbol).and_then(|symbol| {
            symbol
                .timeline
                .layers
                .iter()
                .find(|layer| layer.id == layer_id)
        }) else {
            return;
        };
        let mut properties = layer.properties.clone();
        if let Some(outline) = outline {
            properties.outline = outline;
        }
        if let Some(color) = color
            && layer_outline_color(layer)[..3] != color
        {
            properties.outline_color = Some(color);
        }
        if properties == layer.properties {
            return;
        }
        let symbol = symbol.to_owned();
        let layer_id = layer_id.to_owned();
        self.finish_edits();
        self.transact(move |editor| {
            editor.project.format_version = editor.project.format_version.max(4);
            if let Some(layer) = editor.project.symbol_mut(&symbol).and_then(|symbol| {
                symbol
                    .timeline
                    .layers
                    .iter_mut()
                    .find(|layer| layer.id == layer_id)
            }) {
                layer.properties = properties;
            }
        });
    }
    /// `TL Toggle` Kind=Outline: solid ↔ outline-only view of one layer (one undo step).
    pub(crate) fn toggle_layer_outline(&mut self, layer: &Layer) {
        let symbol_id = self.symbol_id().to_owned();
        self.apply_layer_outline(&symbol_id, &layer.id, Some(!layer.properties.outline), None);
    }
    /// Right-click menu of the outline swatch: change the layer colour.
    pub(crate) fn layer_outline_menu(&mut self, response: &egui::Response, layer: &Layer) {
        response.context_menu(|ui| {
            crate::ui_icons::menu_contents(ui, 240., |ui| {
                neoflash_ui::components::menu_section(ui, &layer.name);
                if ui
                    .add(neoflash_ui::components::MenuItem::new("Outline color…"))
                    .clicked()
                {
                    self.begin_layer_outline_color(ui.ctx(), layer);
                }
            })
        });
    }
    /// `TL Layers Header` columns: toggle every row. When any row is in the default state the
    /// click moves all rows to the other state (Flash behaviour), otherwise it restores them.
    pub(crate) fn toggle_all_layers(
        &mut self,
        column: neoflash_ui::components::timeline::ToggleKind,
    ) {
        use neoflash_ui::components::timeline::ToggleKind;
        let layers = &self.symbol().timeline.layers;
        let target = match column {
            ToggleKind::Visibility => !layers.iter().any(|l| !l.visible),
            ToggleKind::Lock => layers
                .iter()
                .any(|l| !l.locked && l.properties.role != LayerRole::Folder),
            ToggleKind::Outline => layers
                .iter()
                .any(|l| !l.properties.outline && l.properties.role != LayerRole::Folder),
        };
        self.layer_change(|p, id| {
            for l in &mut p.symbol_mut(id).unwrap().timeline.layers {
                match column {
                    ToggleKind::Visibility => l.visible = !target,
                    ToggleKind::Lock if l.properties.role != LayerRole::Folder => l.locked = target,
                    ToggleKind::Outline if l.properties.role != LayerRole::Folder => {
                        l.properties.outline = target
                    }
                    _ => {}
                }
            }
        });
    }
    /// Insert a folder above `index` holding that layer (the Layer menu ▸ Insert Folder command).
    pub(crate) fn insert_layer_folder(&mut self, index: usize) {
        let Some(layer) = self.symbol().timeline.layers.get(index).cloned() else {
            return;
        };
        self.layer_change(|p, id| {
            let t = &mut p.symbol_mut(id).unwrap().timeline;
            let folder = blank_layer(
                "Folder".into(),
                LayerRole::Folder,
                layer.properties.parent.clone(),
            );
            t.layers[index].properties.parent = Some(folder.id.clone());
            t.layers.insert(index, folder);
        });
    }
    pub(crate) fn layer_outline_color_dialog(&mut self, ctx: &egui::Context) {
        let id = self.outline_draft_id();
        let Some(mut draft) = ctx.data(|data| data.get_temp::<LayerOutlineDraft>(id)) else {
            return;
        };
        if self.project.symbol(&draft.symbol).is_none_or(|symbol| {
            !symbol
                .timeline
                .layers
                .iter()
                .any(|layer| layer.id == draft.layer)
        }) {
            ctx.data_mut(|data| data.remove::<LayerOutlineDraft>(id));
            return;
        }
        let mut open = true;
        let mut apply = false;
        let mut cancel = false;
        egui::Window::new("Layer Outline Color")
            .id(id)
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(&draft.name);
                egui::color_picker::color_picker_color32(
                    ui,
                    &mut draft.color,
                    egui::color_picker::Alpha::Opaque,
                );
                ui.horizontal(|ui| {
                    apply = ui.button("Apply").clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
        if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            cancel = true;
        }
        if apply {
            let color = [draft.color.r(), draft.color.g(), draft.color.b()];
            self.apply_layer_outline(&draft.symbol, &draft.layer, None, Some(color));
        }
        ctx.data_mut(|data| {
            if open && !apply && !cancel {
                data.insert_temp(id, draft);
            } else {
                data.remove::<LayerOutlineDraft>(id);
            }
        });
    }
    /// Reorder document rows in painter order. A folder carries descendants;
    /// a mask carries its targets. Explicit parent/mask IDs are preserved.
    pub(crate) fn reorder_timeline_layer(&mut self, layer_id: &str, before: Option<&str>) {
        let layers = &self.symbol().timeline.layers;
        let Some(index) = layers.iter().position(|layer| layer.id == layer_id) else {
            return;
        };
        let group = group_indices(layers, index);
        let moved_ids: HashSet<_> = group
            .iter()
            .map(|index| layers[*index].id.clone())
            .collect();
        if before.is_some_and(|id| moved_ids.contains(id)) {
            return;
        }
        let moving: Vec<_> = layers
            .iter()
            .filter(|layer| moved_ids.contains(&layer.id))
            .cloned()
            .collect();
        let mut remaining: Vec<_> = layers
            .iter()
            .filter(|layer| !moved_ids.contains(&layer.id))
            .cloned()
            .collect();
        let insertion = if let Some(id) = before {
            let Some(index) = remaining.iter().position(|layer| layer.id == id) else {
                return;
            };
            index
        } else {
            remaining.len()
        };
        remaining.splice(insertion..insertion, moving);
        if &remaining == layers {
            return;
        }
        let selected = layer_id.to_owned();
        self.layer_change(move |project, symbol| {
            project.symbol_mut(symbol).unwrap().timeline.layers = remaining
        });
        if let Some(index) = self
            .symbol()
            .timeline
            .layers
            .iter()
            .position(|layer| layer.id == selected)
        {
            self.layer = index;
        }
    }

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
                for group in &mut layer.properties.groups {
                    group.id = new_id("group");
                    for member in &mut group.members {
                        *member = mapping[member].clone();
                    }
                }
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
        crate::ui_icons::menu_contents(ui, 280., |ui| self.timeline_layer_commands(ui, index))
    }
    /// Layer (right-click) menu: the design §8 commands in order, then the editor's further layer
    /// commands under "More".
    fn timeline_layer_commands(&mut self, ui: &mut egui::Ui, index: usize) -> bool {
        use crate::ui_icons::{menu_item, menu_item_enabled, menu_toggle};
        use neoflash_ui::components as nf;
        let mut rename = false;
        let layer = self.symbol().timeline.layers[index].clone();
        let folder = layer.properties.role == LayerRole::Folder;
        let ctx = ui.ctx().clone();
        macro_rules! action {
            ($label:expr,$body:expr) => {
                if menu_item(ui, $label, "").clicked() {
                    $body;
                    ui.close();
                }
            };
        }
        action!(
            "Show all",
            self.layer_change(
                |p, id| for l in &mut p.symbol_mut(id).unwrap().timeline.layers {
                    l.visible = true;
                }
            )
        );
        action!(
            "Lock others",
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
            "Hide others",
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
        nf::menu_separator(ui);
        action!(
            "Insert layer",
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
        if menu_item_enabled(ui, !folder, "Delete layer", "").clicked() {
            self.delete_timeline_layer(index);
            ui.close();
        }
        action!("Duplicate layers", {
            self.copy_timeline_layers(&ctx, index);
            let copy = ctx
                .data(|data| data.get_temp::<LayerClipboard>(self.layer_clipboard_id()))
                .unwrap();
            self.paste_timeline_layers(copy, index);
        });
        action!("Cut layers", {
            self.copy_timeline_layers(&ctx, index);
            self.delete_timeline_layer(index);
        });
        action!("Copy layers", self.copy_timeline_layers(&ctx, index));
        let clipboard = ctx.data(|data| data.get_temp::<LayerClipboard>(self.layer_clipboard_id()));
        if menu_item_enabled(ui, clipboard.is_some(), "Paste layers", "").clicked() {
            if let Some(clipboard) = clipboard {
                self.paste_timeline_layers(clipboard, index);
            }
            ui.close();
        }
        nf::menu_separator(ui);
        let mut guide = layer.properties.role == LayerRole::Guide;
        if ui
            .add_enabled_ui(!folder, |ui| menu_toggle(ui, &mut guide, "Guide", ""))
            .inner
            .changed()
        {
            let role = if guide {
                LayerRole::Guide
            } else {
                LayerRole::Normal
            };
            self.set_layer_role(index, role);
            ui.close();
        }
        Self::unavailable_item(ui, "Add classic motion guide", "");
        let mut mask = layer.properties.role == LayerRole::Mask;
        if ui
            .add_enabled_ui(!folder, |ui| menu_toggle(ui, &mut mask, "Mask", ""))
            .inner
            .changed()
        {
            self.set_layer_role(
                index,
                if mask {
                    LayerRole::Mask
                } else {
                    LayerRole::Normal
                },
            );
            ui.close();
        }
        let mut masking = self.show_layer_masking(&ctx);
        if menu_toggle(ui, &mut masking, "Show masking", "").changed() {
            self.set_layer_masking(&ctx, masking);
        }
        nf::menu_separator(ui);
        action!("Insert folder", self.insert_layer_folder(index));
        if menu_item_enabled(ui, folder, "Delete folder", "").clicked() {
            self.delete_timeline_layer(index);
            ui.close();
        }
        if menu_item_enabled(
            ui,
            folder && layer.properties.collapsed,
            "Expand folder",
            "",
        )
        .clicked()
        {
            self.layer_change(|p, id| {
                p.symbol_mut(id).unwrap().timeline.layers[index]
                    .properties
                    .collapsed = false
            });
            ui.close();
        }
        action!(
            "Collapse all folders",
            self.layer_change(
                |p, id| for l in &mut p.symbol_mut(id).unwrap().timeline.layers {
                    if l.properties.role == LayerRole::Folder {
                        l.properties.collapsed = true;
                    }
                }
            )
        );
        nf::menu_separator(ui);
        Self::planned_menu_item(ui, "Layer menu ▸ Properties…", "");
        nf::menu_separator(ui);
        nf::menu_section(ui, "More");
        action!("Rename layer", rename = true);
        action!(
            "Unlock all",
            self.layer_change(
                |p, id| for l in &mut p.symbol_mut(id).unwrap().timeline.layers {
                    l.locked = false;
                }
            )
        );
        if menu_item(ui, "Outline color…", "").clicked() {
            self.begin_layer_outline_color(&ctx, &layer);
            ui.close();
        }
        action!(
            "Others transparent",
            self.layer_change(|p, id| {
                let layers = &mut p.symbol_mut(id).unwrap().timeline.layers;
                let selected = group_indices(layers, index);
                for (i, layer) in layers.iter_mut().enumerate() {
                    layer.properties.outline =
                        layer.properties.role != LayerRole::Folder && !selected.contains(&i);
                }
            })
        );
        action!(
            "Show all as solid",
            self.layer_change(
                |p, id| for l in &mut p.symbol_mut(id).unwrap().timeline.layers {
                    l.properties.outline = false;
                }
            )
        );
        if folder
            && menu_item_enabled(ui, !layer.properties.collapsed, "Collapse folder", "").clicked()
        {
            self.layer_change(|p, id| {
                p.symbol_mut(id).unwrap().timeline.layers[index]
                    .properties
                    .collapsed = true
            });
            ui.close();
        }
        action!(
            "Expand all folders",
            self.layer_change(
                |p, id| for l in &mut p.symbol_mut(id).unwrap().timeline.layers {
                    l.properties.collapsed = false;
                }
            )
        );
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
        if menu_item_enabled(ui, can_merge, "Merge with next layer", "")
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
        ui.add_enabled_ui_semantic(contiguous, |ui| {
            crate::ui_icons::menu_submenu(ui, "Convert layers to symbol…", |ui| {
                let name_id = ui.id().with("layer-symbol-name");
                let mut name = ctx
                    .data(|data| data.get_temp::<String>(name_id))
                    .unwrap_or_else(|| format!("{} symbol", layer.name));
                ui.label("Movie Clip name");
                ui.text_edit_singleline(&mut name);
                ctx.data_mut(|data| data.insert_temp(name_id, name.clone()));
                if ui
                    .add_enabled_semantic(
                        !name.trim().is_empty(),
                        egui::Button::new("Create Movie Clip"),
                    )
                    .clicked()
                {
                    self.convert_timeline_layers(index, name.trim().to_owned());
                    ui.close();
                }
            });
        });
        crate::ui_icons::menu_submenu(ui, "Folder and mask…", |ui| {
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
