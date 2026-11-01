//! Tools panel — `NF2/Tools Panel`.
//!
//! The layout follows the available width, not a setting:
//!
//! * **2 Columns** (base layout, docked default 80 pt and floating): every tool visible, sections
//!   Tools · View · Colors · Options.
//! * **1 Column** (panel narrower than two tool buttons): related tools share one `NF2/Tool Button`
//!   with a flyout (right-click, long-press or → opens the group), groups separated by
//!   `NF2/Divider`s. As many groups as fit are shown; a trailing "…" button lists the rest. Paint
//!   colours (`NF2/Fill Stroke Chip`, black-and-white, swap) and the tool-options popover stay
//!   pinned to the bottom.
//!
//! Only tools the editor implements are shown. NF2 tools without an engine (Deco, Spray, Bone, Bind,
//! PolyStar, primitives, 3D) and the Snap toggle are omitted rather than faked.
use crate::app::*;
use crate::theme::ScrollTheme as _;
use crate::{ui_icons::Icon, workspace::PanelId};
use eframe::egui::{self, Key, Rect, Vec2};
use neoflash_ui::components::{self as nf, menu::menu_style};
use neoflash_ui::icons::i20;
use neoflash_ui::metrics;
use neoflash_ui::tokens::{TextStyle, space};
use neoflash_ui::typography::rich;

/// Gap between tool buttons (`NF2/Tools Panel` tool grid gap).
const TOOL_GAP: f32 = space::S1 / 2.0;
/// Hold time that opens a tool group (§13.4: long-press opens the flyout).
const LONG_PRESS: f64 = 0.45;
/// Width of the colour-action icon buttons under the Fill Stroke Chip (outline `Color actions`).
const COLOR_ACTION: f32 = 20.0;

#[derive(Clone, Copy)]
struct ToolSpec {
    tool: Tool,
    icon: Icon,
    /// Tooltip name.
    name: &'static str,
    /// Tooltip shortcut chip.
    key: &'static str,
    /// Accessible name (kept from the earlier panel so assistive technology and tests stay stable).
    label: &'static str,
}

const fn spec(
    tool: Tool,
    icon: Icon,
    name: &'static str,
    key: &'static str,
    label: &'static str,
) -> ToolSpec {
    ToolSpec {
        tool,
        icon,
        name,
        key,
        label,
    }
}

const SELECT: ToolSpec = spec(
    Tool::Select,
    Icon::Select,
    "Selection",
    "V",
    "Selection (V)",
);
const SUBSELECT: ToolSpec = spec(
    Tool::Subselect,
    Icon::Subselect,
    "Subselection",
    "A",
    "Subselection (A)",
);
const TRANSFORM: ToolSpec = spec(
    Tool::Transform,
    Icon::Transform,
    "Free Transform",
    "Q",
    "Free Transform (Q)",
);
const GRADIENT: ToolSpec = spec(
    Tool::Gradient,
    Icon::Gradient,
    "Gradient Transform",
    "F",
    "Gradient Transform (F)",
);
const LASSO: ToolSpec = spec(Tool::Lasso, Icon::Lasso, "Lasso", "L", "Lasso (L)");
const PEN: ToolSpec = spec(Tool::Pen, Icon::Pen, "Pen", "P", "Pen (P)");
const LINE: ToolSpec = spec(Tool::Line, Icon::Line, "Line", "N", "Line (N)");
const TEXT: ToolSpec = spec(Tool::Text, Icon::Text, "Text", "T", "Text (T)");
const RECTANGLE: ToolSpec = spec(
    Tool::Rectangle,
    Icon::Rectangle,
    "Rectangle",
    "R",
    "Rectangle (R)",
);
const OVAL: ToolSpec = spec(Tool::Ellipse, Icon::Ellipse, "Oval", "O", "Oval (O)");
const POLYSTAR: ToolSpec = spec(Tool::PolyStar, Icon::PolyStar, "PolyStar", "", "PolyStar");
const PENCIL: ToolSpec = spec(Tool::Pencil, Icon::Pencil, "Pencil", "Y", "Pencil (Y)");
const BRUSH: ToolSpec = spec(Tool::Brush, Icon::Brush, "Brush", "B", "Brush (B)");
const BUCKET: ToolSpec = spec(
    Tool::Bucket,
    Icon::Bucket,
    "Paint Bucket",
    "K",
    "Paint Bucket (K)",
);
const INK: ToolSpec = spec(Tool::Ink, Icon::Ink, "Ink Bottle", "S", "Ink Bottle (S)");
const EYEDROPPER: ToolSpec = spec(
    Tool::Eyedropper,
    Icon::Eyedropper,
    "Eyedropper",
    "I",
    "Eyedropper (I)",
);
const ERASER: ToolSpec = spec(Tool::Eraser, Icon::Eraser, "Eraser", "E", "Eraser (E)");
const HAND: ToolSpec = spec(Tool::Hand, Icon::Hand, "Hand", "H", "Hand (H / hold Space)");
const ZOOM: ToolSpec = spec(Tool::Zoom, Icon::ZoomIn, "Zoom", "Z", "Zoom (Z / M)");

const _LEGACY_TOOLS_PLANNED_ENTRIES: (&str, &str) = (
    "Swatches popover ▸ Gradient",
    "Tool options ▸ PolyStar settings…",
);

/// 1 Column rail: sections (separated by dividers) of tool groups (one button + flyout each).
const RAIL: &[&[&[ToolSpec]]] = &[
    &[&[SELECT, SUBSELECT], &[TRANSFORM, GRADIENT], &[LASSO]],
    &[
        &[PEN, LINE],
        &[TEXT],
        &[RECTANGLE, OVAL, POLYSTAR],
        &[PENCIL, BRUSH],
    ],
    &[&[BUCKET, INK], &[EYEDROPPER], &[ERASER]],
    &[&[HAND, ZOOM]],
];
/// 2 Columns grid, classic Flash order.
const GRID: &[ToolSpec] = &[
    SELECT, SUBSELECT, LINE, LASSO, PEN, TEXT, OVAL, RECTANGLE, POLYSTAR, PENCIL, BRUSH, TRANSFORM,
    GRADIENT, INK, BUCKET, EYEDROPPER, ERASER,
];
const VIEW: &[ToolSpec] = &[HAND, ZOOM];

/// Popover owned by the Tools panel (at most one is open).
#[derive(Clone, Copy, PartialEq)]
enum RailPopover {
    Flyout(usize),
    Overflow,
    Fill,
    Stroke,
    Options,
}

fn popover_id() -> egui::Id {
    egui::Id::new("nf2-tools-popover")
}

impl Editor {
    pub(crate) fn tools_contents(&mut self, ui: &mut egui::Ui) {
        let id = (
            "neoflash-panel-content",
            PanelId::Tools,
            self.clipboard.session.clone(),
        );
        ui.scope_builder(egui::UiBuilder::new().id(id), |ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(TOOL_GAP);
            let two_columns = ui.available_width() >= 2. * metrics::TOOL_BUTTON + TOOL_GAP;
            if two_columns {
                egui::ScrollArea::vertical()
                    .id_salt("tool_controls")
                    .auto_shrink([false, false])
                    .themed()
                    .show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            self.tool_grid(ui);
                            self.view_commands(ui);
                            nf::divider(ui, false);
                            section_caption(ui, "Colors");
                            self.fill_stroke_chip(ui);
                            self.color_actions(ui);
                            nf::divider(ui, false);
                            section_caption(ui, "Options");
                            if ui.available_width() >= inline_options_width() {
                                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                                    ui.spacing_mut().item_spacing = Vec2::splat(space::S2);
                                    self.tool_options_body(ui);
                                });
                            } else {
                                self.tool_options_button(ui);
                            }
                        });
                    });
                return;
            }
            // Rail: paint colours and options stay pinned while only the tools scroll.
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                ui.add_space(space::S1);
                self.tool_options_button(ui);
                self.color_actions(ui);
                self.fill_stroke_chip(ui);
                nf::divider(ui, false);
                ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                    self.tool_rail(ui);
                });
            });
        });
    }

    /// 1 Column: as many groups as fit, then "…" listing the remaining tools and view commands.
    fn tool_rail(&mut self, ui: &mut egui::Ui) {
        let entries = rail_entries();
        let available = ui.available_height();
        let total: f32 = entries.iter().map(|entry| entry.height()).sum();
        let shown = if total <= available {
            entries.len()
        } else {
            let budget = available - RailEntry::BUTTON;
            let mut used = 0.;
            let mut count = 0;
            for entry in &entries {
                used += entry.height();
                if used > budget {
                    break;
                }
                count += 1;
            }
            while count > 0 && matches!(entries[count - 1], RailEntry::Divider) {
                count -= 1;
            }
            count
        };
        for entry in &entries[..shown] {
            match *entry {
                RailEntry::Divider => {
                    nf::divider(ui, false);
                }
                RailEntry::Group(index, group) => self.tool_group_button(ui, group, index),
                RailEntry::ZoomOut => {
                    if ui
                        .add(nf::IconButton::new(i20::ZOOMOUT, "Zoom out"))
                        .clicked()
                    {
                        self.zoom_out_stage();
                    }
                }
                RailEntry::Fit => {
                    if ui.add(nf::IconButton::new(i20::FIT, "Fit stage")).clicked() {
                        self.fit_stage_from_tools();
                    }
                }
            }
        }
        if shown < entries.len() {
            self.rail_overflow(ui, &entries[shown..]);
        }
    }

    /// "…" at the end of a short rail: a popover with every tool and command that did not fit.
    fn rail_overflow(&mut self, ui: &mut egui::Ui, hidden: &[RailEntry]) {
        let ctx = ui.ctx().clone();
        let open = popover(&ctx) == Some(RailPopover::Overflow);
        let response = ui.add(nf::IconButton::new(i20::MORE, "More tools").selected(open));
        if response.clicked() {
            set_popover(
                &ctx,
                if open {
                    None
                } else {
                    Some(RailPopover::Overflow)
                },
            );
        }
        if popover(&ctx) != Some(RailPopover::Overflow) {
            return;
        }
        let keep = rail_popover(ui, response.rect, |ui| {
            ui.set_min_width(220.);
            let mut done = false;
            let split = hidden
                .iter()
                .position(|e| matches!(e, RailEntry::ZoomOut | RailEntry::Fit))
                .unwrap_or(hidden.len());
            let mut tool_hidden = &hidden[..split];
            let view_hidden = &hidden[split..];
            let trailing_sep = !view_hidden.is_empty()
                && tool_hidden
                    .last()
                    .is_some_and(|e| matches!(e, RailEntry::Divider));
            if trailing_sep {
                tool_hidden = &tool_hidden[..tool_hidden.len() - 1];
            }
            if !tool_hidden.is_empty() {
                let max_h = (ui.ctx().content_rect().height() - 120.).max(80.);
                egui::ScrollArea::vertical()
                    .id_salt("rail_overflow_tools")
                    .max_height(max_h)
                    .themed()
                    .show(ui, |ui| {
                        for entry in tool_hidden {
                            match *entry {
                                RailEntry::Divider => nf::menu_separator(ui),
                                RailEntry::Group(index, group) => {
                                    for spec in group {
                                        let item = nf::MenuItem::new(spec.name)
                                            .icon(spec.icon.nf2(false))
                                            .shortcut(spec.key)
                                            .checked(self.tool == spec.tool);
                                        if ui.add(item).clicked() {
                                            self.set_tool(spec.tool);
                                            remember_group_tool(ui.ctx(), index, group, spec.tool);
                                            done = true;
                                        }
                                    }
                                }
                                RailEntry::ZoomOut | RailEntry::Fit => {}
                            }
                        }
                    });
            }
            if !tool_hidden.is_empty() && !view_hidden.is_empty() && trailing_sep {
                nf::menu_separator(ui);
            }
            for entry in view_hidden {
                match *entry {
                    RailEntry::ZoomOut => {
                        if ui
                            .add(nf::MenuItem::new("Zoom out").icon(Icon::ZoomOut.nf2(false)))
                            .clicked()
                        {
                            self.zoom_out_stage();
                            done = true;
                        }
                    }
                    RailEntry::Fit => {
                        if ui
                            .add(nf::MenuItem::new("Fit stage").icon(Icon::Fit.nf2(false)))
                            .clicked()
                        {
                            self.fit_stage_from_tools();
                            done = true;
                        }
                    }
                    _ => {}
                }
            }
            done
        });
        if !keep {
            set_popover(&ctx, None);
        }
    }

    fn zoom_out_stage(&mut self) {
        self.finish_edits();
        self.zoom = (self.zoom / 1.25).max(0.1);
    }

    fn fit_stage_from_tools(&mut self) {
        self.finish_edits();
        self.zoom = 0.;
        self.pan = Vec2::ZERO;
    }

    fn tool_grid(&mut self, ui: &mut egui::Ui) {
        section_caption(ui, "Tools");
        for row in GRID.chunks(2) {
            tool_row(ui, |ui| {
                for spec in row {
                    self.single_tool_button(ui, *spec);
                }
            });
        }
        nf::divider(ui, false);
        section_caption(ui, "View");
        tool_row(ui, |ui| {
            for spec in VIEW {
                self.single_tool_button(ui, *spec);
            }
        });
    }

    /// Zoom out and Fit stay reachable here even when the stage zoom control is hidden.
    fn view_commands(&mut self, ui: &mut egui::Ui) {
        let mut zoom_out = false;
        let mut fit = false;
        tool_row(ui, |ui| {
            zoom_out = ui
                .add(nf::IconButton::new(i20::ZOOMOUT, "Zoom out"))
                .clicked();
            fit = ui.add(nf::IconButton::new(i20::FIT, "Fit stage")).clicked();
        });
        if zoom_out {
            self.zoom_out_stage();
        }
        if fit {
            self.fit_stage_from_tools();
        }
    }

    fn single_tool_button(&mut self, ui: &mut egui::Ui, spec: ToolSpec) {
        let response = tool_button(ui, spec, self.tool == spec.tool, false);
        if response.clicked() {
            self.set_tool(spec.tool);
        }
    }

    /// One rail button for a tool group: shows the group's active (or last used) tool.
    fn tool_group_button(&mut self, ui: &mut egui::Ui, group: &[ToolSpec], index: usize) {
        let ctx = ui.ctx().clone();
        let memory = group_memory(index);
        let mut current = ctx.data(|d| d.get_temp::<usize>(memory)).unwrap_or(0);
        if let Some(active) = group.iter().position(|spec| spec.tool == self.tool) {
            current = active;
            ctx.data_mut(|d| d.insert_temp(memory, current));
        }
        let shown = group[current.min(group.len() - 1)];
        let flyout = group.len() > 1;
        let response = tool_button(ui, shown, self.tool == shown.tool, flyout);
        if response.clicked() {
            self.set_tool(shown.tool);
        }
        if !flyout {
            return;
        }
        let open_now = response.secondary_clicked()
            || (response.has_focus() && ui.input(|i| i.key_pressed(Key::ArrowRight)))
            || long_press(ui, &response);
        if open_now {
            set_popover(&ctx, Some(RailPopover::Flyout(index)));
        }
        if popover(&ctx) != Some(RailPopover::Flyout(index)) {
            return;
        }
        let mut chosen = None;
        let keep = rail_popover(ui, response.rect, |ui| {
            ui.set_min_width(200.);
            for (i, spec) in group.iter().enumerate() {
                let item = nf::MenuItem::new(spec.name)
                    .icon(spec.icon.nf2(false))
                    .shortcut(spec.key)
                    .checked(self.tool == spec.tool);
                if ui.add(item).clicked() {
                    chosen = Some(i);
                }
            }
            chosen.is_some()
        });
        if let Some(i) = chosen {
            remember_group_tool(&ctx, index, group, group[i].tool);
            self.set_tool(group[i].tool);
        }
        if !keep {
            set_popover(&ctx, None);
        }
    }

    /// Black-and-white and swap, under the Fill Stroke Chip.
    fn color_actions(&mut self, ui: &mut egui::Ui) {
        let (reset, swap) = ui
            .allocate_ui_with_layout(
                Vec2::new(2. * COLOR_ACTION + TOOL_GAP, COLOR_ACTION),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = TOOL_GAP;
                    let reset = ui
                        .add(
                            nf::IconButton::new(i20::COLOR_DEFAULT, "Black and white")
                                .side(COLOR_ACTION),
                        )
                        .clicked();
                    let swap = ui
                        .add(nf::IconButton::new(i20::COLOR_SWAP, "Swap colors").side(COLOR_ACTION))
                        .clicked();
                    (reset, swap)
                },
            )
            .inner;
        if reset {
            self.stroke = [0, 0, 0, 255];
            self.fill = [255, 255, 255, 255];
        }
        if swap {
            std::mem::swap(&mut self.fill, &mut self.stroke);
        }
    }

    /// `NF2/Fill Stroke Chip`: click a square to open the `NF2/Swatches Popover` (§6) for that tool
    /// colour. Picking a swatch or editing hex/alpha changes the tool colour; the colour wheel opens the
    /// Color panel (full mixer), the eyedropper selects the Eyedropper tool, and Add swatch / gradients
    /// open their planned previews.
    fn fill_stroke_chip(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let rgb = |c: [u8; 4]| egui::Color32::from_rgb(c[0], c[1], c[2]);
        let chip = nf::FillStrokeChip {
            fill: Some(rgb(self.fill)),
            stroke: Some(rgb(self.stroke)),
        }
        .show(ui);
        if chip.fill_clicked {
            set_popover(&ctx, Some(RailPopover::Fill));
        } else if chip.stroke_clicked {
            set_popover(&ctx, Some(RailPopover::Stroke));
        }
        let fill = match popover(&ctx) {
            Some(RailPopover::Fill) => true,
            Some(RailPopover::Stroke) => false,
            _ => return,
        };
        let recent_id = egui::Id::new("tools-recent-colours");
        let mut recent: Vec<egui::Color32> =
            ctx.data(|d| d.get_temp(recent_id)).unwrap_or_default();
        let palette = nf::default_palette();
        let pick = |i: usize| palette[i % palette.len()];
        let gradients = [
            nf::GradientSwatch {
                from: pick(0),
                to: pick(11),
                radial: false,
            },
            nf::GradientSwatch {
                from: pick(67),
                to: pick(91),
                radial: false,
            },
            nf::GradientSwatch {
                from: pick(48),
                to: pick(84),
                radial: true,
            },
        ];
        let document = [rgb(self.fill), rgb(self.stroke)];
        let (mut eyedropper, mut mixer) = (false, false);
        let mut chosen_gradient: Option<(
            crate::authoring_modify::PaintFillType,
            neoflash_core::Color,
            neoflash_core::Color,
        )> = None;
        let keep = rail_popover(ui, chip.response.rect, |ui| {
            ui.set_width(300.);
            nf::menu_section(ui, if fill { "Fill colour" } else { "Stroke colour" });
            let color = if fill {
                &mut self.fill
            } else {
                &mut self.stroke
            };
            let [r, g, b, a] = *color;
            let mut value = egui::Color32::from_rgba_unmultiplied(r, g, b, a);
            let before = value;
            let out = nf::SwatchesPopover {
                color: &mut value,
                recent: &recent,
                document: &document,
                palette: &palette,
                gradients: &gradients,
            }
            .show(ui);
            if value != before {
                *color = value.to_srgba_unmultiplied();
            }
            if out.picked {
                let opaque = egui::Color32::from_rgb(value.r(), value.g(), value.b());
                recent.retain(|c| *c != opaque);
                recent.insert(0, opaque);
                recent.truncate(nf::swatches::PALETTE_COLUMNS);
            }
            if out.no_colour {
                color[3] = 0;
            }
            eyedropper = out.eyedropper;
            mixer = out.open_mixer;
            if out.add_swatch {
                Editor::open_planned(ui.ctx(), "Swatches popover ▸ Add swatch");
            }
            if let Some(swatch) = out.gradient {
                let mode = if swatch.radial {
                    crate::authoring_modify::PaintFillType::RadialGradient
                } else {
                    crate::authoring_modify::PaintFillType::LinearGradient
                };
                chosen_gradient = Some((
                    mode,
                    swatch.from.to_srgba_unmultiplied(),
                    swatch.to.to_srgba_unmultiplied(),
                ));
            }
            out.picked
                || out.no_colour
                || out.eyedropper
                || out.open_mixer
                || out.gradient.is_some()
        });
        ctx.data_mut(|d| d.insert_temp(recent_id, recent));
        if let Some((mode, from, to)) = chosen_gradient {
            let stroke_well = !fill;
            ctx.data_mut(|d| {
                d.insert_temp(egui::Id::new("color-panel-active-stroke"), stroke_well);
            });
            let stops = vec![
                neoflash_core::GradientStop {
                    ratio: 0,
                    color: from,
                },
                neoflash_core::GradientStop {
                    ratio: 255,
                    color: to,
                },
            ];
            if !self.selection.targets.is_empty() {
                self.authoring_modify.color_selection = true;
                self.set_selection_fill_type(stroke_well, mode);
                self.set_selection_gradient_stops(stroke_well, stops, None);
            } else {
                self.authoring_modify.color_selection = false;
                self.set_drawing_fill_type(stroke_well, mode);
                self.set_selection_gradient_stops(stroke_well, stops, None);
            }
            self.show_panel(PanelId::Color);
        }
        // Eyedropper and colour wheel hand off to the tool / the Color panel's mixer.
        if eyedropper {
            self.set_tool(Tool::Eyedropper);
        }
        if mixer {
            self.show_panel(PanelId::Color);
        }
        if !keep {
            set_popover(&ctx, None);
        }
    }

    /// Settings button opening the active tool's options in a popover.
    fn tool_options_button(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let open = popover(&ctx) == Some(RailPopover::Options);
        let response = ui.add(nf::IconButton::new(i20::SETTINGS, "Tool options").selected(open));
        if response.clicked() {
            set_popover(
                &ctx,
                if open {
                    None
                } else {
                    Some(RailPopover::Options)
                },
            );
        }
        if popover(&ctx) != Some(RailPopover::Options) {
            return;
        }
        let keep = rail_popover(ui, response.rect, |ui| {
            ui.set_width(240.);
            ui.spacing_mut().item_spacing = Vec2::splat(space::S2);
            self.tool_options_body(ui)
        });
        if !keep {
            set_popover(&ctx, None);
        }
    }

    /// Returns `true` when the user moved on to Tool properties (closes the popover).
    fn tool_options_body(&mut self, ui: &mut egui::Ui) -> bool {
        let p = neoflash_ui::palette(ui.ctx());
        nf::menu_section(ui, "Tool options");
        let top = ui.cursor().top();
        self.authoring_tool_options(ui);
        // §6 tool settings the engine does not support yet open their planned previews.
        let planned: &[&str] = match self.tool {
            Tool::Rectangle => &[
                "Tool options ▸ Rectangle primitive…",
                "Tool options ▸ Stroke style…",
            ],
            Tool::Ellipse => &[
                "Tool options ▸ Oval primitive…",
                "Tool options ▸ Stroke style…",
            ],
            Tool::Line | Tool::Pen | Tool::Pencil | Tool::PolyStar => {
                &["Tool options ▸ Stroke style…"]
            }
            Tool::Brush => &["Tool options ▸ Spray brush…", "Tool options ▸ Deco tool…"],
            Tool::Transform => &["Tool options ▸ Bone tool…"],
            _ => &[],
        };
        for path in planned {
            let label = path.rsplit(" ▸ ").next().unwrap_or(path);
            if ui.add(nf::Button::ghost(label)).clicked() {
                Editor::open_planned(ui.ctx(), path);
            }
        }
        if ui.cursor().top() <= top {
            ui.label(rich("No options for this tool.", TextStyle::Small).color(p.text_tertiary));
        }
        let more = ui
            .add(nf::Button::ghost("Tool properties…"))
            .on_hover_text("Tool properties: paint alpha, stroke width and tool options")
            .clicked();
        if more {
            ui.ctx()
                .data_mut(|data| data.insert_temp(egui::Id::new("properties-tool-tab"), true));
            self.show_panel(PanelId::Properties);
        }
        more
    }
}

/// Width from which a two-column Tools panel shows tool options inline instead of in a popover.
fn inline_options_width() -> f32 {
    metrics::INSPECTOR / 2.
}

/// One entry of the 1 Column rail.
#[derive(Clone, Copy)]
enum RailEntry {
    Group(usize, &'static [ToolSpec]),
    Divider,
    ZoomOut,
    Fit,
}

impl RailEntry {
    const BUTTON: f32 = metrics::TOOL_BUTTON + TOOL_GAP;
    fn height(self) -> f32 {
        match self {
            // `NF2/Divider` (horizontal): 1 pt line + S2 margin.
            Self::Divider => 1. + space::S2 + TOOL_GAP,
            _ => Self::BUTTON,
        }
    }
}

fn rail_entries() -> Vec<RailEntry> {
    let mut entries = Vec::new();
    let mut index = 0;
    for (section, groups) in RAIL.iter().enumerate() {
        if section > 0 {
            entries.push(RailEntry::Divider);
        }
        for group in groups.iter() {
            entries.push(RailEntry::Group(index, group));
            index += 1;
        }
    }
    entries.extend([RailEntry::ZoomOut, RailEntry::Fit]);
    entries
}

fn group_memory(index: usize) -> egui::Id {
    egui::Id::new(("nf2-tool-group", index))
}

/// The rail shows the last tool chosen from a group (Flash behaviour).
fn remember_group_tool(ctx: &egui::Context, index: usize, group: &[ToolSpec], tool: Tool) {
    if let Some(i) = group.iter().position(|spec| spec.tool == tool) {
        ctx.data_mut(|d| d.insert_temp(group_memory(index), i));
    }
}

fn popover(ctx: &egui::Context) -> Option<RailPopover> {
    ctx.data(|d| d.get_temp::<Option<RailPopover>>(popover_id()))
        .flatten()
}

fn set_popover(ctx: &egui::Context, value: Option<RailPopover>) {
    ctx.data_mut(|d| d.insert_temp(popover_id(), value));
}

/// `NF2/Tool Button` keeping the panel's accessible name ("Selection (V)") while the tooltip
/// shows "Selection  V".
fn tool_button(ui: &mut egui::Ui, spec: ToolSpec, selected: bool, flyout: bool) -> egui::Response {
    let response = ui.add(
        nf::ToolButton::new(spec.icon.nf2(true), spec.name)
            .key(spec.key)
            .selected(selected)
            .flyout(flyout),
    );
    let enabled = response.enabled();
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, spec.label);
        info.selected = Some(selected);
        info
    });
    response
}

/// Two tool buttons side by side, centred in the panel.
fn tool_row(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui_with_layout(
        Vec2::new(2. * metrics::TOOL_BUTTON + TOOL_GAP, metrics::TOOL_BUTTON),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = TOOL_GAP;
            content(ui)
        },
    );
}

/// Section caption of the 2 Columns layout (Tools · View · Colors · Options).
fn section_caption(ui: &mut egui::Ui, label: &str) {
    let p = neoflash_ui::palette(ui.ctx());
    ui.add_space(space::S1);
    ui.label(rich(label, TextStyle::Overline).color(p.text_secondary));
    ui.add_space(space::S1);
}

fn long_press(ui: &egui::Ui, response: &egui::Response) -> bool {
    if !response.is_pointer_button_down_on() {
        return false;
    }
    let (start, now) = ui.input(|i| (i.pointer.press_start_time(), i.time));
    let Some(start) = start else {
        return false;
    };
    let held = now - start;
    if held < LONG_PRESS {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(LONG_PRESS - held));
        return false;
    }
    true
}

/// NF2 popover beside a rail control. Returns `false` when it should close: `content` returned
/// `true`, Esc, or a press outside the popover, its anchor and any nested popup.
fn rail_popover(ui: &egui::Ui, anchor: Rect, content: impl FnOnce(&mut egui::Ui) -> bool) -> bool {
    let ctx = ui.ctx().clone();
    let on_right = anchor.center().x > ctx.content_rect().center().x;
    let (primary, alt) = if on_right {
        (egui::RectAlign::LEFT_START, egui::RectAlign::LEFT_END)
    } else {
        (egui::RectAlign::RIGHT_START, egui::RectAlign::RIGHT_END)
    };
    let shown = egui::Popup::new(
        popover_id().with("area"),
        ctx.clone(),
        anchor,
        ui.layer_id(),
    )
    .kind(egui::PopupKind::Menu)
    .open(true)
    .align(primary)
    .align_alternatives(&[alt])
    .gap(space::S2)
    .frame(nf::popover_frame(&ctx))
    .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
    .show(|ui| {
        menu_style(ui.style_mut());
        content(ui)
    });
    let Some(shown) = shown else {
        return false;
    };
    let popup = shown.response.rect;
    let outside = ctx.input(|i| {
        i.pointer.any_pressed()
            && i.pointer.interact_pos().is_some_and(|pos| {
                !popup.contains(pos)
                    && !anchor.contains(pos)
                    && ctx
                        .layer_id_at(pos)
                        .is_none_or(|layer| layer.order < egui::Order::Foreground)
            })
    });
    let escape = ctx.input(|i| i.key_pressed(Key::Escape));
    !(shown.inner || outside || escape)
}
