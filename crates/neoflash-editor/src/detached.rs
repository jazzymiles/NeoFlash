//! Native panels share one Editor. Only visibility and geometry are preferences.
use crate::{
    app::Editor,
    clipboard::ClipboardCommand,
    native_screens::available_work_areas,
    workspace::{
        PanelId, PanelPlacement, ScreenRect, WindowGeometry, minimum_window_size, recover_geometry,
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
}

fn viewport(panel: PanelId) -> egui::ViewportId {
    egui::ViewportId::from_hash_of(panel.viewport_key().expect("detachable panel"))
}
fn title(panel: PanelId) -> &'static str {
    match panel {
        PanelId::Tools => "Tools",
        PanelId::Timeline => "Timeline",
        PanelId::Actions => "Actions",
        _ => unreachable!("only detachable panels have native windows"),
    }
}

impl Editor {
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
    }

    fn detached_contents(&mut self, ui: &mut egui::Ui, panel: PanelId) {
        match panel {
            PanelId::Tools => self.tools_contents(ui),
            PanelId::Timeline => self.timeline_contents(ui),
            PanelId::Actions => self.actions_contents(ui),
            _ => unreachable!(),
        }
    }

    fn detached_header(&mut self, ui: &mut egui::Ui, panel: PanelId) {
        let ctx = ui.ctx().clone();
        let target = self.clipboard_target(&ctx);
        let mut edit_open = false;
        ui.horizontal_wrapped(|ui| {
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
