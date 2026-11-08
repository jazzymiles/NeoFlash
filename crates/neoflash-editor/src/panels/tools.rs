//! Tools panel — `NF2/Tools Panel`.
//!
//! The layout follows the available width, not a setting:
//!
//! * **2 Columns** (base layout, docked default 80 pt and floating): every tool visible, sections
//!   Tools · View · Colors · Options.
//! * **1 Column** (panel narrower than two tool buttons): related tools share one `NF2/Tool Button`
//!   with a flyout (corner-mark click, right-click, long-press or → opens the group), groups separated by
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
use neoflash_ui::tokens::{TextStyle, space};
use neoflash_ui::typography::rich;

/// Gap between tool buttons (`NF2/Tools Panel` tool grid gap).
const TOOL_GAP: f32 = space::S1 / 2.0;
/// Hold time that opens a tool group (§13.4: long-press opens the flyout).
const LONG_PRESS: f64 = 0.45;
/// Width of the colour-action icon buttons under the Fill Stroke Chip (outline `Color actions`).
const COLOR_ACTION: f32 = 20.0;

#[derive(Clone, Copy)]
pub(crate) struct ToolSpec {
    pub(crate) tool: Tool,
    pub(crate) icon: Icon,
    /// Tooltip name.
    pub(crate) name: &'static str,
    /// Tooltip shortcut chip.
    pub(crate) key: &'static str,
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

/// 2 Columns grid, classic Flash order.
const GRID: &[ToolSpec] = &[
    SELECT, SUBSELECT, LINE, LASSO, PEN, TEXT, OVAL, RECTANGLE, POLYSTAR, PENCIL, BRUSH, TRANSFORM,
    GRADIENT, INK, BUCKET, EYEDROPPER, ERASER,
];
const VIEW: &[ToolSpec] = &[HAND, ZOOM];

/// Popover owned by the Tools panel (at most one is open).
#[derive(Clone, Copy, PartialEq, Eq)]
enum RailPopover {
    Flyout(usize),
    Overflow,
    Fill,
    Stroke,
    PencilMode,
    BrushMode,
    BrushSize,
    BrushShape,
    GapSize,
    EraserMode,
    EraserSize,
}

fn popover_id() -> egui::Id {
    egui::Id::new("nf2-tools-popover")
}

pub(crate) fn tool_columns_for_width(available_width: f32) -> usize {
    let side = neoflash_ui::metrics::tool_button();
    ((available_width + TOOL_GAP) / (side + TOOL_GAP))
        .floor()
        .max(1.0) as usize
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
            let cols = tool_columns_for_width(ui.available_width());
            if cols >= 2 {
                let panel_top = ui.cursor().top().max(0.0);
                let mut content_h = 0.0_f32;
                egui::ScrollArea::vertical()
                    .id_salt("tool_controls")
                    .auto_shrink([false, true])
                    .themed()
                    .show(ui, |ui| {
                        let inner_top = ui.cursor().top();
                        ui.vertical_centered(|ui| {
                            self.tool_grid(ui, cols);
                            nf::divider(ui, false);
                            section_caption(ui, "Colors");
                            self.colors_section(ui, cols);
                            nf::divider(ui, false);
                            section_caption(ui, "Options");
                            self.tool_modifiers_ui(ui, cols);
                        });
                        content_h = (ui.cursor().top() - inner_top).max(0.0);
                    });
                self.detached.tools_desired_inner_height =
                    Some((panel_top + content_h + space::S2).ceil());
                return;
            }
            self.detached.tools_desired_inner_height = None;
            // Rail: paint colours and snap modifier stay pinned while only the tools scroll.
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                ui.add_space(space::S1);
                self.snap_to_objects_button(ui);
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
        let groups = self.tool_groups();
        let entries = rail_entries(
            &groups,
            self.workspace.tools.slots == crate::tool_preferences::default_slots(),
        );
        let available = ui.available_height();
        let total: f32 = entries.iter().map(|entry| entry.height()).sum();
        let shown = if total <= available {
            entries.len()
        } else {
            let budget = available - (neoflash_ui::metrics::tool_button() + TOOL_GAP);
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
            ui.set_min_width(260.);
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
                                        let key = self.tool_display_key(*spec);
                                        let label = format!("{} tool", spec.name);
                                        let mut item =
                                            nf::ToolFlyoutItem::new(spec.icon.nf2(true), &label)
                                                .active(self.tool == spec.tool)
                                                .width(260.);
                                        if !key.is_empty() {
                                            item = item.shortcut(&key);
                                        }
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
                    RailEntry::Fit
                        if ui
                            .add(nf::MenuItem::new("Fit stage").icon(Icon::Fit.nf2(false)))
                            .clicked() =>
                    {
                        self.fit_stage_from_tools();
                        done = true;
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

    fn tool_grid(&mut self, ui: &mut egui::Ui, cols: usize) {
        section_caption(ui, "Tools");
        let tools: Vec<_> = self
            .tool_groups()
            .into_iter()
            .flatten()
            .filter(|spec| !matches!(spec.tool, Tool::Hand | Tool::Zoom))
            .collect();
        for row in tools.chunks(cols) {
            tool_grid_row(ui, cols, row.len(), |ui, idx| {
                self.single_tool_button(ui, row[idx]);
            });
        }
        nf::divider(ui, false);
        section_caption(ui, "View");
        let view: Vec<_> = self
            .tool_groups()
            .into_iter()
            .flatten()
            .filter(|spec| matches!(spec.tool, Tool::Hand | Tool::Zoom))
            .collect();
        let total_view = view.len() + 2;
        let mut zoom_out = false;
        let mut fit = false;
        let indices: Vec<usize> = (0..total_view).collect();
        for row in indices.chunks(cols) {
            tool_grid_row(ui, cols, row.len(), |ui, idx| {
                let item = row[idx];
                if let Some(&spec) = view.get(item) {
                    self.single_tool_button(ui, spec);
                } else if item == view.len() {
                    if ui
                        .add(nf::IconButton::new(i20::ZOOMOUT, "Zoom out"))
                        .clicked()
                    {
                        zoom_out = true;
                    }
                } else if ui.add(nf::IconButton::new(i20::FIT, "Fit stage")).clicked() {
                    fit = true;
                }
            });
        }
        if zoom_out {
            self.zoom_out_stage();
        }
        if fit {
            self.fit_stage_from_tools();
        }
    }

    fn colors_section(&mut self, ui: &mut egui::Ui, cols: usize) {
        if cols >= 3 {
            let chip_w = 36.0;
            let actions_w = 2.0 * COLOR_ACTION + TOOL_GAP;
            let row_w = chip_w + space::S2 + actions_w;
            let row_h = 36.0_f32.max(COLOR_ACTION);
            ui.allocate_ui_with_layout(
                Vec2::new(row_w, row_h),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = space::S2;
                    self.fill_stroke_chip(ui);
                    self.color_actions(ui);
                },
            );
        } else {
            self.fill_stroke_chip(ui);
            self.color_actions(ui);
        }
    }

    fn single_tool_button(&mut self, ui: &mut egui::Ui, spec: ToolSpec) {
        let response = self.tool_button(ui, spec, self.tool == spec.tool, false);
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
        let response = self.tool_button(ui, shown, self.tool == shown.tool, flyout);
        let mark_clicked = flyout
            && response.clicked()
            && response.interact_pointer_pos().is_some_and(|position| {
                nf::ToolButton::corner_hit(response.rect).contains(position)
            });
        if response.clicked() && !mark_clicked {
            self.set_tool(shown.tool);
        }
        if !flyout {
            return;
        }
        let open_now = mark_clicked
            || response.secondary_clicked()
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
            ui.set_min_width(260.);
            for (i, spec) in group.iter().enumerate() {
                let key = self.tool_display_key(*spec);
                let label = format!("{} tool", spec.name);
                let mut item = nf::ToolFlyoutItem::new(spec.icon.nf2(true), &label)
                    .active(i == current)
                    .width(260.);
                if !key.is_empty() {
                    item = item.shortcut(&key);
                }
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

    /// `NF2/Fill Stroke Chip`: click Fill or Stroke to open the full NF2 Color & Fill Type
    /// popover (Fill type selector `None / Solid colour / Linear gradient / Radial gradient /
    /// Bitmap fill`, 2D colour picker, RGB/HSB fields, interactive gradient stops, bitmap tiles,
    /// `Add to swatches`, `Eyedropper`, and collapsible `Swatches`).
    fn fill_stroke_chip(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let chip_color =
            |c: [u8; 4], grad: Option<&neoflash_core::GradientPaint>| -> Option<egui::Color32> {
                if let Some(g) = grad
                    && let Some(stop) = g.stops.first()
                {
                    return Some(egui::Color32::from_rgba_unmultiplied(
                        stop.color[0],
                        stop.color[1],
                        stop.color[2],
                        stop.color[3],
                    ));
                }
                (c[3] > 0).then(|| egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]))
            };
        let chip = nf::FillStrokeChip {
            fill: chip_color(
                self.fill,
                self.authoring_modify.drawing_fill_gradient.as_ref(),
            ),
            stroke: chip_color(
                self.stroke,
                self.authoring_modify.drawing_stroke_gradient.as_ref(),
            ),
        }
        .show(ui);
        if chip.fill_clicked {
            let next = if popover(&ctx) == Some(RailPopover::Fill) {
                None
            } else {
                Some(RailPopover::Fill)
            };
            ctx.data_mut(|d| d.insert_temp(egui::Id::new("color-panel-active-stroke"), false));
            self.authoring_modify.color_selection = !self.selection.targets.is_empty();
            set_popover(&ctx, next);
        } else if chip.stroke_clicked {
            let next = if popover(&ctx) == Some(RailPopover::Stroke) {
                None
            } else {
                Some(RailPopover::Stroke)
            };
            ctx.data_mut(|d| d.insert_temp(egui::Id::new("color-panel-active-stroke"), true));
            self.authoring_modify.color_selection = !self.selection.targets.is_empty();
            set_popover(&ctx, next);
        }
        let stroke_well = match popover(&ctx) {
            Some(RailPopover::Fill) => false,
            Some(RailPopover::Stroke) => true,
            _ => return,
        };
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("color-panel-active-stroke"), stroke_well));
        let keep = rail_popover(ui, chip.response.rect, |ui| {
            self.color_fill_popover_contents(ui)
        });
        if !keep {
            set_popover(&ctx, None);
        }
    }

    fn snap_to_objects_button(&mut self, ui: &mut egui::Ui) {
        let snap_on = self.stage_view.prefs.objects_snap;
        if ui
            .add(nf::IconButton::new(i20::TOOL_SNAP, "Snap to objects").selected(snap_on))
            .clicked()
        {
            self.toggle_object_snap();
        }
    }

    fn object_drawing_modifier_button(&mut self, ui: &mut egui::Ui) {
        use crate::ui_icons::{ToolModifierGlyph, tool_modifier_button};
        let active = self.workspace.drawing.object_drawing;
        if tool_modifier_button(
            ui,
            ToolModifierGlyph::ObjectDrawing,
            active,
            true,
            "Object drawing (J)",
        )
        .clicked()
        {
            self.workspace.drawing.object_drawing = !active;
        }
    }

    fn lock_fill_modifier_button(&mut self, ui: &mut egui::Ui) {
        use neoflash_ui::icons::i16;
        let locked = self.authoring_tools.brush_lock_fill;
        if ui
            .add(nf::IconButton::new(i16::LOCK_FILL, "Lock fill").selected(locked))
            .clicked()
        {
            self.authoring_tools.brush_lock_fill = !locked;
        }
    }

    fn pencil_mode_modifier_button(&mut self, ui: &mut egui::Ui) {
        use crate::tool_preferences::PencilMode;
        use neoflash_ui::icons::i16;
        let ctx = ui.ctx().clone();
        let open = popover(&ctx) == Some(RailPopover::PencilMode);
        let icon = match self.workspace.tools.pencil_mode {
            PencilMode::Straighten => i16::PENCIL_STRAIGHTEN,
            PencilMode::Smooth => i16::PENCIL_SMOOTH,
            PencilMode::Ink => i16::PENCIL_INK,
        };
        let response = ui.add(nf::IconButton::new(icon, "Pencil mode").selected(open));
        if response.clicked() {
            set_popover(&ctx, (!open).then_some(RailPopover::PencilMode));
        }
        if popover(&ctx) != Some(RailPopover::PencilMode) {
            return;
        }
        let mut chosen = None;
        let keep = rail_popover(ui, response.rect, |ui| {
            ui.set_min_width(180.);
            for (mode, mode_icon, label) in [
                (PencilMode::Straighten, i16::PENCIL_STRAIGHTEN, "Straighten"),
                (PencilMode::Smooth, i16::PENCIL_SMOOTH, "Smooth"),
                (PencilMode::Ink, i16::PENCIL_INK, "Ink"),
            ] {
                if ui
                    .add(
                        nf::MenuItem::new(label)
                            .icon(mode_icon)
                            .checked(self.workspace.tools.pencil_mode == mode),
                    )
                    .clicked()
                {
                    chosen = Some(mode);
                }
            }
            chosen.is_some()
        });
        if let Some(mode) = chosen {
            self.workspace.tools.pencil_mode = mode;
        }
        if !keep {
            set_popover(&ctx, None);
        }
    }

    fn brush_mode_modifier_button(&mut self, ui: &mut egui::Ui) {
        use crate::ui_icons::{ToolModifierGlyph, tool_modifier_button};
        use neoflash_core::BrushPaintMode;
        let ctx = ui.ctx().clone();
        let open = popover(&ctx) == Some(RailPopover::BrushMode);
        let response = tool_modifier_button(
            ui,
            ToolModifierGlyph::BrushMode(self.authoring_tools.brush_paint_mode),
            open,
            true,
            "Brush mode",
        );
        if response.clicked() {
            set_popover(&ctx, (!open).then_some(RailPopover::BrushMode));
        }
        if popover(&ctx) != Some(RailPopover::BrushMode) {
            return;
        }
        let mut chosen = None;
        let keep = rail_popover(ui, response.rect, |ui| {
            ui.set_min_width(180.);
            for (mode, label) in [
                (BrushPaintMode::PaintNormal, "Paint normal"),
                (BrushPaintMode::PaintFills, "Paint fills"),
                (BrushPaintMode::PaintBehind, "Paint behind"),
                (BrushPaintMode::PaintSelection, "Paint selection"),
                (BrushPaintMode::PaintInside, "Paint inside"),
            ] {
                if ui
                    .add(
                        nf::MenuItem::new(label)
                            .checked(self.authoring_tools.brush_paint_mode == mode),
                    )
                    .clicked()
                {
                    chosen = Some(mode);
                }
            }
            chosen.is_some()
        });
        if let Some(mode) = chosen {
            self.authoring_tools.brush_paint_mode = mode;
        }
        if !keep {
            set_popover(&ctx, None);
        }
    }

    fn brush_size_modifier_button(&mut self, ui: &mut egui::Ui) {
        use crate::ui_icons::{ToolModifierGlyph, tool_modifier_button};
        let ctx = ui.ctx().clone();
        let open = popover(&ctx) == Some(RailPopover::BrushSize);
        let response = tool_modifier_button(
            ui,
            ToolModifierGlyph::BrushSize(self.authoring_tools.brush_size),
            open,
            true,
            "Brush size",
        );
        if response.clicked() {
            set_popover(&ctx, (!open).then_some(RailPopover::BrushSize));
        }
        if popover(&ctx) != Some(RailPopover::BrushSize) {
            return;
        }
        let mut chosen = None;
        let keep = rail_popover(ui, response.rect, |ui| {
            ui.set_min_width(160.);
            for preset in [2.0_f32, 4.0, 8.0, 12.0, 18.0, 24.0, 36.0, 48.0] {
                let is_cur = (self.authoring_tools.brush_size - preset).abs() < 0.5;
                if ui
                    .add(nf::MenuItem::new(&format!("{preset:.0} px")).checked(is_cur))
                    .clicked()
                {
                    chosen = Some(preset);
                }
            }
            chosen.is_some()
        });
        if let Some(size) = chosen {
            self.authoring_tools.brush_size = size;
        }
        if !keep {
            set_popover(&ctx, None);
        }
    }

    fn brush_shape_modifier_button(&mut self, ui: &mut egui::Ui) {
        use crate::ui_icons::{ToolModifierGlyph, tool_modifier_button};
        use neoflash_ui::components::BrushShapeKind;
        let ctx = ui.ctx().clone();
        let open = popover(&ctx) == Some(RailPopover::BrushShape);
        let response = tool_modifier_button(
            ui,
            ToolModifierGlyph::BrushShape(self.authoring_tools.brush_shape),
            open,
            true,
            "Brush shape",
        );
        if response.clicked() {
            set_popover(&ctx, (!open).then_some(RailPopover::BrushShape));
        }
        if popover(&ctx) != Some(RailPopover::BrushShape) {
            return;
        }
        let mut chosen = None;
        let keep = rail_popover(ui, response.rect, |ui| {
            ui.set_min_width(180.);
            for shape in BrushShapeKind::ALL {
                if ui
                    .add(
                        nf::MenuItem::new(shape.label())
                            .checked(self.authoring_tools.brush_shape == shape),
                    )
                    .clicked()
                {
                    chosen = Some(shape);
                }
            }
            chosen.is_some()
        });
        if let Some(shape) = chosen {
            self.authoring_tools.brush_shape = shape;
        }
        if !keep {
            set_popover(&ctx, None);
        }
    }

    fn gap_size_modifier_button(&mut self, ui: &mut egui::Ui) {
        use crate::authoring_tools::GapCloseMode;
        use crate::ui_icons::{ToolModifierGlyph, tool_modifier_button};
        let ctx = ui.ctx().clone();
        let open = popover(&ctx) == Some(RailPopover::GapSize);
        let response = tool_modifier_button(
            ui,
            ToolModifierGlyph::GapSize(self.authoring_tools.gap_close),
            open,
            true,
            "Gap size",
        );
        if response.clicked() {
            set_popover(&ctx, (!open).then_some(RailPopover::GapSize));
        }
        if popover(&ctx) != Some(RailPopover::GapSize) {
            return;
        }
        let mut chosen = None;
        let keep = rail_popover(ui, response.rect, |ui| {
            ui.set_min_width(180.);
            for (mode, label) in [
                (GapCloseMode::None, "Don’t close gaps"),
                (GapCloseMode::Small, "Close small gaps"),
                (GapCloseMode::Medium, "Close medium gaps"),
                (GapCloseMode::Large, "Close large gaps"),
            ] {
                if ui
                    .add(nf::MenuItem::new(label).checked(self.authoring_tools.gap_close == mode))
                    .clicked()
                {
                    chosen = Some(mode);
                }
            }
            chosen.is_some()
        });
        if let Some(mode) = chosen {
            self.authoring_tools.gap_close = mode;
        }
        if !keep {
            set_popover(&ctx, None);
        }
    }

    fn eraser_mode_modifier_button(&mut self, ui: &mut egui::Ui) {
        use crate::authoring_tools::EraserMode;
        use crate::ui_icons::{ToolModifierGlyph, tool_modifier_button};
        let ctx = ui.ctx().clone();
        let open = popover(&ctx) == Some(RailPopover::EraserMode);
        let response = tool_modifier_button(
            ui,
            ToolModifierGlyph::EraserMode(self.authoring_tools.eraser_mode),
            open,
            true,
            "Eraser mode",
        );
        if response.clicked() {
            set_popover(&ctx, (!open).then_some(RailPopover::EraserMode));
        }
        if popover(&ctx) != Some(RailPopover::EraserMode) {
            return;
        }
        let mut chosen = None;
        let keep = rail_popover(ui, response.rect, |ui| {
            ui.set_min_width(180.);
            for (mode, label) in [
                (EraserMode::StrokeSegments, "Erase normal"),
                (EraserMode::Fills, "Erase fills"),
                (EraserMode::Strokes, "Erase lines"),
                (EraserMode::Objects, "Erase whole objects"),
            ] {
                if ui
                    .add(nf::MenuItem::new(label).checked(self.authoring_tools.eraser_mode == mode))
                    .clicked()
                {
                    chosen = Some(mode);
                }
            }
            chosen.is_some()
        });
        if let Some(mode) = chosen {
            self.authoring_tools.eraser_mode = mode;
        }
        if !keep {
            set_popover(&ctx, None);
        }
    }

    fn eraser_size_modifier_button(&mut self, ui: &mut egui::Ui) {
        use crate::ui_icons::{ToolModifierGlyph, tool_modifier_button};
        let ctx = ui.ctx().clone();
        let open = popover(&ctx) == Some(RailPopover::EraserSize);
        let response = tool_modifier_button(
            ui,
            ToolModifierGlyph::EraserSize(self.authoring_tools.eraser_size),
            open,
            true,
            "Eraser size",
        );
        if response.clicked() {
            set_popover(&ctx, (!open).then_some(RailPopover::EraserSize));
        }
        if popover(&ctx) != Some(RailPopover::EraserSize) {
            return;
        }
        let mut chosen = None;
        let keep = rail_popover(ui, response.rect, |ui| {
            ui.set_min_width(160.);
            for preset in [4.0_f32, 8.0, 12.0, 18.0, 24.0, 36.0, 48.0] {
                let is_cur = (self.authoring_tools.eraser_size - preset).abs() < 0.5;
                if ui
                    .add(nf::MenuItem::new(&format!("{preset:.0} px")).checked(is_cur))
                    .clicked()
                {
                    chosen = Some(preset);
                }
            }
            chosen.is_some()
        });
        if let Some(size) = chosen {
            self.authoring_tools.eraser_size = size;
        }
        if !keep {
            set_popover(&ctx, None);
        }
    }

    /// Classic Flash per-tool icon modifiers under `Options` in the multi-column Tools panel.
    fn tool_modifiers_ui(&mut self, ui: &mut egui::Ui, cols: usize) {
        use crate::authoring_tools::{EraserMode, FreeTransformMode};
        use crate::ui_icons::{
            ToolModifierGlyph, TransformModifierIcon, tool_modifier_button,
            transform_modifier_button,
        };
        use neoflash_ui::icons::i16;

        match self.tool {
            Tool::Select => {
                let can_shape = self.can_smooth_or_straighten_selection();
                if cols == 2 {
                    tool_grid_row(ui, cols, 1, |ui, _| {
                        self.snap_to_objects_button(ui);
                    });
                    tool_grid_row(ui, cols, 2, |ui, idx| match idx {
                        0 => {
                            if ui
                                .add_enabled(
                                    can_shape,
                                    nf::IconButton::new(i20::TOOL_SMOOTH, "Smooth"),
                                )
                                .clicked()
                            {
                                self.smooth_selection();
                            }
                        }
                        _ => {
                            if ui
                                .add_enabled(
                                    can_shape,
                                    nf::IconButton::new(i20::TOOL_STRAIGHTEN, "Straighten"),
                                )
                                .clicked()
                            {
                                self.straighten_selection();
                            }
                        }
                    });
                } else {
                    let items = [0_usize, 1, 2];
                    for row in items.chunks(cols) {
                        tool_grid_row(ui, cols, row.len(), |ui, idx| match row[idx] {
                            0 => self.snap_to_objects_button(ui),
                            1 => {
                                if ui
                                    .add_enabled(
                                        can_shape,
                                        nf::IconButton::new(i20::TOOL_SMOOTH, "Smooth"),
                                    )
                                    .clicked()
                                {
                                    self.smooth_selection();
                                }
                            }
                            _ => {
                                if ui
                                    .add_enabled(
                                        can_shape,
                                        nf::IconButton::new(i20::TOOL_STRAIGHTEN, "Straighten"),
                                    )
                                    .clicked()
                                {
                                    self.straighten_selection();
                                }
                            }
                        });
                    }
                }
            }
            Tool::Transform => {
                let active = self.active_free_transform_mode();
                let can_deform = self.can_distort_or_envelope_selection();
                let render_item = |s: &mut Self, ui: &mut egui::Ui, item: usize| match item {
                    0 => s.snap_to_objects_button(ui),
                    1 => {
                        if transform_modifier_button(
                            ui,
                            TransformModifierIcon::RotateAndSkew,
                            active == FreeTransformMode::RotateAndSkew,
                            true,
                            "Rotate and Skew",
                        )
                        .clicked()
                        {
                            let next = if active == FreeTransformMode::RotateAndSkew {
                                FreeTransformMode::FreeTransform
                            } else {
                                FreeTransformMode::RotateAndSkew
                            };
                            s.set_free_transform_mode(next);
                        }
                    }
                    2 => {
                        if transform_modifier_button(
                            ui,
                            TransformModifierIcon::Scale,
                            active == FreeTransformMode::Scale,
                            true,
                            "Scale",
                        )
                        .clicked()
                        {
                            let next = if active == FreeTransformMode::Scale {
                                FreeTransformMode::FreeTransform
                            } else {
                                FreeTransformMode::Scale
                            };
                            s.set_free_transform_mode(next);
                        }
                    }
                    3 => {
                        if transform_modifier_button(
                            ui,
                            TransformModifierIcon::Distort,
                            active == FreeTransformMode::Distort,
                            can_deform,
                            "Distort",
                        )
                        .clicked()
                        {
                            let next = if active == FreeTransformMode::Distort {
                                FreeTransformMode::FreeTransform
                            } else {
                                FreeTransformMode::Distort
                            };
                            s.set_free_transform_mode(next);
                        }
                    }
                    _ => {
                        if transform_modifier_button(
                            ui,
                            TransformModifierIcon::Envelope,
                            active == FreeTransformMode::Envelope,
                            can_deform,
                            "Envelope",
                        )
                        .clicked()
                        {
                            let next = if active == FreeTransformMode::Envelope {
                                FreeTransformMode::FreeTransform
                            } else {
                                FreeTransformMode::Envelope
                            };
                            s.set_free_transform_mode(next);
                        }
                    }
                };
                if cols == 2 {
                    tool_grid_row(ui, cols, 1, |ui, _| render_item(self, ui, 0));
                    tool_grid_row(ui, cols, 2, |ui, idx| render_item(self, ui, 1 + idx));
                    tool_grid_row(ui, cols, 2, |ui, idx| render_item(self, ui, 3 + idx));
                } else {
                    let items = [0_usize, 1, 2, 3, 4];
                    for row in items.chunks(cols) {
                        tool_grid_row(ui, cols, row.len(), |ui, idx| {
                            render_item(self, ui, row[idx]);
                        });
                    }
                }
            }
            Tool::Line | Tool::Pen | Tool::Rectangle | Tool::Ellipse => {
                tool_grid_row(ui, cols, 2, |ui, idx| match idx {
                    0 => self.object_drawing_modifier_button(ui),
                    _ => self.snap_to_objects_button(ui),
                });
            }
            Tool::PolyStar => {
                let items = [0_usize, 1, 2];
                for row in items.chunks(cols) {
                    tool_grid_row(ui, cols, row.len(), |ui, idx| match row[idx] {
                        0 => self.object_drawing_modifier_button(ui),
                        1 => self.snap_to_objects_button(ui),
                        _ => {
                            if ui
                                .add(nf::IconButton::new(
                                    i16::SETTINGS,
                                    "PolyStar tool settings…",
                                ))
                                .clicked()
                            {
                                self.open_polystar_settings();
                            }
                        }
                    });
                }
            }
            Tool::Pencil => {
                tool_grid_row(ui, cols, 2, |ui, idx| match idx {
                    0 => self.object_drawing_modifier_button(ui),
                    _ => self.pencil_mode_modifier_button(ui),
                });
            }
            Tool::Brush => {
                let items = [0_usize, 1, 2, 3, 4, 5, 6];
                for row in items.chunks(cols) {
                    tool_grid_row(ui, cols, row.len(), |ui, idx| match row[idx] {
                        0 => self.object_drawing_modifier_button(ui),
                        1 => self.lock_fill_modifier_button(ui),
                        2 => self.brush_mode_modifier_button(ui),
                        3 => self.brush_size_modifier_button(ui),
                        4 => self.brush_shape_modifier_button(ui),
                        5 => {
                            let on = self.authoring_tools.brush_pressure;
                            if ui
                                .add(
                                    nf::IconButton::new(i16::PRESSURE, "Use pressure").selected(on),
                                )
                                .clicked()
                            {
                                self.authoring_tools.brush_pressure = !on;
                            }
                        }
                        _ => {
                            let on = self.authoring_tools.brush_tilt;
                            if ui
                                .add(nf::IconButton::new(i16::TILT, "Use tilt").selected(on))
                                .clicked()
                            {
                                self.authoring_tools.brush_tilt = !on;
                            }
                        }
                    });
                }
            }
            Tool::Bucket => {
                tool_grid_row(ui, cols, 2, |ui, idx| match idx {
                    0 => self.gap_size_modifier_button(ui),
                    _ => self.lock_fill_modifier_button(ui),
                });
            }
            Tool::Eraser => {
                let items = [0_usize, 1, 2];
                for row in items.chunks(cols) {
                    tool_grid_row(ui, cols, row.len(), |ui, idx| match row[idx] {
                        0 => self.eraser_mode_modifier_button(ui),
                        1 => {
                            let faucet_on = self.authoring_tools.eraser_mode == EraserMode::Objects;
                            if tool_modifier_button(
                                ui,
                                ToolModifierGlyph::Faucet,
                                faucet_on,
                                true,
                                "Faucet",
                            )
                            .clicked()
                            {
                                self.authoring_tools.eraser_mode = if faucet_on {
                                    EraserMode::StrokeSegments
                                } else {
                                    EraserMode::Objects
                                };
                            }
                        }
                        _ => self.eraser_size_modifier_button(ui),
                    });
                }
            }
            Tool::Lasso => {
                tool_grid_row(ui, cols, 2, |ui, idx| match idx {
                    0 => {
                        let on = self.authoring_tools.lasso_contact;
                        if tool_modifier_button(
                            ui,
                            ToolModifierGlyph::TouchSelects,
                            on,
                            true,
                            "Touch selects",
                        )
                        .clicked()
                        {
                            self.authoring_tools.lasso_contact = !on;
                        }
                    }
                    _ => {
                        let on = self.authoring_tools.lasso_partial_fills;
                        if tool_modifier_button(
                            ui,
                            ToolModifierGlyph::SliceMergeFills,
                            on,
                            true,
                            "Slice merge fills",
                        )
                        .clicked()
                        {
                            self.authoring_tools.lasso_partial_fills = !on;
                        }
                    }
                });
            }
            Tool::Zoom => {
                let zoom_out = self.authoring_tools.zoom_out;
                tool_grid_row(ui, cols, 2, |ui, idx| match idx {
                    0 => {
                        if ui
                            .add(nf::IconButton::new(i20::ZOOMIN, "Enlarge").selected(!zoom_out))
                            .clicked()
                        {
                            self.authoring_tools.zoom_out = false;
                        }
                    }
                    _ => {
                        if ui
                            .add(nf::IconButton::new(i20::ZOOMOUT, "Reduce").selected(zoom_out))
                            .clicked()
                        {
                            self.authoring_tools.zoom_out = true;
                        }
                    }
                });
            }
            Tool::Subselect
            | Tool::Gradient
            | Tool::Text
            | Tool::Ink
            | Tool::Eyedropper
            | Tool::Hand => {}
        }
    }
}

/// One entry of the 1 Column rail.
#[derive(Clone, Copy)]
enum RailEntry<'a> {
    Group(usize, &'a [ToolSpec]),
    Divider,
    ZoomOut,
    Fit,
}

impl RailEntry<'_> {
    fn height(self) -> f32 {
        match self {
            // `NF2/Divider` (horizontal): 1 pt line + S2 margin.
            Self::Divider => 1. + space::S2 + TOOL_GAP,
            _ => neoflash_ui::metrics::tool_button() + TOOL_GAP,
        }
    }
}

fn rail_entries(groups: &[Vec<ToolSpec>], default_layout: bool) -> Vec<RailEntry<'_>> {
    let mut entries = Vec::new();
    for (index, group) in groups.iter().enumerate() {
        if default_layout && [3, 7, 10].contains(&index) {
            entries.push(RailEntry::Divider);
        }
        entries.push(RailEntry::Group(index, group));
    }
    entries.extend([RailEntry::ZoomOut, RailEntry::Fit]);
    entries
}

pub(crate) fn tool_catalogue() -> Vec<ToolSpec> {
    GRID.iter().chain(VIEW).copied().collect()
}
impl Editor {
    fn tool_groups(&self) -> Vec<Vec<ToolSpec>> {
        let catalogue = tool_catalogue();
        self.workspace
            .tools
            .slots
            .iter()
            .filter_map(|slot| {
                let group: Vec<_> = slot
                    .iter()
                    .filter_map(|name| catalogue.iter().find(|s| s.name == name).copied())
                    .collect();
                (!group.is_empty()).then_some(group)
            })
            .collect()
    }
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

impl Editor {
    fn tool_display_key(&self, spec: ToolSpec) -> String {
        let cmd = crate::commands::CommandId::for_tool(spec.tool);
        if crate::commands::has_override(&self.workspace.shortcuts, cmd) {
            self.sc(cmd)
        } else {
            spec.key.to_owned()
        }
    }

    fn tool_accessible_label(&self, spec: ToolSpec, key: &str) -> String {
        let cmd = crate::commands::CommandId::for_tool(spec.tool);
        if !crate::commands::has_override(&self.workspace.shortcuts, cmd) {
            return spec.label.to_owned();
        }
        if spec.tool == Tool::Hand {
            if key.is_empty() {
                "Hand (hold Space)".to_owned()
            } else {
                format!("Hand ({key} / hold Space)")
            }
        } else if key.is_empty() {
            spec.name.to_owned()
        } else {
            format!("{} ({key})", spec.name)
        }
    }

    /// `NF2/Tool Button` keeping the panel's accessible name ("Selection (V)") while the tooltip
    /// shows "Selection  V" when tooltip shortcuts are enabled.
    fn tool_button(
        &self,
        ui: &mut egui::Ui,
        spec: ToolSpec,
        selected: bool,
        flyout: bool,
    ) -> egui::Response {
        let key = self.tool_display_key(spec);
        let a11y_label = self.tool_accessible_label(spec, &key);
        let mut btn = nf::ToolButton::new(spec.icon.tool_glyph(), spec.name)
            .selected(selected)
            .flyout(flyout);
        if self.workspace.general.tooltip_shortcuts && !key.is_empty() {
            btn = btn.key(&key);
        }
        let response = ui.add(btn);
        let enabled = response.enabled();
        response.widget_info(move || {
            let mut info =
                egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, &a11y_label);
            info.selected = Some(selected);
            info
        });
        response
    }
}

/// Lays out up to `cols` tool/modifier buttons on one row, distributing the `cols` column slots
/// evenly across the available panel width so partial rows stay aligned to the same columns.
fn tool_grid_row(
    ui: &mut egui::Ui,
    cols: usize,
    items: usize,
    mut render_item: impl FnMut(&mut egui::Ui, usize),
) {
    let cols = cols.max(1);
    let side = neoflash_ui::metrics::tool_button();
    let natural_w = cols as f32 * side + (cols.saturating_sub(1)) as f32 * TOOL_GAP;
    let avail_w = ui.available_width().max(natural_w);
    let extra = (avail_w - natural_w).max(0.0);
    let pad = extra / (cols + 1) as f32;
    let step = side + TOOL_GAP + pad;
    let (row_rect, _) = ui.allocate_exact_size(Vec2::new(avail_w, side), egui::Sense::hover());
    let start_x = row_rect.left() + pad;
    for idx in 0..items.min(cols) {
        let cell_rect = Rect::from_min_size(
            egui::pos2(start_x + idx as f32 * step, row_rect.top()),
            Vec2::splat(side),
        );
        let mut cell_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(cell_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        render_item(&mut cell_ui, idx);
    }
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
    // Never call back into `Context` while an input guard is held: egui's Context lock is not
    // re-entrant. The old code called `layer_id_at` inside `ctx.input`, deadlocking as soon as an
    // outside press reached this branch (for example Options, then right-click another tool).
    let (pressed, pointer_pos, escape) = ctx.input(|i| {
        (
            i.pointer.any_pressed(),
            i.pointer.interact_pos(),
            i.key_pressed(Key::Escape),
        )
    });
    let outside = pressed
        && pointer_pos.is_some_and(|pos| {
            !popup.contains(pos)
                && !anchor.contains(pos)
                && ctx
                    .layer_id_at(pos)
                    .is_none_or(|layer| layer.order < egui::Order::Foreground)
        });
    !(shown.inner || outside || escape)
}
