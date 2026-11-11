//! Timeline layer commands commit a complete, valid document in one undo step.
use crate::app::Editor;
use crate::theme::DesignUi as _;
use eframe::egui;
use neoflash_core::*;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::{LazyLock, Mutex};

type SelectedLayerRows = HashMap<(Id, Id), BTreeSet<Id>>;

static SELECTED_LAYER_ROWS: LazyLock<Mutex<SelectedLayerRows>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

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
                ..Default::default()
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
            .or(layer.properties.motion_guide.as_ref())
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
/// with a mask is Masked; a guide that guides a layer is a Motion guide. The editor has no sound,
/// video or armature layers yet.
pub(crate) fn layer_kind(
    timeline: &Timeline,
    layer: &Layer,
) -> neoflash_ui::components::timeline::LayerKind {
    use neoflash_ui::components::timeline::LayerKind;
    match layer.properties.role {
        LayerRole::Folder if layer.properties.collapsed => LayerKind::FolderClosed,
        LayerRole::Folder => LayerKind::FolderOpen,
        LayerRole::Guide
            if timeline
                .layers
                .iter()
                .any(|l| l.properties.motion_guide.as_ref() == Some(&layer.id)) =>
        {
            LayerKind::MotionGuide
        }
        LayerRole::Guide => LayerKind::Guide,
        LayerRole::Mask => LayerKind::Mask,
        LayerRole::Normal if layer.properties.mask.is_some() => LayerKind::Masked,
        // Keys holding only frame sounds (no artwork) make a Sound layer.
        LayerRole::Normal
            if layer.keyframes.iter().any(|k| k.sound.is_some())
                && layer.keyframes.iter().all(|k| k.elements.is_empty()) =>
        {
            LayerKind::Sound
        }
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
        .or(current.properties.motion_guide.as_ref())
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

/// R-040: the 14 Flash layer blend modes in the 6 approved menu groups.
pub(crate) const BLEND_MODE_GROUPS: [&[BlendMode]; 6] = [
    &[BlendMode::Normal, BlendMode::Layer],
    &[BlendMode::Darken, BlendMode::Multiply],
    &[BlendMode::Lighten, BlendMode::Screen, BlendMode::Add],
    &[BlendMode::Overlay, BlendMode::Hardlight],
    &[
        BlendMode::Difference,
        BlendMode::Subtract,
        BlendMode::Invert,
    ],
    &[BlendMode::Alpha, BlendMode::Erase],
];

/// Nearest ancestor folder's non-`Normal` blend mode (`R-040`), if any.
pub(crate) fn inherited_folder_blend_mode(timeline: &Timeline, layer: &Layer) -> Option<BlendMode> {
    let mut seen = HashSet::from([layer.id.as_str()]);
    let mut current = layer;
    while let Some(parent_id) = current.properties.parent.as_deref() {
        if !seen.insert(parent_id) {
            break;
        }
        let Some(parent) = timeline.layers.iter().find(|l| l.id == parent_id) else {
            break;
        };
        if !parent.properties.blend_mode.is_normal() {
            return Some(parent.properties.blend_mode);
        }
        current = parent;
    }
    None
}

/// Select / submenu label for a layer's blend mode (`R-040`): shows
/// `Blending: Inherited (<mode>)` when a parent folder has a non-`Normal` blend mode and the
/// layer's own mode is `Normal`.
pub(crate) fn layer_blending_select_label(
    own_mode: BlendMode,
    inherited_folder_mode: Option<BlendMode>,
) -> String {
    if own_mode.is_normal()
        && let Some(inherited) = inherited_folder_mode.filter(|m| !m.is_normal())
    {
        format!("Blending: Inherited ({})", inherited.label())
    } else {
        format!("Blending: {}", own_mode.label())
    }
}

/// Populate the 6-group 14-mode `R-040` blend-mode menu.
pub(crate) fn populate_blend_mode_menu(
    ui: &mut egui::Ui,
    current: BlendMode,
    mut on_pick: impl FnMut(BlendMode),
) {
    for (gi, group) in BLEND_MODE_GROUPS.iter().enumerate() {
        if gi > 0 {
            neoflash_ui::components::menu_separator(ui);
        }
        for &mode in *group {
            if ui
                .add(neoflash_ui::components::MenuItem::new(mode.label()).checked(current == mode))
                .clicked()
            {
                on_pick(mode);
                ui.close();
            }
        }
    }
}

/// Populate the 3-mode `R-048` / `R-049` mask-mode menu (`Geometry`, `Alpha`, `Luminance`).
pub(crate) fn populate_mask_mode_menu(
    ui: &mut egui::Ui,
    current: MaskMode,
    mut on_pick: impl FnMut(MaskMode),
) {
    for mode in MaskMode::ALL {
        if ui
            .add(neoflash_ui::components::MenuItem::new(mode.label()).checked(current == mode))
            .clicked()
        {
            on_pick(mode);
            ui.close();
        }
    }
}

fn folder_is_descendant_of(layers: &[Layer], candidate_folder_id: &str, ancestor_id: &str) -> bool {
    if candidate_folder_id == ancestor_id {
        return true;
    }
    let mut seen = HashSet::from([candidate_folder_id]);
    let mut current = layers.iter().find(|l| l.id == candidate_folder_id);
    while let Some(layer) = current {
        let Some(parent_id) = layer.properties.parent.as_deref() else {
            break;
        };
        if parent_id == ancestor_id {
            return true;
        }
        if !seen.insert(parent_id) {
            break;
        }
        current = layers.iter().find(|l| l.id == parent_id);
    }
    false
}

#[derive(Clone)]
struct LayerOutlineDraft {
    symbol: Id,
    layer: Id,
    name: String,
    color: egui::Color32,
}

impl Editor {
    fn selected_layers_key(&self) -> (Id, Id) {
        (self.clipboard.session.clone(), self.symbol_id().to_owned())
    }

    pub(crate) fn clear_selected_layer_rows(&mut self) {
        if let Ok(mut guard) = SELECTED_LAYER_ROWS.lock() {
            guard.remove(&self.selected_layers_key());
        }
    }

    pub(crate) fn selected_layer_indices(&self) -> Vec<usize> {
        let layers = &self.symbol().timeline.layers;
        let extra = SELECTED_LAYER_ROWS
            .lock()
            .ok()
            .and_then(|guard| guard.get(&self.selected_layers_key()).cloned())
            .unwrap_or_default();
        let mut indices: Vec<usize> = layers
            .iter()
            .enumerate()
            .filter_map(|(i, l)| (i == self.layer || extra.contains(&l.id)).then_some(i))
            .collect();
        if indices.is_empty() && self.layer < layers.len() {
            indices.push(self.layer);
        }
        indices
    }

    pub(crate) fn is_layer_row_selected(&self, index: usize) -> bool {
        if index == self.layer {
            return true;
        }
        let Some(layer) = self.symbol().timeline.layers.get(index) else {
            return false;
        };
        SELECTED_LAYER_ROWS
            .lock()
            .ok()
            .and_then(|guard| {
                guard
                    .get(&self.selected_layers_key())
                    .map(|set| set.contains(&layer.id))
            })
            .unwrap_or(false)
    }

    pub(crate) fn target_layer_indices(&self, index: usize) -> Vec<usize> {
        if self.is_layer_row_selected(index) {
            let sel = self.selected_layer_indices();
            if !sel.is_empty() {
                return sel;
            }
        }
        vec![index]
    }

    /// Select a timeline layer row with optional `Shift` (visual range) or `⌘` / `Ctrl` (toggle).
    pub(crate) fn select_layer_row(&mut self, index: usize, shift: bool, command: bool) {
        let layers = &self.symbol().timeline.layers;
        if index >= layers.len() {
            return;
        }
        let previous_layer = self.layer;
        let key = self.selected_layers_key();
        if shift {
            let rows = visible_layer_indices(&self.symbol().timeline);
            let anchor_pos = rows.iter().position(|&i| i == self.layer);
            let target_pos = rows.iter().position(|&i| i == index);
            let mut set = SELECTED_LAYER_ROWS
                .lock()
                .ok()
                .and_then(|guard| guard.get(&key).cloned())
                .unwrap_or_default();
            if let (Some(a), Some(b)) = (anchor_pos, target_pos) {
                for &row_idx in &rows[a.min(b)..=a.max(b)] {
                    set.insert(layers[row_idx].id.clone());
                }
            } else {
                for layer in &layers[self.layer.min(index)..=self.layer.max(index)] {
                    set.insert(layer.id.clone());
                }
            }
            if let Ok(mut guard) = SELECTED_LAYER_ROWS.lock() {
                guard.insert(key, set);
            }
            self.layer = index;
        } else if command {
            let mut set = SELECTED_LAYER_ROWS
                .lock()
                .ok()
                .and_then(|guard| guard.get(&key).cloned())
                .unwrap_or_default();
            if let Some(current) = layers.get(self.layer) {
                set.insert(current.id.clone());
            }
            let clicked_id = layers[index].id.clone();
            if set.contains(&clicked_id) && set.len() > 1 {
                set.remove(&clicked_id);
                if self.layer == index
                    && let Some(next_idx) = layers.iter().position(|l| set.contains(&l.id))
                {
                    self.layer = next_idx;
                }
            } else {
                set.insert(clicked_id);
                self.layer = index;
            }
            if let Ok(mut guard) = SELECTED_LAYER_ROWS.lock() {
                guard.insert(key, set);
            }
        } else {
            if let Ok(mut guard) = SELECTED_LAYER_ROWS.lock() {
                guard.remove(&key);
            }
            self.layer = index;
        }
        if self.layer != previous_layer {
            self.clear_timeline_frame_selection();
        }
    }

    pub(crate) fn toggle_selected_layers_visibility(&mut self, index: usize) {
        let Some(target_visible) = self.symbol().timeline.layers.get(index).map(|l| !l.visible)
        else {
            return;
        };
        let targets = self.target_layer_indices(index);
        self.layer_change(move |p, id| {
            let layers = &mut p.symbol_mut(id).unwrap().timeline.layers;
            for i in targets {
                if let Some(l) = layers.get_mut(i) {
                    l.visible = target_visible;
                }
            }
        });
    }

    pub(crate) fn toggle_selected_layers_lock(&mut self, index: usize) {
        let Some(target_locked) = self.symbol().timeline.layers.get(index).map(|l| !l.locked)
        else {
            return;
        };
        let targets = self.target_layer_indices(index);
        self.layer_change(move |p, id| {
            let layers = &mut p.symbol_mut(id).unwrap().timeline.layers;
            for i in targets {
                if let Some(l) = layers.get_mut(i)
                    && l.properties.role != LayerRole::Folder
                {
                    l.locked = target_locked;
                }
            }
        });
    }

    pub(crate) fn toggle_selected_layers_outline(&mut self, index: usize) {
        let targets = self.target_layer_indices(index);
        if targets.len() <= 1 {
            if let Some(layer) = self.symbol().timeline.layers.get(index).cloned() {
                self.toggle_layer_outline(&layer);
            }
            return;
        }
        let Some(target_outline) = self
            .symbol()
            .timeline
            .layers
            .get(index)
            .map(|l| !l.properties.outline)
        else {
            return;
        };
        self.layer_change(move |p, id| {
            let layers = &mut p.symbol_mut(id).unwrap().timeline.layers;
            for i in targets {
                if let Some(l) = layers.get_mut(i)
                    && l.properties.role != LayerRole::Folder
                {
                    l.properties.outline = target_outline;
                }
            }
        });
    }

    /// Reparent `layer_id` (and any other currently selected layer rows when `layer_id` is in the
    /// multi-selection) into `folder_id` (`Some(fid)`) or out to root (`None`) in one Undo step.
    pub(crate) fn reparent_timeline_layer_to_folder(
        &mut self,
        layer_id: &str,
        folder_id: Option<&str>,
    ) {
        let layers = &self.symbol().timeline.layers;
        let Some(primary_idx) = layers.iter().position(|l| l.id == layer_id) else {
            return;
        };
        if let Some(fid) = folder_id
            && !layers
                .iter()
                .any(|l| l.id == fid && l.properties.role == LayerRole::Folder)
        {
            return;
        }
        let targets = self.target_layer_indices(primary_idx);
        let target_ids: Vec<Id> = targets
            .into_iter()
            .filter_map(|i| layers.get(i).map(|l| l.id.clone()))
            .collect();
        let new_parent = folder_id.map(str::to_owned);
        self.layer_change(move |project, symbol| {
            let layers = &mut project.symbol_mut(symbol).unwrap().timeline.layers;
            if let Some(fid) = new_parent.as_deref() {
                for tid in &target_ids {
                    if folder_is_descendant_of(layers, fid, tid) {
                        return;
                    }
                }
                if let Some(folder) = layers.iter_mut().find(|l| l.id == fid) {
                    folder.properties.collapsed = false;
                }
            }
            let target_set: HashSet<Id> = target_ids.into_iter().collect();
            for layer in layers.iter_mut() {
                let linked_to_target = layer
                    .properties
                    .mask
                    .as_ref()
                    .is_some_and(|m| target_set.contains(m))
                    || layer
                        .properties
                        .motion_guide
                        .as_ref()
                        .is_some_and(|g| target_set.contains(g));
                if target_set.contains(&layer.id) || linked_to_target {
                    layer.properties.parent = new_parent.clone();
                    if layer
                        .properties
                        .mask
                        .as_ref()
                        .is_some_and(|m| !target_set.contains(m))
                    {
                        layer.properties.mask = None;
                    }
                    if layer
                        .properties
                        .motion_guide
                        .as_ref()
                        .is_some_and(|g| !target_set.contains(g))
                    {
                        layer.properties.motion_guide = None;
                    }
                }
            }
        });
    }

    /// Reorder `layer_id` in visual row order and optionally reparent it to `new_parent` in one
    /// Undo step (`new_parent = Some(Some(folder_id))` moves into a folder; `Some(None)` moves to
    /// root; `None` keeps existing parent).
    pub(crate) fn drop_timeline_layer_visual(
        &mut self,
        layer_id: &str,
        before: Option<&str>,
        new_parent: Option<Option<&str>>,
    ) {
        let layers = &self.symbol().timeline.layers;
        let Some(index) = layers.iter().position(|layer| layer.id == layer_id) else {
            return;
        };
        let targets = self.target_layer_indices(index);
        let moved: HashSet<_> = targets
            .into_iter()
            .flat_map(|ti| group_indices(layers, ti))
            .collect();
        let insertion = if let Some(target) = before {
            let Some(target) = layers.iter().position(|layer| layer.id == target) else {
                return;
            };
            if moved.contains(&target) {
                if let Some(parent_opt) = new_parent {
                    self.reparent_timeline_layer_to_folder(layer_id, parent_opt);
                }
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
        let parent_override = new_parent.map(|opt| opt.map(str::to_owned));
        self.reorder_and_reparent_timeline_layer(layer_id, insertion.as_deref(), parent_override);
    }
    /// A drop above a visual row belongs after that row's group in painter
    /// order. A drop below every visual row belongs at painter index zero.
    #[cfg_attr(not(test), allow(dead_code))]
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
                let mut rgba = draft.color.to_srgba_unmultiplied();
                let (changed, _) = crate::paint_controls::rgba_color_popover(
                    ui,
                    id.with("outline-rgba"),
                    "Outline colour",
                    &mut rgba,
                );
                if changed {
                    draft.color =
                        egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], 255);
                }
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
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn reorder_timeline_layer(&mut self, layer_id: &str, before: Option<&str>) {
        self.reorder_and_reparent_timeline_layer(layer_id, before, None);
    }

    fn reorder_and_reparent_timeline_layer(
        &mut self,
        layer_id: &str,
        before: Option<&str>,
        new_parent: Option<Option<Id>>,
    ) {
        let layers = &self.symbol().timeline.layers;
        let Some(index) = layers.iter().position(|layer| layer.id == layer_id) else {
            return;
        };
        let targets = self.target_layer_indices(index);
        let direct_target_ids: HashSet<Id> = targets
            .iter()
            .filter_map(|&ti| layers.get(ti).map(|l| l.id.clone()))
            .collect();
        let moved_ids: HashSet<_> = targets
            .into_iter()
            .flat_map(|ti| group_indices(layers, ti))
            .map(|i| layers[i].id.clone())
            .collect();
        if before.is_some_and(|id| moved_ids.contains(id)) {
            if let Some(parent_opt) = new_parent {
                self.reparent_timeline_layer_to_folder(layer_id, parent_opt.as_deref());
            }
            return;
        }
        let mut moving: Vec<_> = layers
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
        if let Some(parent_opt) = &new_parent {
            if let Some(fid) = parent_opt.as_deref() {
                for tid in &direct_target_ids {
                    if folder_is_descendant_of(layers, fid, tid) {
                        return;
                    }
                }
                if let Some(folder) = remaining.iter_mut().find(|l| l.id == fid) {
                    folder.properties.collapsed = false;
                }
            }
            for layer in &mut moving {
                let linked_to_direct = layer
                    .properties
                    .mask
                    .as_ref()
                    .is_some_and(|m| direct_target_ids.contains(m))
                    || layer
                        .properties
                        .motion_guide
                        .as_ref()
                        .is_some_and(|g| direct_target_ids.contains(g));
                if direct_target_ids.contains(&layer.id) || linked_to_direct {
                    layer.properties.parent = parent_opt.clone();
                    if layer
                        .properties
                        .mask
                        .as_ref()
                        .is_some_and(|m| !moved_ids.contains(m))
                    {
                        layer.properties.mask = None;
                    }
                    if layer
                        .properties
                        .motion_guide
                        .as_ref()
                        .is_some_and(|g| !moved_ids.contains(g))
                    {
                        layer.properties.motion_guide = None;
                    }
                }
            }
        }
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
        candidate.prune_motion_guides();
        candidate.upgrade_format_for_features();
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
        let targets = self.target_layer_indices(index);
        self.clear_selected_layer_rows();
        self.layer_change(move |project, symbol| {
            let timeline = &mut project.symbol_mut(symbol).unwrap().timeline;
            let mut removed = HashSet::new();
            for &target_idx in &targets {
                if target_idx < timeline.layers.len() {
                    for i in group_indices(&timeline.layers, target_idx) {
                        removed.insert(timeline.layers[i].id.clone());
                    }
                }
            }
            let parent = timeline
                .layers
                .get(index)
                .and_then(|l| l.properties.parent.clone())
                .filter(|id| !removed.contains(id));
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
                if layer
                    .properties
                    .linked_layer
                    .as_ref()
                    .is_some_and(|id| removed.contains(id))
                {
                    layer.properties.linked_layer = None;
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
        let targets = self.target_layer_indices(index);
        let mut indices = BTreeSet::new();
        for target_idx in targets {
            if target_idx < timeline.layers.len() {
                for i in group_indices(&timeline.layers, target_idx) {
                    indices.insert(i);
                }
            }
        }
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
                layer.properties.motion_guide = layer
                    .properties
                    .motion_guide
                    .as_ref()
                    .map(|id| mapping.get(id).cloned().unwrap_or_else(|| id.clone()));
                layer.properties.linked_layer = layer
                    .properties
                    .linked_layer
                    .as_ref()
                    .and_then(|id| mapping.get(id).cloned());
                for group in &mut layer.properties.groups {
                    group.id = new_id("group");
                    for member in &mut group.members {
                        *member = mapping[member].clone();
                    }
                }
                for key in &mut layer.keyframes {
                    key.remap_motion_element_ids(&mapping);
                    for element in &mut key.elements {
                        element.id = mapping[&element.id].clone();
                    }
                }
            }
            timeline.frame_count = timeline.frame_count.max(clipboard.frames);
            timeline.layers.splice(index + 1..index + 1, layers);
        });
    }
    fn can_merge_timeline_layer(&self, index: usize) -> bool {
        let timeline = &self.symbol().timeline;
        let Some(layer) = timeline.layers.get(index) else {
            return false;
        };
        layer.properties.role == LayerRole::Normal
            && !layer_effectively_locked(timeline, layer)
            && timeline.frame_count <= 10000
            && timeline.layers.get(index + 1).is_some_and(|next| {
                next.properties.role == LayerRole::Normal
                    && !layer_effectively_locked(timeline, next)
                    && next.visible == layer.visible
                    && next.properties.mask == layer.properties.mask
                    && next.properties.parent == layer.properties.parent
                    && next.keyframes.iter().all(|key| key.label.is_empty())
            })
    }

    /// Preserve authored poses and frame sounds when combining two adjacent layers. A single
    /// keyframe cannot carry two sound commands, and adding keys must not truncate a Stream span.
    fn merge_timeline_layer(&mut self, index: usize) {
        self.finish_edits();
        if !self.can_merge_timeline_layer(index) {
            return;
        }
        let fps = f64::from(self.project.fps);
        let timeline = &self.symbol().timeline;
        let first = &timeline.layers[index];
        let next = &timeline.layers[index + 1];
        let end = first
            .properties
            .end_frame
            .unwrap_or(timeline.frame_count)
            .max(next.properties.end_frame.unwrap_or(timeline.frame_count));
        let bake = first
            .keyframes
            .iter()
            .chain(&next.keyframes)
            .any(|key| key.tween.is_some());
        let mut frames: std::collections::BTreeSet<u32> = if bake {
            (0..end).collect()
        } else {
            first
                .keyframes
                .iter()
                .chain(&next.keyframes)
                .map(|key| key.frame)
                .collect()
        };
        // A layer that ends earlier must disappear even when neither layer has a tween.
        for layer in [first, next] {
            if let Some(boundary) = layer
                .properties
                .end_frame
                .filter(|boundary| *boundary < end)
            {
                frames.insert(boundary);
            }
        }
        for key in &first.keyframes {
            if key.sound.is_some()
                && next
                    .keyframes
                    .iter()
                    .any(|other| other.frame == key.frame && other.sound.is_some())
            {
                self.message("Cannot merge layers with two sounds on the same frame. Keep the sounds on separate layers.");
                return;
            }
            if key.video.is_some()
                && next
                    .keyframes
                    .iter()
                    .any(|other| other.frame == key.frame && other.video.is_some())
            {
                self.message("Cannot merge layers with two video clips on the same frame. Keep the videos on separate layers.");
                return;
            }
        }
        let changes_stream_span = [first, next].into_iter().any(|layer| {
            layer.keyframes.iter().any(|key| {
                if !key
                    .sound
                    .as_ref()
                    .is_some_and(|sound| sound.sync == SoundSync::Stream)
                {
                    return false;
                }
                let merged_end = frames
                    .range((
                        std::ops::Bound::Excluded(key.frame),
                        std::ops::Bound::Unbounded,
                    ))
                    .next()
                    .copied()
                    .unwrap_or(end);
                merged_end != sound_span_end(layer, key, timeline.frame_count)
            })
        });
        if changes_stream_span {
            self.message("Cannot merge these layers without shortening a Stream sound. Keep the sound on a separate layer.");
            return;
        }
        let mut merged = first.clone();
        merged.properties.end_frame = (end < timeline.frame_count).then_some(end);
        // Guide transforms are baked below using the original timeline, before either layer moves.
        merged.properties.motion_guide = None;
        let first_blend = first.properties.blend_mode;
        let next_blend = next.properties.blend_mode;
        let differing_layer_blends = first_blend != next_blend;
        if differing_layer_blends {
            merged.properties.blend_mode = BlendMode::Normal;
        }
        let mut group_ids: HashSet<_> = merged
            .properties
            .groups
            .iter()
            .map(|group| group.id.clone())
            .collect();
        for mut group in next.properties.groups.clone() {
            if !group_ids.insert(group.id.clone()) {
                group.id = new_id("group");
                group_ids.insert(group.id.clone());
            }
            merged.properties.groups.push(group);
        }
        merged.keyframes = frames
            .into_iter()
            .map(|frame| {
                let mut elements = sample_layer_in_at_fps(timeline, first, frame, fps);
                if differing_layer_blends && !first_blend.is_normal() {
                    for el in &mut elements {
                        if el.blend_mode.is_normal() {
                            el.blend_mode = first_blend;
                        }
                    }
                }
                let mut next_elements = sample_layer_in_at_fps(timeline, next, frame, fps);
                if differing_layer_blends && !next_blend.is_normal() {
                    for el in &mut next_elements {
                        if el.blend_mode.is_normal() {
                            el.blend_mode = next_blend;
                        }
                    }
                }
                elements.extend(next_elements);
                let first_key = first.keyframes.iter().find(|key| key.frame == frame);
                let next_key = next.keyframes.iter().find(|key| key.frame == frame);
                let mut shape_hints = first_key
                    .map(|key| key.shape_hints.clone())
                    .unwrap_or_default();
                if let Some(nk) = next_key {
                    for hint in &nk.shape_hints {
                        if !shape_hints.iter().any(|h| h.label == hint.label) {
                            shape_hints.push(*hint);
                        }
                    }
                }
                Keyframe {
                    frame,
                    elements,
                    label: first_key.map(|key| key.label.clone()).unwrap_or_default(),
                    sound: first_key
                        .and_then(|key| key.sound.clone())
                        .or_else(|| next_key.and_then(|key| key.sound.clone())),
                    video: first_key
                        .and_then(|key| key.video.clone())
                        .or_else(|| next_key.and_then(|key| key.video.clone())),
                    shape_hints,
                    ..Default::default()
                }
            })
            .collect();
        merged.properties.linked_layer = None;
        let removed_id = next.id.clone();
        let merged_id = merged.id.clone();
        self.layer_change(move |project, symbol| {
            let timeline = &mut project.symbol_mut(symbol).unwrap().timeline;
            timeline.layers[index] = merged;
            timeline.layers.remove(index + 1);
            for layer in &mut timeline.layers {
                if layer
                    .properties
                    .linked_layer
                    .as_deref()
                    .is_some_and(|p| p == removed_id || p == merged_id)
                {
                    layer.properties.linked_layer = None;
                }
            }
        });
    }
    pub(crate) fn set_layer_role(&mut self, index: usize, role: LayerRole) {
        self.layer_change(|project, symbol| {
            let timeline = &mut project.symbol_mut(symbol).unwrap().timeline;
            let id = timeline.layers[index].id.clone();
            timeline.layers[index].properties.role = role;
            timeline.layers[index].properties.mask = None;
            if role != LayerRole::Mask {
                timeline.layers[index].properties.mask_mode = MaskMode::Geometry;
                timeline.layers[index].properties.mask_invert = false;
                for layer in &mut timeline.layers {
                    if layer.properties.mask.as_ref() == Some(&id) {
                        layer.properties.mask = None;
                    }
                }
            } else {
                // Rows are displayed front-to-back, the reverse of serialized paint order.
                // Stay within this folder and never steal another mask/guide's child.
                let parent = timeline.layers[index].properties.parent.clone();
                let rows = visible_layer_indices(timeline);
                let target = rows
                    .iter()
                    .skip_while(|&&row| row != index)
                    .skip(1)
                    .copied()
                    .find(|&row| {
                        let properties = &timeline.layers[row].properties;
                        properties.role == LayerRole::Normal
                            && properties.parent == parent
                            && properties.mask.is_none()
                            && properties.motion_guide.is_none()
                    });
                if let Some(target) = target {
                    timeline.layers[target].properties.mask = Some(id);
                }
            }
        });
    }
    pub(crate) fn set_layer_mask_mode(&mut self, index: usize, mask_mode: MaskMode) {
        self.layer_change(move |project, symbol| {
            let timeline = &mut project.symbol_mut(symbol).unwrap().timeline;
            if let Some(layer) = timeline.layers.get_mut(index)
                && layer.properties.role == LayerRole::Mask
            {
                layer.properties.mask_mode = mask_mode;
            }
        });
    }
    pub(crate) fn set_layer_mask_invert(&mut self, index: usize, mask_invert: bool) {
        self.layer_change(move |project, symbol| {
            let timeline = &mut project.symbol_mut(symbol).unwrap().timeline;
            if let Some(layer) = timeline.layers.get_mut(index)
                && layer.properties.role == LayerRole::Mask
            {
                layer.properties.mask_invert = mask_invert;
            }
        });
    }
    pub(crate) fn lock_other_layers(&mut self, index: usize) {
        let targets = self.target_layer_indices(index);
        self.layer_change(move |p, id| {
            let layers = &mut p.symbol_mut(id).unwrap().timeline.layers;
            let selected: HashSet<usize> = targets
                .into_iter()
                .flat_map(|ti| group_indices(layers, ti))
                .collect();
            for (i, l) in layers.iter_mut().enumerate() {
                l.locked = !selected.contains(&i) && l.properties.role != LayerRole::Folder;
            }
        });
    }
    pub(crate) fn hide_other_layers(&mut self, index: usize) {
        let targets = self.target_layer_indices(index);
        self.layer_change(move |p, id| {
            let layers = &mut p.symbol_mut(id).unwrap().timeline.layers;
            let selected: HashSet<usize> = targets
                .into_iter()
                .flat_map(|ti| group_indices(layers, ti))
                .collect();
            let keep_masks: HashSet<Id> = selected
                .iter()
                .filter_map(|&i| layers[i].properties.mask.clone())
                .collect();
            for (i, l) in layers.iter_mut().enumerate() {
                l.visible = selected.contains(&i)
                    || l.properties.role == LayerRole::Folder
                    || keep_masks.contains(&l.id);
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
        action!("Lock others", self.lock_other_layers(index));
        action!("Hide others", self.hide_other_layers(index));
        let mut solo = self.is_layer_soloed(index);
        if menu_toggle(ui, &mut solo, "Solo", "")
            .on_hover_text("Solo — show only soloed layers (⌥-click: only this one)")
            .changed()
        {
            let exclusive_alt = ui.input(|i| i.modifiers.alt);
            self.toggle_layer_solo(index, exclusive_alt);
            ui.close();
        }
        if menu_item_enabled(ui, self.has_any_solo_layers(), "Clear all solo", "").clicked() {
            self.clear_all_solo_layers();
            ui.close();
        }
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
        if menu_item_enabled(
            ui,
            self.can_add_motion_guide_to(index),
            "Add classic motion guide",
            "",
        )
        .clicked()
        {
            self.layer = index;
            self.add_motion_guide();
            ui.close();
        }
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
        if layer.properties.role == LayerRole::Mask {
            let mode_label = format!("Mask mode: {}", layer.properties.mask_mode.label());
            crate::ui_icons::menu_submenu(ui, &mode_label, |ui| {
                let mut picked = None;
                populate_mask_mode_menu(ui, layer.properties.mask_mode, |mode| {
                    picked = Some(mode);
                });
                if let Some(mode) = picked {
                    self.set_layer_mask_mode(index, mode);
                }
            });
            let mut invert = layer.properties.mask_invert;
            if menu_toggle(ui, &mut invert, "Invert mask", "").changed() {
                self.set_layer_mask_invert(index, invert);
                ui.close();
            }
        }
        let mut masking = self.show_layer_masking(&ctx);
        if menu_toggle(ui, &mut masking, "Show masking", "").changed() {
            self.set_layer_masking(&ctx, masking);
        }
        let non_guide = layer.properties.role != LayerRole::Guide;
        let mut sync_timeline = layer.properties.sync_timeline;
        if ui
            .add_enabled_ui(non_guide, |ui| {
                menu_toggle(ui, &mut sync_timeline, "Sync layer to timeline", "")
            })
            .inner
            .changed()
        {
            self.set_layer_sync_timeline(index, sync_timeline);
            ui.close();
        }
        ui.add_enabled_ui_semantic(non_guide, |ui| {
            let inherited = inherited_folder_blend_mode(&self.symbol().timeline, &layer);
            let blend_label = layer_blending_select_label(layer.properties.blend_mode, inherited);
            crate::ui_icons::menu_submenu(ui, &blend_label, |ui| {
                let mut picked = None;
                populate_blend_mode_menu(ui, layer.properties.blend_mode, |mode| {
                    picked = Some(mode);
                });
                if let Some(mode) = picked {
                    self.set_layer_blend_mode(index, mode);
                }
            });
        });
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
        action!("Properties…", self.open_layer_properties(&ctx, index));
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
        if menu_item_enabled(
            ui,
            self.can_merge_timeline_layer(index),
            "Merge with next layer",
            "",
        )
        .on_hover_text("Preserve both layers’ animation; tweened frames are sampled into keys")
        .clicked()
        {
            self.merge_timeline_layer(index);
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
            if layer.properties.role != LayerRole::Guide {
                ui.checkbox(
                    &mut draft.properties.sync_timeline,
                    "Sync layer to timeline",
                );
                egui::ComboBox::from_id_salt("layer-blend-mode")
                    .selected_text(format!("Blending: {}", draft.properties.blend_mode.label()))
                    .show_ui(ui, |ui| {
                        for mode in BlendMode::ALL {
                            ui.selectable_value(
                                &mut draft.properties.blend_mode,
                                mode,
                                mode.label(),
                            );
                        }
                    });
            }
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
            if layer.properties.role == LayerRole::Mask {
                egui::ComboBox::from_id_salt("layer-mask-mode")
                    .selected_text(format!("Mask mode: {}", draft.properties.mask_mode.label()))
                    .show_ui(ui, |ui| {
                        for mode in MaskMode::ALL {
                            ui.selectable_value(
                                &mut draft.properties.mask_mode,
                                mode,
                                mode.label(),
                            );
                        }
                    });
                ui.checkbox(&mut draft.properties.mask_invert, "Invert mask");
            }
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
                ..Element::default()
            });
            timeline.layers.retain(|l| !selected.contains(&l.id));
            timeline.layers.insert(index, replacement);
            project.symbols.push(Symbol {
                id: symbol_id,
                name,
                folder: String::new(),
                kind: SymbolKind::MovieClip,
                timeline: Timeline {
                    guides: vec![],
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

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(id: &str, x: f64) -> Element {
        Element {
            id: id.into(),
            transform: Transform {
                x,
                ..Default::default()
            },
            kind: ElementKind::Rectangle {
                width: 10.,
                height: 10.,
                fill: [0, 0, 0, 255],
                stroke: [0, 0, 0, 0],
                stroke_width: 0.,
            },
            ..Default::default()
        }
    }

    fn sound_project() -> Project {
        let mut project = Project::blank();
        project.format_version = 6;
        project.resources.push(
            ResourceUnit::from_bytes("sound", "sound.wav", "audio/wav", b"RIFF0000WAVE").unwrap(),
        );
        let timeline = &mut project.symbols[0].timeline;
        timeline.frame_count = 6;
        timeline
            .layers
            .push(blank_layer("Second".into(), LayerRole::Normal, None));
        project
    }

    fn sound_trace(project: &Project) -> Vec<Vec<SoundCommand>> {
        let mut playback = AuthoredPlayback::new(project, &project.root, 0);
        let mut trace = vec![playback.drain_sound_commands()];
        for _ in 1..project.symbols[0].timeline.frame_count {
            playback.tick(project);
            trace.push(playback.drain_sound_commands());
        }
        trace
    }

    #[test]
    fn merging_layers_preserves_sound_and_rejects_unrepresentable_sound_spans() {
        let ctx = egui::Context::default();
        let mut project = sound_project();
        let timeline = &mut project.symbols[0].timeline;
        timeline.layers[0].keyframes = vec![
            Keyframe {
                elements: vec![shape("moving", 0.)],
                tween: Some(Easing::Linear),
                sound: Some(FrameSound::new("sound")),
                ..Default::default()
            },
            Keyframe {
                frame: 5,
                elements: vec![shape("moving", 100.)],
                sound: Some(FrameSound {
                    sync: SoundSync::Stop,
                    ..FrameSound::new("sound")
                }),
                ..Default::default()
            },
        ];
        timeline.layers[1].keyframes.push(Keyframe {
            frame: 2,
            sound: Some(FrameSound {
                sync: SoundSync::Start,
                loops: 3,
                effect: SoundEffect::FadeOut,
                ..FrameSound::new("sound")
            }),
            ..Default::default()
        });
        project.validate().unwrap();
        let before = project.clone();
        let trace = sound_trace(&before);
        let mut editor = Editor::from_project(&ctx, project);
        editor.merge_timeline_layer(0);
        assert_eq!(editor.symbol().timeline.layers.len(), 1);
        assert_eq!(sound_trace(&editor.project), trace);
        assert_eq!(
            editor.symbol().timeline.layers[0]
                .keyframes
                .iter()
                .filter(|key| key.sound.is_some())
                .count(),
            3
        );
        editor.project.validate().unwrap();
        editor.undo();
        assert_eq!(editor.project, before);

        // Two independent commands on the same frame cannot fit in one keyframe.
        let mut conflict = sound_project();
        for layer in &mut conflict.symbols[0].timeline.layers {
            layer.keyframes[0].sound = Some(FrameSound::new("sound"));
        }
        let mut editor = Editor::from_project(&ctx, conflict.clone());
        editor.merge_timeline_layer(0);
        assert_eq!(editor.project, conflict);
        assert!(!editor.history.can_undo());
        assert!(editor.status.contains("two sounds"));
        editor.project.validate().unwrap();

        // A Stream with unchanged key boundaries is preserved. Inserting another
        // layer's key halfway through its span must instead refuse the merge.
        let mut stream = sound_project();
        stream.symbols[0].timeline.layers[0].keyframes[0].sound = Some(FrameSound {
            sync: SoundSync::Stream,
            ..FrameSound::new("sound")
        });
        let before = stream.clone();
        let mut editor = Editor::from_project(&ctx, stream.clone());
        editor.merge_timeline_layer(0);
        assert_eq!(sound_trace(&editor.project), sound_trace(&before));
        editor.project.validate().unwrap();
        editor.undo();
        assert_eq!(editor.project, before);
        stream.symbols[0].timeline.layers[1]
            .keyframes
            .push(Keyframe {
                frame: 2,
                ..Default::default()
            });
        let mut editor = Editor::from_project(&ctx, stream.clone());
        editor.merge_timeline_layer(0);
        assert_eq!(editor.project, stream);
        assert!(!editor.history.can_undo());
        assert!(editor.status.contains("Stream sound"));
        editor.project.validate().unwrap();
    }

    #[test]
    fn copied_guides_are_relinked_and_merged_animation_keeps_guided_poses_and_exposure() {
        let ctx = egui::Context::default();
        let mut project = Project::blank();
        project.format_version = 6;
        let folder = blank_layer("Folder".into(), LayerRole::Folder, None);
        let mut guide = blank_layer("Guide".into(), LayerRole::Guide, Some(folder.id.clone()));
        guide.keyframes[0].elements.push(Element {
            id: "path".into(),
            kind: ElementKind::Path {
                commands: vec![
                    PathCommand::MoveTo { x: 0., y: 0. },
                    PathCommand::LineTo { x: 0., y: 100. },
                    PathCommand::LineTo { x: 100., y: 100. },
                ],
                fill: [0, 0, 0, 0],
                stroke: [0, 0, 0, 255],
                stroke_width: 1.,
                fill_rule: FillRule::NonZero,
            },
            ..Default::default()
        });
        let mut guided = blank_layer("Guided".into(), LayerRole::Normal, Some(folder.id.clone()));
        guided.properties.motion_guide = Some(guide.id.clone());
        guided.keyframes = vec![
            Keyframe {
                elements: vec![shape("moving", 0.)],
                tween: Some(Easing::Linear),
                ..Default::default()
            },
            Keyframe {
                frame: 4,
                elements: vec![Element {
                    transform: Transform {
                        x: 100.,
                        y: 100.,
                        ..Default::default()
                    },
                    ..shape("moving", 0.)
                }],
                ..Default::default()
            },
        ];
        let mut other = blank_layer("Other".into(), LayerRole::Normal, Some(folder.id.clone()));
        other.properties.end_frame = Some(3);
        other.keyframes[0].elements.push(shape("still", 200.));
        project.symbols[0].timeline.frame_count = 6;
        project.symbols[0].timeline.layers = vec![folder, guide, guided, other];
        project.validate().unwrap();
        let before = project.clone();
        let mut editor = Editor::from_project(&ctx, project);
        editor.copy_timeline_layers(&ctx, 0);
        let clipboard = ctx
            .data(|data| data.get_temp::<LayerClipboard>(editor.layer_clipboard_id()))
            .unwrap();
        editor.paste_timeline_layers(clipboard, 0);
        let timeline = &editor.symbol().timeline;
        let guide = timeline
            .layers
            .iter()
            .find(|layer| layer.name == "Guide copy")
            .unwrap();
        let guided = timeline
            .layers
            .iter()
            .find(|layer| layer.name == "Guided copy")
            .unwrap();
        assert_eq!(guided.properties.motion_guide.as_ref(), Some(&guide.id));
        editor.project.validate().unwrap();
        editor.undo();
        assert_eq!(editor.project, before);

        let timeline = &before.symbols[0].timeline;
        let expected: Vec<_> = (0..6)
            .map(|frame| {
                let mut elements = sample_layer_in(timeline, &timeline.layers[2], frame);
                elements.extend(sample_layer_in(timeline, &timeline.layers[3], frame));
                elements
            })
            .collect();
        assert!(
            expected[2][0].transform.y > 0.,
            "the fixture must exercise a curved guide"
        );
        editor.merge_timeline_layer(2);
        assert_eq!(editor.symbol().timeline.layers.len(), 3);
        let merged = &editor.symbol().timeline.layers[2];
        assert!(merged.properties.motion_guide.is_none());
        for (frame, expected) in expected.iter().enumerate() {
            assert_eq!(&sample_layer(merged, frame as u32), expected);
        }
        editor.project.validate().unwrap();
        editor.undo();
        assert_eq!(editor.project, before);
    }

    #[test]
    fn copied_motion_layer_remaps_only_its_format10_object_references() {
        let ctx = egui::Context::default();
        let mut project = Project::blank();
        let timeline = &mut project.symbols[0].timeline;
        timeline.frame_count = 3;
        let mut track = MotionTrackOverride::new("moving");
        track.easing = Some(Easing::EaseOut);
        timeline.layers[0].keyframes = vec![
            Keyframe {
                elements: vec![shape("moving", 0.)],
                tween: Some(Easing::Linear),
                tween_kind: TweenKind::Motion,
                motion_tracks: vec![track],
                ..Default::default()
            },
            Keyframe {
                frame: 1,
                elements: vec![shape("moving", 50.)],
                tween: Some(Easing::Linear),
                tween_kind: TweenKind::Motion,
                keyed_properties: Some(vec![TweenProperty::X]),
                motion_keys: Some(vec![MotionPropertyKey {
                    element_id: "moving".into(),
                    properties: vec![TweenProperty::X],
                    roving: false,
                }]),
                ..Default::default()
            },
            Keyframe {
                frame: 2,
                elements: vec![shape("moving", 100.)],
                ..Default::default()
            },
        ];
        project.upgrade_format_for_features();
        project.validate().unwrap();
        let mut editor = Editor::from_project(&ctx, project);
        editor.copy_timeline_layers(&ctx, 0);
        let clipboard = ctx
            .data(|data| data.get_temp::<LayerClipboard>(editor.layer_clipboard_id()))
            .unwrap();
        editor.paste_timeline_layers(clipboard, 0);

        let copied = editor
            .symbol()
            .timeline
            .layers
            .iter()
            .find(|layer| layer.name.ends_with(" copy"))
            .unwrap();
        let copied_id = copied.keyframes[0].elements[0].id.clone();
        assert_ne!(copied_id, "moving");
        assert_eq!(copied.keyframes[0].motion_tracks[0].element_id, copied_id);
        assert_eq!(
            copied.keyframes[1].motion_keys.as_ref().unwrap()[0].element_id,
            copied_id
        );
        editor.project.validate().unwrap();
    }

    #[test]
    fn folder_hide_others_and_lock_others_preserve_folder_descendants() {
        let ctx = egui::Context::default();
        let mut project = Project::blank();
        project.format_version = 6;
        let folder = blank_layer("Folder".into(), LayerRole::Folder, None);
        let mut child = blank_layer("Inside".into(), LayerRole::Normal, Some(folder.id.clone()));
        child.keyframes[0].elements.push(shape("blue", 20.));
        let mut outside = blank_layer("Outside".into(), LayerRole::Normal, None);
        outside.keyframes[0].elements.push(shape("orange", 80.));
        project.symbols[0].timeline.layers = vec![outside, child, folder];
        project.validate().unwrap();
        let before = project.clone();

        let mut editor = Editor::from_project(&ctx, project);
        editor.hide_other_layers(2);
        let layers = &editor.symbol().timeline.layers;
        assert!(!layers[0].visible, "outside layer should be hidden");
        assert!(layers[1].visible, "folder child should stay visible");
        assert!(layers[2].visible, "folder should stay visible");
        editor.project.validate().unwrap();
        editor.undo();
        assert_eq!(editor.project, before);

        editor.lock_other_layers(2);
        let layers = &editor.symbol().timeline.layers;
        assert!(layers[0].locked, "outside layer should be locked");
        assert!(!layers[1].locked, "folder child should stay unlocked");
        assert!(!layers[2].locked, "folder should stay unlocked");
        editor.project.validate().unwrap();
        editor.undo();
        assert_eq!(editor.project, before);
    }
}
