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
// 8. Right-Hand Node Inspector (% / px / Paddings / Gaps / Typography) & Systems
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn render_inspector_and_systems_column(
    ui: &mut Ui,
    state: &mut UiLayoutStudioState,
    width: f32,
    height: f32,
) {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.set_width(width);

    // Top Segmented Tab Switcher: [Node Inspector | Systems & Tokens | Code Export]
    let mut tab_idx = match state.inspector_tab {
        StudioInspectorTab::NodeInspector => 0usize,
        StudioInspectorTab::SystemsTab => 1usize,
        StudioInspectorTab::CodeExport => 2usize,
    };
    if c::Segmented::show(
        ui,
        &mut tab_idx,
        &[
            (0usize, "Node (% / px)"),
            (1usize, "Systems"),
            (2usize, "Code Export"),
        ],
        true,
    )
    .is_some()
    {
        state.inspector_tab = match tab_idx {
            0 => StudioInspectorTab::NodeInspector,
            1 => StudioInspectorTab::SystemsTab,
            _ => StudioInspectorTab::CodeExport,
        };
    }

    let panel_h = (height - 42.0).max(240.0);
    egui::Frame::new()
        .fill(nf.bg_panel)
        .corner_radius(egui::CornerRadius::same(radius::MD as u8))
        .stroke(Stroke::new(1.0_f32, nf.border_subtle))
        .inner_margin(egui::Margin::same(space::S3 as i8))
        .show(ui, |ui| {
            let inner_w = (width - space::S3 * 2.0).max(200.0);
            ui.set_width(inner_w);
            egui::ScrollArea::vertical()
                .id_salt(("studio-right-inspector-scroll", tab_idx))
                .max_height(panel_h - space::S3 * 2.0)
                .themed()
                .show(ui, |ui| {
                    ui.set_width(inner_w);
                    ui.spacing_mut().item_spacing.y = space::S2;
                    match state.inspector_tab {
                        StudioInspectorTab::NodeInspector => {
                            render_node_inspector_tab(ui, state, inner_w);
                        }
                        StudioInspectorTab::SystemsTab => {
                            render_systems_inspector_tab(ui, state, inner_w);
                        }
                        StudioInspectorTab::CodeExport => {
                            render_code_export_tab(ui, state, inner_w);
                        }
                    }
                });
        });
}

fn render_size_unit_editor(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + Copy,
    axis_label: &str,
    unit: &mut UiSizeUnit,
    reference_px: f32,
) -> bool {
    let nf = neoflash_ui::palette(ui.ctx());
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::S1;
        ui.label(rich(axis_label, TextStyle::SmallStrong).color(nf.text_secondary));
        let mut mode_idx = match unit {
            UiSizeUnit::Percent(_) => 0usize,
            UiSizeUnit::Px(_) => 1usize,
            UiSizeUnit::Auto => 2usize,
            UiSizeUnit::Fill(_) => 3usize,
        };
        if c::Select::options(
            ui,
            (id_salt, "unit-mode"),
            &mut mode_idx,
            &[
                (0usize, "% (Percent)"),
                (1usize, "px (Points)"),
                (2usize, "Auto"),
                (3usize, "Fill (fr)"),
            ],
            106.0,
        ) {
            *unit = match mode_idx {
                0 => unit.to_percent(reference_px),
                1 => unit.to_px(reference_px),
                2 => UiSizeUnit::Auto,
                _ => UiSizeUnit::Fill(1.0),
            };
            changed = true;
        }

        match unit {
            UiSizeUnit::Percent(pct) => {
                let mut v = f64::from(*pct);
                if ui
                    .add(
                        c::Field::new(&mut v, axis_label)
                            .prefix("%")
                            .unit("%")
                            .range(0.0..=100.0)
                            .decimals(1)
                            .width(104.0),
                    )
                    .changed()
                {
                    *pct = (v as f32).clamp(0.0, 100.0);
                    changed = true;
                }
            }
            UiSizeUnit::Px(px) => {
                let mut v = f64::from(*px);
                if ui
                    .add(
                        c::Field::new(&mut v, axis_label)
                            .prefix("px")
                            .unit("px")
                            .range(0.0..=2400.0)
                            .decimals(0)
                            .width(104.0),
                    )
                    .changed()
                {
                    *px = (v as f32).max(0.0);
                    changed = true;
                }
            }
            UiSizeUnit::Fill(fr) => {
                let mut v = f64::from(*fr);
                if ui
                    .add(
                        c::Field::new(&mut v, axis_label)
                            .prefix("fr")
                            .unit("fr")
                            .range(0.1..=12.0)
                            .decimals(1)
                            .width(104.0),
                    )
                    .changed()
                {
                    *fr = (v as f32).max(0.1);
                    changed = true;
                }
            }
            UiSizeUnit::Auto => {
                ui.add(c::Badge::new("Hug Content", c::BadgeType::Neutral));
            }
        }
    });
    changed
}

fn render_spacing_value_editor(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + Copy,
    label: &str,
    val: &mut SpacingValue,
    systems: &UiStudioSystems,
) -> bool {
    let nf = neoflash_ui::palette(ui.ctx());
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::S1;
        ui.label(rich(label, TextStyle::Small).color(nf.text_secondary));
        let mut choice: i32 = match *val {
            SpacingValue::Token(SpacingToken::S0) => 0,
            SpacingValue::Token(SpacingToken::S1) => 1,
            SpacingValue::Token(SpacingToken::S2) => 2,
            SpacingValue::Token(SpacingToken::S3) => 3,
            SpacingValue::Token(SpacingToken::S4) => 4,
            SpacingValue::Token(SpacingToken::S5) => 5,
            SpacingValue::Token(SpacingToken::S6) => 6,
            SpacingValue::Px(_) => 7,
        };
        if c::Select::options(
            ui,
            (id_salt, "sp-tok"),
            &mut choice,
            &[
                (0, "S0 (0)"),
                (1, "S1"),
                (2, "S2"),
                (3, "S3"),
                (4, "S4"),
                (5, "S5"),
                (6, "S6"),
                (7, "Custom px"),
            ],
            96.0,
        ) {
            *val = match choice {
                0 => SpacingValue::Token(SpacingToken::S0),
                1 => SpacingValue::Token(SpacingToken::S1),
                2 => SpacingValue::Token(SpacingToken::S2),
                3 => SpacingValue::Token(SpacingToken::S3),
                4 => SpacingValue::Token(SpacingToken::S4),
                5 => SpacingValue::Token(SpacingToken::S5),
                6 => SpacingValue::Token(SpacingToken::S6),
                _ => SpacingValue::Px(val.resolve(systems)),
            };
            changed = true;
        }
        let mut px_v = f64::from(val.resolve(systems));
        if ui
            .add(
                c::Field::new(&mut px_v, label)
                    .unit("px")
                    .range(0.0..=128.0)
                    .decimals(0)
                    .width(78.0),
            )
            .changed()
        {
            *val = SpacingValue::Px(px_v as f32);
            changed = true;
        }
    });
    changed
}

fn render_node_inspector_tab(ui: &mut Ui, state: &mut UiLayoutStudioState, _width: f32) {
    let nf = neoflash_ui::palette(ui.ctx());
    let sel_id = state.primary_selected_id();
    let Some(mut node_clone) = state.document.find_node(sel_id).cloned() else {
        return;
    };
    let mut modified = false;

    // 1. Node Header & Identity
    let mut reset_clicked = false;
    ui.horizontal(|ui| {
        ui.add(c::Badge::new(
            node_clone.kind.type_badge(),
            c::BadgeType::Accent,
        ));
        ui.label(rich(format!("Node #{sel_id}"), TextStyle::MonoSmall).color(nf.text_tertiary));
        if ui.add(c::Button::ghost("Reset to System")).clicked() {
            state.commit_history("Reset node to system tokens");
            state.document.reset_node_to_system_defaults(sel_id);
            state.status_message = format!("Reset node #{sel_id} to NF2 system tokens.");
            reset_clicked = true;
        }
    });
    if reset_clicked {
        return;
    }

    if ui
        .add(c::TextField::new(&mut node_clone.label, "Label").prefix("Label"))
        .changed()
    {
        modified = true;
    }

    ui.horizontal(|ui| {
        if ui
            .add(c::Checkbox::new(&mut node_clone.visible, "Visible"))
            .changed()
        {
            modified = true;
        }
        if ui
            .add(c::Checkbox::new(&mut node_clone.enabled, "Enabled"))
            .changed()
        {
            modified = true;
        }
    });

    // 1b. Contextual Live Geometry Inspector when editing Full Workspace Shell or Timeline Studio
    if state.active_preset.is_timeline_surface() {
        render_timeline_geometry_controls(ui, state);
    } else if state.active_preset.is_full_shell() {
        // Show Timeline controls first when a timeline sub-node (6..=14) is selected, otherwise Shell + Timeline
        if sel_id >= 6 {
            render_timeline_geometry_controls(ui, state);
            render_workspace_and_stage_controls(ui, state);
        } else {
            render_workspace_and_stage_controls(ui, state);
            render_timeline_geometry_controls(ui, state);
        }
    }

    // 2. Live Box Sizing (% or px / Auto / Fill)
    c::SectionHeader::new("BOX SIZING (% OR PX)")
        .id_salt(("studio-sec-box", sel_id))
        .show(ui, |ui| {
            let ref_w = state.systems.density.active().inspector_width;
            if render_size_unit_editor(
                ui,
                ("node-w", sel_id),
                "Width ",
                &mut node_clone.box_constraints.width,
                ref_w,
            ) {
                modified = true;
            }
            if render_size_unit_editor(
                ui,
                ("node-h", sel_id),
                "Height",
                &mut node_clone.box_constraints.height,
                state.systems.density.active().control_height,
            ) {
                modified = true;
            }

            // Quick 1-click % / px presets
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = space::S1;
                for (lbl, unit) in [
                    ("25%", UiSizeUnit::Percent(25.0)),
                    ("33.3%", UiSizeUnit::Percent(33.3333)),
                    ("50%", UiSizeUnit::Percent(50.0)),
                    ("100%", UiSizeUnit::Percent(100.0)),
                    ("120px", UiSizeUnit::Px(120.0)),
                    ("320px", UiSizeUnit::Px(320.0)),
                    ("Auto", UiSizeUnit::Auto),
                ] {
                    if ui.add(c::Button::new(lbl)).clicked() {
                        node_clone.box_constraints.width = unit;
                        modified = true;
                    }
                }
            });

            // Min / Max width bounds
            ui.horizontal(|ui| {
                let mut min_w = f64::from(node_clone.box_constraints.min_width.unwrap_or(0.0));
                if ui
                    .add(
                        c::Field::new(&mut min_w, "Min W")
                            .prefix("MinW")
                            .unit("px")
                            .range(0.0..=1200.0)
                            .width(110.0),
                    )
                    .changed()
                {
                    node_clone.box_constraints.min_width = if min_w <= 0.0 {
                        None
                    } else {
                        Some(min_w as f32)
                    };
                    modified = true;
                }
                let mut max_w = f64::from(node_clone.box_constraints.max_width.unwrap_or(0.0));
                if ui
                    .add(
                        c::Field::new(&mut max_w, "Max W")
                            .prefix("MaxW")
                            .unit("px")
                            .range(0.0..=2400.0)
                            .width(110.0),
                    )
                    .changed()
                {
                    node_clone.box_constraints.max_width = if max_w <= 0.0 {
                        None
                    } else {
                        Some(max_w as f32)
                    };
                    modified = true;
                }
            });
        });

    // 3. Grid Column Tracks Editor (if node is a Grid)
    if let UiNodeKind::Grid { columns } = &mut node_clone.kind {
        c::SectionHeader::new("GRID COLUMNS (2, 3, 4+ TRACKS)")
            .id_salt(("studio-sec-grid", sel_id))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = space::S1;
                    if ui.add(c::Button::new("2 Cols (50/50%)")).clicked() {
                        *columns = vec![UiSizeUnit::Percent(50.0), UiSizeUnit::Percent(50.0)];
                        modified = true;
                    }
                    if ui.add(c::Button::new("3 Cols (33.3%)")).clicked() {
                        *columns = vec![
                            UiSizeUnit::Percent(33.3333),
                            UiSizeUnit::Percent(33.3333),
                            UiSizeUnit::Percent(33.3334),
                        ];
                        modified = true;
                    }
                    if ui.add(c::Button::new("4 Cols (25%)")).clicked() {
                        *columns = vec![
                            UiSizeUnit::Percent(25.0),
                            UiSizeUnit::Percent(25.0),
                            UiSizeUnit::Percent(25.0),
                            UiSizeUnit::Percent(25.0),
                        ];
                        modified = true;
                    }
                    if ui.add(c::Button::new("120px + 50% + 50%")).clicked() {
                        *columns = vec![
                            UiSizeUnit::Px(120.0),
                            UiSizeUnit::Percent(50.0),
                            UiSizeUnit::Percent(50.0),
                        ];
                        modified = true;
                    }
                });

                for (col_idx, col_unit) in columns.iter_mut().enumerate() {
                    if render_size_unit_editor(
                        ui,
                        ("grid-col", sel_id, col_idx),
                        &format!("Col {}", col_idx + 1),
                        col_unit,
                        280.0,
                    ) {
                        modified = true;
                    }
                }

                ui.horizontal(|ui| {
                    if ui.add(c::Button::new("+ Add Col")).clicked() {
                        columns.push(UiSizeUnit::Percent(25.0));
                        modified = true;
                    }
                    if columns.len() > 1 && ui.add(c::Button::new("- Remove Col")).clicked() {
                        columns.pop();
                        modified = true;
                    }
                });
            });
    }

    // 4. Paddings, Gaps & System Binding
    c::SectionHeader::new("PADDING, GAP & SYSTEM BINDING")
        .id_salt(("studio-sec-pad", sel_id))
        .show(ui, |ui| {
            let mut sys_choice: usize = match node_clone.style.system_binding {
                None => 0,
                Some(SharedSystemKind::TwoColumnForm) => 1,
                Some(SharedSystemKind::SectionHeader) => 2,
                Some(SharedSystemKind::PanelHeader) => 3,
                Some(SharedSystemKind::ToolRail) => 4,
            };
            if c::Select::options(
                ui,
                ("node-sys-bind", sel_id),
                &mut sys_choice,
                &[
                    (0, "System Binding: None (Local)"),
                    (1, "System: Two-Column Form"),
                    (2, "System: Section Header"),
                    (3, "System: Panel Header"),
                    (4, "System: Tool Rail"),
                ],
                220.0,
            ) {
                node_clone.style.system_binding = match sys_choice {
                    1 => Some(SharedSystemKind::TwoColumnForm),
                    2 => Some(SharedSystemKind::SectionHeader),
                    3 => Some(SharedSystemKind::PanelHeader),
                    4 => Some(SharedSystemKind::ToolRail),
                    _ => None,
                };
                modified = true;
            }

            if render_spacing_value_editor(
                ui,
                ("gap-x", sel_id),
                "Gap X ",
                &mut node_clone.style.gap.x,
                &state.systems,
            ) {
                modified = true;
            }
            if render_spacing_value_editor(
                ui,
                ("gap-y", sel_id),
                "Gap Y ",
                &mut node_clone.style.gap.y,
                &state.systems,
            ) {
                modified = true;
            }

            ui.horizontal_wrapped(|ui| {
                ui.label(rich("Uniform Pad:", TextStyle::Small).color(nf.text_secondary));
                for tok in [
                    SpacingToken::S0,
                    SpacingToken::S1,
                    SpacingToken::S2,
                    SpacingToken::S3,
                    SpacingToken::S4,
                ] {
                    if ui.add(c::Button::new(tok.short_name())).clicked() {
                        node_clone.style.padding = Padding::all_token(tok);
                        modified = true;
                    }
                }
            });

            if render_spacing_value_editor(
                ui,
                ("pad-t", sel_id),
                "Pad Top   ",
                &mut node_clone.style.padding.top,
                &state.systems,
            ) {
                modified = true;
            }
            if render_spacing_value_editor(
                ui,
                ("pad-r", sel_id),
                "Pad Right ",
                &mut node_clone.style.padding.right,
                &state.systems,
            ) {
                modified = true;
            }
            if render_spacing_value_editor(
                ui,
                ("pad-b", sel_id),
                "Pad Bottom",
                &mut node_clone.style.padding.bottom,
                &state.systems,
            ) {
                modified = true;
            }
            if render_spacing_value_editor(
                ui,
                ("pad-l", sel_id),
                "Pad Left  ",
                &mut node_clone.style.padding.left,
                &state.systems,
            ) {
                modified = true;
            }
        });

    // 5. Typography & Surface Styling
    c::SectionHeader::new("TYPOGRAPHY & SURFACE TOKENS")
        .id_salt(("studio-sec-typo", sel_id))
        .show(ui, |ui| {
            let mut is_custom = node_clone.style.typography.is_custom();
            if ui
                .add(c::Switch::new(&mut is_custom, "Custom Typography Override"))
                .changed()
            {
                if is_custom {
                    let _ = node_clone
                        .style
                        .typography
                        .promote_to_custom(&state.systems);
                } else {
                    node_clone
                        .style
                        .typography
                        .reset_to_system(node_clone.kind.default_text_style());
                }
                modified = true;
            }

            match &mut node_clone.style.typography {
                TypographyBinding::System(style_tok) => {
                    let opts: Vec<(StudioTextStyle, &str)> = StudioTextStyle::ALL
                        .into_iter()
                        .map(|s| (s, s.label()))
                        .collect();
                    if c::Select::options(ui, ("node-sys-typo", sel_id), style_tok, &opts, 210.0) {
                        modified = true;
                    }
                }
                TypographyBinding::Custom(ov) => {
                    if c::Select::options(
                        ui,
                        ("node-cust-fam", sel_id),
                        &mut ov.font_family,
                        &[
                            (StudioFontFamily::Inter, "Inter (UI)"),
                            (StudioFontFamily::RobotoMono, "Roboto Mono"),
                        ],
                        180.0,
                    ) {
                        modified = true;
                    }
                    ui.horizontal(|ui| {
                        let mut sz = f64::from(ov.size_pt);
                        if ui
                            .add(
                                c::Field::new(&mut sz, "Font Size")
                                    .prefix("Sz")
                                    .unit("pt")
                                    .range(8.0..=48.0)
                                    .decimals(1)
                                    .width(96.0),
                            )
                            .changed()
                        {
                            ov.size_pt = sz as f32;
                            modified = true;
                        }
                        let mut lh = f64::from(ov.line_height_pt);
                        if ui
                            .add(
                                c::Field::new(&mut lh, "Line Height")
                                    .prefix("LH")
                                    .unit("pt")
                                    .range(8.0..=64.0)
                                    .decimals(1)
                                    .width(96.0),
                            )
                            .changed()
                        {
                            ov.line_height_pt = lh as f32;
                            modified = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        if c::Select::options(
                            ui,
                            ("node-cust-wt", sel_id),
                            &mut ov.weight,
                            &[
                                (400, "Regular (400)"),
                                (500, "Medium (500)"),
                                (600, "SemiBold (600)"),
                            ],
                            132.0,
                        ) {
                            modified = true;
                        }
                        if c::Select::options(
                            ui,
                            ("node-cust-align", sel_id),
                            &mut ov.align,
                            &[
                                (StudioTextAlign::Left, "Left"),
                                (StudioTextAlign::Center, "Center"),
                                (StudioTextAlign::Right, "Right"),
                            ],
                            92.0,
                        ) {
                            modified = true;
                        }
                    });
                    let role_opts: Vec<(StudioColorRole, &str)> = StudioColorRole::ALL
                        .into_iter()
                        .map(|r| (r, r.label()))
                        .collect();
                    if c::Select::options(
                        ui,
                        ("node-cust-col", sel_id),
                        &mut ov.color_token,
                        &role_opts,
                        210.0,
                    ) {
                        modified = true;
                    }
                }
            }
        });

    if modified {
        state.commit_history(format!("Edit node #{sel_id}"));
        if let Some(target) = state.document.find_node_mut(sel_id) {
            *target = node_clone;
        }
    }
}

fn render_timeline_geometry_controls(ui: &mut Ui, state: &mut UiLayoutStudioState) {
    c::SectionHeader::new("TWEEN ARROWS (СТРЕЛКИ ТВИНОВ)")
        .id_salt("sys-sec-tween-arrows")
        .show(ui, |ui| {
            let tg = &mut state.systems.timeline_geometry;
            let mut head_len = f64::from(tg.tween_arrow_head_length_px);
            if ui
                .add(
                    c::Field::new(&mut head_len, "Arrow Head Length (Длина наконечника)")
                        .prefix("Head L")
                        .unit("px")
                        .range(2.0..=28.0)
                        .decimals(1)
                        .stepper()
                        .width(176.0),
                )
                .changed()
            {
                tg.tween_arrow_head_length_px = head_len as f32;
            }

            let mut wing = f64::from(tg.tween_arrow_head_wing_px);
            if ui
                .add(
                    c::Field::new(&mut wing, "Arrow Wing (Размах крыльев)")
                        .prefix("Wing")
                        .unit("px")
                        .range(1.0..=16.0)
                        .decimals(1)
                        .stepper()
                        .width(176.0),
                )
                .changed()
            {
                tg.tween_arrow_head_wing_px = wing as f32;
            }

            ui.horizontal(|ui| {
                let mut start_in = f64::from(tg.tween_arrow_start_inset_px);
                if ui
                    .add(
                        c::Field::new(&mut start_in, "Start Inset (Отступ начала)")
                            .prefix("In L")
                            .unit("px")
                            .range(0.0..=40.0)
                            .decimals(1)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.tween_arrow_start_inset_px = start_in as f32;
                }
                let mut end_in = f64::from(tg.tween_arrow_end_inset_px);
                if ui
                    .add(
                        c::Field::new(&mut end_in, "End Inset (Отступ конца)")
                            .prefix("In R")
                            .unit("px")
                            .range(0.0..=40.0)
                            .decimals(1)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.tween_arrow_end_inset_px = end_in as f32;
                }
            });

            ui.horizontal(|ui| {
                let mut stroke_w = f64::from(tg.tween_arrow_stroke_width_px);
                if ui
                    .add(
                        c::Field::new(&mut stroke_w, "Arrow Stroke (Толщина линии)")
                            .prefix("Stroke")
                            .unit("px")
                            .range(0.5..=5.0)
                            .decimals(2)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.tween_arrow_stroke_width_px = stroke_w as f32;
                }
                let mut y_pct = f64::from(tg.tween_arrow_y_pct);
                if ui
                    .add(
                        c::Field::new(&mut y_pct, "Arrow Y Position (%)")
                            .prefix("Y Pos")
                            .unit("%")
                            .range(10.0..=90.0)
                            .decimals(0)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.tween_arrow_y_pct = y_pct as f32;
                }
            });

            ui.horizontal(|ui| {
                let mut dash = f64::from(tg.broken_tween_dash_px);
                if ui
                    .add(
                        c::Field::new(&mut dash, "Broken Dash")
                            .prefix("Dash")
                            .unit("px")
                            .range(1.0..=16.0)
                            .decimals(1)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.broken_tween_dash_px = dash as f32;
                }
                let mut gap = f64::from(tg.broken_tween_gap_px);
                if ui
                    .add(
                        c::Field::new(&mut gap, "Broken Gap")
                            .prefix("Gap")
                            .unit("px")
                            .range(1.0..=16.0)
                            .decimals(1)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.broken_tween_gap_px = gap as f32;
                }
            });
        });

    c::SectionHeader::new("TIMELINE CELLS, KEYFRAMES & BADGES")
        .id_salt("sys-sec-tl-cells")
        .show(ui, |ui| {
            let tg = &mut state.systems.timeline_geometry;
            ui.horizontal(|ui| {
                let mut cw = f64::from(tg.frame_cell_width_px);
                if ui
                    .add(
                        c::Field::new(&mut cw, "Frame Cell Width")
                            .prefix("Cell W")
                            .unit("px")
                            .range(6.0..=32.0)
                            .decimals(1)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.frame_cell_width_px = cw as f32;
                }
                let mut rh = f64::from(tg.layer_row_height_px);
                if ui
                    .add(
                        c::Field::new(&mut rh, "Layer Row Height")
                            .prefix("Row H")
                            .unit("px")
                            .range(14.0..=48.0)
                            .decimals(0)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.layer_row_height_px = rh as f32;
                }
            });

            ui.horizontal(|ui| {
                let mut dot_r = f64::from(tg.keyframe_dot_radius_px);
                if ui
                    .add(
                        c::Field::new(&mut dot_r, "Keyframe Dot Radius (●/○)")
                            .prefix("Dot R")
                            .unit("px")
                            .range(1.5..=8.0)
                            .decimals(1)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.keyframe_dot_radius_px = dot_r as f32;
                }
                let mut dia_r = f64::from(tg.motion_diamond_radius_px);
                if ui
                    .add(
                        c::Field::new(&mut dia_r, "Motion Diamond Radius (◆)")
                            .prefix("Dia R")
                            .unit("px")
                            .range(1.5..=9.0)
                            .decimals(1)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.motion_diamond_radius_px = dia_r as f32;
                }
            });

            ui.horizontal(|ui| {
                let mut end_w = f64::from(tg.span_end_rect_w_px);
                if ui
                    .add(
                        c::Field::new(&mut end_w, "Span End Rect W ([ ])")
                            .prefix("End W")
                            .unit("px")
                            .range(2.0..=12.0)
                            .decimals(1)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.span_end_rect_w_px = end_w as f32;
                }
                let mut end_h = f64::from(tg.span_end_rect_h_px);
                if ui
                    .add(
                        c::Field::new(&mut end_h, "Span End Rect H ([ ])")
                            .prefix("End H")
                            .unit("px")
                            .range(3.0..=16.0)
                            .decimals(1)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.span_end_rect_h_px = end_h as f32;
                }
            });

            ui.horizontal(|ui| {
                let mut act_sz = f64::from(tg.action_marker_size_px);
                if ui
                    .add(
                        c::Field::new(&mut act_sz, "Action Marker 'a' Size")
                            .prefix("Act a")
                            .unit("px")
                            .range(5.0..=14.0)
                            .decimals(1)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.action_marker_size_px = act_sz as f32;
                }
                let mut flag_sz = f64::from(tg.label_flag_size_px);
                if ui
                    .add(
                        c::Field::new(&mut flag_sz, "Label Flag Size")
                            .prefix("Flag")
                            .unit("px")
                            .range(3.0..=12.0)
                            .decimals(1)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.label_flag_size_px = flag_sz as f32;
                }
            });
        });

    c::SectionHeader::new("PLAYHEAD, RULER & ONION SKIN")
        .id_salt("sys-sec-playhead")
        .show(ui, |ui| {
            let tg = &mut state.systems.timeline_geometry;
            ui.horizontal(|ui| {
                let mut ph_f = tg.playhead_frame as f64;
                if ui
                    .add(
                        c::Field::new(&mut ph_f, "Playhead Frame")
                            .prefix("Frame")
                            .range(1.0..=60.0)
                            .decimals(0)
                            .stepper()
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.playhead_frame = (ph_f.round() as usize).clamp(1, 60);
                }
                let mut rul_h = f64::from(tg.ruler_height_px);
                if ui
                    .add(
                        c::Field::new(&mut rul_h, "Ruler Height")
                            .prefix("Ruler H")
                            .unit("px")
                            .range(14.0..=40.0)
                            .decimals(0)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.ruler_height_px = rul_h as f32;
                }
            });

            ui.horizontal(|ui| {
                let mut cap_w = f64::from(tg.playhead_cap_width_px);
                if ui
                    .add(
                        c::Field::new(&mut cap_w, "Playhead Cap W")
                            .prefix("Cap W")
                            .unit("px")
                            .range(6.0..=24.0)
                            .decimals(1)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.playhead_cap_width_px = cap_w as f32;
                }
                let mut cap_h = f64::from(tg.playhead_cap_height_px);
                if ui
                    .add(
                        c::Field::new(&mut cap_h, "Playhead Cap H")
                            .prefix("Cap H")
                            .unit("px")
                            .range(6.0..=24.0)
                            .decimals(1)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.playhead_cap_height_px = cap_h as f32;
                }
            });

            ui.horizontal(|ui| {
                let mut ph_lw = f64::from(tg.playhead_line_width_px);
                if ui
                    .add(
                        c::Field::new(&mut ph_lw, "Playhead Line Width")
                            .prefix("Line W")
                            .unit("px")
                            .range(0.5..=4.0)
                            .decimals(2)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.playhead_line_width_px = ph_lw as f32;
                }
                let mut ob_w = f64::from(tg.onion_bracket_width_px);
                if ui
                    .add(
                        c::Field::new(&mut ob_w, "Onion Bracket Arm W")
                            .prefix("Onion W")
                            .unit("px")
                            .range(2.0..=14.0)
                            .decimals(1)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.onion_bracket_width_px = ob_w as f32;
                }
            });
        });

    c::SectionHeader::new("LAYER TREE COLUMN (% OR PX)")
        .id_salt("sys-sec-layer-col")
        .show(ui, |ui| {
            let _ = render_size_unit_editor(
                ui,
                "tl-layer-col-w",
                "Layer Col W",
                &mut state.systems.timeline_geometry.layer_list_width,
                760.0,
            );
            let tg = &mut state.systems.timeline_geometry;
            ui.horizontal(|ui| {
                let mut ind = f64::from(tg.layer_indent_px);
                if ui
                    .add(
                        c::Field::new(&mut ind, "Folder Indent")
                            .prefix("Indent")
                            .unit("px")
                            .range(6.0..=32.0)
                            .decimals(0)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.layer_indent_px = ind as f32;
                }
                let mut igap = f64::from(tg.layer_icon_gap_px);
                if ui
                    .add(
                        c::Field::new(&mut igap, "Icon Gap")
                            .prefix("IcoGap")
                            .unit("px")
                            .range(2.0..=16.0)
                            .decimals(1)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.layer_icon_gap_px = igap as f32;
                }
            });
        });
}

fn render_workspace_and_stage_controls(ui: &mut Ui, state: &mut UiLayoutStudioState) {
    c::SectionHeader::new("WORKSPACE DOCK PROPORTIONS (% OR PX)")
        .id_salt("sys-sec-shell-docks")
        .show(ui, |ui| {
            let _ = render_size_unit_editor(
                ui,
                "shell-left-tools-w",
                "Left Tools ",
                &mut state.systems.workspace_shell.shell_left_tools_width,
                820.0,
            );
            let _ = render_size_unit_editor(
                ui,
                "shell-right-insp-w",
                "Right Dock ",
                &mut state.systems.workspace_shell.shell_right_inspector_width,
                820.0,
            );
            let _ = render_size_unit_editor(
                ui,
                "shell-bot-tl-h",
                "Timeline H ",
                &mut state.systems.workspace_shell.shell_bottom_timeline_height,
                560.0,
            );

            let ws = &mut state.systems.workspace_shell;
            ui.horizontal(|ui| {
                let mut mh = f64::from(ws.top_menubar_height_px);
                if ui
                    .add(
                        c::Field::new(&mut mh, "Menu Bar H")
                            .prefix("Menu H")
                            .unit("px")
                            .range(18.0..=44.0)
                            .decimals(0)
                            .width(116.0),
                    )
                    .changed()
                {
                    ws.top_menubar_height_px = mh as f32;
                }
                let mut th = f64::from(ws.doc_tabs_height_px);
                if ui
                    .add(
                        c::Field::new(&mut th, "Doc Tabs H")
                            .prefix("Tabs H")
                            .unit("px")
                            .range(20.0..=44.0)
                            .decimals(0)
                            .width(116.0),
                    )
                    .changed()
                {
                    ws.doc_tabs_height_px = th as f32;
                }
            });
        });

    c::SectionHeader::new("STAGE RULERS, GUIDES & HANDLES")
        .id_salt("sys-sec-stage-overlay")
        .show(ui, |ui| {
            let stg = &mut state.systems.stage_overlay;
            ui.horizontal(|ui| {
                ui.add(c::Checkbox::new(&mut stg.show_rulers, "Show Rulers"));
                ui.add(c::Checkbox::new(&mut stg.show_guides, "Show Cyan Guides"));
            });
            ui.horizontal(|ui| {
                let mut rt = f64::from(stg.stage_ruler_thickness_px);
                if ui
                    .add(
                        c::Field::new(&mut rt, "Stage Ruler Thickness")
                            .prefix("Ruler")
                            .unit("px")
                            .range(12.0..=32.0)
                            .decimals(0)
                            .width(116.0),
                    )
                    .changed()
                {
                    stg.stage_ruler_thickness_px = rt as f32;
                }
                let mut hs = f64::from(stg.selection_handle_size_px);
                if ui
                    .add(
                        c::Field::new(&mut hs, "Selection Handle Size")
                            .prefix("Handle")
                            .unit("px")
                            .range(3.0..=12.0)
                            .decimals(1)
                            .width(116.0),
                    )
                    .changed()
                {
                    stg.selection_handle_size_px = hs as f32;
                }
            });
        });
}

fn render_systems_inspector_tab(ui: &mut Ui, state: &mut UiLayoutStudioState, _width: f32) {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = space::S1;
        for (sub, lbl) in [
            (SystemsSubTab::TimelineGeometry, "Timeline & Arrows"),
            (SystemsSubTab::WorkspaceAndStage, "Shell & Stage"),
            (SystemsSubTab::SpacingAndDensity, "Spacing & Density"),
            (SystemsSubTab::TypographyScale, "Typography"),
            (SystemsSubTab::SharedLayouts, "Shared Systems"),
            (SystemsSubTab::PaletteTokens, "Palette"),
        ] {
            let active = state.systems_subtab == sub;
            let btn = if active {
                c::Button::primary(lbl)
            } else {
                c::Button::new(lbl)
            };
            if ui.add(btn).clicked() {
                state.systems_subtab = sub;
            }
        }
    });

    let mut reset_systems_clicked = false;
    ui.horizontal(|ui| {
        ui.label(
            rich(
                "Editing a system updates all bound widgets across the canvas.",
                TextStyle::Caption,
            )
            .color(nf.text_secondary),
        );
        if ui.add(c::Button::ghost("Reset All Systems")).clicked() {
            state.commit_history("Reset all NF2 systems to defaults");
            state.systems.reset_all_to_defaults();
            state.status_message = "Reset all NF2 system scales to defaults.".to_owned();
            reset_systems_clicked = true;
        }
    });
    if reset_systems_clicked {
        return;
    }

    match state.systems_subtab {
        SystemsSubTab::TimelineGeometry => {
            render_timeline_geometry_controls(ui, state);
        }
        SystemsSubTab::WorkspaceAndStage => {
            render_workspace_and_stage_controls(ui, state);
        }
        SystemsSubTab::SpacingAndDensity => {
            c::SectionHeader::new("SPACING SCALE (S1..S6 IN PT)")
                .id_salt("sys-sec-spacing")
                .show(ui, |ui| {
                    for tok in [
                        SpacingToken::S1,
                        SpacingToken::S2,
                        SpacingToken::S3,
                        SpacingToken::S4,
                        SpacingToken::S5,
                        SpacingToken::S6,
                    ] {
                        let mut v = f64::from(state.systems.spacing.resolve(tok));
                        ui.horizontal(|ui| {
                            ui.label(
                                rich(tok.short_name(), TextStyle::MonoSmall)
                                    .color(nf.text_secondary),
                            );
                            if ui
                                .add(
                                    c::Field::new(&mut v, tok.short_name())
                                        .prefix(tok.short_name())
                                        .unit("pt")
                                        .range(0.0..=96.0)
                                        .decimals(0)
                                        .stepper()
                                        .width(128.0),
                                )
                                .changed()
                            {
                                state.systems.spacing.set(tok, v as f32);
                            }
                        });
                    }
                });

            c::SectionHeader::new("DENSITY PROFILE METRICS")
                .id_salt("sys-sec-density")
                .show(ui, |ui| {
                    let mut prof = state.systems.density.active_profile;
                    if c::Select::options(
                        ui,
                        "sys-density-prof",
                        &mut prof,
                        &[
                            (StudioDensityProfile::Comfortable, "Profile: Comfortable"),
                            (StudioDensityProfile::Compact, "Profile: Compact"),
                            (StudioDensityProfile::ClassicFlash, "Profile: Classic Flash"),
                        ],
                        210.0,
                    ) {
                        state.systems.density.active_profile = prof;
                    }

                    let metrics = state.systems.density.active_mut();
                    let mut ctrl_h = f64::from(metrics.control_height);
                    if ui
                        .add(
                            c::Field::new(&mut ctrl_h, "Control Height")
                                .prefix("Ctrl H")
                                .unit("pt")
                                .range(18.0..=48.0)
                                .decimals(0)
                                .width(160.0),
                        )
                        .changed()
                    {
                        metrics.control_height = ctrl_h as f32;
                    }

                    let mut row_h = f64::from(metrics.row_height);
                    if ui
                        .add(
                            c::Field::new(&mut row_h, "Row Height")
                                .prefix("Row H")
                                .unit("pt")
                                .range(16.0..=44.0)
                                .decimals(0)
                                .width(160.0),
                        )
                        .changed()
                    {
                        metrics.row_height = row_h as f32;
                    }

                    let mut layer_h = f64::from(metrics.layer_row_height);
                    if ui
                        .add(
                            c::Field::new(&mut layer_h, "Layer Row Height")
                                .prefix("Layer H")
                                .unit("pt")
                                .range(12.0..=44.0)
                                .decimals(0)
                                .width(160.0),
                        )
                        .changed()
                    {
                        metrics.layer_row_height = layer_h as f32;
                    }

                    let mut icon_g = f64::from(metrics.icon_grid_pt);
                    if ui
                        .add(
                            c::Field::new(&mut icon_g, "Icon Grid")
                                .prefix("Icon")
                                .unit("pt")
                                .range(12.0..=24.0)
                                .decimals(0)
                                .width(160.0),
                        )
                        .changed()
                    {
                        metrics.icon_grid_pt = icon_g as f32;
                    }
                });
        }
        SystemsSubTab::TypographyScale => {
            c::SectionHeader::new("NF2 TYPOGRAPHY SCALE")
                .id_salt("sys-sec-typo-scale")
                .show(ui, |ui| {
                    for style in [
                        StudioTextStyle::Title,
                        StudioTextStyle::Body,
                        StudioTextStyle::BodyStrong,
                        StudioTextStyle::Small,
                        StudioTextStyle::Caption,
                        StudioTextStyle::Overline,
                        StudioTextStyle::Mono,
                        StudioTextStyle::Heading,
                    ] {
                        let entry = state.systems.typography.entry_mut(style);
                        ui.label(
                            rich(style.label(), TextStyle::SmallStrong).color(nf.text_primary),
                        );
                        ui.horizontal(|ui| {
                            let mut sz = f64::from(entry.size_pt);
                            if ui
                                .add(
                                    c::Field::new(&mut sz, "Size")
                                        .prefix("Sz")
                                        .unit("pt")
                                        .range(8.0..=36.0)
                                        .decimals(1)
                                        .width(92.0),
                                )
                                .changed()
                            {
                                entry.size_pt = sz as f32;
                            }
                            let mut lh = f64::from(entry.line_height_pt);
                            if ui
                                .add(
                                    c::Field::new(&mut lh, "Line Height")
                                        .prefix("LH")
                                        .unit("pt")
                                        .range(8.0..=48.0)
                                        .decimals(1)
                                        .width(92.0),
                                )
                                .changed()
                            {
                                entry.line_height_pt = lh as f32;
                            }
                        });
                    }
                });
        }
        SystemsSubTab::SharedLayouts => {
            c::SectionHeader::new("TWO-COLUMN FORM SYSTEM")
                .id_salt("sys-sec-2col")
                .show(ui, |ui| {
                    let mut min_cell = f64::from(state.systems.two_column_form.min_cell_width);
                    if ui
                        .add(
                            c::Field::new(&mut min_cell, "Min Cell W")
                                .prefix("MinW")
                                .unit("px")
                                .range(48.0..=240.0)
                                .width(150.0),
                        )
                        .changed()
                    {
                        state.systems.two_column_form.min_cell_width = min_cell as f32;
                    }
                    let mut lbl_w = f64::from(state.systems.two_column_form.label_width_px);
                    if ui
                        .add(
                            c::Field::new(&mut lbl_w, "Label Width")
                                .prefix("LblW")
                                .unit("px")
                                .range(40.0..=180.0)
                                .width(150.0),
                        )
                        .changed()
                    {
                        state.systems.two_column_form.label_width_px = lbl_w as f32;
                    }
                    let mut gap_px =
                        f64::from(state.systems.two_column_form.gap.resolve(&state.systems));
                    if ui
                        .add(
                            c::Field::new(&mut gap_px, "Form Gap")
                                .prefix("Gap")
                                .unit("px")
                                .range(0.0..=48.0)
                                .width(150.0),
                        )
                        .changed()
                    {
                        state.systems.two_column_form.gap = SpacingValue::Px(gap_px as f32);
                    }
                });

            c::SectionHeader::new("SECTION & PANEL HEADER SYSTEMS")
                .id_salt("sys-sec-headers")
                .show(ui, |ui| {
                    let mut sec_h = f64::from(state.systems.section_header.height);
                    if ui
                        .add(
                            c::Field::new(&mut sec_h, "Section Header H")
                                .prefix("SecH")
                                .unit("pt")
                                .range(20.0..=56.0)
                                .width(150.0),
                        )
                        .changed()
                    {
                        state.systems.section_header.height = sec_h as f32;
                    }
                    let mut pnl_h = f64::from(state.systems.panel_header.height);
                    if ui
                        .add(
                            c::Field::new(&mut pnl_h, "Panel Header H")
                                .prefix("PnlH")
                                .unit("pt")
                                .range(24.0..=64.0)
                                .width(150.0),
                        )
                        .changed()
                    {
                        state.systems.panel_header.height = pnl_h as f32;
                    }
                });

            c::SectionHeader::new("TOOL RAIL SYSTEM")
                .id_salt("sys-sec-toolrail")
                .show(ui, |ui| {
                    let mut cols = state.systems.tool_rail.columns as f64;
                    if ui
                        .add(
                            c::Field::new(&mut cols, "Tool Columns")
                                .prefix("Cols")
                                .range(1.0..=4.0)
                                .stepper()
                                .width(150.0),
                        )
                        .changed()
                    {
                        state.systems.tool_rail.columns = (cols.round() as usize).clamp(1, 4);
                    }
                    let mut side = f64::from(state.systems.tool_rail.button_side);
                    if ui
                        .add(
                            c::Field::new(&mut side, "Tool Button Side")
                                .prefix("Side")
                                .unit("pt")
                                .range(20.0..=48.0)
                                .width(150.0),
                        )
                        .changed()
                    {
                        state.systems.tool_rail.button_side = side as f32;
                    }
                });
        }
        SystemsSubTab::PaletteTokens => {
            let mut theme = state.preview_theme;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = space::S1;
                for candidate in ThemeMode::ALL {
                    let btn = if candidate == theme {
                        c::Button::primary(candidate.label())
                    } else {
                        c::Button::new(candidate.label())
                    };
                    if ui.add(btn).clicked() {
                        theme = candidate;
                        state.set_preview_theme(candidate);
                    }
                }
            });
            ui.add_space(space::S1);
            let heading = match theme {
                ThemeMode::Dark => "DARK PALETTE TOKEN OVERRIDES",
                ThemeMode::Light => "LIGHT PALETTE TOKEN OVERRIDES",
                ThemeMode::MidNeutralGray => "MID NEUTRAL GRAY PALETTE TOKEN OVERRIDES",
                ThemeMode::DarkGraphite => "DARK GRAPHITE PALETTE TOKEN OVERRIDES",
            };
            c::SectionHeader::new(heading)
                .id_salt("sys-sec-palette")
                .show(ui, |ui| {
                    for role in [
                        StudioColorRole::BgApp,
                        StudioColorRole::BgPanel,
                        StudioColorRole::BgRaised,
                        StudioColorRole::BgField,
                        StudioColorRole::TextPrimary,
                        StudioColorRole::TextSecondary,
                        StudioColorRole::AccentDefault,
                        StudioColorRole::BorderDefault,
                        StudioColorRole::BorderSubtle,
                    ] {
                        let mut col = state.systems.palette_overrides.resolve_color(theme, role);
                        ui.horizontal(|ui| {
                            ui.label(
                                rich(role.label(), TextStyle::MonoSmall).color(nf.text_secondary),
                            );
                            let resp = c::ColorField::new(&mut col, role.label())
                                .width(112.0)
                                .show(ui);
                            if resp.response.changed() {
                                state
                                    .systems
                                    .palette_overrides
                                    .set_color_override(theme, role, col);
                            }
                            // 4-theme swatch strip (Dark · Light · Mid Neutral Gray · Dark Graphite)
                            for swatch_theme in ThemeMode::ALL {
                                let sw_col = state
                                    .systems
                                    .palette_overrides
                                    .resolve_color(swatch_theme, role);
                                let (sw_rect, sw_resp) =
                                    ui.allocate_exact_size(egui::vec2(12.0, 12.0), Sense::click());
                                ui.painter().rect_filled(sw_rect, 2.0, sw_col);
                                ui.painter().rect_stroke(
                                    sw_rect,
                                    2.0,
                                    Stroke::new(
                                        if swatch_theme == theme {
                                            1.2_f32
                                        } else {
                                            0.6_f32
                                        },
                                        if swatch_theme == theme {
                                            nf.accent_default
                                        } else {
                                            nf.border_subtle
                                        },
                                    ),
                                    StrokeKind::Inside,
                                );
                                if sw_resp.on_hover_text(swatch_theme.label()).clicked() {
                                    state.set_preview_theme(swatch_theme);
                                }
                            }
                            if state.systems.palette_overrides.has_override(theme, role)
                                && ui.add(c::Button::ghost("Reset")).clicked()
                            {
                                state
                                    .systems
                                    .palette_overrides
                                    .reset_color_override(theme, role);
                            }
                        });
                    }
                });
        }
    }
}

fn render_code_export_tab(ui: &mut Ui, state: &mut UiLayoutStudioState, width: f32) {
    let nf = neoflash_ui::palette(ui.ctx());
    let code = export_rust_layout_with_systems(&state.document, &state.systems);
    let svg_code = state.export_active_svg();

    ui.horizontal_wrapped(|ui| {
        if ui
            .add(c::Button::new("Copy Rust Code").icon(i16::COPY))
            .clicked()
        {
            ui.ctx().copy_text(code.clone());
            state.status_message = "Copied generated Rust NF2 code to clipboard.".to_owned();
        }
        if ui
            .add(c::Button::new("Copy SVG XML").icon(i16::COPY))
            .clicked()
        {
            ui.ctx().copy_text(svg_code.clone());
            state.svg_buffer = svg_code.clone();
            state.status_message = "Copied active artboard SVG XML to clipboard.".to_owned();
        }
        if ui
            .add(c::Button::new("Import SVG → Panel…").icon(i16::UPLOAD))
            .clicked()
        {
            state.svg_import_modal_open = true;
        }
        if ui
            .add(c::Button::primary("Write All to target/ui-studio/").icon(i16::SAVE))
            .clicked()
        {
            let _ = state.save_studio_json(Path::new(DEFAULT_STUDIO_JSON_PATH));
            let _ = state.save_svg_to_file(Path::new(DEFAULT_STUDIO_SVG_EXPORT_PATH));
            if let Ok(path) = state.export_rust_to_file(Path::new(DEFAULT_STUDIO_RUST_EXPORT_PATH))
            {
                state.status_message = format!(
                    "Wrote {}, {}, and {DEFAULT_STUDIO_SVG_EXPORT_PATH}",
                    DEFAULT_STUDIO_JSON_PATH,
                    path.display()
                );
            }
        }
    });

    ui.label(
        rich(
            format!(
                "JSON: {DEFAULT_STUDIO_JSON_PATH}\nRust: {DEFAULT_STUDIO_RUST_EXPORT_PATH}\nSVG:  {DEFAULT_STUDIO_SVG_EXPORT_PATH}"
            ),
            TextStyle::MonoSmall,
        )
        .color(nf.text_tertiary),
    );

    let mut preview_code = code;
    let _ = width;
    ui.label(rich("GENERATED RUST NF2 CODE", TextStyle::Overline).color(nf.text_secondary));
    ui.add(c::TextArea::new(&mut preview_code, "Generated Rust Layout Code").rows(12));

    let mut preview_svg = svg_code;
    ui.label(
        rich(
            "GENERATED SVG ARTBOARD XML (ROUND-TRIP & FIGMA READY)",
            TextStyle::Overline,
        )
        .color(nf.text_secondary),
    );
    ui.add(c::TextArea::new(&mut preview_svg, "Generated SVG XML").rows(10));
}
