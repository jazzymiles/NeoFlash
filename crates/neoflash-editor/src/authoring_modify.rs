//! Classic authoring layout operations. Every command edits the sampled pose
//! at the current frame and keeps placement identity and scripts intact.
use crate::theme::{DesignUi as _, ScrollTheme as _};
use crate::{
    app::Editor,
    paint_controls::{number_field, two_column_row},
    selection::SelectionTarget,
    stage_mapping::StageMapping,
    workspace::PanelId,
};
use eframe::egui::{self, Pos2, Rect, Vec2};
use neoflash_core::*;
use neoflash_ui::{
    Icon, TextStyle,
    components::{
        Button, Checkbox, ColorField, ColorPicker, ColorWell, ColorWellTarget, DragField, Field,
        IconButton, IconGroup, MenuItem, Segmented, Select,
        color_picker::{Hsb, hsv},
    },
    icons::{i16, i20},
    tokens::space,
    typography::rich,
};
use std::collections::{HashMap, HashSet};

/// Vertical scroll body of a padded inspector panel. A form row ends at the body's right edge,
/// so the floating scrollbar is laid out in the panel's right padding (`PanelId::body_padding`)
/// instead of over the last points of every row; the rows keep their width whether the bar shows
/// or not. The area never shrinks to its content.
pub(crate) fn panel_scroll_body<R>(
    ui: &mut egui::Ui,
    panel: PanelId,
    id_salt: &str,
    body: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let gutter = panel.body_padding();
    let rect = ui.available_rect_before_wrap();
    let mut outer = rect;
    outer.max.x += gutter;
    let mut clip = ui.clip_rect();
    // Only a body that reaches its clip edge owns the padding next to it.
    if (clip.right() - rect.right()).abs() < 0.5 {
        clip.max.x += gutter;
    }
    let mut child = ui.new_child(egui::UiBuilder::new().id(ui.id()).max_rect(outer));
    child.set_clip_rect(clip);
    let output = egui::ScrollArea::vertical()
        .id_salt(id_salt)
        .auto_shrink([false, false])
        .themed()
        .show(&mut child, |ui| {
            // Rows still end, and clip, at the body's edge; only the scrollbar uses the gutter.
            ui.set_max_width(rect.width());
            let mut content_clip = ui.clip_rect();
            content_clip.max.x = content_clip.max.x.min(rect.right());
            ui.set_clip_rect(content_clip);
            body(ui)
        });
    ui.allocate_rect(rect, egui::Sense::hover());
    output.inner
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PaintFillType {
    None,
    Solid,
    LinearGradient,
    RadialGradient,
    BitmapFill,
}

impl PaintFillType {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Solid => "Solid colour",
            Self::LinearGradient => "Linear gradient",
            Self::RadialGradient => "Radial gradient",
            Self::BitmapFill => "Bitmap fill",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ActiveColorPopover {
    pub source_id: String,
    pub anchor_rect: Rect,
    pub stroke_well: bool,
    pub selection_mode: bool,
}

fn active_color_popover_id() -> egui::Id {
    egui::Id::new("nf2-active-color-popover")
}

pub(crate) fn request_color_popover(
    ctx: &egui::Context,
    source_id: &str,
    anchor_rect: Rect,
    stroke_well: bool,
    selection_mode: bool,
) {
    let current = ctx.data(|d| d.get_temp::<ActiveColorPopover>(active_color_popover_id()));
    if current.as_ref().is_some_and(|p| p.source_id == source_id) {
        ctx.data_mut(|d| d.remove::<ActiveColorPopover>(active_color_popover_id()));
    } else {
        ctx.data_mut(|d| {
            d.insert_temp(egui::Id::new("color-panel-active-stroke"), stroke_well);
            d.insert_temp(
                active_color_popover_id(),
                ActiveColorPopover {
                    source_id: source_id.to_owned(),
                    anchor_rect,
                    stroke_well,
                    selection_mode,
                },
            );
        });
    }
}

pub(crate) fn update_color_popover_anchor(ctx: &egui::Context, source_id: &str, anchor_rect: Rect) {
    ctx.data_mut(|d| {
        if let Some(mut p) = d.get_temp::<ActiveColorPopover>(active_color_popover_id())
            && p.source_id == source_id
        {
            p.anchor_rect = anchor_rect;
            d.insert_temp(active_color_popover_id(), p);
        }
    });
}

pub(crate) fn close_color_popover(ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove::<ActiveColorPopover>(active_color_popover_id()));
}

/// Whether the NF2 Color & Fill Type popover is open in `ctx`.
pub(crate) fn is_color_popover_open(ctx: &egui::Context) -> bool {
    ctx.data(|d| {
        d.get_temp::<ActiveColorPopover>(active_color_popover_id())
            .is_some()
    })
}

#[derive(Clone)]
pub(crate) struct ActiveDistortSession {
    pub symbol: Id,
    pub frame: u32,
    pub targets: Vec<SelectionTarget>,
    pub source_bounds: crate::transform_geometry::Bounds,
    pub quad: crate::transform_geometry::DistortQuad,
    pub base_elements: Vec<(SelectionTarget, Element)>,
    pub last_applied_elements: Vec<(SelectionTarget, Element)>,
}

#[derive(Clone)]
pub(crate) struct ActiveEnvelopeSession {
    pub symbol: Id,
    pub frame: u32,
    pub targets: Vec<SelectionTarget>,
    pub cage: crate::transform_geometry::EnvelopeCage,
    pub base_elements: Vec<(SelectionTarget, Element)>,
    pub last_applied_elements: Vec<(SelectionTarget, Element)>,
}

pub(crate) struct AuthoringModify {
    pub to_stage: bool,
    pub horizontal_gap: f64,
    pub vertical_gap: f64,
    pub linked_scale: bool,
    pub color_selection: bool,
    pub color_model_hsb: bool,
    pub selected_stop: usize,
    pub drawing_fill_gradient: Option<GradientPaint>,
    pub drawing_stroke_gradient: Option<GradientPaint>,
    pub drawing_bitmap_fill: Option<usize>,
    /// Transient world-space transformation point for a multi-selection in Free Transform.
    pub group_pivot: Option<(Vec<SelectionTarget>, [f64; 2])>,
    pub distort_session: Option<ActiveDistortSession>,
    pub envelope_session: Option<ActiveEnvelopeSession>,
    pub envelope_backups: HashMap<(Id, u32, Id), Element>,
    pub scale_and_rotate_dialog: Option<(f64, f64)>,
}
impl Default for AuthoringModify {
    fn default() -> Self {
        Self {
            to_stage: false,
            horizontal_gap: 16.,
            vertical_gap: 16.,
            linked_scale: true,
            color_selection: true,
            color_model_hsb: false,
            selected_stop: 0,
            drawing_fill_gradient: None,
            drawing_stroke_gradient: None,
            drawing_bitmap_fill: None,
            group_pivot: None,
            distort_session: None,
            envelope_session: None,
            envelope_backups: HashMap::new(),
            scale_and_rotate_dialog: None,
        }
    }
}

#[derive(Clone)]
struct Placement {
    target: SelectionTarget,
    element: Element,
    bounds: Rect,
    context_bounds: Rect,
}

/// An authoring group is one layout object, even though runtime placements
/// stay flat. Aligning a group must never align its children to each other.
struct LayoutUnit {
    members: Vec<Placement>,
    bounds: Rect,
    context_bounds: Rect,
}

#[derive(Clone, Copy)]
pub(crate) enum Arrange {
    Front,
    Forward,
    Backward,
    Back,
}

#[derive(Clone)]
struct DistributeUnit {
    members: Vec<Element>,
    group_name: Option<String>,
    before_first_unselected: bool,
}

#[derive(Clone)]
struct SourceLayerDistribution {
    layer_id: Id,
    units: Vec<DistributeUnit>,
    unselected_count: usize,
}

fn is_default_element_name(name: &str, id: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed == id {
        return true;
    }
    let Some((prefix, suffix)) = trimmed.rsplit_once('_') else {
        return false;
    };
    matches!(
        prefix,
        "rectangle"
            | "oval"
            | "ellipse"
            | "text"
            | "clip"
            | "instance"
            | "path"
            | "shape"
            | "bitmap"
            | "video"
            | "object"
    ) && !suffix.is_empty()
        && suffix.chars().all(|c| c.is_ascii_digit())
}

fn sanitize_layer_name_snippet(raw: &str) -> Option<String> {
    let first_line = raw.lines().map(str::trim).find(|line| !line.is_empty())?;
    let collapsed: String = first_line
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .take(24)
        .collect();
    let trimmed = collapsed.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn distribute_unit_base_name(project: &Project, unit: &DistributeUnit) -> String {
    if unit.members.len() > 1
        && let Some(group_name) = unit
            .group_name
            .as_deref()
            .and_then(sanitize_layer_name_snippet)
    {
        return group_name;
    }
    let Some(el) = unit.members.first() else {
        return "Layer".to_owned();
    };
    if !is_default_element_name(&el.name, &el.id)
        && let Some(explicit) = sanitize_layer_name_snippet(&el.name)
    {
        return explicit;
    }
    match &el.kind {
        ElementKind::Instance { symbol_id, .. } => project
            .symbol(symbol_id)
            .and_then(|sym| sanitize_layer_name_snippet(&sym.name))
            .unwrap_or_else(|| "Instance".to_owned()),
        ElementKind::Text { text, .. } | ElementKind::TextBox { text, .. } => {
            sanitize_layer_name_snippet(text).unwrap_or_else(|| "Text".to_owned())
        }
        ElementKind::Bitmap { resource_id, .. } => project
            .resources
            .iter()
            .find(|r| r.id == *resource_id)
            .and_then(|r| std::path::Path::new(&r.path).file_stem())
            .and_then(|s| s.to_str())
            .and_then(sanitize_layer_name_snippet)
            .unwrap_or_else(|| "Bitmap".to_owned()),
        ElementKind::Video { .. } => "Video".to_owned(),
        ElementKind::Rectangle { .. } => "Rectangle".to_owned(),
        ElementKind::Ellipse { .. } => "Oval".to_owned(),
        ElementKind::Path { .. } | ElementKind::PaintedPath { .. } => "Shape".to_owned(),
    }
}

fn unique_distributed_layer_name(base: &str, used_names: &mut HashSet<String>) -> String {
    if used_names.insert(base.to_owned()) {
        return base.to_owned();
    }
    let mut n = 2usize;
    loop {
        let candidate = format!("{base} {n}");
        if used_names.insert(candidate.clone()) {
            return candidate;
        }
        n += 1;
    }
}

mod break_apart;
mod fill_type;
mod panels_align_transform;
mod panels_color;
#[cfg(test)]
mod tests;

pub(crate) use fill_type::{
    default_gradient_matrix, element_has_fill, element_has_stroke, ensure_solid_non_none,
    update_gradient_for_color_edit,
};

impl Editor {
    fn modify_stage_mapping(&self) -> Option<StageMapping> {
        if self.editing_in_place() {
            self.edit_local_to_context_matrix()
                .ok()
                .and_then(|matrix| StageMapping::try_new(matrix).ok())
        } else {
            Some(StageMapping::identity())
        }
    }

    fn modify_placements(&self, ctx: &egui::Context) -> Vec<Placement> {
        let Some(mapping) = self.modify_stage_mapping() else {
            return vec![];
        };
        let painter = ctx.layer_painter(egui::LayerId::background());
        let mut bounds = HashMap::<String, (Rect, Rect)>::new();
        for draw in evaluate(&self.project, self.symbol_id(), self.frame) {
            if let Some(id) = draw.instance_path.first() {
                let rect = crate::stage::world_bounds(&draw, &painter);
                let mut mapped = draw.clone();
                mapped.transform = mapping.map_matrix(mapped.transform);
                let context_rect = crate::stage::world_bounds(&mapped, &painter);
                if rect.is_finite() && context_rect.is_finite() {
                    bounds
                        .entry(id.clone())
                        .and_modify(|(local, context)| {
                            *local = local.union(rect);
                            *context = context.union(context_rect);
                        })
                        .or_insert((rect, context_rect));
                }
            }
        }
        self.sampled_targets()
            .into_iter()
            .filter(|(target, _)| self.target_editable(target))
            .filter_map(|(target, element)| {
                let (bounds, context_bounds) = match bounds.remove(&element.id) {
                    Some(pair) => pair,
                    None if element.transform.x.is_finite() && element.transform.y.is_finite() => {
                        let fallback =
                            Pos2::new(element.transform.x as f32, element.transform.y as f32);
                        let stage_pos = mapping.edit_to_stage_pos(fallback);
                        if !stage_pos.x.is_finite() || !stage_pos.y.is_finite() {
                            return None;
                        }
                        (
                            Rect::from_min_size(fallback, Vec2::ZERO),
                            Rect::from_min_size(stage_pos, Vec2::ZERO),
                        )
                    }
                    None => return None,
                };
                Some(Placement {
                    target,
                    element,
                    bounds,
                    context_bounds,
                })
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

    fn layout_group(&self, target: &SelectionTarget) -> Option<(&str, &AuthoringGroup)> {
        let layer = &self.symbol().timeline.layers[self.target_layer_index(target)?];
        let group = layer
            .properties
            .groups
            .iter()
            .find(|group| group.members.contains(&target.element))?;
        Some((&layer.id, group))
    }

    /// Menu enablement only needs to know whether there are enough editable units. Avoid sampling
    /// the entire scene and measuring text for each refresh of the native menu.
    pub(crate) fn can_align_selection(&self) -> bool {
        let to_stage = self.authoring_modify.to_stage;
        let required_units = if to_stage { 1 } else { 2 };
        if !self.has_document()
            || self.selection.symbol != self.symbol_id()
            || self.selection.len() < required_units
            || self.modify_stage_mapping().is_none()
        {
            return false;
        }
        let mut targets = self.selection.targets.iter();
        let Some(first) = targets.find(|target| self.target_editable(target)) else {
            return false;
        };
        if to_stage {
            return true;
        }
        if let Some((layer, group)) = self.layout_group(first) {
            let members: HashSet<_> = group.members.iter().map(String::as_str).collect();
            targets.any(|target| {
                (target.layer != layer || !members.contains(target.element.as_str()))
                    && self.target_editable(target)
            })
        } else {
            targets.any(|target| self.target_editable(target))
        }
    }

    fn modify_layout_units(&self, ctx: &egui::Context) -> Vec<LayoutUnit> {
        let mut units: Vec<LayoutUnit> = Vec::new();
        let mut groups = HashMap::<(&str, &str), usize>::new();
        for placement in self.modify_placements(ctx) {
            let group = self
                .layout_group(&placement.target)
                .map(|(layer, group)| (layer, group.id.as_str()));
            if let Some(index) = group.and_then(|group| groups.get(&group)).copied() {
                units[index].bounds = units[index].bounds.union(placement.bounds);
                units[index].context_bounds =
                    units[index].context_bounds.union(placement.context_bounds);
                units[index].members.push(placement);
            } else {
                if let Some(group) = group {
                    groups.insert(group, units.len());
                }
                units.push(LayoutUnit {
                    bounds: placement.bounds,
                    context_bounds: placement.context_bounds,
                    members: vec![placement],
                });
            }
        }
        units
    }

    fn write_modified(&mut self, changes: Vec<(SelectionTarget, Element)>) {
        let multiple = self.timeline_edit.edit_multiple;
        let (onion_start, onion_end) = self.onion_range();
        for (target, element) in changes {
            self.ensure_key_for(&target.layer, false);
            let Some(index) = self.target_layer_index(&target) else {
                continue;
            };
            if element.transform.skew != 0.
                || !element.transform.color_transform.is_identity()
                || matches!(element.kind, ElementKind::PaintedPath { .. })
            {
                self.project.format_version = self.project.format_version.max(5);
            }
            let frame = self.frame;
            let prev_transform = self.symbol().timeline.layers[index]
                .key_at(frame)
                .and_then(|key| key.elements.iter().find(|e| e.id == target.element))
                .map(|e| e.transform);
            let own_key_frame = self.symbol().timeline.layers[index]
                .key_at(frame)
                .map(|key| key.frame);
            if let Some(current) = self.symbol_mut().timeline.layers[index]
                .key_at_mut(frame)
                .and_then(|key| key.elements.iter_mut().find(|e| e.id == target.element))
            {
                current.transform = element.transform;
                current.kind = element.kind;
            }
            if multiple && let Some(old_t) = prev_transform {
                let dx = element.transform.x - old_t.x;
                let dy = element.transform.y - old_t.y;
                let sx_ratio = if old_t.scale_x.abs() > 1e-9 {
                    element.transform.scale_x / old_t.scale_x
                } else {
                    1.0
                };
                let sy_ratio = if old_t.scale_y.abs() > 1e-9 {
                    element.transform.scale_y / old_t.scale_y
                } else {
                    1.0
                };
                let drot = element.transform.rotation - old_t.rotation;
                let dskew = element.transform.skew - old_t.skew;
                for key in &mut self.symbol_mut().timeline.layers[index].keyframes {
                    if !(onion_start..=onion_end).contains(&key.frame)
                        || Some(key.frame) == own_key_frame
                    {
                        continue;
                    }
                    if let Some(other) = key.elements.iter_mut().find(|e| e.id == target.element) {
                        let mut candidate = other.transform;
                        candidate.x += dx;
                        candidate.y += dy;
                        candidate.scale_x *= sx_ratio;
                        candidate.scale_y *= sy_ratio;
                        candidate.rotation += drot;
                        candidate.skew += dskew;
                        if valid_transform(candidate) {
                            other.transform = candidate;
                        }
                    }
                }
            }
        }
    }

    #[allow(dead_code)]
    pub(crate) fn skew_selection(
        &mut self,
        ctx: &egui::Context,
        skew_x_deg: f64,
        skew_y_deg: f64,
    ) -> bool {
        let placements = self.modify_placements(ctx);
        if placements.is_empty() {
            return false;
        }
        let pivot = self
            .authoring_modify
            .group_pivot
            .as_ref()
            .filter(|(targets, _)| *targets == self.selection.targets)
            .map(|(_, p)| Pos2::new(p[0] as f32, p[1] as f32))
            .unwrap_or_else(|| {
                placements
                    .iter()
                    .fold(Rect::NOTHING, |b, p| b.union(p.bounds))
                    .center()
            });
        let shx = skew_x_deg.to_radians().tan();
        let shy = skew_y_deg.to_radians().tan();
        if !shx.is_finite() || !shy.is_finite() {
            return false;
        }
        let matrix = around(pivot, [1.0, shy, shx, 1.0, 0.0, 0.0]);
        let Some(changes) = transformed(&placements, matrix) else {
            return false;
        };
        self.apply_modified(changes, None);
        true
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
            if self.edit_context_is_isolated() {
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
        let changes = if self.authoring_modify.to_stage {
            let Some(mapping) = self.modify_stage_mapping() else {
                return;
            };
            units
                .into_iter()
                .flat_map(|unit| {
                    let offset = at - edge_at(unit.context_bounds, axis, edge);
                    translated_unit_context(unit, mapping, axis, offset)
                })
                .collect()
        } else {
            units
                .into_iter()
                .flat_map(|unit| {
                    translated_unit(unit, axis, |bounds| at - edge_at(bounds, axis, edge))
                })
                .collect()
        };
        self.apply_modified(changes, None);
    }

    pub(crate) fn distribute_selection(&mut self, ctx: &egui::Context, axis: usize, edge: usize) {
        let mut units = self.modify_layout_units(ctx);
        let to_stage = self.authoring_modify.to_stage;
        let min_units = if to_stage { 2 } else { 3 };
        if units.len() < min_units {
            return;
        }
        units.sort_by(|a, b| {
            let a = if to_stage { a.context_bounds } else { a.bounds };
            let b = if to_stage { b.context_bounds } else { b.bounds };
            edge_at(a, axis, edge).total_cmp(&edge_at(b, axis, edge))
        });
        let reference = self.modify_reference(&units);
        let first = if to_stage {
            let item = &units[0];
            reference.min[axis] as f64 + edge as f64 * extent(item.context_bounds, axis) / 2.
        } else {
            edge_at(units[0].bounds, axis, edge)
        };
        let last = if to_stage {
            let item = units.last().unwrap();
            reference.max[axis] as f64 - (2 - edge) as f64 * extent(item.context_bounds, axis) / 2.
        } else {
            edge_at(units.last().unwrap().bounds, axis, edge)
        };
        let step = (last - first) / (units.len() - 1) as f64;
        let changes = if to_stage {
            let Some(mapping) = self.modify_stage_mapping() else {
                return;
            };
            units
                .into_iter()
                .enumerate()
                .flat_map(|(index, unit)| {
                    let offset =
                        first + step * index as f64 - edge_at(unit.context_bounds, axis, edge);
                    translated_unit_context(unit, mapping, axis, offset)
                })
                .collect()
        } else {
            units
                .into_iter()
                .enumerate()
                .flat_map(|(index, unit)| {
                    translated_unit(unit, axis, |bounds| {
                        first + step * index as f64 - edge_at(bounds, axis, edge)
                    })
                })
                .collect()
        };
        self.apply_modified(changes, None);
    }

    /// None keeps the outer bounds and evens the gaps. Some sets an exact gap,
    /// including a negative overlap, keeping the first edge fixed.
    pub(crate) fn space_selection(&mut self, ctx: &egui::Context, axis: usize, gap: Option<f64>) {
        let mut units = self.modify_layout_units(ctx);
        if units.len() < 2 {
            return;
        }
        let to_stage = self.authoring_modify.to_stage;
        units.sort_by(|a, b| {
            let a = if to_stage { a.context_bounds } else { a.bounds };
            let b = if to_stage { b.context_bounds } else { b.bounds };
            a.min[axis].total_cmp(&b.min[axis])
        });
        let reference = self.modify_reference(&units);
        let gap = gap.unwrap_or_else(|| {
            (extent(reference, axis)
                - units
                    .iter()
                    .map(|unit| {
                        extent(
                            if to_stage {
                                unit.context_bounds
                            } else {
                                unit.bounds
                            },
                            axis,
                        )
                    })
                    .sum::<f64>())
                / (units.len() - 1) as f64
        });
        let mut at = if to_stage {
            reference.min[axis] as f64
        } else {
            units[0].bounds.min[axis] as f64
        };
        let changes = if to_stage {
            let Some(mapping) = self.modify_stage_mapping() else {
                return;
            };
            units
                .into_iter()
                .flat_map(|unit| {
                    let offset = at - unit.context_bounds.min[axis] as f64;
                    at += extent(unit.context_bounds, axis) + gap;
                    translated_unit_context(unit, mapping, axis, offset)
                })
                .collect()
        } else {
            units
                .into_iter()
                .flat_map(|unit| {
                    let offset = at - unit.bounds.min[axis] as f64;
                    at += extent(unit.bounds, axis) + gap;
                    translated_unit(unit, axis, |_| offset)
                })
                .collect()
        };
        self.apply_modified(changes, None);
    }

    pub(crate) fn match_selection_size(&mut self, ctx: &egui::Context, axis: usize) {
        let units = self.modify_layout_units(ctx);
        if units.is_empty() {
            return;
        }
        let to_stage = self.authoring_modify.to_stage;
        let size = if to_stage {
            extent(self.modify_reference(&units), axis)
        } else {
            units
                .iter()
                .map(|p| extent(p.bounds, axis))
                .fold(0., f64::max)
        };
        if !size.is_finite() || size <= 1e-9 {
            return;
        }
        let mapping = if to_stage {
            let Some(mapping) = self.modify_stage_mapping() else {
                return;
            };
            Some(mapping)
        } else {
            None
        };
        let mut changes = Vec::new();
        for unit in units {
            let old = extent(
                if to_stage {
                    unit.context_bounds
                } else {
                    unit.bounds
                },
                axis,
            );
            if !old.is_finite() || old <= 1e-9 {
                continue;
            }
            let ratio = size / old;
            if !ratio.is_finite() || (ratio - 1.0).abs() <= 1e-9 {
                continue;
            }
            let mut scale = [1., 1.];
            scale[axis] = ratio;
            let matrix = if let Some(mapping) = mapping {
                let context = around(
                    unit.context_bounds.min,
                    [scale[0], 0., 0., scale[1], 0., 0.],
                );
                let Some(inverse) = invert_affine_matrix(mapping.matrix()) else {
                    return;
                };
                affine_multiply(inverse, affine_multiply(context, mapping.matrix()))
            } else {
                around(unit.bounds.min, [scale[0], 0., 0., scale[1], 0., 0.])
            };
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

    pub(crate) fn rotate_90_cw_selection(&mut self, ctx: &egui::Context) {
        self.affine_selection(ctx, [0., 1., -1., 0., 0., 0.]);
    }

    pub(crate) fn rotate_90_ccw_selection(&mut self, ctx: &egui::Context) {
        self.affine_selection(ctx, [0., -1., 1., 0., 0., 0.]);
    }

    pub(crate) fn flip_horizontal_selection(&mut self, ctx: &egui::Context) {
        self.affine_selection(ctx, [-1., 0., 0., 1., 0., 0.]);
    }

    pub(crate) fn flip_vertical_selection(&mut self, ctx: &egui::Context) {
        self.affine_selection(ctx, [1., 0., 0., -1., 0., 0.]);
    }

    pub(crate) fn can_distort_or_envelope_selection(&self) -> bool {
        let sampled = self.sampled_targets();
        !sampled.is_empty()
            && sampled.iter().all(|(target, element)| {
                self.target_editable(target)
                    && crate::transform_geometry::element_supports_mesh_deform(element)
            })
    }

    pub(crate) fn can_delete_envelope_selection(&self) -> bool {
        let sampled = self.sampled_targets();
        if sampled.is_empty() {
            return false;
        }
        let symbol_id = self.symbol_id();
        let frame = self.frame;
        let has_backup = sampled.iter().any(|(target, element)| {
            self.target_editable(target)
                && self.authoring_modify.envelope_backups.contains_key(&(
                    symbol_id.to_owned(),
                    frame,
                    element.id.clone(),
                ))
        });
        has_backup
            || (self.can_distort_or_envelope_selection()
                && (self.authoring_modify.envelope_session.is_some()
                    || (self.tool == crate::app::Tool::Transform
                        && self.authoring_tools.transform_mode
                            == crate::authoring_tools::FreeTransformMode::Envelope)))
    }

    pub(crate) fn delete_envelope_selection(&mut self) -> bool {
        self.finish_edits();
        let sampled = self.sampled_targets();
        if sampled.is_empty() {
            return false;
        }
        let symbol_id = self.symbol_id().to_owned();
        let frame = self.frame;
        let mut restored = Vec::new();
        for (target, element) in &sampled {
            if !self.target_editable(target) {
                continue;
            }
            if let Some(backup) = self.authoring_modify.envelope_backups.remove(&(
                symbol_id.clone(),
                frame,
                element.id.clone(),
            )) {
                restored.push((target.clone(), backup));
            }
        }
        let had_session = self.authoring_modify.envelope_session.take().is_some();
        let was_envelope_mode = self.tool == crate::app::Tool::Transform
            && self.authoring_tools.transform_mode
                == crate::authoring_tools::FreeTransformMode::Envelope;
        if was_envelope_mode {
            self.authoring_tools.transform_mode =
                crate::authoring_tools::FreeTransformMode::FreeTransform;
        }
        if !restored.is_empty() {
            self.playing = false;
            self.transact_named("Delete Envelope", |editor| editor.write_modified(restored));
            self.status = "Deleted envelope.".to_owned();
            true
        } else if had_session || was_envelope_mode {
            self.status = "Deleted envelope.".to_owned();
            true
        } else {
            false
        }
    }

    pub(crate) fn reset_selection_transform(&mut self, ctx: &egui::Context) {
        self.authoring_modify.distort_session = None;
        self.authoring_modify.envelope_session = None;
        let symbol_id = self.symbol_id().to_owned();
        let frame = self.frame;
        let changes = self
            .modify_placements(ctx)
            .into_iter()
            .map(|mut item| {
                if let Some(backup) = self.authoring_modify.envelope_backups.remove(&(
                    symbol_id.clone(),
                    frame,
                    item.element.id.clone(),
                )) {
                    item.element = backup;
                }
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

    pub(crate) fn can_arrange_selection(&self, direction: Option<Arrange>) -> bool {
        let targets: HashSet<_> = self
            .selection
            .targets
            .iter()
            .filter(|t| self.target_editable(t))
            .cloned()
            .collect();
        if targets.is_empty() {
            return false;
        }
        let frame = self.frame;
        let timeline = &self.symbol().timeline;
        let layer_ids: HashSet<_> = targets.iter().map(|t| t.layer.as_str()).collect();
        for layer_id in layer_ids {
            let Some(layer) = timeline.layers.iter().find(|l| l.id == layer_id) else {
                continue;
            };
            let elements =
                sample_layer_in_at_fps(timeline, layer, frame, f64::from(self.project.fps));
            if elements.len() < 2 {
                continue;
            }
            match direction {
                Some(dir) => {
                    if Self::reordered_layer_elements(
                        &elements,
                        &layer.properties.groups,
                        &targets,
                        layer_id,
                        dir,
                    )
                    .is_some()
                    {
                        return true;
                    }
                }
                None => {
                    if [
                        Arrange::Front,
                        Arrange::Forward,
                        Arrange::Backward,
                        Arrange::Back,
                    ]
                    .into_iter()
                    .any(|dir| {
                        Self::reordered_layer_elements(
                            &elements,
                            &layer.properties.groups,
                            &targets,
                            layer_id,
                            dir,
                        )
                        .is_some()
                    }) {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn reordered_layer_elements(
        elements: &[Element],
        groups: &[AuthoringGroup],
        targets: &HashSet<SelectionTarget>,
        layer_id: &str,
        direction: Arrange,
    ) -> Option<Vec<Element>> {
        if elements.len() < 2 {
            return None;
        }
        let mut selected_ids: HashSet<&str> = elements
            .iter()
            .filter(|e| {
                targets.contains(&SelectionTarget {
                    layer: layer_id.to_owned(),
                    element: e.id.clone(),
                })
            })
            .map(|e| e.id.as_str())
            .collect();
        if selected_ids.is_empty() {
            return None;
        }
        // Any selected member of an authoring group moves its whole group as an atomic depth unit.
        for group in groups {
            if group
                .members
                .iter()
                .any(|m| selected_ids.contains(m.as_str()))
            {
                for member in &group.members {
                    selected_ids.insert(member.as_str());
                }
            }
        }
        let mut units: Vec<(bool, Vec<Element>)> = Vec::with_capacity(elements.len());
        let mut consumed: HashSet<&str> = HashSet::new();
        for element in elements {
            if !consumed.insert(element.id.as_str()) {
                continue;
            }
            if let Some(group) = groups
                .iter()
                .find(|g| g.members.iter().any(|m| m == &element.id))
            {
                let group_set: HashSet<&str> = group.members.iter().map(String::as_str).collect();
                let mut unit = Vec::with_capacity(group.members.len());
                for item in elements {
                    if group_set.contains(item.id.as_str()) {
                        consumed.insert(item.id.as_str());
                        unit.push(item.clone());
                    }
                }
                let is_sel = selected_ids.contains(element.id.as_str());
                units.push((is_sel, unit));
            } else {
                let is_sel = selected_ids.contains(element.id.as_str());
                units.push((is_sel, vec![element.clone()]));
            }
        }
        if units.len() < 2 {
            return None;
        }
        match direction {
            Arrange::Front => units.sort_by_key(|(sel, _)| *sel),
            Arrange::Back => units.sort_by_key(|(sel, _)| !*sel),
            Arrange::Forward => {
                for index in (0..units.len().saturating_sub(1)).rev() {
                    if units[index].0 && !units[index + 1].0 {
                        units.swap(index, index + 1);
                    }
                }
            }
            Arrange::Backward => {
                for index in 1..units.len() {
                    if units[index].0 && !units[index - 1].0 {
                        units.swap(index, index - 1);
                    }
                }
            }
        }
        let reordered: Vec<Element> = units.into_iter().flat_map(|(_, u)| u).collect();
        let changed = reordered
            .iter()
            .zip(elements.iter())
            .any(|(a, b)| a.id != b.id);
        changed.then_some(reordered)
    }

    pub(crate) fn arrange_selection(&mut self, direction: Arrange) {
        self.finish_edits();
        if !self.can_arrange_selection(Some(direction)) {
            return;
        }
        let targets: HashSet<_> = self
            .selection
            .targets
            .iter()
            .filter(|t| self.target_editable(t))
            .cloned()
            .collect();
        self.playing = false;
        self.transact(|editor| {
            let mut layers: Vec<_> = targets.iter().map(|t| t.layer.clone()).collect();
            layers.sort();
            layers.dedup();
            for layer_id in layers {
                let frame = editor.frame;
                let can_reorder = editor
                    .symbol()
                    .timeline
                    .layers
                    .iter()
                    .find(|l| l.id == layer_id)
                    .is_some_and(|layer| {
                        let sampled = sample_layer_in_at_fps(
                            &editor.symbol().timeline,
                            layer,
                            frame,
                            f64::from(editor.project.fps),
                        );
                        Self::reordered_layer_elements(
                            &sampled,
                            &layer.properties.groups,
                            &targets,
                            &layer_id,
                            direction,
                        )
                        .is_some()
                    });
                if !can_reorder {
                    continue;
                }
                editor.ensure_key_for(&layer_id, false);
                let layer = editor
                    .symbol_mut()
                    .timeline
                    .layers
                    .iter_mut()
                    .find(|l| l.id == layer_id)
                    .unwrap();
                let groups = layer.properties.groups.clone();
                let elements = &mut layer.key_at_mut(frame).unwrap().elements;
                if let Some(reordered) = Self::reordered_layer_elements(
                    elements, &groups, &targets, &layer_id, direction,
                ) {
                    *elements = reordered;
                }
            }
        });
    }

    pub(crate) fn can_group_selection(&self) -> bool {
        let targets = &self.selection.targets;
        if targets.is_empty() || targets.iter().any(|target| !self.target_editable(target)) {
            return false;
        }
        for target in targets {
            let Some(layer_index) = self.target_layer_index(target) else {
                return false;
            };
            let layer = &self.symbol().timeline.layers[layer_index];
            if !layer
                .key_at(self.frame)
                .is_some_and(|key| key.elements.iter().any(|e| e.id == target.element))
            {
                return false;
            }
        }
        let first_layer = &targets[0].layer;
        if targets.iter().all(|target| &target.layer == first_layer) {
            let Some(layer_index) = self.target_layer_index(&targets[0]) else {
                return false;
            };
            let layer = &self.symbol().timeline.layers[layer_index];
            let members: HashSet<_> = targets
                .iter()
                .map(|target| target.element.as_str())
                .collect();
            if layer.properties.groups.iter().any(|group| {
                group.members.len() == members.len()
                    && group
                        .members
                        .iter()
                        .all(|member| members.contains(member.as_str()))
            }) {
                return false;
            }
        }
        true
    }

    pub(crate) fn can_ungroup_selection(&self) -> bool {
        if self.selection.is_empty() {
            return false;
        }
        self.symbol().timeline.layers.iter().any(|layer| {
            layer.properties.groups.iter().any(|group| {
                group.members.iter().any(|element| {
                    self.selection.targets.contains(&SelectionTarget {
                        layer: layer.id.clone(),
                        element: element.clone(),
                    })
                })
            })
        })
    }

    pub(crate) fn group_selection(&mut self) {
        self.finish_edits();
        let targets = self.selection.targets.clone();
        if targets.is_empty() {
            return;
        }
        if targets.iter().any(|target| !self.target_editable(target)) {
            self.message("This layer is locked or hidden.");
            return;
        }
        for target in &targets {
            let Some(layer_index) = self.target_layer_index(target) else {
                return;
            };
            let layer = &self.symbol().timeline.layers[layer_index];
            if !layer
                .key_at(self.frame)
                .is_some_and(|key| key.elements.iter().any(|e| e.id == target.element))
            {
                self.message(
                    "Group objects on the current keyframe. Turn off Edit multiple frames or move the playhead to their keyframe.",
                );
                return;
            }
        }
        let same_layer = targets
            .iter()
            .all(|target| target.layer == targets[0].layer);
        if same_layer {
            let layer_id = targets[0].layer.clone();
            let layer_index = self.target_layer_index(&targets[0]).unwrap();
            let layer = &self.symbol().timeline.layers[layer_index];
            let mut members: HashSet<_> = targets
                .iter()
                .map(|target| target.element.clone())
                .collect();
            for group in &layer.properties.groups {
                if group.members.iter().any(|id| members.contains(id)) {
                    members.extend(group.members.iter().cloned());
                }
            }
            if layer.properties.groups.iter().any(|group| {
                group.members.len() == members.len()
                    && group.members.iter().all(|member| members.contains(member))
            }) {
                self.message("The selected objects are already one group.");
                return;
            }
            let key = layer.key_at(self.frame).unwrap();
            let indices = key
                .elements
                .iter()
                .enumerate()
                .filter(|(_, element)| members.contains(&element.id))
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            let is_contiguous = indices
                .first()
                .zip(indices.last())
                .is_some_and(|(first, last)| last - first + 1 == indices.len());
            let last_index = indices.last().copied().unwrap_or(0);
            let mut remaining = members.clone();
            let mut ordered = Vec::new();
            for element in &key.elements {
                if remaining.remove(&element.id) {
                    ordered.push(element.id.clone());
                }
            }
            for element in layer.keyframes.iter().flat_map(|k| &k.elements) {
                if remaining.remove(&element.id) {
                    ordered.push(element.id.clone());
                }
            }
            let name = format!("Group {}", layer.properties.groups.len() + 1);
            let new_selection: Vec<SelectionTarget> = ordered
                .iter()
                .map(|elem_id| SelectionTarget {
                    layer: layer_id.clone(),
                    element: elem_id.clone(),
                })
                .collect();
            self.playing = false;
            self.transact(|editor| {
                editor.project.format_version = editor.project.format_version.max(4);
                if !is_contiguous {
                    editor.ensure_key_for(&layer_id, false);
                }
                let frame = editor.frame;
                let layer = editor
                    .symbol_mut()
                    .timeline
                    .layers
                    .iter_mut()
                    .find(|l| l.id == layer_id)
                    .unwrap();
                if !is_contiguous && let Some(key) = layer.key_at_mut(frame) {
                    let mut before = Vec::with_capacity(key.elements.len());
                    let mut grouped = Vec::with_capacity(indices.len());
                    let mut after = Vec::new();
                    for (idx, el) in key.elements.drain(..).enumerate() {
                        if members.contains(&el.id) {
                            grouped.push(el);
                        } else if idx < last_index {
                            before.push(el);
                        } else {
                            after.push(el);
                        }
                    }
                    before.extend(grouped);
                    before.extend(after);
                    key.elements = before;
                }
                layer
                    .properties
                    .groups
                    .retain(|group| !group.members.iter().any(|id| ordered.contains(id)));
                layer.properties.groups.push(AuthoringGroup {
                    id: new_id("group"),
                    name,
                    members: ordered,
                });
                editor.set_selection(new_selection);
            });
            return;
        }

        // Multi-layer selection: move all selected objects (and any groups they belong to on their
        // respective layers) onto the topmost selected layer in back-to-front visual order.
        let dest_layer_index = targets
            .iter()
            .filter_map(|target| self.target_layer_index(target))
            .max()
            .unwrap();
        let dest_layer_id = self.symbol().timeline.layers[dest_layer_index].id.clone();
        let mut selected_by_layer: HashMap<Id, HashSet<Id>> = HashMap::new();
        for target in &targets {
            selected_by_layer
                .entry(target.layer.clone())
                .or_default()
                .insert(target.element.clone());
        }
        for layer in &self.symbol().timeline.layers {
            if let Some(set) = selected_by_layer.get_mut(&layer.id) {
                for group in &layer.properties.groups {
                    if group.members.iter().any(|id| set.contains(id)) {
                        set.extend(group.members.iter().cloned());
                    }
                }
            }
        }
        self.playing = false;
        self.transact(|editor| {
            editor.project.format_version = editor.project.format_version.max(4);
            let touched_layers: Vec<Id> = selected_by_layer.keys().cloned().collect();
            for lid in &touched_layers {
                editor.ensure_key_for(lid, false);
            }
            editor.ensure_key_for(&dest_layer_id, false);
            let frame = editor.frame;
            let mut moved_elements = Vec::new();
            let mut moved_bitmap_fills = Vec::new();
            let layer_count = editor.symbol().timeline.layers.len();
            for l_idx in 0..layer_count {
                let lid = editor.symbol().timeline.layers[l_idx].id.clone();
                let Some(set) = selected_by_layer.get(&lid) else {
                    continue;
                };
                let layer = &mut editor.symbol_mut().timeline.layers[l_idx];
                layer
                    .properties
                    .groups
                    .retain(|group| !group.members.iter().any(|id| set.contains(id)));
                let still_in_other_keys: HashSet<Id> = if lid != dest_layer_id {
                    layer
                        .keyframes
                        .iter()
                        .filter(|k| k.frame != frame)
                        .flat_map(|k| k.elements.iter().map(|e| e.id.clone()))
                        .collect()
                } else {
                    HashSet::new()
                };
                let Some(key) = layer.key_at_mut(frame) else {
                    continue;
                };
                let mut remaining = Vec::with_capacity(key.elements.len());
                for mut el in key.elements.drain(..) {
                    if set.contains(&el.id) {
                        let old_id = el.id.clone();
                        if still_in_other_keys.contains(&old_id) {
                            el.id = new_id("object");
                        }
                        if let Some(bf_pos) = key
                            .bitmap_fills
                            .iter()
                            .position(|bf| bf.element_id == old_id)
                        {
                            let mut bf = key.bitmap_fills.remove(bf_pos);
                            bf.element_id = el.id.clone();
                            moved_bitmap_fills.push(bf);
                        }
                        moved_elements.push(el);
                    } else {
                        remaining.push(el);
                    }
                }
                key.elements = remaining;
                layer.normalize_motion_keys();
            }
            let ordered: Vec<Id> = moved_elements.iter().map(|el| el.id.clone()).collect();
            let new_selection: Vec<SelectionTarget> = ordered
                .iter()
                .map(|elem_id| SelectionTarget {
                    layer: dest_layer_id.clone(),
                    element: elem_id.clone(),
                })
                .collect();
            let dest_layer = &mut editor.symbol_mut().timeline.layers[dest_layer_index];
            let name = format!("Group {}", dest_layer.properties.groups.len() + 1);
            if let Some(dest_key) = dest_layer.key_at_mut(frame) {
                dest_key.elements.extend(moved_elements);
                for bf in moved_bitmap_fills {
                    dest_key.set_bitmap_fill(bf);
                }
            }
            dest_layer.properties.groups.push(AuthoringGroup {
                id: new_id("group"),
                name,
                members: ordered,
            });
            editor.prune_authoring_groups();
            editor.layer = dest_layer_index;
            editor.set_selection(new_selection);
        });
    }

    pub(crate) fn ungroup_selection(&mut self) {
        self.finish_edits();
        let selected: HashSet<_> = self.selection.targets.iter().cloned().collect();
        if selected.is_empty() {
            return;
        }
        if !self.can_ungroup_selection() {
            return;
        }
        self.playing = false;
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

    pub(crate) fn can_combine_objects(&self) -> bool {
        let sampled = self.sampled_targets();
        if sampled.len() < 2
            || !sampled.iter().all(|(target, el)| {
                self.target_editable(target)
                    && matches!(
                        el.kind,
                        ElementKind::Rectangle { .. }
                            | ElementKind::Ellipse { .. }
                            | ElementKind::Path { .. }
                            | ElementKind::PaintedPath { .. }
                    )
            })
        {
            return false;
        }
        let closed_non_merge_stroke = sampled
            .iter()
            .filter(|(_, el)| {
                !is_merge_stroke(el)
                    && element_world_commands(el)
                        .is_some_and(|(cmds, _)| !flatten_closed_rings(&cmds).is_empty())
            })
            .count();
        if closed_non_merge_stroke >= 2 {
            return true;
        }
        sampled.iter().all(|(_, el)| {
            element_world_commands(el)
                .is_some_and(|(cmds, _)| !flatten_closed_rings(&cmds).is_empty())
        })
    }

    pub(crate) fn combine_objects_selection(&mut self, op: BooleanOp, crop_by_top: bool) {
        self.finish_edits();
        if !self.can_combine_objects() {
            return;
        }
        let sampled = self.sampled_targets();
        let target_layer = sampled[0].0.layer.clone();
        if sampled.iter().any(|(t, _)| t.layer != target_layer) {
            self.message("Select shapes on the same layer to combine them.");
            return;
        }
        let Some(layer_idx) = self.target_layer_index(&sampled[0].0) else {
            return;
        };
        let selected_ids: HashSet<Id> = sampled.into_iter().map(|(t, _)| t.element).collect();
        let ordered_elements: Vec<Element> = sample_layer_at_fps(
            &self.symbol().timeline.layers[layer_idx],
            self.frame,
            f64::from(self.project.fps),
        )
        .into_iter()
        .filter(|e| selected_ids.contains(&e.id))
        .collect();
        let Some(combined) = combine_elements_boolean(&ordered_elements, op, crop_by_top) else {
            self.message("Combine objects requires at least two closed vector shapes.");
            return;
        };
        let uses_painted_path = combined.iter().any(|e| {
            matches!(e.kind, ElementKind::PaintedPath { .. })
                || e.transform.skew != 0.0
                || !e.transform.color_transform.is_identity()
        });
        let combined_ids: Vec<Id> = combined.iter().map(|e| e.id.clone()).collect();
        self.vector.selected_anchors = None;
        self.playing = false;
        self.transact_named("Combine objects", |editor| {
            if uses_painted_path {
                editor.project.format_version = editor.project.format_version.max(5);
            }
            editor.ensure_key_for(&target_layer, false);
            let frame = editor.frame;
            let layer = &mut editor.symbol_mut().timeline.layers[layer_idx];
            let elements = &mut layer.key_at_mut(frame).unwrap().elements;
            let insert_pos = elements
                .iter()
                .position(|e| selected_ids.contains(&e.id))
                .unwrap_or(elements.len());
            elements.retain(|e| !selected_ids.contains(&e.id));
            let pos = insert_pos.min(elements.len());
            let new_targets: Vec<SelectionTarget> = combined
                .iter()
                .map(|e| SelectionTarget {
                    layer: target_layer.clone(),
                    element: e.id.clone(),
                })
                .collect();
            elements.splice(pos..pos, combined);
            if let Some(first_id) = combined_ids.first() {
                for group in &mut layer.properties.groups {
                    if let Some(gpos) = group.members.iter().position(|id| id == first_id) {
                        for (offset, extra_id) in combined_ids.iter().skip(1).enumerate() {
                            if !group.members.contains(extra_id) {
                                group.members.insert(gpos + 1 + offset, extra_id.clone());
                            }
                        }
                        break;
                    }
                }
            }
            editor.prune_authoring_groups();
            editor.set_selection(new_targets);
        });
    }

    fn collect_distribute_plan(&self) -> Vec<SourceLayerDistribution> {
        if self.selection.symbol != self.symbol_id() || self.selection.is_empty() {
            return vec![];
        }
        let selected: HashSet<SelectionTarget> = self
            .selection
            .targets
            .iter()
            .flat_map(|target| self.grouped_targets(target))
            .collect();
        let timeline = &self.symbol().timeline;
        let mut plan = Vec::new();
        for layer in &timeline.layers {
            if layer.properties.role == LayerRole::Folder
                || !layer_effectively_visible(timeline, layer)
                || layer_effectively_locked(timeline, layer)
            {
                continue;
            }
            let sampled =
                sample_layer_in_at_fps(timeline, layer, self.frame, f64::from(self.project.fps));
            let mut seen_unselected = false;
            let mut unselected_count = 0usize;
            let mut group_unit_index: HashMap<String, usize> = HashMap::new();
            let mut units: Vec<DistributeUnit> = Vec::new();
            for element in sampled {
                let target = SelectionTarget {
                    layer: layer.id.clone(),
                    element: element.id.clone(),
                };
                if !selected.contains(&target) {
                    seen_unselected = true;
                    unselected_count += 1;
                    continue;
                }
                if let Some(group) = layer
                    .properties
                    .groups
                    .iter()
                    .find(|g| g.members.contains(&element.id))
                {
                    if let Some(&idx) = group_unit_index.get(&group.id) {
                        units[idx].members.push(element);
                    } else {
                        group_unit_index.insert(group.id.clone(), units.len());
                        units.push(DistributeUnit {
                            members: vec![element],
                            group_name: Some(group.name.clone()),
                            before_first_unselected: !seen_unselected,
                        });
                    }
                } else {
                    units.push(DistributeUnit {
                        members: vec![element],
                        group_name: None,
                        before_first_unselected: !seen_unselected,
                    });
                }
            }
            if !units.is_empty() {
                plan.push(SourceLayerDistribution {
                    layer_id: layer.id.clone(),
                    units,
                    unselected_count,
                });
            }
        }
        plan
    }

    pub(crate) fn can_distribute_to_layers(&self) -> bool {
        let plan = self.collect_distribute_plan();
        let total_units: usize = plan.iter().map(|entry| entry.units.len()).sum();
        total_units >= 2 || (total_units == 1 && plan[0].unselected_count > 0)
    }

    /// `Modify ▸ Timeline ▸ Distribute to layers` (`⇧⌘D`): move each selected object/group on the
    /// current frame onto its own new layer named after the instance/symbol/text/group while
    /// preserving visual back-to-front painter order in one Undo step.
    pub fn distribute_to_layers(&mut self) -> bool {
        self.finish_edits();
        let plan = self.collect_distribute_plan();
        let total_units: usize = plan.iter().map(|entry| entry.units.len()).sum();
        if !(total_units >= 2 || (total_units == 1 && plan[0].unselected_count > 0)) {
            return false;
        }
        self.playing = false;
        self.transact_named("Distribute to layers", |s| {
            let frame = s.frame;
            let mut used_names: HashSet<String> = s
                .symbol()
                .timeline
                .layers
                .iter()
                .map(|l| l.name.clone())
                .collect();
            let mut new_selection: Vec<SelectionTarget> = Vec::new();
            let mut last_created_layer_id: Option<Id> = None;

            for entry in plan {
                s.ensure_key_for(&entry.layer_id, false);
                let Some(src_index) = s
                    .symbol()
                    .timeline
                    .layers
                    .iter()
                    .position(|l| l.id == entry.layer_id)
                else {
                    continue;
                };
                let moved_ids: HashSet<Id> = entry
                    .units
                    .iter()
                    .flat_map(|u| u.members.iter().map(|e| e.id.clone()))
                    .collect();
                if let Some(key) = s.symbol_mut().timeline.layers[src_index].key_at_mut(frame) {
                    key.elements.retain(|e| !moved_ids.contains(&e.id));
                }
                s.symbol_mut().timeline.layers[src_index].normalize_motion_keys();

                let still_on_source: HashSet<Id> = s.symbol().timeline.layers[src_index]
                    .keyframes
                    .iter()
                    .flat_map(|k| &k.elements)
                    .map(|e| e.id.clone())
                    .filter(|id| moved_ids.contains(id))
                    .collect();

                let src_props = &s.symbol().timeline.layers[src_index].properties;
                let inherited_parent = src_props.parent.clone();
                let inherited_mask = (src_props.role == LayerRole::Normal)
                    .then(|| src_props.mask.clone())
                    .flatten();
                let inherited_motion_guide = (src_props.role == LayerRole::Normal)
                    .then(|| src_props.motion_guide.clone())
                    .flatten();
                let inherited_end_frame = src_props.end_frame.map(|end| end.max(frame + 1));
                if inherited_end_frame.is_some() {
                    s.project.format_version = s.project.format_version.max(5);
                }

                let mut before_layers: Vec<Layer> = Vec::new();
                let mut after_layers: Vec<Layer> = Vec::new();

                for unit in entry.units {
                    let base_name = distribute_unit_base_name(&s.project, &unit);
                    let layer_name = unique_distributed_layer_name(&base_name, &mut used_names);
                    let new_layer_id = new_id("layer");
                    let mut members = Vec::with_capacity(unit.members.len());
                    for mut el in unit.members {
                        if still_on_source.contains(&el.id) {
                            el.id = new_id("object");
                        }
                        if el.transform.skew != 0.
                            || !el.transform.color_transform.is_identity()
                            || matches!(el.kind, ElementKind::PaintedPath { .. })
                        {
                            s.project.format_version = s.project.format_version.max(5);
                        }
                        new_selection.push(SelectionTarget {
                            layer: new_layer_id.clone(),
                            element: el.id.clone(),
                        });
                        members.push(el);
                    }
                    let groups = if members.len() >= 2 {
                        s.project.format_version = s.project.format_version.max(4);
                        vec![AuthoringGroup {
                            id: new_id("group"),
                            name: unit.group_name.unwrap_or_else(|| "Group 1".to_owned()),
                            members: members.iter().map(|e| e.id.clone()).collect(),
                        }]
                    } else {
                        vec![]
                    };
                    let keyframes = if frame == 0 {
                        vec![Keyframe {
                            frame: 0,
                            elements: members,
                            ..Default::default()
                        }]
                    } else {
                        vec![
                            Keyframe {
                                frame: 0,
                                elements: vec![],
                                ..Default::default()
                            },
                            Keyframe {
                                frame,
                                elements: members,
                                ..Default::default()
                            },
                        ]
                    };
                    let new_layer = Layer {
                        id: new_layer_id.clone(),
                        name: layer_name,
                        visible: true,
                        locked: false,
                        properties: LayerProperties {
                            role: LayerRole::Normal,
                            parent: inherited_parent.clone(),
                            mask: inherited_mask.clone(),
                            motion_guide: inherited_motion_guide.clone(),
                            end_frame: inherited_end_frame,
                            groups,
                            ..Default::default()
                        },
                        keyframes,
                    };
                    last_created_layer_id = Some(new_layer_id);
                    if entry.unselected_count > 0 && unit.before_first_unselected {
                        before_layers.push(new_layer);
                    } else {
                        after_layers.push(new_layer);
                    }
                }

                let before_count = before_layers.len();
                let layers = &mut s.symbol_mut().timeline.layers;
                layers.splice(src_index..src_index, before_layers);
                let after_at = src_index + before_count + 1;
                layers.splice(after_at..after_at, after_layers);
            }

            s.prune_authoring_groups();
            s.set_selection(new_selection);
            if let Some(top_id) = last_created_layer_id
                && let Some(idx) = s
                    .symbol()
                    .timeline
                    .layers
                    .iter()
                    .position(|l| l.id == top_id)
            {
                s.layer = idx;
            }
        });
        true
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
fn translated_unit_context(
    unit: LayoutUnit,
    mapping: StageMapping,
    axis: usize,
    amount: f64,
) -> Vec<(SelectionTarget, Element)> {
    let mut context_delta = [0., 0.];
    context_delta[axis] = amount;
    let origin = mapping.stage_to_edit_point([0., 0.]);
    let endpoint = mapping.stage_to_edit_point(context_delta);
    let local_delta = [endpoint[0] - origin[0], endpoint[1] - origin[1]];
    unit.members
        .into_iter()
        .map(|mut item| {
            item.element.transform.x += local_delta[0];
            item.element.transform.y += local_delta[1];
            (item.target, item.element)
        })
        .collect()
}
fn around(p: Pos2, mut m: [f64; 6]) -> [f64; 6] {
    m[4] += p.x as f64 - m[0] * p.x as f64 - m[2] * p.y as f64;
    m[5] += p.y as f64 - m[1] * p.x as f64 - m[3] * p.y as f64;
    m
}
pub(crate) fn affine_transform(old: Transform, matrix: [f64; 6]) -> Option<Transform> {
    let mut new = Transform::from_matrix(affine_multiply(matrix, transform_matrix(old)))?;
    new.alpha = old.alpha;
    new.color_transform = old.color_transform;
    new.pivot = old.pivot;
    new.primitive = old.primitive;
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
