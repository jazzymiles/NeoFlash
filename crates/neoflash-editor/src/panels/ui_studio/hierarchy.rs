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

pub(super) fn render_hierarchy_and_palette_column(
    ui: &mut Ui,
    state: &mut UiLayoutStudioState,
    width: f32,
    height: f32,
) {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.set_width(width);

    // Hierarchy Header + Reorder / Duplicate / Delete Toolbar
    ui.horizontal(|ui| {
        ui.label(rich("LAYOUT HIERARCHY", TextStyle::Overline).color(nf.text_secondary));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let sel_id = state.primary_selected_id();
            let is_root = sel_id == state.document.root.id;
            if ui
                .add_enabled(
                    !is_root,
                    c::IconButton::new(i16::TRASH, "Delete selected").side(24.0),
                )
                .clicked()
            {
                let targets = state.selected_nodes.clone();
                state.commit_history("Delete node(s)");
                let count = state.document.delete_nodes(&targets);
                state.select_single(state.document.root.id);
                state.status_message = format!("Deleted {count} node(s).");
            }
            if ui
                .add_enabled(
                    !is_root,
                    c::IconButton::new(i16::COPY, "Duplicate selected").side(24.0),
                )
                .clicked()
            {
                state.commit_history("Duplicate node");
                if let Some(dup_id) = state.document.duplicate_node(sel_id) {
                    state.select_single(dup_id);
                    state.status_message = "Duplicated selected node.".to_owned();
                }
            }
            if ui
                .add_enabled(
                    !is_root,
                    c::IconButton::new(i16::CHEVDOWN, "Move node down").side(24.0),
                )
                .clicked()
            {
                state.commit_history("Move node down");
                state.document.move_node_down(sel_id);
            }
            if ui
                .add_enabled(
                    !is_root,
                    c::IconButton::new(i16::CHEVUP, "Move node up").side(24.0),
                )
                .clicked()
            {
                state.commit_history("Move node up");
                state.document.move_node_up(sel_id);
            }
        });
    });

    let tree_h = (height * 0.54).max(180.0);
    egui::Frame::new()
        .fill(nf.bg_panel)
        .corner_radius(egui::CornerRadius::same(radius::MD as u8))
        .stroke(Stroke::new(1.0_f32, nf.border_subtle))
        .inner_margin(egui::Margin::same(space::S1 as i8))
        .show(ui, |ui| {
            ui.set_width(width - space::S2);
            egui::ScrollArea::vertical()
                .id_salt("studio-hierarchy-scroll")
                .max_height(tree_h)
                .themed()
                .show(ui, |ui| {
                    let mut rows = Vec::new();
                    collect_tree_rows(&state.document.root, 0, &mut rows);
                    let mut clicked_row: Option<(UiNodeId, bool)> = None;
                    let mut drop_target: Option<(UiNodeId, UiNodeId)> = None;

                    for (id, depth, label, meta, kind_icon, is_container) in rows {
                        let is_selected = state.selected_nodes.contains(&id);
                        let item_kind = if is_container {
                            c::TreeItemType::FolderOpen
                        } else {
                            c::TreeItemType::Symbol(kind_icon)
                        };
                        let tree_item = c::TreeItem::new(&label, item_kind)
                            .icon(kind_icon)
                            .depth(depth)
                            .selected(is_selected)
                            .meta(&meta)
                            .show(ui);

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
                });
        });

    ui.add_space(space::S1);
    ui.label(rich("ADD CONTAINER / NESTED GRID", TextStyle::Overline).color(nf.text_secondary));
    let target_id = state.primary_selected_id();

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(space::S1, space::S1);
        if ui.add(c::Button::new("+ Row")).clicked() {
            state.commit_history("Insert Row");
            if let Some(id) = state
                .document
                .insert_node(target_id, "Row", UiNodeKind::Row)
            {
                state.select_single(id);
            }
        }
        if ui.add(c::Button::new("+ Column")).clicked() {
            state.commit_history("Insert Column");
            if let Some(id) = state
                .document
                .insert_node(target_id, "Column", UiNodeKind::Column)
            {
                state.select_single(id);
            }
        }
        if ui.add(c::Button::new("+ Section")).clicked() {
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
                "+ 2-Col Grid",
                vec![UiSizeUnit::Percent(50.0), UiSizeUnit::Percent(50.0)],
            ),
            (
                "+ 3-Col Grid",
                vec![
                    UiSizeUnit::Percent(33.3333),
                    UiSizeUnit::Percent(33.3333),
                    UiSizeUnit::Percent(33.3334),
                ],
            ),
            (
                "+ 4-Col Grid",
                vec![
                    UiSizeUnit::Percent(25.0),
                    UiSizeUnit::Percent(25.0),
                    UiSizeUnit::Percent(25.0),
                    UiSizeUnit::Percent(25.0),
                ],
            ),
            (
                "+ 120px+50%+50%",
                vec![
                    UiSizeUnit::Px(120.0),
                    UiSizeUnit::Percent(50.0),
                    UiSizeUnit::Percent(50.0),
                ],
            ),
        ] {
            if ui.add(c::Button::new(lbl)).clicked() {
                state.commit_history(format!("Insert {lbl}"));
                if let Some(id) = state.document.insert_nested_grid(target_id, cols, true) {
                    state.select_single(id);
                    state.status_message = format!("Inserted nested grid ({lbl}).");
                }
            }
        }
    });

    ui.add_space(space::S1);
    ui.label(rich("ADD NF2 WIDGET", TextStyle::Overline).color(nf.text_secondary));
    egui::ScrollArea::vertical()
        .id_salt("studio-palette-scroll")
        .max_height((height * 0.28).max(96.0))
        .themed()
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(space::S1, space::S1);
                for (btn_lbl, node_lbl, kind) in [
                    (
                        "+ Field",
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
                        "+ TextField",
                        "Text Input",
                        UiNodeKind::TextField {
                            text: "Sample".to_owned(),
                            prefix: "ID".to_owned(),
                            hint: "Enter text…".to_owned(),
                            mono: false,
                        },
                    ),
                    (
                        "+ Select",
                        "Dropdown",
                        UiNodeKind::Select {
                            selected: 0,
                            options: vec!["Option A".to_owned(), "Option B".to_owned()],
                        },
                    ),
                    (
                        "+ Button",
                        "Action",
                        UiNodeKind::Button {
                            button_kind: StudioButtonKind::Secondary,
                            icon_name: None,
                        },
                    ),
                    (
                        "+ IconBtn",
                        "Icon Action",
                        UiNodeKind::IconButton {
                            icon_name: "settings".to_owned(),
                            selected: false,
                            md_size: false,
                        },
                    ),
                    (
                        "+ Checkbox",
                        "Enable option",
                        UiNodeKind::Checkbox { checked: true },
                    ),
                    ("+ Switch", "Live toggle", UiNodeKind::Switch { on: true }),
                    (
                        "+ Swatch",
                        "Fill Color",
                        UiNodeKind::ColorSwatch {
                            rgba: [108, 147, 255, 255],
                            is_stroke: false,
                        },
                    ),
                    (
                        "+ Slider",
                        "Amount",
                        UiNodeKind::Slider {
                            value: 50.0,
                            min: 0.0,
                            max: 100.0,
                            show_field: true,
                        },
                    ),
                    ("+ Label", "Section caption", UiNodeKind::Label),
                    (
                        "+ Divider",
                        "Divider",
                        UiNodeKind::Separator { vertical: false },
                    ),
                    (
                        "+ TableRow",
                        "Table Row",
                        UiNodeKind::TableRow {
                            cells: vec!["Property".to_owned(), "Value".to_owned()],
                            column_fractions: vec![0.0, 0.5],
                            is_header: false,
                            selected: false,
                        },
                    ),
                ] {
                    if ui.add(c::Button::new(btn_lbl)).clicked() {
                        state.commit_history(format!("Insert {node_lbl}"));
                        if let Some(new_id) = state.document.insert_node(target_id, node_lbl, kind)
                        {
                            state.select_single(new_id);
                        }
                    }
                }
            });
        });
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
