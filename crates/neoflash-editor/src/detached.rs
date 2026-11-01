//! Native panels share one Editor. Only visibility and geometry are preferences.
use crate::{
    app::Editor,
    clipboard::{ClipboardCommand, ClipboardTarget},
    dock_grid::{DockGroupTarget, DockViewportTarget, edge_targets, surface_viewport},
    native_screens::available_work_areas,
    workspace::{
        DockDestination, DockEdge, DockSurface, PanelId, PanelPlacement, ScreenRect,
        WindowGeometry, recover_geometry,
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
    pub(crate) root_inner: Option<egui::Rect>,
    pub(crate) dock_groups: Vec<DockGroupTarget>,
    pub(crate) content_header: Option<PanelId>,
    pub(crate) dock_viewports: Vec<DockViewportTarget>,
    pub(crate) activation: u64,
    context_menu_open: bool,
}

/// A header gesture keeps its source viewport until release. This avoids
/// transferring a pressed mouse button between native windows mid-drag.
#[derive(Clone, Copy)]
struct PanelDrag {
    panel: PanelId,
    group: Option<(DockSurface, DockEdge, u32)>,
    source: egui::ViewportId,
    started: egui::Pos2,
    pointer: egui::Pos2,
    grab_offset: egui::Vec2,
    moved: bool,
    released: bool,
    began: Instant,
}

fn viewport(panel: PanelId) -> egui::ViewportId {
    egui::ViewportId::from_hash_of(panel.viewport_key().expect("detachable panel"))
}
fn title(panel: PanelId) -> &'static str {
    panel.title()
}

impl Editor {
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
        self.panel_drag_handle_inner(ui, panel, None, response);
    }

    pub(crate) fn panel_group_drag_handle(
        &mut self,
        ui: &egui::Ui,
        panel: PanelId,
        group: (DockSurface, DockEdge, u32),
        response: &egui::Response,
    ) {
        self.panel_drag_handle_inner(ui, panel, Some(group), response);
    }

    fn panel_drag_handle_inner(
        &mut self,
        ui: &egui::Ui,
        panel: PanelId,
        group: Option<(DockSurface, DockEdge, u32)>,
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
            && let Some(pos) = press.filter(|pos| {
                response.rect.contains(*pos)
                    && ui.clip_rect().contains(*pos)
                    && ctx.layer_id_at(*pos) == Some(ui.layer_id())
            })
        {
            let global = (inner.min + pos.to_vec2()) * zoom;
            self.detached.drag = Some(PanelDrag {
                panel,
                group,
                source,
                started: global,
                pointer: global,
                grab_offset: (pos - response.rect.min) * zoom,
                moved: false,
                released: false,
                began: Instant::now(),
            });
        }
        self.continue_panel_drag(ctx);
        if self.detached.drag.is_some_and(|drag| drag.moved) {
            ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
        }
        response.clone().on_hover_cursor(egui::CursorIcon::Grab).on_hover_text("Drag to an edge, group or tab in any NeoFlash window. Drop outside a dock to detach. Escape cancels.");
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
            self.detached.drag = None;
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
        if let Some((position, down)) = crate::native_screens::pointer_state() {
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
            return;
        }
        drag.moved |= drag.pointer.distance(drag.started) >= 8. * zoom;
        self.detached.drag = Some(drag);
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
        self.detached.drag = None;
        if !drag.moved {
            return;
        }
        let panels = drag
            .group
            .and_then(|(surface, edge, group)| {
                self.current_dock_group(surface, edge, group)
                    .map(|g| g.tabs.clone())
            })
            .unwrap_or_else(|| vec![drag.panel]);
        if let Some(hit) = self.dock_hit(drag.pointer, ctx.zoom_factor()) {
            if drag.group.is_some() {
                self.workspace
                    .dock_group(&panels, drag.panel, hit.destination);
                self.focus_dock_panel(ctx, drag.panel);
            } else {
                self.dock_panel_at(ctx, drag.panel, hit.destination);
            }
        } else {
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
                inner_size: crate::workspace::default_window_size(drag.panel),
            });
            let position = drag.pointer - drag.grab_offset - egui::vec2(0., 28.);
            geometry.outer_position = [position.x, position.y];
            let window = if drag.group.is_some() {
                self.workspace.float_group(&panels, drag.panel, geometry)
            } else {
                self.workspace.float_panel(drag.panel, Some(geometry))
            };
            self.workspace.remember_floating_geometry(window, geometry);
            self.bottom = self.workspace.active_bottom;
            self.detached.focus = Some(drag.panel);
            ctx.send_viewport_cmd_to(
                viewport(window),
                egui::ViewportCommand::OuterPosition(position / ctx.zoom_factor()),
            );
        }
        ctx.request_repaint_of(egui::ViewportId::ROOT);
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
        let local = |rect: egui::Rect| rect.translate(-view.inner.min.to_vec2()) / view.zoom;
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new(("panel-dock-target", surface)),
        ));
        let hit = self
            .dock_hit(drag.pointer, view.zoom)
            .filter(|hit| hit.surface == surface);
        for (edge, global) in edge_targets(view.content, view.zoom) {
            neoflash_ui::components::dock_target(&painter, ctx, local(global), edge.title(), false);
        }
        if let Some(hit) = hit {
            neoflash_ui::components::dock_target(
                &painter,
                ctx,
                local(hit.rect),
                &hit.caption,
                true,
            );
            let pointer = (drag.pointer - view.inner.min).to_pos2() / view.zoom;
            neoflash_ui::components::drag_ghost_at(
                &painter,
                ctx,
                pointer + egui::vec2(12., 24.),
                &format!("{} · {}", title(drag.panel), hit.caption),
            );
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

    fn focus_dock_panel(&mut self, ctx: &egui::Context, panel: PanelId) {
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
        let visible: Vec<_> = self
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
            let minimum = window
                .root
                .as_ref()
                .map_or([80., 80.], |root| root.minimum_size());
            let saved = window.geometry;
            let mut desired = saved.unwrap_or(WindowGeometry {
                outer_position: [fallback.origin[0] + 40., fallback.origin[1] + 40.],
                inner_size: crate::workspace::default_window_size(panel),
            });
            desired.inner_size = [
                desired.inner_size[0].max(minimum[0]),
                desired.inner_size[1].max(minimum[1]),
            ];
            let geometry =
                recover_geometry(panel, Some(desired), &self.detached.monitors, fallback);
            let recovering = topology_changed && saved != Some(geometry);
            let growing = saved.is_some_and(|saved| saved.inner_size != geometry.inner_size);
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
                .with_min_inner_size([
                    minimum[0].min(geometry.inner_size[0]) / zoom,
                    minimum[1].min(geometry.inner_size[1]) / zoom,
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
                if !recovering
                    && !growing
                    && let Some(geometry) = ctx.input(|i| {
                        let v = i.viewport();
                        if v.minimized == Some(true) {
                            return None;
                        }
                        let outer = v.outer_rect?;
                        let inner = v.inner_rect?;
                        Some(WindowGeometry {
                            outer_position: [outer.min.x * zoom, outer.min.y * zoom],
                            inner_size: [inner.width() * zoom, inner.height() * zoom],
                        })
                    })
                {
                    self.workspace
                        .remember_floating_geometry(window_id, geometry);
                }
                if ctx.input(|i| i.viewport().close_requested()) {
                    for panel in &panels {
                        self.set_panel_visible(*panel, false);
                    }
                    if self.detached.drag.is_some_and(|drag| drag.source == id) {
                        self.detached.drag = None;
                    }
                    self.detached
                        .dock_viewports
                        .retain(|v| v.surface != DockSurface::Floating(window_id));
                    self.detached
                        .dock_groups
                        .retain(|g| g.surface != DockSurface::Floating(window_id));
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
                    egui::CentralPanel::default()
                        .frame(egui::Frame::central_panel(&ctx.style()).inner_margin(0.))
                        .show(ctx, |ui| self.floating_dock_contents(ui, window_id));
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
        for (name, shortcut, command) in [
            ("Cut", "⌘X", ClipboardCommand::Cut),
            ("Copy", "⌘C", ClipboardCommand::Copy),
            ("Paste", "⌘V", ClipboardCommand::Paste),
        ] {
            let enabled = text
                || if command == ClipboardCommand::Paste {
                    self.layer_editable()
                } else {
                    !self.selection.is_empty()
                };
            if crate::ui_icons::menu_item_enabled(ui, enabled, name, shortcut).clicked() {
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
        neoflash_ui::metrics::PANEL_TAB,
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
