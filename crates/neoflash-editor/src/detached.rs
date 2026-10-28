//! Native panels share one Editor. Only visibility and geometry are preferences.
use crate::{
    app::Editor,
    clipboard::ClipboardCommand,
    dock_grid::{DockGroupTarget, edge_targets},
    native_screens::available_work_areas,
    workspace::{
        DockDestination, PanelId, PanelPlacement, ScreenRect, WindowGeometry, minimum_window_size,
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
        let source = ctx.viewport_id();
        let zoom = ctx.zoom_factor();
        let inner = ctx
            .input(|i| i.viewport().inner_rect)
            .unwrap_or_else(|| ctx.viewport_rect());
        let to_global = |pos: egui::Pos2| (inner.min + pos.to_vec2()) * zoom;
        if source == egui::ViewportId::ROOT {
            self.detached.root_inner = Some(inner * zoom);
        }
        let (press, release, pointer, down, escape) = ctx.input(|i| {
            let mut press = None;
            let mut release = None;
            for event in &i.events {
                if let egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    ..
                } = event
                {
                    if *pressed {
                        press = Some(*pos);
                    } else {
                        release = Some(*pos);
                    }
                }
            }
            (
                press,
                release,
                i.pointer.latest_pos(),
                i.pointer.primary_down(),
                i.key_pressed(egui::Key::Escape),
            )
        });
        if escape {
            self.detached.drag = None;
        }
        if let Some(pos) = press.filter(|pos| {
            response.rect.contains(*pos)
                && ui.clip_rect().contains(*pos)
                && ctx.layer_id_at(*pos) == Some(ui.layer_id())
        }) {
            let global = to_global(pos);
            self.detached.drag = Some(PanelDrag {
                panel,
                source,
                started: global,
                pointer: global,
                grab_offset: (pos - response.rect.min) * zoom,
                moved: false,
            });
        }
        if let Some(mut drag) = self
            .detached
            .drag
            .filter(|drag| drag.panel == panel && drag.source == source)
        {
            if let Some(pos) = release.or(pointer) {
                drag.pointer = to_global(pos);
                drag.moved |= drag.pointer.distance(drag.started) >= 8. * zoom;
            }
            self.detached.drag = Some(drag);
            if release.is_some() {
                self.detached.drag = None;
                if drag.moved {
                    self.finish_panel_drag(ctx, drag);
                }
            } else if !down && pointer.is_some() {
                self.detached.drag = None;
            } else if drag.moved {
                ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
                ctx.request_repaint_of(egui::ViewportId::ROOT);
                if let Some(pos) = pointer {
                    let painter = ctx.layer_painter(egui::LayerId::new(
                        egui::Order::Tooltip,
                        egui::Id::new("panel-drag-caption"),
                    ));
                    painter.text(
                        pos + egui::vec2(14., 18.),
                        egui::Align2::LEFT_TOP,
                        format!("{} · release to place", title(panel)),
                        egui::FontId::proportional(12.),
                        ui.visuals().text_color(),
                    );
                }
            }
        }
        response.clone().on_hover_cursor(egui::CursorIcon::Grab).on_hover_text("Drag to any edge, group or tab in the main window. Drop outside a dock to detach. Escape cancels.");
    }

    fn finish_panel_drag(&mut self, ctx: &egui::Context, drag: PanelDrag) {
        if let Some(hit) = self.dock_hit(drag.pointer, ctx.zoom_factor()) {
            self.dock_panel_at(ctx, drag.panel, hit.destination);
        } else {
            let mut geometry =
                self.workspace
                    .detached_geometry(drag.panel)
                    .unwrap_or(WindowGeometry {
                        outer_position: [0., 0.],
                        inner_size: crate::workspace::default_window_size(drag.panel),
                    });
            let position = drag.pointer - drag.grab_offset - egui::vec2(0., 28.);
            geometry.outer_position = [position.x, position.y];
            self.workspace.remember_geometry(drag.panel, geometry);
            if self.workspace.is_panel_docked(drag.panel) {
                self.set_panel_placement(ctx, drag.panel, PanelPlacement::Detached);
            } else {
                ctx.send_viewport_cmd_to(
                    viewport(drag.panel),
                    egui::ViewportCommand::OuterPosition(position / ctx.zoom_factor()),
                );
            }
        }
        ctx.request_repaint_of(egui::ViewportId::ROOT);
    }

    /// A release can belong to the destination window instead of the source.
    /// Use its actual screen location; AppKit closes the remaining gap when the
    /// pointer ends over the desktop and neither egui viewport gets a release.
    pub(crate) fn continue_panel_drag(&mut self, ctx: &egui::Context) {
        let Some(mut drag) = self.detached.drag else {
            return;
        };
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.detached.drag = None;
            return;
        }
        let zoom = ctx.zoom_factor();
        let released = ctx.input(|i| {
            i.events.iter().rev().find_map(|event| match event {
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    ..
                } => i
                    .viewport()
                    .inner_rect
                    .map(|inner| (inner.min + pos.to_vec2()) * zoom),
                _ => None,
            })
        });
        let state = released
            .map(|pos| (pos, false))
            .or_else(crate::native_screens::pointer_state);
        if let Some((position, down)) = state {
            drag.pointer = position;
            drag.moved |= drag.pointer.distance(drag.started) >= 8. * zoom;
            if !down {
                self.detached.drag = None;
                if drag.moved {
                    self.finish_panel_drag(ctx, drag);
                }
                return;
            }
            self.detached.drag = Some(drag);
        }
        ctx.request_repaint_after(Duration::from_millis(16));
    }

    fn docking_overlay(&self, ctx: &egui::Context) {
        let Some(drag) = self.detached.drag.filter(|drag| drag.moved) else {
            return;
        };
        let Some(root) = self.detached.root_inner else {
            return;
        };
        let zoom = ctx.zoom_factor();
        let local = |rect: egui::Rect| rect.translate(-root.min.to_vec2()) / zoom;
        let accent = ctx.style().visuals.selection.bg_fill;
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("panel-dock-target"),
        ));
        for (edge, global) in edge_targets(root, zoom) {
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
        if let Some(hit) = self.dock_hit(drag.pointer, zoom) {
            let rect = local(hit.rect);
            painter.rect_filled(rect, 3., accent.gamma_multiply(0.4));
            painter.rect_stroke(
                rect.shrink(1.),
                3.,
                egui::Stroke::new(2.0_f32, accent),
                egui::StrokeKind::Inside,
            );
            let pointer = (drag.pointer - root.min).to_pos2() / zoom;
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
        self.detached.active.remove(&panel);
        if self.detached.focus == Some(panel) {
            self.detached.focus = None;
        }
        self.detached.focus_docked_actions = panel == PanelId::Actions;
        ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Focus);
        ctx.request_repaint_of(egui::ViewportId::ROOT);
    }

    pub(crate) fn set_panel_placement(
        &mut self,
        ctx: &egui::Context,
        panel: PanelId,
        placement: PanelPlacement,
    ) {
        if !panel.detachable() {
            return;
        }
        self.workspace.active_bottom = self.bottom;
        self.workspace.set_placement(panel, placement);
        self.bottom = self.workspace.active_bottom;
        if placement == PanelPlacement::Detached {
            self.detached.focus = Some(panel);
        } else {
            self.detached.active.remove(&panel);
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
        self.detached.root_inner = Some(
            ctx.input(|i| i.viewport().inner_rect)
                .unwrap_or_else(|| ctx.viewport_rect())
                * zoom,
        );
        self.continue_panel_drag(ctx);
        let root_rect = ctx.viewport_rect();
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
        self.detached.active.retain(|panel| {
            self.workspace.is_panel_visible(*panel) && !self.workspace.is_panel_docked(*panel)
        });
        for panel in PanelId::DETACHABLE {
            if !self.workspace.is_panel_visible(panel) || self.workspace.is_panel_docked(panel) {
                continue;
            }
            let initial = !self.detached.active.contains(&panel);
            let saved = self.workspace.detached_geometry(panel);
            let geometry = recover_geometry(panel, saved, &self.detached.monitors, fallback);
            let id = viewport(panel);
            let recovering = topology_changed && saved != Some(geometry);
            let mut builder = egui::ViewportBuilder::default()
                .with_title(format!("{} — NeoFlash", title(panel)))
                .with_min_inner_size({
                    let min = minimum_window_size(panel);
                    [
                        min[0].min(geometry.inner_size[0]) / zoom,
                        min[1].min(geometry.inner_size[1]) / zoom,
                    ]
                });
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
                self.workspace.remember_geometry(panel, geometry);
            } else if recovering {
                ctx.send_viewport_cmd_to(
                    id,
                    egui::ViewportCommand::OuterPosition(egui::pos2(
                        geometry.outer_position[0] / zoom,
                        geometry.outer_position[1] / zoom,
                    )),
                );
                ctx.send_viewport_cmd_to(
                    id,
                    egui::ViewportCommand::InnerSize(egui::vec2(
                        geometry.inner_size[0] / zoom,
                        geometry.inner_size[1] / zoom,
                    )),
                );
                self.workspace.remember_geometry(panel, geometry);
            }
            self.detached.active.insert(panel);
            let focus = self.detached.focus == Some(panel);
            if focus {
                self.detached.focus = None;
            }
            ctx.show_viewport_immediate(id, builder, |ctx, class| {
                if class == egui::ViewportClass::Embedded {
                    // Headless/unsupported integrations have an explicit fallback.
                    // Native eframe on macOS uses the OS viewport branch below.
                    let mut open = true;
                    egui::Window::new(format!("{} — NeoFlash", title(panel)))
                        .id(egui::Id::new(panel.viewport_key()))
                        .open(&mut open)
                        .show(ctx, |ui| {
                            self.detached_header(ui, panel);
                            self.detached_contents(ui, panel);
                        });
                    if !open {
                        self.set_panel_visible(panel, false);
                    }
                    return;
                }
                if !recovering
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
                    self.workspace.remember_geometry(panel, geometry);
                }
                if ctx.input(|i| i.viewport().close_requested()) {
                    self.set_panel_visible(panel, false);
                    ctx.request_repaint_of(egui::ViewportId::ROOT);
                    return;
                }
                if focus {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    if panel == PanelId::Actions {
                        ctx.memory_mut(|memory| memory.request_focus(self.actions_editor_id()));
                    }
                }
                let has_input = ctx.input(|i| !i.events.is_empty());
                self.shortcuts(ctx);
                egui::TopBottomPanel::top(egui::Id::new(("detached-header", panel)))
                    .show(ctx, |ui| self.detached_header(ui, panel));
                if self.workspace.is_panel_visible(panel) && !self.workspace.is_panel_docked(panel)
                {
                    egui::CentralPanel::default().show(ctx, |ui| self.detached_contents(ui, panel));
                }
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
        if !self.detached.active.is_empty() {
            ctx.request_repaint_after(Duration::from_secs(1));
        }
        self.docking_overlay(ctx);
    }

    fn detached_contents(&mut self, ui: &mut egui::Ui, panel: PanelId) {
        self.dock_panel_contents(ui, panel);
    }

    fn detached_header(&mut self, ui: &mut egui::Ui, panel: PanelId) {
        if self.symbol_conversion.is_some() {
            ui.disable();
        }
        let ctx = ui.ctx().clone();
        let target = self.clipboard_target(&ctx);
        let mut edit_open = false;
        ui.horizontal_wrapped(|ui| {
            self.panel_title(ui, panel, title(panel));
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
                for (name, command) in [
                    ("Cut    ⌘X", ClipboardCommand::Cut),
                    ("Copy    ⌘C", ClipboardCommand::Copy),
                    ("Paste    ⌘V", ClipboardCommand::Paste),
                ] {
                    let enabled = text
                        || if command == ClipboardCommand::Paste {
                            self.layer_editable()
                        } else {
                            !self.selection.is_empty()
                        };
                    if ui.add_enabled(enabled, egui::Button::new(name)).clicked() {
                        self.request_clipboard(&ctx, command);
                        ui.close();
                    }
                }
            });
            if ui
                .button("Dock")
                .on_hover_text("Return this panel to the main window")
                .clicked()
            {
                self.set_panel_placement(&ctx, panel, PanelPlacement::Docked);
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
