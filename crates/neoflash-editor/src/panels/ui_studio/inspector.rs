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

fn inner_width_id() -> egui::Id {
    egui::Id::new("nf-ui-studio-inspector-inner-width")
}

/// Width of the inspector body inside its `space::s4()` padding (set by the column every frame).
fn full_w(ui: &Ui) -> f32 {
    ui.data(|d| d.get_temp::<f32>(inner_width_id()))
        .unwrap_or_else(|| ui.available_width())
}

/// One of the two equal form columns.
fn half_w(ui: &Ui) -> f32 {
    ((full_w(ui) - space::s2()) / 2.0).floor()
}

/// A form row of two equal columns; `add` receives the column width.
fn pair<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui, f32) -> R) -> R {
    let w = half_w(ui);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::s2();
        add(ui, w)
    })
    .inner
}

/// Read-only cell of a form row (derived values such as "6 · 4 · 4" or "Hug content").
fn readout(ui: &mut Ui, text: &str, width: f32) {
    let mut text = text.to_owned();
    ui.add_enabled(false, c::TextField::new(&mut text, "").width(width));
}

/// Small caption above a group of rows inside a section.
fn caption(ui: &mut Ui, text: &str) {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.label(rich(text, TextStyle::Small).color(nf.text_secondary));
}

pub(super) fn render_inspector_and_systems_column(
    ui: &mut Ui,
    state: &mut UiLayoutStudioState,
    width: f32,
    height: f32,
) {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.spacing_mut().item_spacing = Vec2::ZERO;
    let column = Rect::from_min_size(ui.max_rect().min, egui::vec2(width, height));
    let tabs = Rect::from_min_size(
        column.min,
        egui::vec2(width, neoflash_ui::metrics::panel_tab()),
    );
    {
        let mut row = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(tabs)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        row.spacing_mut().item_spacing.x = 0.0;
        for (tab, label) in [
            (StudioInspectorTab::NodeInspector, "Node"),
            (StudioInspectorTab::SystemsTab, "Systems"),
            (StudioInspectorTab::CodeExport, "Code"),
        ] {
            if row
                .add(c::PanelTab {
                    label,
                    active: state.inspector_tab == tab,
                })
                .clicked()
            {
                state.inspector_tab = tab;
            }
        }
        ui.painter().hline(
            tabs.x_range(),
            tabs.bottom() - 0.5,
            Stroke::new(1.0_f32, nf.border_subtle),
        );
    }

    let body = Rect::from_min_max(tabs.left_bottom(), column.max);
    let mut body_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(body)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    body_ui.set_clip_rect(body.intersect(ui.clip_rect()));
    let pad = space::s4();
    let inner_w = (width - 2.0 * pad).max(0.0);
    body_ui
        .ctx()
        .data_mut(|d| d.insert_temp(inner_width_id(), inner_w));
    let tab = state.inspector_tab;
    egui::ScrollArea::vertical()
        .id_salt(("studio-right-inspector-scroll", tab))
        .auto_shrink([false, false])
        .themed()
        .show(&mut body_ui, |ui| {
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(pad as i8, space::s2() as i8))
                .show(ui, |ui| {
                    ui.set_width(inner_w);
                    ui.spacing_mut().item_spacing =
                        egui::vec2(space::s2(), neoflash_ui::metrics::row_gap());
                    match tab {
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

/// Sizing row: unit Select with the axis inside (`W  Percent`) and the value in the second column.
/// `px_presets` are offered by the pixel field.
fn render_size_unit_editor(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + Copy,
    axis_label: &str,
    unit: &mut UiSizeUnit,
    reference_px: f32,
) -> bool {
    let mut changed = false;
    pair(ui, |ui, w| {
        let mut mode_idx = match unit {
            UiSizeUnit::Percent(_) => 0usize,
            UiSizeUnit::Px(_) => 1usize,
            UiSizeUnit::Auto => 2usize,
            UiSizeUnit::Fill(_) => 3usize,
        };
        let names = ["Percent", "Pixels", "Auto", "Fill"];
        let current = format!("{axis_label}  {}", names[mode_idx]);
        let picked = c::Select::new((id_salt, "unit-mode"), &current)
            .width(w)
            .show(ui, |ui| {
                let mut picked = None;
                for (idx, name) in names.into_iter().enumerate() {
                    if ui
                        .add(c::MenuItem::new(name).checked(idx == mode_idx))
                        .clicked()
                    {
                        picked = Some(idx);
                    }
                }
                picked
            })
            .1
            .flatten();
        if let Some(idx) = picked.filter(|idx| *idx != mode_idx) {
            mode_idx = idx;
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
                            .unit("%")
                            .range(0.0..=100.0)
                            .decimals(1)
                            .width(w),
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
                            .unit("px")
                            .range(0.0..=2400.0)
                            .decimals(0)
                            .presets(&[120.0, 320.0])
                            .width(w),
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
                            .unit("fr")
                            .range(0.1..=12.0)
                            .decimals(1)
                            .width(w),
                    )
                    .changed()
                {
                    *fr = (v as f32).max(0.1);
                    changed = true;
                }
            }
            UiSizeUnit::Auto => readout(ui, "Hug content", w),
        }
    });
    changed
}

/// Spacing row: a step of the density system (`space 1…6`, `row gap`) with its Normal · Compact ·
/// Classic Flash values, or "Custom px" with a free value for artwork.
fn render_spacing_value_editor(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + Copy,
    label: &str,
    val: &mut SpacingValue,
    systems: &UiStudioSystems,
) -> bool {
    let mut changed = false;
    pair(ui, |ui, w| {
        let step_name = match *val {
            SpacingValue::Token(tok) => tok.label(),
            SpacingValue::Px(_) => "Custom px",
        };
        let current = format!("{label}  {step_name}");
        let picked = c::Select::new((id_salt, "sp-tok"), &current)
            .width(w)
            .show(ui, |ui| {
                ui.set_min_width(neoflash_ui::metrics::menu_width());
                let mut picked = None;
                for tok in SpacingToken::ALL {
                    let text = if tok == SpacingToken::S0 {
                        tok.label().to_owned()
                    } else {
                        format!(
                            "{}   {}",
                            tok.label(),
                            format_density_values(tok.density_values())
                        )
                    };
                    if ui
                        .add(c::MenuItem::new(&text).checked(*val == SpacingValue::Token(tok)))
                        .clicked()
                    {
                        picked = Some(SpacingValue::Token(tok));
                    }
                }
                c::menu_separator(ui);
                if ui
                    .add(c::MenuItem::new("Custom px").checked(val.is_override()))
                    .clicked()
                {
                    picked = Some(SpacingValue::Px(val.resolve(systems)));
                }
                picked
            })
            .1
            .flatten();
        if let Some(next) = picked.filter(|next| next != val) {
            *val = next;
            changed = true;
        }
        match val {
            SpacingValue::Px(px) => {
                let mut px_v = f64::from(*px);
                if ui
                    .add(
                        c::Field::new(&mut px_v, label)
                            .unit("px")
                            .range(0.0..=128.0)
                            .decimals(0)
                            .width(w),
                    )
                    .changed()
                {
                    *px = px_v as f32;
                    changed = true;
                }
            }
            SpacingValue::Token(tok) if systems.spacing.is_override(*tok) => {
                readout(
                    ui,
                    &format!("{:.0} (override)", systems.spacing.resolve(*tok)),
                    w,
                );
            }
            SpacingValue::Token(tok) => {
                readout(ui, &format_density_values(tok.density_values()), w);
            }
        }
    });
    changed
}

/// Uniform padding: one step for all four sides ("Mixed" while the sides differ).
fn render_uniform_padding(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + Copy,
    pad: &mut Padding,
) -> bool {
    let uniform = match pad.top {
        SpacingValue::Token(tok)
            if pad.right == pad.top && pad.bottom == pad.top && pad.left == pad.top =>
        {
            Some(tok)
        }
        _ => None,
    };
    let mut changed = false;
    pair(ui, |ui, w| {
        let current = format!("All  {}", uniform.map_or("mixed", SpacingToken::label));
        let picked = c::Select::new((id_salt, "pad-all"), &current)
            .width(w)
            .show(ui, |ui| {
                ui.set_min_width(neoflash_ui::metrics::menu_width());
                let mut picked = None;
                for tok in SpacingToken::ALL {
                    let text = if tok == SpacingToken::S0 {
                        tok.label().to_owned()
                    } else {
                        format!(
                            "{}   {}",
                            tok.label(),
                            format_density_values(tok.density_values())
                        )
                    };
                    if ui
                        .add(c::MenuItem::new(&text).checked(uniform == Some(tok)))
                        .clicked()
                    {
                        picked = Some(tok);
                    }
                }
                picked
            })
            .1
            .flatten();
        if let Some(tok) = picked.filter(|tok| uniform != Some(*tok)) {
            *pad = Padding::all_token(tok);
            changed = true;
        }
        let values = uniform.map_or_else(
            || "—".to_owned(),
            |tok| format_density_values(tok.density_values()),
        );
        readout(ui, &values, w);
    });
    changed
}

fn render_node_inspector_tab(ui: &mut Ui, state: &mut UiLayoutStudioState, _width: f32) {
    let sel_id = state.primary_selected_id();
    let Some(mut node_clone) = state.document.find_node(sel_id).cloned() else {
        return;
    };
    let mut modified = false;

    // 1. Node header: kind, name, reset to the system tokens
    let mut reset_clicked = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::s1();
        let side = neoflash_ui::metrics::control();
        let subtitle = format!("{}  #{sel_id}", node_clone.label);
        ui.allocate_ui(
            egui::vec2(
                full_w(ui) - side - space::s1(),
                neoflash_ui::metrics::panel_tab(),
            ),
            |ui| {
                ui.add(c::TypeHeader {
                    icon: super::hierarchy::icon_for_node_kind(&node_clone.kind),
                    title: node_clone.kind.type_badge(),
                    subtitle: &subtitle,
                });
            },
        );
        if ui
            .add(c::IconButton::new(
                i16::RESET_TRANSFORM,
                "Reset node to system tokens",
            ))
            .clicked()
        {
            state.commit_history("Reset node to system tokens");
            state.document.reset_node_to_system_defaults(sel_id);
            state.status_message = format!("Reset node #{sel_id} to the system tokens.");
            reset_clicked = true;
        }
    });
    if reset_clicked {
        return;
    }

    c::SectionHeader::new("Node")
        .id_salt("studio-sec-node")
        .show(ui, |ui| {
            if ui
                .add(
                    c::TextField::new(&mut node_clone.label, "Label")
                        .prefix("Label")
                        .width(full_w(ui)),
                )
                .changed()
            {
                modified = true;
            }
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = space::s4();
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

    // 2. Box sizing: percent, pixels, auto (hug) or fill
    c::SectionHeader::new("Box sizing")
        .id_salt("studio-sec-box")
        .show(ui, |ui| {
            let density = state.systems.density.active();
            if render_size_unit_editor(
                ui,
                ("node-w", sel_id),
                "W",
                &mut node_clone.box_constraints.width,
                density.inspector_width,
            ) {
                modified = true;
            }
            if render_size_unit_editor(
                ui,
                ("node-h", sel_id),
                "H",
                &mut node_clone.box_constraints.height,
                density.control_height,
            ) {
                modified = true;
            }

            // Quick widths
            let quick = [
                (0_i32, "25", UiSizeUnit::Percent(25.0)),
                (1, "33", UiSizeUnit::Percent(33.3333)),
                (2, "50", UiSizeUnit::Percent(50.0)),
                (3, "100", UiSizeUnit::Percent(100.0)),
                (4, "Auto", UiSizeUnit::Auto),
            ];
            let mut quick_idx = quick
                .iter()
                .find(|(_, _, unit)| *unit == node_clone.box_constraints.width)
                .map_or(-1, |(idx, _, _)| *idx);
            ui.allocate_ui(
                egui::vec2(full_w(ui), neoflash_ui::metrics::control()),
                |ui| {
                    if c::Segmented::show(
                        ui,
                        &mut quick_idx,
                        &quick.map(|(idx, label, _)| (idx, label)),
                        true,
                    )
                    .is_some()
                        && let Some((_, _, unit)) =
                            quick.iter().find(|(idx, _, _)| *idx == quick_idx)
                    {
                        node_clone.box_constraints.width = *unit;
                        modified = true;
                    }
                },
            );

            // Min / Max width bounds
            pair(ui, |ui, w| {
                let mut min_w = f64::from(node_clone.box_constraints.min_width.unwrap_or(0.0));
                if ui
                    .add(
                        c::Field::new(&mut min_w, "Min W")
                            .prefix("Min W")
                            .unit("px")
                            .range(0.0..=1200.0)
                            .width(w),
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
                            .prefix("Max W")
                            .unit("px")
                            .range(0.0..=2400.0)
                            .width(w),
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
        c::SectionHeader::new("Grid columns")
            .id_salt("studio-sec-grid")
            .show(ui, |ui| {
                let presets: [(&str, Vec<UiSizeUnit>); 4] = [
                    (
                        "2 columns, 50 % each",
                        vec![UiSizeUnit::Percent(50.0), UiSizeUnit::Percent(50.0)],
                    ),
                    (
                        "3 columns, 33.3 % each",
                        vec![
                            UiSizeUnit::Percent(33.3333),
                            UiSizeUnit::Percent(33.3333),
                            UiSizeUnit::Percent(33.3334),
                        ],
                    ),
                    (
                        "4 columns, 25 % each",
                        vec![
                            UiSizeUnit::Percent(25.0),
                            UiSizeUnit::Percent(25.0),
                            UiSizeUnit::Percent(25.0),
                            UiSizeUnit::Percent(25.0),
                        ],
                    ),
                    (
                        "120 px + 50 % + 50 %",
                        vec![
                            UiSizeUnit::Px(120.0),
                            UiSizeUnit::Percent(50.0),
                            UiSizeUnit::Percent(50.0),
                        ],
                    ),
                ];
                let current = presets
                    .iter()
                    .find(|(_, tracks)| tracks == columns)
                    .map_or("Custom tracks", |(label, _)| *label);
                let picked = c::Select::new(("grid-preset", sel_id), current)
                    .width(full_w(ui))
                    .show(ui, |ui| {
                        let mut picked = None;
                        for (label, tracks) in &presets {
                            if ui
                                .add(c::MenuItem::new(label).checked(tracks == columns))
                                .clicked()
                            {
                                picked = Some(tracks.clone());
                            }
                        }
                        picked
                    })
                    .1
                    .flatten();
                if let Some(tracks) = picked {
                    *columns = tracks;
                    modified = true;
                }

                for (col_idx, col_unit) in columns.iter_mut().enumerate() {
                    if render_size_unit_editor(
                        ui,
                        ("grid-col", sel_id, col_idx),
                        &format!("{}", col_idx + 1),
                        col_unit,
                        neoflash_ui::metrics::inspector(),
                    ) {
                        modified = true;
                    }
                }

                pair(ui, |ui, w| {
                    if ui.add(c::Button::new("Add column").width(w)).clicked() {
                        columns.push(UiSizeUnit::Percent(25.0));
                        modified = true;
                    }
                    if ui
                        .add_enabled(columns.len() > 1, c::Button::new("Remove column").width(w))
                        .clicked()
                    {
                        columns.pop();
                        modified = true;
                    }
                });
            });
    }

    // 4. Spacing: steps of the density system, shared system binding
    c::SectionHeader::new("Density spacing")
        .id_salt("studio-sec-pad")
        .show(ui, |ui| {
            let bindings: [(Option<SharedSystemKind>, &str); 5] = [
                (None, "Binding  None (local)"),
                (
                    Some(SharedSystemKind::TwoColumnForm),
                    "Binding  Two-column form",
                ),
                (
                    Some(SharedSystemKind::SectionHeader),
                    "Binding  Section header",
                ),
                (Some(SharedSystemKind::PanelHeader), "Binding  Panel header"),
                (Some(SharedSystemKind::ToolRail), "Binding  Tool rail"),
            ];
            if c::Select::options(
                ui,
                ("node-sys-bind", sel_id),
                &mut node_clone.style.system_binding,
                &bindings,
                full_w(ui),
            ) {
                modified = true;
            }

            caption(ui, "Gap");
            if render_spacing_value_editor(
                ui,
                ("gap-x", sel_id),
                "X",
                &mut node_clone.style.gap.x,
                &state.systems,
            ) {
                modified = true;
            }
            if render_spacing_value_editor(
                ui,
                ("gap-y", sel_id),
                "Y",
                &mut node_clone.style.gap.y,
                &state.systems,
            ) {
                modified = true;
            }

            caption(ui, "Padding");
            if render_uniform_padding(ui, ("pad-all", sel_id), &mut node_clone.style.padding) {
                modified = true;
            }
            for (salt, label, side) in [
                ("pad-t", "T", &mut node_clone.style.padding.top),
                ("pad-r", "R", &mut node_clone.style.padding.right),
                ("pad-b", "B", &mut node_clone.style.padding.bottom),
                ("pad-l", "L", &mut node_clone.style.padding.left),
            ] {
                if render_spacing_value_editor(ui, (salt, sel_id), label, side, &state.systems) {
                    modified = true;
                }
            }
        });

    // 5. Typography
    c::SectionHeader::new("Typography")
        .id_salt("studio-sec-typo")
        .show(ui, |ui| {
            let mut is_custom = node_clone.style.typography.is_custom();
            if ui
                .add(c::Checkbox::new(&mut is_custom, "Custom typography"))
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
                    let labels: Vec<String> = StudioTextStyle::ALL
                        .into_iter()
                        .map(|s| {
                            let entry = state.systems.typography.entry(s);
                            format!(
                                "{} {:.0} / {:.0}",
                                s.label(),
                                entry.size_pt,
                                entry.line_height_pt
                            )
                        })
                        .collect();
                    let opts: Vec<(StudioTextStyle, &str)> = StudioTextStyle::ALL
                        .into_iter()
                        .zip(labels.iter().map(String::as_str))
                        .collect();
                    if c::Select::options(
                        ui,
                        ("node-sys-typo", sel_id),
                        style_tok,
                        &opts,
                        full_w(ui),
                    ) {
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
                        full_w(ui),
                    ) {
                        modified = true;
                    }
                    pair(ui, |ui, w| {
                        let mut sz = f64::from(ov.size_pt);
                        if ui
                            .add(
                                c::Field::new(&mut sz, "Font size")
                                    .prefix("Size")
                                    .unit("pt")
                                    .range(8.0..=48.0)
                                    .decimals(1)
                                    .width(w),
                            )
                            .changed()
                        {
                            ov.size_pt = sz as f32;
                            modified = true;
                        }
                        let mut lh = f64::from(ov.line_height_pt);
                        if ui
                            .add(
                                c::Field::new(&mut lh, "Line height")
                                    .prefix("Line")
                                    .unit("pt")
                                    .range(8.0..=64.0)
                                    .decimals(1)
                                    .width(w),
                            )
                            .changed()
                        {
                            ov.line_height_pt = lh as f32;
                            modified = true;
                        }
                    });
                    pair(ui, |ui, w| {
                        if c::Select::options(
                            ui,
                            ("node-cust-wt", sel_id),
                            &mut ov.weight,
                            &[(400, "Regular"), (500, "Medium"), (600, "Semibold")],
                            w,
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
                            w,
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
                        full_w(ui),
                    ) {
                        modified = true;
                    }
                }
            }
        });

    let node_drag_id = ui.id().with("studio-inspector-node-drag");
    let pointer_down = ui.input(|i| i.pointer.any_down());
    if modified {
        let doc_before = state.document.clone();
        if let Some(target) = state.document.find_node_mut(sel_id) {
            *target = node_clone;
        }
        state.sync_active_to_artboard();
        let mut snap = state.capture_snapshot(format!("Edit node #{sel_id}"));
        snap.document = doc_before.clone();
        if let Some(ab) = snap.artboards.get_mut(snap.active_artboard_idx) {
            ab.document = doc_before;
        }
        if pointer_down {
            let has_pending = ui
                .ctx()
                .data(|d| d.get_temp::<UiStudioSnapshot>(node_drag_id).is_some());
            if !has_pending {
                ui.ctx().data_mut(|d| d.insert_temp(node_drag_id, snap));
            }
        } else if let Some(pending) = take_temp::<UiStudioSnapshot>(ui.ctx(), node_drag_id) {
            if state.document != pending.document {
                state.push_history_snapshot(pending);
            }
        } else if state.document != snap.document {
            state.push_history_snapshot(snap);
        }
    } else if !pointer_down
        && let Some(pending) = take_temp::<UiStudioSnapshot>(ui.ctx(), node_drag_id)
        && state.document != pending.document
    {
        state.push_history_snapshot(pending);
    }
}

fn render_timeline_geometry_controls(ui: &mut Ui, state: &mut UiLayoutStudioState) {
    c::SectionHeader::new("Tween arrows")
        .id_salt("sys-sec-tween-arrows")
        .show(ui, |ui| {
            let tg = &mut state.systems.timeline_geometry;
            let mut head_len = f64::from(tg.tween_arrow_head_length_px);
            if ui
                .add(
                    c::Field::new(&mut head_len, "Arrow Head Length")
                        .prefix("Head L")
                        .unit("px")
                        .range(2.0..=28.0)
                        .decimals(1)
                        .stepper()
                        .width(half_w(ui)),
                )
                .changed()
            {
                tg.tween_arrow_head_length_px = head_len as f32;
            }

            let mut wing = f64::from(tg.tween_arrow_head_wing_px);
            if ui
                .add(
                    c::Field::new(&mut wing, "Arrow Wing")
                        .prefix("Wing")
                        .unit("px")
                        .range(1.0..=16.0)
                        .decimals(1)
                        .stepper()
                        .width(half_w(ui)),
                )
                .changed()
            {
                tg.tween_arrow_head_wing_px = wing as f32;
            }

            ui.horizontal(|ui| {
                let mut start_in = f64::from(tg.tween_arrow_start_inset_px);
                if ui
                    .add(
                        c::Field::new(&mut start_in, "Start Inset")
                            .prefix("In L")
                            .unit("px")
                            .range(0.0..=40.0)
                            .decimals(1)
                            .width(half_w(ui)),
                    )
                    .changed()
                {
                    tg.tween_arrow_start_inset_px = start_in as f32;
                }
                let mut end_in = f64::from(tg.tween_arrow_end_inset_px);
                if ui
                    .add(
                        c::Field::new(&mut end_in, "End Inset")
                            .prefix("In R")
                            .unit("px")
                            .range(0.0..=40.0)
                            .decimals(1)
                            .width(half_w(ui)),
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
                        c::Field::new(&mut stroke_w, "Arrow Stroke")
                            .prefix("Stroke")
                            .unit("px")
                            .range(0.5..=5.0)
                            .decimals(2)
                            .width(half_w(ui)),
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
                            .width(half_w(ui)),
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
                            .width(half_w(ui)),
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
                            .width(half_w(ui)),
                    )
                    .changed()
                {
                    tg.broken_tween_gap_px = gap as f32;
                }
            });
        });

    c::SectionHeader::new("Cells, keyframes and badges")
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
                            .width(half_w(ui)),
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
                            .width(half_w(ui)),
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
                            .width(half_w(ui)),
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
                            .width(half_w(ui)),
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
                            .width(half_w(ui)),
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
                            .width(half_w(ui)),
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
                            .width(half_w(ui)),
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
                            .width(half_w(ui)),
                    )
                    .changed()
                {
                    tg.label_flag_size_px = flag_sz as f32;
                }
            });
        });

    c::SectionHeader::new("Playhead, ruler and onion skin")
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
                            .width(half_w(ui)),
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
                            .width(half_w(ui)),
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
                            .width(half_w(ui)),
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
                            .width(half_w(ui)),
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
                            .width(half_w(ui)),
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
                            .width(half_w(ui)),
                    )
                    .changed()
                {
                    tg.onion_bracket_width_px = ob_w as f32;
                }
            });
        });

    c::SectionHeader::new("Layer column")
        .id_salt("sys-sec-layer-col")
        .show(ui, |ui| {
            let _ = render_size_unit_editor(
                ui,
                "tl-layer-col-w",
                "W",
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
                            .width(half_w(ui)),
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
                            .width(half_w(ui)),
                    )
                    .changed()
                {
                    tg.layer_icon_gap_px = igap as f32;
                }
            });
        });
}

fn render_workspace_and_stage_controls(ui: &mut Ui, state: &mut UiLayoutStudioState) {
    c::SectionHeader::new("Workspace docks")
        .id_salt("sys-sec-shell-docks")
        .show(ui, |ui| {
            caption(ui, "Tools dock");
            let _ = render_size_unit_editor(
                ui,
                "shell-left-tools-w",
                "W",
                &mut state.systems.workspace_shell.shell_left_tools_width,
                820.0,
            );
            caption(ui, "Right dock");
            let _ = render_size_unit_editor(
                ui,
                "shell-right-insp-w",
                "W",
                &mut state.systems.workspace_shell.shell_right_inspector_width,
                820.0,
            );
            caption(ui, "Timeline");
            let _ = render_size_unit_editor(
                ui,
                "shell-bot-tl-h",
                "H",
                &mut state.systems.workspace_shell.shell_bottom_timeline_height,
                560.0,
            );

            caption(ui, "Bars");
            let ws = &mut state.systems.workspace_shell;
            ui.horizontal(|ui| {
                let mut mh = f64::from(ws.top_menubar_height_px);
                if ui
                    .add(
                        c::Field::new(&mut mh, "Menu Bar H")
                            .prefix("Menu H")
                            .unit("px")
                            .range(18.0..=48.0)
                            .decimals(0)
                            .width(half_w(ui)),
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
                            .width(half_w(ui)),
                    )
                    .changed()
                {
                    ws.doc_tabs_height_px = th as f32;
                }
            });
        });

    c::SectionHeader::new("Stage rulers, guides and handles")
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
                            .width(half_w(ui)),
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
                            .width(half_w(ui)),
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
    c::Select::options(
        ui,
        "studio-systems-subtab",
        &mut state.systems_subtab,
        &[
            (SystemsSubTab::TimelineGeometry, "Timeline and arrows"),
            (SystemsSubTab::WorkspaceAndStage, "Workspace and Stage"),
            (SystemsSubTab::SpacingAndDensity, "Spacing and density"),
            (SystemsSubTab::TypographyScale, "Typography"),
            (SystemsSubTab::SharedLayouts, "Shared layouts"),
            (SystemsSubTab::PaletteTokens, "Palette"),
        ],
        full_w(ui),
    );

    let mut reset_systems_clicked = false;
    if ui
        .add(c::Button::ghost("Reset all systems").icon(i16::RESET_TRANSFORM))
        .clicked()
    {
        state.commit_history("Reset all NF2 systems to defaults");
        state.systems.reset_all_to_defaults();
        state.status_message = "Reset all system scales to their defaults.".to_owned();
        reset_systems_clicked = true;
    }
    ui.label(
        rich(
            "A system change updates every bound widget on the canvas.",
            TextStyle::Caption,
        )
        .color(nf.text_secondary),
    );
    if reset_systems_clicked {
        return;
    }

    let systems_before = state.systems.clone();
    match state.systems_subtab {
        SystemsSubTab::TimelineGeometry => {
            render_timeline_geometry_controls(ui, state);
        }
        SystemsSubTab::WorkspaceAndStage => {
            render_workspace_and_stage_controls(ui, state);
        }
        SystemsSubTab::SpacingAndDensity => {
            c::SectionHeader::new("Spacing scale")
                .id_salt("sys-sec-spacing")
                .show(ui, |ui| {
                    caption(ui, "Normal value · Normal, Compact, Classic Flash");
                    for tok in [
                        SpacingToken::S1,
                        SpacingToken::S2,
                        SpacingToken::S3,
                        SpacingToken::S4,
                        SpacingToken::S5,
                        SpacingToken::S6,
                    ] {
                        // The stored step is the Normal reference; any other value overrides the
                        // density system in the Studio.
                        let overridden = state.systems.spacing.is_override(tok);
                        let mut v = f64::from(if overridden {
                            state.systems.spacing.resolve(tok)
                        } else {
                            tok.density_values()[0]
                        });
                        let side = neoflash_ui::metrics::control();
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = space::s2();
                            let w = half_w(ui);
                            if ui
                                .add(
                                    c::Field::new(&mut v, tok.label())
                                        .prefix(tok.label())
                                        .range(0.0..=96.0)
                                        .decimals(0)
                                        .width(w),
                                )
                                .changed()
                            {
                                state.systems.spacing.set(tok, v as f32);
                            }
                            let note = if overridden {
                                "override".to_owned()
                            } else {
                                format_density_values(tok.density_values())
                            };
                            readout(ui, &note, w - side - space::s1());
                            ui.spacing_mut().item_spacing.x = space::s1();
                            if ui
                                .add_enabled(
                                    overridden,
                                    c::IconButton::new(
                                        i16::RESET_TRANSFORM,
                                        "Follow the density system again",
                                    ),
                                )
                                .clicked()
                            {
                                state.systems.spacing.reset(tok);
                            }
                        });
                    }
                    pair(ui, |ui, w| {
                        readout(ui, SpacingToken::RowGap.label(), w);
                        readout(
                            ui,
                            &format_density_values(SpacingToken::RowGap.density_values()),
                            w,
                        );
                    });
                });

            c::SectionHeader::new("Density metrics")
                .id_salt("sys-sec-density")
                .show(ui, |ui| {
                    caption(
                        ui,
                        "Read from neoflash_ui::metrics; shared with the editor.",
                    );
                    let columns = [0.0, 0.52, 0.68, 0.84];
                    ui.scope(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.add(c::TableRow {
                            cells: &["Metric", "Normal", "Compact", "Classic"],
                            kind: c::TableRowKind::Header,
                            columns: &columns,
                        });
                        for (name, values) in neoflash_ui::metrics::DENSITY_TABLE {
                            let label = name.replace('_', " ");
                            let cells = values.map(|v| format!("{v:.0}"));
                            ui.add(c::TableRow {
                                cells: &[&label, &cells[0], &cells[1], &cells[2]],
                                kind: c::TableRowKind::Row,
                                columns: &columns,
                            });
                        }
                    });
                });
        }
        SystemsSubTab::TypographyScale => {
            c::SectionHeader::new("Typography scale")
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
                                        .width(half_w(ui)),
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
                                        .width(half_w(ui)),
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
            c::SectionHeader::new("Two-column form")
                .id_salt("sys-sec-2col")
                .show(ui, |ui| {
                    let mut min_cell = f64::from(state.systems.two_column_form.min_cell_width);
                    if ui
                        .add(
                            c::Field::new(&mut min_cell, "Min Cell W")
                                .prefix("MinW")
                                .unit("px")
                                .range(48.0..=240.0)
                                .width(half_w(ui)),
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
                                .width(half_w(ui)),
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
                                .width(half_w(ui)),
                        )
                        .changed()
                    {
                        state.systems.two_column_form.gap = SpacingValue::Px(gap_px as f32);
                    }
                });

            c::SectionHeader::new("Section and panel headers")
                .id_salt("sys-sec-headers")
                .show(ui, |ui| {
                    let mut sec_h = f64::from(state.systems.section_header.height);
                    if ui
                        .add(
                            c::Field::new(&mut sec_h, "Section Header H")
                                .prefix("SecH")
                                .unit("pt")
                                .range(20.0..=56.0)
                                .width(half_w(ui)),
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
                                .width(half_w(ui)),
                        )
                        .changed()
                    {
                        state.systems.panel_header.height = pnl_h as f32;
                    }
                });

            c::SectionHeader::new("Tool rail")
                .id_salt("sys-sec-toolrail")
                .show(ui, |ui| {
                    let mut cols = state.systems.tool_rail.columns as f64;
                    if ui
                        .add(
                            c::Field::new(&mut cols, "Tool Columns")
                                .prefix("Cols")
                                .range(1.0..=4.0)
                                .stepper()
                                .width(half_w(ui)),
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
                                .width(half_w(ui)),
                        )
                        .changed()
                    {
                        state.systems.tool_rail.button_side = side as f32;
                    }
                });
        }
        SystemsSubTab::PaletteTokens => {
            // The preview theme of the toolbar chooses whose overrides are edited.
            let theme = state.preview_theme;
            let heading = format!("{} overrides", theme.label());
            c::SectionHeader::new(&heading)
                .id_salt("sys-sec-palette")
                .show(ui, |ui| {
                    let chip = neoflash_ui::metrics::swatch();
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
                        // Caption line: role, then the role in the four themes.
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = space::s1();
                            ui.label(
                                rich(role.label(), TextStyle::MonoSmall).color(nf.text_secondary),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    for swatch_theme in ThemeMode::ALL.into_iter().rev() {
                                        let sw_col = state
                                            .systems
                                            .palette_overrides
                                            .resolve_color(swatch_theme, role);
                                        let (sw_rect, sw_resp) = ui.allocate_exact_size(
                                            egui::vec2(chip, chip),
                                            Sense::click(),
                                        );
                                        c::display::paint_swatch(ui, sw_rect, sw_col, radius::sm());
                                        if swatch_theme == theme {
                                            ui.painter().rect_stroke(
                                                sw_rect,
                                                radius::sm(),
                                                Stroke::new(1.0_f32, nf.accent_default),
                                                StrokeKind::Outside,
                                            );
                                        }
                                        if c::tooltip(sw_resp, swatch_theme.label(), None).clicked()
                                        {
                                            state.set_preview_theme(swatch_theme);
                                        }
                                    }
                                },
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = space::s1();
                            let side = neoflash_ui::metrics::control();
                            let resp = c::ColorField::new(&mut col, role.label())
                                .width(full_w(ui) - side - space::s1())
                                .show(ui);
                            if resp.response.changed() {
                                state
                                    .systems
                                    .palette_overrides
                                    .set_color_override(theme, role, col);
                            }
                            if ui
                                .add_enabled(
                                    state.systems.palette_overrides.has_override(theme, role),
                                    c::IconButton::new(i16::RESET_TRANSFORM, "Reset override"),
                                )
                                .clicked()
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
    coalesce_systems_history(ui.ctx(), state, &systems_before);
}

fn coalesce_systems_history(
    ctx: &egui::Context,
    state: &mut UiLayoutStudioState,
    systems_before: &UiStudioSystems,
) {
    let sys_drag_id = egui::Id::new("nf2-studio-inspector-systems-drag");
    let pointer_down = ctx.input(|i| i.pointer.any_down());
    let mutated_this_frame = state.systems != *systems_before;
    if mutated_this_frame {
        let mut snap = state.capture_snapshot("Edit NF2 system tokens");
        snap.systems = systems_before.clone();
        if pointer_down {
            let has_pending = ctx.data(|d| d.get_temp::<UiStudioSnapshot>(sys_drag_id).is_some());
            if !has_pending {
                ctx.data_mut(|d| d.insert_temp(sys_drag_id, snap));
            }
        } else if let Some(pending) = take_temp::<UiStudioSnapshot>(ctx, sys_drag_id) {
            if state.systems != pending.systems {
                state.push_history_snapshot(pending);
            }
        } else {
            state.push_history_snapshot(snap);
        }
    } else if !pointer_down
        && let Some(pending) = take_temp::<UiStudioSnapshot>(ctx, sys_drag_id)
        && state.systems != pending.systems
    {
        state.push_history_snapshot(pending);
    }
}

fn render_code_export_tab(ui: &mut Ui, state: &mut UiLayoutStudioState, _width: f32) {
    let nf = neoflash_ui::palette(ui.ctx());
    let code = export_rust_layout_with_systems(&state.document, &state.systems);
    let svg_code = state.export_active_svg();

    pair(ui, |ui, w| {
        if ui.add(c::Button::new("Copy Rust").width(w)).clicked() {
            ui.ctx().copy_text(code.clone());
            state.status_message = "Copied the generated Rust code.".to_owned();
        }
        if ui.add(c::Button::new("Copy SVG").width(w)).clicked() {
            ui.ctx().copy_text(svg_code.clone());
            state.svg_buffer = svg_code.clone();
            state.status_message = "Copied the SVG of the active panel.".to_owned();
        }
    });
    pair(ui, |ui, w| {
        if ui.add(c::Button::new("Import SVG…").width(w)).clicked() {
            state.svg_import_modal_open = true;
        }
        if ui.add(c::Button::new("Write all files").width(w)).clicked() {
            let _ = state.save_studio_json(Path::new(DEFAULT_STUDIO_JSON_PATH));
            let _ = state.save_svg_to_file(Path::new(DEFAULT_STUDIO_SVG_EXPORT_PATH));
            if state
                .export_rust_to_file(Path::new(DEFAULT_STUDIO_RUST_EXPORT_PATH))
                .is_ok()
            {
                state.last_output = "Wrote JSON, Rust and SVG to target/ui-studio/".to_owned();
                state.status_message = "Wrote all three files.".to_owned();
            }
        }
    });

    caption(ui, "Files");
    for path in [
        DEFAULT_STUDIO_JSON_PATH,
        DEFAULT_STUDIO_RUST_EXPORT_PATH,
        DEFAULT_STUDIO_SVG_EXPORT_PATH,
    ] {
        // File names only: the folder is the same and the column is narrow.
        let name = path.rsplit('/').next().unwrap_or(path);
        c::tooltip(
            ui.label(rich(name, TextStyle::MonoSmall).color(nf.text_tertiary)),
            path,
            None,
        );
    }

    let mut preview_code = code;
    c::SectionHeader::new("Generated Rust")
        .id_salt("studio-sec-code-rust")
        .show(ui, |ui| {
            ui.add(c::TextArea::new(&mut preview_code, "Generated Rust layout code").rows(14));
        });

    let mut preview_svg = svg_code;
    c::SectionHeader::new("Generated SVG")
        .id_salt("studio-sec-code-svg")
        .show(ui, |ui| {
            ui.add(c::TextArea::new(&mut preview_svg, "Generated SVG").rows(10));
        });
}
