//! Classic authoring layout operations. Every command edits the sampled pose
//! at the current frame and keeps placement identity and scripts intact.
use crate::{
    app::Editor,
    paint_controls::{compact_number, numeric_row, paint_row, property_row},
    selection::SelectionTarget,
    workspace::PanelId,
};
use eframe::egui::{self, Pos2, Rect, Vec2};
use neoflash_core::*;
use std::collections::{HashMap, HashSet};

pub(crate) struct AuthoringModify {
    pub to_stage: bool,
    pub horizontal_gap: f64,
    pub vertical_gap: f64,
    pub linked_scale: bool,
    pub color_selection: bool,
}
impl Default for AuthoringModify {
    fn default() -> Self {
        Self {
            to_stage: false,
            horizontal_gap: 16.,
            vertical_gap: 16.,
            linked_scale: true,
            color_selection: true,
        }
    }
}

#[derive(Clone)]
struct Placement {
    target: SelectionTarget,
    element: Element,
    bounds: Rect,
}

/// An authoring group is one layout object, even though runtime placements
/// stay flat. Aligning a group must never align its children to each other.
struct LayoutUnit {
    members: Vec<Placement>,
    bounds: Rect,
}

#[derive(Clone, Copy)]
pub(crate) enum Arrange {
    Front,
    Forward,
    Backward,
    Back,
}

impl Editor {
    fn modify_placements(&self, ctx: &egui::Context) -> Vec<Placement> {
        let painter = ctx.layer_painter(egui::LayerId::background());
        let mut bounds = HashMap::<String, Rect>::new();
        for draw in evaluate(&self.project, self.symbol_id(), self.frame) {
            if let Some(id) = draw.instance_path.first() {
                let rect = crate::stage::world_bounds(&draw, &painter);
                if rect.is_finite() {
                    bounds
                        .entry(id.clone())
                        .and_modify(|b| *b = b.union(rect))
                        .or_insert(rect);
                }
            }
        }
        self.sampled_targets()
            .into_iter()
            .filter(|(target, _)| self.target_editable(target))
            .map(|(target, element)| {
                let bounds = bounds.remove(&element.id).unwrap_or_else(|| {
                    Rect::from_min_size(
                        Pos2::new(element.transform.x as f32, element.transform.y as f32),
                        Vec2::ZERO,
                    )
                });
                Placement {
                    target,
                    element,
                    bounds,
                }
            })
            .collect()
    }

    fn modify_scope(&self) -> egui::Id {
        egui::Id::new((
            "modify-selection",
            &self.clipboard.session,
            self.symbol_id(),
            self.frame,
            self.selection
                .targets
                .iter()
                .map(|t| (&t.layer, &t.element))
                .collect::<Vec<_>>(),
        ))
    }

    fn modify_layout_units(&self, ctx: &egui::Context) -> Vec<LayoutUnit> {
        let mut units: Vec<LayoutUnit> = Vec::new();
        let mut groups = HashMap::<(String, String), usize>::new();
        for placement in self.modify_placements(ctx) {
            let group = self
                .symbol()
                .timeline
                .layers
                .iter()
                .find(|layer| layer.id == placement.target.layer)
                .and_then(|layer| {
                    layer
                        .properties
                        .groups
                        .iter()
                        .find(|group| group.members.contains(&placement.target.element))
                })
                .map(|group| (placement.target.layer.clone(), group.id.clone()));
            if let Some(index) = group.as_ref().and_then(|group| groups.get(group)).copied() {
                units[index].bounds = units[index].bounds.union(placement.bounds);
                units[index].members.push(placement);
            } else {
                if let Some(group) = group {
                    groups.insert(group, units.len());
                }
                units.push(LayoutUnit {
                    bounds: placement.bounds,
                    members: vec![placement],
                });
            }
        }
        units
    }

    fn write_modified(&mut self, changes: Vec<(SelectionTarget, Element)>) {
        for (target, element) in changes {
            self.ensure_key_for(&target.layer, false);
            let Some(index) = self.target_layer_index(&target) else {
                continue;
            };
            if element.transform.skew != 0. || !element.transform.color_transform.is_identity() {
                self.project.format_version = self.project.format_version.max(5);
            }
            let frame = self.frame;
            if let Some(current) = self.symbol_mut().timeline.layers[index]
                .key_at_mut(frame)
                .and_then(|key| key.elements.iter_mut().find(|e| e.id == target.element))
            {
                current.transform = element.transform;
                current.kind = element.kind;
            }
        }
    }

    fn apply_modified(
        &mut self,
        changes: Vec<(SelectionTarget, Element)>,
        control: Option<egui::Id>,
    ) {
        if changes.is_empty() {
            return;
        }
        if changes
            .iter()
            .any(|(_, element)| !valid_transform(element.transform))
        {
            self.message("This transformation exceeds the document coordinate limits.");
            return;
        }
        if let Some(control) = control {
            self.begin_property_edit(self.modify_scope(), control);
            self.write_modified(changes);
        } else {
            self.finish_edits();
            self.playing = false;
            self.transact(|editor| editor.write_modified(changes));
        }
    }

    fn modify_reference(&self, units: &[LayoutUnit]) -> Rect {
        if self.authoring_modify.to_stage {
            let size = Vec2::new(self.project.width as f32, self.project.height as f32);
            if self.editing.len() > 1 {
                Rect::from_center_size(Pos2::ZERO, size)
            } else {
                Rect::from_min_size(Pos2::ZERO, size)
            }
        } else {
            units
                .iter()
                .fold(Rect::NOTHING, |bounds, unit| bounds.union(unit.bounds))
        }
    }

    /// axis: 0 horizontal, 1 vertical; edge: 0 leading, 1 center, 2 trailing.
    pub(crate) fn align_selection(&mut self, ctx: &egui::Context, axis: usize, edge: usize) {
        let units = self.modify_layout_units(ctx);
        if units.is_empty() {
            return;
        }
        let reference = self.modify_reference(&units);
        let at = edge_at(reference, axis, edge);
        let changes = units
            .into_iter()
            .flat_map(|unit| translated_unit(unit, axis, |bounds| at - edge_at(bounds, axis, edge)))
            .collect();
        self.apply_modified(changes, None);
    }

    pub(crate) fn distribute_selection(&mut self, ctx: &egui::Context, axis: usize, edge: usize) {
        let mut units = self.modify_layout_units(ctx);
        if units.len() < 3 {
            return;
        }
        units.sort_by(|a, b| {
            edge_at(a.bounds, axis, edge).total_cmp(&edge_at(b.bounds, axis, edge))
        });
        let reference = self.modify_reference(&units);
        let first = if self.authoring_modify.to_stage {
            let item = &units[0];
            reference.min[axis] as f64 + edge as f64 * extent(item.bounds, axis) / 2.
        } else {
            edge_at(units[0].bounds, axis, edge)
        };
        let last = if self.authoring_modify.to_stage {
            let item = units.last().unwrap();
            reference.max[axis] as f64 - (2 - edge) as f64 * extent(item.bounds, axis) / 2.
        } else {
            edge_at(units.last().unwrap().bounds, axis, edge)
        };
        let step = (last - first) / (units.len() - 1) as f64;
        let changes = units
            .into_iter()
            .enumerate()
            .flat_map(|(index, unit)| {
                translated_unit(unit, axis, |bounds| {
                    first + step * index as f64 - edge_at(bounds, axis, edge)
                })
            })
            .collect();
        self.apply_modified(changes, None);
    }

    /// None keeps the outer bounds and evens the gaps. Some sets an exact gap,
    /// including a negative overlap, keeping the first edge fixed.
    pub(crate) fn space_selection(&mut self, ctx: &egui::Context, axis: usize, gap: Option<f64>) {
        let mut units = self.modify_layout_units(ctx);
        if units.len() < 2 {
            return;
        }
        units.sort_by(|a, b| a.bounds.min[axis].total_cmp(&b.bounds.min[axis]));
        let reference = self.modify_reference(&units);
        let gap = gap.unwrap_or_else(|| {
            (extent(reference, axis) - units.iter().map(|p| extent(p.bounds, axis)).sum::<f64>())
                / (units.len() - 1) as f64
        });
        let mut at = if self.authoring_modify.to_stage {
            reference.min[axis] as f64
        } else {
            units[0].bounds.min[axis] as f64
        };
        let changes = units
            .into_iter()
            .flat_map(|unit| {
                let offset = at - unit.bounds.min[axis] as f64;
                at += extent(unit.bounds, axis) + gap;
                translated_unit(unit, axis, |_| offset)
            })
            .collect();
        self.apply_modified(changes, None);
    }

    pub(crate) fn match_selection_size(&mut self, ctx: &egui::Context, axis: usize) {
        let units = self.modify_layout_units(ctx);
        let size = if self.authoring_modify.to_stage {
            extent(self.modify_reference(&units), axis)
        } else {
            units
                .iter()
                .map(|p| extent(p.bounds, axis))
                .fold(0., f64::max)
        };
        if size <= 0. {
            return;
        }
        let mut changes = Vec::new();
        for unit in units {
            let old = extent(unit.bounds, axis);
            if old <= 1e-9 {
                continue;
            }
            let mut scale = [1., 1.];
            scale[axis] = size / old;
            let matrix = around(unit.bounds.min, [scale[0], 0., 0., scale[1], 0., 0.]);
            let Some(members) = transformed(&unit.members, matrix) else {
                self.message("This size exceeds transform limits.");
                return;
            };
            changes.extend(members);
        }
        self.apply_modified(changes, None);
    }

    pub(crate) fn affine_selection(&mut self, ctx: &egui::Context, linear: [f64; 6]) {
        let placements = self.modify_placements(ctx);
        if placements.is_empty() {
            return;
        }
        let matrix = around(combined_bounds(&placements).center(), linear);
        let mut changes = Vec::new();
        for mut item in placements {
            let Some(t) = affine_transform(item.element.transform, matrix) else {
                self.message("This transformation exceeds the document coordinate limits.");
                return;
            };
            item.element.transform = t;
            changes.push((item.target, item.element));
        }
        self.apply_modified(changes, None);
    }

    pub(crate) fn reset_selection_transform(&mut self, ctx: &egui::Context) {
        let changes = self
            .modify_placements(ctx)
            .into_iter()
            .map(|mut item| {
                let mut t = item.element.transform;
                t.scale_x = 1.;
                t.scale_y = 1.;
                t.rotation = 0.;
                t.skew = 0.;
                item.element.transform =
                    retain_center(item.element.transform, t, item.bounds.center());
                (item.target, item.element)
            })
            .collect();
        self.apply_modified(changes, None);
    }

    pub(crate) fn arrange_selection(&mut self, direction: Arrange) {
        self.finish_edits();
        let targets: HashSet<_> = self
            .selection
            .targets
            .iter()
            .filter(|t| self.target_editable(t))
            .cloned()
            .collect();
        if targets.is_empty() {
            return;
        }
        self.playing = false;
        self.transact(|editor| {
            let layers: HashSet<_> = targets.iter().map(|t| t.layer.clone()).collect();
            for layer_id in layers {
                editor.ensure_key_for(&layer_id, false);
                let frame = editor.frame;
                let layer = editor
                    .symbol_mut()
                    .timeline
                    .layers
                    .iter_mut()
                    .find(|l| l.id == layer_id)
                    .unwrap();
                let elements = &mut layer.key_at_mut(frame).unwrap().elements;
                let selected = |e: &Element| {
                    targets.contains(&SelectionTarget {
                        layer: layer_id.clone(),
                        element: e.id.clone(),
                    })
                };
                match direction {
                    Arrange::Front => elements.sort_by_key(|e| selected(e)),
                    Arrange::Back => elements.sort_by_key(|e| !selected(e)),
                    Arrange::Forward => {
                        for index in (0..elements.len().saturating_sub(1)).rev() {
                            if selected(&elements[index]) && !selected(&elements[index + 1]) {
                                elements.swap(index, index + 1);
                            }
                        }
                    }
                    Arrange::Backward => {
                        for index in 1..elements.len() {
                            if selected(&elements[index]) && !selected(&elements[index - 1]) {
                                elements.swap(index, index - 1);
                            }
                        }
                    }
                }
            }
        });
    }

    pub(crate) fn group_selection(&mut self) {
        self.finish_edits();
        let targets = self.selection.targets.clone();
        if targets.len() < 2 {
            return;
        }
        let layer_id = &targets[0].layer;
        if targets
            .iter()
            .any(|target| &target.layer != layer_id || !self.target_editable(target))
        {
            self.message("Group objects on one editable layer. Move them to the same layer first.");
            return;
        }
        let layer = &self.symbol().timeline.layers[self.target_layer_index(&targets[0]).unwrap()];
        let mut members: HashSet<_> = targets
            .iter()
            .map(|target| target.element.clone())
            .collect();
        let indices = layer
            .key_at(self.frame)
            .unwrap()
            .elements
            .iter()
            .enumerate()
            .filter(|(_, element)| members.contains(&element.id))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if indices.last().unwrap() - indices[0] + 1 != indices.len() {
            self.message(
                "Group a contiguous stack of objects; objects between them must be selected too.",
            );
            return;
        }
        for group in &layer.properties.groups {
            if group.members.iter().any(|id| members.contains(id)) {
                members.extend(group.members.iter().cloned());
            }
        }
        let layer_id = layer_id.clone();
        let mut ordered = Vec::new();
        for element in layer.keyframes.iter().flat_map(|key| &key.elements) {
            if members.remove(&element.id) {
                ordered.push(element.id.clone());
            }
        }
        let name = format!("Group {}", layer.properties.groups.len() + 1);
        self.transact(|editor| {
            editor.project.format_version = editor.project.format_version.max(4);
            let layer = editor
                .symbol_mut()
                .timeline
                .layers
                .iter_mut()
                .find(|l| l.id == layer_id)
                .unwrap();
            layer
                .properties
                .groups
                .retain(|group| !group.members.iter().any(|id| ordered.contains(id)));
            layer.properties.groups.push(AuthoringGroup {
                id: new_id("group"),
                name,
                members: ordered,
            });
        });
    }

    pub(crate) fn ungroup_selection(&mut self) {
        self.finish_edits();
        let selected: HashSet<_> = self.selection.targets.iter().cloned().collect();
        if selected.is_empty() {
            return;
        }
        self.transact(|editor| {
            for layer in &mut editor.symbol_mut().timeline.layers {
                let id = layer.id.clone();
                layer.properties.groups.retain(|group| {
                    !group.members.iter().any(|element| {
                        selected.contains(&SelectionTarget {
                            layer: id.clone(),
                            element: element.clone(),
                        })
                    })
                });
            }
        });
    }

    pub(crate) fn break_apart_selection(&mut self) {
        self.finish_edits();
        let selected = self.sampled_targets();
        if selected.is_empty() {
            return;
        }
        let mut replacements = Vec::new();
        for (target, instance) in selected {
            let ElementKind::Instance { symbol_id, .. } = &instance.kind else {
                self.message("Break Apart currently supports static symbol instances. Text and bitmap conversion are separate operations.");
                return;
            };
            if !self.target_editable(&target)
                || !instance.scripts.is_empty()
                || instance.accessibility.is_some()
            {
                self.message("Break Apart would remove instance scripts or accessibility. Keep this instance intact.");
                return;
            }
            let Some(symbol) = self.project.symbol(symbol_id) else {
                return;
            };
            if symbol.timeline.frame_count != 1
                || !symbol.timeline.scripts.is_empty()
                || symbol
                    .timeline
                    .layers
                    .iter()
                    .any(|l| !l.visible || l.locked || l.properties != LayerProperties::default())
                || contains_scripts(&self.project, symbol_id, &mut HashSet::new())
            {
                self.message("Break Apart requires a static symbol without scripts, hidden/locked layers, guides or masks. These would lose behavior when flattened.");
                return;
            }
            let parent = instance.transform;
            let mut children = Vec::new();
            for layer in &symbol.timeline.layers {
                for mut child in sample_layer(layer, 0) {
                    let Some(mut transform) =
                        affine_transform(child.transform, transform_matrix(parent))
                    else {
                        self.message("Break Apart would exceed transform limits.");
                        return;
                    };
                    transform.alpha = 1.;
                    transform.color_transform = child
                        .transform
                        .color_transform
                        .with_alpha(child.transform.alpha)
                        .then(parent.color_transform.with_alpha(parent.alpha));
                    if transform.color_transform.validate().is_err() {
                        self.message("Break Apart cannot preserve this combined color transform within document limits.");
                        return;
                    }
                    child.transform = transform;
                    child.id = new_id("object");
                    children.push(child);
                }
            }
            replacements.push((target, children));
        }
        self.playing = false;
        self.transact(|editor| {
            let mut selection = Vec::new();
            for (target, children) in replacements {
                editor.ensure_key_for(&target.layer, false);
                let index = editor.target_layer_index(&target).unwrap();
                let frame = editor.frame;
                let elements = &mut editor.symbol_mut().timeline.layers[index]
                    .key_at_mut(frame)
                    .unwrap()
                    .elements;
                if let Some(at) = elements.iter().position(|e| e.id == target.element) {
                    selection.extend(children.iter().map(|e| SelectionTarget {
                        layer: target.layer.clone(),
                        element: e.id.clone(),
                    }));
                    elements.splice(at..=at, children);
                }
            }
            editor.prune_authoring_groups();
            editor.project.format_version = editor.project.format_version.max(5);
            editor.set_selection(selection);
        });
    }

    pub(crate) fn modify_menu_ui(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Modify", |ui| {
            let selected = !self.selection.is_empty();
            if ui
                .add_enabled(
                    self.can_convert_selection(),
                    egui::Button::new("Convert to Symbol…").shortcut_text("F8"),
                )
                .clicked()
            {
                self.convert_to_symbol();
                ui.close();
            }
            if ui
                .add_enabled(selected, egui::Button::new("Break Apart"))
                .clicked()
            {
                self.break_apart_selection();
                ui.close();
            }
            if ui
                .add_enabled(self.selection.len() >= 2, egui::Button::new("Group"))
                .clicked()
            {
                self.group_selection();
                ui.close();
            }
            if ui
                .add_enabled(selected, egui::Button::new("Ungroup"))
                .clicked()
            {
                self.ungroup_selection();
                ui.close();
            }
            ui.separator();
            ui.menu_button("Arrange", |ui| {
                for (label, direction) in [
                    ("Bring to Front", Arrange::Front),
                    ("Bring Forward", Arrange::Forward),
                    ("Send Backward", Arrange::Backward),
                    ("Send to Back", Arrange::Back),
                ] {
                    if ui.add_enabled(selected, egui::Button::new(label)).clicked() {
                        self.arrange_selection(direction);
                        ui.close();
                    }
                }
            });
            ui.menu_button("Transform", |ui| {
                if ui.button("Transform panel").clicked() {
                    self.show_panel(PanelId::Transform);
                    ui.close();
                }
                ui.add_enabled_ui(selected, |ui| self.transform_commands(ui));
            });
            for (label, panel) in [("Align…", PanelId::Align), ("Color…", PanelId::Color)] {
                if ui.button(label).clicked() {
                    self.show_panel(panel);
                    ui.close();
                }
            }
        });
    }

    pub(crate) fn align_panel_ui(&mut self, ui: &mut egui::Ui) {
        self.panel_title(ui, PanelId::Align, "Align");
        egui::ScrollArea::vertical()
            .id_salt("align-content")
            .auto_shrink([false, false])
            .show(ui, |ui| self.align_panel_contents(ui));
    }

    fn align_panel_contents(&mut self, ui: &mut egui::Ui) {
        ui.checkbox(&mut self.authoring_modify.to_stage, "To Stage");
        let count = self.modify_layout_units(ui.ctx()).len();
        ui.add_enabled_ui(count > 0, |ui| {
            ui.label("Align");
            align_buttons(ui, |ui, axis, edge| {
                self.align_selection(ui.ctx(), axis, edge)
            });
            ui.separator();
            ui.label("Distribute positions");
            ui.add_enabled_ui(count >= 3, |ui| {
                align_buttons(ui, |ui, axis, edge| {
                    self.distribute_selection(ui.ctx(), axis, edge)
                })
            });
            ui.separator();
            ui.label("Spacing");
            ui.add_enabled_ui(count >= 2, |ui| {
                for axis in 0..2 {
                    property_row(ui, if axis == 0 { "H gap" } else { "V gap" }, |ui| {
                        let gap = if axis == 0 {
                            &mut self.authoring_modify.horizontal_gap
                        } else {
                            &mut self.authoring_modify.vertical_gap
                        };
                        let width = (ui.available_width() - 54.).clamp(30., 96.);
                        ui.allocate_ui_with_layout(
                            Vec2::new(width, 24.),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                compact_number(ui, gap, -100_000. ..=100_000., 1., "");
                            },
                        );
                        let value = *gap;
                        if ui.button("Apply").clicked() {
                            self.space_selection(ui.ctx(), axis, Some(value));
                        }
                    });
                }
                ui.horizontal(|ui| {
                    if ui
                        .button("Even H")
                        .on_hover_text("Equal horizontal gaps, retaining outer bounds")
                        .clicked()
                    {
                        self.space_selection(ui.ctx(), 0, None);
                    }
                    if ui
                        .button("Even V")
                        .on_hover_text("Equal vertical gaps, retaining outer bounds")
                        .clicked()
                    {
                        self.space_selection(ui.ctx(), 1, None);
                    }
                });
            });
            ui.separator();
            ui.label("Match size");
            ui.horizontal(|ui| {
                if ui.button("Width").clicked() {
                    self.match_selection_size(ui.ctx(), 0);
                }
                if ui.button("Height").clicked() {
                    self.match_selection_size(ui.ctx(), 1);
                }
            });
        });
    }

    pub(crate) fn transform_panel_ui(&mut self, ui: &mut egui::Ui) {
        self.panel_title(ui, PanelId::Transform, "Transform");
        egui::ScrollArea::vertical()
            .id_salt("transform-content")
            .auto_shrink([false, false])
            .show(ui, |ui| self.transform_panel_contents(ui));
    }

    fn transform_panel_contents(&mut self, ui: &mut egui::Ui) {
        let placements = self.modify_placements(ui.ctx());
        if placements.is_empty() {
            ui.weak("Select artwork on the Stage.");
            return;
        }
        let scope = self.modify_scope();
        self.property_edit_scope(scope);
        ui.push_id(scope.with("transform"), |ui| {
            let bounds = combined_bounds(&placements);
            ui.checkbox(
                &mut self.authoring_modify.linked_scale,
                "Constrain proportions",
            );
            let mut fields = [
                bounds.min.x as f64,
                bounds.min.y as f64,
                bounds.width() as f64,
                bounds.height() as f64,
            ];
            let original = fields;
            for (index, label) in ["X", "Y", "W", "H"].iter().enumerate() {
                let range = if index < 2 {
                    -1_000_000. ..=1_000_000.
                } else {
                    0.01..=1_000_000.
                };
                let response = numeric_row(ui, label, &mut fields[index], range, 1.);
                if response.changed() {
                    let mut matrix = [1., 0., 0., 1., 0., 0.];
                    if index < 2 {
                        matrix[4 + index] = fields[index] - original[index];
                    } else if original[index] > 1e-9 {
                        let scale = fields[index] / original[index];
                        let axis = index - 2;
                        matrix[if axis == 0 { 0 } else { 3 }] = scale;
                        if self.authoring_modify.linked_scale {
                            matrix[0] = scale;
                            matrix[3] = scale;
                        }
                        matrix = around(bounds.min, matrix);
                    }
                    if let Some(changes) = transformed(&placements, matrix) {
                        self.apply_modified(changes, Some(response.id));
                    } else {
                        self.message("This size exceeds transform limits.");
                    }
                }
            }
            if placements.len() == 1 {
                ui.separator();
                let item = &placements[0];
                let old = item.element.transform;
                for (label, value, range, speed) in [
                    ("Scale X", old.scale_x * 100., -1_000_000. ..=1_000_000., 1.),
                    ("Scale Y", old.scale_y * 100., -1_000_000. ..=1_000_000., 1.),
                    ("Rotate", old.rotation, -360_000. ..=360_000., 1.),
                    ("Skew", old.skew, -360_000. ..=360_000., 1.),
                ] {
                    let mut value = value;
                    let response = property_row(ui, label, |ui| {
                        compact_number(
                            ui,
                            &mut value,
                            range,
                            speed,
                            if label.starts_with("Scale") {
                                " %"
                            } else {
                                "°"
                            },
                        )
                    });
                    if response.changed() {
                        let mut new = old;
                        match label {
                            "Scale X" => {
                                new.scale_x = value / 100.;
                                if self.authoring_modify.linked_scale && old.scale_x.abs() > 1e-9 {
                                    new.scale_y *= new.scale_x / old.scale_x;
                                }
                            }
                            "Scale Y" => {
                                new.scale_y = value / 100.;
                                if self.authoring_modify.linked_scale && old.scale_y.abs() > 1e-9 {
                                    new.scale_x *= new.scale_y / old.scale_y;
                                }
                            }
                            "Rotate" => new.rotation = value,
                            _ => new.skew = value,
                        }
                        if new.scale_x.abs().max(new.scale_y.abs()) <= 1e4 {
                            let mut element = item.element.clone();
                            element.transform = retain_center(old, new, item.bounds.center());
                            self.apply_modified(
                                vec![(item.target.clone(), element)],
                                Some(response.id),
                            );
                        }
                    }
                }
            }
            ui.separator();
            self.transform_commands(ui);
        });
    }

    fn transform_commands(&mut self, ui: &mut egui::Ui) {
        for (label, matrix) in [
            ("Rotate 90° CW", [0., 1., -1., 0., 0., 0.]),
            ("Rotate 90° CCW", [0., -1., 1., 0., 0., 0.]),
            ("Flip Horizontal", [-1., 0., 0., 1., 0., 0.]),
            ("Flip Vertical", [1., 0., 0., -1., 0., 0.]),
        ] {
            if ui.button(label).clicked() {
                self.affine_selection(ui.ctx(), matrix);
            }
        }
        if ui.button("Remove Transform").clicked() {
            self.reset_selection_transform(ui.ctx());
        }
    }

    pub(crate) fn color_panel_ui(&mut self, ui: &mut egui::Ui) {
        self.panel_title(ui, PanelId::Color, "Color");
        egui::ScrollArea::vertical()
            .id_salt("color-content")
            .auto_shrink([false, false])
            .show(ui, |ui| self.color_panel_contents(ui));
    }

    fn color_panel_contents(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(
                &mut self.authoring_modify.color_selection,
                true,
                "Selection",
            );
            ui.selectable_value(&mut self.authoring_modify.color_selection, false, "Drawing");
        });
        if !self.authoring_modify.color_selection {
            paint_row(ui, "drawing-fill", "Fill", &mut self.fill);
            paint_row(ui, "drawing-stroke", "Stroke", &mut self.stroke);
            numeric_row(ui, "Width", &mut self.stroke_width, 0. ..=100., 0.25);
            return;
        }
        let targets = self
            .sampled_targets()
            .into_iter()
            .filter(|(t, _)| self.target_editable(t))
            .collect::<Vec<_>>();
        let Some((fill, stroke, width, supports_stroke)) =
            targets.iter().find_map(|(_, e)| paints(&e.kind))
        else {
            ui.weak("Select a shape, path or text object; use Drawing for tool colors.");
            return;
        };
        let scope = self.modify_scope();
        self.property_edit_scope(scope);
        ui.push_id(scope.with("color"), |ui| {
            let (mut new_fill, mut new_stroke, mut new_width) = (fill, stroke, width);
            let fill_response = paint_row(ui, "selected-fill", "Fill", &mut new_fill);
            let mut control = fill_response.changed().then_some(fill_response.id);
            if supports_stroke {
                let response = paint_row(ui, "selected-stroke", "Stroke", &mut new_stroke);
                if response.changed() {
                    control = Some(response.id);
                }
                let response = numeric_row(ui, "Width", &mut new_width, 0. ..=100., 0.25);
                if response.changed() {
                    control = Some(response.id);
                }
            }
            if targets.iter().any(|(_, e)| {
                matches!(
                    &e.kind,
                    ElementKind::PaintedPath {
                        fill_gradient: Some(_),
                        ..
                    } | ElementKind::PaintedPath {
                        stroke_gradient: Some(_),
                        ..
                    }
                )
            }) {
                ui.small("Changing an RGB color replaces its gradient with a solid paint.");
            }
            if let Some(control) = control {
                let changes = targets
                    .into_iter()
                    .filter_map(|(target, mut element)| {
                        let changed = edit_paints(
                            &mut element.kind,
                            (fill, stroke, width),
                            (new_fill, new_stroke, new_width),
                        );
                        changed.then_some((target, element))
                    })
                    .collect();
                self.apply_modified(changes, Some(control));
            }
        });
    }
}

fn combined_bounds(items: &[Placement]) -> Rect {
    items
        .iter()
        .fold(Rect::NOTHING, |rect, item| rect.union(item.bounds))
}
fn extent(rect: Rect, axis: usize) -> f64 {
    (rect.max[axis] - rect.min[axis]) as f64
}
fn edge_at(rect: Rect, axis: usize, edge: usize) -> f64 {
    rect.min[axis] as f64 + extent(rect, axis) * edge as f64 / 2.
}
fn translate(t: &mut Transform, axis: usize, amount: f64) {
    if axis == 0 {
        t.x += amount;
    } else {
        t.y += amount;
    }
}
fn translated_unit(
    unit: LayoutUnit,
    axis: usize,
    offset: impl FnOnce(Rect) -> f64,
) -> Vec<(SelectionTarget, Element)> {
    let offset = offset(unit.bounds);
    unit.members
        .into_iter()
        .map(|mut item| {
            translate(&mut item.element.transform, axis, offset);
            (item.target, item.element)
        })
        .collect()
}
fn around(p: Pos2, mut m: [f64; 6]) -> [f64; 6] {
    m[4] += p.x as f64 - m[0] * p.x as f64 - m[2] * p.y as f64;
    m[5] += p.y as f64 - m[1] * p.x as f64 - m[3] * p.y as f64;
    m
}
fn affine_transform(old: Transform, matrix: [f64; 6]) -> Option<Transform> {
    let mut new = Transform::from_matrix(affine_multiply(matrix, transform_matrix(old)))?;
    new.alpha = old.alpha;
    new.color_transform = old.color_transform;
    Some(new)
}
fn transformed(items: &[Placement], matrix: [f64; 6]) -> Option<Vec<(SelectionTarget, Element)>> {
    items
        .iter()
        .map(|item| {
            let mut element = item.element.clone();
            element.transform = affine_transform(element.transform, matrix)?;
            Some((item.target.clone(), element))
        })
        .collect()
}
fn retain_center(old: Transform, mut new: Transform, center: Pos2) -> Transform {
    let [a, b, c, d, x, y] = transform_matrix(old);
    let determinant = a * d - b * c;
    if determinant.abs() > 1e-12 {
        let dx = center.x as f64 - x;
        let dy = center.y as f64 - y;
        let local = [
            (d * dx - c * dy) / determinant,
            (-b * dx + a * dy) / determinant,
        ];
        let m = transform_matrix(new);
        new.x = center.x as f64 - m[0] * local[0] - m[2] * local[1];
        new.y = center.y as f64 - m[1] * local[0] - m[3] * local[1];
    }
    new
}
fn align_buttons(ui: &mut egui::Ui, mut action: impl FnMut(&mut egui::Ui, usize, usize)) {
    for (axis, labels) in [
        (0, ["Left", "Center", "Right"]),
        (1, ["Top", "Middle", "Bottom"]),
    ] {
        ui.horizontal(|ui| {
            for (edge, label) in labels.into_iter().enumerate() {
                if ui.button(label).clicked() {
                    action(ui, axis, edge);
                }
            }
        });
    }
}
fn contains_scripts(project: &Project, id: &str, seen: &mut HashSet<String>) -> bool {
    if !seen.insert(id.into()) {
        return false;
    }
    project.symbol(id).is_some_and(|s| {
        s.timeline.frame_count != 1
            || s.class_name.is_some()
            || !s.timeline.scripts.is_empty()
            || s.timeline
                .layers
                .iter()
                .flat_map(|l| &l.keyframes)
                .flat_map(|k| &k.elements)
                .any(|e| {
                    !e.scripts.is_empty()
                        || match &e.kind {
                            ElementKind::Instance { symbol_id, .. } => {
                                contains_scripts(project, symbol_id, seen)
                            }
                            _ => false,
                        }
                })
    })
}
fn valid_transform(t: Transform) -> bool {
    [t.x, t.y, t.scale_x, t.scale_y, t.rotation, t.skew, t.alpha]
        .into_iter()
        .all(f64::is_finite)
        && t.x.abs().max(t.y.abs()) <= 1e12
        && t.scale_x.abs().max(t.scale_y.abs()) <= 1e4
        && t.rotation.abs().max(t.skew.abs()) <= 1e9
        && (0. ..=1.).contains(&t.alpha)
        && t.color_transform.validate().is_ok()
}
fn paints(kind: &ElementKind) -> Option<(Color, Color, f64, bool)> {
    match kind {
        ElementKind::Rectangle {
            fill,
            stroke,
            stroke_width,
            ..
        }
        | ElementKind::Ellipse {
            fill,
            stroke,
            stroke_width,
            ..
        }
        | ElementKind::Path {
            fill,
            stroke,
            stroke_width,
            ..
        }
        | ElementKind::PaintedPath {
            fill,
            stroke,
            stroke_width,
            ..
        } => Some((*fill, *stroke, *stroke_width, true)),
        ElementKind::Text { color, .. } => Some((*color, [0; 4], 0., false)),
        _ => None,
    }
}
fn edit_paints(kind: &mut ElementKind, old: (Color, Color, f64), new: (Color, Color, f64)) -> bool {
    let apply_color = |value: &mut Color, before: Color, after: Color| {
        if before[..3] != after[..3] {
            value[..3].copy_from_slice(&after[..3]);
        }
        if before[3] != after[3] {
            value[3] = after[3];
        }
    };
    match kind {
        ElementKind::Rectangle {
            fill,
            stroke,
            stroke_width,
            ..
        }
        | ElementKind::Ellipse {
            fill,
            stroke,
            stroke_width,
            ..
        }
        | ElementKind::Path {
            fill,
            stroke,
            stroke_width,
            ..
        }
        | ElementKind::PaintedPath {
            fill,
            stroke,
            stroke_width,
            ..
        } => {
            apply_color(fill, old.0, new.0);
            apply_color(stroke, old.1, new.1);
            if old.2 != new.2 {
                *stroke_width = new.2;
            }
        }
        ElementKind::Text { color, .. } => apply_color(color, old.0, new.0),
        _ => return false,
    }
    if let ElementKind::PaintedPath {
        fill_gradient,
        stroke_gradient,
        ..
    } = kind
    {
        for (gradient, before, after) in [
            (fill_gradient, old.0, new.0),
            (stroke_gradient, old.1, new.1),
        ] {
            if before[..3] != after[..3] {
                *gradient = None;
            } else if before[3] != after[3]
                && let Some(paint) = gradient
            {
                for stop in &mut paint.stops {
                    stop.color[3] = after[3];
                }
            }
        }
    }
    true
}
