//! Scene panel (NF2 `Panel / Scene`, PP-08; reference `panels-more.png`): find, list (current scene
//! highlighted), Add / Duplicate / Delete, double-click to rename, drag to reorder. Every document
//! change is one Undo step; play order is `Project.scenes`, whose first entry is always `root`.
use crate::app::Editor;
use crate::theme::ScrollTheme as _;
use eframe::egui::{self, Ui};
use neoflash_core::*;
use neoflash_ui::components as c;
use neoflash_ui::icons::i16;
use neoflash_ui::tokens::space;
use std::collections::HashMap;

/// Inline rename draft of one scene row.
#[derive(Clone, Debug, PartialEq)]
struct SceneRename {
    id: Id,
    text: String,
    focus: bool,
}

/// Panel-only state (egui temp memory, never saved).
#[derive(Clone, Debug, Default, PartialEq)]
struct ScenePanelState {
    filter: String,
    rename: Option<SceneRename>,
    /// Scene being dragged to a new position.
    drag: Option<Id>,
}

fn state_id() -> egui::Id {
    egui::Id::new("nf2-scene-panel")
}

/// Fresh IDs for a copied timeline: layers and elements get new IDs, links between layers
/// (parent, mask, motion guide) and authoring groups follow them. An element keeps one ID on every
/// keyframe of its layer, so tweens keep pairing it.
fn refresh_timeline_ids(timeline: &mut Timeline) {
    let mut layers: HashMap<Id, Id> = HashMap::new();
    let mut elements: HashMap<Id, Id> = HashMap::new();
    for layer in &mut timeline.layers {
        let id = new_id("layer");
        layers.insert(std::mem::replace(&mut layer.id, id.clone()), id);
        for key in &mut layer.keyframes {
            for element in &mut key.elements {
                let fresh = elements
                    .entry(element.id.clone())
                    .or_insert_with(|| new_id("element"))
                    .clone();
                element.id = fresh;
            }
        }
    }
    for layer in &mut timeline.layers {
        let properties = &mut layer.properties;
        for link in [
            &mut properties.parent,
            &mut properties.mask,
            &mut properties.motion_guide,
        ] {
            if let Some(id) = link.as_mut() {
                match layers.get(id) {
                    Some(fresh) => *id = fresh.clone(),
                    None => *link = None,
                }
            }
        }
        for group in &mut properties.groups {
            group.id = new_id("group");
            for member in &mut group.members {
                if let Some(fresh) = elements.get(member) {
                    *member = fresh.clone();
                }
            }
        }
    }
}

impl Editor {
    /// Duplicate the current scene after itself ("<name> copy") with fresh symbol, layer and element
    /// IDs, then show it. One Undo step.
    pub(crate) fn duplicate_scene(&mut self) {
        let current = self.current_scene().to_owned();
        let Some(source) = self.project.symbol(&current).cloned() else {
            return;
        };
        self.finish_edits();
        let names: std::collections::HashSet<_> = self
            .project
            .symbols
            .iter()
            .map(|s| s.name.clone())
            .collect();
        let mut name = format!("{} copy", source.name);
        let mut n = 2;
        while names.contains(&name) {
            name = format!("{} copy {n}", source.name);
            n += 1;
        }
        let id = new_id("symbol");
        let mut copy = source;
        copy.id = id.clone();
        copy.name = name;
        copy.class_name = None;
        copy.linkage_id = None;
        refresh_timeline_ids(&mut copy.timeline);
        self.transact(|s| {
            if s.project.scenes.is_empty() {
                s.project.scenes.push(s.project.root.clone());
            }
            let index = s
                .project
                .scenes
                .iter()
                .position(|scene| *scene == current)
                .map_or(s.project.scenes.len(), |i| i + 1);
            s.project.scenes.insert(index, id.clone());
            s.project.symbols.push(copy);
            s.project.format_version = s.project.format_version.max(6);
        });
        self.switch_scene(&id);
    }

    /// Rename a scene (its symbol). Empty names and names another symbol uses are refused.
    /// One Undo step; returns whether the name changed.
    pub(crate) fn rename_scene(&mut self, id: &str, name: &str) -> bool {
        let name = name.trim();
        let Some(symbol) = self.project.symbol(id) else {
            return false;
        };
        if !self.project.is_scene(id) || name.is_empty() || symbol.name == name {
            return false;
        }
        if self
            .project
            .symbols
            .iter()
            .any(|s| s.id != id && s.name == name)
        {
            self.message(format!("A scene or symbol is already named “{name}”."));
            return false;
        }
        self.finish_edits();
        self.transact(|s| {
            if let Some(symbol) = s.project.symbol_mut(id) {
                symbol.name = name.to_owned();
            }
        });
        true
    }

    /// Move scene `from` to position `to` in play order. The first scene is the root timeline.
    /// One Undo step; a document with one scene has nothing to reorder.
    pub(crate) fn move_scene(&mut self, from: usize, to: usize) -> bool {
        let count = self.project.scenes.len();
        if count < 2 || from >= count || from == to.min(count - 1) {
            return false;
        }
        self.finish_edits();
        self.transact(|s| {
            let scene = s.project.scenes.remove(from);
            let to = to.min(s.project.scenes.len());
            s.project.scenes.insert(to, scene);
            s.project.root = s.project.scenes[0].clone();
        });
        true
    }

    /// Window ▸ Scene (⇧F2).
    pub(crate) fn scene_panel_ui(&mut self, ui: &mut Ui) {
        self.panel_title(ui, crate::workspace::PanelId::Scene, "Scene");
        egui::ScrollArea::vertical()
            .id_salt("scene-content")
            .auto_shrink([false, false])
            .themed()
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(space::S2, space::S2);
                self.scene_panel(ui);
            });
    }

    /// Panel / Scene body (moved from the planned preview; bound to `Editor::scene_list`).
    fn scene_panel(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let mut state: ScenePanelState = ctx.data(|d| d.get_temp(state_id())).unwrap_or_default();
        let w = ui.available_width();
        ui.add(c::Search::new(&mut state.filter, "Find scene").width(w));
        let scenes = self.scene_list();
        let current = self.current_scene().to_owned();
        let query = state.filter.trim().to_lowercase();
        let mut switch = None;
        let mut commit_rename = None;
        let mut rows: Vec<(usize, egui::Rect)> = Vec::new();
        ui.spacing_mut().item_spacing.y = 0.0;
        for (index, (id, name)) in scenes.iter().enumerate() {
            if !query.is_empty() && !name.to_lowercase().contains(&query) {
                continue;
            }
            if let Some(draft) = state.rename.as_mut().filter(|draft| draft.id == *id) {
                let response = ui.add(
                    c::TextField::new(&mut draft.text, "Scene name").width(ui.available_width()),
                );
                let edit = c::TextField::edit_id(response.id);
                if draft.focus {
                    ui.memory_mut(|m| m.request_focus(edit));
                    draft.focus = false;
                } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    state.rename = None;
                } else if response.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    commit_rename = Some((id.clone(), draft.text.clone()));
                }
                continue;
            }
            let row = c::TreeItem::new(name, c::TreeItemType::Symbol(i16::SCENE))
                .selected(*id == current)
                .show(ui)
                .row;
            rows.push((index, row.rect));
            if row.double_clicked() {
                state.rename = Some(SceneRename {
                    id: id.clone(),
                    text: name.clone(),
                    focus: true,
                });
            } else if row.clicked() && *id != current {
                switch = Some(id.clone());
            }
            if row.drag_started() && query.is_empty() && scenes.len() > 1 {
                state.drag = Some(id.clone());
            }
        }
        // Drag to reorder: insertion line between rows, drop on release.
        let mut reorder = None;
        if let Some(dragged) = state.drag.clone() {
            let pointer = ctx.input(|i| i.pointer.interact_pos());
            let target = pointer.and_then(|p| {
                rows.iter()
                    .find(|(_, r)| p.y < r.center().y)
                    .map(|(i, _)| *i)
                    .or_else(|| rows.last().map(|(i, _)| i + 1))
            });
            if let (Some(target), Some(first), Some(last)) = (target, rows.first(), rows.last()) {
                let y = rows
                    .iter()
                    .find(|(i, _)| *i == target)
                    .map_or(last.1.bottom(), |(_, r)| r.top());
                c::surface::drop_indicator(
                    ui.painter(),
                    &ctx,
                    egui::pos2(first.1.left() + space::S2, y),
                    egui::pos2(first.1.right() - space::S2, y),
                );
            }
            if !ctx.input(|i| i.pointer.any_down()) {
                state.drag = None;
                if let (Some(target), Some(from)) =
                    (target, scenes.iter().position(|(id, _)| *id == dragged))
                {
                    let to = if target > from { target - 1 } else { target };
                    reorder = Some((from, to));
                }
            } else {
                ctx.request_repaint();
            }
        }
        ui.spacing_mut().item_spacing.y = space::S2;
        ui.add_space(space::S2);
        let (mut add, mut duplicate, mut delete) = (false, false, false);
        ui.horizontal(|ui| {
            add = ui.add(c::IconButton::new(i16::PLUS, "Add scene")).clicked();
            duplicate = ui
                .add(c::IconButton::new(i16::COPY, "Duplicate scene"))
                .clicked();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                delete = ui
                    .add_enabled(
                        scenes.len() > 1,
                        c::IconButton::new(i16::TRASH, "Delete scene"),
                    )
                    .clicked();
            });
        });
        if let Some((id, text)) = commit_rename {
            state.rename = None;
            self.rename_scene(&id, &text);
        }
        if let Some((from, to)) = reorder {
            self.move_scene(from, to);
        }
        if let Some(id) = switch {
            self.switch_scene(&id);
        }
        if add {
            self.new_scene();
        }
        if duplicate {
            self.duplicate_scene();
        }
        if delete {
            self.delete_scene();
        }
        ctx.data_mut(|d| d.insert_temp(state_id(), state));
    }
}
