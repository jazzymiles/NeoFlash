//! NF2 window chrome around the dock grid (§4 of the design reference):
//! top bar title / actions, status bar, the stage's floating breadcrumb, size badge and zoom control,
//! and the `NEOFLASH_SCREENSHOT` capture used to compare the native UI with the Figma exports.
//! Everything here is built from `neoflash_ui` components; no colours or sizes are defined locally.
use crate::app::{Editor, Tool};
use eframe::egui::{self, Rect, Vec2};
use neoflash_ui::components::{self as nf, surface::ZoomAction};
use neoflash_ui::icons::i16;
use neoflash_ui::tokens::{TextStyle, space};
use neoflash_ui::{metrics, palette, typography};

impl Editor {
    /// Centred project title and `Unsaved` badge. Hidden when the bar is too narrow to keep the
    /// menus and actions readable.
    pub(crate) fn top_bar_title(&self, ui: &mut egui::Ui, bar: Rect) {
        if bar.width() < 1180. {
            return;
        }
        let p = palette(ui.ctx());
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| self.project.name.clone());
        // A child Ui does not move the parent's cursor, so the menu bar still starts at the left edge.
        let badge = if self.dirty() { 90. } else { 0. };
        let text_w = ui.fonts_mut(|f| {
            f.layout_no_wrap(
                name.clone(),
                typography::font(TextStyle::BodyStrong),
                p.text_primary,
            )
            .size()
            .x
        });
        let width = text_w + badge;
        let row = Rect::from_center_size(bar.center(), Vec2::new(width, metrics::TOP_BAR));
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(row)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        child.label(typography::rich(&name, TextStyle::BodyStrong).color(p.text_primary));
        if self.dirty() {
            child.add(nf::Badge::new("Unsaved", nf::BadgeType::Warning));
        }
    }

    /// Right side of the top bar: workspace select, Test Movie (secondary), Publish (primary).
    pub(crate) fn top_bar_actions(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = space::S2;
            if ui
                .add(
                    nf::Button::primary("Publish")
                        .icon(i16::UPLOAD)
                        .shortcut("⇧⌥F12"),
                )
                .clicked()
            {
                self.export(false);
            }
            if ui
                .add(nf::Button::new("Test Movie").icon(i16::PLAY).shortcut("⌘↵"))
                .clicked()
            {
                self.export(true);
            }
            // The menu bar takes the row's remaining width, so decide by window width.
            if ui.ctx().content_rect().width() >= 1280. {
                let current = self.workspace.template;
                let label = format!("{} workspace", current.label());
                nf::Select::new("workspace", &label)
                    .width(180.)
                    .show(ui, |ui| {
                        self.workspace_menu(ui, &ctx);
                    });
            }
        });
    }

    /// Workspace templates + reset; shared by the top-bar select and Window ▸ Workspace.
    pub(crate) fn workspace_menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        use crate::workspace::WorkspaceTemplate;
        let current = self.workspace.template;
        nf::menu_section(ui, "Workspaces");
        for template in WorkspaceTemplate::ALL {
            let item = ui
                .add(nf::MenuItem::new(template.label()).checked(template == current))
                .on_hover_text(template.description());
            if item.clicked() {
                self.apply_workspace_template(ctx, template);
            }
        }
        nf::menu_separator(ui);
        let reset = format!("Reset {} workspace", current.label());
        if ui.add(nf::MenuItem::new(&reset)).clicked() {
            self.restore_workspace(ctx);
        }
    }

    /// NF2 status bar (24 pt): state, document summary, shortcut chips.
    pub(crate) fn status_bar(&mut self, ctx: &egui::Context) {
        let p = palette(ctx);
        let frames = self.symbol().timeline.frame_count.max(1);
        let summary = format!(
            "Stage {} × {} · {} fps · {} frames",
            self.project.width, self.project.height, self.project.fps, frames
        );
        // The initial status embeds the shortcut hints; the bar shows them as chips instead.
        let message = self
            .status
            .split(" · F5")
            .next()
            .unwrap_or(&self.status)
            .to_owned();
        let ok =
            !message.to_lowercase().contains("error") && !message.to_lowercase().contains("fail");
        egui::TopBottomPanel::bottom("status-bar")
            .exact_height(metrics::STATUS_BAR)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(p.bg_app))
            .show(ctx, |ui| {
                let hints: &[(&str, &str)] = if ui.available_width() > 900. {
                    &[
                        ("F5", "frame"),
                        ("F6", "keyframe"),
                        ("F7", "blank keyframe"),
                        ("⌘↵", "Test Movie"),
                    ]
                } else {
                    &[]
                };
                nf::surface::status_bar(ui, ok, &message, &summary, hints);
            });
    }

    /// Floating chrome over the canvas: breadcrumb + drafting actions (top-left), stage size badge
    /// (top-right) and the zoom control (bottom centre).
    pub(crate) fn stage_chrome(&mut self, ui: &mut egui::Ui, canvas: Rect) {
        let ctx = ui.ctx().clone();
        // Floating chrome starts below / right of the rulers when they are shown.
        let ruler = if self.stage_view.prefs.rulers {
            neoflash_ui::components::canvas::RULER
        } else {
            0.
        };
        // Breadcrumb: Scene ▸ symbol ▸ nested symbol. Clicking an item returns to that level.
        egui::Area::new(egui::Id::new("stage-breadcrumb"))
            .order(egui::Order::Middle)
            .fixed_pos(canvas.min + Vec2::splat(space::S3 + ruler))
            .constrain_to(canvas)
            .show(&ctx, |ui| {
                nf::Surface::Floating
                    .frame(&ctx)
                    .corner_radius(neoflash_ui::tokens::radius::LG)
                    .inner_margin(egui::Margin::same(3))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.;
                            let path = self.editing.clone();
                            let visible_indices: Vec<usize> = if path.len() > 4 {
                                vec![0, path.len() - 2, path.len() - 1]
                            } else {
                                (0..path.len()).collect()
                            };
                            for (slot, &index) in visible_indices.iter().enumerate() {
                                if slot > 0 {
                                    nf::breadcrumb_separator(ui);
                                }
                                if path.len() > 4 && slot == 1 {
                                    let overflow = nf::breadcrumb_overflow(ui);
                                    nf::popup_below(&overflow, 200.0, |ui| {
                                        for hidden in 1..path.len() - 2 {
                                            let hidden_name = self
                                                .project
                                                .symbol(&path[hidden])
                                                .map(|s| s.name.clone())
                                                .unwrap_or_default();
                                            if ui.add(nf::MenuItem::new(&hidden_name)).clicked() {
                                                self.leave_to_level(hidden);
                                                ui.close();
                                            }
                                        }
                                    });
                                    nf::breadcrumb_separator(ui);
                                }
                                let id = &path[index];
                                let symbol = self.project.symbol(id);
                                let name = symbol.map(|s| s.name.clone()).unwrap_or_default();
                                let kind = match symbol.map(|s| s.kind) {
                                    _ if index == 0 => nf::BreadcrumbType::Scene,
                                    Some(neoflash_core::SymbolKind::Graphic) => nf::BreadcrumbType::Graphic,
                                    Some(neoflash_core::SymbolKind::Button) => nf::BreadcrumbType::Button,
                                    _ => nf::BreadcrumbType::MovieClip,
                                };
                                let current = index + 1 == path.len();
                                // §5.5: the current item's ▾ opens the Symbols & scenes popover.
                                let item = ui.add(nf::BreadcrumbItem { label: &name, kind, current, menu: current });
                                if item.clicked() {
                                    if current {
                                        self.toggle_symbols_popover(ui.ctx(), Some(item.rect));
                                    } else {
                                        self.close_symbols_popover(ui.ctx());
                                        self.leave_to_level(index);
                                    }
                                }
                            }
                            if self.onion {
                                ui.add_space(space::S2);
                                ui.add(nf::Badge::new("Onion skin", nf::BadgeType::Accent)).on_hover_text(
                                    "Tinted outlines or shapes show neighbouring frames. They are not content of the current frame; turn off Onion Skin to see this frame alone.",
                                );
                            }
                            if self.vector.draft.is_some() {
                                ui.add_space(space::S2);
                                ui.spacing_mut().item_spacing.x = space::S1;
                                if ui.add(nf::Button::primary("Finish").shortcut("Enter")).clicked() {
                                    self.finish_path(false);
                                }
                                if ui.add(nf::Button::new("Close path")).clicked() {
                                    self.finish_path(true);
                                }
                                if ui.add(nf::Button::ghost("Cancel").shortcut("Esc")).clicked() {
                                    self.vector.draft = None;
                                }
                            }
                        });
                    });
            });
        self.symbols_popover(&ctx, canvas);
        // Stage size badge.
        egui::Area::new(egui::Id::new("stage-size-badge"))
            .order(egui::Order::Middle)
            .pivot(egui::Align2::RIGHT_TOP)
            .fixed_pos(canvas.right_top() + Vec2::new(-space::S3, space::S3 + ruler))
            .interactable(false)
            .show(&ctx, |ui| {
                let label = format!("{} × {}", self.project.width, self.project.height);
                ui.add(nf::Badge::new(&label, nf::BadgeType::Neutral));
            });
        // Zoom control with the rulers / guides / grid / snap-to-guides toggles (same state as the
        // View menu) and ⚙, which opens the View settings popover anchored to it.
        if canvas.width() < 420. || canvas.height() < 160. {
            self.stage_view.settings_button = None;
            self.view_settings_popover(&ctx, canvas);
            return;
        }
        let v = &self.stage_view.prefs;
        let (action, settings) = nf::ZoomControl {
            zoom: self.zoom,
            hand: self.tool == Tool::Hand,
            rulers: Some(v.rulers),
            guides: Some(v.guides_show),
            grid: Some(v.grid_show),
            snap: Some(v.guides_snap),
        }
        .show(
            &ctx,
            egui::Id::new("stage-zoom-control"),
            canvas.center_bottom() - Vec2::new(0., space::S4),
        );
        self.stage_view.settings_button = settings.map(|r| r.rect);
        match action {
            ZoomAction::Hand => self.tool = Tool::Hand,
            ZoomAction::ZoomIn => self.zoom_in(),
            ZoomAction::ZoomOut => self.zoom_out(),
            ZoomAction::ZoomReset => self.zoom_actual(),
            ZoomAction::Fit => self.fit_stage(),
            ZoomAction::Rulers => self.toggle_rulers(),
            ZoomAction::Guides => self.toggle_guides(),
            ZoomAction::Grid => self.toggle_grid(),
            ZoomAction::Snap => self.toggle_guide_snap(),
            ZoomAction::ViewSettings => {
                self.stage_view.settings_open = !self.stage_view.settings_open;
            }
            ZoomAction::None => {}
        }
        self.view_settings_popover(&ctx, canvas);
    }

    pub(crate) fn zoom_in(&mut self) {
        self.finish_edits();
        self.zoom = (self.zoom * 1.25).min(4.);
    }
    pub(crate) fn zoom_out(&mut self) {
        self.finish_edits();
        self.zoom = (self.zoom / 1.25).max(0.1);
    }
    pub(crate) fn zoom_actual(&mut self) {
        self.finish_edits();
        self.zoom = 1.;
    }

    /// View shortcuts: ⌘= / ⌘− zoom, ⌘1 100 %, ⌘0 fit (skipped while typing).
    pub(crate) fn view_shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, Modifiers};
        if ctx.wants_keyboard_input() {
            return;
        }
        self.stage_view_shortcuts(ctx);
        let pressed = |key| ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, key));
        if pressed(Key::Equals) || pressed(Key::Plus) {
            self.zoom_in();
        }
        if pressed(Key::Minus) {
            self.zoom_out();
        }
        if pressed(Key::Num1) {
            self.zoom_actual();
        }
        if pressed(Key::Num0) {
            self.fit_stage();
        }
    }

    /// Return to editing level `index` of the symbol path (0 = scene).
    pub(crate) fn leave_to_level(&mut self, index: usize) {
        self.finish_edits();
        self.timeline_key_drag = None;
        self.editing.truncate(index + 1);
        self.reset_authored_preview();
        self.frame = 0;
        self.layer = 0;
        self.clear_selection();
    }

    /// Fit the stage in the canvas (zoom 0 = recompute fit on the next frame).
    pub(crate) fn fit_stage(&mut self) {
        self.end_vector_pointer_phase();
        self.cancel_selection_gesture();
        self.shape_draft = None;
        if self.vector.point_drag.is_some() {
            self.cancel_vector();
        }
        self.zoom = 0.;
        self.pan = Vec2::ZERO;
    }
}

/// `NEOFLASH_SCREENSHOT=path.png` saves a PNG of the main window after a few frames and quits.
/// Optional `NEOFLASH_SCREENSHOT_THEME=light|dark`. Used to compare against
/// `interface/design-system/exports/interface/{dark,light}/png/editor-main.png`.
pub(crate) fn capture(ctx: &egui::Context, frame_index: u64) {
    let Ok(path) = std::env::var("NEOFLASH_SCREENSHOT") else {
        return;
    };
    // macOS may restore a full-screen or zoomed window from the last session; leave it first so
    // `NEOFLASH_WINDOW` sizes are honoured, then give the window time to settle.
    if let Some(size) = fixed_window_size() {
        if frame_index == 1 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
        }
        if frame_index == 40 || frame_index == 80 {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                size[0], size[1],
            )));
        }
    }
    if frame_index == 120 {
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
    }
    let image = ctx.input(|i| {
        i.events.iter().find_map(|e| match e {
            egui::Event::Screenshot { image, .. } => Some(image.clone()),
            _ => None,
        })
    });
    if let Some(image) = image {
        let [w, h] = image.size;
        let bytes: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
        if let Some(dir) = std::path::Path::new(&path).parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(error) =
            image::save_buffer(&path, &bytes, w as u32, h as u32, image::ColorType::Rgba8)
        {
            eprintln!("NEOFLASH_SCREENSHOT: {error}");
        }
        std::process::exit(0);
    }
    ctx.request_repaint();
}

/// Window size forced by `NEOFLASH_WINDOW=WxH`, if any.
pub(crate) fn fixed_window_size() -> Option<[f32; 2]> {
    let value = std::env::var("NEOFLASH_WINDOW").ok()?;
    let (w, h) = value.split_once('x')?;
    Some([w.parse().ok()?, h.parse().ok()?])
}

/// Workspace template forced by `NEOFLASH_SCREENSHOT_WORKSPACE=classic|animator|designer|developer|essentials`.
pub(crate) fn screenshot_workspace() -> Option<crate::workspace::WorkspaceTemplate> {
    let name = std::env::var("NEOFLASH_SCREENSHOT_WORKSPACE")
        .ok()?
        .to_lowercase();
    crate::workspace::WorkspaceTemplate::ALL
        .into_iter()
        .find(|t| t.label().to_lowercase() == name)
}

/// Theme forced by `NEOFLASH_SCREENSHOT_THEME`, if any.
pub(crate) fn screenshot_theme() -> Option<crate::theme::ThemeMode> {
    match std::env::var("NEOFLASH_SCREENSHOT_THEME").ok()?.as_str() {
        "light" => Some(crate::theme::ThemeMode::Light),
        "dark" => Some(crate::theme::ThemeMode::Dark),
        _ => None,
    }
}
