//! Timeline panel (NF2 Timeline 2.0, reference §5). The controls bar lives in `timeline_controls.rs`;
//! this file lays out the body: `TL Layers Header` + one `TL Layer` row per layer (280 pt column, user
//! resizable), the `TL Ruler` (32), and the frame area painted with the `TL Grid`, `TL Span`,
//! `TL Grid Lines`, `TL Marker` and `TL Frame Label` painters, plus `TL Playhead` and `TL Onion Range`.
//! Frame `n` (1-based) starts at `(n − 1) × frame width`; the editor keeps 0-based frames.
use crate::app::*;
use crate::theme::ScrollTheme as _;
use crate::{
    layer_controls::{layer_color, layer_depth, layer_kind, visible_layer_indices},
    theme::UiPalette,
    timeline_paint::{frame_at_x, ruler_height, timeline_cell_at},
    workspace::PanelId,
};
use eframe::egui::{self, Align2, Pos2, Rect, Sense, Stroke, Vec2};
use neoflash_core::*;
use neoflash_ui::components as nf;
use neoflash_ui::components::timeline::{
    self as tl, FrameLabelKind, Geometry, GridKind, LayerRow, MarkerKind, SpanType, ToggleKind,
};
use neoflash_ui::metrics::LAYER_TOGGLE;
use neoflash_ui::tokens::space;
use neoflash_ui::{TextStyle, typography};

#[path = "output.rs"]
pub(crate) mod output;
#[allow(unused_imports)]
pub(crate) use output::*;

/// Default vertical overscan (in rows) for timeline row virtualization.
pub const DEFAULT_TIMELINE_OVERSCAN_ROWS: usize = 4;

/// Default horizontal overscan (in frames) for timeline frame-column virtualization.
pub const DEFAULT_TIMELINE_OVERSCAN_FRAMES: u32 = 16;

/// Deterministic viewport virtualization window for the Timeline panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimelineVirtualWindow {
    /// First visible/overscanned row index (inclusive, 0-based).
    pub first_row: usize,
    /// Last visible/overscanned row index (inclusive, 0-based).
    pub last_row: usize,
    /// First visible/overscanned frame index (inclusive, 0-based).
    pub first_frame: u32,
    /// Last visible/overscanned frame index (inclusive, 0-based).
    pub last_frame: u32,
    /// Vertical overscan margin in rows.
    pub overscan_rows: usize,
    /// Horizontal overscan margin in frames.
    pub overscan_frames: u32,
}

impl TimelineVirtualWindow {
    /// Compute the virtualized row and frame window from a viewport/clip rect, content origin,
    /// row height (`row_h`), and frame cell width (`cell_w`).
    #[allow(clippy::too_many_arguments)]
    pub fn compute(
        clip_rect: Rect,
        content_origin: Pos2,
        row_h: f32,
        cell_w: f32,
        total_rows: usize,
        total_frames: u32,
        overscan_rows: usize,
        overscan_frames: u32,
    ) -> Self {
        let last_valid_row = total_rows.saturating_sub(1);
        let (first_row, last_row) = if total_rows == 0
            || row_h <= 0.0
            || !clip_rect.top().is_finite()
            || !clip_rect.bottom().is_finite()
            || clip_rect.bottom() <= clip_rect.top()
        {
            (0, last_valid_row)
        } else {
            let raw_first = ((clip_rect.top() - content_origin.y) / row_h)
                .floor()
                .max(0.0) as usize;
            let raw_last = (((clip_rect.bottom() - content_origin.y) / row_h)
                .ceil()
                .max(1.0) as usize)
                .saturating_sub(1);
            let first = raw_first.saturating_sub(overscan_rows).min(last_valid_row);
            let last = raw_last
                .saturating_add(overscan_rows)
                .min(last_valid_row)
                .max(first);
            (first, last)
        };

        let last_valid_frame = total_frames.saturating_sub(1);
        let (first_frame, last_frame) = if total_frames == 0
            || cell_w <= 0.0
            || !clip_rect.left().is_finite()
            || !clip_rect.right().is_finite()
            || clip_rect.right() <= clip_rect.left()
        {
            (0, last_valid_frame)
        } else {
            let raw_first = ((clip_rect.left() - content_origin.x) / cell_w)
                .floor()
                .max(0.0) as u32;
            let raw_last = (((clip_rect.right() - content_origin.x) / cell_w)
                .ceil()
                .max(1.0) as u32)
                .saturating_sub(1);
            let first = raw_first
                .saturating_sub(overscan_frames)
                .min(last_valid_frame);
            let last = raw_last
                .saturating_add(overscan_frames)
                .min(last_valid_frame)
                .max(first);
            (first, last)
        };

        Self {
            first_row,
            last_row,
            first_frame,
            last_frame,
            overscan_rows,
            overscan_frames,
        }
    }

    /// Compute with the standard overscan margins (`4` rows, `16` frames).
    pub fn with_default_overscan(
        clip_rect: Rect,
        content_origin: Pos2,
        row_h: f32,
        cell_w: f32,
        total_rows: usize,
        total_frames: u32,
    ) -> Self {
        Self::compute(
            clip_rect,
            content_origin,
            row_h,
            cell_w,
            total_rows,
            total_frames,
            DEFAULT_TIMELINE_OVERSCAN_ROWS,
            DEFAULT_TIMELINE_OVERSCAN_FRAMES,
        )
    }

    /// Half-open row range (`start..end`) clamped to `total_rows`.
    pub fn row_range(&self, total_rows: usize) -> std::ops::Range<usize> {
        if total_rows == 0 {
            return 0..0;
        }
        let start = self.first_row.min(total_rows);
        let end = self.last_row.saturating_add(1).min(total_rows).max(start);
        start..end
    }

    /// Half-open frame range (`start..end`) clamped to `total_frames`.
    pub fn frame_range(&self, total_frames: u32) -> std::ops::Range<u32> {
        if total_frames == 0 {
            return 0..0;
        }
        let start = self.first_frame.min(total_frames);
        let end = self
            .last_frame
            .saturating_add(1)
            .min(total_frames)
            .max(start);
        start..end
    }

    /// Exact top spacer height above `first_row`.
    #[allow(dead_code)]
    pub fn top_spacer_height(&self, row_h: f32, total_rows: usize) -> f32 {
        (self.row_range(total_rows).start as f32 * row_h.max(0.0)).max(0.0)
    }

    /// Exact bottom spacer height below `last_row`.
    #[allow(dead_code)]
    pub fn bottom_spacer_height(&self, row_h: f32, total_rows: usize) -> f32 {
        let range = self.row_range(total_rows);
        (total_rows.saturating_sub(range.end) as f32 * row_h.max(0.0)).max(0.0)
    }

    /// Exact left spacer width before `first_frame`.
    #[allow(dead_code)]
    pub fn left_spacer_width(&self, cell_w: f32, total_frames: u32) -> f32 {
        (self.frame_range(total_frames).start as f32 * cell_w.max(0.0)).max(0.0)
    }

    /// Exact right spacer width after `last_frame`.
    #[allow(dead_code)]
    pub fn right_spacer_width(&self, cell_w: f32, total_frames: u32) -> f32 {
        let range = self.frame_range(total_frames);
        (total_frames.saturating_sub(range.end) as f32 * cell_w.max(0.0)).max(0.0)
    }

    /// Whether a frame span `[start_frame..end_frame_exclusive)` (or single frame when
    /// `end_frame_exclusive <= start_frame`) intersects `[first_frame..=last_frame]`.
    pub fn span_intersects(&self, start_frame: u32, end_frame_exclusive: u32) -> bool {
        let end_inclusive = end_frame_exclusive.max(start_frame);
        end_inclusive >= self.first_frame && start_frame <= self.last_frame.saturating_add(1)
    }
}

/// Telemetry recorded during the most recent `timeline_contents` pass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimelineVirtualStats {
    pub window: TimelineVirtualWindow,
    pub total_rows: usize,
    pub total_frames: u32,
    pub drawn_layer_rows: usize,
    pub drawn_grid_rows: usize,
    pub drawn_frame_columns: u32,
    pub drawn_spans: usize,
    pub layer_list_height: f32,
    pub scroll_offset_y: f32,
}

const TIMELINE_VIRTUAL_STATS_ID: &str = "neoflash_timeline_virtual_stats";
const TIMELINE_LAYER_FOCUS_ID: &str = "neoflash_timeline_layer_focus";

fn fast_layer_kind(layer: &Layer, guided_ids: &std::collections::HashSet<String>) -> tl::LayerKind {
    match layer.properties.role {
        LayerRole::Folder if layer.properties.collapsed => tl::LayerKind::FolderClosed,
        LayerRole::Folder => tl::LayerKind::FolderOpen,
        LayerRole::Guide if guided_ids.contains(&layer.id) => tl::LayerKind::MotionGuide,
        LayerRole::Guide => tl::LayerKind::Guide,
        LayerRole::Mask => tl::LayerKind::Mask,
        LayerRole::Normal if layer.properties.mask.is_some() => tl::LayerKind::Masked,
        LayerRole::Normal
            if layer.keyframes.iter().any(|k| k.sound.is_some())
                && layer.keyframes.iter().all(|k| k.elements.is_empty()) =>
        {
            tl::LayerKind::Sound
        }
        LayerRole::Normal => tl::LayerKind::Layer,
    }
}

fn fast_layer_depth(
    layers: &[Layer],
    by_id: &std::collections::HashMap<String, usize>,
    layer: &Layer,
) -> usize {
    let mut depth = 0usize;
    let mut seen = std::collections::HashSet::from([layer.id.as_str()]);
    let mut current = layer;
    while let Some(parent) = current
        .properties
        .mask
        .as_ref()
        .or(current.properties.motion_guide.as_ref())
        .or(current.properties.parent.as_ref())
        .and_then(|id| by_id.get(id.as_str()).and_then(|&idx| layers.get(idx)))
    {
        if !seen.insert(parent.id.as_str()) {
            break;
        }
        depth += 1;
        current = parent;
    }
    depth
}

fn fast_layer_effective_blend_mode(
    layers: &[Layer],
    by_id: &std::collections::HashMap<String, usize>,
    layer: &Layer,
) -> BlendMode {
    let mut current = Some(layer);
    for _ in 0..=layers.len() {
        let Some(node) = current else {
            return BlendMode::Normal;
        };
        if !node.properties.blend_mode.is_normal() {
            return node.properties.blend_mode;
        }
        current = node
            .properties
            .parent
            .as_ref()
            .and_then(|id| by_id.get(id.as_str()).and_then(|&idx| layers.get(idx)));
    }
    BlendMode::Normal
}

/// Compute the visible layer row index range (`start..end`) inside `clip_top..=clip_bottom`
/// for timeline row virtualization (with a 2-row overscan margin).
#[allow(dead_code)]
pub(crate) fn visible_layer_row_range(
    clip_top: f32,
    clip_bottom: f32,
    list_top: f32,
    row_height: f32,
    total_rows: usize,
) -> std::ops::Range<usize> {
    if total_rows == 0 || row_height <= 0.0 || !clip_top.is_finite() || !clip_bottom.is_finite() {
        return 0..total_rows;
    }
    let first = ((clip_top - list_top) / row_height).floor().max(0.0) as usize;
    let last = ((clip_bottom - list_top) / row_height).ceil().max(0.0) as usize;
    let start = first.saturating_sub(2).min(total_rows);
    let end = (last + 2).min(total_rows).max(start);
    start..end
}

/// Resize bounds of the layer column and the frame area it must leave. The toggle columns and the
/// Mode cell keep their size in every density, so these widths do not scale.
const LAYER_COLUMN_MIN: f32 = 140.;
const LAYER_COLUMN_MAX: f32 = 420.;
const FRAMES_MIN: f32 = 100.;
/// Layer icon and name keep at least this much before the Mode column appears.
const LAYER_NAME_MIN: f32 = 100.;

/// Layer column width from which the header and the rows show the `Mode` cell: name, Mode cell,
/// four toggle columns and the insets on both sides of the toggles.
fn mode_column_from() -> f32 {
    LAYER_NAME_MIN
        + neoflash_ui::metrics::LAYER_MODE
        + 4. * LAYER_TOGGLE
        + 2. * neoflash_ui::components::timeline::TOGGLES_INSET
}

/// Context menus with long command labels: one menu plus one numeric field wide.
fn wide_menu_width() -> f32 {
    neoflash_ui::metrics::menu_width() + neoflash_ui::metrics::field_num()
}

impl Editor {
    /// Return the virtualization telemetry recorded by the most recent `timeline_contents` pass.
    #[allow(dead_code)]
    pub fn last_timeline_virtual_stats(&self, ctx: &egui::Context) -> Option<TimelineVirtualStats> {
        ctx.data(|d| d.get_temp::<TimelineVirtualStats>(egui::Id::new(TIMELINE_VIRTUAL_STATS_ID)))
    }

    /// Return the `TimelineVirtualWindow` recorded by the most recent `timeline_contents` pass.
    #[allow(dead_code)]
    pub fn last_timeline_virtual_window(
        &self,
        ctx: &egui::Context,
    ) -> Option<TimelineVirtualWindow> {
        self.last_timeline_virtual_stats(ctx).map(|s| s.window)
    }

    /// Test helper: paint the dock edge that currently holds the Timeline.
    #[cfg(test)]
    pub fn timeline(&mut self, ctx: &egui::Context) {
        let edge = self
            .workspace
            .dock
            .location(PanelId::Timeline)
            .map_or(crate::workspace::DockEdge::Bottom, |location| location.edge);
        self.dock_edge(ctx, edge);
    }
    pub(crate) fn timeline_contents(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.handle_timeline_accesskit_actions(&ctx);
        let palette = UiPalette::current(&ctx);
        let p = neoflash_ui::palette(&ctx);
        let id = (
            "neoflash-panel-content",
            PanelId::Timeline,
            self.clipboard.session.clone(),
            self.symbol_id().to_owned(),
        );
        #[derive(Clone)]
        struct LayerRename {
            layer: String,
            text: String,
            focus: bool,
        }
        #[derive(Clone)]
        struct LayerReorder {
            symbol: String,
            layer: String,
            start: Pos2,
            moved: bool,
        }
        #[derive(Clone)]
        struct ColumnResize {
            start: Pos2,
            width: f32,
            saved: f32,
            moved: bool,
        }
        let reorder_id = egui::Id::new((&id, "layer-reorder"));
        let resize_id = egui::Id::new((&id, "layer-column-resize"));
        let mut reorder = ctx.data(|data| data.get_temp::<LayerReorder>(reorder_id));
        let mut resize = ctx.data(|data| data.get_temp::<ColumnResize>(resize_id));
        let mut commit_reorder: Option<(String, Option<String>, Option<Option<String>>)> = None;
        let mut commit_reparent_folder: Option<(String, String)> = None;
        let (pressed, released, pointer, down, cancelled) = ctx.input(|input| {
            let mut pressed = None;
            let mut released = None;
            for event in &input.events {
                if let egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: held,
                    ..
                } = event
                {
                    if *held {
                        pressed = Some(*pos);
                        released = None;
                    } else {
                        released = Some(*pos);
                    }
                }
            }
            (
                pressed,
                released,
                input.pointer.latest_pos().or(released),
                input.pointer.primary_down(),
                input.key_pressed(egui::Key::Escape) || !input.focused,
            )
        });
        if cancelled {
            if let Some(resize) = resize.take() {
                self.workspace.sizes.layer_column_width = resize.saved;
            }
            reorder = None;
        }
        let rename_id = egui::Id::new((&id, "layer-rename"));
        let mut rename = ctx.data(|data| data.get_temp::<LayerRename>(rename_id));
        let mut commit_rename = None;
        let mut cancel_rename = false;
        let previous_cell = self.timeline_cell;
        let row_height = self.workspace.timeline.row_height();
        let timeline_panel_rect = ui.max_rect();
        if let Some(pt) = pressed
            && !timeline_panel_rect.contains(pt)
        {
            ctx.data_mut(|d| d.insert_temp(egui::Id::new(TIMELINE_LAYER_FOCUS_ID), false));
        }
        let timeline_width = ui.available_width();
        let maximum_layer_width =
            (timeline_width - FRAMES_MIN).clamp(LAYER_COLUMN_MIN, LAYER_COLUMN_MAX);
        let mut layer_column_width = self
            .workspace
            .sizes
            .layer_column_width
            .clamp(LAYER_COLUMN_MIN, maximum_layer_width);
        ui.scope_builder(egui::UiBuilder::new().id(id), |ui| {
            // Keep a usable frame canvas even in the smallest native Timeline window. Bar controls
            // that do not fit hide by priority into the More menu (R-052).
            ui.spacing_mut().scroll.floating = false;
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            let controls = self.timeline_controls_bar(
                ui,
                layer_column_width,
                timeline_width - layer_column_width - space::s4(),
            );
            let zoom_changed = self.timeline_cell != previous_cell;
            if zoom_changed {
                self.timeline_key_drag = None;
            }
            let scroll_to = controls.scroll_to;
            let recenter = (zoom_changed || controls.recenter) && scroll_to.is_none();
            let view = self.workspace.timeline;
            let button_states = self.symbol().kind == SymbolKind::Button;
            let cell_width = if button_states {
                64.
            } else {
                self.timeline_cell
            };
            let height = (ui.available_height() - ruler_height()).max(neoflash_ui::metrics::row());
            let (header, _) = ui.allocate_exact_size(
                Vec2::new(ui.available_width(), ruler_height()),
                Sense::hover(),
            );
            let divider_x = header.left() + layer_column_width;
            let divider_rect = Rect::from_min_max(
                Pos2::new(divider_x - 3., header.top()),
                Pos2::new(divider_x + 3., header.bottom() + height),
            );
            let divider = ui.interact(
                divider_rect.intersect(ui.clip_rect()),
                ui.id().with("layer-column-divider"),
                Sense::click_and_drag(),
            );
            divider.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    ui.is_enabled(),
                    "Resize layer column",
                )
            });
            if !cancelled
                && let Some(start) = pressed.filter(|point| {
                    divider_rect.contains(*point)
                        && ui.clip_rect().contains(*point)
                        && ui.is_enabled()
                        && ctx.layer_id_at(*point) == Some(ui.layer_id())
                })
            {
                resize = Some(ColumnResize {
                    start,
                    width: layer_column_width,
                    saved: self.workspace.sizes.layer_column_width,
                    moved: false,
                });
                reorder = None;
                self.timeline_key_drag = None;
            }
            if divider.hovered() || resize.is_some() {
                ctx.set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            if let Some(drag) = &mut resize
                && let Some(position) = pointer
            {
                drag.moved |= (position.x - drag.start.x).abs() > 0.5;
                if drag.moved {
                    layer_column_width = (drag.width + position.x - drag.start.x)
                        .clamp(LAYER_COLUMN_MIN, maximum_layer_width);
                    self.workspace.sizes.layer_column_width = layer_column_width;
                    ctx.request_repaint();
                }
            }
            // NF2/TL Layers Header: "LAYERS" + Mode + eye / solo / lock / outline columns.
            let show_mode = layer_column_width >= mode_column_from();
            let header_toggles = ui
                .scope_builder(
                    egui::UiBuilder::new().max_rect(Rect::from_min_size(
                        header.min,
                        Vec2::new(layer_column_width, ruler_height()),
                    )),
                    |ui| {
                        tl::layers_header_with(
                            ui,
                            layer_column_width,
                            tl::LayersHeaderColumns {
                                mode: show_mode,
                                solo: true,
                            },
                        )
                    },
                )
                .inner;
            if header_toggles.clear_solo {
                self.clear_all_solo_layers();
            }
            for (clicked, column) in [
                (header_toggles.all_visibility, ToggleKind::Visibility),
                (header_toggles.all_lock, ToggleKind::Lock),
                (header_toggles.all_outline, ToggleKind::Outline),
            ] {
                if clicked {
                    self.toggle_all_layers(column);
                }
            }
            // Frame exposures are ordered by their frame, independent of JSON order.
            if self
                .symbol()
                .timeline
                .layers
                .iter()
                .any(|l| !l.keyframes.windows(2).all(|w| w[0].frame <= w[1].frame))
            {
                for layer in &mut self.symbol_mut().timeline.layers {
                    layer.keyframes.sort_by_key(|key| key.frame);
                }
            }
            let layer_indices = visible_layer_indices(&self.symbol().timeline);
            if !layer_indices.contains(&self.layer) {
                self.sync_active_layer_to_visible(&layer_indices);
            }
            let total_rows = layer_indices.len();
            let timeline_layers_focused = ctx
                .data(|d| d.get_temp::<bool>(egui::Id::new(TIMELINE_LAYER_FOCUS_ID)))
                .unwrap_or(false);
            if timeline_layers_focused
                && !cancelled
                && rename.is_none()
                && reorder.is_none()
                && resize.is_none()
                && total_rows > 0
                && !self.has_modal_dialog_in(&ctx)
                && !ctx.wants_keyboard_input()
                && ctx.memory(|m| m.focused().is_none())
                && self.selection.is_empty()
            {
                let cur_row = layer_indices
                    .iter()
                    .position(|&idx| idx == self.layer)
                    .unwrap_or(0);
                let page_rows = ((height / row_height).floor() as usize).max(1);
                let nav = ctx.input_mut(|input| {
                    for shift in [true, false] {
                        let mods = if shift {
                            egui::Modifiers::SHIFT
                        } else {
                            egui::Modifiers::NONE
                        };
                        if input.consume_key(mods, egui::Key::ArrowUp) {
                            return Some((cur_row.saturating_sub(1), shift));
                        }
                        if input.consume_key(mods, egui::Key::ArrowDown) {
                            return Some(((cur_row + 1).min(total_rows - 1), shift));
                        }
                        if input.consume_key(mods, egui::Key::PageUp) {
                            return Some((cur_row.saturating_sub(page_rows), shift));
                        }
                        if input.consume_key(mods, egui::Key::PageDown) {
                            return Some(((cur_row + page_rows).min(total_rows - 1), shift));
                        }
                        if input.consume_key(mods, egui::Key::Home) {
                            return Some((0, shift));
                        }
                        if input.consume_key(mods, egui::Key::End) {
                            return Some((total_rows - 1, shift));
                        }
                    }
                    None
                });
                if let Some((target_row, extend_shift)) = nav
                    && let Some(&target_idx) = layer_indices.get(target_row)
                {
                    self.finish_edits();
                    self.timeline_key_drag = None;
                    self.select_layer_row(target_idx, extend_shift, false);
                    self.clear_selection();
                }
            }
            let count = self.symbol().timeline.frame_count;
            let mut script_frames: Vec<u32> = self
                .symbol()
                .timeline
                .scripts
                .iter()
                .filter(|script| !script.script.source.trim().is_empty())
                .map(|script| script.frame)
                .collect();
            script_frames.sort_unstable();
            script_frames.dedup();
            let content_height = total_rows as f32 * row_height;
            // Solid scrollbars own a separate strip instead of covering the last
            // visible key. Content drags belong to frames, not kinetic scrolling.
            let scroll_source = egui::scroll_area::ScrollSource::SCROLL_BAR
                | egui::scroll_area::ScrollSource::MOUSE_WHEEL;
            let mut grid_rect = Rect::NOTHING;
            let mut grid_clip = Rect::NOTHING;
            let mut drawn_layer_rows = 0usize;
            let mut measured_layer_list_height = 0.0_f32;
            let mut virtual_stats: Option<TimelineVirtualStats> = None;
            let a11y_active = crate::accessibility_bridge::is_accesskit_active(&ctx);
            let mut a11y_layer_tree: Option<(egui::Id, Rect)> = None;
            let mut a11y_layer_rows: Vec<crate::accessibility_bridge::TimelineLayerRowA11y> =
                Vec::new();
            let mut a11y_grid: Option<crate::accessibility_bridge::TimelineGridA11y> = None;
            let mut a11y_playhead: Option<(egui::Id, Rect)> = None;
            let (id_to_idx, guided_ids) = {
                let layers = &self.symbol().timeline.layers;
                let mut by_id = std::collections::HashMap::with_capacity(layers.len());
                let mut guided = std::collections::HashSet::new();
                for (i, l) in layers.iter().enumerate() {
                    by_id.insert(l.id.clone(), i);
                    if let Some(mg) = &l.properties.motion_guide {
                        guided.insert(mg.clone());
                    }
                }
                (by_id, guided)
            };
            let rows = egui::ScrollArea::vertical()
                .id_salt("timeline_rows")
                .scroll_source(scroll_source)
                .min_scrolled_height(neoflash_ui::metrics::row())
                .max_height(height)
                .auto_shrink([false, false])
                .themed()
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = Vec2::ZERO;
                    ui.horizontal_top(|ui| {
                        ui.allocate_ui_with_layout(
                            Vec2::new(layer_column_width, content_height),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                if a11y_active {
                                    a11y_layer_tree = Some((ui.unique_id(), ui.clip_rect()));
                                }
                                ui.set_width(layer_column_width);
                                ui.spacing_mut().item_spacing.y = 0.;
                                let list_origin = ui.cursor().min;
                                let row_window = TimelineVirtualWindow::with_default_overscan(
                                    ui.clip_rect(),
                                    list_origin,
                                    row_height,
                                    cell_width,
                                    total_rows,
                                    count.max(1),
                                );
                                let row_range = row_window.row_range(total_rows);
                                let mut rendered_rows: Vec<usize> = row_range.collect();
                                if let Some(drag) = &reorder
                                    && let Some(&drag_idx) = id_to_idx.get(&drag.layer)
                                    && let Some(r) =
                                        layer_indices.iter().position(|&idx| idx == drag_idx)
                                {
                                    rendered_rows.push(r);
                                }
                                if let Some(draft) = &rename
                                    && let Some(&ren_idx) = id_to_idx.get(&draft.layer)
                                    && let Some(r) =
                                        layer_indices.iter().position(|&idx| idx == ren_idx)
                                {
                                    rendered_rows.push(r);
                                }
                                rendered_rows.sort_unstable();
                                rendered_rows.dedup();
                                let mut cursor_row = 0usize;
                                for row_index in rendered_rows {
                                    if row_index > cursor_row {
                                        let gap_spacer =
                                            (row_index - cursor_row) as f32 * row_height;
                                        ui.allocate_exact_size(
                                            Vec2::new(layer_column_width, gap_spacer),
                                            Sense::hover(),
                                        );
                                    }
                                    cursor_row = row_index + 1;
                                    let Some(&idx) = layer_indices.get(row_index) else {
                                        continue;
                                    };
                                    let (layer, depth, kind, effective_blend_label, color) = {
                                        let timeline = &self.symbol().timeline;
                                        let Some(real_layer) = timeline.layers.get(idx) else {
                                            continue;
                                        };
                                        let depth = if total_rows <= 4 {
                                            layer_depth(timeline, real_layer)
                                        } else {
                                            fast_layer_depth(
                                                &timeline.layers,
                                                &id_to_idx,
                                                real_layer,
                                            )
                                        };
                                        let kind = if total_rows <= 4 {
                                            layer_kind(timeline, real_layer)
                                        } else {
                                            fast_layer_kind(real_layer, &guided_ids)
                                        };
                                        let effective_blend_label =
                                            fast_layer_effective_blend_mode(
                                                &timeline.layers,
                                                &id_to_idx,
                                                real_layer,
                                            )
                                            .label();
                                        let color = layer_color(real_layer);
                                        let header_layer = Layer {
                                            id: real_layer.id.clone(),
                                            name: real_layer.name.clone(),
                                            visible: real_layer.visible,
                                            locked: real_layer.locked,
                                            keyframes: Vec::new(),
                                            properties: real_layer.properties.clone(),
                                        };
                                        (header_layer, depth, kind, effective_blend_label, color)
                                    };
                                    drawn_layer_rows += 1;
                                    let (row_rect, row_response) = ui.allocate_exact_size(
                                        Vec2::new(layer_column_width, row_height),
                                        Sense::click_and_drag(),
                                    );
                                    let renaming = rename
                                        .as_ref()
                                        .is_some_and(|draft| draft.layer == layer.id);
                                    let is_mask_row = layer.properties.role == LayerRole::Mask;
                                    let mode_cell = if !show_mode {
                                        tl::ModeCell::Hidden
                                    } else if is_mask_row
                                        || matches!(
                                            kind,
                                            tl::LayerKind::Guide
                                                | tl::LayerKind::MotionGuide
                                                | tl::LayerKind::Sound
                                        )
                                    {
                                        tl::ModeCell::Empty
                                    } else {
                                        tl::ModeCell::Mode(effective_blend_label)
                                    };
                                    let row_selected = self.is_layer_row_selected(idx);
                                    let row = LayerRow {
                                        label: if renaming { "" } else { &layer.name },
                                        kind,
                                        selected: row_selected,
                                        indent: depth,
                                        editing: !self.are_layers_deselected() && idx == self.layer,
                                        visible: layer.visible,
                                        locked: layer.locked,
                                        outline: layer.properties.outline,
                                        color,
                                    }
                                    .paint_at_with(
                                        ui,
                                        row_rect,
                                        row_response,
                                        tl::LayerColumns {
                                            mode: mode_cell,
                                            solo: Some(self.is_layer_soloed(idx)),
                                        },
                                    );
                                    if a11y_active {
                                        let clipped_row = if row_rect.intersects(ui.clip_rect()) {
                                            row_rect.intersect(ui.clip_rect())
                                        } else {
                                            row_rect
                                        };
                                        a11y_layer_rows.push(
                                            crate::accessibility_bridge::TimelineLayerRowA11y {
                                                response_id: row.row.id,
                                                layer_idx: idx,
                                                rect: clipped_row,
                                                kind,
                                                depth,
                                                row_selected,
                                            },
                                        );
                                    }
                                    if row.disclosure {
                                        self.toggle_layer_folder(idx);
                                    }
                                    if row.visibility {
                                        if ui.input(|i| i.modifiers.alt) {
                                            self.hide_other_layers(idx);
                                        } else {
                                            self.toggle_selected_layers_visibility(idx);
                                        }
                                    }
                                    if row.solo {
                                        let alt = ui.input(|i| i.modifiers.alt);
                                        if !alt && self.is_layer_row_selected(idx) {
                                            let targets = self.target_layer_indices(idx);
                                            if targets.len() > 1 {
                                                let target_solo = !self.is_layer_soloed(idx);
                                                for ti in targets {
                                                    if self.is_layer_soloed(ti) != target_solo {
                                                        self.toggle_layer_solo(ti, false);
                                                    }
                                                }
                                            } else {
                                                self.toggle_layer_solo(idx, false);
                                            }
                                        } else {
                                            self.toggle_layer_solo(idx, alt);
                                        }
                                    }
                                    if row.lock {
                                        if ui.input(|i| i.modifiers.alt) {
                                            self.lock_other_layers(idx);
                                        } else {
                                            self.toggle_selected_layers_lock(idx);
                                        }
                                    }
                                    if row.outline {
                                        self.toggle_selected_layers_outline(idx);
                                    }
                                    if let Some(mode_rect) = row.mode_rect {
                                        if is_mask_row && show_mode {
                                            self.paint_mask_row_mode_and_icons(
                                                ui,
                                                row.row.id,
                                                mode_rect,
                                                row_rect,
                                                idx,
                                                layer.properties.mask_mode,
                                                layer.properties.mask_invert,
                                                row_selected,
                                            );
                                        } else if matches!(mode_cell, tl::ModeCell::Mode(_)) {
                                            let mode_popup_id = row.row.id.with("mode-popup");
                                            egui::Popup::new(
                                                mode_popup_id,
                                                ui.ctx().clone(),
                                                egui::PopupAnchor::ParentRect(mode_rect),
                                                ui.layer_id(),
                                            )
                                            .kind(egui::PopupKind::Menu)
                                            .open_memory(
                                                row.mode.then_some(egui::SetOpenCommand::Toggle),
                                            )
                                            .gap(space::s1())
                                            .width(neoflash_ui::metrics::menu_width())
                                            .frame(nf::popover_frame(ui.ctx()))
                                            .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
                                            .show(
                                                |ui| {
                                                    nf::menu::menu_style(ui.style_mut());
                                                    ui.set_min_width(
                                                        neoflash_ui::metrics::menu_width(),
                                                    );
                                                    crate::layer_controls::populate_blend_mode_menu(
                                                        ui,
                                                        layer.properties.blend_mode,
                                                        |mode| {
                                                            self.set_layer_blend_mode(idx, mode);
                                                        },
                                                    );
                                                },
                                            );
                                        }
                                    }
                                    // The outline swatch keeps its right-click colour menu.
                                    let swatch = Rect::from_min_size(
                                        Pos2::new(
                                            row_rect.right()
                                                - neoflash_ui::components::timeline::TOGGLES_INSET
                                                - LAYER_TOGGLE,
                                            row_rect.top(),
                                        ),
                                        Vec2::new(LAYER_TOGGLE, row_rect.height()),
                                    );
                                    self.record_layer_outline_swatch_rect(
                                        ui.ctx(),
                                        &layer.id,
                                        swatch,
                                    );
                                    let swatch = ui.interact(
                                        swatch,
                                        row.row.id.with(ToggleKind::Outline as u8),
                                        Sense::click(),
                                    );
                                    self.layer_outline_menu(&swatch, &layer);
                                    if let Some(draft) =
                                        rename.as_mut().filter(|draft| draft.layer == layer.id)
                                    {
                                        let rename_style =
                                            if row_height < neoflash_ui::metrics::LAYER_ROW_SHORT {
                                                TextStyle::Small
                                            } else {
                                                TextStyle::Body
                                            };
                                        // The NF text field in its row-high inline form.
                                        let mut slot = ui.new_child(
                                            egui::UiBuilder::new()
                                                .id_salt(("layer-name", &layer.id))
                                                .max_rect(row.label_rect)
                                                .layout(egui::Layout::left_to_right(
                                                    egui::Align::Min,
                                                )),
                                        );
                                        let mut field = slot.add(
                                            nf::TextField::new(&mut draft.text, "Layer name")
                                                .hint("Layer name")
                                                .compact(row.label_rect.height())
                                                .text_style(rename_style)
                                                .width(row.label_rect.width()),
                                        );
                                        field.id = nf::TextField::edit_id(field.id);
                                        if draft.focus {
                                            field.request_focus();
                                            if let Some(mut state) =
                                                egui::text_edit::TextEditState::load(
                                                    ui.ctx(),
                                                    field.id,
                                                )
                                            {
                                                state.cursor.set_char_range(Some(
                                                    egui::text::CCursorRange::two(
                                                        egui::text::CCursor::new(0),
                                                        egui::text::CCursor::new(
                                                            draft.text.chars().count(),
                                                        ),
                                                    ),
                                                ));
                                                state.store(ui.ctx(), field.id);
                                            }
                                            draft.focus = false;
                                        } else if ui.input_mut(|i| {
                                            i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
                                        }) {
                                            cancel_rename = true;
                                            field.surrender_focus();
                                        } else if field.lost_focus()
                                            || ui.input_mut(|i| {
                                                i.consume_key(
                                                    egui::Modifiers::NONE,
                                                    egui::Key::Enter,
                                                )
                                            })
                                        {
                                            let _ = ui.input_mut(|i| {
                                                i.consume_key(
                                                    egui::Modifiers::NONE,
                                                    egui::Key::Enter,
                                                )
                                            });
                                            commit_rename = Some((
                                                draft.layer.clone(),
                                                draft.text.trim().to_owned(),
                                            ));
                                            field.surrender_focus();
                                        }
                                        if !cancel_rename && commit_rename.is_none() {
                                            super::dialog_helpers::mark_inline_edit_active(
                                                ui.ctx(),
                                            );
                                        }
                                        continue;
                                    }
                                    // Name area (left of the column toggles) starts a reorder drag.
                                    let grab = Rect::from_min_max(
                                        row_rect.min,
                                        Pos2::new(
                                            row_rect.right()
                                                - neoflash_ui::components::timeline::TOGGLES_INSET
                                                - 4. * LAYER_TOGGLE,
                                            row_rect.bottom(),
                                        ),
                                    );
                                    if !cancelled
                                        && resize.is_none()
                                        && let Some(start) = pressed.filter(|point| {
                                            grab.contains(*point)
                                                && ui.clip_rect().contains(*point)
                                                && ui.is_enabled()
                                                && ctx.layer_id_at(*point) == Some(ui.layer_id())
                                        })
                                    {
                                        let mods = ui.input(|i| i.modifiers);
                                        if !mods.shift
                                            && !mods.command
                                            && !self.is_layer_row_selected(idx)
                                        {
                                            self.finish_edits();
                                            self.timeline_key_drag = None;
                                            self.select_layer_row(idx, false, false);
                                            self.clear_selection();
                                        }
                                        reorder = Some(LayerReorder {
                                            symbol: self.symbol_id().to_owned(),
                                            layer: layer.id.clone(),
                                            start,
                                            moved: false,
                                        });
                                    }
                                    let response = row.row;
                                    if response.clicked() {
                                        ctx.data_mut(|d| {
                                            d.insert_temp(
                                                egui::Id::new(TIMELINE_LAYER_FOCUS_ID),
                                                true,
                                            );
                                        });
                                        let mods = ui.input(|i| i.modifiers);
                                        self.finish_edits();
                                        self.timeline_key_drag = None;
                                        self.select_layer_row(idx, mods.shift, mods.command);
                                        self.clear_selection();
                                    } else if response.secondary_clicked() {
                                        ctx.data_mut(|d| {
                                            d.insert_temp(
                                                egui::Id::new(TIMELINE_LAYER_FOCUS_ID),
                                                true,
                                            );
                                        });
                                        self.finish_edits();
                                        self.timeline_key_drag = None;
                                        if !self.is_layer_row_selected(idx) {
                                            self.select_layer_row(idx, false, false);
                                        } else {
                                            let previous_layer = self.layer;
                                            self.layer = idx;
                                            if self.layer != previous_layer {
                                                self.clear_timeline_frame_selection();
                                            }
                                        }
                                        self.clear_selection();
                                    }
                                    if response.double_clicked() {
                                        reorder = None;
                                        let pointer = response
                                            .interact_pointer_pos()
                                            .or_else(|| ui.input(|i| i.pointer.latest_pos()));
                                        if pointer.is_some_and(|p| p.x < row.label_rect.left()) {
                                            self.open_layer_properties(ui.ctx(), idx);
                                        } else {
                                            rename = Some(LayerRename {
                                                layer: layer.id.clone(),
                                                text: layer.name.clone(),
                                                focus: true,
                                            });
                                        }
                                        ui.ctx().request_repaint();
                                    }
                                    neoflash_ui::components::tooltip(
                                        response,
                                        "Drag to reorder. Double-click to rename layer",
                                        None,
                                    )
                                    .context_menu(|ui| {
                                        if self.timeline_layer_menu(ui, idx) {
                                            rename = Some(LayerRename {
                                                layer: layer.id.clone(),
                                                text: layer.name.clone(),
                                                focus: true,
                                            });
                                            ui.ctx().request_repaint();
                                        }
                                        crate::ui_icons::menu_contents(
                                            ui,
                                            wide_menu_width(),
                                            |ui| {
                                                neoflash_ui::components::menu_separator(ui);
                                                if crate::ui_icons::menu_item_enabled(
                                                    ui,
                                                    self.can_link_av_layers(),
                                                    "Link video & audio",
                                                    "",
                                                )
                                                .clicked()
                                                {
                                                    self.link_selected_or_adjacent_av_layers();
                                                    ui.close();
                                                }
                                                if crate::ui_icons::menu_item_enabled(
                                                    ui,
                                                    self.can_unlink_av_layers(),
                                                    "Unlink video & audio",
                                                    "",
                                                )
                                                .clicked()
                                                {
                                                    self.unlink_active_layers();
                                                    ui.close();
                                                }
                                                neoflash_ui::components::menu_separator(ui);
                                                if crate::ui_icons::menu_item(
                                                    ui,
                                                    "Delete empty layers",
                                                    "",
                                                )
                                                .clicked()
                                                {
                                                    self.delete_empty_layers();
                                                    ui.close();
                                                }
                                                if crate::ui_icons::menu_item(
                                                    ui,
                                                    "Rename layers from symbol",
                                                    "",
                                                )
                                                .clicked()
                                                {
                                                    self.rename_layers_from_content();
                                                    ui.close();
                                                }
                                                if crate::ui_icons::menu_item(
                                                    ui,
                                                    "Add actions & stop() layer",
                                                    "",
                                                )
                                                .clicked()
                                                {
                                                    self.add_actions_and_stop_layer();
                                                    ui.close();
                                                }
                                            },
                                        );
                                    });
                                }
                                if total_rows > cursor_row {
                                    let bottom_spacer =
                                        (total_rows - cursor_row) as f32 * row_height;
                                    ui.allocate_exact_size(
                                        Vec2::new(layer_column_width, bottom_spacer),
                                        Sense::hover(),
                                    );
                                }
                                measured_layer_list_height = ui.cursor().min.y - list_origin.y;
                            },
                        );
                        let frames = egui::ScrollArea::horizontal()
                            .id_salt("frames")
                            .scroll_source(scroll_source)
                            .scroll_bar_visibility(
                                egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded,
                            )
                            .auto_shrink([false, false])
                            .themed()
                            .show(ui, |ui| {
                                let width =
                                    ((count + 1) as f32 * cell_width).max(ui.available_width());
                                let hscroll_strip = ui.spacing().scroll.bar_width
                                    + ui.spacing().scroll.bar_inner_margin
                                    + ui.spacing().scroll.bar_outer_margin;
                                let grid_min_height =
                                    (height - hscroll_strip).max(neoflash_ui::metrics::row());
                                let (rect, response) = ui.allocate_exact_size(
                                    Vec2::new(width, content_height.max(grid_min_height)),
                                    Sense::click_and_drag(),
                                );
                                let grid_count = if button_states {
                                    count
                                } else {
                                    ((rect.width() / cell_width).floor().max(1.) as u32)
                                        .max(count)
                                        .min(1_000_000)
                                };
                                grid_rect = rect;
                                grid_clip = ui.clip_rect().intersect(rect);
                                if response.clicked() || response.secondary_clicked() {
                                    ctx.data_mut(|d| {
                                        d.insert_temp(egui::Id::new(TIMELINE_LAYER_FOCUS_ID), true);
                                    });
                                }
                                let was_deselected = self.are_layers_deselected();
                                self.timeline_grid_interact(
                                    ui,
                                    &response,
                                    rect,
                                    grid_clip,
                                    cell_width,
                                    grid_count,
                                    &layer_indices,
                                    resize.is_some() || reorder.is_some(),
                                );
                                if was_deselected && !self.are_layers_deselected() {
                                    ui.ctx().request_repaint();
                                }
                                let painter = ui.painter_at(rect);
                                painter.rect_filled(rect, 0, palette.panel_fill);
                                let mut stats = self.paint_timeline_rows(
                                    &painter,
                                    rect,
                                    grid_clip,
                                    cell_width,
                                    &layer_indices,
                                    &script_frames,
                                    view.tinted,
                                );
                                stats.drawn_layer_rows = drawn_layer_rows;
                                stats.layer_list_height = measured_layer_list_height;
                                virtual_stats = Some(stats);
                                if a11y_active {
                                    a11y_grid =
                                        Some(crate::accessibility_bridge::TimelineGridA11y {
                                            response_id: response.id,
                                            grid_rect: rect,
                                            grid_clip,
                                            cell_width,
                                            row_height,
                                        });
                                }
                                self.paint_frame_selection(
                                    ui,
                                    rect,
                                    grid_clip,
                                    cell_width,
                                    &layer_indices,
                                );
                                let selected_cell = Rect::from_min_size(
                                    Pos2::new(
                                        rect.left() + self.frame as f32 * cell_width,
                                        rect.top()
                                            + layer_indices
                                                .iter()
                                                .position(|i| *i == self.layer)
                                                .unwrap_or(0)
                                                as f32
                                                * row_height,
                                    ),
                                    Vec2::new(cell_width, row_height),
                                );
                                if response.hovered()
                                    && let Some(pos) = response.hover_pos()
                                    && let Some((frame, layer_row)) = timeline_cell_at(
                                        pos, rect.min, cell_width, grid_count, total_rows,
                                        row_height,
                                    )
                                    && let Some(&layer_idx) = layer_indices.get(layer_row)
                                    && let Some(hover_layer) =
                                        self.symbol().timeline.layers.get(layer_idx)
                                {
                                    let layer_end = hover_layer
                                        .properties
                                        .end_frame
                                        .unwrap_or(count)
                                        .min(count);
                                    let key = (frame < layer_end)
                                        .then(|| hover_layer.key_at(frame))
                                        .flatten();
                                    if key.is_some_and(|key| key.frame == frame)
                                        && !hover_layer.locked
                                        && hover_layer.visible
                                    {
                                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                                    }
                                }
                                if response.dragged_by(egui::PointerButton::Primary)
                                    && let Some((symbol, layer_id, from)) = &self.timeline_key_drag
                                    && symbol == self.symbol_id()
                                    && let Some(layer_idx) = self
                                        .symbol()
                                        .timeline
                                        .layers
                                        .iter()
                                        .position(|layer| &layer.id == layer_id)
                                {
                                    let layer = &self.symbol().timeline.layers[layer_idx];
                                    let key_conflict = self.frame != *from
                                        && layer
                                            .keyframes
                                            .binary_search_by_key(&self.frame, |key| key.frame)
                                            .is_ok();
                                    let script_conflict = self.frame != *from
                                        && self
                                            .symbol()
                                            .timeline
                                            .key_carries_frame_script(layer_idx, *from)
                                        && self.symbol().timeline.has_frame_script(self.frame);
                                    let blocked = key_conflict || script_conflict;
                                    let stroke = Stroke::new(
                                        2_f32,
                                        if blocked {
                                            palette.error_text
                                        } else {
                                            palette.accent
                                        },
                                    );
                                    painter.rect_stroke(
                                        selected_cell.shrink(1.),
                                        0,
                                        stroke,
                                        egui::StrokeKind::Inside,
                                    );
                                    ui.ctx().set_cursor_icon(if blocked {
                                        egui::CursorIcon::NotAllowed
                                    } else {
                                        egui::CursorIcon::Grabbing
                                    });
                                    let text = if key_conflict {
                                        "Destination already has a keyframe".to_owned()
                                    } else if script_conflict {
                                        "Destination already has a frame script".to_owned()
                                    } else {
                                        format!(
                                            "{} key {} → {}",
                                            if ui.input(|i| i.modifiers.alt) {
                                                "Copy"
                                            } else {
                                                "Move"
                                            },
                                            from + 1,
                                            self.frame + 1
                                        )
                                    };
                                    painter.text(
                                        Pos2::new(
                                            grid_clip.left() + space::s1(),
                                            grid_clip.bottom() - space::s1(),
                                        ),
                                        Align2::LEFT_BOTTOM,
                                        text,
                                        typography::font(TextStyle::Small),
                                        stroke.color,
                                    );
                                }
                                if response.context_menu_opened()
                                    || response.interact_pointer_pos().is_some_and(|pos| {
                                        timeline_cell_at(
                                            pos, rect.min, cell_width, grid_count, total_rows,
                                            row_height,
                                        )
                                        .is_some()
                                    })
                                {
                                    response.context_menu(|ui| {
                                        crate::ui_icons::menu_contents(
                                            ui,
                                            wide_menu_width(),
                                            |ui| self.frame_context_menu(ui),
                                        );
                                    });
                                }
                            });
                        if let Some(first) = scroll_to {
                            let mut state = frames.state;
                            let maximum =
                                (frames.content_size.x - frames.inner_rect.width()).max(0.);
                            state.offset.x = (first as f32 * cell_width).clamp(0., maximum);
                            state.store(ui.ctx(), frames.id);
                            ui.ctx().request_repaint();
                        }
                        if recenter {
                            let mut state = frames.state;
                            let maximum =
                                (frames.content_size.x - frames.inner_rect.width()).max(0.);
                            state.offset.x = ((self.frame as f32 + 0.5) * cell_width
                                - frames.inner_rect.width() * 0.5)
                                .clamp(0., maximum);
                            state.store(ui.ctx(), frames.id);
                            ui.ctx().request_repaint();
                        }
                        // Continue an existing key drag at the horizontal viewport edge;
                        // never start a drag from a scrollbar or an out-of-range cell.
                        if self.timeline_edit.dragging()
                            && ui.input(|i| {
                                i.pointer.primary_down() && i.pointer.is_decidedly_dragging()
                            })
                            && let Some(pos) = ui.input(|i| i.pointer.latest_pos())
                        {
                            let margin = space::s5();
                            let direction = if pos.x < frames.inner_rect.left() + margin {
                                -1.
                            } else if pos.x > frames.inner_rect.right() - margin {
                                1.
                            } else {
                                0.
                            };
                            if direction != 0. {
                                let mut state = frames.state;
                                let dt = ui.input(|i| i.stable_dt).min(0.05);
                                let maximum =
                                    (frames.content_size.x - frames.inner_rect.width()).max(0.);
                                let next =
                                    (state.offset.x + direction * 360. * dt).clamp(0., maximum);
                                if next != state.offset.x {
                                    state.offset.x = next;
                                    state.store(ui.ctx(), frames.id);
                                    ui.ctx().request_repaint();
                                }
                            }
                        }
                    });
                });
            let mut active_scroll_y = rows.state.offset.y;
            let revealed_layer_id = ui.id().with("revealed-active-layer");
            let current_active_layer_id = if self.are_layers_deselected() {
                String::new()
            } else {
                self.symbol()
                    .timeline
                    .layers
                    .get(self.layer)
                    .map(|l| l.id.clone())
                    .unwrap_or_default()
            };
            let previous_active_layer_id = ctx.data(|d| d.get_temp::<String>(revealed_layer_id));
            ctx.data_mut(|d| d.insert_temp(revealed_layer_id, current_active_layer_id.clone()));
            if let Some(prev_id) = previous_active_layer_id
                && !current_active_layer_id.is_empty()
                && prev_id != current_active_layer_id
                && reorder.is_none()
                && !self.timeline_edit.dragging()
                && let Some(visual_row) = layer_indices.iter().position(|&idx| idx == self.layer)
            {
                let view_h = rows.inner_rect.height().max(row_height);
                let max_y = (rows.content_size.y - rows.inner_rect.height()).max(0.0);
                let row_top = visual_row as f32 * row_height;
                let row_bottom = row_top + row_height;
                let next_y = if row_top < active_scroll_y {
                    row_top
                } else if row_bottom > active_scroll_y + view_h {
                    row_bottom - view_h
                } else {
                    active_scroll_y
                }
                .clamp(0.0, max_y);
                if (next_y - active_scroll_y).abs() > 0.5 {
                    let mut state = rows.state;
                    state.offset.y = next_y;
                    state.store(&ctx, rows.id);
                    active_scroll_y = next_y;
                    ctx.request_repaint();
                }
            }
            if let Some(mut stats) = virtual_stats {
                stats.scroll_offset_y = active_scroll_y;
                ctx.data_mut(|data| {
                    data.insert_temp(egui::Id::new(TIMELINE_VIRTUAL_STATS_ID), stats);
                });
            }
            // Clicking (left or right) in the unused Layers space below the final visible row
            // deselects all layers; right-clicking opens the compact 10-item contextless menu.
            // Keep this response invisible and outside the frame grid so blank space never becomes
            // a frame selection target.
            let blank_layers_rect = Rect::from_min_max(
                Pos2::new(header.left(), grid_rect.top() + content_height),
                Pos2::new(divider_x - 3.0, rows.inner_rect.bottom()),
            )
            .intersect(rows.inner_rect)
            .intersect(ui.clip_rect());
            if blank_layers_rect.is_positive() {
                let response = ui.interact(
                    blank_layers_rect,
                    ui.id().with("blank-layers-context"),
                    Sense::click(),
                );
                if response.clicked()
                    || response.secondary_clicked()
                    || (response.context_menu_opened() && !self.are_layers_deselected())
                {
                    self.deselect_all_layers();
                    ui.ctx().request_repaint();
                }
                response.context_menu(|ui| {
                    self.timeline_blank_layers_menu(ui);
                });
            }
            // Continue an existing frame-range drag at the vertical viewport edge.
            if self.timeline_edit.dragging()
                && ui.input(|i| i.pointer.primary_down() && i.pointer.is_decidedly_dragging())
                && let Some(pos) = pointer
            {
                let margin = space::s5();
                let direction = if pos.y < rows.inner_rect.top() + margin {
                    -1.
                } else if pos.y > rows.inner_rect.bottom() - margin {
                    1.
                } else {
                    0.
                };
                if direction != 0. {
                    let mut state = rows.state;
                    let dt = ui.input(|i| i.stable_dt).min(0.05);
                    let maximum = (rows.content_size.y - rows.inner_rect.height()).max(0.);
                    let next = (state.offset.y + direction * 260. * dt).clamp(0., maximum);
                    if next != state.offset.y {
                        state.offset.y = next;
                        state.store(ui.ctx(), rows.id);
                        ctx.request_repaint();
                    }
                }
            }
            ui.painter().vline(
                divider_x,
                header.top()..=rows.inner_rect.bottom(),
                Stroke::new(
                    if resize.is_some() { 2.0_f32 } else { 1.0_f32 },
                    if resize.is_some() {
                        palette.accent
                    } else {
                        palette.divider
                    },
                ),
            );
            if let Some(drag) = &mut reorder {
                fn layer_row_at<'a>(
                    timeline: &'a Timeline,
                    layer_indices: &[usize],
                    row: usize,
                ) -> Option<&'a Layer> {
                    let &idx = layer_indices.get(row)?;
                    timeline.layers.get(idx)
                }
                if drag.symbol != self.symbol_id()
                    || !layer_indices
                        .iter()
                        .any(|&idx| self.symbol().timeline.layers[idx].id == drag.layer)
                {
                    reorder = None;
                } else if let Some(position) = pointer {
                    drag.moved |= position.distance(drag.start) >= 6.;
                    if drag.moved {
                        let names_view = Rect::from_min_max(
                            Pos2::new(header.left(), rows.inner_rect.top()),
                            Pos2::new(header.left() + layer_column_width, rows.inner_rect.bottom()),
                        );
                        let x_tolerance = space::s5();
                        let within = position.x >= names_view.left() - x_tolerance
                            && position.x <= names_view.right() + x_tolerance;
                        ctx.set_cursor_icon(if within {
                            egui::CursorIcon::Grabbing
                        } else {
                            egui::CursorIcon::NotAllowed
                        });
                        if within {
                            let clamped_y = position.y.clamp(
                                names_view.top(),
                                (names_view.bottom() - 0.5).max(names_view.top()),
                            );
                            let timeline = &self.symbol().timeline;
                            let rel_y = (clamped_y - grid_rect.top()) / row_height;
                            let hover_row = rel_y.floor().max(0.) as usize;
                            let row_frac = rel_y - hover_row as f32;
                            let folder_target = layer_row_at(timeline, &layer_indices, hover_row)
                                .filter(|l| {
                                    l.properties.role == LayerRole::Folder
                                        && l.id != drag.layer
                                        && (0.22..=0.78).contains(&row_frac)
                                });
                            if let Some(folder) = folder_target {
                                let highlight = Rect::from_min_max(
                                    Pos2::new(
                                        names_view.left(),
                                        grid_rect.top() + hover_row as f32 * row_height,
                                    ),
                                    Pos2::new(
                                        rows.inner_rect.right(),
                                        grid_rect.top() + (hover_row + 1) as f32 * row_height,
                                    ),
                                )
                                .intersect(rows.inner_rect);
                                let hp = ui.painter_at(rows.inner_rect);
                                hp.rect_filled(highlight, 0., p.timeline_row_selected);
                                hp.rect_stroke(
                                    highlight,
                                    0.,
                                    egui::Stroke::new(1.0_f32, p.accent_default),
                                    egui::StrokeKind::Inside,
                                );
                                if released.is_some() {
                                    commit_reparent_folder =
                                        Some((drag.layer.clone(), folder.id.clone()));
                                }
                            } else {
                                let boundary = (rel_y.round().max(0.) as usize).min(total_rows);
                                let y = (grid_rect.top() + boundary as f32 * row_height)
                                    .clamp(names_view.top(), names_view.bottom());
                                let above = boundary
                                    .checked_sub(1)
                                    .and_then(|i| layer_row_at(timeline, &layer_indices, i));
                                let below = layer_row_at(timeline, &layer_indices, boundary);
                                let dragged_parent = timeline
                                    .layers
                                    .iter()
                                    .find(|l| l.id == drag.layer)
                                    .and_then(|l| l.properties.parent.clone());
                                let outdent_threshold = neoflash_ui::metrics::LAYER_INDENT
                                    + neoflash_ui::metrics::control();
                                let new_parent = if let Some(a) = above
                                    && a.properties.role == LayerRole::Folder
                                    && !a.properties.collapsed
                                    && a.id != drag.layer
                                {
                                    Some(Some(a.id.clone()))
                                } else if let Some(b) = below
                                    && b.properties.parent.is_some()
                                    && above
                                        .is_some_and(|a| a.properties.parent == b.properties.parent)
                                {
                                    Some(b.properties.parent.clone())
                                } else if let Some(a) = above
                                    && a.properties.parent.is_some()
                                    && below
                                        .is_none_or(|b| b.properties.parent != a.properties.parent)
                                {
                                    if dragged_parent == a.properties.parent
                                        || position.x < names_view.left() + outdent_threshold
                                    {
                                        Some(below.and_then(|b| b.properties.parent.clone()))
                                    } else {
                                        Some(a.properties.parent.clone())
                                    }
                                } else if below.is_none_or(|b| b.properties.parent.is_none())
                                    && above.is_none_or(|a| {
                                        a.properties.parent.is_none()
                                            && (a.properties.role != LayerRole::Folder
                                                || a.properties.collapsed)
                                    })
                                {
                                    Some(None)
                                } else {
                                    None
                                };
                                let indicator_left =
                                    if new_parent.as_ref().is_some_and(|p| p.is_some()) {
                                        names_view.left() + neoflash_ui::metrics::toggle_row()
                                    } else {
                                        names_view.left()
                                    };
                                nf::surface::drop_indicator(
                                    ui.painter(),
                                    &ctx,
                                    Pos2::new(indicator_left, y),
                                    Pos2::new(rows.inner_rect.right(), y),
                                );
                                if released.is_some() {
                                    commit_reorder = Some((
                                        drag.layer.clone(),
                                        below.map(|layer| layer.id.clone()),
                                        new_parent,
                                    ));
                                }
                            }
                        }
                        if down && within {
                            let wheel_dy = ui.input(|input| {
                                input
                                    .events
                                    .iter()
                                    .filter_map(|ev| match ev {
                                        egui::Event::MouseWheel { unit, delta, .. } => {
                                            let scale = match unit {
                                                egui::MouseWheelUnit::Point => 1.0,
                                                egui::MouseWheelUnit::Line => row_height,
                                                egui::MouseWheelUnit::Page => names_view.height(),
                                            };
                                            Some(-delta.y * scale)
                                        }
                                        _ => None,
                                    })
                                    .sum::<f32>()
                            });
                            let scroll_edge = space::s4();
                            let direction = if position.y < names_view.top() + scroll_edge {
                                -1.
                            } else if position.y > names_view.bottom() - scroll_edge {
                                1.
                            } else {
                                0.
                            };
                            let edge_dy = direction
                                * 220.
                                * ui.input(|input| input.stable_dt).clamp(0.0, 0.05);
                            if edge_dy != 0. || wheel_dy != 0. {
                                let mut state = rows.state;
                                let maximum =
                                    (rows.content_size.y - rows.inner_rect.height()).max(0.);
                                state.offset.y =
                                    (state.offset.y + edge_dy + wheel_dy).clamp(0., maximum);
                                state.store(ui.ctx(), rows.id);
                                ctx.request_repaint();
                            }
                        }
                    }
                }
            }
            if divider.hovered() || resize.is_some() {
                ctx.set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            if released.is_some() || !down {
                reorder = None;
                resize = None;
            }
            // Paint the fixed ruler after scrolling, using the same current X
            // origin as the frame canvas. Layer and grid rows share one Y scroll.
            if grid_rect.is_finite() {
                let ruler = Rect::from_min_max(
                    Pos2::new(grid_clip.left(), header.top()),
                    Pos2::new(grid_clip.right(), header.bottom()),
                );
                let geo = Geometry {
                    origin_x: grid_rect.left(),
                    frame_width: cell_width,
                    row_height,
                };
                let end_x = grid_rect.left() + count as f32 * cell_width;
                let grid_end_x = if button_states {
                    end_x
                } else {
                    grid_rect.right().max(end_x)
                };
                let grid_count = if button_states {
                    count
                } else {
                    ((grid_rect.width() / cell_width).floor().max(1.) as u32)
                        .max(count)
                        .min(1_000_000)
                };
                let rp = ui.painter_at(ruler);
                rp.rect_filled(ruler, 0, p.bg_panel);
                rp.hline(
                    ruler.x_range(),
                    ruler.bottom() - 0.5,
                    Stroke::new(1.0_f32, p.border_subtle),
                );
                let show_onion_range = self.onion || self.timeline_edit.edit_multiple;
                let onion_range = if show_onion_range {
                    let (start, end) = self.onion_range();
                    let (x0, x1) = (geo.frame_x(start + 1), geo.frame_x(end + 2));
                    Some((x0, x1))
                } else {
                    None
                };
                let seconds_row = if view.show_seconds && !button_states {
                    ruler_height() * 0.45
                } else {
                    0.
                };
                if button_states {
                    // Button symbols: Up · Over · Down · Hit instead of frame numbers.
                    for (frame, name) in crate::authoring_symbols::BUTTON_STATES
                        .iter()
                        .enumerate()
                        .take(count as usize)
                    {
                        let x = geo.frame_x(frame as u32 + 1);
                        rp.vline(
                            x + 0.5,
                            ruler.bottom() - 7.0..=ruler.bottom(),
                            Stroke::new(1.0_f32, p.border_default),
                        );
                        rp.text(
                            Pos2::new(x + cell_width / 2., ruler.center().y),
                            Align2::CENTER_CENTER,
                            *name,
                            typography::font(TextStyle::MonoSmall),
                            p.text_tertiary,
                        );
                    }
                } else {
                    // Ticks, frame numbers and seconds span the full visible timeline grid.
                    let digits = grid_count.max(1).ilog10() as f32 + 1.;
                    let overhang = (digits * 7. - cell_width).max(0.);
                    let full_ruler = Rect::from_min_max(
                        Pos2::new(grid_rect.left(), ruler.top()),
                        Pos2::new(grid_end_x + overhang, ruler.bottom()),
                    );
                    let fps = self.project.fps.max(1) as f32;
                    let numbers = rp.with_clip_rect(Rect::from_min_max(
                        Pos2::new(ruler.left(), ruler.top() + seconds_row),
                        Pos2::new(grid_end_x + overhang, ruler.bottom()),
                    ));
                    tl::paint_ruler(&numbers, &ctx, full_ruler, &geo, fps, view.show_seconds);
                    let seconds = rp.with_clip_rect(Rect::from_min_max(
                        ruler.min,
                        Pos2::new(grid_end_x, ruler.top() + seconds_row),
                    ));
                    tl::paint_ruler(&seconds, &ctx, full_ruler, &geo, fps, view.show_seconds);
                }
                let ruler_first = (((ruler.left() - grid_rect.left()) / cell_width)
                    .floor()
                    .max(0.) as u32)
                    .min(count);
                let ruler_last = (((ruler.right() - grid_rect.left()) / cell_width)
                    .ceil()
                    .max(0.) as u32)
                    .min(count);
                let start_sf = script_frames.partition_point(|&f| f < ruler_first);
                for &frame in &script_frames[start_sf..] {
                    if frame > ruler_last {
                        break;
                    }
                    let needs_ruler_marker =
                        self.symbol().timeline.frame_has_unplaced_script(frame);
                    if needs_ruler_marker && frame < count {
                        let cell = geo.cell(ruler, frame + 1);
                        rp.text(
                            Pos2::new(
                                cell.center().x,
                                ruler.top() + seconds_row / 2. + space::s1(),
                            ),
                            Align2::CENTER_CENTER,
                            "a",
                            typography::font(TextStyle::MonoSmall),
                            palette.timeline_script,
                        );
                    }
                }
                let response =
                    ui.interact(ruler, ui.id().with("ruler_seek"), Sense::click_and_drag());
                if a11y_active {
                    a11y_playhead = Some((response.id, ruler.intersect(ui.clip_rect())));
                }
                let mut onion_handle_busy = false;
                if let Some((x0, x1)) = onion_range {
                    let handles = tl::paint_onion_range(&rp, &ctx, ruler, x0, x1);
                    let mid = (x0 + x1) * 0.5;
                    let r0 = Rect::from_min_max(
                        Pos2::new(handles.0.min.x - 3.0, ruler.top()),
                        Pos2::new((handles.0.max.x + 3.0).min(mid), ruler.bottom()),
                    );
                    let r1 = Rect::from_min_max(
                        Pos2::new((handles.1.min.x - 3.0).max(mid), ruler.top()),
                        Pos2::new(handles.1.max.x + 3.0, ruler.bottom()),
                    );
                    let last = count.saturating_sub(1);
                    for (is_start, r) in [(true, r0), (false, r1)] {
                        let hit_rect = r.intersect(ruler);
                        let handle_id = ui.id().with(("onion", is_start));
                        let handle = ui.interact(hit_rect, handle_id, Sense::click_and_drag());
                        let press_key = handle_id.with("press_origin_x");
                        let released_drag_pos = if ctx.input(|i| i.pointer.primary_down()) {
                            if let Some(origin) = ctx.input(|i| i.pointer.press_origin())
                                && hit_rect.contains(origin)
                            {
                                ui.data_mut(|d| d.insert_temp(press_key, origin.x));
                            }
                            None
                        } else if let Some(origin_x) =
                            ui.data_mut(|d| d.remove_temp::<f32>(press_key))
                        {
                            ctx.input(|i| {
                                i.pointer
                                    .latest_pos()
                                    .or_else(|| i.pointer.interact_pos())
                                    .filter(|latest| (latest.x - origin_x).abs() >= 1.0)
                            })
                        } else {
                            None
                        };
                        if handle.hovered()
                            || handle.dragged()
                            || handle.drag_stopped()
                            || handle.clicked()
                            || handle.is_pointer_button_down_on()
                            || released_drag_pos.is_some()
                        {
                            onion_handle_busy = true;
                            ctx.set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                        }
                        let active_pos = released_drag_pos.or_else(|| {
                            (handle.dragged() || handle.drag_stopped()).then(|| {
                                handle
                                    .interact_pointer_pos()
                                    .or_else(|| ctx.input(|i| i.pointer.latest_pos()))
                            })?
                        });
                        if let Some(pos) = active_pos {
                            let raw = (pos.x - grid_rect.left()) / cell_width
                                - if is_start { 0.0 } else { 1.0 };
                            let f = (raw.round().max(0.) as u32).min(last);
                            if self.onion_anchor {
                                if is_start {
                                    self.anchored_range.0 = f.min(self.anchored_range.1.min(last));
                                } else {
                                    self.anchored_range.1 =
                                        f.min(last).max(self.anchored_range.0.min(last));
                                }
                                self.onion_before =
                                    self.frame.saturating_sub(self.anchored_range.0);
                                self.onion_after = self.anchored_range.1.saturating_sub(self.frame);
                            } else if is_start {
                                self.onion_before = self.frame.saturating_sub(f.min(self.frame));
                            } else {
                                self.onion_after = f.max(self.frame).saturating_sub(self.frame);
                            }
                            self.workspace.onion_range = [self.onion_before, self.onion_after];
                        }
                    }
                }
                if !onion_handle_busy
                    && (response.clicked() || response.dragged())
                    && let Some(pos) = response.interact_pointer_pos()
                {
                    self.finish_edits();
                    self.timeline_key_drag = None;
                    self.set_playing(false);
                    if pos.x < grid_end_x {
                        self.reset_authored_preview();
                        let frame = frame_at_x(pos.x, grid_rect.left(), cell_width, grid_count);
                        self.frame = if view.snap_playhead
                            && !ui.input(|i| i.modifiers.shift)
                            && frame < count
                        {
                            self.snap_scrub_frame(frame, pos.x, grid_rect.left(), cell_width)
                        } else {
                            frame
                        };
                        if !self.are_layers_deselected() {
                            self.select_frame_range(FrameRange {
                                layers: vec![self.symbol().timeline.layers[self.layer].id.clone()],
                                start: self.frame,
                                end: self.frame,
                            });
                        }
                        self.script_target = ScriptTarget::Frame;
                        self.clear_selection();
                    }
                }
                // NF2/TL Playhead over ruler and rows: frame chip, time chip on the seconds row.
                let rows_bottom = (grid_rect.top() + content_height)
                    .min(grid_clip.bottom())
                    .max(header.bottom());
                let full = Rect::from_min_max(ruler.min, Pos2::new(ruler.right(), rows_bottom));
                let time = (seconds_row > 0.)
                    .then(|| format!("{:.2}s", self.frame as f64 / self.project.fps.max(1) as f64));
                tl::paint_playhead(
                    &ui.painter_at(full.expand2(Vec2::new(0., 1.))),
                    &ctx,
                    full,
                    ruler,
                    &geo,
                    geo.frame_x(self.frame + 1),
                    self.frame + 1,
                    time.as_deref(),
                );
            }
            if let Some(crate::library::LibraryDrag::Symbol(sym_id)) =
                self.library_panel.drag.clone()
            {
                if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                    self.library_panel.drag = None;
                } else {
                    let alt = ctx.input(|i| i.modifiers.alt);
                    if alt
                        && let Some(pos) = pointer
                        && (rows.inner_rect.contains(pos) || grid_clip.contains(pos))
                    {
                        let row_idx =
                            ((pos.y - grid_rect.top()) / row_height).floor().max(0.) as usize;
                        if let Some(&target_layer_idx) = layer_indices.get(row_idx)
                            && self.can_swap_layer_symbol(target_layer_idx, &sym_id)
                        {
                            let highlight = Rect::from_min_max(
                                Pos2::new(
                                    rows.inner_rect.left(),
                                    grid_rect.top() + row_idx as f32 * row_height,
                                ),
                                Pos2::new(
                                    rows.inner_rect.right(),
                                    grid_rect.top() + (row_idx + 1) as f32 * row_height,
                                ),
                            )
                            .intersect(rows.inner_rect);
                            let hp = ui.painter_at(rows.inner_rect);
                            hp.rect_filled(highlight, 0., p.timeline_row_selected);
                            hp.rect_stroke(
                                highlight,
                                0.,
                                egui::Stroke::new(1.0_f32, p.accent_default),
                                egui::StrokeKind::Inside,
                            );
                            let layer_name =
                                self.symbol().timeline.layers[target_layer_idx].name.clone();
                            let ghost_painter = ctx.layer_painter(egui::LayerId::new(
                                egui::Order::Tooltip,
                                egui::Id::new("library-symbol-timeline-swap-ghost"),
                            ));
                            nf::drag_ghost_at(
                                &ghost_painter,
                                &ctx,
                                pos + Vec2::new(12., 12.),
                                &format!("Replace on all keyframes of “{layer_name}”"),
                            );
                            if !down {
                                self.library_panel.drag = None;
                                let previous_layer = self.layer;
                                self.layer = target_layer_idx;
                                if self.layer != previous_layer {
                                    self.clear_timeline_frame_selection();
                                }
                                self.swap_layer_or_selected_symbol(target_layer_idx, &sym_id);
                            }
                        }
                    }
                    if !down {
                        self.library_panel.drag = None;
                    }
                }
            }
            if a11y_active {
                self.emit_timeline_accesskit_tree(
                    &ctx,
                    ui.unique_id(),
                    a11y_layer_tree,
                    a11y_playhead,
                    &a11y_layer_rows,
                    a11y_grid,
                    &layer_indices,
                    &script_frames,
                );
            }
        });
        if let Some((layer, folder_id)) = commit_reparent_folder {
            self.reparent_timeline_layer_to_folder(&layer, Some(&folder_id));
        } else if let Some((layer, before, new_parent)) = commit_reorder {
            self.drop_timeline_layer_visual(
                &layer,
                before.as_deref(),
                new_parent.as_ref().map(|opt| opt.as_deref()),
            );
        }
        ctx.data_mut(|data| {
            if let Some(drag) = reorder {
                data.insert_temp(reorder_id, drag);
            } else {
                data.remove::<LayerReorder>(reorder_id);
            }
            if let Some(drag) = resize {
                data.insert_temp(resize_id, drag);
            } else {
                data.remove::<ColumnResize>(resize_id);
            }
        });
        if cancel_rename {
            rename = None;
        } else if let Some((layer_id, name)) = commit_rename {
            if !name.is_empty()
                && !name.chars().any(char::is_control)
                && let Some(index) = self
                    .symbol()
                    .timeline
                    .layers
                    .iter()
                    .position(|layer| layer.id == layer_id)
                && self.symbol().timeline.layers[index].name != name
            {
                self.finish_edits();
                self.transact(|s| s.symbol_mut().timeline.layers[index].name = name);
            }
            rename = None;
        }
        ctx.data_mut(|data| {
            if let Some(rename) = rename.filter(|draft| {
                self.symbol()
                    .timeline
                    .layers
                    .iter()
                    .any(|layer| layer.id == draft.layer)
            }) {
                data.insert_temp(rename_id, rename);
            } else {
                data.remove::<LayerRename>(rename_id);
            }
        });
        self.layer_outline_color_popover(&ctx);
    }

    /// Rows of the frame area, back to front per row: `TL Grid` (empty frames, 5th shaded; folders
    /// a flat strip) → `TL Span`s → `TL Grid Lines` → selected frames (`TL Span` Selected) →
    /// `TL Marker`s (Tone = On Selection inside the selection) → `TL Frame Label`s.
    #[allow(clippy::too_many_arguments)]
    fn paint_timeline_rows(
        &mut self,
        painter: &egui::Painter,
        rect: Rect,
        clip: Rect,
        cell: f32,
        layer_indices: &[usize],
        script_frames: &[u32],
        tinted: bool,
    ) -> TimelineVirtualStats {
        #[derive(Clone, Copy)]
        struct VisibleSpan {
            ki: usize,
            start: u32,
            next: u32,
            kind: Option<SpanType>,
        }

        let ctx = painter.ctx().clone();
        let p = neoflash_ui::palette(&ctx);
        let count = self.symbol().timeline.frame_count;
        let row_height = self.workspace.timeline.row_height();
        let geo = Geometry {
            origin_x: rect.left(),
            frame_width: cell,
            row_height,
        };
        let total_rows = layer_indices.len();
        let window = TimelineVirtualWindow::with_default_overscan(
            clip,
            rect.min,
            row_height,
            cell,
            total_rows,
            count.max(1),
        );
        let first = window.first_frame.min(count);
        let last = window.last_frame.min(count);
        let row_range = window.row_range(total_rows);
        let frame_range = window.frame_range(count.max(1));
        let drawn_frame_columns = frame_range.end.saturating_sub(frame_range.start);
        let end_x = rect.left() + count as f32 * cell;
        let button_states = self.symbol().kind == SymbolKind::Button;
        let grid_end_x = if button_states {
            end_x
        } else {
            rect.right().max(end_x)
        };
        let selection = self.frame_range();
        let preview = self.workspace.timeline.preview != crate::workspace::FramePreview::Off;
        if preview {
            let visible_layer_ids: Vec<String> = row_range
                .clone()
                .filter_map(|li| {
                    let &idx = layer_indices.get(li)?;
                    self.symbol().timeline.layers.get(idx).map(|l| l.id.clone())
                })
                .collect();
            self.begin_thumbnail_pass_for_window(first, last, &visible_layer_ids);
        }
        let any_solo = self.has_any_solo_layers();
        let mut drawn_grid_rows = 0usize;
        let mut drawn_spans = 0usize;
        for li in row_range {
            let Some(&layer_idx) = layer_indices.get(li) else {
                continue;
            };
            let y = rect.top() + li as f32 * row_height;
            let row =
                Rect::from_min_max(Pos2::new(rect.left(), y), Pos2::new(end_x, y + row_height));
            let grid_row = Rect::from_min_max(
                Pos2::new(rect.left(), y),
                Pos2::new(grid_end_x, y + row_height),
            );
            if !grid_row.intersects(clip) {
                continue;
            }
            drawn_grid_rows += 1;
            let (layer_id, layer_role, layer_end) = {
                let Some(layer) = self.symbol().timeline.layers.get(layer_idx) else {
                    continue;
                };
                (
                    layer.id.clone(),
                    layer.properties.role,
                    layer.properties.end_frame.unwrap_or(count).min(count),
                )
            };
            let rp = painter.with_clip_rect(clip.intersect(row));
            let grid_rp = painter.with_clip_rect(clip.intersect(grid_row));
            if layer_role == LayerRole::Folder {
                tl::paint_grid(&grid_rp, &ctx, grid_row, &geo, GridKind::Folder);
                if any_solo && !self.is_layer_id_effectively_soloed(&layer_id) {
                    grid_rp.rect_filled(grid_row, 0., p.bg_panel.gamma_multiply(0.55));
                }
                continue;
            }
            tl::paint_grid(&grid_rp, &ctx, grid_row, &geo, GridKind::Layer);
            // A motion tween is one span through its ◆ property keys (which have no span of
            // their own); classic and shape tweens without an end keyframe are broken.
            let spans: Vec<VisibleSpan> = {
                let keyframes = &self.symbol().timeline.layers[layer_idx].keyframes;
                let mut start_ki = keyframes
                    .partition_point(|k| k.frame <= first)
                    .saturating_sub(1);
                while start_ki > 0 && keyframes[start_ki].keyed_properties.is_some() {
                    start_ki -= 1;
                }
                let span_end = |ki: usize| {
                    let key = &keyframes[ki];
                    let motion = key.tween.is_some() && key.tween_kind == TweenKind::Motion;
                    keyframes[ki + 1..]
                        .iter()
                        .find(|k| !(motion && k.keyed_properties.is_some()))
                        .map(|k| k.frame)
                };
                let mut visible_spans = Vec::new();
                for (offset, key) in keyframes[start_ki..].iter().enumerate() {
                    let ki = start_ki + offset;
                    if key.frame > last.saturating_add(1) {
                        break;
                    }
                    if key.keyed_properties.is_some() {
                        if window.span_intersects(key.frame, key.frame) {
                            visible_spans.push(VisibleSpan {
                                ki,
                                start: key.frame,
                                next: key.frame,
                                kind: None,
                            });
                        }
                        continue;
                    }
                    let next_key = span_end(ki);
                    let next = next_key.unwrap_or(layer_end).min(count);
                    if next > key.frame && window.span_intersects(key.frame, next) {
                        let kind = if key.tween.is_some() {
                            Some(match key.tween_kind {
                                TweenKind::Motion => SpanType::MotionTween,
                                _ if !next_key.is_some_and(|f| f < count) => SpanType::BrokenTween,
                                TweenKind::Shape => SpanType::ShapeTween,
                                TweenKind::Classic => SpanType::ClassicTween,
                            })
                        } else if key.sound.is_some() {
                            Some(SpanType::Sound)
                        } else if !key.elements.is_empty() {
                            Some(SpanType::Static)
                        } else {
                            None
                        };
                        visible_spans.push(VisibleSpan {
                            ki,
                            start: key.frame,
                            next,
                            kind,
                        });
                    }
                }
                visible_spans
            };
            for &VisibleSpan {
                start, next, kind, ..
            } in &spans
            {
                if !window.span_intersects(start, next) {
                    continue;
                }
                drawn_spans += 1;
                if let Some(kind) = kind {
                    let span = geo.span(row, start + 1, next);
                    // Sound's Static background is identical; omit the gallery-only decoration.
                    tl::paint_span(
                        &rp,
                        &ctx,
                        span,
                        if kind == SpanType::Sound {
                            SpanType::Static
                        } else {
                            kind
                        },
                        tinted,
                    );
                } else if next > start {
                    let span = geo.span(row, start + 1, next);
                    let body =
                        Rect::from_min_max(span.min, Pos2::new(span.right(), span.bottom() - 1.0));
                    if body.width() >= 3.0 {
                        let edge = Stroke::new(1.0_f32, p.timeline_grid);
                        rp.vline(body.left() + 0.5, body.y_range(), edge);
                        rp.vline(body.right() - 0.5, body.y_range(), edge);
                    }
                }
            }
            // NF2 v2.2: frame ticks (`TL Grid Lines`) belong to `paint_grid`, below spans.
            let selected = selection
                .as_ref()
                .filter(|range| range.layers.contains(&layer_id))
                .map(|range| (range.start, range.end));
            if let Some((start, end)) = selected {
                tl::paint_span(
                    &grid_rp,
                    &ctx,
                    geo.span(grid_row, start + 1, end + 1),
                    SpanType::Selected,
                    tinted,
                );
            }
            // Frame view ▸ Preview: keyframe thumbnails under the markers (visible spans only).
            if preview {
                for &VisibleSpan {
                    ki,
                    start,
                    next,
                    kind,
                } in &spans
                {
                    if kind.is_none() || !window.span_intersects(start, next) {
                        continue;
                    }
                    if let Some(thumb) =
                        crate::timeline_preview::thumbnail_rect(&geo, row, start, next)
                    {
                        let key_element_ids: Vec<Id> = self.symbol().timeline.layers[layer_idx]
                            .keyframes[ki]
                            .elements
                            .iter()
                            .map(|el| el.id.clone())
                            .collect();
                        self.paint_span_thumbnail_by_id(
                            &rp,
                            &layer_id,
                            &key_element_ids,
                            start,
                            thumb,
                        );
                    }
                }
            }
            // Decode-free peak lookup after selection/preview fills, so selected sounds remain legible.
            for &VisibleSpan {
                ki,
                start,
                next,
                kind,
            } in &spans
            {
                if kind == Some(SpanType::Sound)
                    && window.span_intersects(start, next)
                    && let Some(sound) = self.symbol().timeline.layers[layer_idx].keyframes[ki]
                        .sound
                        .clone()
                {
                    self.timeline_audio.paint_waveform(
                        &self.project,
                        &sound,
                        &rp,
                        geo.span(row, start + 1, next),
                        f64::from(cell) * f64::from(self.project.fps),
                    );
                }
            }
            let on_selection =
                |frame: u32| selected.is_some_and(|(start, end)| start <= frame && frame <= end);
            for &VisibleSpan {
                ki,
                start,
                next,
                kind,
            } in &spans
            {
                if !window.span_intersects(start, next) {
                    continue;
                }
                let action = script_frames.binary_search(&start).is_ok()
                    && self
                        .symbol()
                        .timeline
                        .key_carries_frame_script(layer_idx, start);
                let key = &self.symbol().timeline.layers[layer_idx].keyframes[ki];
                let marker = match (key.elements.is_empty(), action) {
                    _ if key.keyed_properties.is_some() => MarkerKind::PropertyKeyframe,
                    (true, true) => MarkerKind::ActionBlankKeyframe,
                    (true, false) => MarkerKind::BlankKeyframe,
                    (false, true) => MarkerKind::ActionKeyframe,
                    (false, false) => MarkerKind::Keyframe,
                };
                tl::paint_marker(
                    &rp,
                    &ctx,
                    geo.cell(row, start + 1),
                    marker,
                    on_selection(start),
                );
                let tween = matches!(
                    kind,
                    Some(
                        SpanType::ClassicTween
                            | SpanType::ShapeTween
                            | SpanType::MotionTween
                            | SpanType::BrokenTween
                    )
                ) || key.keyed_properties.is_some();
                if !tween && next > start + 1 {
                    tl::paint_marker(
                        &rp,
                        &ctx,
                        geo.cell(row, next),
                        MarkerKind::SpanEnd,
                        on_selection(next - 1),
                    );
                }
                if !key.label.is_empty() {
                    let (kind, text) = match key.label_type {
                        FrameLabelType::Anchor => (FrameLabelKind::Anchor, key.label.as_str()),
                        FrameLabelType::Comment => (
                            FrameLabelKind::Comment,
                            key.label
                                .strip_prefix("//")
                                .map_or(key.label.as_str(), str::trim_start),
                        ),
                        FrameLabelType::Name => match key.label.strip_prefix("//") {
                            Some(comment) => (FrameLabelKind::Comment, comment.trim_start()),
                            None => (FrameLabelKind::Label, key.label.as_str()),
                        },
                    };
                    tl::paint_frame_label(
                        &rp,
                        &ctx,
                        geo.cell(row, start + 1),
                        kind,
                        text,
                        geo.frame_x(next + 1),
                    );
                }
            }
            // R-045: while any layer is soloed, frame cells of non-soloed rows are drawn at 45 %.
            if any_solo && !self.is_layer_id_effectively_soloed(&layer_id) {
                grid_rp.rect_filled(grid_row, 0., p.bg_panel.gamma_multiply(0.55));
            }
        }
        TimelineVirtualStats {
            window,
            total_rows,
            total_frames: count,
            drawn_layer_rows: 0,
            drawn_grid_rows,
            drawn_frame_columns,
            drawn_spans,
            layer_list_height: 0.0,
            scroll_offset_y: 0.0,
        }
    }

    /// Frame (right-click) menu, design §8.
    pub(crate) fn frame_context_menu(&mut self, ui: &mut egui::Ui) {
        use crate::commands::CommandId as C;
        use crate::timeline_edit::FrameCommand as F;
        use crate::ui_icons::{menu_item, menu_item_enabled};
        let editable = self.layer_editable();
        let tweenable = self.can_create_tween();
        for (label, cmd_id, kind) in [
            (
                "Create motion tween",
                C::InsertMotionTween,
                TweenKind::Motion,
            ),
            ("Create shape tween", C::InsertShapeTween, TweenKind::Shape),
            (
                "Create classic tween",
                C::InsertClassicTween,
                TweenKind::Classic,
            ),
        ] {
            if menu_item_enabled(
                ui,
                tweenable && self.current_tween() != Some(kind),
                label,
                &self.sc(cmd_id),
            )
            .clicked()
            {
                self.create_tween(kind);
                ui.close();
            }
        }
        nf::menu_separator(ui);
        if menu_item_enabled(ui, editable, "Insert frame", &self.sc(C::InsertFrame)).clicked() {
            self.insert_frame();
            ui.close();
        }
        let remove_sc = self.sc(C::EditTimelineRemoveFrames);
        self.frame_command_item(ui, "Remove frames", &remove_sc, F::Remove);
        if menu_item_enabled(ui, editable, "Insert keyframe", &self.sc(C::InsertKeyframe)).clicked()
        {
            self.finish_edits();
            self.make_key(false);
            ui.close();
        }
        if menu_item_enabled(
            ui,
            editable,
            "Insert blank keyframe",
            &self.sc(C::InsertBlankKeyframe),
        )
        .clicked()
        {
            self.finish_edits();
            self.make_key(true);
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
            editable,
            "Convert to blank keyframes",
            &self.sc(C::ModifyTimelineConvertToBlankKeyframes),
        )
        .clicked()
        {
            self.convert_to_keyframes(true);
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
        nf::submenu(ui, "Utils", |ui| {
            crate::ui_icons::menu_contents(ui, wide_menu_width(), |ui| {
                let can_step_keys = editable && self.can_convert_to_keyframes_stepped();
                if menu_item_enabled(ui, can_step_keys, "Tween2Keys (On 2s)", "").clicked() {
                    self.tween_to_keys_on_twos();
                    ui.close();
                }
                if menu_item_enabled(ui, can_step_keys, "Tween2Keys (On 3s)", "").clicked() {
                    self.tween_to_keys_on_threes();
                    ui.close();
                }
                if menu_item_enabled(
                    ui,
                    can_step_keys,
                    "Convert to keyframes (on 2s, keep ease)",
                    "",
                )
                .clicked()
                {
                    self.convert_to_keyframes_stepped(2, false);
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
            });
        });
        nf::menu_separator(ui);
        for (label, cmd) in [
            ("Cut frames", F::Cut),
            ("Copy frames", F::Copy),
            ("Paste frames", F::Paste),
            ("Clear frames", F::Clear),
            ("Select all frames", F::SelectAll),
        ] {
            let sc = self.sc(C::for_frame_command(cmd));
            self.frame_command_item(ui, label, &sc, cmd);
        }
        nf::menu_separator(ui);
        if menu_item_enabled(
            ui,
            self.can_copy_motion(),
            "Copy motion",
            &self.sc(C::EditTimelineCopyMotion),
        )
        .clicked()
        {
            self.copy_motion();
            ui.close();
        }
        if menu_item_enabled(
            ui,
            self.can_paste_motion(),
            "Paste motion",
            &self.sc(C::EditTimelinePasteMotion),
        )
        .clicked()
        {
            self.paste_motion();
            ui.close();
        }
        if menu_item_enabled(ui, self.can_paste_motion(), "Paste motion special…", "").clicked() {
            self.open_paste_motion_special();
            ui.close();
        }
        nf::menu_separator(ui);
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
        nf::menu_separator(ui);
        if menu_item(ui, "Actions", &self.sc(C::WindowActions)).clicked() {
            self.open_frame_actions();
            ui.close();
        }
        if menu_item_enabled(ui, self.tween_span().is_some(), "Ease…", "").clicked() {
            self.show_panel(crate::workspace::PanelId::Ease);
            ui.close();
        }
        if menu_item_enabled(ui, self.can_copy_active_keyframe_ease(), "Copy ease", "").clicked() {
            self.copy_active_keyframe_ease();
            ui.close();
        }
        if menu_item_enabled(
            ui,
            editable && self.has_copied_keyframe_ease(),
            "Apply ease",
            "",
        )
        .clicked()
        {
            self.apply_copied_ease_to_selection();
            ui.close();
        }
    }

    /// One frame-range command row (Edit ▸ Timeline and the frame menu).
    pub(crate) fn frame_command_item(
        &mut self,
        ui: &mut egui::Ui,
        label: &str,
        shortcut: &str,
        command: crate::timeline_edit::FrameCommand,
    ) {
        let enabled = self.frame_command_enabled(command);
        let resp = crate::ui_icons::menu_item_enabled(ui, enabled, label, shortcut);
        if crate::ui_icons::hover_tooltip(
            resp,
            label,
            (!shortcut.is_empty()).then_some(shortcut),
            Some("Select frames in the Timeline first"),
        )
        .clicked()
        {
            self.run_frame_command(ui.ctx(), command);
            ui.close();
        }
    }

    pub fn onion_range(&self) -> (u32, u32) {
        let last = self.symbol().timeline.frame_count.saturating_sub(1);
        let current = self.frame.min(last);
        if self.onion_anchor {
            let start = self.anchored_range.0.min(last);
            let end = self.anchored_range.1.min(last).max(start);
            (start, end)
        } else {
            (
                current.saturating_sub(self.onion_before),
                (current + self.onion_after).min(last),
            )
        }
    }

    /// Paint the 2 inline 16×16 (`12×12` on compact rows) NF2 mask icons (`R-049`: Mask Channel
    /// Geometry / Alpha / Luminance, and Mask Invert) inside the `Mode` cell on a `LayerRole::Mask`
    /// row.
    #[allow(clippy::too_many_arguments)]
    fn paint_mask_row_mode_and_icons(
        &mut self,
        ui: &mut egui::Ui,
        row_id: egui::Id,
        mode_rect: Rect,
        row_rect: Rect,
        idx: usize,
        mask_mode: MaskMode,
        mask_invert: bool,
        row_selected: bool,
    ) {
        let p = neoflash_ui::palette(ui.ctx());
        let cell_rect = Rect::from_min_max(
            Pos2::new(mode_rect.left(), mode_rect.top().max(row_rect.top())),
            Pos2::new(mode_rect.right(), mode_rect.bottom().min(row_rect.bottom())),
        );
        let half_s1 = space::s1() * 0.5;
        let icon_side = tl::layer_row_icon(row_rect.height());
        let icon_w = icon_side;
        let icon_h = icon_side.min(row_rect.height());
        let icon_y = cell_rect.center().y - icon_h * 0.5;
        let invert_icon_rect = Rect::from_min_size(
            Pos2::new(cell_rect.right() - icon_w, icon_y),
            Vec2::new(icon_w, icon_h),
        );
        let channel_icon_rect = Rect::from_min_size(
            Pos2::new(invert_icon_rect.left() - half_s1 - icon_w, icon_y),
            Vec2::new(icon_w, icon_h),
        );

        let channel_resp = ui.interact(
            channel_icon_rect,
            row_id.with("mask-channel-icon"),
            Sense::click(),
        );
        channel_resp.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                format!("Mask channel: {}", mask_mode.label()),
            )
        });
        let invert_resp = ui.interact(
            invert_icon_rect,
            row_id.with("mask-invert-icon"),
            Sense::click(),
        );
        invert_resp.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Invert mask")
        });

        let mode_active = !mask_mode.is_geometry();
        let channel_fg = if mode_active {
            p.accent_default
        } else if channel_resp.hovered() || row_selected {
            p.text_primary
        } else {
            p.text_secondary
        };
        if mode_active || channel_resp.hovered() {
            ui.painter().rect_filled(
                channel_icon_rect,
                neoflash_ui::tokens::radius::sm(),
                if mode_active {
                    p.accent_subtle
                } else {
                    p.bg_hover
                },
            );
        }
        let channel_icon = match mask_mode {
            MaskMode::Geometry => neoflash_ui::icons::i16::MASK_GEOMETRY,
            MaskMode::Alpha => neoflash_ui::icons::i16::MASK_ALPHA,
            MaskMode::Luminance => neoflash_ui::icons::i16::MASK_LUMA,
        };
        neoflash_ui::icons::paint_sized(
            ui.painter(),
            ui.ctx(),
            channel_icon_rect,
            channel_icon,
            icon_side,
            channel_fg,
        );

        let invert_fg = if mask_invert {
            p.accent_default
        } else if invert_resp.hovered() || row_selected {
            p.text_primary
        } else {
            p.text_secondary
        };
        if mask_invert || invert_resp.hovered() {
            ui.painter().rect_filled(
                invert_icon_rect,
                neoflash_ui::tokens::radius::sm(),
                if mask_invert {
                    p.accent_subtle
                } else {
                    p.bg_hover
                },
            );
        }
        neoflash_ui::icons::paint_sized(
            ui.painter(),
            ui.ctx(),
            invert_icon_rect,
            neoflash_ui::icons::i16::MASK_INVERT,
            icon_side,
            invert_fg,
        );

        if channel_resp.clicked() {
            let next_mode = match mask_mode {
                MaskMode::Geometry => MaskMode::Alpha,
                MaskMode::Alpha => MaskMode::Luminance,
                MaskMode::Luminance => MaskMode::Geometry,
            };
            self.set_layer_mask_mode(idx, next_mode);
        }
        if invert_resp.clicked() {
            self.set_layer_mask_invert(idx, !mask_invert);
        }

        let open_popup = channel_resp.secondary_clicked();
        let mode_popup_id = row_id.with("mask-mode-popup");
        egui::Popup::new(
            mode_popup_id,
            ui.ctx().clone(),
            egui::PopupAnchor::ParentRect(mode_rect),
            ui.layer_id(),
        )
        .kind(egui::PopupKind::Menu)
        .open_memory(open_popup.then_some(egui::SetOpenCommand::Toggle))
        .gap(space::s1())
        .width(neoflash_ui::metrics::menu_width())
        .frame(nf::popover_frame(ui.ctx()))
        .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
        .show(|ui| {
            nf::menu::menu_style(ui.style_mut());
            ui.set_min_width(neoflash_ui::metrics::menu_width());
            crate::layer_controls::populate_mask_mode_menu(ui, mask_mode, |picked| {
                self.set_layer_mask_mode(idx, picked);
            });
        });

        let _ = nf::tooltip(
            channel_resp,
            &format!(
                "Mask mode: {} — click to cycle Geometry / Alpha / Luminance (right-click for menu)",
                mask_mode.label()
            ),
            None,
        );
        let _ = nf::tooltip(
            invert_resp,
            if mask_invert {
                "Invert mask: On (click to use normal mask)"
            } else {
                "Invert mask: Off (click to invert mask coverage)"
            },
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t410_thousand_layer_virtualization_scroll_jump_and_offscreen_reorder() {
        let ctx = egui::Context::default();
        let mut project = Project::blank();
        project.symbols[0].timeline.frame_count = 120;
        project.symbols[0].timeline.layers = (0..1_000)
            .map(|i| Layer {
                id: format!("layer_{i}"),
                name: format!("Layer {i}"),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: Vec::new(),
                    tween: None,
                    label: String::new(),
                    ..Default::default()
                }],
                properties: LayerProperties::default(),
            })
            .collect();
        let mut editor = Editor::from_project(&ctx, project);
        editor.project.validate().unwrap();
        let row_h = editor.workspace.timeline.row_height();

        let render = |ed: &mut Editor, events: Vec<egui::Event>, down: bool| -> egui::FullOutput {
            let mut raw = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1_280.0, 320.0))),
                events,
                ..Default::default()
            };
            raw.focused = true;
            if down {
                raw.modifiers = egui::Modifiers::NONE;
            }
            ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ed.timeline_contents(ui);
                });
            })
        };

        // 1. Initial 1,000-layer render pass: exact total height & O(visible) row count.
        let _ = render(&mut editor, vec![], false);
        let stats = editor
            .last_timeline_virtual_stats(&ctx)
            .expect("1,000-layer stats recorded");
        assert_eq!(stats.total_rows, 1_000);
        assert!(
            (stats.layer_list_height - 1_000.0 * row_h).abs() < 1e-2,
            "expected exact height {}, got {}",
            1_000.0 * row_h,
            stats.layer_list_height
        );
        assert!(
            stats.drawn_layer_rows > 0 && stats.drawn_layer_rows <= 28,
            "drawn_layer_rows={}",
            stats.drawn_layer_rows
        );
        assert!(
            stats.drawn_grid_rows > 0 && stats.drawn_grid_rows <= 28,
            "drawn_grid_rows={}",
            stats.drawn_grid_rows
        );

        // 2. Selecting serialized layer 500 (visual row 499) then serialized layer 0 (visual row 999)
        //    automatically jumps viewport scroll to reveal each newly selected layer.
        editor.select_layer_row(500, false, false);
        let _ = render(&mut editor, vec![], false);
        let _ = render(&mut editor, vec![], false);
        let stats_mid = editor.last_timeline_virtual_stats(&ctx).unwrap();
        assert!(
            stats_mid.window.first_row <= 499 && 499 <= stats_mid.window.last_row,
            "selected layer at visual row 499 must scroll into view: {:?}",
            stats_mid.window
        );

        editor.select_layer_row(0, false, false);
        let _ = render(&mut editor, vec![], false);
        let _ = render(&mut editor, vec![], false);
        let stats_jumped = editor.last_timeline_virtual_stats(&ctx).unwrap();
        assert!(
            stats_jumped.window.last_row >= 999,
            "selected layer at visual row 999 must scroll into view: {:?}",
            stats_jumped.window
        );
        assert!((stats_jumped.layer_list_height - 1_000.0 * row_h).abs() < 1e-2);

        // 3. Keyboard navigation (`Home`, `ArrowDown`, `Shift+ArrowDown`) jumps back to top & extends selection.
        ctx.data_mut(|d| d.insert_temp(egui::Id::new(TIMELINE_LAYER_FOCUS_ID), true));
        let _ = render(
            &mut editor,
            vec![egui::Event::Key {
                key: egui::Key::Home,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            false,
        );
        let _ = render(&mut editor, vec![], false);
        assert_eq!(editor.layer, 999, "Home selects top visual row (layer_999)");
        assert_eq!(
            editor
                .last_timeline_virtual_stats(&ctx)
                .unwrap()
                .window
                .first_row,
            0
        );

        let _ = render(
            &mut editor,
            vec![egui::Event::Key {
                key: egui::Key::ArrowDown,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::SHIFT,
            }],
            false,
        );
        let _ = render(&mut editor, vec![], false);
        assert!(editor.is_layer_row_selected(999));
        assert!(editor.is_layer_row_selected(998));

        // 4. Drag top visual row (`layer_999`) while scrolled to bottom: `dragged_layer` stays rendered
        //    with exact `1000 * row_h` height and commits reorder cleanly on release!
        editor.select_layer_row(999, false, false);
        let out_top = render(&mut editor, vec![], false);
        let row0_pos = out_top
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.job.text == "Layer 999" => {
                    Some(egui::Rect::from_min_size(t.pos, t.galley.size()).center())
                }
                _ => None,
            })
            .expect("top row `Layer 999` rendered");

        // Press and drag >= 6 pt on `Layer 999`.
        let drag_mid = row0_pos + Vec2::new(0.0, 12.0);
        let _ = render(
            &mut editor,
            vec![
                egui::Event::PointerMoved(row0_pos),
                egui::Event::PointerButton {
                    pos: row0_pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            true,
        );
        let _ = render(&mut editor, vec![egui::Event::PointerMoved(drag_mid)], true);

        // Scroll 24,000 pt down via MouseWheel while holding drag so row 0 is ~980 rows offscreen!
        let _ = render(
            &mut editor,
            vec![
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: Vec2::new(0.0, -24_000.0),
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerMoved(Pos2::new(row0_pos.x, 180.0)),
            ],
            true,
        );
        let out_scrolled_drag = render(
            &mut editor,
            vec![egui::Event::PointerMoved(Pos2::new(row0_pos.x, 180.0))],
            true,
        );
        let drag_stats = editor.last_timeline_virtual_stats(&ctx).unwrap();
        assert!(
            drag_stats.window.first_row > 500,
            "scrolled deep into 1,000-layer list: {:?}",
            drag_stats.window
        );
        let visible_window_len = drag_stats.window.row_range(1_000).len();
        assert_eq!(
            drag_stats.drawn_layer_rows,
            visible_window_len + 1,
            "pinned offscreen dragged_layer must remain rendered alongside visible window"
        );
        assert!(
            (drag_stats.layer_list_height - 1_000.0 * row_h).abs() < 1e-2,
            "multi-gap spacer must preserve exact 1,000 * row_h height"
        );
        let _ = out_scrolled_drag;

        // Release inside the layer column near the bottom to reorder `layer_999`!
        let drop_pos = Pos2::new(row0_pos.x, 260.0);
        let _ = render(
            &mut editor,
            vec![
                egui::Event::PointerMoved(drop_pos),
                egui::Event::PointerButton {
                    pos: drop_pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            false,
        );
        assert!(
            editor.history.can_undo(),
            "releasing offscreen-origin layer reorder must commit in 1 Undo step"
        );
        let new_pos = editor
            .symbol()
            .timeline
            .layers
            .iter()
            .position(|l| l.id == "layer_999")
            .unwrap();
        assert!(
            new_pos < 500,
            "layer_999 moved from top (999) into bottom half: {new_pos}"
        );
        editor.project.validate().unwrap();
        editor.undo();
        assert_eq!(editor.symbol().timeline.layers[999].id, "layer_999");
        editor.project.validate().unwrap();
    }

    #[test]
    fn t414_folder_collapse_selects_parent_folder_reparent_is_contiguous_and_multi_layer_make_key()
    {
        let ctx = egui::Context::default();
        let mut project = Project::blank();
        project.format_version = 6;
        let mut layers = Vec::with_capacity(1_002);
        for i in 0..999 {
            layers.push(Layer {
                id: format!("bg_{i}"),
                name: format!("BG {i}"),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe::default()],
                properties: LayerProperties::default(),
            });
        }
        layers.push(Layer {
            id: "child_top".into(),
            name: "Child Top".into(),
            visible: true,
            locked: false,
            keyframes: vec![Keyframe::default()],
            properties: LayerProperties {
                parent: Some("folder_top".into()),
                ..LayerProperties::default()
            },
        });
        layers.push(Layer {
            id: "folder_top".into(),
            name: "Folder Top".into(),
            visible: true,
            locked: false,
            keyframes: Vec::new(),
            properties: LayerProperties {
                role: LayerRole::Folder,
                collapsed: false,
                ..LayerProperties::default()
            },
        });
        project.symbols[0].timeline.layers = layers;
        project.symbols[0].timeline.frame_count = 10;
        project.validate().unwrap();

        let mut editor = Editor::from_project(&ctx, project);
        // Active layer is `child_top` (index 999) inside `folder_top` (index 1000).
        editor.layer = 999;
        editor.toggle_layer_folder(1000);
        assert_eq!(
            editor.layer, 1000,
            "collapsing a Folder containing active self.layer must select the collapsed Folder instead of jumping to index 0"
        );

        // Reparenting `bg_0` (at bottom index 0) into `folder_top` moves `bg_0` contiguous with `folder_top`
        // so visual row order and Stage painter z-order agree!
        editor.reparent_timeline_layer_to_folder("bg_0", Some("folder_top"));
        let folder_pos = editor
            .symbol()
            .timeline
            .layers
            .iter()
            .position(|l| l.id == "folder_top")
            .unwrap();
        let bg0_pos = editor
            .symbol()
            .timeline
            .layers
            .iter()
            .position(|l| l.id == "bg_0")
            .unwrap();
        assert!(
            bg0_pos + 2 >= folder_pos,
            "reparented layer must be moved contiguous with folder_top: bg0={bg0_pos}, folder={folder_pos}"
        );
        editor.project.validate().unwrap();

        // Multi-layer `make_key` (F6 / F7) keys all editable layers in `frame_range()` in 1 Undo step.
        let undo_before = editor.history.undo_len();
        editor.frame = 4;
        editor.select_frame_range(FrameRange {
            layers: vec!["bg_1".into(), "bg_2".into(), "folder_top".into()],
            start: 4,
            end: 4,
        });
        editor.make_key(false);
        assert_eq!(editor.history.undo_len(), undo_before + 1);
        for lid in ["bg_1", "bg_2"] {
            let l = editor
                .symbol()
                .timeline
                .layers
                .iter()
                .find(|l| l.id == lid)
                .unwrap();
            assert!(
                l.keyframes.iter().any(|k| k.frame == 4),
                "layer {lid} must receive keyframe at frame 4"
            );
        }
        editor.project.validate().unwrap();
    }
}
