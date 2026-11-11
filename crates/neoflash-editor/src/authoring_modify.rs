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
            .map(|(target, element)| {
                let fallback = Pos2::new(element.transform.x as f32, element.transform.y as f32);
                let (bounds, context_bounds) = bounds.remove(&element.id).unwrap_or_else(|| {
                    (
                        Rect::from_min_size(fallback, Vec2::ZERO),
                        Rect::from_min_size(mapping.edit_to_stage_pos(fallback), Vec2::ZERO),
                    )
                });
                Placement {
                    target,
                    element,
                    bounds,
                    context_bounds,
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
        if units.len() < 3 {
            return;
        }
        let to_stage = self.authoring_modify.to_stage;
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
        let to_stage = self.authoring_modify.to_stage;
        let size = if to_stage {
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
            if old <= 1e-9 {
                continue;
            }
            let mut scale = [1., 1.];
            scale[axis] = size / old;
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

    pub(crate) fn open_scale_and_rotate_dialog(&mut self) {
        self.finish_edits();
        if self.selection.is_empty() {
            return;
        }
        let current = self
            .authoring_modify
            .scale_and_rotate_dialog
            .unwrap_or((100.0, 0.0));
        self.authoring_modify.scale_and_rotate_dialog = Some(current);
    }

    pub(crate) fn apply_scale_and_rotate(
        &mut self,
        ctx: &egui::Context,
        scale_percent: f64,
        rotate_degrees: f64,
    ) -> bool {
        if !scale_percent.is_finite() || !rotate_degrees.is_finite() {
            return false;
        }
        let s = (scale_percent / 100.0).clamp(0.01, 100.0);
        let (sin, cos) = rotate_degrees.to_radians().sin_cos();
        self.affine_selection(ctx, [s * cos, s * sin, -s * sin, s * cos, 0.0, 0.0]);
        true
    }

    pub(crate) fn scale_and_rotate_modal(&mut self, ctx: &egui::Context) {
        let Some((mut scale_pct, mut rotate_deg)) = self.authoring_modify.scale_and_rotate_dialog
        else {
            return;
        };
        use neoflash_ui::{
            components as c,
            tokens::{TextStyle, space},
            typography::rich,
        };
        let mut accept = false;
        let mut cancel = false;
        let modal = egui::Modal::new(egui::Id::new("nf2-scale-and-rotate"))
            .frame(c::Surface::Dialog.frame(ctx))
            .show(ctx, |ui| {
                ui.set_width(340.0);
                ui.spacing_mut().item_spacing = egui::vec2(space::S2, space::S2);
                cancel |= c::DialogHeader {
                    title: "Scale and Rotate",
                }
                .show(ui);
                let w = ui.available_width();
                ui.add(
                    c::Field::new(&mut scale_pct, "Scale")
                        .prefix("Scale")
                        .unit("%")
                        .range(1.0..=1000.0)
                        .decimals(1)
                        .stepper()
                        .width(w),
                );
                ui.add(
                    c::Field::new(&mut rotate_deg, "Rotate")
                        .prefix("Rotate")
                        .unit("°")
                        .range(-360.0..=360.0)
                        .decimals(1)
                        .stepper()
                        .width(w),
                );
                let p = neoflash_ui::palette(ui.ctx());
                ui.label(
                    rich(
                        "Scales and rotates the selection around its transformation point.",
                        TextStyle::Small,
                    )
                    .color(p.text_tertiary),
                );
                ui.add_space(space::S2);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(c::Button::primary("OK").shortcut("↵")).clicked() {
                        accept = true;
                    }
                    if ui.add(c::Button::new("Cancel").shortcut("Esc")).clicked() {
                        cancel = true;
                    }
                });
            });
        if modal.should_close() {
            cancel = true;
        }
        if !ctx.wants_keyboard_input() && ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            accept = true;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            cancel = true;
        }
        if accept {
            self.authoring_modify.scale_and_rotate_dialog = None;
            self.apply_scale_and_rotate(ctx, scale_pct, rotate_deg);
        } else if cancel {
            self.authoring_modify.scale_and_rotate_dialog = None;
        } else {
            self.authoring_modify.scale_and_rotate_dialog = Some((scale_pct, rotate_deg));
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

    pub(crate) fn can_break_apart_selection(&self) -> bool {
        let sampled = self.sampled_targets();
        if sampled.is_empty()
            || sampled
                .iter()
                .any(|(target, _)| !self.target_editable(target))
        {
            return false;
        }
        if self.can_ungroup_selection() {
            return true;
        }
        sampled.iter().any(|(_, el)| match &el.kind {
            ElementKind::Instance { symbol_id, .. } => self.project.symbol(symbol_id).is_some(),
            ElementKind::Bitmap { .. }
            | ElementKind::Rectangle { .. }
            | ElementKind::Ellipse { .. } => true,
            ElementKind::Text { text, .. } | ElementKind::TextBox { text, .. } => {
                text.chars().any(|ch| !ch.is_whitespace())
            }
            ElementKind::Path { .. } | ElementKind::PaintedPath { .. } => !el.merge,
            ElementKind::Video { .. } => false,
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

    pub(crate) fn break_apart_selection(&mut self) {
        self.finish_edits();
        let selected = self.sampled_targets();
        if selected.is_empty() {
            return;
        }
        let grouped_members: HashSet<SelectionTarget> = self
            .symbol()
            .timeline
            .layers
            .iter()
            .flat_map(|layer| {
                let layer_id = layer.id.clone();
                layer
                    .properties
                    .groups
                    .iter()
                    .filter(|group| !group.members.is_empty())
                    .flat_map(move |group| {
                        let lid = layer_id.clone();
                        group.members.iter().map(move |elem_id| SelectionTarget {
                            layer: lid.clone(),
                            element: elem_id.clone(),
                        })
                    })
            })
            .collect();
        let mut unpack_group_targets = HashSet::new();
        let mut unchanged_targets = HashSet::new();
        let mut did_break_any = false;
        let mut replacements = Vec::new();
        let mut unpacked_groups: Vec<(Id, AuthoringGroup)> = Vec::new();
        let mut unpacked_bitmap_fills: Vec<(Id, AuthoredBitmapFill)> = Vec::new();
        let mut expanded_instance = false;
        let mut uses_skew = false;
        let mut empty_text_skipped = false;
        for (target, instance) in selected {
            if !self.target_editable(&target) {
                self.message("This layer is locked or hidden.");
                return;
            }
            if grouped_members.contains(&target) {
                unpack_group_targets.insert(target.clone());
                unchanged_targets.insert(target.clone());
                replacements.push((target, vec![instance]));
                did_break_any = true;
                continue;
            }
            match &instance.kind {
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
                } => {
                    let (commands, fill_rule) = instance.to_path_commands().unwrap_or_else(|| {
                        (
                            rectangle_path_commands(0.0, 0.0, RectangleOptions::default()),
                            FillRule::NonZero,
                        )
                    });
                    let effective_fill = if matches!(instance.kind, ElementKind::Ellipse { .. })
                        && !instance.oval_options().close_path
                    {
                        [0, 0, 0, 0]
                    } else {
                        *fill
                    };
                    let mut broken = instance.clone();
                    broken.transform.primitive = None;
                    broken.merge = true;
                    broken.kind = ElementKind::Path {
                        commands,
                        fill_rule,
                        fill: effective_fill,
                        stroke: *stroke,
                        stroke_width: *stroke_width,
                    };
                    replacements.push((target, vec![broken]));
                    did_break_any = true;
                }
                ElementKind::Instance { symbol_id, .. } => {
                    let Some(symbol) = self.project.symbol(symbol_id).cloned() else {
                        return;
                    };
                    let child_frame = evaluate_instance_pose(
                        &self.project,
                        self.symbol_id(),
                        self.frame,
                        &target.layer,
                        &target.element,
                    )
                    .ok()
                    .map(|pose| pose.frame)
                    .unwrap_or(0);
                    let parent = instance.transform;
                    let parent_matrix = transform_matrix(parent);
                    let mut children = Vec::new();
                    for layer in &symbol.timeline.layers {
                        if !layer_effectively_visible(&symbol.timeline, layer)
                            || !matches!(layer.properties.role, LayerRole::Normal | LayerRole::Mask)
                        {
                            continue;
                        }
                        let active_key = layer.key_at(child_frame);
                        let mut id_map: HashMap<Id, Id> = HashMap::new();
                        for mut child in sample_layer_in_at_fps(
                            &symbol.timeline,
                            layer,
                            child_frame,
                            f64::from(self.project.fps),
                        ) {
                            let old_id = child.id.clone();
                            let Some(mut transform) =
                                affine_transform(child.transform, parent_matrix)
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
                            if transform.skew != 0.0 {
                                uses_skew = true;
                            }
                            child.transform = transform;
                            child.visible &= instance.visible;
                            if child.blend_mode.is_normal() && !instance.blend_mode.is_normal() {
                                child.blend_mode = instance.blend_mode;
                            }
                            let new_child_id = new_id("object");
                            child.id = new_child_id.clone();
                            id_map.insert(old_id.clone(), new_child_id.clone());
                            if let Some(key) = active_key
                                && let Some(bf) =
                                    key.bitmap_fills.iter().find(|bf| bf.element_id == old_id)
                            {
                                let mut mapped_bf = bf.clone();
                                mapped_bf.element_id = new_child_id;
                                unpacked_bitmap_fills.push((target.layer.clone(), mapped_bf));
                            }
                            children.push(child);
                        }
                        for group in &layer.properties.groups {
                            let mapped_members: Vec<Id> = group
                                .members
                                .iter()
                                .filter_map(|m| id_map.get(m).cloned())
                                .collect();
                            if !mapped_members.is_empty() {
                                unpacked_groups.push((
                                    target.layer.clone(),
                                    AuthoringGroup {
                                        id: new_id("group"),
                                        name: group.name.clone(),
                                        members: mapped_members,
                                    },
                                ));
                            }
                        }
                    }
                    expanded_instance = true;
                    did_break_any = true;
                    replacements.push((target, children));
                }
                ElementKind::Bitmap {
                    resource_id,
                    width,
                    height,
                    smoothing,
                } => {
                    let mut broken = instance.clone();
                    broken.id = new_id("object");
                    broken.name.clear();
                    broken.scripts.clear();
                    broken.accessibility = None;
                    broken.transform.primitive = None;
                    broken.merge = true;
                    broken.kind = ElementKind::Path {
                        commands: rectangle_path_commands(
                            f64::from(*width),
                            f64::from(*height),
                            RectangleOptions::default(),
                        ),
                        fill_rule: FillRule::NonZero,
                        fill: [255, 255, 255, 255],
                        stroke: [0; 4],
                        stroke_width: 0.0,
                    };
                    unpacked_bitmap_fills.push((
                        target.layer.clone(),
                        AuthoredBitmapFill {
                            element_id: broken.id.clone(),
                            resource_id: resource_id.clone(),
                            matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                            repeat: false,
                            smoothing: *smoothing,
                        },
                    ));
                    did_break_any = true;
                    replacements.push((target, vec![broken]));
                }
                ElementKind::TextBox { text, .. } => {
                    let total_chars = text.chars().count();
                    let visible_chars = text.chars().filter(|ch| !ch.is_whitespace()).count();
                    let Some(field) = TextFieldState::from_kind(&instance.kind) else {
                        self.message("Break Apart could not read text box parameters.");
                        return;
                    };
                    if visible_chars == 0 {
                        empty_text_skipped = true;
                        unchanged_targets.insert(target.clone());
                        replacements.push((target, vec![instance]));
                        continue;
                    }
                    if total_chars <= 1 {
                        let commands = match self.fonts.field_outline_commands(&field) {
                            Ok(commands) if !commands.is_empty() => commands,
                            Ok(_) => {
                                self.message("Text has no visible glyph outlines.");
                                return;
                            }
                            Err(err) => {
                                self.message(format!("Break Apart could not outline text: {err}"));
                                return;
                            }
                        };
                        let format =
                            field.format_at_utf16(0, field.text.encode_utf16().count() as u32);
                        let mut broken = instance.clone();
                        broken.scripts.clear();
                        broken.accessibility = None;
                        broken.transform.primitive = None;
                        broken.merge = true;
                        broken.kind = ElementKind::Path {
                            commands,
                            fill_rule: FillRule::NonZero,
                            fill: [
                                ((format.color >> 16) & 0xff) as u8,
                                ((format.color >> 8) & 0xff) as u8,
                                (format.color & 0xff) as u8,
                                field.alpha,
                            ],
                            stroke: [0; 4],
                            stroke_width: 0.0,
                        };
                        did_break_any = true;
                        replacements.push((target, vec![broken]));
                        continue;
                    }
                    let placements = match self.fonts.field_character_placements(&field) {
                        Ok(placements) => placements,
                        Err(err) => {
                            self.message(format!("Break Apart text layout failed: {err}"));
                            return;
                        }
                    };
                    let parent = instance.transform;
                    let parent_matrix = transform_matrix(parent);
                    let mut children = Vec::with_capacity(visible_chars);
                    for (idx, placement) in placements
                        .into_iter()
                        .filter(|placement| !placement.ch.is_whitespace())
                        .enumerate()
                    {
                        let ch = placement.ch;
                        let cbox = placement.bounds;
                        let mut char_format = placement.format;
                        char_format.align = TextAlign::Left;
                        char_format.left_margin = 0.0;
                        char_format.right_margin = 0.0;
                        char_format.indent = 0.0;
                        char_format.block_indent = 0.0;
                        char_format.letter_spacing = 0.0;
                        char_format.bullet = false;
                        char_format.tab_stops.clear();
                        let local = Transform {
                            x: cbox.x - 2.0,
                            y: cbox.y - 2.0,
                            ..Transform::default()
                        };
                        let Some(mut transform) = affine_transform(local, parent_matrix) else {
                            self.message("Break Apart would exceed transform limits.");
                            return;
                        };
                        transform.alpha = parent.alpha;
                        transform.color_transform = parent.color_transform;
                        transform.pivot = None;
                        transform.primitive = None;
                        if transform.skew != 0.0 {
                            uses_skew = true;
                        }
                        let width = (cbox.width + 4.0)
                            .max(char_format.size * 0.4 + 4.0)
                            .clamp(4.0, 1_000_000.0);
                        let height = (cbox.height + 4.0)
                            .max(char_format.size * 1.2 + 4.0)
                            .clamp(4.0, 1_000_000.0);
                        let name = if instance.name.is_empty() {
                            String::new()
                        } else {
                            format!("{}_{}", instance.name, idx + 1)
                        };
                        children.push(Element {
                            id: new_id("object"),
                            name,
                            transform,
                            kind: ElementKind::TextBox {
                                text: ch.to_string(),
                                width,
                                height,
                                format: char_format,
                                word_wrap: false,
                                multiline: false,
                            },
                            scripts: vec![],
                            accessibility: None,
                            visible: instance.visible,
                            cache_as_bitmap: instance.cache_as_bitmap,
                            blend_mode: instance.blend_mode,
                            filters: instance.filters.clone(),
                            ..Element::default()
                        });
                    }
                    did_break_any = true;
                    replacements.push((target, children));
                }
                ElementKind::Text {
                    text,
                    font_family,
                    font_size,
                    color,
                } => {
                    let total_chars = text.chars().count();
                    let visible_chars = text.chars().filter(|ch| !ch.is_whitespace()).count();
                    if visible_chars == 0 {
                        empty_text_skipped = true;
                        unchanged_targets.insert(target.clone());
                        replacements.push((target, vec![instance]));
                        continue;
                    }
                    if total_chars <= 1 {
                        let Some(field) = TextFieldState::from_kind(&instance.kind) else {
                            self.message("Break Apart could not read text parameters.");
                            return;
                        };
                        let commands = match self.fonts.field_outline_commands(&field) {
                            Ok(commands) if !commands.is_empty() => commands,
                            Ok(_) => {
                                self.message("Text has no visible glyph outlines.");
                                return;
                            }
                            Err(err) => {
                                self.message(format!("Break Apart could not outline text: {err}"));
                                return;
                            }
                        };
                        let mut broken = instance.clone();
                        broken.scripts.clear();
                        broken.accessibility = None;
                        broken.transform.primitive = None;
                        broken.merge = true;
                        broken.kind = ElementKind::Path {
                            commands,
                            fill_rule: FillRule::NonZero,
                            fill: *color,
                            stroke: [0; 4],
                            stroke_width: 0.0,
                        };
                        did_break_any = true;
                        replacements.push((target, vec![broken]));
                        continue;
                    }
                    let parent = instance.transform;
                    let parent_matrix = transform_matrix(parent);
                    let line_advance = (*font_size * 1.2).max(1.0);
                    let mut children = Vec::with_capacity(visible_chars);
                    let mut letter_idx = 0usize;
                    for (line_idx, line) in text.split('\n').enumerate() {
                        let line_y = line_idx as f64 * line_advance;
                        for (ch, char_x, _) in
                            self.fonts
                                .char_x_offsets(font_family, false, false, *font_size, line)
                        {
                            if ch.is_whitespace() {
                                continue;
                            }
                            letter_idx += 1;
                            let local = Transform {
                                x: char_x,
                                y: line_y,
                                ..Transform::default()
                            };
                            let Some(mut transform) = affine_transform(local, parent_matrix) else {
                                self.message("Break Apart would exceed transform limits.");
                                return;
                            };
                            transform.alpha = parent.alpha;
                            transform.color_transform = parent.color_transform;
                            transform.pivot = None;
                            transform.primitive = None;
                            if transform.skew != 0.0 {
                                uses_skew = true;
                            }
                            let name = if instance.name.is_empty() {
                                String::new()
                            } else {
                                format!("{}_{letter_idx}", instance.name)
                            };
                            children.push(Element {
                                id: new_id("object"),
                                name,
                                transform,
                                kind: ElementKind::Text {
                                    text: ch.to_string(),
                                    font_family: font_family.clone(),
                                    font_size: *font_size,
                                    color: *color,
                                },
                                scripts: vec![],
                                accessibility: None,
                                visible: instance.visible,
                                cache_as_bitmap: instance.cache_as_bitmap,
                                blend_mode: instance.blend_mode,
                                filters: instance.filters.clone(),
                                ..Element::default()
                            });
                        }
                    }
                    did_break_any = true;
                    replacements.push((target, children));
                }
                ElementKind::Path { .. } | ElementKind::PaintedPath { .. } if !instance.merge => {
                    if let Some((fill_opt, strokes)) = decompose_element_to_merge_parts(&instance) {
                        let mut parts = Vec::new();
                        if let Some(f) = fill_opt {
                            parts.push(f);
                        }
                        parts.extend(strokes);
                        if !parts.is_empty() {
                            did_break_any = true;
                            replacements.push((target, parts));
                            continue;
                        }
                    }
                    let mut broken = instance.clone();
                    broken.merge = true;
                    did_break_any = true;
                    replacements.push((target, vec![broken]));
                }
                _ => {
                    unchanged_targets.insert(target.clone());
                    replacements.push((target, vec![instance]));
                }
            }
        }
        if !did_break_any {
            if empty_text_skipped {
                self.message("Text contains no visible glyphs to break apart.");
            } else {
                self.message("Break Apart currently supports groups, symbol instances, bitmaps, text, and Object Drawing shapes.");
            }
            return;
        }
        self.playing = false;
        self.transact(|editor| {
            if !unpack_group_targets.is_empty() {
                for layer in &mut editor.symbol_mut().timeline.layers {
                    let lid = layer.id.clone();
                    layer.properties.groups.retain(|group| {
                        !group.members.iter().any(|elem_id| {
                            unpack_group_targets.contains(&SelectionTarget {
                                layer: lid.clone(),
                                element: elem_id.clone(),
                            })
                        })
                    });
                }
            }
            let mut selection = Vec::new();
            for (target, children) in replacements {
                if unchanged_targets.contains(&target) {
                    selection.push(target);
                    continue;
                }
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
            for (layer_id, bf) in unpacked_bitmap_fills {
                let frame = editor.frame;
                if let Some(layer) = editor
                    .symbol_mut()
                    .timeline
                    .layers
                    .iter_mut()
                    .find(|l| l.id == layer_id)
                    && let Some(key) = layer.key_at_mut(frame)
                {
                    key.set_bitmap_fill(bf);
                }
            }
            for (layer_id, group) in unpacked_groups {
                if let Some(layer) = editor
                    .symbol_mut()
                    .timeline
                    .layers
                    .iter_mut()
                    .find(|l| l.id == layer_id)
                {
                    layer.properties.groups.push(group);
                }
            }
            editor.prune_authoring_groups();
            if expanded_instance || uses_skew {
                editor.project.format_version = editor.project.format_version.max(5);
            }
            editor.project.upgrade_format_for_features();
            editor.set_selection(selection);
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

    /// Modify (§8).
    pub(crate) fn modify_menu_ui(&mut self, ui: &mut egui::Ui) {
        use crate::commands::CommandId as C;
        use crate::ui_icons::{menu_item_enabled, menu_popup};
        menu_popup(ui, "Modify", "Convert to symbol…", "F8", |ui| {
            if crate::ui_icons::menu_item(ui, "Document…", &self.sc(C::ModifyDocument)).clicked()
            {
                self.open_document_settings();
                ui.close();
            }
            neoflash_ui::components::menu_separator(ui);
            if menu_item_enabled(
                ui,
                self.can_convert_selection(),
                "Convert to symbol…",
                &self.sc(C::ModifyConvertToSymbol),
            )
            .clicked()
            {
                self.convert_to_symbol();
                ui.close();
            }
            let _ = menu_item_enabled(ui, false, "Convert to Bitmap", "");
            if menu_item_enabled(
                ui,
                self.can_break_apart_selection(),
                "Break apart",
                &self.sc(C::ModifyBreakApart),
            )
            .clicked()
            {
                self.break_apart_selection();
                ui.close();
            }
            neoflash_ui::components::menu_separator(ui);
            let trace_sc = self.sc(C::ModifyTraceBitmap);
            menu_popup(ui, "Bitmap", "Trace bitmap…", &trace_sc, |ui| {
                if menu_item_enabled(ui, self.can_trace_bitmap(), "Trace bitmap…", &trace_sc)
                    .clicked()
                {
                    self.open_trace_bitmap(ui.ctx());
                    ui.close();
                }
            });
            Self::unavailable_submenu(ui, "Symbol");
            let add_hint_sc = self.sc(C::ModifyAddShapeHint);
            let remove_hints_sc = self.sc(C::ModifyRemoveAllShapeHints);
            menu_popup(ui, "Shape", "Add shape hint", &add_hint_sc, |ui| {
                if menu_item_enabled(
                    ui,
                    self.can_add_shape_hint(),
                    "Add shape hint",
                    &add_hint_sc,
                )
                .clicked()
                {
                    self.add_shape_hint();
                    ui.close();
                }
                if menu_item_enabled(
                    ui,
                    self.can_remove_shape_hints(),
                    "Remove all hints",
                    &remove_hints_sc,
                )
                .clicked()
                {
                    self.remove_all_shape_hints();
                    ui.close();
                }
            });
            let can_combine = self.can_combine_objects();
            let can_del_envelope = self.can_delete_envelope_selection();
            let del_env_sc = self.sc(C::ModifyDeleteEnvelope);
            menu_popup(
                ui,
                "Combine objects",
                "Delete Envelope",
                &del_env_sc,
                |ui| {
                    if menu_item_enabled(ui, can_del_envelope, "Delete Envelope", &del_env_sc)
                        .clicked()
                    {
                        self.delete_envelope_selection();
                        ui.close();
                    }
                    neoflash_ui::components::menu_separator(ui);
                    if menu_item_enabled(ui, can_combine, "Union", "").clicked() {
                        self.combine_objects_selection(BooleanOp::Union, false);
                        ui.close();
                    }
                    if menu_item_enabled(ui, can_combine, "Intersect", "").clicked() {
                        self.combine_objects_selection(BooleanOp::Intersection, false);
                        ui.close();
                    }
                    if menu_item_enabled(ui, can_combine, "Punch", "").clicked() {
                        self.combine_objects_selection(BooleanOp::Difference, false);
                        ui.close();
                    }
                    if menu_item_enabled(ui, can_combine, "Crop", "").clicked() {
                        self.combine_objects_selection(BooleanOp::Intersection, true);
                        ui.close();
                    }
                },
            );
            neoflash_ui::components::menu_separator(ui);
            menu_popup(
                ui,
                "Timeline",
                "Convert to keyframes (Stepped 2s)",
                "⇧⌘D",
                |ui| {
                    if menu_item_enabled(
                        ui,
                        self.can_distribute_to_layers(),
                        "Distribute to layers",
                        &self.sc(C::ModifyDistributeToLayers),
                    )
                    .clicked()
                    {
                        self.distribute_to_layers();
                        ui.close();
                    }
                    if menu_item_enabled(
                        ui,
                        self.can_distribute_to_keyframes(),
                        "Distribute to keyframes",
                        "",
                    )
                    .clicked()
                    {
                        self.distribute_selection_to_keyframes();
                        ui.close();
                    }
                    if crate::ui_icons::menu_item(
                        ui,
                        "Layer properties…",
                        &self.sc(C::ModifyTimelineLayerProperties),
                    )
                    .clicked()
                    {
                        self.open_layer_properties(ui.ctx(), self.layer);
                        ui.close();
                    }
                    let editable = self.layer_editable();
                    if menu_item_enabled(
                        ui,
                        self.can_reverse_frames(),
                        "Reverse frames",
                        &self.sc(C::EditTimelineReverseFrames),
                    )
                    .clicked()
                    {
                        self.reverse_frames();
                        ui.close();
                    }
                    if menu_item_enabled(
                        ui,
                        self.can_sync_symbols(),
                        "Synchronise symbols",
                        &self.sc(C::EditTimelineSyncSymbols),
                    )
                    .clicked()
                    {
                        self.sync_symbols();
                        ui.close();
                    }
                    if menu_item_enabled(
                        ui,
                        editable && self.frame > 0,
                        "Split clip at playhead",
                        &self.sc(C::ModifyTimelineSplitClip),
                    )
                    .clicked()
                    {
                        self.split_clip_at_playhead();
                        ui.close();
                    }
                    if menu_item_enabled(
                        ui,
                        self.can_link_av_layers(),
                        "Link video & audio",
                        &self.sc(C::ModifyTimelineLinkTracks),
                    )
                    .clicked()
                    {
                        self.link_selected_or_adjacent_av_layers();
                        ui.close();
                    }
                    if menu_item_enabled(
                        ui,
                        self.can_unlink_av_layers(),
                        "Unlink video & audio",
                        &self.sc(C::ModifyTimelineUnlinkTracks),
                    )
                    .clicked()
                    {
                        self.unlink_active_layers();
                        ui.close();
                    }
                    neoflash_ui::components::menu_separator(ui);
                    if menu_item_enabled(
                        ui,
                        editable,
                        "Convert to keyframes",
                        &self.sc(C::ModifyTimelineConvertToKeyframes),
                    )
                    .clicked()
                    {
                        self.convert_to_keyframes(false);
                        ui.close();
                    }
                    if menu_item_enabled(
                        ui,
                        self.can_convert_to_keyframes_stepped(),
                        "Tween2Keys (On 2s)",
                        "",
                    )
                    .clicked()
                    {
                        self.tween_to_keys_on_twos();
                        ui.close();
                    }
                    if menu_item_enabled(
                        ui,
                        self.can_convert_to_keyframes_stepped(),
                        "Tween2Keys (On 3s)",
                        "",
                    )
                    .clicked()
                    {
                        self.tween_to_keys_on_threes();
                        ui.close();
                    }
                    if menu_item_enabled(
                        ui,
                        self.can_convert_to_keyframes_stepped(),
                        "Convert to keyframes (Stepped 2s)",
                        "",
                    )
                    .clicked()
                    {
                        self.convert_to_keyframes_stepped(2, false);
                        ui.close();
                    }
                    if menu_item_enabled(
                        ui,
                        editable,
                        "Clear keyframe",
                        &self.sc(C::ModifyTimelineClearKeyframe),
                    )
                    .clicked()
                    {
                        self.clear_keyframe();
                        ui.close();
                    }
                    if menu_item_enabled(
                        ui,
                        editable,
                        "Convert to blank keyframes",
                        &self.sc(C::ModifyTimelineConvertToBlankKeyframes),
                    )
                    .clicked()
                    {
                        self.convert_to_keyframes(true);
                        ui.close();
                    }
                    neoflash_ui::components::menu_separator(ui);
                    if menu_item_enabled(ui, self.can_copy_active_keyframe_ease(), "Copy ease", "")
                        .clicked()
                    {
                        self.copy_active_keyframe_ease();
                        ui.close();
                    }
                    if menu_item_enabled(
                        ui,
                        self.has_copied_keyframe_ease(),
                        "Apply copied ease",
                        "",
                    )
                    .clicked()
                    {
                        self.apply_copied_ease_to_selection();
                        ui.close();
                    }
                    neoflash_ui::components::menu_separator(ui);
                    if crate::ui_icons::menu_item(ui, "Delete empty layers", "").clicked() {
                        self.delete_empty_layers();
                        ui.close();
                    }
                    if crate::ui_icons::menu_item(ui, "Rename layers from content", "").clicked() {
                        self.rename_layers_from_content();
                        ui.close();
                    }
                    if crate::ui_icons::menu_item(ui, "Add actions & stop() layer", "").clicked() {
                        self.add_actions_and_stop_layer();
                        ui.close();
                    }
                },
            );
            menu_popup(
                ui,
                "Commands & JSFL",
                "Batch bitmap compression & smoothing…",
                "",
                |ui| {
                    let ctx = ui.ctx().clone();
                    if crate::ui_icons::menu_item(ui, "Run JSFL script (.jsfl)…", "").clicked() {
                        self.run_jsfl_script_dialog(None);
                        ui.close();
                    }
                    neoflash_ui::components::menu_separator(ui);
                    if crate::ui_icons::menu_item(ui, "Batch rename library items…", "").clicked()
                    {
                        self.open_batch_rename_library_modal(&ctx);
                        ui.close();
                    }
                    if crate::ui_icons::menu_item(
                        ui,
                        "Batch linkage (Export for ActionScript)…",
                        "",
                    )
                    .clicked()
                    {
                        self.open_batch_linkage_modal(&ctx);
                        ui.close();
                    }
                    if crate::ui_icons::menu_item(ui, "Batch bitmap compression & smoothing…", "")
                        .clicked()
                    {
                        self.open_batch_bitmap_compression_modal(&ctx);
                        ui.close();
                    }
                    if crate::ui_icons::menu_item(ui, "Delete unused library items", "").clicked() {
                        self.find_and_delete_unused_library_items();
                        ui.close();
                    }
                    neoflash_ui::components::menu_separator(ui);
                    if crate::ui_icons::menu_item(ui, "Auto Instance Namer…", "").clicked() {
                        self.open_auto_instance_namer_modal(&ctx);
                        ui.close();
                    }
                    if crate::ui_icons::menu_item(ui, "Randomize Selection…", "").clicked() {
                        self.open_randomize_selection_modal(&ctx);
                        ui.close();
                    }
                    if menu_item_enabled(
                        ui,
                        !self.editable_targets().is_empty(),
                        "Pixel snap (round to whole pixels)",
                        "",
                    )
                    .clicked()
                    {
                        self.pixel_snap_selection();
                        ui.close();
                    }
                },
            );
            self.transform_menu(ui);
            self.arrange_menu(ui);
            self.align_menu(ui);
            neoflash_ui::components::menu_separator(ui);
            if menu_item_enabled(
                ui,
                self.can_group_selection(),
                "Group",
                &self.sc(C::ModifyGroup),
            )
            .clicked()
            {
                self.group_selection();
                ui.close();
            }
            if menu_item_enabled(
                ui,
                self.can_ungroup_selection(),
                "Ungroup",
                &self.sc(C::ModifyUngroup),
            )
            .clicked()
            {
                self.ungroup_selection();
                ui.close();
            }
        });
    }

    /// Transform ▸ (Modify and the stage object menu).
    pub(crate) fn transform_menu(&mut self, ui: &mut egui::Ui) {
        use crate::authoring_tools::FreeTransformMode;
        use crate::commands::CommandId as C;
        use crate::ui_icons::{menu_check_enabled, menu_item_enabled};
        let selected = !self.selection.is_empty();
        let can_deform = self.can_distort_or_envelope_selection();
        let is_transform_tool = self.tool == crate::app::Tool::Transform;
        let active_mode = self.active_free_transform_mode();
        let scale_rot_sc = self.sc(C::ModifyScaleAndRotate);
        let cw_sc = self.sc(C::ModifyRotate90Cw);
        let ccw_sc = self.sc(C::ModifyRotate90Ccw);
        let remove_custom_sc = self.sc(C::ModifyRemoveTransform);
        let remove_sc = if remove_custom_sc.is_empty() {
            "⇧⌘Z".to_owned()
        } else {
            remove_custom_sc
        };
        crate::ui_icons::menu_popup(ui, "Transform", "Scale and Rotate…", &scale_rot_sc, |ui| {
            if menu_check_enabled(
                ui,
                selected,
                "Free Transform",
                &self.sc(C::ModifyFreeTransform),
                is_transform_tool && active_mode == FreeTransformMode::FreeTransform,
            )
            .clicked()
            {
                self.set_free_transform_mode(FreeTransformMode::FreeTransform);
                ui.close();
            }
            if menu_check_enabled(
                ui,
                can_deform,
                "Distort",
                &self.sc(C::ModifyDistort),
                is_transform_tool && active_mode == FreeTransformMode::Distort,
            )
            .clicked()
            {
                self.set_free_transform_mode(FreeTransformMode::Distort);
                ui.close();
            }
            if menu_check_enabled(
                ui,
                can_deform,
                "Envelope",
                &self.sc(C::ModifyEnvelope),
                is_transform_tool && active_mode == FreeTransformMode::Envelope,
            )
            .clicked()
            {
                self.set_free_transform_mode(FreeTransformMode::Envelope);
                ui.close();
            }
            if menu_check_enabled(
                ui,
                selected,
                "Scale",
                &self.sc(C::ModifyScale),
                is_transform_tool && active_mode == FreeTransformMode::Scale,
            )
            .clicked()
            {
                self.set_free_transform_mode(FreeTransformMode::Scale);
                ui.close();
            }
            if menu_check_enabled(
                ui,
                selected,
                "Rotate and Skew",
                &self.sc(C::ModifyRotateAndSkew),
                is_transform_tool && active_mode == FreeTransformMode::RotateAndSkew,
            )
            .clicked()
            {
                self.set_free_transform_mode(FreeTransformMode::RotateAndSkew);
                ui.close();
            }
            if menu_item_enabled(ui, selected, "Scale and Rotate…", &scale_rot_sc).clicked() {
                self.open_scale_and_rotate_dialog();
                ui.close();
            }
            neoflash_ui::components::menu_separator(ui);
            if menu_item_enabled(ui, selected, "Rotate 90° CW", &cw_sc).clicked() {
                self.rotate_90_cw_selection(ui.ctx());
                ui.close();
            }
            if menu_item_enabled(ui, selected, "Rotate 90° CCW", &ccw_sc).clicked() {
                self.rotate_90_ccw_selection(ui.ctx());
                ui.close();
            }
            neoflash_ui::components::menu_separator(ui);
            if menu_item_enabled(
                ui,
                selected,
                "Flip Vertical",
                &self.sc(C::ModifyFlipVertical),
            )
            .clicked()
            {
                self.flip_vertical_selection(ui.ctx());
                ui.close();
            }
            if menu_item_enabled(
                ui,
                selected,
                "Flip Horizontal",
                &self.sc(C::ModifyFlipHorizontal),
            )
            .clicked()
            {
                self.flip_horizontal_selection(ui.ctx());
                ui.close();
            }
            neoflash_ui::components::menu_separator(ui);
            if menu_item_enabled(ui, selected, "Remove Transform", &remove_sc).clicked() {
                self.reset_selection_transform(ui.ctx());
                ui.close();
            }
            neoflash_ui::components::menu_separator(ui);
            if crate::ui_icons::menu_item(ui, "Transform panel", &self.sc(C::WindowTransform))
                .clicked()
            {
                self.show_panel(PanelId::Transform);
                ui.close();
            }
        });
    }

    /// Arrange ▸ (Modify and the stage object menu).
    pub(crate) fn arrange_menu(&mut self, ui: &mut egui::Ui) {
        use crate::commands::CommandId as C;
        crate::ui_icons::menu_popup(ui, "Arrange", "Bring Forward", "⇧⌘↑", |ui| {
            for (label, cmd_id, direction) in [
                ("Bring to front", C::ModifyArrangeFront, Arrange::Front),
                ("Bring forward", C::ModifyArrangeForward, Arrange::Forward),
                ("Send backward", C::ModifyArrangeBackward, Arrange::Backward),
                ("Send to back", C::ModifyArrangeBack, Arrange::Back),
            ] {
                let enabled = self.can_arrange_selection(Some(direction));
                if crate::ui_icons::menu_item_enabled(ui, enabled, label, &self.sc(cmd_id))
                    .clicked()
                {
                    self.arrange_selection(direction);
                    ui.close();
                }
            }
        });
    }

    /// Align ▸ (Modify and the stage object menu): the Align panel's edge commands.
    pub(crate) fn align_menu(&mut self, ui: &mut egui::Ui) {
        use crate::commands::CommandId as C;
        crate::ui_icons::menu_popup(ui, "Align", "Vertical middle", "", |ui| {
            let ctx = ui.ctx().clone();
            let count = self.modify_layout_units(&ctx).len();
            let enabled = count >= 2 || (count > 0 && self.authoring_modify.to_stage);
            for (axis, labels) in [
                ["Left", "Horizontal centre", "Right"],
                ["Top", "Vertical middle", "Bottom"],
            ]
            .into_iter()
            .enumerate()
            {
                for (edge, label) in labels.into_iter().enumerate() {
                    if crate::ui_icons::menu_item_enabled(ui, enabled, label, "").clicked() {
                        self.align_selection(&ctx, axis, edge);
                        ui.close();
                    }
                }
                neoflash_ui::components::menu_separator(ui);
            }
            crate::ui_icons::menu_toggle(ui, &mut self.authoring_modify.to_stage, "To stage", "");
            if crate::ui_icons::menu_item(ui, "Align panel", &self.sc(C::WindowAlign)).clicked() {
                self.show_panel(PanelId::Align);
                ui.close();
            }
        });
    }

    /// Color panel ⋯ menu (§6 "Color — panel menu"): Add swatch · Help · Close · Close group.
    pub(crate) fn color_panel_menu(&mut self, ui: &mut egui::Ui) {
        Self::planned_menu_item(ui, "Color menu ▸ Add swatch", "");
        Self::unavailable_item(ui, "Help", "F1");
        if crate::ui_icons::menu_item(ui, "Close", "⌘W").clicked() {
            self.set_panel_visible(PanelId::Color, false);
            ui.close();
        }
        if crate::ui_icons::menu_item(ui, "Close group", "").clicked() {
            let group = self
                .workspace
                .dock
                .location(PanelId::Color)
                .and_then(|at| {
                    self.workspace
                        .dock
                        .area(at.edge)
                        .groups
                        .iter()
                        .find(|g| g.id == at.group)
                        .map(|g| g.tabs.clone())
                })
                .or_else(|| {
                    self.workspace
                        .floating_window(PanelId::Color)
                        .map(|w| w.panels())
                });
            for panel in group.unwrap_or_else(|| vec![PanelId::Color]) {
                self.set_panel_visible(panel, false);
            }
            ui.close();
        }
    }

    pub(crate) fn align_panel_ui(&mut self, ui: &mut egui::Ui) {
        self.panel_title(ui, PanelId::Align, "Align");
        egui::ScrollArea::vertical()
            .id_salt("align-content")
            .auto_shrink([false, false])
            .themed()
            .show(ui, |ui| self.align_panel_contents(ui));
    }

    fn align_panel_contents(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = space::S2;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::S2;
            caption_label(ui, "Align to");
            Segmented::show(
                ui,
                &mut self.authoring_modify.to_stage,
                &[(false, "Selection"), (true, "Stage")],
                false,
            );
        });
        let count = self.modify_layout_units(ui.ctx()).len();
        ui.add_enabled_ui_semantic(count > 0, |ui| {
            overline(ui, "Align");
            ui.add_enabled_ui_semantic(
                count >= 2 || (count > 0 && self.authoring_modify.to_stage),
                |ui| {
                    alignment_groups(ui, "Align", ALIGN_ICONS, |ui, axis, edge| {
                        self.align_selection(ui.ctx(), axis, edge)
                    });
                },
            )
            .response
            .on_disabled_hover_text("Select two objects, or choose Stage to align one object.");
            overline(ui, "Distribute");
            ui.add_enabled_ui_semantic(count >= 3, |ui| {
                alignment_groups(ui, "Distribute", DISTRIBUTE_ICONS, |ui, axis, edge| {
                    self.distribute_selection(ui.ctx(), axis, edge)
                })
            })
            .response
            .on_disabled_hover_text("Select at least three objects to distribute their positions.");
            overline(ui, "Spacing");
            ui.add_enabled_ui_semantic(count >= 2, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = space::S2;
                    let even = IconGroup {
                        items: &[
                            (
                                i16::SPACE_H,
                                "Space evenly horizontally (keeps outer bounds)",
                            ),
                            (i16::SPACE_V, "Space evenly vertically (keeps outer bounds)"),
                        ],
                        selected: None,
                    }
                    .show(ui);
                    if let Some(axis) = even {
                        self.space_selection(ui.ctx(), axis, None);
                    }
                    let gap = &mut self.authoring_modify.horizontal_gap;
                    let before = *gap;
                    ui.add(
                        DragField::new(gap, "Spacing gap (px)")
                            .prefix("Gap")
                            .suffix(" px")
                            .range(-100_000. ..=100_000.)
                            .max_decimals(2)
                            .width(ui.available_width().min(132.)),
                    );
                    if *gap != before {
                        self.authoring_modify.vertical_gap = *gap;
                    }
                });
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = space::S2;
                    let apply = IconGroup {
                        items: &[
                            (i16::SPACE_H, "Apply gap horizontally"),
                            (i16::SPACE_V, "Apply gap vertically"),
                        ],
                        selected: None,
                    }
                    .show(ui);
                    caption_label(ui, "Apply the gap");
                    if let Some(axis) = apply {
                        let gap = if axis == 0 {
                            self.authoring_modify.horizontal_gap
                        } else {
                            self.authoring_modify.vertical_gap
                        };
                        self.space_selection(ui.ctx(), axis, Some(gap));
                    }
                });
            })
            .response
            .on_disabled_hover_text("Select two or more objects to space them.");
            overline(ui, "Match size");
            ui.add_enabled_ui_semantic(count >= 2, |ui| {
                let clicked = IconGroup {
                    items: &[
                        (i16::MATCH_W, "Match width"),
                        (i16::MATCH_H, "Match height"),
                    ],
                    selected: None,
                }
                .show(ui);
                if let Some(axis) = clicked {
                    self.match_selection_size(ui.ctx(), axis);
                }
            })
            .response
            .on_disabled_hover_text("Select two or more objects to match their size.");
        });
    }

    pub(crate) fn transform_panel_ui(&mut self, ui: &mut egui::Ui) {
        self.panel_title(ui, PanelId::Transform, "Transform");
        egui::ScrollArea::vertical()
            .id_salt("transform-content")
            .auto_shrink([false, false])
            .themed()
            .show(ui, |ui| self.transform_panel_contents(ui));
    }

    fn transform_panel_contents(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = space::S2;
        let placements = self.modify_placements(ui.ctx());
        if placements.is_empty() {
            crate::panels::properties::hint(ui, "Select artwork on the Stage.");
            return;
        }
        let scope = self.modify_scope();
        self.property_edit_scope(scope);
        ui.push_id(scope.with("transform"), |ui| {
            let bounds = combined_bounds(&placements);
            let mut fields = [
                bounds.min.x as f64,
                bounds.min.y as f64,
                bounds.width() as f64,
                bounds.height() as f64,
            ];
            let original = fields;
            overline(ui, "Position and size");
            let [ref mut x, ref mut y, ref mut width, ref mut height] = fields;
            let [rx, ry] = field_pair(ui, ["X", "Y"], [x, y], -1_000_000. ..=1_000_000., "", None);
            let linked = &mut self.authoring_modify.linked_scale;
            let [rw, rh] = field_pair(
                ui,
                ["W", "H"],
                [width, height],
                0.01..=1_000_000.,
                "",
                Some(linked),
            );
            for (index, response) in [rx, ry, rw, rh].into_iter().enumerate() {
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
                let item = &placements[0];
                let old = item.element.transform;
                let mut values = [
                    old.scale_x * 100.,
                    old.scale_y * 100.,
                    old.rotation,
                    old.skew,
                ];
                overline(ui, "Scale");
                let [ref mut sx, ref mut sy, ref mut rotation, ref mut skew] = values;
                let linked = &mut self.authoring_modify.linked_scale;
                let [rsx, rsy] = field_pair(
                    ui,
                    ["W", "H"],
                    [sx, sy],
                    -1_000_000. ..=1_000_000.,
                    " %",
                    Some(linked),
                );
                overline(ui, "Rotate and skew");
                let [rr, rk] = field_pair(
                    ui,
                    ["Rotate", "Skew"],
                    [rotation, skew],
                    -360_000. ..=360_000.,
                    "°",
                    None,
                );
                for (index, response) in [rsx, rsy, rr, rk].into_iter().enumerate() {
                    if !response.changed() {
                        continue;
                    }
                    let value = values[index];
                    let mut new = old;
                    match index {
                        0 => {
                            new.scale_x = value / 100.;
                            if self.authoring_modify.linked_scale && old.scale_x.abs() > 1e-9 {
                                new.scale_y *= new.scale_x / old.scale_x;
                            }
                        }
                        1 => {
                            new.scale_y = value / 100.;
                            if self.authoring_modify.linked_scale && old.scale_y.abs() > 1e-9 {
                                new.scale_x *= new.scale_y / old.scale_y;
                            }
                        }
                        2 => new.rotation = value,
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
            } else if placements.len() > 1 {
                let mut values = [100.0_f64, 100.0_f64, 0.0_f64, 0.0_f64];
                overline(ui, "Scale");
                let [ref mut sx, ref mut sy, ref mut rotation, ref mut skew] = values;
                let linked = &mut self.authoring_modify.linked_scale;
                let [rsx, rsy] = field_pair(
                    ui,
                    ["W", "H"],
                    [sx, sy],
                    -1_000_000. ..=1_000_000.,
                    " %",
                    Some(linked),
                );
                overline(ui, "Rotate and skew");
                let [rr, rk] = field_pair(
                    ui,
                    ["Rotate", "Skew"],
                    [rotation, skew],
                    -360_000. ..=360_000.,
                    "°",
                    None,
                );
                let pivot = self
                    .authoring_modify
                    .group_pivot
                    .as_ref()
                    .filter(|(targets, _)| *targets == self.selection.targets)
                    .map(|(_, p)| Pos2::new(p[0] as f32, p[1] as f32))
                    .unwrap_or_else(|| bounds.center());
                for (index, response) in [rsx, rsy, rr, rk].into_iter().enumerate() {
                    if !response.changed() {
                        continue;
                    }
                    let value = values[index];
                    let matrix = match index {
                        0 => {
                            let s = value / 100.0;
                            let sy_f = if self.authoring_modify.linked_scale {
                                s
                            } else {
                                1.0
                            };
                            around(pivot, [s, 0.0, 0.0, sy_f, 0.0, 0.0])
                        }
                        1 => {
                            let s = value / 100.0;
                            let sx_f = if self.authoring_modify.linked_scale {
                                s
                            } else {
                                1.0
                            };
                            around(pivot, [sx_f, 0.0, 0.0, s, 0.0, 0.0])
                        }
                        2 => {
                            let (sin, cos) = value.to_radians().sin_cos();
                            around(pivot, [cos, sin, -sin, cos, 0.0, 0.0])
                        }
                        _ => {
                            let shx = value.to_radians().tan();
                            around(pivot, [1.0, 0.0, shx, 1.0, 0.0, 0.0])
                        }
                    };
                    if let Some(changes) = transformed(&placements, matrix) {
                        self.apply_modified(changes, Some(response.id));
                    } else {
                        self.message("This transformation exceeds transform limits.");
                    }
                }
            }
            neoflash_ui::components::divider(ui, false);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = space::S2;
                let clicked = IconGroup {
                    items: &[
                        (i16::ROTATE_CCW, "Rotate 90° CCW"),
                        (i16::ROTATE_CW, "Rotate 90° CW"),
                        (i16::FLIP_H, "Flip Horizontal"),
                        (i16::FLIP_V, "Flip Vertical"),
                    ],
                    selected: None,
                }
                .show(ui);
                if let Some(index) = clicked {
                    self.affine_selection(ui.ctx(), TRANSFORM_COMMANDS[index].1);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(IconButton::new(i16::RESET_TRANSFORM, "Remove Transform"))
                        .clicked()
                    {
                        self.reset_selection_transform(ui.ctx());
                    }
                });
            });
        });
    }

    #[allow(dead_code)]
    fn transform_commands(&mut self, ui: &mut egui::Ui) {
        for (label, matrix) in TRANSFORM_COMMANDS {
            let response = if egui::containers::menu::is_in_menu(ui) {
                crate::ui_icons::menu_item(ui, label, "")
            } else {
                ui.add(neoflash_ui::components::Button::new(label).width(ui.available_width()))
            };
            if response.clicked() {
                self.affine_selection(ui.ctx(), matrix);
            }
        }
        let reset = if egui::containers::menu::is_in_menu(ui) {
            crate::ui_icons::menu_item(ui, "Remove Transform", "")
        } else {
            ui.add(
                neoflash_ui::components::Button::new("Remove Transform")
                    .width(ui.available_width()),
            )
        };
        if reset.clicked() {
            self.reset_selection_transform(ui.ctx());
        }
    }

    pub(crate) fn color_panel_ui(&mut self, ui: &mut egui::Ui) {
        self.panel_title(ui, PanelId::Color, "Color");
        // Compact state (R-055): the mixers read these and request a toggle / a recent colour.
        let recent: Vec<Color> = self.workspace.recent_colors.clone();
        ui.data_mut(|d| {
            d.insert_temp(
                color_picker_expanded(),
                self.workspace.color_picker_expanded,
            );
            d.insert_temp(color_recent(), recent);
            d.remove::<bool>(color_picker_toggle());
        });
        egui::ScrollArea::vertical()
            .id_salt("color-content")
            .auto_shrink([false, false])
            .themed()
            .show(ui, |ui| self.color_panel_contents(ui));
        if ui.data_mut(|d| d.remove_temp::<bool>(color_picker_toggle())) == Some(true) {
            self.workspace.color_picker_expanded = !self.workspace.color_picker_expanded;
        }
    }

    pub(crate) fn apply_drawing_gradients_to_element(&self, element: &mut Element) -> bool {
        if self.authoring_modify.drawing_fill_gradient.is_none()
            && self.authoring_modify.drawing_stroke_gradient.is_none()
        {
            return false;
        }
        let Some((commands, fill_rule)) = element.to_path_commands() else {
            return false;
        };
        let Some((fill, stroke, stroke_width, _)) = paints(&element.kind) else {
            return false;
        };
        let matrix = default_gradient_matrix(&commands);
        let fill_gradient = self
            .authoring_modify
            .drawing_fill_gradient
            .clone()
            .map(|mut paint| {
                if self.tool != crate::app::Tool::Brush || !self.authoring_tools.brush_lock_fill {
                    paint.matrix = matrix;
                }
                paint
            });
        let stroke_gradient =
            self.authoring_modify
                .drawing_stroke_gradient
                .clone()
                .map(|mut paint| {
                    paint.matrix = matrix;
                    paint
                });
        let effective_fill = if matches!(element.kind, ElementKind::Ellipse { .. })
            && !element.oval_options().close_path
        {
            [0, 0, 0, 0]
        } else {
            fill
        };
        element.transform.primitive = None;
        element.kind = ElementKind::PaintedPath {
            commands,
            fill_rule,
            fill: if effective_fill == [0, 0, 0, 0] && fill_gradient.is_none() {
                effective_fill
            } else {
                fill
            },
            stroke,
            fill_gradient: if effective_fill == [0, 0, 0, 0] {
                None
            } else {
                fill_gradient
            },
            stroke_gradient,
            stroke_width,
        };
        true
    }

    pub(crate) fn target_has_bitmap_fill(&self, target: &SelectionTarget) -> bool {
        self.symbol()
            .timeline
            .layers
            .iter()
            .find(|l| l.id == target.layer)
            .and_then(|l| l.key_at(self.frame))
            .is_some_and(|k| {
                k.bitmap_fills
                    .iter()
                    .any(|bf| bf.element_id == target.element)
            })
    }

    pub(crate) fn contextual_paints(
        &self,
    ) -> (
        Color,
        Option<GradientPaint>,
        Color,
        Option<GradientPaint>,
        f64,
    ) {
        if self.authoring_modify.color_selection {
            let values: Vec<_> = self
                .sampled_targets()
                .into_iter()
                .filter(|(t, _)| self.target_editable(t))
                .filter_map(|(t, e)| {
                    paints_with_gradients_for_element(&e, self.target_has_bitmap_fill(&t))
                })
                .collect();
            if !values.is_empty() {
                let (fill, fill_grad) =
                    if let Some(fill_sample) = values.iter().find(|v| v.has_fill) {
                        (fill_sample.fill, fill_sample.fill_gradient.clone())
                    } else {
                        ([0, 0, 0, 0], None)
                    };
                let (stroke, stroke_grad, width) =
                    if let Some(stroke_sample) = values.iter().find(|v| v.has_stroke) {
                        (
                            stroke_sample.stroke,
                            stroke_sample.stroke_gradient.clone(),
                            stroke_sample.stroke_width,
                        )
                    } else if let Some(stroke_sample) = values.iter().find(|v| v.supports_stroke) {
                        ([0, 0, 0, 0], None, stroke_sample.stroke_width)
                    } else {
                        (
                            self.stroke,
                            self.authoring_modify.drawing_stroke_gradient.clone(),
                            self.stroke_width,
                        )
                    };
                return (fill, fill_grad, stroke, stroke_grad, width);
            }
        }
        (
            self.fill,
            self.authoring_modify.drawing_fill_gradient.clone(),
            self.stroke,
            self.authoring_modify.drawing_stroke_gradient.clone(),
            self.stroke_width,
        )
    }

    pub(crate) fn set_selection_fill_type(&mut self, stroke_well: bool, mode: PaintFillType) {
        if !self.authoring_modify.color_selection {
            self.set_drawing_fill_type(stroke_well, mode);
            return;
        }
        if mode == PaintFillType::BitmapFill {
            if !stroke_well {
                let idx = self.authoring_modify.drawing_bitmap_fill.unwrap_or(0);
                self.authoring_modify.drawing_bitmap_fill = Some(idx);
                self.authoring_modify.drawing_fill_gradient = None;
                let _ = self.apply_bitmap_swatch_tile(idx);
            }
            return;
        }
        if !stroke_well {
            self.authoring_modify.drawing_bitmap_fill = None;
        }
        let targets: Vec<_> = self
            .sampled_targets()
            .into_iter()
            .filter(|(t, _)| self.target_editable(t))
            .collect();
        if targets.is_empty() {
            self.set_drawing_fill_type(stroke_well, mode);
            return;
        }
        let multi = targets.len() > 1;
        let had_bitmap_targets: Vec<SelectionTarget> = if !stroke_well {
            targets
                .iter()
                .filter(|(t, _)| self.target_has_bitmap_fill(t))
                .map(|(t, _)| t.clone())
                .collect()
        } else {
            Vec::new()
        };
        let changes: Vec<_> = targets
            .into_iter()
            .filter_map(|(target, mut element)| {
                let had_bitmap = had_bitmap_targets.contains(&target);
                let eligible = if stroke_well {
                    element_has_stroke(&element) || (!multi && !is_merge_fill(&element))
                } else {
                    element_has_fill(&element, had_bitmap)
                        || (!multi && !element_has_stroke(&element) && !is_merge_stroke(&element))
                };
                if !eligible {
                    return None;
                }
                (set_element_fill_type(&mut element, stroke_well, mode) || had_bitmap)
                    .then_some((target, element))
            })
            .collect();
        if matches!(mode, PaintFillType::None | PaintFillType::Solid)
            && let Some((_, first_el)) = changes.first()
            && let Some(sample) = paints_with_gradients(&first_el.kind)
        {
            if stroke_well {
                self.stroke = sample.stroke;
                self.authoring_modify.drawing_stroke_gradient = None;
            } else {
                self.fill = sample.fill;
                self.authoring_modify.drawing_fill_gradient = None;
            }
        }
        if !had_bitmap_targets.is_empty() {
            let frame = self.frame;
            self.finish_edits();
            self.transact(|editor| {
                for target in &had_bitmap_targets {
                    editor.ensure_key_for(&target.layer, false);
                    if let Some(layer) = editor
                        .symbol_mut()
                        .timeline
                        .layers
                        .iter_mut()
                        .find(|l| l.id == target.layer)
                        && let Some(key) = layer.key_at_mut(frame)
                    {
                        key.bitmap_fills
                            .retain(|bf| bf.element_id != target.element);
                    }
                }
                for (target, element) in changes {
                    editor.ensure_key_for(&target.layer, false);
                    if let Some(layer) = editor
                        .symbol_mut()
                        .timeline
                        .layers
                        .iter_mut()
                        .find(|l| l.id == target.layer)
                        && let Some(key) = layer.key_at_mut(frame)
                        && let Some(slot) = key.elements.iter_mut().find(|e| e.id == target.element)
                    {
                        *slot = element;
                    }
                }
            });
            return;
        }
        self.apply_modified(changes, None);
    }

    pub(crate) fn set_drawing_fill_type(&mut self, stroke_well: bool, mode: PaintFillType) {
        if mode == PaintFillType::BitmapFill {
            if !stroke_well {
                let idx = self.authoring_modify.drawing_bitmap_fill.unwrap_or(0);
                self.authoring_modify.drawing_bitmap_fill = Some(idx);
                self.authoring_modify.drawing_fill_gradient = None;
                let _ = self.apply_bitmap_swatch_tile(idx);
            }
            return;
        }
        if !stroke_well {
            self.authoring_modify.drawing_bitmap_fill = None;
        }
        let (color, grad) = if stroke_well {
            (
                &mut self.stroke,
                &mut self.authoring_modify.drawing_stroke_gradient,
            )
        } else {
            (
                &mut self.fill,
                &mut self.authoring_modify.drawing_fill_gradient,
            )
        };
        match mode {
            PaintFillType::None => {
                *color = [0, 0, 0, 0];
                *grad = None;
            }
            PaintFillType::Solid => {
                if *color == [0, 0, 0, 0] {
                    *color = if stroke_well {
                        [0, 0, 0, 255]
                    } else {
                        [255, 255, 255, 255]
                    };
                } else if grad.is_some() && color[3] == 0 {
                    color[3] = 255;
                }
                *grad = None;
            }
            PaintFillType::LinearGradient | PaintFillType::RadialGradient => {
                let kind = if mode == PaintFillType::LinearGradient {
                    GradientKind::Linear
                } else {
                    GradientKind::Radial
                };
                if color[3] == 0 {
                    *color = if stroke_well {
                        [0, 0, 0, 255]
                    } else {
                        [255, 255, 255, 255]
                    };
                }
                if let Some(existing) = grad {
                    existing.kind = kind;
                } else {
                    let box_cmds =
                        rectangle_path_commands(100.0, 100.0, RectangleOptions::default());
                    *grad = Some(default_gradient_paint(kind, *color, &box_cmds));
                }
            }
            PaintFillType::BitmapFill => {}
        }
    }

    pub(crate) fn set_selection_gradient_stops(
        &mut self,
        stroke_well: bool,
        stops: Vec<GradientStop>,
        control: Option<egui::Id>,
    ) {
        let normalized = normalize_gradient_stops(stops);
        if normalized.is_empty() {
            return;
        }
        {
            let (color, grad) = if stroke_well {
                (
                    &mut self.stroke,
                    &mut self.authoring_modify.drawing_stroke_gradient,
                )
            } else {
                (
                    &mut self.fill,
                    &mut self.authoring_modify.drawing_fill_gradient,
                )
            };
            if let Some(paint) = grad {
                paint.stops = normalized.clone();
                *color = normalized[0].color;
            }
        }
        if !self.authoring_modify.color_selection {
            return;
        }
        let targets: Vec<_> = self
            .sampled_targets()
            .into_iter()
            .filter(|(t, _)| self.target_editable(t))
            .collect();
        let mut first_updated_grad = None;
        let changes: Vec<_> = targets
            .into_iter()
            .filter_map(|(target, mut element)| {
                if let ElementKind::PaintedPath {
                    fill,
                    stroke,
                    fill_gradient,
                    stroke_gradient,
                    ..
                } = &mut element.kind
                {
                    let (base, slot) = if stroke_well {
                        (stroke, stroke_gradient)
                    } else {
                        (fill, fill_gradient)
                    };
                    if let Some(paint) = slot
                        && paint.stops != normalized
                    {
                        paint.stops = normalized.clone();
                        *base = normalized[0].color;
                        if first_updated_grad.is_none() {
                            first_updated_grad = Some(paint.clone());
                        }
                        return Some((target, element));
                    }
                }
                None
            })
            .collect();
        if let Some(updated) = first_updated_grad {
            let (color, grad) = if stroke_well {
                (
                    &mut self.stroke,
                    &mut self.authoring_modify.drawing_stroke_gradient,
                )
            } else {
                (
                    &mut self.fill,
                    &mut self.authoring_modify.drawing_fill_gradient,
                )
            };
            *color = updated.stops[0].color;
            if grad.is_none() {
                *grad = Some(updated);
            }
        }
        self.apply_modified(changes, control);
    }

    pub(crate) fn set_selection_gradient_options(
        &mut self,
        stroke_well: bool,
        spread: neoflash_core::GradientSpread,
        linear_rgb: bool,
    ) {
        {
            let grad = if stroke_well {
                &mut self.authoring_modify.drawing_stroke_gradient
            } else {
                &mut self.authoring_modify.drawing_fill_gradient
            };
            if let Some(paint) = grad {
                paint.spread = spread;
                paint.linear_rgb = linear_rgb;
            }
        }
        if !self.authoring_modify.color_selection {
            return;
        }
        let targets: Vec<_> = self
            .sampled_targets()
            .into_iter()
            .filter(|(t, _)| self.target_editable(t))
            .collect();
        let mut first_updated_grad = None;
        let changes: Vec<_> = targets
            .into_iter()
            .filter_map(|(target, mut element)| {
                if let ElementKind::PaintedPath {
                    fill_gradient,
                    stroke_gradient,
                    ..
                } = &mut element.kind
                {
                    let slot = if stroke_well {
                        stroke_gradient
                    } else {
                        fill_gradient
                    };
                    if let Some(paint) = slot
                        && (paint.spread != spread || paint.linear_rgb != linear_rgb)
                    {
                        paint.spread = spread;
                        paint.linear_rgb = linear_rgb;
                        if first_updated_grad.is_none() {
                            first_updated_grad = Some(paint.clone());
                        }
                        return Some((target, element));
                    }
                }
                None
            })
            .collect();
        if let Some(updated) = first_updated_grad {
            let grad = if stroke_well {
                &mut self.authoring_modify.drawing_stroke_gradient
            } else {
                &mut self.authoring_modify.drawing_fill_gradient
            };
            if grad.is_none() {
                *grad = Some(updated);
            }
        }
        self.apply_modified(changes, None);
    }

    pub(crate) fn swap_active_paints(&mut self) {
        let (ctx_fill, ctx_fill_grad, ctx_stroke, ctx_stroke_grad, _) = self.contextual_paints();
        self.fill = ctx_stroke;
        self.stroke = ctx_fill;
        self.authoring_modify.drawing_fill_gradient = ctx_stroke_grad;
        self.authoring_modify.drawing_stroke_gradient = ctx_fill_grad;
        if !self.authoring_modify.color_selection {
            return;
        }
        let targets: Vec<_> = self
            .sampled_targets()
            .into_iter()
            .filter(|(t, _)| self.target_editable(t))
            .collect();
        let multi = targets.len() > 1;
        let changes: Vec<_> = targets
            .into_iter()
            .filter_map(|(target, mut element)| {
                if multi
                    && (!element_has_fill(&element, self.target_has_bitmap_fill(&target))
                        || !element_has_stroke(&element))
                {
                    return None;
                }
                match &mut element.kind {
                    ElementKind::Rectangle { fill, stroke, .. }
                    | ElementKind::Ellipse { fill, stroke, .. }
                    | ElementKind::Path { fill, stroke, .. } => {
                        std::mem::swap(fill, stroke);
                        Some((target, element))
                    }
                    ElementKind::PaintedPath {
                        fill,
                        stroke,
                        fill_gradient,
                        stroke_gradient,
                        ..
                    } => {
                        std::mem::swap(fill, stroke);
                        std::mem::swap(fill_gradient, stroke_gradient);
                        Some((target, element))
                    }
                    _ => None,
                }
            })
            .collect();
        self.apply_modified(changes, None);
    }

    pub(crate) fn reset_default_paints(&mut self) {
        self.fill = [255, 255, 255, 255];
        self.stroke = [0, 0, 0, 255];
        self.authoring_modify.drawing_fill_gradient = None;
        self.authoring_modify.drawing_stroke_gradient = None;
        self.authoring_modify.drawing_bitmap_fill = None;
        if !self.authoring_modify.color_selection {
            return;
        }
        let targets: Vec<_> = self
            .sampled_targets()
            .into_iter()
            .filter(|(t, _)| self.target_editable(t))
            .collect();
        let changes: Vec<_> = targets
            .into_iter()
            .filter_map(|(target, mut element)| {
                let has_fill = element_has_fill(&element, self.target_has_bitmap_fill(&target));
                let has_stroke = element_has_stroke(&element);
                match &mut element.kind {
                    ElementKind::Rectangle { fill, stroke, .. }
                    | ElementKind::Ellipse { fill, stroke, .. }
                    | ElementKind::Path { fill, stroke, .. } => {
                        let mut changed = false;
                        if has_fill && *fill != [255, 255, 255, 255] {
                            *fill = [255, 255, 255, 255];
                            changed = true;
                        }
                        if has_stroke && *stroke != [0, 0, 0, 255] {
                            *stroke = [0, 0, 0, 255];
                            changed = true;
                        }
                        changed.then_some((target, element))
                    }
                    ElementKind::PaintedPath {
                        fill,
                        stroke,
                        fill_gradient,
                        stroke_gradient,
                        ..
                    } => {
                        let mut changed = false;
                        if has_fill && (*fill != [255, 255, 255, 255] || fill_gradient.is_some()) {
                            *fill = [255, 255, 255, 255];
                            *fill_gradient = None;
                            changed = true;
                        }
                        if has_stroke && (*stroke != [0, 0, 0, 255] || stroke_gradient.is_some()) {
                            *stroke = [0, 0, 0, 255];
                            *stroke_gradient = None;
                            changed = true;
                        }
                        changed.then_some((target, element))
                    }
                    ElementKind::Text { color, .. } => {
                        let changed = *color != [0, 0, 0, 255];
                        *color = [0, 0, 0, 255];
                        changed.then_some((target, element))
                    }
                    ElementKind::TextBox { format, .. } => {
                        let changed = format.color != 0;
                        format.color = 0;
                        changed.then_some((target, element))
                    }
                    _ => None,
                }
            })
            .collect();
        self.apply_modified(changes, None);
    }

    pub(crate) fn bitmap_fill_section_ui(&mut self, ui: &mut egui::Ui) {
        let nf = neoflash_ui::palette(ui.ctx());
        let mut selected_tile = self.authoring_modify.drawing_bitmap_fill.unwrap_or(0);
        let lock_id = egui::Id::new("nf2-color-bitmap-lock-fill");
        let mut lock_fill = ui
            .ctx()
            .data(|d| d.get_temp::<bool>(lock_id))
            .unwrap_or(false);
        let tile = |h: f32, s: f32| -> (egui::Color32, egui::Color32) {
            (hsv(h, s, 0.85, 1.0).into(), hsv(h, s, 0.35, 1.0).into())
        };
        let bitmaps = [
            tile(0.07, 0.7),
            tile(0.64, 0.55),
            tile(0.38, 0.55),
            tile(0.0, 0.0),
            tile(0.96, 0.65),
            tile(0.62, 0.15),
        ];
        for (row_idx, row) in bitmaps.chunks(4).enumerate() {
            ui.horizontal(|ui| {
                for (col_idx, (from, to)) in row.iter().enumerate() {
                    let idx = row_idx * 4 + col_idx;
                    let (rect, resp) =
                        ui.allocate_exact_size(egui::vec2(58.0, 58.0), egui::Sense::click());
                    neoflash_ui::components::swatches::paint_gradient(
                        ui.painter(),
                        rect,
                        neoflash_ui::components::GradientSwatch {
                            from: *from,
                            to: *to,
                            radial: false,
                        },
                    );
                    let selected = selected_tile == idx;
                    let (w, color) = if selected {
                        (2.0_f32, nf.accent_default)
                    } else {
                        (1.0, nf.border_subtle)
                    };
                    ui.painter().rect_stroke(
                        rect,
                        egui::CornerRadius::same(neoflash_ui::tokens::radius::MD as u8),
                        egui::Stroke::new(w, color),
                        egui::StrokeKind::Inside,
                    );
                    if resp
                        .on_hover_text(format!("tile_{}.png", idx + 1))
                        .clicked()
                    {
                        selected_tile = idx;
                        self.authoring_modify.drawing_bitmap_fill = Some(idx);
                        let _ = self.apply_bitmap_swatch_tile(idx);
                    }
                }
            });
        }
        ui.horizontal(|ui| {
            if ui.add(Button::new("Import…").icon(i16::PLUS)).clicked() {
                self.authoring_modify.drawing_bitmap_fill = Some(selected_tile);
                let _ = self.apply_bitmap_swatch_tile(selected_tile);
            }
            if ui
                .add(IconButton::new(i16::LOCK_FILL, "Lock fill").selected(lock_fill))
                .clicked()
            {
                lock_fill = !lock_fill;
                ui.ctx().data_mut(|d| d.insert_temp(lock_id, lock_fill));
            }
            caption_label(ui, "Lock fill");
        });
        let img_count = self
            .project
            .resources
            .iter()
            .filter(|r| r.is_image())
            .count();
        caption_label(
            ui,
            &format!(
                "tile_{}.png · {} bitmap resource(s) · tiles across selection",
                selected_tile + 1,
                img_count
            ),
        );
    }

    pub(crate) fn color_fill_popover_contents(&mut self, ui: &mut egui::Ui) -> bool {
        ui.set_min_width(276.0);
        ui.set_max_width(276.0);
        ui.data_mut(|d| {
            d.remove::<bool>(color_picker_expanded());
            d.remove::<bool>(color_picker_toggle());
        });
        self.color_panel_contents(ui);
        let mut should_close = false;
        ui.horizontal(|ui| {
            if ui
                .add(Button::new("Add to swatches").icon(i16::PLUS))
                .clicked()
            {
                self.add_active_swatch(ui.ctx());
            }
            if ui
                .add(IconButton::new(i16::EYEDROPPER, "Eyedropper"))
                .clicked()
            {
                self.set_tool(crate::app::Tool::Eyedropper);
                should_close = true;
            }
        });
        should_close
    }

    pub(crate) fn show_active_color_popover(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let Some(active) =
            ctx.data(|d| d.get_temp::<ActiveColorPopover>(active_color_popover_id()))
        else {
            return;
        };
        let pass_guard_id = egui::Id::new("nf2-active-color-popover-pass");
        let pass = ctx.cumulative_pass_nr();
        if ctx.data(|d| d.get_temp::<u64>(pass_guard_id)) == Some(pass) {
            return;
        }
        ctx.data_mut(|d| d.insert_temp(pass_guard_id, pass));
        let mut close_now = false;
        let area_resp =
            egui::Area::new(egui::Id::new(("nf2-color-fill-popover", &active.source_id)))
                .order(egui::Order::Foreground)
                .constrain(true)
                .fixed_pos(egui::pos2(
                    active.anchor_rect.left(),
                    active.anchor_rect.bottom() + 4.0,
                ))
                .show(&ctx, |ui| {
                    neoflash_ui::components::popover_frame(&ctx).show(ui, |ui| {
                        close_now = self.color_fill_popover_contents(ui);
                    });
                });
        let popup_rect = area_resp.response.rect;
        let (pressed, pointer_pos, escape) = ctx.input(|i| {
            (
                i.pointer.any_pressed(),
                i.pointer.interact_pos(),
                i.key_pressed(egui::Key::Escape),
            )
        });
        let clicked_outside = pressed
            && pointer_pos.is_some_and(|pos| {
                !popup_rect.contains(pos)
                    && !active.anchor_rect.contains(pos)
                    && ctx
                        .layer_id_at(pos)
                        .is_none_or(|layer| layer.order < egui::Order::Foreground)
            });
        if close_now || clicked_outside || escape {
            close_color_popover(&ctx);
        }
    }

    fn color_panel_contents(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = space::S2;
        let targets = self
            .sampled_targets()
            .into_iter()
            .filter(|(t, _)| self.target_editable(t))
            .collect::<Vec<_>>();
        let values: Vec<_> = targets
            .iter()
            .filter_map(|(t, e)| {
                paints_with_gradients_for_element(e, self.target_has_bitmap_fill(t))
            })
            .collect();
        // Which paint the wells edit: remembered per panel, Fill by default.
        let active_id = egui::Id::new("color-panel-active-stroke");
        let mut stroke_active = ui.data(|d| d.get_temp::<bool>(active_id).unwrap_or(false));
        // Unified contextual colour: with nothing paintable selected the Color panel edits the
        // tool (drawing) colours; when paintable objects are selected, edits apply to both the
        // selection and the active tool paint without switching modes.
        if !self.authoring_modify.color_selection || targets.is_empty() || values.is_empty() {
            let fill_grad = self.authoring_modify.drawing_fill_gradient.clone();
            let stroke_grad = self.authoring_modify.drawing_stroke_gradient.clone();
            let action = color_wells_and_actions(
                ui,
                &mut stroke_active,
                self.fill,
                fill_grad.as_ref(),
                self.stroke,
                stroke_grad.as_ref(),
                true,
            );
            ui.data_mut(|d| d.insert_temp(active_id, stroke_active));
            match action {
                Some(WellAction::Swap) => {
                    self.swap_active_paints();
                    return;
                }
                Some(WellAction::NoColor) => {
                    self.set_drawing_fill_type(stroke_active, PaintFillType::None);
                    return;
                }
                Some(WellAction::DefaultBlackWhite) => {
                    self.authoring_modify.drawing_bitmap_fill = None;
                    self.reset_default_paints();
                    return;
                }
                None => {}
            }
            let active_color = if stroke_active {
                self.stroke
            } else {
                self.fill
            };
            let active_grad = if stroke_active {
                self.authoring_modify.drawing_stroke_gradient.clone()
            } else {
                self.authoring_modify.drawing_fill_gradient.clone()
            };
            let has_bitmap = !stroke_active && self.authoring_modify.drawing_bitmap_fill.is_some();
            let current_mode = classify_fill_type(active_color, active_grad.as_ref(), has_bitmap);
            if let Some(new_mode) = fill_type_select(ui, current_mode, stroke_active) {
                self.set_drawing_fill_type(stroke_active, new_mode);
                return;
            }
            if current_mode == PaintFillType::BitmapFill {
                self.bitmap_fill_section_ui(ui);
            } else if let Some(mut grad) = active_grad {
                if let Some((spread, linear_rgb)) = flow_row(ui, grad.spread, grad.linear_rgb) {
                    grad.spread = spread;
                    grad.linear_rgb = linear_rgb;
                    self.set_selection_gradient_options(stroke_active, spread, linear_rgb);
                }
                let stop_count = grad.stops.len().max(1);
                if self.authoring_modify.selected_stop >= stop_count {
                    self.authoring_modify.selected_stop = stop_count - 1;
                }
                let mut stop_color = grad.stops[self.authoring_modify.selected_stop].color;
                let mixer_key = egui::Id::new((
                    "drawing-grad-hsb",
                    stroke_active,
                    self.authoring_modify.selected_stop,
                ));
                if color_mixer_ui(
                    ui,
                    mixer_key,
                    if stroke_active {
                        "Stroke colour"
                    } else {
                        "Fill colour"
                    },
                    &mut stop_color,
                    &mut self.authoring_modify.color_model_hsb,
                )
                .is_some()
                {
                    grad.stops[self.authoring_modify.selected_stop].color = stop_color;
                    self.set_selection_gradient_stops(stroke_active, grad.stops.clone(), None);
                }
                if gradient_stops_bar_ui(ui, &mut grad, &mut self.authoring_modify.selected_stop)
                    .is_some()
                {
                    self.set_selection_gradient_stops(stroke_active, grad.stops, None);
                }
            } else {
                let target_color = if stroke_active {
                    &mut self.stroke
                } else {
                    &mut self.fill
                };
                let mixer_key = egui::Id::new(("drawing-solid-hsb", stroke_active));
                let _ = color_mixer_ui(
                    ui,
                    mixer_key,
                    if stroke_active {
                        "Stroke colour"
                    } else {
                        "Fill colour"
                    },
                    target_color,
                    &mut self.authoring_modify.color_model_hsb,
                );
            }
            let _ = stroke_section_ui(ui, &mut self.stroke_width, false);
            return;
        }

        let (fill, fill_grad, stroke, stroke_grad, width) = self.contextual_paints();
        let supports_stroke = values.iter().any(|value| value.supports_stroke);
        let mixed_fill = values
            .iter()
            .filter(|v| v.has_fill)
            .any(|v| v.fill != fill || v.fill_gradient != fill_grad);
        let mixed_stroke = values
            .iter()
            .filter(|v| v.has_stroke)
            .any(|v| v.stroke != stroke || v.stroke_gradient != stroke_grad);
        let mixed_width = values
            .iter()
            .filter(|v| v.has_stroke)
            .any(|v| v.stroke_width != width);
        if !supports_stroke {
            stroke_active = false;
        }
        let scope = self.modify_scope();
        self.property_edit_scope(scope);
        ui.push_id(scope.with("color"), |ui| {
            let action = color_wells_and_actions(
                ui,
                &mut stroke_active,
                fill,
                fill_grad.as_ref(),
                stroke,
                stroke_grad.as_ref(),
                supports_stroke,
            );
            ui.data_mut(|d| d.insert_temp(active_id, stroke_active));
            match action {
                Some(WellAction::Swap) => {
                    self.swap_active_paints();
                    return;
                }
                Some(WellAction::NoColor) => {
                    self.set_selection_fill_type(stroke_active, PaintFillType::None);
                    return;
                }
                Some(WellAction::DefaultBlackWhite) => {
                    self.authoring_modify.drawing_bitmap_fill = None;
                    self.reset_default_paints();
                    return;
                }
                None => {}
            }

            let active_color = if stroke_active { stroke } else { fill };
            let active_grad = if stroke_active {
                stroke_grad.clone()
            } else {
                fill_grad.clone()
            };
            let has_bitmap = !stroke_active
                && (self.authoring_modify.drawing_bitmap_fill.is_some()
                    || targets.iter().any(|(t, _)| self.target_has_bitmap_fill(t)));
            let current_mode = classify_fill_type(active_color, active_grad.as_ref(), has_bitmap);
            if let Some(new_mode) = fill_type_select(ui, current_mode, stroke_active) {
                self.set_drawing_fill_type(stroke_active, new_mode);
                self.set_selection_fill_type(stroke_active, new_mode);
                return;
            }

            if current_mode == PaintFillType::BitmapFill {
                self.bitmap_fill_section_ui(ui);
            } else if let Some(mut grad) = active_grad {
                if let Some((spread, linear_rgb)) = flow_row(ui, grad.spread, grad.linear_rgb) {
                    grad.spread = spread;
                    grad.linear_rgb = linear_rgb;
                    self.set_selection_gradient_options(stroke_active, spread, linear_rgb);
                }
                let stop_count = grad.stops.len().max(1);
                if self.authoring_modify.selected_stop >= stop_count {
                    self.authoring_modify.selected_stop = stop_count - 1;
                }
                let mut stop_color = grad.stops[self.authoring_modify.selected_stop].color;
                let mixer_key = scope.with((
                    "sel-grad-hsb",
                    stroke_active,
                    self.authoring_modify.selected_stop,
                ));
                if let Some(control) = color_mixer_ui(
                    ui,
                    mixer_key,
                    if stroke_active {
                        "Stroke colour"
                    } else {
                        "Fill colour"
                    },
                    &mut stop_color,
                    &mut self.authoring_modify.color_model_hsb,
                ) {
                    grad.stops[self.authoring_modify.selected_stop].color = stop_color;
                    self.set_selection_gradient_stops(
                        stroke_active,
                        grad.stops.clone(),
                        Some(control),
                    );
                }
                if let Some(control) =
                    gradient_stops_bar_ui(ui, &mut grad, &mut self.authoring_modify.selected_stop)
                {
                    self.set_selection_gradient_stops(stroke_active, grad.stops, Some(control));
                }
            } else {
                let (mut new_fill, mut new_stroke) = (fill, stroke);
                let control = if !stroke_active {
                    color_mixer_ui(
                        ui,
                        scope.with("sel-fill-hsb"),
                        "Fill colour",
                        &mut new_fill,
                        &mut self.authoring_modify.color_model_hsb,
                    )
                } else {
                    color_mixer_ui(
                        ui,
                        scope.with("sel-stroke-hsb"),
                        "Stroke colour",
                        &mut new_stroke,
                        &mut self.authoring_modify.color_model_hsb,
                    )
                };
                if let Some(control) = control {
                    if stroke_active {
                        self.stroke = new_stroke;
                        update_gradient_for_color_edit(
                            &mut self.authoring_modify.drawing_stroke_gradient,
                            stroke,
                            new_stroke,
                        );
                    } else {
                        self.fill = new_fill;
                        update_gradient_for_color_edit(
                            &mut self.authoring_modify.drawing_fill_gradient,
                            fill,
                            new_fill,
                        );
                    }
                    let multi = targets.len() > 1;
                    let changes = targets
                        .iter()
                        .cloned()
                        .filter_map(|(target, mut element)| {
                            let has_bm = self.target_has_bitmap_fill(&target);
                            let eligible = if stroke_active {
                                element_has_stroke(&element) || (!multi && !is_merge_fill(&element))
                            } else {
                                element_has_fill(&element, has_bm)
                                    || (!multi
                                        && !element_has_stroke(&element)
                                        && !is_merge_stroke(&element))
                            };
                            if !eligible {
                                return None;
                            }
                            let previous = paints(&element.kind)?;
                            let old_fill = if !stroke_active && (mixed_fill || previous.0 != fill) {
                                [
                                    new_fill[0].wrapping_add(1),
                                    new_fill[1],
                                    new_fill[2],
                                    new_fill[3].wrapping_add(1),
                                ]
                            } else {
                                fill
                            };
                            let old_stroke =
                                if stroke_active && (mixed_stroke || previous.1 != stroke) {
                                    [
                                        new_stroke[0].wrapping_add(1),
                                        new_stroke[1],
                                        new_stroke[2],
                                        new_stroke[3].wrapping_add(1),
                                    ]
                                } else {
                                    stroke
                                };
                            let changed = edit_paints(
                                &mut element.kind,
                                (
                                    if !stroke_active { old_fill } else { previous.0 },
                                    if stroke_active {
                                        old_stroke
                                    } else {
                                        previous.1
                                    },
                                    previous.2,
                                ),
                                (
                                    if !stroke_active { new_fill } else { previous.0 },
                                    if stroke_active {
                                        new_stroke
                                    } else {
                                        previous.1
                                    },
                                    previous.2,
                                ),
                            );
                            changed.then_some((target, element))
                        })
                        .collect();
                    self.apply_modified(changes, Some(control));
                }
            }

            if supports_stroke {
                let mut new_width = width;
                if let Some(control) = stroke_section_ui(ui, &mut new_width, true) {
                    self.stroke_width = new_width;
                    let multi = targets.len() > 1;
                    let changes = targets
                        .into_iter()
                        .filter_map(|(target, mut element)| {
                            if (multi || is_merge_fill(&element)) && !element_has_stroke(&element) {
                                return None;
                            }
                            let previous = paints(&element.kind)?;
                            let changed = edit_paints(
                                &mut element.kind,
                                (
                                    previous.0,
                                    previous.1,
                                    if mixed_width || previous.2 != width {
                                        new_width - 1.0
                                    } else {
                                        width
                                    },
                                ),
                                (previous.0, previous.1, new_width),
                            );
                            changed.then_some((target, element))
                        })
                        .collect();
                    self.apply_modified(changes, Some(control));
                }
            }
        });
    }
}

pub(crate) fn ensure_solid_non_none(color: Color) -> Color {
    if color == [0, 0, 0, 0] {
        [1, 0, 0, 0]
    } else {
        color
    }
}

pub(crate) fn element_has_fill(element: &Element, has_bitmap_fill: bool) -> bool {
    if is_merge_stroke(element) {
        return false;
    }
    match &element.kind {
        ElementKind::Text { .. } | ElementKind::TextBox { .. } => true,
        ElementKind::Rectangle { fill, .. } => *fill != [0, 0, 0, 0] || has_bitmap_fill,
        ElementKind::Ellipse { fill, .. } => {
            element.oval_options().close_path && (*fill != [0, 0, 0, 0] || has_bitmap_fill)
        }
        ElementKind::Path { commands, fill, .. } => {
            !flatten_closed_rings(commands).is_empty() && (*fill != [0, 0, 0, 0] || has_bitmap_fill)
        }
        ElementKind::PaintedPath {
            commands,
            fill,
            fill_gradient,
            ..
        } => {
            !flatten_closed_rings(commands).is_empty()
                && (fill_gradient.is_some() || *fill != [0, 0, 0, 0] || has_bitmap_fill)
        }
        _ => false,
    }
}

pub(crate) fn element_has_stroke(element: &Element) -> bool {
    if is_merge_fill(element) {
        return false;
    }
    match &element.kind {
        ElementKind::Rectangle {
            stroke,
            stroke_width,
            ..
        }
        | ElementKind::Ellipse {
            stroke,
            stroke_width,
            ..
        }
        | ElementKind::Path {
            stroke,
            stroke_width,
            ..
        } => *stroke_width > 0.0 && *stroke != [0, 0, 0, 0],
        ElementKind::PaintedPath {
            stroke,
            stroke_gradient,
            stroke_width,
            ..
        } => *stroke_width > 0.0 && (*stroke != [0, 0, 0, 0] || stroke_gradient.is_some()),
        _ => false,
    }
}

#[derive(Clone)]
struct PaintSample {
    fill: Color,
    stroke: Color,
    fill_gradient: Option<GradientPaint>,
    stroke_gradient: Option<GradientPaint>,
    stroke_width: f64,
    supports_stroke: bool,
    has_fill: bool,
    has_stroke: bool,
}

fn paints_with_gradients_for_element(
    element: &Element,
    has_bitmap_fill: bool,
) -> Option<PaintSample> {
    let mut sample = paints_with_gradients(&element.kind)?;
    sample.has_fill = element_has_fill(element, has_bitmap_fill);
    sample.has_stroke = element_has_stroke(element);
    Some(sample)
}

fn paints_with_gradients(kind: &ElementKind) -> Option<PaintSample> {
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
        } => Some(PaintSample {
            fill: *fill,
            stroke: *stroke,
            fill_gradient: None,
            stroke_gradient: None,
            stroke_width: *stroke_width,
            supports_stroke: true,
            has_fill: *fill != [0, 0, 0, 0],
            has_stroke: *stroke_width > 0.0 && *stroke != [0, 0, 0, 0],
        }),
        ElementKind::PaintedPath {
            fill,
            stroke,
            fill_gradient,
            stroke_gradient,
            stroke_width,
            ..
        } => Some(PaintSample {
            fill: *fill,
            stroke: *stroke,
            fill_gradient: fill_gradient.clone(),
            stroke_gradient: stroke_gradient.clone(),
            stroke_width: *stroke_width,
            supports_stroke: true,
            has_fill: fill_gradient.is_some() || *fill != [0, 0, 0, 0],
            has_stroke: *stroke_width > 0.0
                && (*stroke != [0, 0, 0, 0] || stroke_gradient.is_some()),
        }),
        ElementKind::Text { color, .. } => Some(PaintSample {
            fill: *color,
            stroke: [0; 4],
            fill_gradient: None,
            stroke_gradient: None,
            stroke_width: 0.,
            supports_stroke: false,
            has_fill: true,
            has_stroke: false,
        }),
        ElementKind::TextBox { format, .. } => {
            let [_, r, g, b] = format.color.to_be_bytes();
            Some(PaintSample {
                fill: [r, g, b, 255],
                stroke: [0; 4],
                fill_gradient: None,
                stroke_gradient: None,
                stroke_width: 0.,
                supports_stroke: false,
                has_fill: true,
                has_stroke: false,
            })
        }
        _ => None,
    }
}

fn classify_fill_type(
    color: Color,
    gradient: Option<&GradientPaint>,
    has_bitmap_fill: bool,
) -> PaintFillType {
    if has_bitmap_fill {
        return PaintFillType::BitmapFill;
    }
    match gradient.map(|g| g.kind) {
        Some(GradientKind::Linear) => PaintFillType::LinearGradient,
        Some(GradientKind::Radial) => PaintFillType::RadialGradient,
        None if color == [0, 0, 0, 0] => PaintFillType::None,
        None => PaintFillType::Solid,
    }
}

pub(crate) fn default_gradient_matrix(commands: &[PathCommand]) -> [f64; 6] {
    let (w, h, cx, cy) = match path_bounds(commands).ok().flatten() {
        Some(b) => (
            b.width().max(1.0),
            b.height().max(1.0),
            (b.x_min + b.x_max) * 0.5,
            (b.y_min + b.y_max) * 0.5,
        ),
        None => (100.0, 100.0, 50.0, 50.0),
    };
    [
        (w / 1638.4).clamp(1e-4, 1e4),
        0.0,
        0.0,
        (h / 1638.4).clamp(1e-4, 1e4),
        cx.clamp(-1e9, 1e9),
        cy.clamp(-1e9, 1e9),
    ]
}

fn default_gradient_paint(
    kind: GradientKind,
    base_color: Color,
    commands: &[PathCommand],
) -> GradientPaint {
    let start = if base_color[3] == 0 {
        [255, 255, 255, 255]
    } else {
        base_color
    };
    let end = if u16::from(start[0]) + u16::from(start[1]) + u16::from(start[2]) < 128 {
        [255, 255, 255, start[3]]
    } else {
        [0, 0, 0, start[3]]
    };
    GradientPaint {
        kind,
        matrix: default_gradient_matrix(commands),
        stops: vec![
            GradientStop {
                ratio: 0,
                color: start,
            },
            GradientStop {
                ratio: 255,
                color: end,
            },
        ],
        focal_point_ratio: 0.0,
        ..Default::default()
    }
}

fn normalize_gradient_stops(mut stops: Vec<GradientStop>) -> Vec<GradientStop> {
    stops.sort_by_key(|s| s.ratio);
    stops.truncate(15);
    stops
}

fn set_element_fill_type(element: &mut Element, stroke_well: bool, mode: PaintFillType) -> bool {
    match mode {
        PaintFillType::None => match &mut element.kind {
            ElementKind::Rectangle { fill, stroke, .. }
            | ElementKind::Ellipse { fill, stroke, .. }
            | ElementKind::Path { fill, stroke, .. } => {
                let target = if stroke_well { stroke } else { fill };
                if *target == [0, 0, 0, 0] {
                    return false;
                }
                *target = [0, 0, 0, 0];
                true
            }
            ElementKind::PaintedPath {
                fill,
                stroke,
                fill_gradient,
                stroke_gradient,
                ..
            } => {
                let (target, slot) = if stroke_well {
                    (stroke, stroke_gradient)
                } else {
                    (fill, fill_gradient)
                };
                if *target == [0, 0, 0, 0] && slot.is_none() {
                    return false;
                }
                *target = [0, 0, 0, 0];
                *slot = None;
                true
            }
            ElementKind::Text { color, .. } if !stroke_well => {
                if *color == [0, 0, 0, 0] {
                    return false;
                }
                *color = [0, 0, 0, 0];
                true
            }
            _ => false,
        },
        PaintFillType::Solid => match &mut element.kind {
            ElementKind::Rectangle { fill, stroke, .. }
            | ElementKind::Ellipse { fill, stroke, .. }
            | ElementKind::Path { fill, stroke, .. } => {
                let target = if stroke_well { stroke } else { fill };
                if *target != [0, 0, 0, 0] {
                    return false;
                }
                *target = if stroke_well {
                    [0, 0, 0, 255]
                } else {
                    [255, 255, 255, 255]
                };
                true
            }
            ElementKind::PaintedPath {
                fill,
                stroke,
                fill_gradient,
                stroke_gradient,
                ..
            } => {
                let (target, slot) = if stroke_well {
                    (stroke, stroke_gradient)
                } else {
                    (fill, fill_gradient)
                };
                if *target != [0, 0, 0, 0] && slot.is_none() {
                    return false;
                }
                if let Some(prev) = slot.take()
                    && let Some(first) = prev.stops.first()
                {
                    *target = ensure_solid_non_none(first.color);
                } else if *target == [0, 0, 0, 0] {
                    *target = if stroke_well {
                        [0, 0, 0, 255]
                    } else {
                        [255, 255, 255, 255]
                    };
                }
                true
            }
            ElementKind::Text { color, .. } if !stroke_well => {
                if *color != [0, 0, 0, 0] {
                    return false;
                }
                *color = [0, 0, 0, 255];
                true
            }
            _ => false,
        },
        PaintFillType::LinearGradient | PaintFillType::RadialGradient => {
            let kind = if mode == PaintFillType::LinearGradient {
                GradientKind::Linear
            } else {
                GradientKind::Radial
            };
            if let ElementKind::PaintedPath {
                commands,
                fill,
                stroke,
                fill_gradient,
                stroke_gradient,
                ..
            } = &mut element.kind
            {
                let (base, slot) = if stroke_well {
                    (stroke, stroke_gradient)
                } else {
                    (fill, fill_gradient)
                };
                if let Some(existing) = slot {
                    if existing.kind == kind {
                        return false;
                    }
                    existing.kind = kind;
                } else {
                    if base[3] == 0 {
                        *base = if stroke_well {
                            [0, 0, 0, 255]
                        } else {
                            [255, 255, 255, 255]
                        };
                    }
                    *slot = Some(default_gradient_paint(kind, *base, commands));
                }
                return true;
            }
            let Some((commands, fill_rule)) = element.to_path_commands() else {
                return false;
            };
            let (mut fill, mut stroke, stroke_width) = match &element.kind {
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
                } => (*fill, *stroke, *stroke_width),
                _ => return false,
            };
            if matches!(element.kind, ElementKind::Ellipse { .. })
                && !element.oval_options().close_path
            {
                fill = [0, 0, 0, 0];
            }
            let base = if stroke_well { &mut stroke } else { &mut fill };
            if base[3] == 0 {
                *base = if stroke_well {
                    [0, 0, 0, 255]
                } else {
                    [255, 255, 255, 255]
                };
            }
            let paint = default_gradient_paint(kind, *base, &commands);
            element.transform.primitive = None;
            element.kind = ElementKind::PaintedPath {
                commands,
                fill_rule,
                fill,
                stroke,
                fill_gradient: (!stroke_well).then_some(paint.clone()),
                stroke_gradient: stroke_well.then_some(paint),
                stroke_width,
            };
            true
        }
        PaintFillType::BitmapFill => false,
    }
}

#[derive(Clone, Copy)]
enum WellAction {
    Swap,
    NoColor,
    DefaultBlackWhite,
}

/// Fill and Stroke `NF2/Color Well`s (Fill first on the left, Stroke second on the right)
/// plus Swap / No colour / Black and white quick actions.
fn color_wells_and_actions(
    ui: &mut egui::Ui,
    stroke_active: &mut bool,
    fill: Color,
    fill_grad: Option<&GradientPaint>,
    stroke: Color,
    stroke_grad: Option<&GradientPaint>,
    stroke_enabled: bool,
) -> Option<WellAction> {
    let shown = |c: Color, g: Option<&GradientPaint>| {
        if let Some(paint) = g
            && let Some(stop) = paint.stops.first()
        {
            return Some(egui::Color32::from_rgba_unmultiplied(
                stop.color[0],
                stop.color[1],
                stop.color[2],
                stop.color[3],
            ));
        }
        (c != [0, 0, 0, 0]).then(|| egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]))
    };
    let overpaint_well_gradient =
        |ui: &egui::Ui, well_rect: egui::Rect, grad: Option<&GradientPaint>| {
            if let Some(g) = grad {
                let sw = egui::Rect::from_center_size(
                    egui::pos2(well_rect.left() + 38.0, well_rect.center().y),
                    egui::vec2(18.0, 18.0),
                );
                crate::paint_controls::paint_gradient_swatch(
                    ui.painter(),
                    sw,
                    g,
                    neoflash_ui::tokens::radius::SM,
                );
            }
        };
    let mut action = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let fill_well = ui.add(ColorWell {
            target: ColorWellTarget::Fill,
            color: shown(fill, fill_grad),
            active: !*stroke_active,
        });
        overpaint_well_gradient(ui, fill_well.rect, fill_grad);
        if fill_well.clicked() {
            *stroke_active = false;
        }
        let stroke_well = ui.add_enabled(
            stroke_enabled,
            ColorWell {
                target: ColorWellTarget::Stroke,
                color: shown(stroke, stroke_grad),
                active: *stroke_active,
            },
        );
        overpaint_well_gradient(ui, stroke_well.rect, stroke_grad);
        if stroke_well.clicked() {
            *stroke_active = true;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(
                    stroke_enabled,
                    IconButton::new(i20::COLOR_SWAP, "Swap colours").side(28.0),
                )
                .clicked()
            {
                action = Some(WellAction::Swap);
            }
            if ui
                .add(IconButton::new(i20::COLOR_NONE, "No colour").side(28.0))
                .clicked()
            {
                action = Some(WellAction::NoColor);
            }
            if ui
                .add(IconButton::new(i20::COLOR_DEFAULT, "Black and white").side(28.0))
                .clicked()
            {
                action = Some(WellAction::DefaultBlackWhite);
            }
        });
    });
    action
}

/// Fill type `NF2/Select` (§6 "Color — type menu"): edits None, Solid colour, Linear gradient,
/// Radial gradient and Bitmap fill directly.
fn fill_type_select(
    ui: &mut egui::Ui,
    current: PaintFillType,
    stroke_active: bool,
) -> Option<PaintFillType> {
    let width = ui.available_width();
    let mut chosen = None;
    Select::new("color-fill-type", current.label())
        .width(width)
        .show(ui, |ui| {
            for mode in [
                PaintFillType::None,
                PaintFillType::Solid,
                PaintFillType::LinearGradient,
                PaintFillType::RadialGradient,
                PaintFillType::BitmapFill,
            ] {
                if mode == PaintFillType::BitmapFill && stroke_active {
                    continue;
                }
                if ui
                    .add(MenuItem::new(mode.label()).checked(current == mode))
                    .clicked()
                {
                    chosen = Some(mode);
                    ui.close();
                }
            }
        });
    chosen
}

fn spread_label(spread: neoflash_core::GradientSpread) -> &'static str {
    match spread {
        neoflash_core::GradientSpread::Extend => "Extend",
        neoflash_core::GradientSpread::Reflect => "Reflect",
        neoflash_core::GradientSpread::Repeat => "Repeat",
    }
}

/// Paints the classic Flash gradient overflow diagram (`[dark -> light | dashed boundary | extend/reflect/repeat]`).
fn paint_overflow_preview(
    painter: &egui::Painter,
    rect: egui::Rect,
    spread: neoflash_core::GradientSpread,
) {
    let nf = neoflash_ui::palette(painter.ctx());
    let inner = rect.shrink(0.5);
    if inner.width() <= 2.0 || inner.height() <= 2.0 {
        return;
    }
    let dark = egui::Color32::from_rgb(38, 42, 48);
    let light = egui::Color32::from_rgb(236, 239, 244);
    let mid_x = inner.center().x;
    let mut mesh = egui::Mesh::default();
    let add_horiz_grad =
        |mesh: &mut egui::Mesh, x0: f32, x1: f32, c0: egui::Color32, c1: egui::Color32| {
            let base = mesh.vertices.len() as u32;
            mesh.colored_vertex(egui::pos2(x0, inner.top()), c0);
            mesh.colored_vertex(egui::pos2(x0, inner.bottom()), c0);
            mesh.colored_vertex(egui::pos2(x1, inner.top()), c1);
            mesh.colored_vertex(egui::pos2(x1, inner.bottom()), c1);
            mesh.add_triangle(base, base + 1, base + 2);
            mesh.add_triangle(base + 1, base + 3, base + 2);
        };
    add_horiz_grad(&mut mesh, inner.left(), mid_x, dark, light);
    match spread {
        neoflash_core::GradientSpread::Extend => {
            add_horiz_grad(&mut mesh, mid_x, inner.right(), light, light);
        }
        neoflash_core::GradientSpread::Reflect => {
            add_horiz_grad(&mut mesh, mid_x, inner.right(), light, dark);
        }
        neoflash_core::GradientSpread::Repeat => {
            add_horiz_grad(&mut mesh, mid_x, inner.right(), dark, light);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
    // Dashed boundary line down the middle, matching classic Flash Overflow Styles preview.
    let dash_color = egui::Color32::from_rgb(220, 75, 45);
    let top = inner.top() + 1.0;
    let bot = inner.bottom() - 1.0;
    let span = (bot - top).max(1.0);
    for seg in 0..3 {
        let y0 = top + span * (seg as f32 * 2.0 / 5.0);
        let y1 = top + span * ((seg as f32 * 2.0 + 1.1) / 5.0);
        painter.line_segment(
            [egui::pos2(mid_x, y0), egui::pos2(mid_x, y1.min(bot))],
            egui::Stroke::new(1.0_f32, dash_color),
        );
    }
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(2),
        egui::Stroke::new(1.0_f32, nf.border_default),
        egui::StrokeKind::Inside,
    );
}

fn flow_row(
    ui: &mut egui::Ui,
    mut spread: neoflash_core::GradientSpread,
    mut linear_rgb: bool,
) -> Option<(neoflash_core::GradientSpread, bool)> {
    let nf = neoflash_ui::palette(ui.ctx());
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(rich("Overflow", TextStyle::Body).color(nf.text_secondary));
        let mut chosen = None;
        let (select_resp, _) = Select::new("color-gradient-overflow", spread_label(spread))
            .width(114.0)
            .icon(i16::FLOW_EXTEND)
            .show(ui, |ui| {
                ui.set_min_width(136.0);
                for (mode, padded_label) in [
                    (neoflash_core::GradientSpread::Extend, "       Extend"),
                    (neoflash_core::GradientSpread::Reflect, "       Reflect"),
                    (neoflash_core::GradientSpread::Repeat, "       Repeat"),
                ] {
                    let item_resp = ui.add(MenuItem::new(padded_label).checked(spread == mode));
                    let preview_rect = egui::Rect::from_center_size(
                        egui::pos2(item_resp.rect.left() + 38.0, item_resp.rect.center().y),
                        egui::vec2(22.0, 14.0),
                    );
                    paint_overflow_preview(ui.painter(), preview_rect, mode);
                    if item_resp.clicked() {
                        chosen = Some(mode);
                        ui.close();
                    }
                }
            });
        // Overpaint the 16x16 placeholder icon slot inside the closed Select with the visual overflow preview.
        let closed_icon_bg = if select_resp.hovered() {
            nf.bg_hover
        } else {
            nf.bg_raised
        };
        let closed_preview = egui::Rect::from_center_size(
            egui::pos2(
                select_resp.rect.left() + space::S3 + 8.0,
                select_resp.rect.center().y,
            ),
            egui::vec2(20.0, 13.0),
        );
        ui.painter().rect_filled(
            closed_preview.expand(1.0),
            egui::CornerRadius::ZERO,
            closed_icon_bg,
        );
        paint_overflow_preview(ui.painter(), closed_preview, spread);
        if let Some(next_spread) = chosen
            && next_spread != spread
        {
            spread = next_spread;
            changed = true;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(Checkbox::new(&mut linear_rgb, "Linear RGB"))
                .changed()
            {
                changed = true;
            }
        });
    });
    changed.then_some((spread, linear_rgb))
}

fn color_to_hsb(c: Color) -> Hsb {
    let (r, g, b, a) = (
        c[0] as f32 / 255.0,
        c[1] as f32 / 255.0,
        c[2] as f32 / 255.0,
        c[3] as f32 / 255.0,
    );
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d <= 1e-6 {
        0.0
    } else if (max - r).abs() <= 1e-6 {
        ((g - b) / d).rem_euclid(6.0) / 6.0
    } else if (max - g).abs() <= 1e-6 {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    let s = if max <= 1e-6 { 0.0 } else { d / max };
    hsv(h, s, max, a)
}

/// Color panel compact state (R-055), shared with the mixers through egui temp data.
fn color_picker_expanded() -> egui::Id {
    egui::Id::new("color-picker-expanded")
}
fn color_picker_toggle() -> egui::Id {
    egui::Id::new("color-picker-toggle")
}
fn color_recent() -> egui::Id {
    egui::Id::new("color-recent")
}

fn color_mixer_ui(
    ui: &mut egui::Ui,
    hsb_cache_id: egui::Id,
    field_label: &str,
    color: &mut Color,
    model_hsb: &mut bool,
) -> Option<egui::Id> {
    let was_none = *color == [0, 0, 0, 0];
    let mut hsb = ui
        .data(|d| d.get_temp::<Hsb>(hsb_cache_id))
        .filter(|cached| egui::Color32::from(*cached).to_srgba_unmultiplied() == *color)
        .unwrap_or_else(|| color_to_hsb(*color));
    let mut changed_id = None;
    // Outside the Color panel (no flag set) the mixer keeps its full picker.
    let expanded = ui
        .data(|d| d.get_temp::<bool>(color_picker_expanded()))
        .unwrap_or(true);
    if !expanded {
        // Compact: hex · alpha with the picker toggle, then recent colours.
        let mut field_c32 =
            egui::Color32::from_rgba_unmultiplied(color[0], color[1], color[2], color[3]);
        // The picker toggle leads the row, clear of the panel's overlay scroll bar.
        ui.horizontal(|ui| {
            if ui
                .add(neoflash_ui::components::IconButton::new(
                    neoflash_ui::icons::i16::CHEVRIGHT,
                    "Show color picker",
                ))
                .clicked()
            {
                ui.data_mut(|d| d.insert_temp(color_picker_toggle(), true));
            }
            let w = ui.available_width();
            let cf = ColorField::new(&mut field_c32, field_label)
                .width(w)
                .show(ui);
            if cf.swatch_clicked {
                ui.data_mut(|d| d.insert_temp(color_picker_toggle(), true));
            }
            if cf.response.changed() {
                let prev = *color;
                *color = field_c32.to_srgba_unmultiplied();
                if was_none && color[..3] != prev[..3] && color[3] == 0 {
                    color[3] = 255;
                }
                *color = ensure_solid_non_none(*color);
                changed_id = Some(cf.response.id);
            }
        });
        let recent: Vec<Color> = ui.data(|d| d.get_temp(color_recent())).unwrap_or_default();
        if !recent.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(space::S1, space::S1);
                for c in recent {
                    let c32 = egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]);
                    let r = ui
                        .add(neoflash_ui::components::SwatchCell {
                            color: c32,
                            selected: c == *color,
                        })
                        .on_hover_text(format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2]));
                    if r.clicked() {
                        *color = ensure_solid_non_none(c);
                        changed_id = Some(r.id);
                    }
                }
            });
        }
        ui.data_mut(|d| d.insert_temp(hsb_cache_id, color_to_hsb(*color)));
        return changed_id;
    }

    // Expanded (or outside the Color panel): the hex · alpha row stays on top, where the compact
    // state shows it, and the picker opens below it (R-055).
    let mut field_c32 =
        egui::Color32::from_rgba_unmultiplied(color[0], color[1], color[2], color[3]);
    let in_color_panel = ui
        .data(|d| d.get_temp::<bool>(color_picker_expanded()))
        .is_some();
    ui.horizontal(|ui| {
        if in_color_panel
            && ui
                .add(neoflash_ui::components::IconButton::new(
                    neoflash_ui::icons::i16::CHEVDOWN,
                    "Hide color picker",
                ))
                .clicked()
        {
            ui.data_mut(|d| d.insert_temp(color_picker_toggle(), true));
        }
        let w = ui.available_width();
        let cf = ColorField::new(&mut field_c32, field_label)
            .width(w)
            .show(ui);
        if cf.response.changed() {
            let prev = *color;
            *color = field_c32.to_srgba_unmultiplied();
            if was_none && color[..3] != prev[..3] && color[3] == 0 {
                color[3] = 255;
            }
            *color = ensure_solid_non_none(*color);
            hsb = color_to_hsb(*color);
            changed_id = Some(cf.response.id);
        }
    });

    let before_hsb = hsb;
    let picker_r = ui.add(ColorPicker { color: &mut hsb });
    if picker_r.changed() {
        if was_none
            && before_hsb.a == 0.0
            && (hsb.h != before_hsb.h || hsb.s != before_hsb.s || hsb.v != before_hsb.v)
        {
            hsb.a = 1.0;
        }
        *color = ensure_solid_non_none(egui::Color32::from(hsb).to_srgba_unmultiplied());
        changed_id = Some(picker_r.id);
    }

    Segmented::show(ui, model_hsb, &[(false, "RGB"), (true, "HSB")], false);

    ui.horizontal(|ui| {
        if !*model_hsb {
            let mut r = f64::from(color[0]);
            let mut g = f64::from(color[1]);
            let mut b = f64::from(color[2]);
            let rr = ui.add(
                Field::new(&mut r, "Red")
                    .prefix("R")
                    .range(0.0..=255.0)
                    .decimals(0)
                    .width(80.0),
            );
            let rg = ui.add(
                Field::new(&mut g, "Green")
                    .prefix("G")
                    .range(0.0..=255.0)
                    .decimals(0)
                    .width(80.0),
            );
            let rb = ui.add(
                Field::new(&mut b, "Blue")
                    .prefix("B")
                    .range(0.0..=255.0)
                    .decimals(0)
                    .width(80.0),
            );
            for resp in [&rr, &rg, &rb] {
                if resp.changed() {
                    color[0] = r.round().clamp(0.0, 255.0) as u8;
                    color[1] = g.round().clamp(0.0, 255.0) as u8;
                    color[2] = b.round().clamp(0.0, 255.0) as u8;
                    if was_none && color[3] == 0 {
                        color[3] = 255;
                    }
                    *color = ensure_solid_non_none(*color);
                    hsb = color_to_hsb(*color);
                    changed_id = Some(resp.id);
                }
            }
        } else {
            let mut h_deg = f64::from(hsb.h * 360.0);
            let mut s_pct = f64::from(hsb.s * 100.0);
            let mut v_pct = f64::from(hsb.v * 100.0);
            let rh = ui.add(
                Field::new(&mut h_deg, "Hue")
                    .prefix("H")
                    .unit("°")
                    .range(0.0..=360.0)
                    .decimals(0)
                    .width(80.0),
            );
            let rs = ui.add(
                Field::new(&mut s_pct, "Saturation")
                    .prefix("S")
                    .unit("%")
                    .range(0.0..=100.0)
                    .decimals(0)
                    .width(80.0),
            );
            let rv = ui.add(
                Field::new(&mut v_pct, "Brightness")
                    .prefix("B")
                    .unit("%")
                    .range(0.0..=100.0)
                    .decimals(0)
                    .width(80.0),
            );
            for resp in [&rh, &rs, &rv] {
                if resp.changed() {
                    hsb.h = (h_deg.clamp(0.0, 360.0) / 360.0) as f32;
                    hsb.s = (s_pct.clamp(0.0, 100.0) / 100.0) as f32;
                    hsb.v = (v_pct.clamp(0.0, 100.0) / 100.0) as f32;
                    if was_none && hsb.a == 0.0 {
                        hsb.a = 1.0;
                    }
                    *color =
                        ensure_solid_non_none(egui::Color32::from(hsb).to_srgba_unmultiplied());
                    changed_id = Some(resp.id);
                }
            }
        }
    });

    ui.data_mut(|d| d.insert_temp(hsb_cache_id, hsb));
    changed_id
}

fn gradient_stops_bar_ui(
    ui: &mut egui::Ui,
    grad: &mut GradientPaint,
    selected_stop: &mut usize,
) -> Option<egui::Id> {
    let p = neoflash_ui::palette(ui.ctx());
    if grad.stops.is_empty() {
        return None;
    }
    grad.stops.sort_by_key(|s| s.ratio);
    if *selected_stop >= grad.stops.len() {
        *selected_stop = grad.stops.len() - 1;
    }
    let width = ui.available_width().max(120.0);
    let (rect, mut response) =
        ui.allocate_exact_size(egui::vec2(width, 40.0), egui::Sense::hover());
    let bar = egui::Rect::from_min_size(rect.min, egui::vec2(width, 20.0));
    crate::paint_controls::paint_checkerboard(ui.painter(), bar.shrink(1.0), 4.0);

    // 64-slice mesh sampled through `GradientPaint::sample_at_unit` so the bar matches
    // sRGB / Linear RGB and multi-stop ratios accurately.
    let inner = bar.shrink(1.0);
    let mut mesh = egui::Mesh::default();
    const SLICES: usize = 64;
    for i in 0..=SLICES {
        let u = i as f64 / SLICES as f64;
        let x = egui::lerp(inner.left()..=inner.right(), i as f32 / SLICES as f32);
        let [r, g, b, a] = grad.sample_at_unit(u);
        let c = egui::Color32::from_rgba_unmultiplied(r, g, b, a);
        mesh.colored_vertex(egui::pos2(x, inner.top()), c);
        mesh.colored_vertex(egui::pos2(x, inner.bottom()), c);
    }
    for i in 0..(SLICES as u32) {
        let b = i * 2;
        mesh.add_triangle(b, b + 1, b + 2);
        mesh.add_triangle(b + 1, b + 3, b + 2);
    }
    ui.painter().add(egui::Shape::mesh(mesh));
    ui.painter().rect_stroke(
        bar,
        egui::CornerRadius::same(neoflash_ui::tokens::radius::SM as u8),
        egui::Stroke::new(1.0_f32, p.border_default),
        egui::StrokeKind::Inside,
    );

    let mut changed = false;
    let mut stop_hit = false;
    let mut remove_idx: Option<usize> = None;
    let mut dragged_target: Option<(usize, u8, Color)> = None;

    for i in 0..grad.stops.len() {
        let stop = grad.stops[i].clone();
        let t = f32::from(stop.ratio) / 255.0;
        let x = bar.left() + t * bar.width();
        let sr = egui::Rect::from_center_size(
            egui::pos2(x, bar.bottom() + 10.0),
            egui::vec2(12.0, 16.0),
        );
        let id = response.id.with(("stop", i));
        let r = ui.interact(sr, id, egui::Sense::click_and_drag());
        if r.clicked() || r.drag_started() || r.dragged() {
            *selected_stop = i;
            stop_hit = true;
        }
        if r.dragged()
            && let Some(pos) = r.interact_pointer_pos()
        {
            if (pos.y - bar.bottom()).abs() > 36.0 && grad.stops.len() > 2 {
                remove_idx = Some(i);
            } else {
                let next_t = ((pos.x - bar.left()) / bar.width()).clamp(0.0, 1.0);
                let next_ratio = (next_t * 255.0).round() as u8;
                if next_ratio != stop.ratio {
                    dragged_target = Some((i, next_ratio, stop.color));
                }
            }
        }
        let sel = i == *selected_stop;
        let border = if sel {
            p.accent_default
        } else {
            p.border_strong
        };
        let top = egui::pos2(x, bar.bottom() + 2.0);
        let bl = egui::pos2(sr.left(), bar.bottom() + 7.0);
        let br = egui::pos2(sr.right(), bar.bottom() + 7.0);
        ui.painter().add(egui::Shape::convex_polygon(
            vec![top, br, bl],
            p.bg_raised,
            egui::Stroke::new(if sel { 1.5_f32 } else { 1.0_f32 }, border),
        ));
        let body = egui::Rect::from_min_max(bl, sr.max);
        ui.painter()
            .rect_filled(body, egui::CornerRadius::same(2), p.bg_raised);
        let c32 = egui::Color32::from_rgba_unmultiplied(
            stop.color[0],
            stop.color[1],
            stop.color[2],
            stop.color[3],
        );
        ui.painter().rect_filled(
            egui::Rect::from_center_size(body.center(), egui::vec2(6.0, 6.0)),
            egui::CornerRadius::same(1),
            c32,
        );
        ui.painter().rect_stroke(
            body,
            egui::CornerRadius::same(2),
            egui::Stroke::new(if sel { 1.5_f32 } else { 1.0_f32 }, border),
            egui::StrokeKind::Inside,
        );
    }

    if let Some((idx, next_ratio, color)) = dragged_target {
        grad.stops[idx].ratio = next_ratio;
        let mut tagged: Vec<(bool, GradientStop)> = grad
            .stops
            .iter()
            .cloned()
            .enumerate()
            .map(|(i, s)| (i == idx, s))
            .collect();
        tagged.sort_by_key(|(_, s)| s.ratio);
        if let Some(new_sel) = tagged
            .iter()
            .position(|(is_dragged, s)| *is_dragged && s.color == color)
        {
            *selected_stop = new_sel;
        }
        grad.stops = tagged.into_iter().map(|(_, s)| s).collect();
        changed = true;
    } else if let Some(idx) = remove_idx {
        grad.stops.remove(idx);
        *selected_stop = (*selected_stop).min(grad.stops.len().saturating_sub(1));
        changed = true;
    } else {
        // Click anywhere on the 20px gradient bar OR in the 20px pointer strip below it
        // (outside existing stop pointers) to insert a new stop sampled at that position.
        let add_zone = ui.interact(rect, response.id.with("bar-or-rail"), egui::Sense::click());
        if add_zone.clicked()
            && !stop_hit
            && grad.stops.len() < 15
            && let Some(pos) = add_zone.interact_pointer_pos()
        {
            let t = ((pos.x - bar.left()) / bar.width()).clamp(0.0, 1.0);
            let ratio = (t * 255.0).round() as u8;
            let color = grad.sample_at_unit(f64::from(t));
            let mut tagged: Vec<(bool, GradientStop)> =
                grad.stops.iter().cloned().map(|s| (false, s)).collect();
            tagged.push((true, GradientStop { ratio, color }));
            tagged.sort_by_key(|(_, s)| s.ratio);
            if let Some(new_sel) = tagged.iter().position(|(is_new, _)| *is_new) {
                *selected_stop = new_sel;
            }
            grad.stops = tagged.into_iter().map(|(_, s)| s).collect();
            changed = true;
        }
    }

    if changed {
        response.mark_changed();
        return Some(response.id);
    }
    None
}

fn stroke_section_ui(ui: &mut egui::Ui, width: &mut f64, with_divider: bool) -> Option<egui::Id> {
    let nf = neoflash_ui::palette(ui.ctx());
    if with_divider {
        ui.add_space(space::S1);
        let r = ui.available_rect_before_wrap();
        ui.painter().hline(
            r.x_range(),
            r.top(),
            egui::Stroke::new(1.0_f32, nf.border_subtle),
        );
        ui.add_space(space::S2);
    }
    ui.label(rich("Stroke", TextStyle::Overline).color(nf.text_tertiary));
    let mut changed = None;
    let (width_response, _) = two_column_row(
        ui,
        |ui, cell_width| {
            ui.add(
                Field::new(width, "Stroke width")
                    .prefix("Width")
                    .unit("px")
                    .range(0.0..=100.0)
                    .decimals(1)
                    .width(cell_width),
            )
        },
        |ui, cell_width| {
            ui.add_enabled_ui(false, |ui| {
                Select::new("color-stroke-style", "Solid")
                    .width(cell_width)
                    .show(ui, |_| {});
            })
        },
    );
    if width_response.changed() {
        changed = Some(width_response.id);
    }
    changed
}

/// Rotate / flip commands in the order of the Transform panel icon group.
const TRANSFORM_COMMANDS: [(&str, [f64; 6]); 4] = [
    ("Rotate 90° CCW", [0., -1., 1., 0., 0., 0.]),
    ("Rotate 90° CW", [0., 1., -1., 0., 0., 0.]),
    ("Flip Horizontal", [-1., 0., 0., 1., 0., 0.]),
    ("Flip Vertical", [1., 0., 0., -1., 0., 0.]),
];

type EdgeIcons = [[(Icon, &'static str); 3]; 2];
const ALIGN_ICONS: EdgeIcons = [
    [
        (i16::ALIGN_LEFT, "Left"),
        (i16::ALIGN_HCENTER, "Center"),
        (i16::ALIGN_RIGHT, "Right"),
    ],
    [
        (i16::ALIGN_TOP, "Top"),
        (i16::ALIGN_VMIDDLE, "Middle"),
        (i16::ALIGN_BOTTOM, "Bottom"),
    ],
];
const DISTRIBUTE_ICONS: EdgeIcons = [
    [
        (i16::DIST_LEFT, "Left"),
        (i16::DIST_HCENTER, "Center"),
        (i16::DIST_RIGHT, "Right"),
    ],
    [
        (i16::DIST_TOP, "Top"),
        (i16::DIST_VMIDDLE, "Middle"),
        (i16::DIST_BOTTOM, "Bottom"),
    ],
];

/// Two `NF2/Icon Group`s (horizontal edges · vertical edges). Accessible names
/// stay "Align Left", "Distribute Top", ….
fn alignment_groups(
    ui: &mut egui::Ui,
    operation: &str,
    icons: EdgeIcons,
    mut action: impl FnMut(&mut egui::Ui, usize, usize),
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::S2;
        for (axis, group) in icons.iter().enumerate() {
            let names = group.map(|(_, label)| format!("{operation} {label}"));
            let items: Vec<(Icon, &str)> = group
                .iter()
                .zip(&names)
                .map(|((icon, _), name)| (*icon, name.as_str()))
                .collect();
            if let Some(edge) = (IconGroup {
                items: &items,
                selected: None,
            })
            .show(ui)
            {
                action(ui, axis, edge);
            }
        }
    });
}

/// Uncollapsible group caption (`NF2/Overline`, secondary), as in Align and Transform.
fn overline(ui: &mut egui::Ui, text: &str) {
    let p = neoflash_ui::palette(ui.ctx());
    let color = if ui.is_enabled() {
        p.text_secondary
    } else {
        p.text_disabled
    };
    ui.add_space(space::S1);
    ui.label(rich(text, TextStyle::Overline).color(color));
}

/// Inline row label (`NF2/Body`, secondary).
fn caption_label(ui: &mut egui::Ui, text: &str) {
    let p = neoflash_ui::palette(ui.ctx());
    let color = if ui.is_enabled() {
        p.text_secondary
    } else {
        p.text_disabled
    };
    ui.label(rich(text, TextStyle::Body).color(color));
}

/// Two prefixed fields side by side and an optional link (constrain) toggle.
fn field_pair(
    ui: &mut egui::Ui,
    prefixes: [&str; 2],
    values: [&mut f64; 2],
    range: std::ops::RangeInclusive<f64>,
    suffix: &str,
    link: Option<&mut bool>,
) -> [egui::Response; 2] {
    let [a, b] = values;
    let first_range = range.clone();
    if let Some(linked) = link {
        let (first, (second, link_response)) = two_column_row(
            ui,
            |ui, width| number_field(ui, prefixes[0], a, first_range, 1., suffix, Some(width)),
            |ui, width| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = space::S2;
                    let field_width = (width - crate::theme::control_h() - space::S2).max(0.0);
                    let second =
                        number_field(ui, prefixes[1], b, range, 1., suffix, Some(field_width));
                    let link_response = ui
                        .add(IconButton::new(i16::LINK, "Constrain proportions").selected(*linked));
                    (second, link_response)
                })
                .inner
            },
        );
        if link_response.clicked() {
            *linked = !*linked;
        }
        [first, second]
    } else {
        let (first, second) = two_column_row(
            ui,
            |ui, width| number_field(ui, prefixes[0], a, first_range, 1., suffix, Some(width)),
            |ui, width| number_field(ui, prefixes[1], b, range, 1., suffix, Some(width)),
        );
        [first, second]
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
        ElementKind::TextBox { format, .. } => {
            let [_, r, g, b] = format.color.to_be_bytes();
            Some(([r, g, b, 255], [0; 4], 0., false))
        }
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
        ElementKind::TextBox { format, .. } => {
            let [_, r, g, b] = format.color.to_be_bytes();
            let mut color = [r, g, b, 255];
            apply_color(&mut color, old.0, new.0);
            format.color = u32::from_be_bytes([0, color[0], color[1], color[2]]);
        }
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
            update_gradient_for_color_edit(gradient, before, after);
        }
    }
    true
}

/// Solid RGB fields replace a gradient; a standalone opacity edit keeps its
/// geometry and stop colors, applying the requested alpha to every stop.
pub(crate) fn update_gradient_for_color_edit(
    gradient: &mut Option<GradientPaint>,
    before: Color,
    after: Color,
) {
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

#[cfg(test)]
mod arrange_tests {
    use super::*;
    use crate::commands::{CommandId, effective_shortcuts, primary_shortcut_label};
    use crate::selection::SelectionTarget;
    use crate::workspace::ShortcutPrefs;

    fn make_rect(id: &str, x: f64) -> Element {
        Element {
            id: id.into(),
            name: id.into(),
            transform: Transform {
                x,
                ..Transform::default()
            },
            kind: ElementKind::Rectangle {
                width: 20.0,
                height: 20.0,
                fill: [200, 80, 80, 255],
                stroke: [0, 0, 0, 255],
                stroke_width: 1.0,
            },
            ..Element::default()
        }
    }

    fn layer_element_ids(editor: &Editor) -> Vec<String> {
        editor.symbol().timeline.layers[0].keyframes[0]
            .elements
            .iter()
            .map(|e| e.id.clone())
            .collect()
    }

    #[test]
    fn arrange_selection_reorders_layer_depth_respects_groups_and_binds_default_shortcuts() {
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        let layer_id = editor.symbol().timeline.layers[0].id.clone();
        editor.symbol_mut().timeline.layers[0].keyframes[0].elements = vec![
            make_rect("back", 0.0),
            make_rect("mid", 30.0),
            make_rect("front", 60.0),
        ];
        editor.project.validate().unwrap();

        // 1. Default shortcuts for Modify ▸ Arrange must be bound to ⇧⌘↑/⌘↑/⌘↓/⇧⌘↓ and ⇧⌘]/⌘]/⌘[/⇧⌘[.
        let prefs = ShortcutPrefs::default();
        assert_eq!(
            primary_shortcut_label(&prefs, CommandId::ModifyArrangeFront),
            "⇧⌘↑"
        );
        assert_eq!(
            primary_shortcut_label(&prefs, CommandId::ModifyArrangeForward),
            "⌘↑"
        );
        assert_eq!(
            primary_shortcut_label(&prefs, CommandId::ModifyArrangeBackward),
            "⌘↓"
        );
        assert_eq!(
            primary_shortcut_label(&prefs, CommandId::ModifyArrangeBack),
            "⇧⌘↓"
        );
        assert_eq!(
            effective_shortcuts(&prefs, CommandId::ModifyArrangeFront).len(),
            2
        );
        assert_eq!(
            effective_shortcuts(&prefs, CommandId::ModifyArrangeForward).len(),
            2
        );
        assert_eq!(
            effective_shortcuts(&prefs, CommandId::ModifyArrangeBackward).len(),
            2
        );
        assert_eq!(
            effective_shortcuts(&prefs, CommandId::ModifyArrangeBack).len(),
            2
        );

        // 2. Boundary no-op checks: "back" cannot move further back; "front" cannot move further forward.
        editor.selection.targets = vec![SelectionTarget {
            layer: layer_id.clone(),
            element: "back".into(),
        }];
        assert!(!editor.can_arrange_selection(Some(Arrange::Backward)));
        assert!(!editor.can_arrange_selection(Some(Arrange::Back)));
        assert!(editor.can_arrange_selection(Some(Arrange::Forward)));
        assert!(editor.can_arrange_selection(Some(Arrange::Front)));

        // 3. Step "back" forward once -> [mid, back, front], then to front -> [mid, front, back].
        editor.arrange_selection(Arrange::Forward);
        assert_eq!(layer_element_ids(&editor), vec!["mid", "back", "front"]);
        editor.arrange_selection(Arrange::Front);
        assert_eq!(layer_element_ids(&editor), vec!["mid", "front", "back"]);
        assert!(!editor.can_arrange_selection(Some(Arrange::Forward)));
        assert!(!editor.can_arrange_selection(Some(Arrange::Front)));

        // Step "back" backward once -> [mid, back, front], then to back -> [back, mid, front].
        editor.arrange_selection(Arrange::Backward);
        assert_eq!(layer_element_ids(&editor), vec!["mid", "back", "front"]);
        editor.arrange_selection(Arrange::Back);
        assert_eq!(layer_element_ids(&editor), vec!["back", "mid", "front"]);

        // Undo restores the previous step cleanly.
        editor.undo();
        assert_eq!(layer_element_ids(&editor), vec!["mid", "back", "front"]);
        editor.redo();
        assert_eq!(layer_element_ids(&editor), vec!["back", "mid", "front"]);

        // 4. Authoring group on the same layer is treated as an atomic depth unit:
        // stepping "back" forward jumps past the entire group [mid, front] without splitting it.
        editor.symbol_mut().timeline.layers[0].properties.groups = vec![AuthoringGroup {
            id: "grp-1".into(),
            name: "Group 1".into(),
            members: vec!["mid".into(), "front".into()],
        }];
        editor.project.format_version = editor.project.format_version.max(4);
        editor.project.validate().unwrap();

        editor.selection.targets = vec![SelectionTarget {
            layer: layer_id.clone(),
            element: "back".into(),
        }];
        editor.arrange_selection(Arrange::Forward);
        assert_eq!(
            layer_element_ids(&editor),
            vec!["mid", "front", "back"],
            "Bring forward must step past an entire authoring group without splitting its members"
        );
        editor.project.validate().unwrap();

        editor.arrange_selection(Arrange::Backward);
        assert_eq!(
            layer_element_ids(&editor),
            vec!["back", "mid", "front"],
            "Send backward must step behind an entire authoring group without splitting its members"
        );
        editor.project.validate().unwrap();
    }

    #[test]
    fn grouped_selection_shows_single_combined_bounding_box_and_ungroup_restores_individual_boxes()
    {
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        let layer_id = editor.symbol().timeline.layers[0].id.clone();
        let mut a = make_rect("a", 10.0);
        a.merge = true;
        let mut b = make_rect("b", 80.0);
        b.merge = true;
        editor.symbol_mut().timeline.layers[0].keyframes[0].elements = vec![a, b];

        editor.set_selection(vec![
            SelectionTarget {
                layer: layer_id.clone(),
                element: "a".into(),
            },
            SelectionTarget {
                layer: layer_id.clone(),
                element: "b".into(),
            },
        ]);

        let painter = ctx.layer_painter(egui::LayerId::background());
        let scene: Vec<DrawItem> = editor
            .presented_interactive_authored_scene(false)
            .draws
            .into_iter()
            .map(|d| d.item)
            .collect();
        let mapping = crate::stage_mapping::StageMapping::identity();

        // Before grouping: 2 separate boxes (one for "a" and one for "b", both merge shapes).
        let before_boxes = editor.selection_boxes(&scene, &painter, mapping);
        assert_eq!(before_boxes.len(), 2);
        assert!(before_boxes.iter().all(|(_, _, _, _, is_merge)| *is_merge));

        // Group via ⌘G (`group_selection`): must coalesce into 1 single combined Group bounding box.
        editor.group_selection();
        let grouped_boxes = editor.selection_boxes(&scene, &painter, mapping);
        assert_eq!(
            grouped_boxes.len(),
            1,
            "Grouped objects must display a single combined bounding box"
        );
        let (_, world_rect, context_rect, kind, is_merge) = &grouped_boxes[0];
        assert_eq!(*kind, neoflash_ui::components::canvas::SelectionKind::Group);
        assert!(!*is_merge);
        assert!(world_rect.min.x <= 10.5 && world_rect.max.x >= 99.5);
        assert!(context_rect.min.x <= 10.5 && context_rect.max.x >= 99.5);

        // Ungroup via ⇧⌘G (`ungroup_selection`): restores individual bounding boxes for each object.
        editor.ungroup_selection();
        let after_boxes = editor.selection_boxes(&scene, &painter, mapping);
        assert_eq!(
            after_boxes.len(),
            2,
            "After Ungroup, each object must display its own individual bounding box"
        );
        assert!(after_boxes.iter().all(|(_, _, _, _, is_merge)| *is_merge));
        editor.project.validate().unwrap();
    }

    #[test]
    fn transform_submodes_menu_commands_envelope_delete_and_hotkey_bindings_work() {
        use crate::authoring_tools::FreeTransformMode;
        use crate::transform_geometry::{
            Bounds, EnvelopeCage, EnvelopeHandle, deform_element_envelope,
        };

        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        let layer_id = editor.symbol().timeline.layers[0].id.clone();
        editor.symbol_mut().timeline.layers[0].keyframes[0].elements =
            vec![make_rect("shape1", 20.0), make_rect("shape2", 60.0)];

        let prefs = ShortcutPrefs::default();
        assert_eq!(primary_shortcut_label(&prefs, CommandId::ModifyGroup), "⌘G");
        assert_eq!(
            primary_shortcut_label(&prefs, CommandId::ModifyUngroup),
            "⇧⌘G"
        );
        assert_eq!(
            primary_shortcut_label(&prefs, CommandId::ModifyBreakApart),
            "⌘B"
        );
        assert_eq!(
            primary_shortcut_label(&prefs, CommandId::ModifyScaleAndRotate),
            "⌥⌘S"
        );
        assert_eq!(
            primary_shortcut_label(&prefs, CommandId::ModifyRotate90Cw),
            "⇧⌘9"
        );
        assert_eq!(
            primary_shortcut_label(&prefs, CommandId::ModifyRotate90Ccw),
            "⇧⌘7"
        );

        editor.set_selection(vec![SelectionTarget {
            layer: layer_id.clone(),
            element: "shape1".into(),
        }]);
        assert!(editor.can_distort_or_envelope_selection());

        // Switching Free Transform sub-modes activates Tool::Transform and sets the mode.
        for mode in [
            FreeTransformMode::RotateAndSkew,
            FreeTransformMode::Scale,
            FreeTransformMode::Distort,
            FreeTransformMode::Envelope,
            FreeTransformMode::FreeTransform,
        ] {
            editor.set_free_transform_mode(mode);
            assert_eq!(editor.tool, crate::app::Tool::Transform);
            assert_eq!(editor.active_free_transform_mode(), mode);
        }

        // Rotate 90° CW / CCW and Flip Horizontal / Vertical.
        editor.rotate_90_cw_selection(&ctx);
        let rot_cw = editor.selected_element().unwrap().transform.rotation;
        assert!((rot_cw - 90.0).abs() < 1e-4);
        editor.rotate_90_ccw_selection(&ctx);
        let rot_ccw = editor.selected_element().unwrap().transform.rotation;
        assert!(rot_ccw.abs() < 1e-4);

        editor.flip_horizontal_selection(&ctx);
        let m_h = transform_matrix(editor.selected_element().unwrap().transform);
        assert!((m_h[0] + 1.0).abs() < 1e-4 && (m_h[3] - 1.0).abs() < 1e-4);
        editor.flip_vertical_selection(&ctx);
        let m_hv = transform_matrix(editor.selected_element().unwrap().transform);
        assert!((m_hv[0] + 1.0).abs() < 1e-4 && (m_hv[3] + 1.0).abs() < 1e-4);

        // Scale and Rotate numeric dialog apply.
        assert!(editor.apply_scale_and_rotate(&ctx, 150.0, 45.0));
        assert!((editor.selected_element().unwrap().transform.scale_x.abs() - 1.5).abs() < 1e-3);

        // Remove Transform resets scale/rotation/skew to identity.
        editor.reset_selection_transform(&ctx);
        let reset_t = editor.selected_element().unwrap().transform;
        assert!((reset_t.scale_x - 1.0).abs() < 1e-4);
        assert!((reset_t.scale_y - 1.0).abs() < 1e-4);
        assert!(reset_t.rotation.abs() < 1e-4);
        assert!(reset_t.skew.abs() < 1e-4);

        // Envelope deformation and Delete Envelope restoration.
        let orig = editor.selected_element().unwrap().clone();
        let cage = EnvelopeCage::from_bounds(Bounds {
            min: [20.0, 0.0],
            max: [40.0, 20.0],
        })
        .dragged(EnvelopeHandle::Anchor(1), [30.0, 0.0], [30.0, -10.0])
        .unwrap();
        let warped = deform_element_envelope(&orig, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], &cage).unwrap();
        editor.authoring_modify.envelope_backups.insert(
            (editor.symbol_id().to_owned(), editor.frame, orig.id.clone()),
            orig.clone(),
        );
        editor.symbol_mut().timeline.layers[0].keyframes[0].elements[0] = warped;
        assert!(editor.can_delete_envelope_selection());
        assert!(editor.delete_envelope_selection());
        assert_eq!(editor.selected_element().unwrap().kind, orig.kind);
        editor.project.validate().unwrap();
    }

    #[test]
    fn gradient_transform_icon_and_color_fill_type_popover_work_everywhere() {
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        let layer_id = editor.symbol().timeline.layers[0].id.clone();

        // 1. Gradient Transform icon (16px and 20px) uses a crisp bordered horizontal linearGradient.
        let g16 = crate::ui_icons::Icon::Gradient.nf2(false);
        let g20 = crate::ui_icons::Icon::Gradient.tool_glyph();
        assert!(g16.svg.contains("<linearGradient"));
        assert!(g20.svg.contains("<linearGradient"));
        assert_eq!(g16.size, 16);
        assert_eq!(g20.size, 20);
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            let p = ctx.layer_painter(egui::LayerId::background());
            neoflash_ui::icons::paint(
                &p,
                ctx,
                egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(16.0, 16.0)),
                g16,
                egui::Color32::WHITE,
            );
            neoflash_ui::icons::paint(
                &p,
                ctx,
                egui::Rect::from_min_size(egui::pos2(20.0, 0.0), egui::vec2(20.0, 20.0)),
                g20,
                egui::Color32::WHITE,
            );
        });

        // 2. Switching a selected Rectangle through Linear gradient, Radial gradient, Bitmap fill,
        // None and Solid colour via `set_selection_fill_type`, plus Undo/Redo.
        editor.symbol_mut().timeline.layers[0].keyframes[0].elements = vec![make_rect("r1", 10.0)];
        editor.set_selection(vec![SelectionTarget {
            layer: layer_id.clone(),
            element: "r1".into(),
        }]);
        editor.authoring_modify.color_selection = true;

        editor.set_selection_fill_type(false, PaintFillType::LinearGradient);
        assert!(matches!(
            editor.selected_element().unwrap().kind,
            ElementKind::PaintedPath {
                fill_gradient: Some(GradientPaint {
                    kind: GradientKind::Linear,
                    ..
                }),
                ..
            }
        ));
        editor.project.validate().unwrap();

        editor.set_selection_fill_type(false, PaintFillType::RadialGradient);
        assert!(matches!(
            editor.selected_element().unwrap().kind,
            ElementKind::PaintedPath {
                fill_gradient: Some(GradientPaint {
                    kind: GradientKind::Radial,
                    ..
                }),
                ..
            }
        ));
        editor.project.validate().unwrap();

        editor.set_selection_fill_type(false, PaintFillType::BitmapFill);
        assert!(editor.target_has_bitmap_fill(&SelectionTarget {
            layer: layer_id.clone(),
            element: "r1".into(),
        }));
        editor.project.validate().unwrap();

        editor.set_selection_fill_type(false, PaintFillType::Solid);
        assert!(!editor.target_has_bitmap_fill(&SelectionTarget {
            layer: layer_id.clone(),
            element: "r1".into(),
        }));
        editor.project.validate().unwrap();

        // 3. Opening the anchored Color & Fill Type popover from Properties (`shape-fill`),
        // Tool defaults (`inspector-tool-fill`), Brushes (`tool-well-fill`) and Swatches
        // (`swatches-open-mixer`) renders without panic and exposes all fill types.
        for (key, stroke_well, sel) in [
            ("shape-fill", false, true),
            ("inspector-tool-fill", false, false),
            ("tool-well-fill", false, false),
            ("swatches-open-mixer", false, true),
        ] {
            request_color_popover(
                &ctx,
                key,
                egui::Rect::from_min_size(egui::pos2(20.0, 20.0), egui::vec2(22.0, 22.0)),
                stroke_well,
                sel,
            );
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    editor.properties(ui, ctx);
                });
            });
            assert!(
                ctx.data(|d| d.get_temp::<ActiveColorPopover>(active_color_popover_id()))
                    .is_some()
            );
            close_color_popover(&ctx);
        }

        // 4. Gradient stop insertion (on bar and in pointer rail below it) selects the newly
        // inserted middle stop (index 1, not the right endpoint index 2), and overflow modes
        // (`Extend`, `Reflect`, `Repeat`) + `Linear RGB` recalculate accurately.
        editor.set_selection_fill_type(false, PaintFillType::LinearGradient);
        editor.set_selection_gradient_options(false, GradientSpread::Reflect, true);
        {
            let ElementKind::PaintedPath {
                fill_gradient: Some(fg),
                ..
            } = &editor.selected_element().unwrap().kind
            else {
                panic!("expected linear gradient");
            };
            assert_eq!(fg.spread, GradientSpread::Reflect);
            assert!(fg.linear_rgb);
            // Reflect maps u = 1.25 to 0.75:
            assert!((fg.spread.apply(1.25) - 0.75).abs() < 1e-6);
            assert!((GradientSpread::Repeat.apply(1.25) - 0.25).abs() < 1e-6);
        }
        editor.project.validate().unwrap();
    }
}
