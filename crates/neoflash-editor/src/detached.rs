//! Native panels share one Editor. Only visibility and geometry are preferences.
use crate::{
    app::Editor,
    clipboard::ClipboardCommand,
    dock_grid::{DockGroupTarget, DockViewportTarget, edge_targets, surface_viewport},
    native_screens::available_work_areas,
    workspace::{
        DockDestination, DockSurface, PanelId, PanelPlacement, ScreenRect, WindowGeometry,
        recover_geometry,
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
}

/// A header gesture keeps its source viewport until release. This avoids
/// transferring a pressed mouse button between native windows mid-drag.
#[derive(Clone, Copy)]
struct PanelDrag {
    panel: PanelId,
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
    pub(crate) fn panel_title(&mut self, ui: &mut egui::Ui, panel: PanelId, label: &str) {
        if self.detached.content_header == Some(panel) {
            return;
        }
        let response = ui.add(
            egui::Label::new(egui::RichText::new(label).strong().size(12.))
                .sense(egui::Sense::click_and_drag()),
        );
        self.panel_drag_handle(ui, panel, &response);
    }

    pub(crate) fn panel_drag_handle(
        &mut self,
        ui: &egui::Ui,
        panel: PanelId,
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
        if self.symbol_conversion.is_some()
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
        if let Some(hit) = self.dock_hit(drag.pointer, ctx.zoom_factor()) {
            self.dock_panel_at(ctx, drag.panel, hit.destination);
        } else {
            let grouped = self
                .workspace
                .floating_window(drag.panel)
                .is_some_and(|window| window.panels().len() > 1);
            let mut geometry = if grouped {
                None
            } else {
                self.workspace.detached_geometry(drag.panel)
            }
            .unwrap_or(WindowGeometry {
                outer_position: [0., 0.],
                inner_size: crate::workspace::default_window_size(drag.panel),
            });
            let position =
                drag.pointer - drag.grab_offset - egui::vec2(0., 28. + 26. * ctx.zoom_factor());
            geometry.outer_position = [position.x, position.y];
            let window = self.workspace.float_panel(drag.panel, Some(geometry));
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
        let accent = ctx.style().visuals.selection.bg_fill;
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new(("panel-dock-target", surface)),
        ));
        for (edge, global) in edge_targets(view.content, view.zoom) {
            let rect = local(global);
            painter.rect_filled(rect, 3., accent.gamma_multiply(0.12));
            painter.rect_stroke(
                rect.shrink(1.),
                3.,
                egui::Stroke::new(1.0_f32, accent),
                egui::StrokeKind::Inside,
            );
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                edge.title(),
                egui::FontId::proportional(12.),
                ctx.style().visuals.text_color(),
            );
        }
        if let Some(hit) = self
            .dock_hit(drag.pointer, view.zoom)
            .filter(|hit| hit.surface == surface)
        {
            let rect = local(hit.rect);
            painter.rect_filled(rect, 3., accent.gamma_multiply(0.4));
            painter.rect_stroke(
                rect.shrink(1.),
                3.,
                egui::Stroke::new(2.0_f32, accent),
                egui::StrokeKind::Inside,
            );
            let pointer = (drag.pointer - view.inner.min).to_pos2() / view.zoom;
            painter.text(
                pointer + egui::vec2(12., 24.),
                egui::Align2::LEFT_TOP,
                format!("{} · {}", title(drag.panel), hit.caption),
                egui::FontId::proportional(13.),
                ctx.style().visuals.text_color(),
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
                desired.inner_size[1].max(minimum[1] + 26. * zoom),
            ];
            let geometry =
                recover_geometry(panel, Some(desired), &self.detached.monitors, fallback);
            let recovering = topology_changed && saved != Some(geometry);
            let growing = saved.is_some_and(|saved| saved.inner_size != geometry.inner_size);
            let title = format!(
                "{} — NeoFlash",
                panels
                    .iter()
                    .map(|panel| panel.title())
                    .collect::<Vec<_>>()
                    .join(" + ")
            );
            let mut builder = egui::ViewportBuilder::default()
                .with_title(&title)
                .with_min_inner_size([
                    minimum[0].min(geometry.inner_size[0]) / zoom,
                    (minimum[1] + 26. * zoom).min(geometry.inner_size[1]) / zoom,
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
                if class == egui::ViewportClass::Embedded {
                    let mut open = true;
                    egui::Window::new(&title)
                        .id(egui::Id::new(("floating-dock", window_id)))
                        .open(&mut open)
                        .show(ctx, |ui| {
                            self.floating_header(ui, window_id);
                            self.floating_dock_contents(ui, window_id);
                        });
                    if !open {
                        for panel in &panels {
                            self.set_panel_visible(*panel, false);
                        }
                    }
                    self.continue_panel_drag(ctx);
                    self.docking_overlay(ctx, DockSurface::Floating(window_id));
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
                egui::TopBottomPanel::top(egui::Id::new(("floating-toolbar", window_id)))
                    .exact_height(26.)
                    .show(ctx, |ui| self.floating_header(ui, window_id));
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

    fn floating_header(&mut self, ui: &mut egui::Ui, window: PanelId) {
        if self.symbol_conversion.is_some() {
            ui.disable();
        }
        let ctx = ui.ctx().clone();
        let target = self.clipboard_target(&ctx);
        let mut edit_open = false;
        ui.horizontal(|ui| {
            ui.menu_button("Edit", |ui| {
                edit_open = true;
                if !self.clipboard.menu_open
                    || self.clipboard.menu_viewport != Some(ctx.viewport_id())
                {
                    self.clipboard.menu_target = Some(target.clone());
                }
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
                    if ui
                        .add_enabled(enabled, egui::Button::new(name).shortcut_text(shortcut))
                        .clicked()
                    {
                        self.request_clipboard(&ctx, command);
                        ui.close();
                    }
                }
            });
            if ui
                .button("Dock all")
                .on_hover_text("Return all panels in this window to their last main-window groups")
                .clicked()
            {
                let panels = self
                    .workspace
                    .floating
                    .iter()
                    .find(|w| w.id == window)
                    .map(|w| w.panels())
                    .unwrap_or_default();
                for panel in panels {
                    self.set_panel_placement(&ctx, panel, PanelPlacement::Docked);
                }
            }
        });
        if edit_open {
            self.clipboard.menu_open = true;
            self.clipboard.menu_viewport = Some(ctx.viewport_id());
        } else if self.clipboard.menu_viewport == Some(ctx.viewport_id()) {
            self.clipboard.menu_open = false;
            self.clipboard.menu_target = None;
            self.clipboard.menu_viewport = None;
        }
    }
}
