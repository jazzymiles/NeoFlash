//! Native panels share one Editor. Only visibility and geometry are preferences.
use crate::{
    app::Editor,
    clipboard::{ClipboardCommand, ClipboardTarget},
    dock_grid::{DockGroupTarget, DockViewportTarget, edge_targets, surface_viewport},
    native_screens::available_work_areas,
    workspace::{
        DockDestination, DockEdge, DockSurface, PanelId, PanelPlacement, ScreenRect,
        ToolsStageAnchor, WindowGeometry, minimum_tools_window_width, recover_geometry_with_bounds,
    },
};
use eframe::egui;
use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

#[derive(Default)]
pub(crate) struct DetachedState {
    active: HashSet<PanelId>,
    pub focus: Option<PanelId>,
    pub focus_docked_actions: bool,
    monitors: Vec<ScreenRect>,
    checked_monitors: Option<Instant>,
    drag: Option<PanelDrag>,
    drag_preview: Option<PanelDragPreview>,
    pub(crate) resize: Option<FloatingResize>,
    pub(crate) root_inner: Option<egui::Rect>,
    pub(crate) dock_groups: Vec<DockGroupTarget>,
    pub(crate) content_header: Option<PanelId>,
    pub(crate) dock_viewports: Vec<DockViewportTarget>,
    pub(crate) activation: u64,
    context_menu_open: bool,
    pub(crate) tools_desired_inner_height: Option<f32>,
}

/// Interactive edge/corner resize state for borderless floating panel windows.
#[derive(Clone, Copy)]
pub(crate) struct FloatingResize {
    pub(crate) window_id: PanelId,
    pub(crate) left: bool,
    pub(crate) right: bool,
    pub(crate) top: bool,
    pub(crate) bottom: bool,
    pub(crate) start_pointer: egui::Pos2,
    pub(crate) start_geometry: WindowGeometry,
}

/// A header gesture keeps its source viewport until release. This avoids
/// transferring a pressed mouse button between native windows mid-drag.
#[derive(Clone, Copy)]
struct PanelDrag {
    panel: PanelId,
    group: Option<(DockSurface, DockEdge, u32)>,
    whole_window: Option<PanelId>,
    source: egui::ViewportId,
    started: egui::Pos2,
    pointer: egui::Pos2,
    grab_offset: egui::Vec2,
    window_grab_offset: egui::Vec2,
    moved: bool,
    released: bool,
    began: Instant,
}

/// Read-only copies of the source panel's current paint output. Replaying
/// shapes keeps its widgets, focus and document callbacks in one viewport.
struct PanelDragPreview {
    panel: PanelId,
    source: egui::ViewportId,
    grab_offset: egui::Vec2,
    zoom: f32,
    size: egui::Vec2,
    shapes: Vec<egui::epaint::ClippedShape>,
}

fn viewport(panel: PanelId) -> egui::ViewportId {
    egui::ViewportId::from_hash_of(panel.viewport_key().expect("detachable panel"))
}
fn title(panel: PanelId) -> &'static str {
    panel.title()
}

fn floating_collapsed_id(window: PanelId) -> egui::Id {
    egui::Id::new(("native-floating-collapsed", window))
}

const TOOLS_ANCHOR_MARGIN: f32 = 16.;
const TOOLS_ANCHOR_DISTANCE: f32 = 112.;
pub(crate) const TOOLS_GRIP: f32 = neoflash_ui::metrics::PANEL_TAB;

/// Width required by the locked floating-header grip, menu, collapse and close controls.
/// Expressed only in approved NF2 metrics.
fn floating_header_chrome_width() -> f32 {
    neoflash_ui::tokens::space::S2
        + neoflash_ui::icons::i16::GRIP.side()
        + neoflash_ui::tokens::space::S1
        + neoflash_ui::tokens::space::S1
        + 3.0 * neoflash_ui::metrics::MIN_HIT
        + neoflash_ui::tokens::space::S1
}

/// Minimum width for a floating header of `panels`.
/// A standalone `Tools` window switches to the compact `TOOLS_GRIP` + More (`⋯`)
/// header at narrow widths so 1-column and 2-column `Tools` rails are never
/// forced to the 5-column full-chrome width.
pub(crate) fn floating_header_chrome_width_for(panels: &[PanelId]) -> f32 {
    if panels == [PanelId::Tools] {
        minimum_tools_window_width()
    } else {
        floating_header_chrome_width()
    }
}

/// Width at which a standalone floating `Tools` window has room for the full
/// titled `FloatingPanelHeader` (grip + `"Tools"` tab + menu/collapse/close).
pub(crate) fn floating_full_tools_header_width() -> f32 {
    floating_header_chrome_width()
        + neoflash_ui::metrics::CONTROL
        + 2.0 * neoflash_ui::tokens::space::S3
}

/// Minimum logical width of a standalone floating `Tools` window: one tool
/// button plus existing NF2 padding and border (`42 pt` in Comfortable,
/// `40 pt` in Compact/Classic).
pub(crate) fn floating_tools_width() -> f32 {
    minimum_tools_window_width()
}

pub(crate) fn scaled_size(size: [f32; 2], zoom: f32) -> [f32; 2] {
    [size[0] * zoom, size[1] * zoom]
}

fn anchor_position(
    anchor: ToolsStageAnchor,
    stage: egui::Rect,
    size: [f32; 2],
    margin: f32,
) -> egui::Pos2 {
    let x = match anchor {
        ToolsStageAnchor::TopLeft | ToolsStageAnchor::MiddleLeft | ToolsStageAnchor::BottomLeft => {
            stage.left() + margin
        }
        ToolsStageAnchor::TopCenter | ToolsStageAnchor::BottomCenter => {
            stage.center().x - size[0] * 0.5
        }
        ToolsStageAnchor::TopRight
        | ToolsStageAnchor::MiddleRight
        | ToolsStageAnchor::BottomRight => stage.right() - margin - size[0],
    };
    let y = match anchor {
        ToolsStageAnchor::TopLeft | ToolsStageAnchor::TopCenter | ToolsStageAnchor::TopRight => {
            stage.top() + margin
        }
        ToolsStageAnchor::MiddleLeft | ToolsStageAnchor::MiddleRight => {
            stage.center().y - size[1] * 0.5
        }
        ToolsStageAnchor::BottomLeft
        | ToolsStageAnchor::BottomCenter
        | ToolsStageAnchor::BottomRight => stage.bottom() - margin - size[1],
    };
    egui::pos2(x, y)
}

pub(crate) fn apply_tools_stage_anchor(
    anchor: ToolsStageAnchor,
    stage: egui::Rect,
    mut geometry: WindowGeometry,
    zoom: f32,
) -> WindowGeometry {
    let margin = TOOLS_ANCHOR_MARGIN * zoom;
    // Anchors describe the Tools layout, not only its position. Reapply the logical
    // width whenever UI scale changes so the horizontal bar does not silently shrink
    // into a rail after moving between scale factors.
    geometry.inner_size[0] = if anchor.vertical() {
        floating_tools_width() * zoom
    } else {
        (2.0 * neoflash_ui::metrics::tools_two_columns()).max(floating_tools_width()) * zoom
    };
    geometry.outer_position = anchor_position(anchor, stage, geometry.inner_size, margin).into();
    geometry
}

pub(crate) fn snap_tools_stage_anchor(
    stage: egui::Rect,
    mut geometry: WindowGeometry,
    zoom: f32,
) -> Option<(ToolsStageAnchor, WindowGeometry)> {
    if !stage.is_finite() || !stage.is_positive() {
        return None;
    }
    let rect =
        egui::Rect::from_min_size(geometry.outer_position.into(), geometry.inner_size.into());
    let margin = TOOLS_ANCHOR_MARGIN * zoom;
    let distance = |anchor: ToolsStageAnchor| {
        let x = match anchor {
            ToolsStageAnchor::TopLeft
            | ToolsStageAnchor::MiddleLeft
            | ToolsStageAnchor::BottomLeft => (rect.left() - (stage.left() + margin)).abs(),
            ToolsStageAnchor::TopCenter | ToolsStageAnchor::BottomCenter => {
                (rect.center().x - stage.center().x).abs()
            }
            ToolsStageAnchor::TopRight
            | ToolsStageAnchor::MiddleRight
            | ToolsStageAnchor::BottomRight => (rect.right() - (stage.right() - margin)).abs(),
        };
        let y = match anchor {
            ToolsStageAnchor::TopLeft
            | ToolsStageAnchor::TopCenter
            | ToolsStageAnchor::TopRight => (rect.top() - (stage.top() + margin)).abs(),
            ToolsStageAnchor::MiddleLeft | ToolsStageAnchor::MiddleRight => {
                (rect.center().y - stage.center().y).abs()
            }
            ToolsStageAnchor::BottomLeft
            | ToolsStageAnchor::BottomCenter
            | ToolsStageAnchor::BottomRight => (rect.bottom() - (stage.bottom() - margin)).abs(),
        };
        egui::vec2(x, y).length()
    };
    let anchor = ToolsStageAnchor::ALL
        .into_iter()
        .map(|anchor| (anchor, distance(anchor)))
        .min_by(|a, b| a.1.total_cmp(&b.1))?;
    if anchor.1 > TOOLS_ANCHOR_DISTANCE * zoom {
        return None;
    }
    geometry = apply_tools_stage_anchor(anchor.0, stage, geometry, zoom);
    Some((anchor.0, geometry))
}

impl Editor {
    fn tools_anchor_stage_rect(&self, zoom: f32) -> Option<egui::Rect> {
        let root = self.detached.root_inner?;
        let canvas = self.stage_view.canvas;
        (canvas.is_finite() && canvas.is_positive())
            .then(|| (canvas * zoom).translate(root.min.to_vec2()))
    }

    /// Header titles and bottom tabs use the same bidirectional docking gesture.
    /// Drawn as the NF2 `Floating Panel Header` grip + active `Panel Tab` (36 px) when a panel body is
    /// shown outside a dock group header (the dock group draws its own tabs otherwise).
    pub(crate) fn panel_title(&mut self, ui: &mut egui::Ui, panel: PanelId, label: &str) {
        if self.detached.content_header == Some(panel) {
            return;
        }
        let response = panel_title_tab(ui, label);
        self.panel_drag_handle(ui, panel, &response);
    }

    pub(crate) fn panel_drag_handle(
        &mut self,
        ui: &egui::Ui,
        panel: PanelId,
        response: &egui::Response,
    ) {
        self.panel_drag_handle_inner(ui, panel, None, None, response);
    }

    pub(crate) fn panel_group_drag_handle(
        &mut self,
        ui: &egui::Ui,
        panel: PanelId,
        group: (DockSurface, DockEdge, u32),
        response: &egui::Response,
    ) {
        self.panel_drag_handle_inner(ui, panel, Some(group), None, response);
    }

    fn floating_window_drag_handle(
        &mut self,
        ui: &egui::Ui,
        window: PanelId,
        panel: PanelId,
        response: &egui::Response,
    ) {
        self.panel_drag_handle_inner(ui, panel, None, Some(window), response);
    }

    fn panel_drag_handle_inner(
        &mut self,
        ui: &egui::Ui,
        panel: PanelId,
        group: Option<(DockSurface, DockEdge, u32)>,
        whole_window: Option<PanelId>,
        response: &egui::Response,
    ) {
        if !ui.is_enabled() {
            return;
        }
        let ctx = ui.ctx();
        let source = surface_viewport(self.workspace.panel_surface(panel));
        let zoom = ctx.zoom_factor();
        let inner = ctx
            .input(|i| i.viewport().inner_rect)
            .unwrap_or_else(|| ctx.viewport_rect());
        if source == egui::ViewportId::ROOT {
            let current = inner * zoom;
            if let Some(view) = self
                .detached
                .dock_viewports
                .iter_mut()
                .find(|v| v.surface == DockSurface::Main)
            {
                let delta = current.min - view.inner.min;
                view.inner = current;
                view.content = view.content.translate(delta);
                for target in self
                    .detached
                    .dock_groups
                    .iter_mut()
                    .filter(|g| g.surface == DockSurface::Main)
                {
                    target.rect = target.rect.translate(delta);
                    target.header = target.header.translate(delta);
                    for (_, tab) in &mut target.tabs {
                        *tab = tab.translate(delta);
                    }
                }
            }
            self.detached.root_inner = Some(current);
        }
        let press = ctx.input(|i| {
            i.events.iter().find_map(|event| match event {
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    ..
                } => Some(*pos),
                _ => None,
            })
        });
        if self.detached.drag.is_none()
            && self.detached.resize.is_none()
            && let Some(pos) = press.filter(|pos| {
                response.rect.contains(*pos)
                    && ui.clip_rect().contains(*pos)
                    && ctx.layer_id_at(*pos) == Some(ui.layer_id())
                    && !Self::in_floating_resize_margin(
                        ctx,
                        source == egui::ViewportId::ROOT,
                        panel,
                        *pos,
                    )
            })
        {
            let global = (inner.min + pos.to_vec2()) * zoom;
            self.detached.drag = Some(PanelDrag {
                panel,
                group,
                whole_window,
                source,
                started: global,
                pointer: global,
                grab_offset: (pos - response.rect.min) * zoom,
                window_grab_offset: pos.to_vec2() * zoom,
                moved: false,
                released: false,
                began: Instant::now(),
            });
            self.detached.drag_preview = None;
        }
        self.continue_panel_drag(ctx);
        if self.detached.drag.is_some_and(|drag| drag.moved) {
            ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
        }
        response.clone().on_hover_cursor(egui::CursorIcon::Grab).on_hover_text("Drag to an edge, group or tab in any NeoFlash window. Drop outside a dock to detach. Escape cancels.");
    }

    /// When all visible panels of a floating window are being dragged together,
    /// return that floating window's `PanelId` so it moves as a native window
    /// and never self-docks into its own surface.
    pub(crate) fn dragged_whole_floating_window(&self) -> Option<PanelId> {
        let drag = self.detached.drag?;
        if let Some(window) = drag.whole_window {
            return (drag.source == viewport(window)).then_some(window);
        }
        let window = self.workspace.floating_window(drag.panel)?;
        if drag.source != viewport(window.id) {
            return None;
        }
        let visible_in_window: Vec<PanelId> = window
            .panels()
            .into_iter()
            .filter(|panel| self.workspace.is_panel_visible(*panel))
            .collect();
        let dragged_panels: Vec<PanelId> = drag
            .group
            .and_then(|(surface, edge, group)| {
                self.current_dock_group(surface, edge, group).map(|g| {
                    g.tabs
                        .iter()
                        .copied()
                        .filter(|panel| self.workspace.is_panel_visible(*panel))
                        .collect()
                })
            })
            .unwrap_or_else(|| vec![drag.panel]);
        (visible_in_window.len() == dragged_panels.len()
            && visible_in_window
                .iter()
                .all(|panel| dragged_panels.contains(panel)))
        .then_some(window.id)
    }

    pub(crate) fn floating_window_title(&self, window_id: PanelId) -> Option<String> {
        let window = self
            .workspace
            .floating
            .iter()
            .find(|window| window.id == window_id)?;
        let titles = window
            .panels()
            .into_iter()
            .filter(|panel| self.workspace.is_panel_visible(*panel))
            .map(|panel| panel.title())
            .collect::<Vec<_>>();
        (!titles.is_empty()).then(|| format!("{} — NeoFlash", titles.join(" + ")))
    }

    /// Collect input from every viewport. Never infer a mouse release from
    /// !primary_down: focus/capture loss can clear local state while the physical
    /// button is still held. AppKit supplies the desktop position and button.
    pub(crate) fn continue_panel_drag(&mut self, ctx: &egui::Context) {
        let Some(mut drag) = self.detached.drag else {
            return;
        };
        if self.has_modal_dialog()
            || ctx.input(|i| i.key_pressed(egui::Key::Escape))
            || !self.workspace.is_panel_visible(drag.panel)
            || surface_viewport(self.workspace.panel_surface(drag.panel)) != drag.source
        {
            if drag.moved
                && let Some(window_id) = self.dragged_whole_floating_window()
                && let Some(saved) = self
                    .workspace
                    .floating
                    .iter()
                    .find(|w| w.id == window_id)
                    .and_then(|w| w.geometry)
            {
                let zoom = ctx.zoom_factor();
                ctx.send_viewport_cmd_to(
                    viewport(window_id),
                    egui::ViewportCommand::OuterPosition(egui::pos2(
                        saved.outer_position[0] / zoom,
                        saved.outer_position[1] / zoom,
                    )),
                );
            }
            self.detached.drag = None;
            self.detached.drag_preview = None;
            return;
        }
        if drag.released {
            return;
        }
        let zoom = ctx.zoom_factor();
        let inner = ctx
            .input(|i| i.viewport().inner_rect)
            .unwrap_or_else(|| ctx.viewport_rect());
        let (pointer, released) = ctx.input(|i| {
            let mut pointer = None;
            let mut released = false;
            for event in &i.events {
                match event {
                    egui::Event::PointerMoved(pos) => pointer = Some(*pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        ..
                    } => {
                        pointer = Some(*pos);
                        released = true;
                        break;
                    }
                    _ => {}
                }
            }
            (
                pointer.map(|pos| (inner.min + pos.to_vec2()) * zoom),
                released,
            )
        });
        if let Some(pointer) = pointer {
            drag.pointer = pointer;
        }
        drag.released = released;
        let native_pointer = crate::native_screens::pointer_state();
        if let Some((position, down)) = native_pointer {
            // Some backends synthesize an up event when focus/capture leaves
            // a window. A still-held native button proves that is not a drop.
            if !released || down {
                drag.pointer = position;
                drag.released = !down;
            }
        } else if !released
            && drag.began.elapsed() > Duration::from_secs(30)
            && !ctx.input(|i| i.pointer.primary_down())
        {
            // A backend with neither native capture nor a release event can
            // cancel safely; it must never invent a drop at a stale point.
            self.detached.drag = None;
            self.detached.drag_preview = None;
            return;
        }
        drag.moved |= drag.pointer.distance(drag.started) >= 8. * zoom;
        self.detached.drag = Some(drag);
        if drag.moved
            && !drag.released
            && native_pointer.is_some()
            && let Some(window_id) = self.dragged_whole_floating_window()
        {
            let position = drag.pointer - drag.window_grab_offset;
            ctx.send_viewport_cmd_to(
                viewport(window_id),
                egui::ViewportCommand::OuterPosition(position / zoom),
            );
        }
        ctx.request_repaint_of(egui::ViewportId::ROOT);
        for view in &self.detached.dock_viewports {
            ctx.request_repaint_of(view.viewport);
        }
        ctx.request_repaint_after(Duration::from_millis(16));
    }

    /// Apply exactly once, after all immediate viewports published their hit
    /// geometry. Moving tabs mid-paint would render the same panel twice and
    /// reuse egui widget IDs in two native windows during one frame.
    pub(crate) fn finalize_panel_drag(&mut self, ctx: &egui::Context) {
        self.continue_panel_drag(ctx);
        let Some(drag) = self.detached.drag.filter(|drag| drag.released) else {
            return;
        };
        let whole_window = self.dragged_whole_floating_window();
        let whole_floating = whole_window.is_some();
        let hit = drag
            .moved
            .then(|| self.dock_hit(drag.pointer, ctx.zoom_factor()))
            .flatten();
        self.detached.drag = None;
        let preview = self.detached.drag_preview.take();
        if !drag.moved {
            return;
        }
        let panels = whole_window
            .and_then(|window_id| {
                self.workspace
                    .floating
                    .iter()
                    .find(|window| window.id == window_id)
                    .map(|window| window.panels())
            })
            .or_else(|| {
                drag.group.and_then(|(surface, edge, group)| {
                    self.current_dock_group(surface, edge, group)
                        .map(|g| g.tabs.clone())
                })
            })
            .unwrap_or_else(|| vec![drag.panel]);
        if let Some(hit) = hit {
            if drag.group.is_some() || whole_window.is_some() {
                self.workspace
                    .dock_group(&panels, drag.panel, hit.destination);
                self.focus_dock_panel(ctx, drag.panel);
            } else {
                self.dock_panel_at(ctx, drag.panel, hit.destination);
            }
        } else {
            let zoom = ctx.zoom_factor();
            let grouped = self
                .workspace
                .floating_window(drag.panel)
                .is_some_and(|window| window.panels().len() > panels.len());
            let mut geometry = if grouped {
                None
            } else {
                self.workspace.detached_geometry(drag.panel)
            }
            .unwrap_or(WindowGeometry {
                outer_position: [0., 0.],
                inner_size: scaled_size(crate::workspace::default_window_size(drag.panel), zoom),
            });
            let grab_offset = if whole_floating {
                drag.window_grab_offset
            } else if let Some(preview) = preview {
                geometry.inner_size = preview.size.into();
                preview.grab_offset
            } else {
                drag.grab_offset
            };
            let position = drag.pointer - grab_offset;
            geometry.outer_position = [position.x, position.y];
            let mut anchor = None;
            if panels == [PanelId::Tools]
                && let Some(stage) = self.tools_anchor_stage_rect(zoom)
                && let Some((snapped, snapped_geometry)) =
                    snap_tools_stage_anchor(stage, geometry, zoom)
            {
                anchor = Some(snapped);
                geometry = snapped_geometry;
            }
            let window = if let Some(window) = whole_window {
                window
            } else if drag.group.is_some() {
                self.workspace.float_group(&panels, drag.panel, geometry)
            } else {
                self.workspace.float_panel(drag.panel, Some(geometry))
            };
            self.workspace.remember_floating_geometry(window, geometry);
            self.workspace.set_tools_stage_anchor(window, anchor);
            self.bottom = self.workspace.active_bottom;
            self.detached.focus = Some(drag.panel);
            ctx.send_viewport_cmd_to(
                viewport(window),
                egui::ViewportCommand::OuterPosition(
                    egui::pos2(geometry.outer_position[0], geometry.outer_position[1]) / zoom,
                ),
            );
            ctx.send_viewport_cmd_to(
                viewport(window),
                egui::ViewportCommand::InnerSize(
                    egui::vec2(geometry.inner_size[0], geometry.inner_size[1]) / zoom,
                ),
            );
        }
        ctx.request_repaint_of(egui::ViewportId::ROOT);
    }

    pub(crate) fn dragged_panel_in_group(
        &self,
        surface: DockSurface,
        group: &crate::workspace::DockGroup,
    ) -> Option<PanelId> {
        self.detached
            .drag
            .filter(|drag| {
                drag.source == surface_viewport(surface) && group.tabs.contains(&drag.panel)
            })
            .map(|drag| drag.panel)
    }

    pub(crate) fn remember_panel_drag_preview(
        &mut self,
        ui: &egui::Ui,
        surface: DockSurface,
        group: &crate::workspace::DockGroup,
        rect: egui::Rect,
        capture: (usize, Option<egui::Vec2>),
    ) {
        let (first_shape, grab_offset) = capture;
        let Some(panel) = self.dragged_panel_in_group(surface, group) else {
            return;
        };
        let Some(drag) = self.detached.drag else {
            return;
        };
        let ctx = ui.ctx();
        let mut shapes = vec![egui::epaint::ClippedShape {
            clip_rect: rect,
            // The parent panel paints its background after its closure returns.
            shape: neoflash_ui::components::Surface::PanelFlush
                .frame(ctx)
                .paint(rect),
        }];
        ctx.graphics(|graphics| {
            if let Some(list) = graphics.get(ui.layer_id()) {
                shapes.extend(list.all_entries().skip(first_shape).filter_map(|shape| {
                    let mut shape = shape.clone();
                    shape.clip_rect = shape.clip_rect.intersect(rect);
                    shape.clip_rect.is_positive().then_some(shape)
                }));
            }
        });
        for shape in &mut shapes {
            shape.transform(egui::emath::TSTransform::from_translation(
                -rect.min.to_vec2(),
            ));
        }
        let zoom = ctx.zoom_factor();
        let inner = ctx
            .input(|i| i.viewport().inner_rect)
            .unwrap_or_else(|| ctx.viewport_rect());
        self.detached.drag_preview = Some(PanelDragPreview {
            panel,
            source: drag.source,
            grab_offset: grab_offset
                .unwrap_or_else(|| drag.started - (inner.min + rect.min.to_vec2()) * zoom),
            zoom,
            size: rect.size() * zoom,
            shapes,
        });
    }

    /// Horizontal docks stretch to the workspace width. A torn-off panel uses
    /// its floating layout immediately, including old preferences that may
    /// have accidentally remembered the entire dock width.
    pub(crate) fn compact_panel_drag_layout(
        &self,
        surface: DockSurface,
        edge: DockEdge,
        group: &crate::workspace::DockGroup,
        source_rect: egui::Rect,
        zoom: f32,
    ) -> Option<(crate::workspace::DockGroup, egui::Vec2, egui::Vec2)> {
        let panel = self.dragged_panel_in_group(surface, group)?;
        let drag = self.detached.drag?;
        if !drag.moved
            || surface != DockSurface::Main
            || !matches!(edge, DockEdge::Top | DockEdge::Bottom)
        {
            return None;
        }
        let default = scaled_size(crate::workspace::default_window_size(panel), zoom);
        let minimum = scaled_size(crate::workspace::minimum_window_size(panel), zoom);
        let saved = self
            .workspace
            .detached_geometry(panel)
            .map_or(default, |geometry| geometry.inner_size);
        let size = egui::vec2(
            saved[0].min(default[0]).max(minimum[0]),
            saved[1].min(default[1]).max(minimum[1]),
        );
        let mut floating_group = group.clone();
        floating_group.active = panel;
        let mut grab_offset = drag.grab_offset;
        if drag.group.is_none() {
            floating_group.tabs = vec![panel];
        } else {
            // Keep the relative grip position when a whole tab group shrinks.
            let source = self
                .detached
                .dock_groups
                .iter()
                .find(|target| target.surface == surface && target.group == group.id)?;
            grab_offset = drag.started - source.rect.min;
            grab_offset.x *= size.x / (source_rect.width() * zoom);
        }
        grab_offset.x = grab_offset.x.clamp(0., size.x);
        grab_offset.y = grab_offset
            .y
            .clamp(0., neoflash_ui::metrics::panel_header() * zoom);
        Some((floating_group, size / zoom, grab_offset))
    }

    #[cfg(test)]
    pub(crate) fn panel_drag_preview_visible(&self) -> bool {
        self.detached
            .drag
            .is_some_and(|drag| drag.moved && !drag.released)
            && self.detached.drag_preview.is_some()
    }

    fn docking_overlay(&self, ctx: &egui::Context, surface: DockSurface) {
        let Some(drag) = self.detached.drag.filter(|drag| drag.moved) else {
            return;
        };
        let Some(view) = self
            .detached
            .dock_viewports
            .iter()
            .find(|view| view.surface == surface)
        else {
            return;
        };
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new(("panel-dock-target", surface)),
        ));
        if matches!(surface, DockSurface::Floating(id) if self.dragged_whole_floating_window() == Some(id))
        {
            if crate::native_screens::pointer_state().is_some()
                && let Some(hit) = self.dock_hit(drag.pointer, view.zoom)
            {
                let pointer = (drag.window_grab_offset / view.zoom).to_pos2();
                neoflash_ui::components::drag_ghost_at(
                    &painter,
                    ctx,
                    pointer + egui::vec2(12., 24.),
                    &format!("{} · {}", title(drag.panel), hit.caption),
                );
            }
            return;
        }
        let local = |rect: egui::Rect| rect.translate(-view.inner.min.to_vec2()) / view.zoom;
        let hit = self
            .dock_hit(drag.pointer, view.zoom)
            .filter(|hit| hit.surface == surface);
        for (edge, global) in edge_targets(view.content, view.zoom) {
            neoflash_ui::components::dock_target(&painter, ctx, local(global), edge.title(), false);
        }
        if let Some(hit) = &hit {
            neoflash_ui::components::dock_target(
                &painter,
                ctx,
                local(hit.rect),
                &hit.caption,
                true,
            );
        }
        // Follow the pointer with the panel's actual contents, not just its tab
        // label. Only paint copies: the source remains the sole widget owner.
        if self
            .dock_surface_at(drag.pointer)
            .is_some_and(|under_pointer| under_pointer.surface == surface)
        {
            let pointer = (drag.pointer - view.inner.min).to_pos2() / view.zoom;
            if (self.dragged_whole_floating_window().is_none()
                || crate::native_screens::pointer_state().is_none())
                && let Some(preview) = &self.detached.drag_preview
                && preview.panel == drag.panel
                && preview.source == drag.source
            {
                let transform = egui::emath::TSTransform {
                    translation: (drag.pointer - preview.grab_offset - view.inner.min) / view.zoom,
                    scaling: preview.zoom / view.zoom,
                };
                for shape in &preview.shapes {
                    let mut shape = shape.clone();
                    shape.transform(transform);
                    shape.clip_rect = shape.clip_rect.intersect(ctx.viewport_rect());
                    if shape.clip_rect.is_positive() {
                        painter.with_clip_rect(shape.clip_rect).add(shape.shape);
                    }
                }
                // Capture the frame that actually paints the preview, even if
                // an automated drag also delivered its release in this frame.
                if ctx.viewport_id() == egui::ViewportId::ROOT
                    && std::env::var_os("NEOFLASH_SCREENSHOT").is_some()
                    && std::env::var_os("NEOFLASH_SCREENSHOT_DRAG").is_some()
                {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
                }
            }
            if let Some(hit) = hit {
                neoflash_ui::components::drag_ghost_at(
                    &painter,
                    ctx,
                    pointer + egui::vec2(12., 24.),
                    &format!("{} · {}", title(drag.panel), hit.caption),
                );
            }
        }
    }

    pub(crate) fn dock_panel_at(
        &mut self,
        ctx: &egui::Context,
        panel: PanelId,
        destination: DockDestination,
    ) {
        self.workspace.dock_panel(panel, destination);
        self.focus_dock_panel(ctx, panel);
    }

    pub(crate) fn focus_dock_panel(&mut self, ctx: &egui::Context, panel: PanelId) {
        self.bottom = self.workspace.active_bottom;
        let surface = self.workspace.panel_surface(panel);
        if surface == DockSurface::Main {
            self.detached.focus_docked_actions = panel == PanelId::Actions;
            if self.detached.focus == Some(panel) {
                self.detached.focus = None;
            }
        } else {
            self.detached.focus = Some(panel);
        }
        ctx.send_viewport_cmd_to(surface_viewport(surface), egui::ViewportCommand::Focus);
        ctx.request_repaint_of(egui::ViewportId::ROOT);
    }

    pub(crate) fn set_panel_placement(
        &mut self,
        ctx: &egui::Context,
        panel: PanelId,
        placement: PanelPlacement,
    ) {
        self.workspace.active_bottom = self.bottom;
        self.workspace.set_placement(panel, placement);
        self.bottom = self.workspace.active_bottom;
        if placement == PanelPlacement::Detached {
            self.detached.focus = Some(panel);
        } else {
            self.detached.focus_docked_actions = panel == PanelId::Actions;
            ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Focus);
        }
        ctx.request_repaint_of(egui::ViewportId::ROOT);
    }

    pub(crate) fn prepare_docked_panel_focus(&mut self, ctx: &egui::Context) {
        if std::mem::take(&mut self.detached.focus_docked_actions)
            && self.workspace.is_panel_docked(PanelId::Actions)
            && self.workspace.is_panel_visible(PanelId::Actions)
        {
            ctx.memory_mut(|memory| memory.request_focus(self.actions_editor_id()));
        }
    }

    pub(crate) fn detached_panels(&mut self, ctx: &egui::Context) {
        let zoom = ctx.zoom_factor();
        let root_rect = ctx.viewport_rect();
        self.detached.root_inner =
            Some(ctx.input(|i| i.viewport().inner_rect).unwrap_or(root_rect) * zoom);
        self.continue_panel_drag(ctx);
        let fallback = ctx.input(|i| {
            let rect = i.viewport().outer_rect.unwrap_or(root_rect);
            ScreenRect {
                origin: [rect.min.x * zoom, rect.min.y * zoom],
                size: [rect.width() * zoom, rect.height() * zoom],
            }
        });
        let tools_stage = self.tools_anchor_stage_rect(zoom);
        let mut topology_changed = false;
        if self
            .detached
            .checked_monitors
            .is_none_or(|at| at.elapsed() >= Duration::from_secs(1))
        {
            let monitors = available_work_areas();
            topology_changed = monitors != self.detached.monitors;
            self.detached.monitors = monitors;
            self.detached.checked_monitors = Some(Instant::now());
        }
        let visible: Vec<_> = if self.panels_hidden {
            Vec::new()
        } else {
            self.workspace
                .floating
                .iter()
                .filter(|window| {
                    window
                        .panels()
                        .iter()
                        .any(|panel| self.workspace.is_panel_visible(*panel))
                })
                .map(|window| window.id)
                .collect()
        };
        self.detached.active.retain(|id| visible.contains(id));
        self.detached.dock_viewports.retain(|v| match v.surface {
            DockSurface::Main => true,
            DockSurface::Floating(id) => visible.contains(&id),
        });
        self.detached.dock_groups.retain(|g| match g.surface {
            DockSurface::Main => true,
            DockSurface::Floating(id) => visible.contains(&id),
        });
        for window_id in visible {
            let Some(window) = self
                .workspace
                .floating
                .iter()
                .find(|window| window.id == window_id)
                .cloned()
            else {
                continue;
            };
            let panels = window.panels();
            let Some(&panel) = panels.first() else {
                continue;
            };
            let id = viewport(window_id);
            let initial = !self.detached.active.contains(&window_id);
            let collapsed = ctx
                .data(|data| data.get_temp::<bool>(floating_collapsed_id(window_id)))
                .unwrap_or(false);
            let mut minimum_logical = window
                .root
                .as_ref()
                .map_or([80., 80.], |root| root.minimum_size());
            if matches!(
                window.root.as_ref(),
                Some(crate::workspace::FloatingNode::Split { .. })
            ) {
                minimum_logical[1] += neoflash_ui::metrics::panel_tab();
            }
            if panels == [PanelId::Tools] {
                minimum_logical[0] = minimum_logical[0].max(floating_tools_width());
            }
            let root_maximum_logical = window
                .root
                .as_ref()
                .map_or([4096., 2160.], |root| root.maximum_size());
            let mut maximum_logical = root_maximum_logical;
            let pointer_down = crate::native_screens::pointer_state().is_some_and(|(_, down)| down)
                || ctx.input(|i| i.pointer.any_down());
            let active_resize_or_drag =
                self.detached.drag.is_some() || self.detached.resize.is_some() || pointer_down;
            if panels == [PanelId::Tools]
                && !initial
                && !active_resize_or_drag
                && let Some(desired_h) = self.detached.tools_desired_inner_height
                && window
                    .geometry
                    .is_some_and(|saved| ((saved.inner_size[1] / zoom) - desired_h).abs() <= 1.5)
            {
                maximum_logical[1] = desired_h.max(minimum_logical[1]);
            }
            maximum_logical[0] = maximum_logical[0].max(minimum_logical[0]);
            maximum_logical[1] = maximum_logical[1].max(minimum_logical[1]);
            let minimum = scaled_size(minimum_logical, zoom);
            let root_maximum = scaled_size(
                [
                    root_maximum_logical[0].max(minimum_logical[0]),
                    root_maximum_logical[1].max(minimum_logical[1]),
                ],
                zoom,
            );
            let saved = window.geometry;
            let mut desired = saved.unwrap_or(WindowGeometry {
                outer_position: [fallback.origin[0] + 40., fallback.origin[1] + 40.],
                inner_size: scaled_size(crate::workspace::default_window_size(panel), zoom),
            });
            desired.inner_size = [
                desired.inner_size[0].clamp(minimum[0], root_maximum[0]),
                desired.inner_size[1].clamp(minimum[1], root_maximum[1]),
            ];
            if !active_resize_or_drag
                && panels == [PanelId::Tools]
                && let (Some(anchor), Some(stage)) = (window.tools_anchor, tools_stage)
            {
                desired = apply_tools_stage_anchor(anchor, stage, desired, zoom);
            }
            let geometry = recover_geometry_with_bounds(
                panel,
                Some(desired),
                minimum,
                root_maximum,
                &self.detached.monitors,
                fallback,
            );
            let anchored =
                !active_resize_or_drag && window.tools_anchor.is_some() && saved != Some(geometry);
            let recovering =
                !active_resize_or_drag && (topology_changed || anchored) && saved != Some(geometry);
            let growing = !active_resize_or_drag
                && saved.is_some_and(|saved| saved.inner_size != geometry.inner_size);
            let title = format!(
                "{} — NeoFlash",
                panels
                    .iter()
                    .filter(|panel| self.workspace.is_panel_visible(**panel))
                    .map(|panel| panel.title())
                    .collect::<Vec<_>>()
                    .join(" + ")
            );
            let mut builder = egui::ViewportBuilder::default()
                .with_title(&title)
                .with_decorations(false)
                .with_title_shown(false)
                .with_titlebar_shown(false)
                .with_titlebar_buttons_shown(false)
                .with_close_button(false)
                .with_minimize_button(false)
                .with_maximize_button(false)
                .with_window_level(egui::WindowLevel::AlwaysOnTop)
                .with_min_inner_size([
                    minimum_logical[0],
                    if collapsed {
                        neoflash_ui::metrics::panel_tab()
                    } else {
                        minimum_logical[1]
                    },
                ])
                .with_max_inner_size([
                    maximum_logical[0],
                    if collapsed {
                        neoflash_ui::metrics::panel_tab()
                    } else {
                        maximum_logical[1]
                    },
                ]);
            if initial {
                builder = builder
                    .with_position([
                        geometry.outer_position[0] / zoom,
                        geometry.outer_position[1] / zoom,
                    ])
                    .with_inner_size([
                        geometry.inner_size[0] / zoom,
                        geometry.inner_size[1] / zoom,
                    ]);
                self.workspace
                    .remember_floating_geometry(window_id, geometry);
            } else if recovering || growing {
                if recovering {
                    ctx.send_viewport_cmd_to(
                        id,
                        egui::ViewportCommand::OuterPosition(egui::pos2(
                            geometry.outer_position[0] / zoom,
                            geometry.outer_position[1] / zoom,
                        )),
                    );
                }
                ctx.send_viewport_cmd_to(
                    id,
                    egui::ViewportCommand::InnerSize(egui::vec2(
                        geometry.inner_size[0] / zoom,
                        geometry.inner_size[1] / zoom,
                    )),
                );
                self.workspace
                    .remember_floating_geometry(window_id, geometry);
            }
            self.detached.active.insert(window_id);
            let focus = self.detached.focus.filter(|panel| panels.contains(panel));
            if focus.is_some() {
                self.detached.focus = None;
            }
            ctx.show_viewport_immediate(id, builder, |ctx, class| {
                self.detached.context_menu_open = false;
                // Native file events belong to the receiving window. A panel
                // has no Stage drop coordinates, so never silently consume a
                // Finder drop or place artwork at an unrelated remembered point.
                let dropped_on_panel = ctx.input_mut(|input| {
                    let dropped = !input.raw.dropped_files.is_empty();
                    input.raw.dropped_files.clear();
                    dropped
                });
                if dropped_on_panel {
                    self.message("Drop graphics onto the Stage in the main window, or use File → Import graphics to Stage.");
                    ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Focus);
                    ctx.request_repaint_of(egui::ViewportId::ROOT);
                }
                if class == egui::ViewportClass::Embedded {
                    // Backends without native child windows: an in-window NF2 floating panel
                    // (`bg/raised`, radius 12, border, popover shadow, `Floating Panel Header`).
                    let open = self.embedded_floating_panel(ctx, window_id, &panels);
                    if !open {
                        for panel in &panels {
                            self.set_panel_visible(*panel, false);
                        }
                    }
                    self.continue_panel_drag(ctx);
                    self.docking_overlay(ctx, DockSurface::Floating(window_id));
                    self.finish_floating_context_menu(ctx);
                    return;
                }
                if !initial
                    && !collapsed
                    && !recovering
                    && !growing
                    && self.detached.drag.is_none()
                    && self.detached.resize.is_none()
                    && let Some((raw_w, raw_h, mut geometry)) = ctx.input(|i| {
                        let v = i.viewport();
                        if v.minimized == Some(true) {
                            return None;
                        }
                        let outer = v.outer_rect?;
                        let inner = v.inner_rect?;
                        let raw_w = inner.width() * zoom;
                        let raw_h = inner.height() * zoom;
                        Some((
                            raw_w,
                            raw_h,
                            WindowGeometry {
                                outer_position: [outer.min.x * zoom, outer.min.y * zoom],
                                inner_size: [
                                    raw_w.clamp(minimum[0], root_maximum[0]),
                                    raw_h.clamp(minimum[1], root_maximum[1]),
                                ],
                            },
                        ))
                    })
                {
                    if panels == [PanelId::Tools]
                        && self
                            .workspace
                            .floating
                            .iter()
                            .find(|w| w.id == window_id)
                            .and_then(|w| w.geometry)
                            .is_some_and(|prev| (prev.inner_size[0] - geometry.inner_size[0]).abs() > 1.5)
                    {
                        self.workspace.set_tools_stage_anchor(window_id, None);
                    }
                    if (raw_w - geometry.inner_size[0]).abs() > 0.5
                        || (raw_h - geometry.inner_size[1]).abs() > 0.5
                    {
                        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                            geometry.inner_size[0] / zoom,
                            geometry.inner_size[1] / zoom,
                        )));
                    }
                    geometry = recover_geometry_with_bounds(
                        panel,
                        Some(geometry),
                        minimum,
                        root_maximum,
                        &self.detached.monitors,
                        fallback,
                    );
                    self.workspace
                        .remember_floating_geometry(window_id, geometry);
                }
                if ctx.input(|i| i.viewport().close_requested()) {
                    self.close_floating_window(window_id);
                    ctx.request_repaint_of(egui::ViewportId::ROOT);
                    return;
                }
                if let Some(focus) = focus {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    if focus == PanelId::Actions {
                        ctx.memory_mut(|memory| memory.request_focus(self.actions_editor_id()));
                    }
                }
                let has_input = ctx.input(|i| !i.events.is_empty());
                self.continue_panel_drag(ctx);
                self.shortcuts(ctx);
                if self.workspace.floating.iter().any(|window| {
                    window.id == window_id
                        && window
                            .panels()
                            .iter()
                            .any(|panel| self.workspace.is_panel_visible(*panel))
                }) {
                    let viewport_rect = ctx.viewport_rect();
                    let inner_size = ctx
                        .input(|i| i.viewport().inner_rect.map(|r| r.size()))
                        .unwrap_or(viewport_rect.size());
                    let full_rect = egui::Rect::from_min_size(
                        viewport_rect.min,
                        egui::vec2(
                            viewport_rect.width().max(inner_size.x).max(minimum_logical[0]),
                            viewport_rect.height().max(inner_size.y),
                        ),
                    );
                    ctx.layer_painter(egui::LayerId::background()).rect_filled(
                        full_rect,
                        0.0,
                        neoflash_ui::palette(ctx).bg_panel,
                    );
                    let mut open = true;
                    egui::CentralPanel::default()
                        .frame(neoflash_ui::components::Surface::PanelFlush.frame(ctx))
                        .show(ctx, |ui| {
                            ui.set_min_size(viewport_rect.size());
                            open = self.native_floating_panel(
                                ui,
                                window_id,
                                &panels,
                                minimum_logical,
                                zoom,
                            );
                        });
                    if !open {
                        self.close_floating_window(window_id);
                        ctx.request_repaint_of(egui::ViewportId::ROOT);
                        return;
                    }
                    let collapsed = ctx
                        .data(|data| {
                            data.get_temp::<bool>(floating_collapsed_id(window_id))
                        })
                        .unwrap_or(false);
                    if !collapsed {
                        self.floating_window_resize_borders(
                            ctx, window_id, &panels, minimum, root_maximum, zoom,
                        );
                    }
                    if !collapsed
                        && panels.as_slice() == [PanelId::Tools]
                        && let Some(desired_h) = self.detached.tools_desired_inner_height
                    {
                        self.compact_floating_tools_window_bottom_up(
                            ctx,
                            window_id,
                            desired_h,
                            minimum[0],
                            minimum[1],
                            zoom,
                        );
                    }
                }
                self.docking_overlay(ctx, DockSurface::Floating(window_id));
                self.finish_floating_context_menu(ctx);
                if has_input {
                    ctx.request_repaint_of(egui::ViewportId::ROOT);
                    if self.confirm.is_some() {
                        ctx.send_viewport_cmd_to(
                            egui::ViewportId::ROOT,
                            egui::ViewportCommand::Focus,
                        );
                    }
                }
            });
        }
        self.docking_overlay(ctx, DockSurface::Main);
        self.finalize_panel_drag(ctx);
        if !self.detached.active.is_empty() {
            ctx.request_repaint_after(Duration::from_secs(1));
        }
    }

    pub(crate) fn close_floating_window(&mut self, window_id: PanelId) {
        let panels = self
            .workspace
            .floating
            .iter()
            .find(|w| w.id == window_id)
            .map(|w| w.panels())
            .unwrap_or_default();
        for panel in panels {
            self.set_panel_visible(panel, false);
        }
        let id = viewport(window_id);
        self.detached.active.remove(&window_id);
        if self.detached.drag.is_some_and(|drag| drag.source == id) {
            self.detached.drag = None;
            self.detached.drag_preview = None;
        }
        if self
            .detached
            .resize
            .is_some_and(|resize| resize.window_id == window_id)
        {
            self.detached.resize = None;
        }
        self.detached
            .dock_viewports
            .retain(|v| v.surface != DockSurface::Floating(window_id));
        self.detached
            .dock_groups
            .retain(|g| g.surface != DockSurface::Floating(window_id));
    }

    pub(crate) fn close_floating_viewport(&mut self, viewport_id: egui::ViewportId) -> bool {
        let Some(window_id) = self
            .workspace
            .floating
            .iter()
            .map(|w| w.id)
            .find(|&id| viewport(id) == viewport_id)
        else {
            return false;
        };
        self.close_floating_window(window_id);
        true
    }

    /// Apply pending native close events on detached panel windows before menus or viewports draw,
    /// so the Window menu checkmark updates immediately and closing viewports are not re-submitted.
    pub(crate) fn sync_closed_detached_panels(&mut self, ctx: &egui::Context) {
        if self.panels_hidden {
            return;
        }
        let zoom = ctx.zoom_factor();
        let windows: Vec<PanelId> = self
            .workspace
            .floating
            .iter()
            .filter(|window| {
                window
                    .panels()
                    .iter()
                    .any(|panel| self.workspace.is_panel_visible(*panel))
            })
            .map(|window| window.id)
            .collect();
        for window_id in windows {
            let id = viewport(window_id);
            let close_geometry = ctx.input(|i| {
                let v = i.raw.viewports.get(&id)?;
                if !v.close_requested() {
                    return None;
                }
                let geometry = if v.minimized == Some(true) {
                    None
                } else {
                    v.outer_rect
                        .zip(v.inner_rect)
                        .map(|(outer, inner)| WindowGeometry {
                            outer_position: [outer.min.x * zoom, outer.min.y * zoom],
                            inner_size: [inner.width() * zoom, inner.height() * zoom],
                        })
                };
                Some(geometry)
            });
            if let Some(geometry) = close_geometry {
                if let Some(geometry) = geometry {
                    self.workspace
                        .remember_floating_geometry(window_id, geometry);
                }
                self.close_floating_window(window_id);
                ctx.request_repaint_of(egui::ViewportId::ROOT);
            }
        }
    }

    pub(crate) fn floating_panel_commands(
        &mut self,
        ui: &mut egui::Ui,
        window: PanelId,
        target: &ClipboardTarget,
    ) {
        let ctx = ui.ctx().clone();
        self.detached.context_menu_open = true;
        if !self.clipboard.menu_open || self.clipboard.menu_viewport != Some(ctx.viewport_id()) {
            self.clipboard.menu_target = Some(target.clone());
        }
        self.clipboard.menu_open = true;
        self.clipboard.menu_viewport = Some(ctx.viewport_id());
        let text = self
            .clipboard
            .menu_target
            .as_ref()
            .is_some_and(|t| t.is_text());
        for (name, cmd_id, command) in [
            (
                "Cut",
                crate::commands::CommandId::EditCut,
                ClipboardCommand::Cut,
            ),
            (
                "Copy",
                crate::commands::CommandId::EditCopy,
                ClipboardCommand::Copy,
            ),
            (
                "Paste",
                crate::commands::CommandId::EditPaste,
                ClipboardCommand::Paste,
            ),
        ] {
            let enabled = text
                || if command == ClipboardCommand::Paste {
                    self.layer_editable()
                } else {
                    !self.selection.is_empty()
                };
            if crate::ui_icons::menu_item_enabled(ui, enabled, name, &self.sc(cmd_id)).clicked() {
                self.request_clipboard(&ctx, command);
                ui.close();
            }
        }
        ui.separator();
        if crate::ui_icons::menu_item(ui, "Dock all panels in this window", "").clicked() {
            let panels = self
                .workspace
                .floating
                .iter()
                .find(|w| w.id == window)
                .map(|w| w.panels())
                .unwrap_or_default();
            for panel in panels {
                let visible = self.workspace.is_panel_visible(panel);
                self.set_panel_placement(&ctx, panel, PanelPlacement::Docked);
                self.set_panel_visible(panel, visible);
            }
            ui.close();
        }
        ui.separator();
    }

    /// Native borderless windows still use the locked NF2 floating-window chrome.
    /// Dock-group headers remain below it so tabs and split groups retain their behavior.
    pub(crate) fn native_floating_panel(
        &mut self,
        ui: &mut egui::Ui,
        window_id: PanelId,
        panels: &[PanelId],
        minimum_logical: [f32; 2],
        zoom: f32,
    ) -> bool {
        ui.painter()
            .rect_filled(ui.max_rect(), 0.0, neoflash_ui::palette(ui.ctx()).bg_panel);
        // Each native viewport republishes its live hit geometry every frame. Clear the previous
        // body first so a collapsed 36 pt header cannot leave an invisible full-window docking
        // target behind; expanded bodies register their current targets again below.
        let surface = crate::workspace::DockSurface::Floating(window_id);
        self.detached
            .dock_viewports
            .retain(|target| target.surface != surface);
        self.detached
            .dock_groups
            .retain(|target| target.surface != surface);
        let collapsed_id = floating_collapsed_id(window_id);
        let collapsed = ui
            .ctx()
            .data(|data| data.get_temp::<bool>(collapsed_id))
            .unwrap_or(false);
        let single_group = self
            .workspace
            .floating
            .iter()
            .find(|window| window.id == window_id)
            .and_then(|window| match window.root.as_ref()? {
                crate::workspace::FloatingNode::Group(group) => Some(group.clone()),
                crate::workspace::FloatingNode::Split { .. } => None,
            });
        let mut visible_tabs: Vec<PanelId> = single_group
            .as_ref()
            .map(|group| {
                let mut tabs: Vec<_> = group
                    .tabs
                    .iter()
                    .copied()
                    .filter(|panel| self.workspace.is_panel_visible(*panel))
                    .collect();
                if tabs.len() > 3 {
                    if tabs
                        .iter()
                        .position(|panel| *panel == group.active)
                        .is_some_and(|index| index >= 3)
                    {
                        tabs = vec![tabs[0], tabs[1], group.active];
                    } else {
                        tabs.truncate(3);
                    }
                }
                tabs
            })
            .unwrap_or_default();
        if let Some(group) = &single_group {
            let palette = neoflash_ui::palette(ui.ctx());
            let tab_budget =
                (ui.available_width() - floating_header_chrome_width_for(panels)).max(1.0);
            let tab_width = |ui: &egui::Ui, panel: PanelId| {
                ui.fonts_mut(|fonts| {
                    fonts
                        .layout_no_wrap(
                            panel.title().into(),
                            neoflash_ui::typography::font(neoflash_ui::TextStyle::SmallStrong),
                            palette.text_primary,
                        )
                        .size()
                        .x
                        + 2.0 * neoflash_ui::tokens::space::S3
                })
            };
            while visible_tabs.len() > 1
                && visible_tabs
                    .iter()
                    .copied()
                    .map(|panel| tab_width(ui, panel))
                    .sum::<f32>()
                    > tab_budget
            {
                let remove = visible_tabs
                    .iter()
                    .rposition(|panel| *panel != group.active)
                    .unwrap_or(visible_tabs.len() - 1);
                visible_tabs.remove(remove);
            }
        }
        let active_index = single_group
            .as_ref()
            .and_then(|group| visible_tabs.iter().position(|panel| *panel == group.active))
            .unwrap_or(0);
        let active = visible_tabs
            .get(active_index)
            .copied()
            .or_else(|| {
                self.workspace
                    .floating
                    .iter()
                    .find(|window| window.id == window_id)
                    .and_then(|window| {
                        window.panels().into_iter().find(|panel| {
                            self.workspace.is_panel_visible(*panel)
                                && window
                                    .root
                                    .as_ref()
                                    .is_some_and(|root| active_in(root, *panel))
                        })
                    })
            })
            .or_else(|| {
                panels
                    .iter()
                    .copied()
                    .find(|panel| self.workspace.is_panel_visible(*panel))
            })
            .unwrap_or_else(|| panels.first().copied().unwrap_or(window_id));
        let titles: Vec<&str> = visible_tabs.iter().map(|panel| panel.title()).collect();
        let clipboard_target = self.clipboard_target(ui.ctx());
        let compact_tools_header = panels == [PanelId::Tools]
            && ui.available_width() + 0.5 < floating_full_tools_header_width();
        let mut header_tabs = Vec::new();
        let mut header_collapse = false;
        let mut header_close = false;
        let (header_rect, header_menu) = if compact_tools_header {
            let palette = neoflash_ui::palette(ui.ctx());
            let width = ui.available_width().max(minimum_logical[0]);
            let height = TOOLS_GRIP.min(neoflash_ui::metrics::panel_tab());
            let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
            if ui.is_rect_visible(rect) {
                ui.painter().rect_filled(rect, 0.0, palette.bg_panel);
                ui.painter().line_segment(
                    [rect.left_bottom(), rect.right_bottom()],
                    egui::Stroke::new(1.0_f32, palette.border_subtle),
                );
            }
            let more_side = if width < 64.0 {
                neoflash_ui::tokens::space::S5
            } else {
                neoflash_ui::metrics::min_hit()
            };
            let right_pad = if width < 64.0 {
                0.5 * neoflash_ui::tokens::space::S1
            } else {
                neoflash_ui::tokens::space::S1
            };
            let more_rect = egui::Rect::from_center_size(
                egui::pos2(rect.right() - right_pad - more_side * 0.5, rect.center().y),
                egui::vec2(more_side, more_side),
            );
            let grip_rect = egui::Rect::from_min_max(
                rect.min,
                egui::pos2(more_rect.left().max(rect.left()), rect.bottom()),
            );
            let grip_response = ui.interact(
                grip_rect,
                ui.id().with(("native-floating-tools-grip", window_id)),
                egui::Sense::click_and_drag(),
            );
            grip_response.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    ui.is_enabled(),
                    PanelId::Tools.title(),
                )
            });
            if ui.is_rect_visible(grip_rect) {
                let icon_side = neoflash_ui::metrics::ICON_UI.min(grip_rect.width());
                let icon_rect = egui::Rect::from_center_size(
                    grip_rect.center(),
                    egui::vec2(icon_side, icon_side),
                );
                let fg = if grip_response.hovered() || grip_response.dragged() {
                    palette.text_secondary
                } else {
                    palette.text_tertiary
                };
                neoflash_ui::icons::paint(
                    ui.painter(),
                    ui.ctx(),
                    icon_rect,
                    neoflash_ui::icons::i16::GRIP_H,
                    fg,
                );
            }
            self.floating_window_drag_handle(ui, window_id, PanelId::Tools, &grip_response);
            grip_response.context_menu(|ui| {
                self.dock_header_menu(
                    ui,
                    PanelId::Tools,
                    crate::workspace::DockSurface::Floating(window_id),
                    &clipboard_target,
                );
                ui.separator();
                let collapse_label = if collapsed {
                    "Expand panel"
                } else {
                    "Collapse panel"
                };
                if crate::ui_icons::menu_item(ui, collapse_label, "").clicked() {
                    header_collapse = true;
                    ui.close();
                }
                if crate::ui_icons::menu_item(ui, "Close panel", "").clicked() {
                    header_close = true;
                    ui.close();
                }
            });
            header_tabs.push((PanelId::Tools, grip_rect));
            let menu = ui.put(
                more_rect,
                neoflash_ui::components::IconButton::new(
                    neoflash_ui::icons::i16::MORE,
                    "Panel menu",
                )
                .side(more_side),
            );
            (rect, menu)
        } else {
            let header = neoflash_ui::components::FloatingPanelHeader {
                // Single-group windows use these as their only tabs. Split windows
                // retain the per-group headers below this window-level chrome.
                tabs: &titles,
                active: active_index,
                collapsed,
            }
            .show(ui);
            if let Some(panel) = header.tab.and_then(|index| visible_tabs.get(index)) {
                self.show_panel(*panel);
            }
            // `FloatingPanelHeader` intentionally returns only the clicked index, while docking
            // also needs per-tab drag/target rectangles. Reconstruct its locked tab geometry and
            // layer drag responses over the same painted tabs so a single-group window keeps the
            // individual reorder/tear-off behavior of an ordinary dock header.
            let tab_safe_right = header.drag.rect.right()
                - neoflash_ui::tokens::space::S1
                - 3.0 * neoflash_ui::metrics::MIN_HIT
                - neoflash_ui::tokens::space::S1;
            let tab_strip_start = header.drag.rect.left()
                + neoflash_ui::tokens::space::S2
                + neoflash_ui::icons::i16::GRIP.side()
                + neoflash_ui::tokens::space::S1;
            let mut tab_left = tab_strip_start;
            let palette = neoflash_ui::palette(ui.ctx());
            for (panel, title) in visible_tabs.iter().copied().zip(titles.iter().copied()) {
                let label_width = ui.fonts_mut(|fonts| {
                    fonts
                        .layout_no_wrap(
                            title.into(),
                            neoflash_ui::typography::font(neoflash_ui::TextStyle::SmallStrong),
                            palette.text_primary,
                        )
                        .size()
                        .x
                });
                let width = label_width + 2.0 * neoflash_ui::tokens::space::S3;
                let painted = egui::Rect::from_min_size(
                    egui::pos2(tab_left, header.drag.rect.top()),
                    egui::vec2(width, neoflash_ui::metrics::panel_tab()),
                );
                let interactive = painted.intersect(egui::Rect::from_min_max(
                    header.drag.rect.min,
                    egui::pos2(
                        tab_safe_right.max(header.drag.rect.left()),
                        header.drag.rect.bottom(),
                    ),
                ));
                tab_left += width;
                if !interactive.is_positive() {
                    continue;
                }
                let response = ui.interact(
                    interactive,
                    ui.id().with(("native-floating-tab-drag", window_id, panel)),
                    egui::Sense::click_and_drag(),
                );
                if response.clicked() {
                    self.show_panel(panel);
                }
                self.panel_drag_handle(ui, panel, &response);
                response.context_menu(|ui| {
                    self.dock_header_menu(
                        ui,
                        panel,
                        crate::workspace::DockSurface::Floating(window_id),
                        &clipboard_target,
                    )
                });
                header_tabs.push((panel, interactive));
            }
            // Move the whole window from the grip or genuinely empty header space only. Menu,
            // collapse and close presses remain buttons; painted tab presses move one panel.
            let whole_drag_rects = [
                egui::Rect::from_min_max(
                    header.drag.rect.min,
                    egui::pos2(tab_strip_start, header.drag.rect.bottom()),
                ),
                egui::Rect::from_min_max(
                    egui::pos2(
                        header_tabs
                            .last()
                            .map_or(tab_strip_start, |(_, rect)| rect.right()),
                        header.drag.rect.top(),
                    ),
                    egui::pos2(
                        tab_safe_right.max(header.drag.rect.left()),
                        header.drag.rect.bottom(),
                    ),
                ),
            ];
            for (index, rect) in whole_drag_rects.into_iter().enumerate() {
                if !rect.is_positive() {
                    continue;
                }
                let response = ui.interact(
                    rect,
                    ui.id()
                        .with(("native-floating-window-drag", window_id, index)),
                    egui::Sense::click_and_drag(),
                );
                self.floating_window_drag_handle(ui, window_id, active, &response);
            }
            header_collapse = header.collapse;
            header_close = header.close;
            (header.drag.rect, header.menu)
        };
        neoflash_ui::components::popup_below(&header_menu, 240., |ui| {
            if let Some(group) = &single_group {
                let tabs: Vec<_> = group
                    .tabs
                    .iter()
                    .copied()
                    .filter(|panel| self.workspace.is_panel_visible(*panel))
                    .collect();
                if tabs.len() > 1 {
                    ui.label(egui::RichText::new("Panels in this group").small());
                    for panel in tabs {
                        let mut selected = panel == group.active;
                        if crate::ui_icons::menu_toggle(ui, &mut selected, panel.title(), "")
                            .clicked()
                        {
                            self.show_panel(panel);
                            ui.close();
                        }
                    }
                    ui.separator();
                }
            }
            self.dock_header_commands(
                ui,
                active,
                crate::workspace::DockSurface::Floating(window_id),
                &clipboard_target,
            );
            if compact_tools_header {
                ui.separator();
                let collapse_label = if collapsed {
                    "Expand panel"
                } else {
                    "Collapse panel"
                };
                if crate::ui_icons::menu_item(ui, collapse_label, "").clicked() {
                    header_collapse = true;
                    ui.close();
                }
                if crate::ui_icons::menu_item(ui, "Close panel", "").clicked() {
                    header_close = true;
                    ui.close();
                }
            }
        });
        if header_collapse {
            let next = !collapsed;
            ui.ctx()
                .data_mut(|data| data.insert_temp(collapsed_id, next));
            let width = ui.ctx().viewport_rect().width().max(minimum_logical[0]);
            let height = if next {
                neoflash_ui::metrics::panel_tab()
            } else {
                self.workspace
                    .floating
                    .iter()
                    .find(|window| window.id == window_id)
                    .and_then(|window| window.geometry)
                    .map_or(minimum_logical[1], |geometry| {
                        (geometry.inner_size[1] / zoom).max(minimum_logical[1])
                    })
            };
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(width, height)));
        }
        if collapsed {
            let width = ui.ctx().viewport_rect().width().max(minimum_logical[0]);
            let height = neoflash_ui::metrics::panel_tab();
            if (ui.ctx().viewport_rect().height() - height).abs() > 1.0 {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(width, height)));
            }
        } else {
            let body = ui.available_rect_before_wrap();
            if body.is_positive() {
                ui.painter()
                    .rect_filled(body, 0.0, neoflash_ui::palette(ui.ctx()).bg_panel);
                let mut body_ui = ui.new_child(egui::UiBuilder::new().max_rect(body));
                body_ui.set_clip_rect(ui.clip_rect().intersect(body));
                if let Some(group) = &single_group {
                    self.floating_single_group_body_contents(
                        &mut body_ui,
                        window_id,
                        group,
                        header_rect,
                        &header_tabs,
                    );
                } else {
                    self.floating_dock_contents(&mut body_ui, window_id);
                }
            }
        }
        !header_close
    }

    /// Embedded fallback of a floating dock window. Returns `false` when the header's close was used.
    fn embedded_floating_panel(
        &mut self,
        ctx: &egui::Context,
        window_id: PanelId,
        panels: &[PanelId],
    ) -> bool {
        let id = egui::Id::new(("floating-dock", window_id));
        let collapsed_id = id.with("collapsed");
        let collapsed = ctx
            .data(|d| d.get_temp::<bool>(collapsed_id))
            .unwrap_or(false);
        let visible: Vec<PanelId> = panels
            .iter()
            .copied()
            .filter(|panel| self.workspace.is_panel_visible(*panel))
            .take(3)
            .collect();
        let titles: Vec<&str> = visible.iter().map(|panel| panel.title()).collect();
        // The first visible panel that is the active tab of its group in this window.
        let active = self
            .workspace
            .floating
            .iter()
            .find(|w| w.id == window_id)
            .and_then(|w| {
                visible
                    .iter()
                    .position(|panel| w.root.as_ref().is_some_and(|root| active_in(root, *panel)))
            })
            .unwrap_or(0);
        let mut open = true;
        let clipboard_target = self.clipboard_target(ctx);
        // The header replaces egui's title bar, so it moves the window itself.
        let position_id = id.with("position");
        let mut window = egui::Window::new(titles.join(" + ")).id(id);
        if let Some(position) = ctx.data(|d| d.get_temp::<egui::Pos2>(position_id)) {
            window = window.current_pos(position);
        }
        if !collapsed {
            let max_size = self
                .workspace
                .floating
                .iter()
                .find(|w| w.id == window_id)
                .and_then(|w| w.root.as_ref())
                .map_or([4096., 2160.], |root| root.maximum_size());
            window = window.max_width(max_size[0]).max_height(max_size[1]);
            if visible.as_slice() == [PanelId::Tools]
                && let Some(desired_h) = self.detached.tools_desired_inner_height
            {
                window = window.max_height(desired_h);
            }
        }
        window
            .title_bar(false)
            .frame(neoflash_ui::components::Surface::Floating.frame(ctx))
            .resizable(!collapsed)
            .show(ctx, |ui| {
                let header = neoflash_ui::components::FloatingPanelHeader {
                    tabs: &titles,
                    active,
                    collapsed,
                }
                .show(ui);
                if let Some(tab) = header.tab.and_then(|tab| visible.get(tab)) {
                    self.show_panel(*tab);
                }
                if header.drag.dragged() {
                    let delta = header.drag.drag_delta();
                    if let Some(rect) = ctx.memory(|m| m.area_rect(id)) {
                        ctx.data_mut(|d| d.insert_temp(position_id, rect.min + delta));
                        ctx.request_repaint();
                    }
                }
                neoflash_ui::components::popup_below(&header.menu, 240., |ui| {
                    self.floating_panel_commands(ui, window_id, &clipboard_target);
                });
                if header.collapse {
                    ctx.data_mut(|d| d.insert_temp(collapsed_id, !collapsed));
                }
                if header.close {
                    open = false;
                }
                if !collapsed {
                    self.floating_dock_contents(ui, window_id);
                }
            });
        open
    }

    fn in_floating_resize_margin(
        ctx: &egui::Context,
        is_root: bool,
        panel: PanelId,
        pos: egui::Pos2,
    ) -> bool {
        if is_root {
            return false;
        }
        let rect = ctx.viewport_rect();
        let edge = 5.0;
        let horizontal = pos.x <= rect.left() + edge || pos.x >= rect.right() - edge;
        let vertical = panel != PanelId::Tools
            && (pos.y <= rect.top() + edge || pos.y >= rect.bottom() - edge);
        horizontal || vertical
    }

    pub(crate) fn floating_window_resize_borders(
        &mut self,
        ctx: &egui::Context,
        window_id: PanelId,
        panels: &[PanelId],
        minimum: [f32; 2],
        maximum: [f32; 2],
        zoom: f32,
    ) {
        let viewport_rect = ctx.viewport_rect();
        let border_painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new(("floating-window-border", window_id)),
        ));
        border_painter.rect_stroke(
            viewport_rect,
            0.0_f32,
            egui::Stroke::new(1.0_f32, neoflash_ui::palette(ctx).border_subtle),
            egui::StrokeKind::Inside,
        );
        if self.detached.drag.is_some() {
            return;
        }
        let is_tools_only = panels == [PanelId::Tools];
        if let Some(resize) = self
            .detached
            .resize
            .filter(|resize| resize.window_id == window_id)
        {
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.workspace
                    .remember_floating_geometry(window_id, resize.start_geometry);
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
                    resize.start_geometry.outer_position[0] / zoom,
                    resize.start_geometry.outer_position[1] / zoom,
                )));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                    resize.start_geometry.inner_size[0] / zoom,
                    resize.start_geometry.inner_size[1] / zoom,
                )));
                self.detached.resize = None;
                return;
            }
            let inner = ctx
                .input(|i| i.viewport().inner_rect)
                .unwrap_or(viewport_rect);
            let (local_pointer, local_released) = ctx.input(|i| {
                let mut ptr = i.pointer.interact_pos();
                let mut rel = false;
                for event in &i.events {
                    match event {
                        egui::Event::PointerMoved(p) => ptr = Some(*p),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed: false,
                            ..
                        } => {
                            ptr = Some(*pos);
                            rel = true;
                            break;
                        }
                        _ => {}
                    }
                }
                (ptr.map(|p| (inner.min + p.to_vec2()) * zoom), rel)
            });
            let (pointer, down) = if let Some((pos, down)) = crate::native_screens::pointer_state()
            {
                (pos, down && !local_released)
            } else {
                (
                    local_pointer.unwrap_or(resize.start_pointer),
                    !local_released && ctx.input(|i| i.pointer.primary_down()),
                )
            };
            let delta = pointer - resize.start_pointer;
            let mut left_px = resize.start_geometry.outer_position[0];
            let mut top_px = resize.start_geometry.outer_position[1];
            let right_px = left_px + resize.start_geometry.inner_size[0];
            let bottom_px = top_px + resize.start_geometry.inner_size[1];
            let mut width_px = resize.start_geometry.inner_size[0];
            let mut height_px = resize.start_geometry.inner_size[1];
            let max_w = maximum[0].max(minimum[0]);
            let max_h = maximum[1].max(minimum[1]);
            if resize.left {
                width_px = (right_px - (left_px + delta.x)).clamp(minimum[0], max_w);
                left_px = right_px - width_px;
            } else if resize.right {
                width_px = (width_px + delta.x).clamp(minimum[0], max_w);
            }
            if !is_tools_only {
                if resize.top {
                    height_px = (bottom_px - (top_px + delta.y)).clamp(minimum[1], max_h);
                    top_px = bottom_px - height_px;
                } else if resize.bottom {
                    height_px = (height_px + delta.y).clamp(minimum[1], max_h);
                }
            } else if let Some(desired_h) = self.detached.tools_desired_inner_height {
                height_px = (desired_h * zoom).clamp(minimum[1], max_h);
            }
            let geometry = WindowGeometry {
                outer_position: [left_px, top_px],
                inner_size: [width_px, height_px],
            };
            self.workspace
                .remember_floating_geometry(window_id, geometry);
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
                left_px / zoom,
                top_px / zoom,
            )));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                width_px / zoom,
                height_px / zoom,
            )));
            ctx.set_cursor_icon(resize_cursor(
                resize.left,
                resize.right,
                resize.top,
                resize.bottom,
            ));
            if !down {
                self.detached.resize = None;
            } else {
                ctx.request_repaint_after(Duration::from_millis(16));
            }
            return;
        }
        let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) else {
            return;
        };
        let edge = 5.0;
        let corner = 10.0;
        let near_l = pos.x <= viewport_rect.left() + edge;
        let near_r = pos.x >= viewport_rect.right() - edge;
        let near_t = !is_tools_only && pos.y <= viewport_rect.top() + edge;
        let near_b = !is_tools_only && pos.y >= viewport_rect.bottom() - edge;
        if !(near_l || near_r || near_t || near_b) {
            return;
        }
        let left = near_l || ((near_t || near_b) && pos.x <= viewport_rect.left() + corner);
        let right = near_r || ((near_t || near_b) && pos.x >= viewport_rect.right() - corner);
        let top = near_t
            || (!is_tools_only && (near_l || near_r) && pos.y <= viewport_rect.top() + corner);
        let bottom = near_b
            || (!is_tools_only && (near_l || near_r) && pos.y >= viewport_rect.bottom() - corner);
        ctx.set_cursor_icon(resize_cursor(left, right, top, bottom));
        let pressed = ctx.input(|i| {
            i.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::PointerButton {
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        ..
                    }
                )
            })
        });
        if pressed
            && let Some(start_geometry) = self
                .workspace
                .floating
                .iter()
                .find(|w| w.id == window_id)
                .and_then(|w| w.geometry)
        {
            if is_tools_only {
                self.workspace.set_tools_stage_anchor(window_id, None);
            }
            let inner = ctx
                .input(|i| i.viewport().inner_rect)
                .unwrap_or(viewport_rect);
            let start_pointer = crate::native_screens::pointer_state()
                .map(|(p, _)| p)
                .unwrap_or_else(|| (inner.min + pos.to_vec2()) * zoom);
            self.detached.resize = Some(FloatingResize {
                window_id,
                left,
                right,
                top,
                bottom,
                start_pointer,
                start_geometry,
            });
        }
    }

    pub(crate) fn compact_floating_tools_window_bottom_up(
        &mut self,
        ctx: &egui::Context,
        window_id: PanelId,
        desired_inner_h: f32,
        min_w_px: f32,
        min_h_px: f32,
        zoom: f32,
    ) {
        let max_size_logical = self
            .workspace
            .floating
            .iter()
            .find(|w| w.id == window_id)
            .and_then(|w| w.root.as_ref())
            .map_or(
                crate::workspace::maximum_window_size(PanelId::Tools),
                |root| root.maximum_size(),
            );
        let max_w_px = (max_size_logical[0] * zoom).max(min_w_px);
        let max_h_px = (max_size_logical[1] * zoom).max(min_h_px);
        let Some(mut geom) = self
            .workspace
            .floating
            .iter()
            .find(|w| w.id == window_id)
            .and_then(|w| w.geometry)
        else {
            return;
        };
        if self.detached.resize.is_none()
            && let Some((outer, inner)) = ctx.input(|i| {
                let v = i.viewport();
                Some((v.outer_rect?, v.inner_rect?))
            })
        {
            geom.outer_position = [outer.min.x * zoom, outer.min.y * zoom];
            geom.inner_size = [
                (inner.width() * zoom).clamp(min_w_px, max_w_px),
                inner.height() * zoom,
            ];
        } else {
            geom.inner_size[0] = geom.inner_size[0].clamp(min_w_px, max_w_px);
        }
        let target_h_px = (desired_inner_h * zoom).clamp(min_h_px, max_h_px);
        let target_h_logical = target_h_px / zoom;
        if (geom.inner_size[1] - target_h_px).abs() <= 1.5 {
            self.workspace.remember_floating_geometry(window_id, geom);
            return;
        }
        let pointer_down = crate::native_screens::pointer_state().is_some_and(|(_, down)| down)
            || ctx.input(|i| i.pointer.any_down());
        if pointer_down || self.detached.resize.is_some() {
            // Defer sending programmatic window size/position changes while a live native window
            // or border resize drag is in progress so macOS AppKit and egui never desync or flicker.
            ctx.request_repaint_after(Duration::from_millis(16));
            return;
        }
        // Keep the edge facing away from the Stage fixed: top anchors (and unanchored windows)
        // compact bottom-up keeping `outer_position[1]` (the top edge) fixed; bottom anchors
        // compact upward keeping the bottom edge fixed.
        let anchor = self
            .workspace
            .floating
            .iter()
            .find(|window| window.id == window_id)
            .and_then(|window| window.tools_anchor);
        let old_bottom = geom.outer_position[1] + geom.inner_size[1];
        geom.inner_size[1] = target_h_px;
        if anchor.is_some_and(ToolsStageAnchor::bottom_aligned) {
            geom.outer_position[1] = old_bottom - target_h_px;
        }
        self.workspace.remember_floating_geometry(window_id, geom);
        ctx.send_viewport_cmd(egui::ViewportCommand::MaxInnerSize(egui::vec2(
            max_w_px / zoom,
            target_h_logical,
        )));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
            geom.inner_size[0] / zoom,
            target_h_logical,
        )));
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
            geom.outer_position[0] / zoom,
            geom.outer_position[1] / zoom,
        )));
        ctx.request_repaint();
    }

    fn finish_floating_context_menu(&mut self, ctx: &egui::Context) {
        if !self.detached.context_menu_open
            && self.clipboard.menu_viewport == Some(ctx.viewport_id())
        {
            self.clipboard.menu_open = false;
            self.clipboard.menu_target = None;
            self.clipboard.menu_viewport = None;
        }
    }
}

fn resize_cursor(left: bool, right: bool, top: bool, bottom: bool) -> egui::CursorIcon {
    match (left, right, top, bottom) {
        (true, false, true, false) | (false, true, false, true) => egui::CursorIcon::ResizeNwSe,
        (false, true, true, false) | (true, false, false, true) => egui::CursorIcon::ResizeNeSw,
        (true, false, false, false) | (false, true, false, false) => {
            egui::CursorIcon::ResizeHorizontal
        }
        _ => egui::CursorIcon::ResizeVertical,
    }
}

/// `true` when `panel` is the active tab of its group in a floating window tree.
fn active_in(node: &crate::workspace::FloatingNode, panel: PanelId) -> bool {
    match node {
        crate::workspace::FloatingNode::Group(group) => group.active == panel,
        crate::workspace::FloatingNode::Split { first, second, .. } => {
            active_in(first, panel) || active_in(second, panel)
        }
    }
}

/// NF2 `Floating Panel Header` look for a lone panel title: drag grip + active `Panel Tab`.
/// The whole chip is the drag handle (same docking gesture as dock tabs).
fn panel_title_tab(ui: &mut egui::Ui, label: &str) -> egui::Response {
    use neoflash_ui::tokens::{TextStyle, space};
    let p = neoflash_ui::palette(ui.ctx());
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        neoflash_ui::typography::font(TextStyle::SmallStrong),
        p.text_primary,
    );
    let grip = neoflash_ui::metrics::ICON_UI;
    let size = egui::vec2(
        space::S2 + grip + space::S1 + space::S3 + galley.size().x + space::S3,
        neoflash_ui::metrics::panel_tab(),
    );
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    if ui.is_rect_visible(rect) {
        let grip_rect = egui::Rect::from_center_size(
            egui::pos2(rect.left() + space::S2 + grip / 2., rect.center().y),
            egui::vec2(grip, grip),
        );
        neoflash_ui::icons::paint(
            ui.painter(),
            ui.ctx(),
            grip_rect,
            neoflash_ui::icons::i16::GRIP,
            p.text_tertiary,
        );
        let tab = egui::Rect::from_min_max(
            egui::pos2(grip_rect.right() + space::S1, rect.top()),
            rect.max,
        );
        ui.painter().galley(
            egui::pos2(
                tab.left() + space::S3,
                tab.center().y - galley.size().y / 2.,
            ),
            galley,
            p.text_primary,
        );
        ui.painter().rect_filled(
            egui::Rect::from_min_max(egui::pos2(tab.left(), tab.bottom() - 2.), tab.max),
            1.,
            p.accent_default,
        );
        if neoflash_ui::theme::keyboard_focus(ui, &response) {
            neoflash_ui::theme::focus_ring(ui, rect.shrink(2.), neoflash_ui::tokens::radius::SM);
        }
    }
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), label));
    response
}
