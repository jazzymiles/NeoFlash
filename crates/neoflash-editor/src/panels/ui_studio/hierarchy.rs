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

pub(super) fn icon_for_node_kind(kind: &UiNodeKind) -> Icon {
    match kind {
        UiNodeKind::Panel { .. } => i16::DESKTOP,
        UiNodeKind::Section { .. } => i16::FOLDEROPEN,
        UiNodeKind::Row => i16::SPACE_H,
        UiNodeKind::Column => i16::SPACE_V,
        UiNodeKind::Grid { .. } => i16::GRID,
        UiNodeKind::Field { .. } | UiNodeKind::TextField { .. } => i16::TEXT,
        UiNodeKind::Select { .. } => i16::CHEVDOWN,
        UiNodeKind::Button { .. } | UiNodeKind::IconButton { .. } => i16::SELECT,
        UiNodeKind::Checkbox { .. } | UiNodeKind::Switch { .. } => i16::CHECK,
        UiNodeKind::ColorSwatch { .. } => i16::SWATCHES,
        UiNodeKind::Slider { .. } => i16::MIXER,
        UiNodeKind::Label => i16::FONT,
        UiNodeKind::Separator { .. } => i16::LINE,
        UiNodeKind::TableRow { .. } => i16::LAYERS,
    }
}

pub(super) fn resolve_icon_by_name(name: &str) -> Icon {
    let trimmed = name.trim().trim_start_matches("16/");
    if let Some(ico) = Icon::by_path(&format!("16/{trimmed}")) {
        return ico;
    }
    match trimmed {
        "select" => i16::SELECT,
        "subselect" => i16::SUBSELECT,
        "transform" => i16::TRANSFORM,
        "lasso" => i16::LASSO,
        "pen" => i16::PEN,
        "text" => i16::TEXT,
        "line" => i16::LINE,
        "rect" => i16::RECT,
        "oval" => i16::OVAL,
        "pencil" | "ink" | "inkbottle" => i16::PENCIL,
        "brush" => i16::BRUSH,
        "bucket" => i16::BUCKET,
        "eyedropper" => i16::EYEDROPPER,
        "eraser" => i16::ERASER,
        "hand" => i16::HAND,
        "zoom" | "search" => i16::SEARCH,
        "align-left" => i16::ALIGN_LEFT,
        "align-right" => i16::ALIGN_RIGHT,
        "align-top" => i16::ALIGN_TOP,
        "align-bottom" => i16::ALIGN_BOTTOM,
        "space-h" | "align-center-h" | "distribute-h" | "match-width" => i16::SPACE_H,
        "space-v" | "align-center-v" | "distribute-v" | "match-height" => i16::SPACE_V,
        "flip-h" => i16::FLIP_H,
        "flip-v" => i16::FLIP_V,
        "rotate-cw" => i16::ROTATE_CW,
        "rotate-ccw" => i16::ROTATE_CCW,
        "eye" => i16::EYE,
        "lock" => i16::LOCK,
        "folder" | "folderopen" => i16::FOLDEROPEN,
        "folderplus" => i16::FOLDERPLUS,
        "layer" | "layers" => i16::LAYERS,
        "layerplus" | "plus" => i16::PLUS,
        "trash" | "delete" => i16::TRASH,
        "play" => i16::PLAY,
        "loop" => i16::LOOP,
        "onion" => i16::ONION,
        "settings" => i16::SETTINGS,
        "code" => i16::CODE,
        "save" => i16::SAVE,
        "copy" => i16::COPY,
        _ => i16::SELECT,
    }
}

/// Node commands of the Hierarchy header, in the order they give way to the ⋯ menu.
#[derive(Clone, Copy, PartialEq, Eq)]
enum NodeAction {
    Delete,
    Duplicate,
    MoveUp,
    MoveDown,
}

impl NodeAction {
    /// Left to right on the header; the last ones move into the ⋯ menu first.
    const ALL: [Self; 4] = [Self::MoveUp, Self::MoveDown, Self::Duplicate, Self::Delete];

    const fn label(self) -> &'static str {
        match self {
            Self::Delete => "Delete selected",
            Self::Duplicate => "Duplicate selected",
            Self::MoveUp => "Move node up",
            Self::MoveDown => "Move node down",
        }
    }

    const fn icon(self) -> Icon {
        match self {
            Self::Delete => i16::TRASH,
            Self::Duplicate => i16::COPY,
            Self::MoveUp => i16::CHEVUP,
            Self::MoveDown => i16::CHEVDOWN,
        }
    }

    fn run(self, state: &mut UiLayoutStudioState) {
        let sel_id = state.primary_selected_id();
        match self {
            Self::Delete => {
                let targets = state.selected_nodes.clone();
                state.commit_history("Delete node(s)");
                let count = state.document.delete_nodes(&targets);
                state.select_single(state.document.root.id);
                state.status_message = format!("Deleted {count} node(s).");
            }
            Self::Duplicate => {
                state.commit_history("Duplicate node");
                if let Some(dup_id) = state.document.duplicate_node(sel_id) {
                    state.select_single(dup_id);
                    state.status_message = "Duplicated selected node.".to_owned();
                }
            }
            Self::MoveUp => {
                state.commit_history("Move node up");
                state.document.move_node_up(sel_id);
            }
            Self::MoveDown => {
                state.commit_history("Move node down");
                state.document.move_node_down(sel_id);
            }
        }
    }
}

/// Containers the selection can be wrapped in (Hierarchy ⋯ ▸ Wrap in).
const WRAP_OPTIONS: [(&str, &str); 5] = [
    ("Row", "Row"),
    ("Column", "Column"),
    ("2-grid", "2-column Grid (50%/50%)"),
    ("3-grid", "3-column Grid (33.3% × 3)"),
    ("4-grid", "4-column Grid (25% × 4)"),
];

fn wrap_preset(index: usize) -> ContainerWrapPreset {
    match index {
        0 => ContainerWrapPreset::Row,
        1 => ContainerWrapPreset::Column,
        2 => ContainerWrapPreset::Grid2Equal,
        3 => ContainerWrapPreset::Grid3Equal,
        _ => ContainerWrapPreset::Grid4Equal,
    }
}

/// Header of the left column: Hierarchy | Add tabs, then the node commands of the selection. Icon
/// buttons are shown while they fit; the rest and the wrap commands are in the ⋯ menu.
fn render_hierarchy_header(ui: &mut Ui, state: &mut UiLayoutStudioState, rect: Rect) {
    let nf = neoflash_ui::palette(ui.ctx());
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let ui = &mut row;
    ui.spacing_mut().item_spacing.x = 0.0;
    for (tab, label) in [
        (StudioHierarchyTab::Hierarchy, "Hierarchy"),
        (StudioHierarchyTab::Add, "Add"),
    ] {
        if ui
            .add(c::PanelTab {
                label,
                active: state.hierarchy_tab == tab,
            })
            .clicked()
        {
            state.hierarchy_tab = tab;
        }
    }

    let is_root = state.primary_selected_id() == state.document.root.id;
    let side = neoflash_ui::metrics::control();
    let edge = space::s1();
    // The ⋯ button always fits; the icon buttons take what is left after the tabs.
    let free = rect.right() - ui.min_rect().right() - edge - side;
    let shown = ((free / side).floor().max(0.0) as usize).min(NodeAction::ALL.len());
    let (on_row, in_menu) = NodeAction::ALL.split_at(shown);

    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.add_space(edge);
        let more = ui.add(c::IconButton::new(i16::MORE, "Node commands"));
        c::popup_below(&more, neoflash_ui::metrics::menu_width(), |ui| {
            c::submenu(ui, "Wrap in", |ui| {
                for (index, (label, what)) in WRAP_OPTIONS.into_iter().enumerate() {
                    if ui.add(c::MenuItem::new(label)).clicked() {
                        state.wrap_selection(wrap_preset(index), what);
                    }
                }
            });
            if ui.add(c::MenuItem::new("Unwrap")).clicked() {
                state.unwrap_selection();
            }
            if !in_menu.is_empty() {
                c::menu_separator(ui);
            }
            for action in in_menu {
                let mut item = c::MenuItem::new(action.label()).icon(action.icon());
                if *action == NodeAction::Delete {
                    item = item.danger();
                }
                if ui.add_enabled(!is_root, item).clicked() {
                    action.run(state);
                }
            }
        });
        for action in on_row.iter().rev() {
            if ui
                .add_enabled(!is_root, c::IconButton::new(action.icon(), action.label()))
                .clicked()
            {
                action.run(state);
            }
        }
    });
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0_f32, nf.border_subtle),
    );
}

pub(super) fn render_hierarchy_and_palette_column(
    ui: &mut Ui,
    state: &mut UiLayoutStudioState,
    width: f32,
    height: f32,
) {
    ui.spacing_mut().item_spacing = Vec2::ZERO;
    let column = Rect::from_min_size(ui.max_rect().min, egui::vec2(width, height));
    let header = Rect::from_min_size(
        column.min,
        egui::vec2(width, neoflash_ui::metrics::panel_tab()),
    );
    render_hierarchy_header(ui, state, header);

    let body = Rect::from_min_max(header.left_bottom(), column.max);
    let mut body_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(body)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    body_ui.set_clip_rect(body.intersect(ui.clip_rect()));
    let ui = &mut body_ui;
    let pad = space::s1();
    let tab = state.hierarchy_tab;
    egui::ScrollArea::vertical()
        .id_salt(("studio-hierarchy-scroll", tab))
        .auto_shrink([false, false])
        .themed()
        .show(ui, |ui| {
            egui::Frame::new()
                .inner_margin(egui::Margin::same(pad as i8))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(0.0, 1.0);
                    let inner_w = (width - 2.0 * pad).max(0.0);
                    ui.set_width(inner_w);
                    match tab {
                        StudioHierarchyTab::Hierarchy => render_hierarchy_tree(ui, state, inner_w),
                        StudioHierarchyTab::Add => render_add_palette(ui, state, inner_w),
                    }
                });
        });
}

fn render_hierarchy_tree(ui: &mut Ui, state: &mut UiLayoutStudioState, row_w: f32) {
    let mut rows = Vec::new();
    collect_tree_rows(&state.document.root, 0, &mut rows);
    let mut clicked_row: Option<(UiNodeId, bool)> = None;
    let mut drop_target: Option<(UiNodeId, UiNodeId)> = None;
    let icon = neoflash_ui::metrics::ICON_UI;

    for (id, depth, label, meta, kind_icon, is_container) in rows {
        let is_selected = state.selected_nodes.contains(&id);
        let item_kind = if is_container {
            c::TreeItemType::FolderOpen
        } else {
            c::TreeItemType::Symbol(kind_icon)
        };
        // Chevron, icon and gaps before the label; the size note keeps its width at the end.
        let prefix_w = space::s2()
            + depth as f32 * neoflash_ui::metrics::LAYER_INDENT
            + icon
            + space::s1()
            + icon
            + space::s2();
        // The size note is shown only next to a whole name; a name that still does not fit ends
        // with "…" and keeps the full text in its tooltip.
        let free_w = (row_w - prefix_w - space::s3()).max(0.0);
        let meta_w = super::text_width(ui, &meta, TextStyle::Small) + space::s2();
        let label_w = super::text_width(ui, &label, TextStyle::Body);
        let meta = if label_w + meta_w <= free_w {
            meta
        } else {
            String::new()
        };
        let display_label = super::elided(ui, &label, TextStyle::Body, free_w);
        let tree_item = ui
            .allocate_ui_with_layout(
                egui::vec2(row_w, neoflash_ui::metrics::row()),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    c::TreeItem::new(&display_label, item_kind)
                        .icon(kind_icon)
                        .depth(depth)
                        .selected(is_selected)
                        .meta(&meta)
                        .show(ui)
                },
            )
            .inner;
        if display_label != label {
            c::tooltip(tree_item.row.clone(), &label, None);
        }

        if tree_item.row.clicked() {
            let multi = ui.input(|i| i.modifiers.shift || i.modifiers.command);
            clicked_row = Some((id, multi));
        }
        if tree_item.row.drag_started() && id != state.document.root.id {
            state.drag_source_node = Some(id);
        }
        if let Some(drag_id) = state.drag_source_node
            && drag_id != id
            && is_container
            && tree_item.row.hovered()
            && ui.input(|i| i.pointer.any_released())
        {
            drop_target = Some((drag_id, id));
        }
    }

    if ui.input(|i| i.pointer.any_released()) {
        state.drag_source_node = None;
    }
    if let Some((id, multi)) = clicked_row {
        if multi {
            state.toggle_multi_select(id);
        } else {
            state.select_single(id);
        }
    }
    if let Some((drag_id, target_container)) = drop_target {
        state.commit_history("Reparent node in hierarchy");
        if state
            .document
            .reparent_node(drag_id, target_container, None)
        {
            state.select_single(drag_id);
            state.status_message = "Moved node into container.".to_owned();
        }
    }
}

/// One insertable item of the Add tab: a `row()` high record with the kind icon.
fn add_row(ui: &mut Ui, row_w: f32, icon: Icon, label: &str) -> bool {
    ui.allocate_ui_with_layout(
        egui::vec2(row_w, neoflash_ui::metrics::row()),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            c::TreeItem::new(label, c::TreeItemType::Symbol(icon))
                .icon(icon)
                .show(ui)
                .row
                .clicked()
        },
    )
    .inner
}

/// Add tab: containers, nested grids and widgets inserted into the selected node.
fn render_add_palette(ui: &mut Ui, state: &mut UiLayoutStudioState, row_w: f32) {
    let target_id = state.primary_selected_id();

    c::menu_section(ui, "Containers");
    if add_row(ui, row_w, i16::SPACE_H, "Row") {
        state.commit_history("Insert Row");
        if let Some(id) = state
            .document
            .insert_node(target_id, "Row", UiNodeKind::Row)
        {
            state.select_single(id);
        }
    }
    if add_row(ui, row_w, i16::SPACE_V, "Column") {
        state.commit_history("Insert Column");
        if let Some(id) = state
            .document
            .insert_node(target_id, "Column", UiNodeKind::Column)
        {
            state.select_single(id);
        }
    }
    if add_row(ui, row_w, i16::FOLDEROPEN, "Section") {
        state.commit_history("Insert Section");
        if let Some(id) = state.document.insert_node(
            target_id,
            "NEW SECTION",
            UiNodeKind::Section {
                collapsible: true,
                expanded: true,
            },
        ) {
            state.select_single(id);
        }
    }
    for (lbl, cols) in [
        (
            "Grid, 2 columns",
            vec![UiSizeUnit::Percent(50.0), UiSizeUnit::Percent(50.0)],
        ),
        (
            "Grid, 3 columns",
            vec![
                UiSizeUnit::Percent(33.3333),
                UiSizeUnit::Percent(33.3333),
                UiSizeUnit::Percent(33.3334),
            ],
        ),
        (
            "Grid, 4 columns",
            vec![
                UiSizeUnit::Percent(25.0),
                UiSizeUnit::Percent(25.0),
                UiSizeUnit::Percent(25.0),
                UiSizeUnit::Percent(25.0),
            ],
        ),
        (
            "Grid, 120 px + 50 % + 50 %",
            vec![
                UiSizeUnit::Px(120.0),
                UiSizeUnit::Percent(50.0),
                UiSizeUnit::Percent(50.0),
            ],
        ),
    ] {
        if add_row(ui, row_w, i16::GRID, lbl) {
            state.commit_history(format!("Insert {lbl}"));
            if let Some(id) = state.document.insert_nested_grid(target_id, cols, true) {
                state.select_single(id);
                state.status_message = format!("Inserted nested grid ({lbl}).");
            }
        }
    }

    c::menu_section(ui, "Widgets");
    for (btn_lbl, node_lbl, kind) in [
        (
            "Field",
            "Numeric Field",
            UiNodeKind::Field {
                value: 100.0,
                prefix: "W".to_owned(),
                unit: "px".to_owned(),
                decimals: 0,
                stepper: false,
            },
        ),
        (
            "Text field",
            "Text Input",
            UiNodeKind::TextField {
                text: "Sample".to_owned(),
                prefix: "ID".to_owned(),
                hint: "Enter text…".to_owned(),
                mono: false,
            },
        ),
        (
            "Select",
            "Dropdown",
            UiNodeKind::Select {
                selected: 0,
                options: vec!["Option A".to_owned(), "Option B".to_owned()],
            },
        ),
        (
            "Button",
            "Action",
            UiNodeKind::Button {
                button_kind: StudioButtonKind::Secondary,
                icon_name: None,
            },
        ),
        (
            "Icon button",
            "Icon Action",
            UiNodeKind::IconButton {
                icon_name: "settings".to_owned(),
                selected: false,
                md_size: false,
            },
        ),
        (
            "Checkbox",
            "Enable option",
            UiNodeKind::Checkbox { checked: true },
        ),
        ("Switch", "Live toggle", UiNodeKind::Switch { on: true }),
        (
            "Swatch",
            "Fill Color",
            UiNodeKind::ColorSwatch {
                rgba: [108, 147, 255, 255],
                is_stroke: false,
            },
        ),
        (
            "Slider",
            "Amount",
            UiNodeKind::Slider {
                value: 50.0,
                min: 0.0,
                max: 100.0,
                show_field: true,
            },
        ),
        ("Label", "Section caption", UiNodeKind::Label),
        (
            "Divider",
            "Divider",
            UiNodeKind::Separator { vertical: false },
        ),
        (
            "Table row",
            "Table Row",
            UiNodeKind::TableRow {
                cells: vec!["Property".to_owned(), "Value".to_owned()],
                column_fractions: vec![0.0, 0.5],
                is_header: false,
                selected: false,
            },
        ),
    ] {
        let icon = icon_for_node_kind(&kind);
        if add_row(ui, row_w, icon, btn_lbl) {
            state.commit_history(format!("Insert {node_lbl}"));
            if let Some(new_id) = state.document.insert_node(target_id, node_lbl, kind) {
                state.select_single(new_id);
                state.status_message = format!("Inserted {node_lbl}.");
            }
        }
    }
}

fn collect_tree_rows(
    node: &UiStudioNode,
    depth: usize,
    out: &mut Vec<(UiNodeId, usize, String, String, Icon, bool)>,
) {
    let w_str = node.box_constraints.width.format_compact();
    let h_str = node.box_constraints.height.format_compact();
    let meta = match &node.kind {
        UiNodeKind::Grid { columns } => format!("{}c · {w_str}", columns.len()),
        _ => format!("{w_str} × {h_str}"),
    };
    out.push((
        node.id,
        depth,
        node.label.clone(),
        meta,
        icon_for_node_kind(&node.kind),
        node.kind.is_container(),
    ));
    for child in &node.children {
        collect_tree_rows(child, depth + 1, out);
    }
}
