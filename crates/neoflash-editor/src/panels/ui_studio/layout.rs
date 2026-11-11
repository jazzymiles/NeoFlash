#![allow(unused_imports)]
use super::*;
use crate::theme::{ScrollTheme as _, ThemeMode};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use neoflash_ui::components as c;
use neoflash_ui::components::timeline::{
    self as tl, GridKind, LayerColumns, LayerKind, LayerRow, LayersHeaderColumns, ModeCell,
};
use neoflash_ui::icons::{self, Icon, i16};
use neoflash_ui::tokens::{Palette, TextStyle, radius, space};
use neoflash_ui::typography::rich;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

// ─────────────────────────────────────────────────────────────────────────────
// 5. Pure Deterministic Box & Grid Layout Engine (`compute_studio_layout`)
// ─────────────────────────────────────────────────────────────────────────────

/// Resolved layout box and styling metrics for a single [`UiStudioNode`].
#[derive(Clone, Debug, PartialEq)]
pub struct ComputedNodeBox {
    pub id: UiNodeId,
    pub rect: Rect,
    pub content_rect: Rect,
    pub resolved_width: f32,
    pub resolved_height: f32,
    pub padding: [f32; 4],
    pub gap: Vec2,
    pub typography: TypographyOverride,
}

/// Complete layout solution for a [`UiStudioDocument`] at a given canvas origin and viewport size.
#[derive(Clone, Debug, PartialEq)]
pub struct ComputedStudioLayout {
    pub boxes: BTreeMap<UiNodeId, ComputedNodeBox>,
    pub order: Vec<UiNodeId>,
    pub root_rect: Rect,
}

impl Default for ComputedStudioLayout {
    fn default() -> Self {
        Self {
            boxes: BTreeMap::new(),
            order: Vec::new(),
            root_rect: Rect::NOTHING,
        }
    }
}

impl ComputedStudioLayout {
    pub fn node_box(&self, id: UiNodeId) -> Option<&ComputedNodeBox> {
        self.boxes.get(&id)
    }

    /// Hit-test the deepest visible node containing `pos`.
    pub fn hit_test(&self, pos: Pos2) -> Option<UiNodeId> {
        for &id in self.order.iter().rev() {
            if let Some(b) = self.boxes.get(&id)
                && b.rect.contains(pos)
            {
                return Some(id);
            }
        }
        None
    }
}

/// Intrinsic default height of a leaf widget given the active [`UiStudioSystems`].
pub fn intrinsic_leaf_height(
    kind: &UiNodeKind,
    systems: &UiStudioSystems,
    typo: &TypographyOverride,
) -> f32 {
    let density = systems.density.active();
    match kind {
        UiNodeKind::Field { .. }
        | UiNodeKind::TextField { .. }
        | UiNodeKind::Select { .. }
        | UiNodeKind::Button { .. }
        | UiNodeKind::ColorSwatch { .. } => density.control_height,
        UiNodeKind::IconButton { md_size: true, .. } => (density.control_height + 8.0).max(28.0),
        UiNodeKind::IconButton { md_size: false, .. } => {
            if systems.tool_rail.button_side > 0.0 {
                density.control_height
            } else {
                32.0
            }
        }
        UiNodeKind::Checkbox { .. } | UiNodeKind::Switch { .. } | UiNodeKind::Slider { .. } => {
            density.row_height.max(20.0)
        }
        UiNodeKind::Label => typo.line_height_pt.max(typo.size_pt + 4.0).max(16.0),
        UiNodeKind::Separator { vertical: false } => 1.0,
        UiNodeKind::Separator { vertical: true } => 20.0,
        UiNodeKind::TableRow { .. } => density.row_height,
        UiNodeKind::Panel { .. }
        | UiNodeKind::Section { .. }
        | UiNodeKind::Row
        | UiNodeKind::Column
        | UiNodeKind::Grid { .. } => density.control_height,
    }
}

/// Intrinsic default width of a leaf widget when `UiSizeUnit::Auto` is used.
pub fn intrinsic_leaf_width(
    node: &UiStudioNode,
    systems: &UiStudioSystems,
    typo: &TypographyOverride,
) -> f32 {
    let density = systems.density.active();
    match &node.kind {
        UiNodeKind::IconButton { md_size: true, .. } => (density.control_height + 8.0).max(28.0),
        UiNodeKind::IconButton { md_size: false, .. } => density.control_height,
        UiNodeKind::ColorSwatch { .. } => 74.0,
        UiNodeKind::Button { .. } => {
            let char_w = (typo.size_pt * 0.58).max(5.5);
            (node.label.chars().count() as f32 * char_w + 28.0).max(64.0)
        }
        UiNodeKind::Checkbox { .. } | UiNodeKind::Switch { .. } => {
            let char_w = (typo.size_pt * 0.58).max(5.5);
            (node.label.chars().count() as f32 * char_w + 36.0).max(64.0)
        }
        UiNodeKind::Label => {
            let char_w = (typo.size_pt * 0.58).max(5.5);
            (node.label.chars().count() as f32 * char_w + 8.0).max(32.0)
        }
        UiNodeKind::Separator { vertical: true } => 1.0,
        _ => 120.0,
    }
}

/// Resolve effective `[top, right, bottom, left]` padding and `[gap_x, gap_y]` for `node`,
/// honoring `node.style.system_binding` unless locally overridden.
pub fn resolve_node_spacing(node: &UiStudioNode, systems: &UiStudioSystems) -> ([f32; 4], Vec2) {
    let mut pad = node.style.padding.resolve(systems);
    let mut gap = node.style.gap.resolve(systems);

    match node.style.system_binding {
        Some(SharedSystemKind::TwoColumnForm) => {
            if !node.style.gap.x.is_override() {
                gap.x = systems.two_column_form.gap.resolve(systems);
            }
            if !node.style.gap.y.is_override() {
                gap.y = systems.two_column_form.gap.resolve(systems);
            }
        }
        Some(SharedSystemKind::SectionHeader) => {
            if !node.style.gap.y.is_override() {
                gap.y = systems.section_header.body_gap.resolve(systems);
            }
        }
        Some(SharedSystemKind::PanelHeader) => {
            if pad == [0.0, 0.0, 0.0, 0.0] {
                let p = systems.panel_header.body_padding.resolve(systems);
                pad = [p, p, p, p];
            }
        }
        Some(SharedSystemKind::ToolRail) => {
            if pad == [0.0, 0.0, 0.0, 0.0] {
                let p = systems.tool_rail.padding;
                pad = [p, p, p, p];
            }
            if !node.style.gap.x.is_override() {
                gap.x = systems.tool_rail.gap;
            }
            if !node.style.gap.y.is_override() {
                gap.y = systems.tool_rail.gap;
            }
        }
        None => {
            if matches!(node.kind, UiNodeKind::Panel { .. }) && pad == [0.0, 0.0, 0.0, 0.0] {
                let p = systems.panel_header.body_padding.resolve(systems);
                pad = [p, p, p, p];
            }
        }
    }

    (pad, gap)
}

/// Compute the deterministic layout (`ComputedStudioLayout`) for `doc` and `systems`.
pub fn compute_studio_layout(
    doc: &UiStudioDocument,
    systems: &UiStudioSystems,
    origin: Pos2,
    available_size: Vec2,
) -> ComputedStudioLayout {
    let mut out = ComputedStudioLayout::default();
    let root_rect = layout_node_recursive(
        &doc.root,
        systems,
        origin,
        available_size.x.max(64.0),
        available_size.y.max(64.0),
        None,
        &mut out,
    );
    out.root_rect = root_rect;
    out
}

fn layout_node_recursive(
    node: &UiStudioNode,
    systems: &UiStudioSystems,
    origin: Pos2,
    parent_content_width: f32,
    parent_content_height: f32,
    forced_width: Option<f32>,
    out: &mut ComputedStudioLayout,
) -> Rect {
    let mut typo = node.style.typography.resolve(systems);
    if !node.style.typography.is_custom() {
        match node.style.system_binding {
            Some(SharedSystemKind::SectionHeader) => {
                typo = systems
                    .typography
                    .resolve_style(systems.section_header.text_style);
            }
            Some(SharedSystemKind::PanelHeader) => {
                typo = systems
                    .typography
                    .resolve_style(systems.panel_header.text_style);
            }
            _ => {}
        }
    }

    let (pad, gap) = resolve_node_spacing(node, systems);
    let pad_h = pad[1] + pad[3];
    let pad_v = pad[0] + pad[2];

    let intrinsic_w = intrinsic_leaf_width(node, systems, &typo);
    let mut resolved_width = forced_width.unwrap_or_else(|| {
        node.box_constraints
            .resolve_width(parent_content_width, intrinsic_w)
    });
    if let Some(min_w) = node.box_constraints.min_width {
        resolved_width = resolved_width.max(min_w);
    }
    if let Some(max_w) = node.box_constraints.max_width {
        resolved_width = resolved_width.min(max_w);
    }

    let inner_width = (resolved_width - pad_h).max(0.0);
    let header_h = match &node.kind {
        UiNodeKind::Panel { tabs, .. } => {
            if tabs.is_empty() {
                0.0
            } else {
                systems.panel_header.height.max(24.0)
            }
        }
        UiNodeKind::Section { .. } => systems.section_header.height.max(20.0),
        _ => 0.0,
    };

    let content_origin = egui::pos2(origin.x + pad[3], origin.y + header_h + pad[0]);
    let node_order_idx = out.order.len();
    out.order.push(node.id);

    let children_height = match &node.kind {
        UiNodeKind::Panel { .. } | UiNodeKind::Column => layout_column_children(
            &node.children,
            systems,
            content_origin,
            inner_width,
            gap.y,
            out,
        ),
        UiNodeKind::Section { expanded, .. } => {
            if *expanded {
                layout_column_children(
                    &node.children,
                    systems,
                    content_origin,
                    inner_width,
                    gap.y,
                    out,
                )
            } else {
                0.0
            }
        }
        UiNodeKind::Row => layout_row_children(
            &node.children,
            systems,
            content_origin,
            inner_width,
            gap.x,
            out,
        ),
        UiNodeKind::Grid { columns } => {
            let effective_cols = if node.style.system_binding == Some(SharedSystemKind::ToolRail) {
                vec![
                    UiSizeUnit::Px(systems.tool_rail.button_side);
                    systems.tool_rail.columns.max(1)
                ]
            } else if columns.is_empty() {
                vec![UiSizeUnit::Percent(100.0)]
            } else {
                columns.clone()
            };
            layout_grid_children(
                &node.children,
                &effective_cols,
                systems,
                content_origin,
                inner_width,
                gap,
                out,
            )
        }
        _ => intrinsic_leaf_height(&node.kind, systems, &typo),
    };

    let auto_total_h = header_h + pad_v + children_height;
    let resolved_height = node
        .box_constraints
        .resolve_height(parent_content_height, auto_total_h);

    let rect = Rect::from_min_size(origin, egui::vec2(resolved_width, resolved_height));
    let content_rect = Rect::from_min_size(
        content_origin,
        egui::vec2(inner_width, (resolved_height - header_h - pad_v).max(0.0)),
    );

    let _ = node_order_idx;
    out.boxes.insert(
        node.id,
        ComputedNodeBox {
            id: node.id,
            rect,
            content_rect,
            resolved_width,
            resolved_height,
            padding: pad,
            gap,
            typography: typo,
        },
    );

    rect
}

fn layout_column_children(
    children: &[UiStudioNode],
    systems: &UiStudioSystems,
    origin: Pos2,
    inner_width: f32,
    gap_y: f32,
    out: &mut ComputedStudioLayout,
) -> f32 {
    let mut y = origin.y;
    let mut visible_count = 0usize;
    for child in children {
        if !child.visible {
            continue;
        }
        if visible_count > 0 {
            y += gap_y;
        }
        let child_rect = layout_node_recursive(
            child,
            systems,
            egui::pos2(origin.x, y),
            inner_width,
            0.0,
            None,
            out,
        );
        y = child_rect.bottom();
        visible_count += 1;
    }
    (y - origin.y).max(0.0)
}

fn layout_row_children(
    children: &[UiStudioNode],
    systems: &UiStudioSystems,
    origin: Pos2,
    inner_width: f32,
    gap_x: f32,
    out: &mut ComputedStudioLayout,
) -> f32 {
    let visible: Vec<&UiStudioNode> = children.iter().filter(|c| c.visible).collect();
    if visible.is_empty() {
        return 0.0;
    }
    let tracks: Vec<UiSizeUnit> = visible.iter().map(|c| c.box_constraints.width).collect();
    let intrinsics: Vec<f32> = visible
        .iter()
        .map(|c| {
            let t = c.style.typography.resolve(systems);
            intrinsic_leaf_width(c, systems, &t)
        })
        .collect();
    let widths = resolve_track_sizes(&tracks, inner_width, gap_x, &intrinsics);

    let mut x = origin.x;
    let mut max_h = 0.0_f32;
    for (i, child) in visible.into_iter().enumerate() {
        let w = widths.get(i).copied().unwrap_or(64.0);
        let r = layout_node_recursive(
            child,
            systems,
            egui::pos2(x, origin.y),
            inner_width,
            0.0,
            Some(w),
            out,
        );
        max_h = max_h.max(r.height());
        x += w + gap_x;
    }
    max_h
}

fn layout_grid_children(
    children: &[UiStudioNode],
    columns: &[UiSizeUnit],
    systems: &UiStudioSystems,
    origin: Pos2,
    inner_width: f32,
    gap: Vec2,
    out: &mut ComputedStudioLayout,
) -> f32 {
    let visible: Vec<&UiStudioNode> = children.iter().filter(|c| c.visible).collect();
    if visible.is_empty() || columns.is_empty() {
        return 0.0;
    }
    let col_count = columns.len().max(1);
    let intrinsics = vec![64.0_f32; col_count];
    let col_widths = resolve_track_sizes(columns, inner_width, gap.x, &intrinsics);

    let mut y = origin.y;
    for (row_idx, row_slice) in visible.chunks(col_count).enumerate() {
        if row_idx > 0 {
            y += gap.y;
        }
        let mut x = origin.x;
        let mut row_h = 0.0_f32;
        for (col_idx, child) in row_slice.iter().copied().enumerate() {
            let cell_w = col_widths.get(col_idx).copied().unwrap_or(64.0);
            // Inside a grid cell: if the child is 100% width, it fills `cell_w`; if it has a custom
            // `%` or `px` width, it resolves relative to `cell_w`!
            let child_w = child.box_constraints.resolve_width(
                cell_w,
                intrinsic_leaf_width(child, systems, &child.style.typography.resolve(systems)),
            );
            let r = layout_node_recursive(
                child,
                systems,
                egui::pos2(x, y),
                cell_w,
                0.0,
                Some(child_w),
                out,
            );
            row_h = row_h.max(r.height());
            x += cell_w + gap.x;
        }
        y += row_h;
    }
    (y - origin.y).max(0.0)
}
