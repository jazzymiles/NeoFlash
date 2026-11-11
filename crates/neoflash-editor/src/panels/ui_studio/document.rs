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
// 4. Node Tree & Structural Operations (Wrap / Unwrap / Grids / Reorder)
// ─────────────────────────────────────────────────────────────────────────────

/// Button style variant inside the studio widget tree.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StudioButtonKind {
    Primary,
    #[default]
    Secondary,
    Ghost,
    Danger,
}

impl StudioButtonKind {
    pub const ALL: [Self; 4] = [Self::Primary, Self::Secondary, Self::Ghost, Self::Danger];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Primary => "Primary",
            Self::Secondary => "Secondary",
            Self::Ghost => "Ghost",
            Self::Danger => "Danger",
        }
    }

    pub const fn to_nf2(self) -> c::ButtonType {
        match self {
            Self::Primary => c::ButtonType::Primary,
            Self::Secondary => c::ButtonType::Secondary,
            Self::Ghost => c::ButtonType::Ghost,
            Self::Danger => c::ButtonType::Danger,
        }
    }
}

/// Container and leaf widget kinds supported by the UI Layout Studio.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UiNodeKind {
    // ── Containers ──
    /// Root or nested panel surface with optional header tabs.
    Panel {
        tabs: Vec<String>,
        active_tab: usize,
    },
    /// Collapsible or static section group (`NF2/Section Header` + body).
    Section { collapsible: bool, expanded: bool },
    /// Horizontal flex/track container (`Row`).
    Row,
    /// Vertical stack container (`Column`).
    Column,
    /// Multi-column grid container (`2`, `3`, `4+` columns in `%`, `px`, `Auto`, or `Fill`).
    /// Any cell (child) inside a `Grid` can itself be another `Grid`, `Row`, or `Column`!
    Grid { columns: Vec<UiSizeUnit> },

    // ── Leaf Widgets ──
    /// Numeric field (`NF2/Field` / `NF2/Stepper Field`).
    Field {
        value: f64,
        prefix: String,
        unit: String,
        decimals: usize,
        stepper: bool,
    },
    /// Text input (`NF2/TextField`).
    TextField {
        text: String,
        prefix: String,
        hint: String,
        mono: bool,
    },
    /// Dropdown select (`NF2/Select`).
    Select {
        selected: usize,
        options: Vec<String>,
    },
    /// Action button (`NF2/Button`).
    Button {
        button_kind: StudioButtonKind,
        icon_name: Option<String>,
    },
    /// Square icon button (`NF2/Icon Button`).
    IconButton {
        icon_name: String,
        selected: bool,
        md_size: bool,
    },
    /// Boolean checkbox (`NF2/Checkbox`).
    Checkbox { checked: bool },
    /// Immediate toggle switch (`NF2/Switch`).
    Switch { on: bool },
    /// Color well / swatch chip (`NF2/Color Well` / `NF2/Swatch`).
    ColorSwatch { rgba: [u8; 4], is_stroke: bool },
    /// Numeric slider (`NF2/Slider`).
    Slider {
        value: f64,
        min: f64,
        max: f64,
        show_field: bool,
    },
    /// Text label (`NF2/*` typography).
    Label,
    /// Horizontal or vertical divider line (`NF2/Divider`).
    Separator { vertical: bool },
    /// Multi-column table row (`NF2/Table Row`).
    TableRow {
        cells: Vec<String>,
        column_fractions: Vec<f32>,
        is_header: bool,
        selected: bool,
    },
}

impl UiNodeKind {
    pub const fn is_container(&self) -> bool {
        matches!(
            self,
            Self::Panel { .. }
                | Self::Section { .. }
                | Self::Row
                | Self::Column
                | Self::Grid { .. }
        )
    }

    pub const fn type_badge(&self) -> &'static str {
        match self {
            Self::Panel { .. } => "Panel",
            Self::Section { .. } => "Section",
            Self::Row => "Row",
            Self::Column => "Column",
            Self::Grid { .. } => "Grid",
            Self::Field { .. } => "Field",
            Self::TextField { .. } => "TextField",
            Self::Select { .. } => "Select",
            Self::Button { .. } => "Button",
            Self::IconButton { .. } => "IconButton",
            Self::Checkbox { .. } => "Checkbox",
            Self::Switch { .. } => "Switch",
            Self::ColorSwatch { .. } => "ColorSwatch",
            Self::Slider { .. } => "Slider",
            Self::Label => "Label",
            Self::Separator { .. } => "Separator",
            Self::TableRow { .. } => "TableRow",
        }
    }

    pub fn default_text_style(&self) -> StudioTextStyle {
        match self {
            Self::Panel { .. } => StudioTextStyle::SmallStrong,
            Self::Section { .. } => StudioTextStyle::Overline,
            Self::Field { .. } | Self::ColorSwatch { .. } => StudioTextStyle::Mono,
            Self::TableRow {
                is_header: true, ..
            } => StudioTextStyle::Overline,
            Self::TableRow {
                is_header: false, ..
            } => StudioTextStyle::Small,
            _ => StudioTextStyle::Body,
        }
    }
}

/// One node in the UI Layout Studio hierarchy.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UiStudioNode {
    pub id: UiNodeId,
    pub label: String,
    pub kind: UiNodeKind,
    pub box_constraints: UiBoxConstraints,
    pub style: NodeVisualStyle,
    pub visible: bool,
    pub enabled: bool,
    pub children: Vec<UiStudioNode>,
}

impl UiStudioNode {
    pub fn new(id: UiNodeId, label: impl Into<String>, kind: UiNodeKind) -> Self {
        let default_text = kind.default_text_style();
        let mut style = NodeVisualStyle {
            typography: TypographyBinding::System(default_text),
            ..Default::default()
        };
        let box_constraints = match &kind {
            UiNodeKind::Panel { .. } => {
                UiBoxConstraints::new(UiSizeUnit::Px(320.0), UiSizeUnit::Auto)
            }
            UiNodeKind::Section { .. } => {
                style.system_binding = Some(SharedSystemKind::SectionHeader);
                style.gap = GapPair::both_token(SpacingToken::S2);
                UiBoxConstraints::percent_width(100.0)
            }
            UiNodeKind::Row | UiNodeKind::Column | UiNodeKind::Grid { .. } => {
                style.gap = GapPair::both_token(SpacingToken::S2);
                UiBoxConstraints::percent_width(100.0)
            }
            UiNodeKind::IconButton { .. } => {
                UiBoxConstraints::new(UiSizeUnit::Px(32.0), UiSizeUnit::Auto)
            }
            UiNodeKind::ColorSwatch { .. } => {
                UiBoxConstraints::new(UiSizeUnit::Px(74.0), UiSizeUnit::Auto)
            }
            UiNodeKind::Button { .. } => UiBoxConstraints::new(UiSizeUnit::Auto, UiSizeUnit::Auto),
            UiNodeKind::Separator { vertical: false } => {
                UiBoxConstraints::new(UiSizeUnit::Percent(100.0), UiSizeUnit::Px(1.0))
            }
            UiNodeKind::Separator { vertical: true } => {
                UiBoxConstraints::new(UiSizeUnit::Px(1.0), UiSizeUnit::Px(20.0))
            }
            _ => UiBoxConstraints::percent_width(100.0),
        };
        Self {
            id,
            label: label.into(),
            kind,
            box_constraints,
            style,
            visible: true,
            enabled: true,
            children: Vec::new(),
        }
    }

    pub fn with_width(mut self, width: UiSizeUnit) -> Self {
        self.box_constraints.width = width;
        self
    }

    pub fn with_height(mut self, height: UiSizeUnit) -> Self {
        self.box_constraints.height = height;
        self
    }

    pub fn with_padding(mut self, padding: Padding) -> Self {
        self.style.padding = padding;
        self
    }

    pub fn with_gap(mut self, gap: GapPair) -> Self {
        self.style.gap = gap;
        self
    }

    pub fn with_system_binding(mut self, sys: SharedSystemKind) -> Self {
        self.style.system_binding = Some(sys);
        self
    }

    pub fn with_children(mut self, children: Vec<UiStudioNode>) -> Self {
        self.children = children;
        self
    }
}

/// 1-click container presets for wrapping selected elements or inserting new containers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ContainerWrapPreset {
    /// Horizontal `Row`.
    Row,
    /// Vertical `Column`.
    Column,
    /// 2-column equal percentage grid (`[50%, 50%]`).
    Grid2Equal,
    /// 3-column equal percentage grid (`[33.33%, 33.33%, 33.34%]`).
    Grid3Equal,
    /// 4-column equal percentage grid (`[25%, 25%, 25%, 25%]`).
    Grid4Equal,
    /// Label column in `px` + 2 equal `%` columns (`[120px, 50%, 50%]`).
    GridMixedLabel2Col,
    /// Custom column tracks.
    GridCustom(Vec<UiSizeUnit>),
    /// Collapsible `Section`.
    Section,
}

impl ContainerWrapPreset {
    pub fn to_node_kind(&self) -> (String, UiNodeKind, Option<SharedSystemKind>) {
        match self {
            Self::Row => ("Row".to_owned(), UiNodeKind::Row, None),
            Self::Column => ("Column".to_owned(), UiNodeKind::Column, None),
            Self::Grid2Equal => (
                "Grid (2 Cols)".to_owned(),
                UiNodeKind::Grid {
                    columns: vec![UiSizeUnit::Percent(50.0), UiSizeUnit::Percent(50.0)],
                },
                Some(SharedSystemKind::TwoColumnForm),
            ),
            Self::Grid3Equal => (
                "Grid (3 Cols)".to_owned(),
                UiNodeKind::Grid {
                    columns: vec![
                        UiSizeUnit::Percent(33.3333),
                        UiSizeUnit::Percent(33.3333),
                        UiSizeUnit::Percent(33.3334),
                    ],
                },
                None,
            ),
            Self::Grid4Equal => (
                "Grid (4 Cols)".to_owned(),
                UiNodeKind::Grid {
                    columns: vec![
                        UiSizeUnit::Percent(25.0),
                        UiSizeUnit::Percent(25.0),
                        UiSizeUnit::Percent(25.0),
                        UiSizeUnit::Percent(25.0),
                    ],
                },
                None,
            ),
            Self::GridMixedLabel2Col => (
                "Grid (120px + 50% + 50%)".to_owned(),
                UiNodeKind::Grid {
                    columns: vec![
                        UiSizeUnit::Px(120.0),
                        UiSizeUnit::Percent(50.0),
                        UiSizeUnit::Percent(50.0),
                    ],
                },
                None,
            ),
            Self::GridCustom(cols) => (
                format!("Grid ({} Cols)", cols.len()),
                UiNodeKind::Grid {
                    columns: cols.clone(),
                },
                None,
            ),
            Self::Section => (
                "Section".to_owned(),
                UiNodeKind::Section {
                    collapsible: true,
                    expanded: true,
                },
                Some(SharedSystemKind::SectionHeader),
            ),
        }
    }
}

/// Authored UI Layout Studio document (panel tree + next ID generator).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UiStudioDocument {
    pub name: String,
    pub root: UiStudioNode,
    pub next_id: UiNodeId,
}

impl Default for UiStudioDocument {
    fn default() -> Self {
        StudioPanelPreset::PropertiesMovieClip.build_document()
    }
}

impl UiStudioDocument {
    pub fn alloc_id(&mut self) -> UiNodeId {
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        id
    }

    pub fn all_node_ids(&self) -> Vec<UiNodeId> {
        fn visit(node: &UiStudioNode, out: &mut Vec<UiNodeId>) {
            out.push(node.id);
            for child in &node.children {
                visit(child, out);
            }
        }
        let mut out = Vec::new();
        visit(&self.root, &mut out);
        out
    }

    pub fn find_node(&self, id: UiNodeId) -> Option<&UiStudioNode> {
        fn visit(node: &UiStudioNode, id: UiNodeId) -> Option<&UiStudioNode> {
            if node.id == id {
                return Some(node);
            }
            for child in &node.children {
                if let Some(found) = visit(child, id) {
                    return Some(found);
                }
            }
            None
        }
        visit(&self.root, id)
    }

    pub fn find_node_mut(&mut self, id: UiNodeId) -> Option<&mut UiStudioNode> {
        fn visit(node: &mut UiStudioNode, id: UiNodeId) -> Option<&mut UiStudioNode> {
            if node.id == id {
                return Some(node);
            }
            for child in &mut node.children {
                if let Some(found) = visit(child, id) {
                    return Some(found);
                }
            }
            None
        }
        visit(&mut self.root, id)
    }

    pub fn find_parent_id(&self, target_id: UiNodeId) -> Option<UiNodeId> {
        fn visit(node: &UiStudioNode, target_id: UiNodeId) -> Option<UiNodeId> {
            for child in &node.children {
                if child.id == target_id {
                    return Some(node.id);
                }
                if let Some(p) = visit(child, target_id) {
                    return Some(p);
                }
            }
            None
        }
        visit(&self.root, target_id)
    }

    /// Returns `true` if `candidate_id` is inside `ancestor_id`'s subtree (including `ancestor_id` itself).
    pub fn is_descendant(&self, ancestor_id: UiNodeId, candidate_id: UiNodeId) -> bool {
        let Some(ancestor) = self.find_node(ancestor_id) else {
            return false;
        };
        fn visit(node: &UiStudioNode, candidate_id: UiNodeId) -> bool {
            if node.id == candidate_id {
                return true;
            }
            node.children.iter().any(|c| visit(c, candidate_id))
        }
        visit(ancestor, candidate_id)
    }

    /// Wrap 1 or more selected nodes into a new container (`Row`, `Column`, `Grid(2/3/4)`, or `Section`).
    ///
    /// Selected nodes that belong to the first selected node's parent are extracted in their original
    /// sibling order and placed inside the newly created container at the position of the first selected node.
    pub fn wrap_selected_nodes(
        &mut self,
        selected_ids: &[UiNodeId],
        preset: ContainerWrapPreset,
    ) -> Option<UiNodeId> {
        let first_id = *selected_ids
            .iter()
            .find(|&&id| id != self.root.id && self.find_parent_id(id).is_some())?;
        let parent_id = self.find_parent_id(first_id)?;
        let wrapper_id = self.alloc_id();
        let (label, kind, sys_binding) = preset.to_node_kind();
        let mut wrapper = UiStudioNode::new(wrapper_id, label, kind);
        if let Some(sys) = sys_binding {
            wrapper.style.system_binding = Some(sys);
        }

        let parent = self.find_node_mut(parent_id)?;
        let first_pos = parent.children.iter().position(|c| c.id == first_id)?;

        let mut extracted = Vec::new();
        let mut idx = 0;
        while idx < parent.children.len() {
            if selected_ids.contains(&parent.children[idx].id) {
                extracted.push(parent.children.remove(idx));
            } else {
                idx += 1;
            }
        }
        if extracted.is_empty() {
            return None;
        }
        wrapper.children = extracted;
        let insert_at = first_pos.min(parent.children.len());
        parent.children.insert(insert_at, wrapper);
        Some(wrapper_id)
    }

    /// Unwrap (`dissolve`) a container (`Row`, `Column`, `Grid`, `Section`), splicing all of its
    /// children directly into its parent container at the exact position of `container_id`.
    pub fn unwrap_selected_container(&mut self, container_id: UiNodeId) -> bool {
        if container_id == self.root.id {
            return false;
        }
        let Some(parent_id) = self.find_parent_id(container_id) else {
            return false;
        };
        let Some(parent) = self.find_node_mut(parent_id) else {
            return false;
        };
        let Some(pos) = parent.children.iter().position(|c| c.id == container_id) else {
            return false;
        };
        if !parent.children[pos].kind.is_container() {
            return false;
        }
        let removed = parent.children.remove(pos);
        for (offset, child) in removed.children.into_iter().enumerate() {
            parent.children.insert(pos + offset, child);
        }
        true
    }

    /// Extract a single child node out of its parent container (`Row`/`Column`/`Grid`/`Section`)
    /// into the grandparent container right after the parent container.
    pub fn extract_node_to_parent(&mut self, node_id: UiNodeId) -> bool {
        if node_id == self.root.id {
            return false;
        }
        let Some(parent_id) = self.find_parent_id(node_id) else {
            return false;
        };
        if parent_id == self.root.id {
            return false;
        }
        let Some(grandparent_id) = self.find_parent_id(parent_id) else {
            return false;
        };
        let Some(parent) = self.find_node_mut(parent_id) else {
            return false;
        };
        let Some(child_pos) = parent.children.iter().position(|c| c.id == node_id) else {
            return false;
        };
        let extracted = parent.children.remove(child_pos);

        let Some(grandparent) = self.find_node_mut(grandparent_id) else {
            return false;
        };
        let insert_pos = grandparent
            .children
            .iter()
            .position(|c| c.id == parent_id)
            .map_or(grandparent.children.len(), |p| p + 1);
        grandparent.children.insert(insert_pos, extracted);
        true
    }

    /// Insert a new node inside `parent_id` (or inside `parent_id`'s parent if `parent_id` is a leaf).
    pub fn insert_node(
        &mut self,
        target_id: UiNodeId,
        label: impl Into<String>,
        kind: UiNodeKind,
    ) -> Option<UiNodeId> {
        let new_id = self.alloc_id();
        let node = UiStudioNode::new(new_id, label, kind);
        let is_container = self
            .find_node(target_id)
            .is_some_and(|n| n.kind.is_container());
        if is_container {
            let parent = self.find_node_mut(target_id)?;
            parent.children.push(node);
            Some(new_id)
        } else if let Some(parent_id) = self.find_parent_id(target_id) {
            let parent = self.find_node_mut(parent_id)?;
            let pos = parent
                .children
                .iter()
                .position(|c| c.id == target_id)
                .map_or(parent.children.len(), |p| p + 1);
            parent.children.insert(pos, node);
            Some(new_id)
        } else {
            self.root.children.push(node);
            Some(new_id)
        }
    }

    /// Insert a nested `Grid` (e.g. 2, 3, or 4 columns) inside `target_container_id` (such as a cell
    /// of an outer `Row` or `Grid`), optionally pre-populating sample `Field` cells.
    pub fn insert_nested_grid(
        &mut self,
        target_container_id: UiNodeId,
        columns: Vec<UiSizeUnit>,
        populate_sample_cells: bool,
    ) -> Option<UiNodeId> {
        let col_count = columns.len().max(1);
        let grid_id = self.insert_node(
            target_container_id,
            format!("Nested Grid ({col_count} Cols)"),
            UiNodeKind::Grid { columns },
        )?;
        if populate_sample_cells {
            for col_idx in 0..col_count {
                let cell_id = self.alloc_id();
                let cell = UiStudioNode::new(
                    cell_id,
                    format!("Cell {}", col_idx + 1),
                    UiNodeKind::Field {
                        value: (col_idx + 1) as f64 * 10.0,
                        prefix: format!("C{}", col_idx + 1),
                        unit: "px".to_owned(),
                        decimals: 0,
                        stepper: false,
                    },
                );
                if let Some(grid_node) = self.find_node_mut(grid_id) {
                    grid_node.children.push(cell);
                }
            }
        }
        Some(grid_id)
    }

    /// Deep-clone `node_id` (regenerating fresh IDs for the entire cloned subtree) and insert right after `node_id`.
    pub fn duplicate_node(&mut self, node_id: UiNodeId) -> Option<UiNodeId> {
        if node_id == self.root.id {
            return None;
        }
        let parent_id = self.find_parent_id(node_id)?;
        let original = self.find_node(node_id)?.clone();

        fn clone_with_fresh_ids(
            doc: &mut UiStudioDocument,
            mut node: UiStudioNode,
            is_top: bool,
        ) -> UiStudioNode {
            node.id = doc.alloc_id();
            if is_top {
                node.label = format!("{} Copy", node.label);
            }
            node.children = node
                .children
                .into_iter()
                .map(|c| clone_with_fresh_ids(doc, c, false))
                .collect();
            node
        }

        let cloned = clone_with_fresh_ids(self, original, true);
        let new_id = cloned.id;
        let parent = self.find_node_mut(parent_id)?;
        let pos = parent
            .children
            .iter()
            .position(|c| c.id == node_id)
            .map_or(parent.children.len(), |p| p + 1);
        parent.children.insert(pos, cloned);
        Some(new_id)
    }

    /// Delete all specified nodes (never deletes the root `Panel`). Returns count of deleted top-level targets.
    pub fn delete_nodes(&mut self, ids: &[UiNodeId]) -> usize {
        fn remove_rec(node: &mut UiStudioNode, ids: &[UiNodeId]) -> usize {
            let mut removed = 0;
            let mut i = 0;
            while i < node.children.len() {
                if ids.contains(&node.children[i].id) {
                    node.children.remove(i);
                    removed += 1;
                } else {
                    removed += remove_rec(&mut node.children[i], ids);
                    i += 1;
                }
            }
            removed
        }
        remove_rec(&mut self.root, ids)
    }

    /// Move `node_id` one slot earlier (`↑`) inside its parent container.
    pub fn move_node_up(&mut self, node_id: UiNodeId) -> bool {
        let Some(parent_id) = self.find_parent_id(node_id) else {
            return false;
        };
        let Some(parent) = self.find_node_mut(parent_id) else {
            return false;
        };
        let Some(pos) = parent.children.iter().position(|c| c.id == node_id) else {
            return false;
        };
        if pos == 0 {
            return false;
        }
        parent.children.swap(pos - 1, pos);
        true
    }

    /// Move `node_id` one slot later (`↓`) inside its parent container.
    pub fn move_node_down(&mut self, node_id: UiNodeId) -> bool {
        let Some(parent_id) = self.find_parent_id(node_id) else {
            return false;
        };
        let Some(parent) = self.find_node_mut(parent_id) else {
            return false;
        };
        let Some(pos) = parent.children.iter().position(|c| c.id == node_id) else {
            return false;
        };
        if pos + 1 >= parent.children.len() {
            return false;
        }
        parent.children.swap(pos, pos + 1);
        true
    }

    /// Move `node_id` into `new_parent_id` at `insert_index` (or at the end if `None`),
    /// rejecting invalid moves where `new_parent_id` is not a container or is inside `node_id`.
    pub fn reparent_node(
        &mut self,
        node_id: UiNodeId,
        new_parent_id: UiNodeId,
        insert_index: Option<usize>,
    ) -> bool {
        if node_id == self.root.id || self.is_descendant(node_id, new_parent_id) {
            return false;
        }
        if !self
            .find_node(new_parent_id)
            .is_some_and(|n| n.kind.is_container())
        {
            return false;
        }
        let Some(old_parent_id) = self.find_parent_id(node_id) else {
            return false;
        };
        let Some(old_parent) = self.find_node_mut(old_parent_id) else {
            return false;
        };
        let Some(old_pos) = old_parent.children.iter().position(|c| c.id == node_id) else {
            return false;
        };
        let extracted = old_parent.children.remove(old_pos);

        let Some(new_parent) = self.find_node_mut(new_parent_id) else {
            return false;
        };
        let idx = insert_index
            .unwrap_or(new_parent.children.len())
            .min(new_parent.children.len());
        new_parent.children.insert(idx, extracted);
        true
    }

    /// Reset a node's local style overrides back to NF2 system tokens.
    pub fn reset_node_to_system_defaults(&mut self, node_id: UiNodeId) -> bool {
        let Some(node) = self.find_node_mut(node_id) else {
            return false;
        };
        let default_text = node.kind.default_text_style();
        node.style.reset_to_system_tokens(default_text);
        true
    }
}

/// Snapshot entry in [`UiStudioHistory`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UiStudioSnapshot {
    pub label: String,
    pub document: UiStudioDocument,
    pub systems: UiStudioSystems,
    #[serde(default)]
    pub artboards: Vec<StudioArtboard>,
    #[serde(default)]
    pub active_artboard_idx: usize,
}

/// Multi-step Undo/Redo history stack for the UI Layout Studio.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UiStudioHistory {
    pub undo_stack: Vec<UiStudioSnapshot>,
    pub redo_stack: Vec<UiStudioSnapshot>,
    pub max_entries: usize,
}

impl UiStudioHistory {
    pub fn new(max_entries: usize) -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            max_entries: max_entries.max(8),
        }
    }

    pub fn push_snapshot(
        &mut self,
        label: impl Into<String>,
        document: &UiStudioDocument,
        systems: &UiStudioSystems,
    ) {
        self.push_board_snapshot(label, document, systems, &[], 0);
    }

    pub fn push_board_snapshot(
        &mut self,
        label: impl Into<String>,
        document: &UiStudioDocument,
        systems: &UiStudioSystems,
        artboards: &[StudioArtboard],
        active_artboard_idx: usize,
    ) {
        let cap = if self.max_entries == 0 {
            64
        } else {
            self.max_entries
        };
        if self.undo_stack.len() >= cap {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(UiStudioSnapshot {
            label: label.into(),
            document: document.clone(),
            systems: systems.clone(),
            artboards: artboards.to_vec(),
            active_artboard_idx,
        });
        self.redo_stack.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn undo(
        &mut self,
        current_doc: &mut UiStudioDocument,
        current_systems: &mut UiStudioSystems,
    ) -> Option<String> {
        let mut empty_board = Vec::new();
        let mut active_idx = 0usize;
        self.undo_board(
            current_doc,
            current_systems,
            &mut empty_board,
            &mut active_idx,
        )
    }

    pub fn undo_board(
        &mut self,
        current_doc: &mut UiStudioDocument,
        current_systems: &mut UiStudioSystems,
        current_artboards: &mut Vec<StudioArtboard>,
        current_active_idx: &mut usize,
    ) -> Option<String> {
        let prev = self.undo_stack.pop()?;
        let label = prev.label.clone();
        self.redo_stack.push(UiStudioSnapshot {
            label: label.clone(),
            document: current_doc.clone(),
            systems: current_systems.clone(),
            artboards: current_artboards.clone(),
            active_artboard_idx: *current_active_idx,
        });
        *current_doc = prev.document;
        *current_systems = prev.systems;
        if !prev.artboards.is_empty() {
            *current_artboards = prev.artboards;
            *current_active_idx = prev
                .active_artboard_idx
                .min(current_artboards.len().saturating_sub(1));
        }
        Some(label)
    }

    pub fn redo(
        &mut self,
        current_doc: &mut UiStudioDocument,
        current_systems: &mut UiStudioSystems,
    ) -> Option<String> {
        let mut empty_board = Vec::new();
        let mut active_idx = 0usize;
        self.redo_board(
            current_doc,
            current_systems,
            &mut empty_board,
            &mut active_idx,
        )
    }

    pub fn redo_board(
        &mut self,
        current_doc: &mut UiStudioDocument,
        current_systems: &mut UiStudioSystems,
        current_artboards: &mut Vec<StudioArtboard>,
        current_active_idx: &mut usize,
    ) -> Option<String> {
        let next = self.redo_stack.pop()?;
        let label = next.label.clone();
        self.undo_stack.push(UiStudioSnapshot {
            label: label.clone(),
            document: current_doc.clone(),
            systems: current_systems.clone(),
            artboards: current_artboards.clone(),
            active_artboard_idx: *current_active_idx,
        });
        *current_doc = next.document;
        *current_systems = next.systems;
        if !next.artboards.is_empty() {
            *current_artboards = next.artboards;
            *current_active_idx = next
                .active_artboard_idx
                .min(current_artboards.len().saturating_sub(1));
        }
        Some(label)
    }
}
