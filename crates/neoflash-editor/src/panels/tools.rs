//! Tools panel — `NF3 Panel / Tools`.
//!
//! One layout for every width: four blocks separated by 1 px dividers — tools · view · colours ·
//! options — each a grid of `tool_button()` cells centred in the panel. The column count follows the
//! available width (`tool_columns_for_width`); N columns need `tools_natural_width(N)`.
//!
//! * **2 or more columns**: every tool is a cell; the view block holds Hand, Zoom, Zoom out and Fit.
//! * **1 column** (rail): related tools share one cell with a flyout (corner-mark click, right-click,
//!   long-press or → opens the group); the view block holds Hand and Zoom, the colours block the
//!   two chips, the options block Snap.
//!
//! "⋯" closes the tools block — as its last cell, or as a low full-width bar under the grid when
//! the tools fill their rows and the cell would start a row of its own (two or more columns). It
//! opens the palette menu: the tools and commands that are not on the
//! surface (a short panel; Zoom out, Fit, default colours and swap on the rail), Customize tools
//! and the panel placement commands. The options block reserves the rows of the largest modifier set of all tools, so the
//! panel height never depends on the active tool.
use crate::app::*;
use crate::theme::ScrollTheme as _;
use crate::{ui_icons::Icon, workspace::PanelId};
use eframe::egui::{self, Key, Rect, Vec2};
use neoflash_ui::components::{self as nf, menu::menu_style};
use neoflash_ui::icons::i20;
use neoflash_ui::metrics;
use neoflash_ui::tokens::space;

/// Gap between tool cells (`gap/tool`).
fn tool_gap() -> f32 {
    metrics::density().pick(2.0, 2.0, 1.0)
}
/// Padding around every block of the palette (`pad/tools`).
fn tools_pad() -> f32 {
    metrics::density().pick(6.0, 6.0, 5.0)
}
/// Line between two blocks.
const DIVIDER: f32 = 1.0;
/// Height of the "⋯" bar under a tools grid whose rows are full, and its glyph size.
fn more_bar_height() -> f32 {
    metrics::density().pick(14.0, 12.0, 12.0)
}
fn more_bar_glyph() -> f32 {
    metrics::density().pick(16.0, 12.0, 12.0)
}
/// Hold time that opens a tool group (§13.4: long-press opens the flyout).
const LONG_PRESS: f64 = 0.45;
/// Swatch inside the stroke and fill chips, and the ring of the hollow stroke chip.
const CHIP_SWATCH: f32 = 16.0;
const CHIP_RING: f32 = 3.0;
/// Cells of the colours block: stroke chip, fill chip, then default colours and swap (the rail
/// keeps the two chips and lists the two commands in "⋯").
const COLOUR_CELLS: usize = 4;
const RAIL_COLOUR_CELLS: usize = 2;

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
const RAZOR: ToolSpec = spec(Tool::Razor, Icon::Razor, "Razor", "C", "Razor (C)");
const DECO: ToolSpec = spec(Tool::Deco, Icon::Deco, "Deco", "U", "Deco (U)");
const SPRAY_BRUSH: ToolSpec = spec(
    Tool::SprayBrush,
    Icon::SprayBrush,
    "Spray Brush",
    "⇧B",
    "Spray Brush",
);
const BONE: ToolSpec = spec(Tool::Bone, Icon::Bone, "Bone", "X", "Bone (X)");
const BIND: ToolSpec = spec(Tool::Bind, Icon::Bind, "Bind", "⇧X", "Bind");
const ASSET_WARP: ToolSpec = spec(
    Tool::AssetWarp,
    Icon::Puppet,
    "Asset Warp",
    "⇧W",
    "Asset Warp",
);
const TRANSLATE_3D: ToolSpec = spec(
    Tool::Translate3D,
    Icon::Translate3D,
    "3D Translation",
    "G",
    "3D Translation (G)",
);
const ROTATE_3D: ToolSpec = spec(
    Tool::Rotate3D,
    Icon::Rotate3D,
    "3D Rotation",
    "W",
    "3D Rotation (W)",
);
const HAND: ToolSpec = spec(Tool::Hand, Icon::Hand, "Hand", "H", "Hand (H / hold Space)");
const ZOOM: ToolSpec = spec(Tool::Zoom, Icon::ZoomIn, "Zoom", "Z", "Zoom (Z / M)");

/// 2 Columns grid, classic Flash order plus Razor and advanced Stage tools.
const GRID: &[ToolSpec] = &[
    SELECT,
    SUBSELECT,
    LINE,
    LASSO,
    PEN,
    TEXT,
    OVAL,
    RECTANGLE,
    POLYSTAR,
    PENCIL,
    BRUSH,
    TRANSFORM,
    GRADIENT,
    INK,
    BUCKET,
    EYEDROPPER,
    ERASER,
    RAZOR,
    DECO,
    SPRAY_BRUSH,
    BONE,
    BIND,
    ASSET_WARP,
    TRANSLATE_3D,
    ROTATE_3D,
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

fn popover_side_id() -> egui::Id {
    popover_id().with("prefer-left")
}

/// Natural Tools panel width for `cols` columns: `N × cell + (N − 1) × gap + 2 × padding`.
pub(crate) fn tools_natural_width(cols: usize) -> f32 {
    let cols = cols.max(1) as f32;
    cols * metrics::tool_button() + (cols - 1.0) * tool_gap() + 2.0 * tools_pad()
}

pub(crate) fn tool_columns_for_width(available_width: f32) -> usize {
    cells_fitting(available_width).max(1)
}

/// Cells of one row (or rows of one block) that fit `extent` once the block padding is taken off.
fn cells_fitting(extent: f32) -> usize {
    let step = metrics::tool_button() + tool_gap();
    (((extent - 2.0 * tools_pad() + tool_gap()) / step) + 0.01)
        .floor()
        .max(0.0) as usize
}

/// Height of a block of `rows` cell rows with its padding; an empty block takes no room.
fn block_height(rows: usize) -> f32 {
    if rows == 0 {
        0.0
    } else {
        tools_natural_width(rows)
    }
}

/// Height of the non-empty `blocks` stacked with a divider between neighbours.
fn stack_height(blocks: &[f32]) -> f32 {
    let shown = blocks.iter().filter(|height| **height > 0.0).count();
    blocks.iter().sum::<f32>() + shown.saturating_sub(1) as f32 * DIVIDER
}

fn is_view_tool(spec: &ToolSpec) -> bool {
    matches!(spec.tool, Tool::Hand | Tool::Zoom)
}

/// One cell of the tools block.
#[derive(Clone, Copy)]
enum ToolCell<'a> {
    Tool(ToolSpec),
    /// Rail: a slot of related tools behind one button with a flyout.
    Group(usize, &'a [ToolSpec]),
}

/// One cell of the view block, or a view command listed in the "⋯" menu.
#[derive(Clone, Copy)]
enum ViewCell {
    Tool(ToolSpec),
    ZoomOut,
    Fit,
}

/// Content of the palette for a column count, before it is fitted to the panel height.
struct ToolsPlan<'a> {
    cols: usize,
    tools: Vec<ToolCell<'a>>,
    view: Vec<ViewCell>,
    /// View commands that are never on the surface in this layout (rail: Zoom out, Fit).
    menu_only: Vec<ViewCell>,
    /// Rows the options block reserves, whatever the active tool is.
    option_rows: usize,
}

/// What of a [`ToolsPlan`] fits the panel height.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ToolsFit {
    tool_rows: usize,
    /// "⋯" is the bar under the grid, not its last cell.
    more_bar: bool,
    show_view: bool,
    option_rows: usize,
}

impl<'a> ToolsPlan<'a> {
    fn new(groups: &'a [Vec<ToolSpec>], cols: usize) -> Self {
        let cols = cols.max(1);
        if cols == 1 {
            let view_only = |group: &&Vec<ToolSpec>| group.iter().all(is_view_tool);
            return Self {
                cols,
                tools: groups
                    .iter()
                    .enumerate()
                    .filter(|(_, group)| !view_only(group))
                    .map(|(slot, group)| ToolCell::Group(slot, group.as_slice()))
                    .collect(),
                view: groups
                    .iter()
                    .filter(view_only)
                    .flatten()
                    .map(|spec| ViewCell::Tool(*spec))
                    .collect(),
                menu_only: vec![ViewCell::ZoomOut, ViewCell::Fit],
                // Snap to objects.
                option_rows: 1,
            };
        }
        let all = || groups.iter().flatten().copied();
        Self {
            cols,
            tools: all()
                .filter(|spec| !is_view_tool(spec))
                .map(ToolCell::Tool)
                .collect(),
            view: all()
                .filter(is_view_tool)
                .map(ViewCell::Tool)
                .chain([ViewCell::ZoomOut, ViewCell::Fit])
                .collect(),
            menu_only: Vec::new(),
            option_rows: max_modifier_rows(cols),
        }
    }

    /// With every tool on the surface "⋯" is a bar when it would otherwise start a row of its
    /// own: two or more columns and the tools fill their rows.
    fn more_is_bar(&self) -> bool {
        self.cols > 1 && !self.tools.is_empty() && self.tools.len().is_multiple_of(self.cols)
    }
    /// Rows of the tools block with every tool on the surface.
    fn tool_rows(&self) -> usize {
        if self.more_is_bar() {
            self.tools.len() / self.cols
        } else {
            (self.tools.len() + 1).div_ceil(self.cols)
        }
    }
    fn view_rows(&self) -> usize {
        self.view.len().div_ceil(self.cols)
    }
    fn colour_cells(&self) -> usize {
        if self.cols == 1 {
            RAIL_COLOUR_CELLS
        } else {
            COLOUR_CELLS
        }
    }
    fn colour_rows(&self) -> usize {
        self.colour_cells().div_ceil(self.cols)
    }

    fn height(&self, fit: ToolsFit) -> f32 {
        let more_bar = if fit.more_bar { more_bar_height() } else { 0.0 };
        stack_height(&[
            block_height(fit.tool_rows) + more_bar,
            if fit.show_view {
                block_height(self.view_rows())
            } else {
                0.0
            },
            block_height(self.colour_rows()),
            block_height(fit.option_rows),
        ])
    }

    /// Height of the whole palette. It does not depend on the active tool.
    fn natural_height(&self) -> f32 {
        self.height(ToolsFit {
            tool_rows: self.tool_rows(),
            more_bar: self.more_is_bar(),
            show_view: true,
            option_rows: self.option_rows,
        })
    }

    /// Tools on the surface for `fit`: every cell of its rows, less the one "⋯" takes as a cell.
    fn tools_shown(&self, fit: &ToolsFit) -> usize {
        (fit.tool_rows * self.cols)
            .saturating_sub(usize::from(!fit.more_bar))
            .min(self.tools.len())
    }

    /// What fits `available`. A panel shorter than the palette first gives up the option rows
    /// reserved for other tools. If that is not enough, tools leave the surface row by row, or the
    /// view block does, whichever hides fewer cells; whatever is hidden is listed in "⋯".
    fn fit(&self, available: f32, active_option_rows: usize) -> ToolsFit {
        let active = active_option_rows.min(self.option_rows);
        let fits = |fit: &ToolsFit| self.height(*fit) <= available + 0.5;
        let everything = (active..=self.option_rows)
            .rev()
            .map(|option_rows| ToolsFit {
                tool_rows: self.tool_rows(),
                more_bar: self.more_is_bar(),
                show_view: true,
                option_rows,
            })
            .find(fits);
        if let Some(fit) = everything {
            return fit;
        }
        // Every tool first; then "⋯" as the last cell of fewer and fewer rows.
        let most_tools = |show_view: bool| {
            let rows = |tool_rows, more_bar| ToolsFit {
                tool_rows,
                more_bar,
                show_view,
                option_rows: active,
            };
            std::iter::once(rows(self.tool_rows(), self.more_is_bar()))
                .chain((1..=self.tool_rows()).rev().map(|n| rows(n, false)))
                .find(fits)
        };
        let hidden = |fit: &ToolsFit| {
            self.tools.len() - self.tools_shown(fit)
                + if fit.show_view { 0 } else { self.view.len() }
        };
        match (most_tools(true), most_tools(false)) {
            (Some(with_view), Some(without)) if hidden(&without) < hidden(&with_view) => without,
            (Some(with_view), _) => with_view,
            (None, Some(without)) => without,
            (None, None) => ToolsFit {
                tool_rows: 1,
                more_bar: false,
                show_view: false,
                option_rows: active,
            },
        }
    }
}

/// Icon button that fills one tool cell, with the glyph master of the current density.
fn cell_button(icon: neoflash_ui::Icon, label: &str) -> nf::IconButton<'_> {
    nf::IconButton::new(icon.at(metrics::icon_tool()), label).side(metrics::tool_button())
}

impl Editor {
    pub(crate) fn tools_contents(&mut self, ui: &mut egui::Ui) {
        let stage_anchor = self.workspace.tools_stage_anchor(PanelId::Tools);
        let prefer_left = stage_anchor.map(|_| self.workspace.tools_popover_prefers_left());
        ui.ctx().data_mut(|data| {
            if let Some(prefer_left) = prefer_left {
                data.insert_temp(popover_side_id(), prefer_left);
            } else {
                data.remove::<bool>(popover_side_id());
            }
        });
        // A popover whose panel was not painted in the previous frame, or moved to another window,
        // has lost its anchor: close it instead of reopening it somewhere else.
        let ctx = ui.ctx().clone();
        let painted = (
            ctx.cumulative_frame_nr_for(egui::ViewportId::ROOT),
            ctx.viewport_id(),
        );
        let last_painted = ctx.data_mut(|data| {
            let slot = data.get_temp_mut_or(popover_id().with("painted"), painted);
            std::mem::replace(slot, painted)
        });
        if last_painted.1 != painted.1 || painted.0 > last_painted.0 + 1 {
            set_popover(&ctx, None);
        }
        let id = (
            "neoflash-panel-content",
            PanelId::Tools,
            self.clipboard.session.clone(),
        );
        ui.scope_builder(egui::UiBuilder::new().id(id), |ui| {
            let full_rect = ui.max_rect();
            if full_rect.is_positive() {
                ui.painter()
                    .rect_filled(full_rect, 0.0, neoflash_ui::palette(ui.ctx()).bg_panel);
            }
            // A side-centre Stage anchor is always the one-column rail and keeps the Stage height.
            let rail_anchor = stage_anchor.is_some_and(|anchor| anchor.vertical());
            let cols = if rail_anchor {
                1
            } else {
                tool_columns_for_width(ui.available_width())
            };
            let groups = self.tool_groups();
            let plan = ToolsPlan::new(&groups, cols);
            let panel_top = ui.cursor().top().max(0.0);
            self.detached.tools_desired_inner_height = (!rail_anchor).then(|| {
                (panel_top + plan.natural_height())
                    .ceil()
                    .min(crate::workspace::maximum_window_size(PanelId::Tools)[1])
            });
            let mut panel = ui.available_rect_before_wrap();
            if !panel.height().is_finite() {
                panel.set_height(plan.natural_height());
            }
            let active_option_rows = if cols == 1 {
                1
            } else {
                tool_modifier_row_count(self.tool, cols)
            };
            let fit = plan.fit(panel.height(), active_option_rows);
            let mut body = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(panel)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            body.set_clip_rect(ui.clip_rect().intersect(panel));
            self.tools_blocks(&mut body, panel, &plan, fit);
            ui.allocate_rect(panel, egui::Sense::hover());
        });
    }

    /// Paints the fitted blocks top to bottom, then the popover of the "⋯" cell.
    fn tools_blocks(
        &mut self,
        ui: &mut egui::Ui,
        panel: Rect,
        plan: &ToolsPlan<'_>,
        fit: ToolsFit,
    ) {
        let cols = plan.cols;
        let mut top = panel.top();
        // Tools: "⋯" closes the grid as its last cell, or as the bar under full rows.
        let shown = plan.tools_shown(&fit);
        for (index, cell) in plan.tools[..shown].iter().enumerate() {
            let mut cell_ui = tool_cell_ui(ui, panel, top, cols, index);
            match *cell {
                ToolCell::Tool(spec) => self.single_tool_button(&mut cell_ui, spec),
                ToolCell::Group(slot, group) => self.tool_group_button(&mut cell_ui, group, slot),
            }
        }
        top += block_height(fit.tool_rows);
        let more = if fit.more_bar {
            let bar = Rect::from_min_size(
                egui::pos2(panel.left(), top),
                Vec2::new(panel.width(), more_bar_height()),
            );
            top = bar.bottom();
            self.overflow_more_bar(ui, bar).rect
        } else {
            let cell_top = top - block_height(fit.tool_rows);
            self.overflow_more_button(&mut tool_cell_ui(ui, panel, cell_top, cols, shown))
                .rect
        };

        let mut menu_view = plan.menu_only.clone();
        if fit.show_view && !plan.view.is_empty() {
            top = block_divider(ui, panel, top);
            for (index, cell) in plan.view.iter().enumerate() {
                self.view_cell(&mut tool_cell_ui(ui, panel, top, cols, index), *cell);
            }
            top += block_height(plan.view_rows());
        } else {
            menu_view = plan.view.iter().chain(&plan.menu_only).copied().collect();
        }

        top = block_divider(ui, panel, top);
        self.colour_cells(ui, panel, top, cols, plan.colour_cells());
        top += block_height(plan.colour_rows());

        if fit.option_rows > 0 {
            top = block_divider(ui, panel, top);
            if cols == 1 {
                self.snap_to_objects_button(&mut tool_cell_ui(ui, panel, top, cols, 0));
            } else {
                let cells = tool_modifier_count(self.tool).min(fit.option_rows * cols);
                for index in 0..cells {
                    self.tool_modifier_cell(&mut tool_cell_ui(ui, panel, top, cols, index), index);
                }
            }
        }
        let colour_commands = plan.colour_cells() < COLOUR_CELLS;
        self.palette_menu_popover(ui, more, &plan.tools[shown..], &menu_view, colour_commands);
    }

    fn view_cell(&mut self, ui: &mut egui::Ui, cell: ViewCell) {
        match cell {
            ViewCell::Tool(spec) => self.single_tool_button(ui, spec),
            ViewCell::ZoomOut => {
                if ui.add(cell_button(i20::ZOOMOUT, "Zoom out")).clicked() {
                    self.zoom_out_stage();
                }
            }
            ViewCell::Fit => {
                if ui.add(cell_button(i20::FIT, "Fit stage")).clicked() {
                    self.fit_stage_from_tools();
                }
            }
        }
    }

    /// Opens the palette menu of the "⋯" cell (screenshot aid).
    pub(crate) fn open_tools_menu(&self, ctx: &egui::Context) {
        set_popover(ctx, Some(RailPopover::Overflow));
    }

    /// "⋯", the last cell of the tools block: opens the palette menu.
    fn overflow_more_button(&self, ui: &mut egui::Ui) -> egui::Response {
        let ctx = ui.ctx().clone();
        let open = popover(&ctx) == Some(RailPopover::Overflow);
        let response = ui.add(cell_button(i20::MORE, "More tools").selected(open));
        if response.clicked() {
            set_popover(&ctx, (!open).then_some(RailPopover::Overflow));
        }
        response
    }

    /// "⋯" as a low bar across the panel, under a tools grid whose rows are full: the same menu,
    /// tooltip and accessible name as the cell.
    fn overflow_more_bar(&self, ui: &mut egui::Ui, bar: Rect) -> egui::Response {
        let ctx = ui.ctx().clone();
        let open = popover(&ctx) == Some(RailPopover::Overflow);
        let response = ui.interact(bar, ui.id().with("more-bar"), egui::Sense::click());
        if response.clicked() {
            set_popover(&ctx, (!open).then_some(RailPopover::Overflow));
        }
        if ui.is_rect_visible(bar) {
            let p = neoflash_ui::palette(&ctx);
            let (bg, fg) = match (open, response.hovered()) {
                (true, _) => (p.bg_selected, p.accent_default),
                (false, true) => (p.bg_hover, p.text_primary),
                (false, false) => (p.bg_panel, p.text_secondary),
            };
            ui.painter().rect_filled(bar, 0.0, bg);
            let glyph = more_bar_glyph();
            neoflash_ui::icons::paint_sized(
                ui.painter(),
                &ctx,
                Rect::from_center_size(bar.center(), Vec2::splat(glyph)),
                neoflash_ui::icons::i16::MORE,
                glyph,
                fg,
            );
            if neoflash_ui::theme::keyboard_focus(ui, &response) {
                neoflash_ui::theme::focus_ring(ui, bar, neoflash_ui::tokens::radius::sm());
            }
        }
        let enabled = response.enabled();
        response.widget_info(move || {
            let mut info =
                egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, "More tools");
            info.selected = Some(open);
            info
        });
        nf::tooltip(response, "More tools", None)
    }

    /// Row of a tool list (group flyout or "⋯" menu). Returns `true` when it was chosen.
    fn paint_overflow_tool_item(
        &mut self,
        ui: &mut egui::Ui,
        spec: ToolSpec,
        group: Option<(usize, &[ToolSpec])>,
        width: f32,
    ) -> bool {
        let key = self.tool_display_key(spec);
        let label = format!("{} tool", spec.name);
        let mut item = nf::ToolFlyoutItem::new(spec.icon.tool_glyph(), &label)
            .active(self.tool == spec.tool)
            .width(width);
        if !key.is_empty() {
            item = item.shortcut(&key);
        }
        if ui.add(item).clicked() {
            if !spec.tool.preserves_selection_on_rail_click() {
                self.clear_selection();
            }
            self.set_tool(spec.tool);
            if let Some((index, group_specs)) = group {
                remember_group_tool(ui.ctx(), index, group_specs, spec.tool);
            }
            return true;
        }
        false
    }

    /// Width of a tool list: `menu_width()`, wider only when a tool name with its shortcut needs it.
    fn tool_list_width<'s>(&self, ui: &egui::Ui, specs: impl Iterator<Item = &'s ToolSpec>) -> f32 {
        let measure = |text: &str, style: neoflash_ui::TextStyle| {
            ui.painter()
                .layout_no_wrap(
                    text.to_owned(),
                    neoflash_ui::typography::font(style),
                    egui::Color32::PLACEHOLDER,
                )
                .size()
                .x
        };
        // `ToolFlyoutItem`: active mark, 20 pt icon box, label and shortcut with `s2` gaps, 10 pt end.
        let chrome = 5.0 * space::s2() + 6.0 + 20.0 + 10.0;
        specs
            .map(|spec| {
                chrome
                    + measure(&format!("{} tool", spec.name), neoflash_ui::TextStyle::Body)
                    + measure(
                        &self.tool_display_key(*spec),
                        neoflash_ui::TextStyle::MonoSmall,
                    )
            })
            .fold(metrics::menu_width(), f32::max)
            .ceil()
    }

    /// Popover of the "⋯" cell: the tools, view commands and (`colour_commands`) colour commands
    /// that are not on the surface, Customize tools and the panel placement commands.
    fn palette_menu_popover(
        &mut self,
        ui: &mut egui::Ui,
        anchor: Rect,
        tools: &[ToolCell<'_>],
        view: &[ViewCell],
        colour_commands: bool,
    ) {
        let ctx = ui.ctx().clone();
        if popover(&ctx) != Some(RailPopover::Overflow) {
            return;
        }
        let mut customize = false;
        let keep = rail_popover(ui, anchor, |ui| {
            let listed = tools
                .iter()
                .flat_map(|cell| match cell {
                    ToolCell::Tool(spec) => std::slice::from_ref(spec),
                    ToolCell::Group(_, group) => *group,
                })
                .chain(view.iter().filter_map(|cell| match cell {
                    ViewCell::Tool(spec) => Some(spec),
                    ViewCell::ZoomOut | ViewCell::Fit => None,
                }));
            let width = self.tool_list_width(ui, listed);
            ui.set_width(width);
            let mut done = false;
            let view_tools = view.iter().filter_map(|cell| match cell {
                ViewCell::Tool(spec) => Some(*spec),
                ViewCell::ZoomOut | ViewCell::Fit => None,
            });
            let commands = view.len() - view_tools.clone().count();
            if !tools.is_empty() || view.len() > commands {
                // Only the tool list scrolls; the commands below it stay in reach.
                let colour_rows = if colour_commands { 2.0 } else { 0.0 };
                let pinned = (commands as f32 + colour_rows) * metrics::row()
                    + 7.0 * crate::theme::row_h()
                    + 4.0 * metrics::menu_separator();
                let max_height =
                    (ui.ctx().content_rect().height() - pinned - 2.0 * space::s5()).max(80.0);
                egui::ScrollArea::vertical()
                    .id_salt("tools_palette_menu")
                    .max_height(max_height)
                    .themed()
                    .show(ui, |ui| {
                        for cell in tools {
                            match *cell {
                                ToolCell::Tool(spec) => {
                                    done |= self.paint_overflow_tool_item(ui, spec, None, width);
                                }
                                ToolCell::Group(slot, group) => {
                                    for spec in group {
                                        done |= self.paint_overflow_tool_item(
                                            ui,
                                            *spec,
                                            Some((slot, group)),
                                            width,
                                        );
                                    }
                                }
                            }
                        }
                        if !tools.is_empty() && view.len() > commands {
                            nf::menu_separator(ui);
                        }
                        for spec in view_tools {
                            done |= self.paint_overflow_tool_item(ui, spec, None, width);
                        }
                    });
                nf::menu_separator(ui);
            }
            for cell in view {
                match *cell {
                    ViewCell::ZoomOut => {
                        if ui
                            .add(nf::MenuItem::new("Zoom out").icon(Icon::ZoomOut.nf2(false)))
                            .clicked()
                        {
                            self.zoom_out_stage();
                            done = true;
                        }
                    }
                    ViewCell::Fit => {
                        if ui
                            .add(nf::MenuItem::new("Fit stage").icon(Icon::Fit.nf2(false)))
                            .clicked()
                        {
                            self.fit_stage_from_tools();
                            done = true;
                        }
                    }
                    ViewCell::Tool(_) => {}
                }
            }
            if commands > 0 {
                nf::menu_separator(ui);
            }
            if colour_commands {
                use neoflash_ui::icons::i16;
                if ui
                    .add(nf::MenuItem::new("Black and white").icon(i16::COLOR_DEFAULT))
                    .clicked()
                {
                    self.reset_default_paints();
                    done = true;
                }
                if ui
                    .add(nf::MenuItem::new("Swap colors").icon(i16::COLOR_SWAP))
                    .clicked()
                {
                    self.swap_active_paints();
                    done = true;
                }
                nf::menu_separator(ui);
            }
            if crate::ui_icons::menu_item(ui, "Customize tools…", "").clicked() {
                customize = true;
                done = true;
            }
            nf::menu_separator(ui);
            // The placement rows close their menu themselves; catch that here.
            done |= ui
                .scope_builder(egui::UiBuilder::new().closable(), |ui| {
                    self.panel_dock_menu(ui, PanelId::Tools);
                })
                .response
                .should_close();
            done
        });
        if customize {
            self.open_customize_tools(&ctx);
        }
        if !keep {
            set_popover(&ctx, None);
        }
    }

    fn zoom_out_stage(&mut self) {
        self.zoom_out();
    }

    fn fit_stage_from_tools(&mut self) {
        self.finish_edits();
        self.fit_stage();
    }

    /// Colours block: stroke chip, fill chip and, with `cells` = 4, default colours and swap. A chip
    /// opens the NF2 Color & Fill Type popover (the real Color panel when Tools is detached).
    fn colour_cells(
        &mut self,
        ui: &mut egui::Ui,
        panel: Rect,
        top: f32,
        cols: usize,
        cells: usize,
    ) {
        let ctx = ui.ctx().clone();
        let solid = |color: [u8; 4], gradient: Option<&neoflash_core::GradientPaint>| {
            let color = gradient
                .and_then(|gradient| gradient.stops.first())
                .map_or(color, |stop| stop.color);
            (color != [0, 0, 0, 0]).then(|| {
                egui::Color32::from_rgba_unmultiplied(color[0], color[1], color[2], color[3])
            })
        };
        let (fill, fill_gradient, stroke, stroke_gradient, _) = self.contextual_paints();
        let stroke_chip = paint_chip(
            &mut tool_cell_ui(ui, panel, top, cols, 0),
            true,
            solid(stroke, stroke_gradient.as_ref()),
            stroke_gradient.as_ref(),
        );
        let fill_chip = paint_chip(
            &mut tool_cell_ui(ui, panel, top, cols, 1),
            false,
            solid(fill, fill_gradient.as_ref()),
            fill_gradient.as_ref(),
        );
        if cells > 2
            && tool_cell_ui(ui, panel, top, cols, 2)
                .add(cell_button(i20::COLOR_DEFAULT, "Black and white"))
                .clicked()
        {
            self.reset_default_paints();
        }
        if cells > 3
            && tool_cell_ui(ui, panel, top, cols, 3)
                .add(cell_button(i20::COLOR_SWAP, "Swap colors"))
                .clicked()
        {
            self.swap_active_paints();
        }

        let tools_detached = !self.workspace.is_panel_docked(PanelId::Tools);
        let clicked = if fill_chip.clicked() {
            Some(RailPopover::Fill)
        } else if stroke_chip.clicked() {
            Some(RailPopover::Stroke)
        } else {
            None
        };
        if let Some(target) = clicked {
            ctx.data_mut(|d| {
                d.insert_temp(
                    egui::Id::new("color-panel-active-stroke"),
                    target == RailPopover::Stroke,
                )
            });
            self.authoring_modify.color_selection = true;
            if tools_detached {
                set_popover(&ctx, None);
                self.show_panel(PanelId::Color);
                self.focus_dock_panel(&ctx, PanelId::Color);
                return;
            }
            set_popover(&ctx, (popover(&ctx) != Some(target)).then_some(target));
        } else if tools_detached {
            // Native child viewports clip egui popovers to their own bounds. Never carry an
            // inline colour picker into a detached Tools window; the next click opens the real
            // Color panel with the same target instead.
            if matches!(popover(&ctx), Some(RailPopover::Fill | RailPopover::Stroke)) {
                set_popover(&ctx, None);
            }
            return;
        }
        if !matches!(popover(&ctx), Some(RailPopover::Fill | RailPopover::Stroke)) {
            return;
        }
        let anchor = stroke_chip.rect.union(fill_chip.rect);
        let keep = rail_popover(ui, anchor, |ui| self.color_fill_popover_contents(ui));
        if !keep {
            set_popover(&ctx, None);
        } else if let Some(active_stroke) =
            ctx.data(|d| d.get_temp::<bool>(egui::Id::new("color-panel-active-stroke")))
        {
            set_popover(
                &ctx,
                Some(if active_stroke {
                    RailPopover::Stroke
                } else {
                    RailPopover::Fill
                }),
            );
        }
    }

    fn single_tool_button(&mut self, ui: &mut egui::Ui, spec: ToolSpec) {
        let response = self.tool_button(ui, spec, self.tool == spec.tool, false);
        if response.clicked() {
            if !spec.tool.preserves_selection_on_rail_click() {
                self.clear_selection();
            }
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
            if !shown.tool.preserves_selection_on_rail_click() {
                self.clear_selection();
            }
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
            let width = self.tool_list_width(ui, group.iter());
            ui.set_min_width(width);
            for (i, spec) in group.iter().enumerate() {
                let key = self.tool_display_key(*spec);
                let label = format!("{} tool", spec.name);
                let mut item = nf::ToolFlyoutItem::new(spec.icon.tool_glyph(), &label)
                    .active(i == current)
                    .width(width);
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
            if !group[i].tool.preserves_selection_on_rail_click() {
                self.clear_selection();
            }
            self.set_tool(group[i].tool);
        }
        if !keep {
            set_popover(&ctx, None);
        }
    }

    fn snap_to_objects_button(&mut self, ui: &mut egui::Ui) {
        let snap_on = self.stage_view.prefs.objects_snap;
        if ui
            .add(cell_button(i20::TOOL_SNAP, "Snap to objects").selected(snap_on))
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
            .add(cell_button(i16::LOCK_FILL, "Lock fill").selected(locked))
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
        let response = ui.add(cell_button(icon, "Pencil mode").selected(open));
        if response.clicked() {
            set_popover(&ctx, (!open).then_some(RailPopover::PencilMode));
        }
        if popover(&ctx) != Some(RailPopover::PencilMode) {
            return;
        }
        let mut chosen = None;
        let keep = rail_popover(ui, response.rect, |ui| {
            ui.set_width(metrics::menu_width());
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
            ui.set_width(metrics::menu_width());
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
            ui.set_width(metrics::menu_width());
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
            ui.set_width(metrics::menu_width());
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
            ui.set_width(metrics::menu_width());
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
            ui.set_width(metrics::menu_width());
            for (mode, label) in [
                (EraserMode::StrokeSegments, "Erase normal"),
                (EraserMode::Fills, "Erase fills"),
                (EraserMode::Strokes, "Erase lines"),
                (EraserMode::SelectedFills, "Erase selected fills"),
                (EraserMode::Inside, "Erase inside"),
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
        use crate::authoring_tools::EraserShape;
        use crate::ui_icons::{ToolModifierGlyph, tool_modifier_button};
        let ctx = ui.ctx().clone();
        let open = popover(&ctx) == Some(RailPopover::EraserSize);
        let response = tool_modifier_button(
            ui,
            ToolModifierGlyph::EraserSize(
                self.authoring_tools.eraser_size,
                self.authoring_tools.eraser_shape,
            ),
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
        let mut chosen_size = None;
        let mut chosen_shape = None;
        let keep = rail_popover(ui, response.rect, |ui| {
            ui.set_width(metrics::menu_width());
            for (shape, label) in [
                (EraserShape::Round, "Round"),
                (EraserShape::Square, "Square"),
            ] {
                if ui
                    .add(
                        nf::MenuItem::new(label)
                            .checked(self.authoring_tools.eraser_shape == shape),
                    )
                    .clicked()
                {
                    chosen_shape = Some(shape);
                }
            }
            neoflash_ui::components::menu_separator(ui);
            for preset in [4.0_f32, 8.0, 12.0, 18.0, 24.0, 36.0, 48.0] {
                let is_cur = (self.authoring_tools.eraser_size - preset).abs() < 0.5;
                if ui
                    .add(nf::MenuItem::new(&format!("{preset:.0} px")).checked(is_cur))
                    .clicked()
                {
                    chosen_size = Some(preset);
                }
            }
            chosen_size.is_some() || chosen_shape.is_some()
        });
        if let Some(shape) = chosen_shape {
            self.authoring_tools.eraser_shape = shape;
        }
        if let Some(size) = chosen_size {
            self.authoring_tools.eraser_size = size;
        }
        if !keep {
            set_popover(&ctx, None);
        }
    }

    fn smooth_modifier_button(&mut self, ui: &mut egui::Ui) {
        let enabled = self.can_smooth_or_straighten_selection();
        if ui
            .add_enabled(enabled, cell_button(i20::TOOL_SMOOTH, "Smooth"))
            .clicked()
        {
            self.smooth_selection();
        }
    }

    fn straighten_modifier_button(&mut self, ui: &mut egui::Ui) {
        let enabled = self.can_smooth_or_straighten_selection();
        if ui
            .add_enabled(enabled, cell_button(i20::TOOL_STRAIGHTEN, "Straighten"))
            .clicked()
        {
            self.straighten_selection();
        }
    }

    /// Free Transform sub-mode toggle: a second click returns to plain Free Transform.
    fn transform_mode_modifier_button(
        &mut self,
        ui: &mut egui::Ui,
        icon: crate::ui_icons::TransformModifierIcon,
        mode: crate::authoring_tools::FreeTransformMode,
        label: &str,
    ) {
        use crate::authoring_tools::FreeTransformMode;
        let active = self.active_free_transform_mode();
        let enabled = !matches!(
            mode,
            FreeTransformMode::Distort | FreeTransformMode::Envelope
        ) || self.can_distort_or_envelope_selection();
        if crate::ui_icons::transform_modifier_button(ui, icon, active == mode, enabled, label)
            .clicked()
        {
            self.set_free_transform_mode(if active == mode {
                FreeTransformMode::FreeTransform
            } else {
                mode
            });
        }
    }

    fn lasso_modifier_button(&mut self, ui: &mut egui::Ui, index: usize) {
        use crate::ui_icons::{ToolModifierGlyph, tool_modifier_button};
        match index {
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
            1 => {
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
            2 => {
                let on = self.authoring_tools.lasso_polygon_mode;
                if tool_modifier_button(
                    ui,
                    ToolModifierGlyph::LassoPolygon,
                    on,
                    true,
                    "Polygon mode",
                )
                .clicked()
                {
                    self.authoring_tools.lasso_polygon_mode = !on;
                    if self.authoring_tools.lasso_polygon_mode {
                        self.authoring_tools.lasso_magic_wand = false;
                    } else {
                        self.authoring_tools.lasso_polygon.clear();
                    }
                }
            }
            3 => {
                let on = self.authoring_tools.lasso_magic_wand;
                if tool_modifier_button(ui, ToolModifierGlyph::MagicWand, on, true, "Magic wand")
                    .clicked()
                {
                    self.authoring_tools.lasso_magic_wand = !on;
                    if self.authoring_tools.lasso_magic_wand {
                        self.authoring_tools.lasso_polygon_mode = false;
                        self.authoring_tools.lasso_polygon.clear();
                    }
                }
            }
            _ => {
                let open = self.authoring_tools.magic_wand_dialog.is_some();
                if tool_modifier_button(
                    ui,
                    ToolModifierGlyph::MagicWandSettings,
                    open,
                    true,
                    "Magic wand settings…",
                )
                .clicked()
                {
                    self.open_magic_wand_settings();
                }
            }
        }
    }

    /// Option cell `index` of the active tool (classic Flash per-tool modifiers). The cells of a
    /// tool wrap by the column count; see [`tool_modifier_count`].
    fn tool_modifier_cell(&mut self, ui: &mut egui::Ui, index: usize) {
        use crate::authoring_tools::{EraserMode, FreeTransformMode};
        use crate::ui_icons::{ToolModifierGlyph, TransformModifierIcon, tool_modifier_button};
        use neoflash_ui::icons::i16;

        match (self.tool, index) {
            (Tool::Select, 0) => self.snap_to_objects_button(ui),
            (Tool::Select, 1) => self.smooth_modifier_button(ui),
            (Tool::Select, _) => self.straighten_modifier_button(ui),
            (Tool::Transform, 0) => self.snap_to_objects_button(ui),
            (Tool::Transform, 1) => self.transform_mode_modifier_button(
                ui,
                TransformModifierIcon::RotateAndSkew,
                FreeTransformMode::RotateAndSkew,
                "Rotate and Skew",
            ),
            (Tool::Transform, 2) => self.transform_mode_modifier_button(
                ui,
                TransformModifierIcon::Scale,
                FreeTransformMode::Scale,
                "Scale",
            ),
            (Tool::Transform, 3) => self.transform_mode_modifier_button(
                ui,
                TransformModifierIcon::Distort,
                FreeTransformMode::Distort,
                "Distort",
            ),
            (Tool::Transform, _) => self.transform_mode_modifier_button(
                ui,
                TransformModifierIcon::Envelope,
                FreeTransformMode::Envelope,
                "Envelope",
            ),
            (Tool::Line | Tool::Pen | Tool::Rectangle | Tool::Ellipse | Tool::PolyStar, 0) => {
                self.object_drawing_modifier_button(ui)
            }
            (Tool::PolyStar, 2..) => {
                if ui
                    .add(cell_button(i16::SETTINGS, "PolyStar tool settings…"))
                    .clicked()
                {
                    self.open_polystar_settings();
                }
            }
            (Tool::Line | Tool::Pen | Tool::Rectangle | Tool::Ellipse | Tool::PolyStar, _) => {
                self.snap_to_objects_button(ui)
            }
            (Tool::Pencil, 0) => self.object_drawing_modifier_button(ui),
            (Tool::Pencil, _) => self.pencil_mode_modifier_button(ui),
            (Tool::Brush, 0) => self.object_drawing_modifier_button(ui),
            (Tool::Brush, 1) => self.lock_fill_modifier_button(ui),
            (Tool::Brush, 2) => self.brush_mode_modifier_button(ui),
            (Tool::Brush, 3) => self.brush_size_modifier_button(ui),
            (Tool::Brush, 4) => self.brush_shape_modifier_button(ui),
            (Tool::Brush, 5) => {
                let on = self.authoring_tools.brush_pressure;
                if ui
                    .add(cell_button(i16::PRESSURE, "Use pressure").selected(on))
                    .clicked()
                {
                    self.authoring_tools.brush_pressure = !on;
                }
            }
            (Tool::Brush, _) => {
                let on = self.authoring_tools.brush_tilt;
                if ui
                    .add(cell_button(i16::TILT, "Use tilt").selected(on))
                    .clicked()
                {
                    self.authoring_tools.brush_tilt = !on;
                }
            }
            (Tool::Bucket, 0) => self.gap_size_modifier_button(ui),
            (Tool::Bucket, _) => self.lock_fill_modifier_button(ui),
            (Tool::Eraser, 0) => self.eraser_mode_modifier_button(ui),
            (Tool::Eraser, 1) => {
                let faucet_on = self.authoring_tools.eraser_mode == EraserMode::Objects;
                if tool_modifier_button(ui, ToolModifierGlyph::Faucet, faucet_on, true, "Faucet")
                    .clicked()
                {
                    self.authoring_tools.eraser_mode = if faucet_on {
                        EraserMode::StrokeSegments
                    } else {
                        EraserMode::Objects
                    };
                }
            }
            (Tool::Eraser, _) => self.eraser_size_modifier_button(ui),
            (Tool::Lasso, index) => self.lasso_modifier_button(ui, index),
            (Tool::Zoom, 0) => {
                let zoom_out = self.authoring_tools.zoom_out;
                if ui
                    .add(cell_button(i20::ZOOMIN, "Enlarge").selected(!zoom_out))
                    .clicked()
                {
                    self.authoring_tools.zoom_out = false;
                }
            }
            (Tool::Zoom, _) => {
                let zoom_out = self.authoring_tools.zoom_out;
                if ui
                    .add(cell_button(i20::ZOOMOUT, "Reduce").selected(zoom_out))
                    .clicked()
                {
                    self.authoring_tools.zoom_out = true;
                }
            }
            (
                Tool::Subselect
                | Tool::Gradient
                | Tool::Text
                | Tool::Ink
                | Tool::Eyedropper
                | Tool::Hand
                | Tool::Razor
                | Tool::Deco
                | Tool::SprayBrush
                | Tool::Bone
                | Tool::Bind
                | Tool::AssetWarp
                | Tool::Translate3D
                | Tool::Rotate3D,
                _,
            ) => {}
        }
    }
}

/// Option cells of `tool` in the multi-column layouts.
fn tool_modifier_count(tool: Tool) -> usize {
    match tool {
        Tool::Brush => 7,
        Tool::Transform | Tool::Lasso => 5,
        Tool::Select | Tool::PolyStar | Tool::Eraser => 3,
        Tool::Line
        | Tool::Pen
        | Tool::Rectangle
        | Tool::Ellipse
        | Tool::Pencil
        | Tool::Bucket
        | Tool::Zoom => 2,
        Tool::Subselect
        | Tool::Gradient
        | Tool::Text
        | Tool::Ink
        | Tool::Eyedropper
        | Tool::Hand
        | Tool::Razor
        | Tool::Deco
        | Tool::SprayBrush
        | Tool::Bone
        | Tool::Bind
        | Tool::AssetWarp
        | Tool::Translate3D
        | Tool::Rotate3D => 0,
    }
}

fn tool_modifier_row_count(tool: Tool, cols: usize) -> usize {
    tool_modifier_count(tool).div_ceil(cols.max(1))
}

/// Rows of the largest modifier set over every tool: the options block reserves them, so the
/// palette keeps its height when the tool changes.
fn max_modifier_rows(cols: usize) -> usize {
    tool_catalogue()
        .iter()
        .map(|spec| tool_modifier_row_count(spec.tool, cols))
        .max()
        .unwrap_or(0)
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

/// Whether any Tools panel popover (flyout, overflow, fill/stroke or tool options) is open.
pub(crate) fn is_rail_popover_open(ctx: &egui::Context) -> bool {
    popover(ctx).is_some()
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
            if matches!(spec.tool, Tool::SprayBrush | Tool::Bind | Tool::AssetWarp)
                && !key.is_empty()
            {
                return format!("{} ({})", spec.name, nf::platform_shortcut(key));
            }
            return spec.label.to_owned();
        }
        if spec.tool == Tool::Hand {
            if key.is_empty() {
                "Hand (hold Space)".to_owned()
            } else {
                format!("Hand ({} / hold Space)", nf::platform_shortcut(key))
            }
        } else if key.is_empty() {
            spec.name.to_owned()
        } else {
            format!("{} ({})", spec.name, nf::platform_shortcut(key))
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

/// A child UI over cell `index` of the block that starts at `top`: `cols` cells per row, rows
/// left-aligned inside a grid that is centred in `panel`.
fn tool_cell_ui(ui: &mut egui::Ui, panel: Rect, top: f32, cols: usize, index: usize) -> egui::Ui {
    let cols = cols.max(1);
    let side = metrics::tool_button();
    let step = side + tool_gap();
    let grid = cols as f32 * step - tool_gap();
    let left = (panel.center().x - grid * 0.5).round();
    let min = egui::pos2(
        left + (index % cols) as f32 * step,
        top + tools_pad() + (index / cols) as f32 * step,
    );
    ui.new_child(
        egui::UiBuilder::new()
            .max_rect(Rect::from_min_size(min, Vec2::splat(side)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    )
}

/// 1 px line between two blocks at `top`; returns the top of the next block.
fn block_divider(ui: &egui::Ui, panel: Rect, top: f32) -> f32 {
    ui.painter().hline(
        panel.x_range(),
        top + DIVIDER * 0.5,
        egui::Stroke::new(DIVIDER, neoflash_ui::palette(ui.ctx()).border_subtle),
    );
    top + DIVIDER
}

/// Stroke or fill chip: a tool cell with a 16 pt swatch inside; the stroke chip is a hollow square.
fn paint_chip(
    ui: &mut egui::Ui,
    stroke: bool,
    solid: Option<egui::Color32>,
    gradient: Option<&neoflash_core::GradientPaint>,
) -> egui::Response {
    let label = if stroke {
        "Stroke colour"
    } else {
        "Fill colour"
    };
    let p = neoflash_ui::palette(ui.ctx());
    let (rect, response) =
        ui.allocate_exact_size(Vec2::splat(metrics::tool_button()), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let radius = neoflash_ui::tokens::radius::md();
        let bg = if response.hovered() {
            p.bg_hover
        } else {
            p.bg_panel
        };
        ui.painter().rect_filled(rect, radius, bg);
        let swatch = Rect::from_center_size(rect.center(), Vec2::splat(CHIP_SWATCH));
        let swatch_radius = neoflash_ui::tokens::radius::sm();
        match (gradient, solid) {
            (Some(gradient), _) => crate::paint_controls::paint_gradient_swatch(
                ui.painter(),
                swatch,
                gradient,
                swatch_radius,
            ),
            (None, Some(color)) => nf::display::paint_swatch(ui, swatch, color, swatch_radius),
            (None, None) => nf::color::paint_none(ui, swatch),
        }
        if stroke && (gradient.is_some() || solid.is_some()) {
            // Hollow square: the cell shows through the middle of the swatch.
            ui.painter().rect_filled(swatch.shrink(CHIP_RING), 0.0, bg);
        }
        if neoflash_ui::theme::keyboard_focus(ui, &response) {
            neoflash_ui::theme::focus_ring(ui, rect, radius);
        }
    }
    let enabled = response.enabled();
    response.widget_info(move || {
        egui::WidgetInfo::labeled(egui::WidgetType::ColorButton, enabled, label)
    });
    nf::tooltip(response, label, None)
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
    let open_left = ctx
        .data(|data| data.get_temp::<bool>(popover_side_id()))
        .unwrap_or_else(|| anchor.center().x > ctx.content_rect().center().x);
    let (primary, alt) = if open_left {
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
    .gap(space::s2())
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

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::Pos2;
    use neoflash_core::Project;

    /// Runs `body` in every density and restores Normal afterwards.
    fn in_every_density(mut body: impl FnMut(metrics::Density)) {
        for density in metrics::Density::ALL {
            metrics::set_density(density);
            body(density);
        }
        metrics::set_density(metrics::Density::default());
    }

    fn render(editor: &mut Editor, ctx: &egui::Context, size: Vec2) -> egui::FullOutput {
        let panel = Rect::from_min_size(Pos2::new(8.0, 8.0), size);
        ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size + Vec2::splat(16.0))),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.scope_builder(egui::UiBuilder::new().max_rect(panel), |ui| {
                        editor.tools_contents(ui)
                    });
                });
            },
        )
    }

    fn bounds(output: &egui::FullOutput, label: &str) -> Option<Rect> {
        output
            .platform_output
            .accesskit_update
            .as_ref()?
            .nodes
            .iter()
            .find_map(|(_, node)| {
                (node.label() == Some(label))
                    .then(|| node.bounds())
                    .flatten()
                    .map(|r| {
                        Rect::from_min_max(
                            Pos2::new(r.x0 as f32, r.y0 as f32),
                            Pos2::new(r.x1 as f32, r.y1 as f32),
                        )
                    })
            })
    }

    #[test]
    fn natural_widths_are_the_docked_tools_metrics() {
        in_every_density(|density| {
            assert_eq!(
                tools_natural_width(1),
                metrics::tools_rail(),
                "{density:?}: one column is the rail"
            );
            let slack = metrics::tools_two_columns() - tools_natural_width(2);
            assert!(
                (0.0..=2.0).contains(&slack),
                "{density:?}: two columns leave {slack} pt"
            );
            assert_eq!(tool_columns_for_width(metrics::tools_rail()), 1);
            assert_eq!(tool_columns_for_width(metrics::tools_two_columns()), 2);
            for cols in 1..=6 {
                assert_eq!(tool_columns_for_width(tools_natural_width(cols)), cols);
                assert_eq!(
                    tool_columns_for_width(tools_natural_width(cols + 1) - 0.5),
                    cols,
                    "{density:?}: a column appears only at its natural width"
                );
            }
        });
    }

    #[test]
    fn desired_height_does_not_depend_on_the_active_tool() {
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        in_every_density(|density| {
            for cols in 1..=3 {
                let size = Vec2::new(tools_natural_width(cols), 2000.0);
                let mut heights = Vec::new();
                for spec in tool_catalogue() {
                    editor.set_tool(spec.tool);
                    render(&mut editor, &ctx, size);
                    let height = editor
                        .detached
                        .tools_desired_inner_height
                        .expect("a free palette records its height");
                    heights.push((spec.name, height));
                }
                let expected = heights[0].1;
                assert!(
                    heights.iter().all(|(_, height)| *height == expected),
                    "{density:?}, {cols} columns: {heights:?}"
                );
                let groups = editor.tool_groups();
                assert_eq!(
                    expected,
                    (8.0 + ToolsPlan::new(&groups, cols).natural_height()).ceil(),
                    "{density:?}, {cols} columns: the height is the stacked blocks"
                );
            }
        });
    }

    /// Slots with the first `count` drawing tools of the catalogue, two per slot, then Hand and Zoom.
    fn slots_with(count: usize) -> Vec<Vec<String>> {
        let names: Vec<String> = GRID.iter().map(|spec| spec.name.to_owned()).collect();
        let mut slots: Vec<Vec<String>> =
            names[..count].chunks(2).map(<[String]>::to_vec).collect();
        slots.push(VIEW.iter().map(|spec| spec.name.to_owned()).collect());
        slots
    }

    #[test]
    fn tools_grid_is_centred_and_more_never_leaves_a_hole_before_it() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        in_every_density(|density| {
            for (count, cols) in [(17, 2), (18, 2), (25, 2), (17, 3), (18, 3), (25, 3)] {
                editor.workspace.tools.slots = slots_with(count);
                let width = if cols == 2 {
                    metrics::tools_two_columns()
                } else {
                    tools_natural_width(cols)
                };
                let size = Vec2::new(width, 2000.0);
                render(&mut editor, &ctx, size);
                let out = render(&mut editor, &ctx, size);
                let case = format!("{density:?}, {count} tools, {cols} columns");
                let side = metrics::tool_button();
                let step = side + tool_gap();
                let left = 8.0 + (width - (cols as f32 * step - tool_gap())) * 0.5;
                let top = 8.0 + tools_pad();
                let cell_at = |index: usize| {
                    Rect::from_min_size(
                        Pos2::new(
                            left + (index % cols) as f32 * step,
                            top + (index / cols) as f32 * step,
                        ),
                        Vec2::splat(side),
                    )
                };
                let same = |a: Rect, b: Rect| {
                    (a.min - b.min).length() < 0.6 && (a.max - b.max).length() < 0.6
                };
                // Every tool is a cell, in order, row after row.
                for (index, spec) in GRID[..count].iter().enumerate() {
                    let label =
                        editor.tool_accessible_label(*spec, &editor.tool_display_key(*spec));
                    let cell = bounds(&out, &label).unwrap_or_else(|| panic!("{case}: {label}"));
                    assert!(same(cell, cell_at(index)), "{case}: {label} at {cell:?}");
                }
                let more = bounds(&out, "More tools").unwrap_or_else(|| panic!("{case}: More"));
                let rows = count.div_ceil(cols);
                if count % cols == 0 {
                    // Full rows: a bar across the panel right under the grid, not a lonely cell.
                    let bar = Rect::from_min_size(
                        Pos2::new(8.0, 8.0 + block_height(rows)),
                        Vec2::new(width, more_bar_height()),
                    );
                    assert!(same(more, bar), "{case}: bar {more:?}, expected {bar:?}");
                } else {
                    // Otherwise the cell right after the last tool, in the same row.
                    assert!(same(more, cell_at(count)), "{case}: cell {more:?}");
                    assert_eq!(count / cols, (rows * cols - 1) / cols, "{case}: same row");
                }
                if cols == 2 && count % cols != 0 {
                    assert_eq!(
                        count % cols,
                        cols - 1,
                        "{case}: the cell fills the last row"
                    );
                }
                // The next block starts right under the tools block.
                let hand = bounds(&out, "Hand (H / hold Space)").expect("view block");
                let tools_bottom = 8.0
                    + block_height(rows)
                    + if count % cols == 0 {
                        more_bar_height()
                    } else {
                        0.0
                    };
                assert!(
                    (hand.top() - (tools_bottom + DIVIDER + tools_pad())).abs() < 0.6,
                    "{case}: view block at {hand:?}"
                );
                // The grid is centred: the same margin on both sides.
                let first = cell_at(0);
                let last_in_row = cell_at(cols - 1);
                assert!(
                    ((first.left() - 8.0) - (8.0 + width - last_in_row.right())).abs() < 1.1,
                    "{case}"
                );
                // Chips are tool cells, and the four view cells are on the surface.
                for chip in ["Stroke colour", "Fill colour"] {
                    let cell = bounds(&out, chip).unwrap();
                    assert_eq!(cell.size(), Vec2::splat(side), "{case}: {chip}");
                }
                for view in ["Zoom (Z / M)", "Zoom out", "Fit stage"] {
                    assert!(bounds(&out, view).is_some(), "{case}: {view}");
                }
            }
        });
    }

    #[test]
    fn a_short_panel_moves_cells_into_more_without_losing_any() {
        let groups: Vec<Vec<ToolSpec>> = crate::tool_preferences::default_slots()
            .iter()
            .map(|slot| {
                slot.iter()
                    .filter_map(|name| tool_catalogue().into_iter().find(|s| s.name == name))
                    .collect()
            })
            .collect();
        in_every_density(|density| {
            for cols in 1..=3 {
                let plan = ToolsPlan::new(&groups, cols);
                let natural = plan.natural_height();
                let full = plan.fit(natural, 0);
                assert_eq!(
                    full,
                    ToolsFit {
                        tool_rows: plan.tool_rows(),
                        more_bar: plan.more_is_bar(),
                        show_view: true,
                        option_rows: plan.option_rows,
                    },
                    "{density:?}, {cols} columns"
                );
                // One row short: the reserved option rows go first, every cell stays.
                let active = plan.option_rows.saturating_sub(1);
                let tight = plan.fit(natural - metrics::tool_button(), active);
                if active < plan.option_rows {
                    assert_eq!(
                        (tight.tool_rows, tight.show_view, tight.option_rows),
                        (plan.tool_rows(), true, active),
                        "{density:?}, {cols} columns"
                    );
                }
                // Always at least one row of tools, never taller than the panel allows.
                let mut height = natural;
                while height > block_height(1) + block_height(plan.colour_rows()) + 2.0 * DIVIDER {
                    let fit = plan.fit(height, 0);
                    assert!(fit.tool_rows >= 1);
                    assert!(
                        plan.height(fit) <= height + 0.5,
                        "{density:?}, {cols}: {fit:?}"
                    );
                    assert!(plan.tools_shown(&fit) <= plan.tools.len());
                    assert!(!fit.more_bar || plan.tools_shown(&fit) == plan.tools.len());
                    height -= 7.0;
                }
            }
        });
    }
}
