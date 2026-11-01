//! Application menu bar inside the NF2 top bar (design §8: File · Edit · View · Insert · Modify ·
//! Control · Window · Help) and the table of natural entry points that open planned previews.
//!
//! Every item follows one rule: a command the editor supports runs the existing editor method; a
//! command whose NF2 panel or dialog is only a read-only preview opens that preview; anything else is
//! shown disabled with "Not available yet". Labels, order, separators and shortcuts follow §8.
use crate::app::*;
use crate::clipboard::ClipboardCommand;
use crate::panels::planned::{self, PlannedPanel};
use crate::theme::DesignUi as _;
use crate::theme::ThemeMode;
use crate::ui_icons::{menu_item, menu_item_enabled, menu_popup, menu_toggle};
use crate::workspace::{PanelId, PanelPlacement};
use eframe::egui;
use neoflash_core::*;
use neoflash_ui::components::{self as nf, MenuItem};

/// Tooltip of every designed command the editor cannot perform yet.
pub(crate) const NOT_AVAILABLE: &str = "Not available yet";

use PlannedPanel as P;

/// Natural entry points (design §4–§9) that open a planned preview, by location. Menu rows use the last
/// path segment as their label. `GraphicProperties` has no entry: graphic looping is a working
/// Properties section (`instance_playback_ui`), so its preview stays under Window ▸ Planned panels.
pub(crate) const PLANNED_ENTRY_POINTS: &[(&str, PlannedPanel)] = &[
    // Menu bar (§8).
    ("File ▸ Import ▸ Import video…", P::ImportVideo),
    ("File ▸ Export ▸ Export image…", P::ExportImage),
    ("File ▸ Export ▸ Export video…", P::ExportVideo),
    ("File ▸ Export ▸ Export audio…", P::ExportAudio),
    ("File ▸ Publish settings…", P::PublishSettings),
    ("Edit ▸ Find and replace", P::FindReplace),
    ("Edit ▸ Preferences…", P::Preferences),
    ("Edit ▸ Keyboard shortcuts…", P::Preferences),
    ("Modify ▸ Timeline ▸ Layer properties…", P::LayerProperties),
    ("Control ▸ Test scene", P::TestMovie),
    ("Window ▸ Compiler errors", P::CompilerErrors),
    // §6 panels the §8 Window list does not name.
    ("Window ▸ Debugger", P::Debugger),
    ("Window ▸ Code snippets", P::CodeSnippets),
    ("Window ▸ Movie Explorer", P::MovieExplorer),
    ("Window ▸ Filters", P::PropertiesFilters),
    // Context menus (§8).
    ("Layer menu ▸ Properties…", P::LayerProperties),
    // Panel menus (§6).
    ("Color fill type ▸ Linear gradient", P::ColorGradient),
    ("Color fill type ▸ Radial gradient", P::ColorGradient),
    ("Color fill type ▸ Bitmap fill", P::BitmapFill),
    ("Library menu ▸ Sound properties…", P::SoundProperties),
    ("Library menu ▸ Edit envelope…", P::EditEnvelope),
    ("Library menu ▸ Font embedding…", P::FontEmbedding),
    // Buttons and popovers (§4–§6).
    ("Breadcrumb ▸ Symbols & scenes", P::SymbolsPopover),
    ("Properties ▸ Publish settings…", P::PublishSettings),
    ("Swatches popover ▸ Gradient", P::ColorGradient),
    (
        "Tool options ▸ Rectangle primitive…",
        P::PropertiesRectPrimitive,
    ),
    ("Tool options ▸ Oval primitive…", P::PropertiesOvalPrimitive),
    ("Tool options ▸ PolyStar settings…", P::PolyStarSettings),
    ("Tool options ▸ Stroke style…", P::StrokeStyle),
    ("Tool options ▸ Spray brush…", P::SprayBrush),
    ("Tool options ▸ Deco tool…", P::DecoTool),
    ("Tool options ▸ Bone tool…", P::BoneProperties),
];

/// The preview registered for an entry point.
pub(crate) fn planned_entry(path: &str) -> Option<PlannedPanel> {
    let panel = PLANNED_ENTRY_POINTS
        .iter()
        .find(|(entry, _)| *entry == path)
        .map(|(_, panel)| *panel);
    debug_assert!(panel.is_some(), "unknown planned entry point {path:?}");
    panel
}

/// Menu label of an entry point: its last path segment.
fn entry_label(path: &str) -> &str {
    path.rsplit(" ▸ ").next().unwrap_or(path)
}

/// Real panels in Window, in §8 order, with their shortcuts.
const WINDOW_PANELS: [(&str, &str, PanelId); 17] = [
    ("Timeline", "⌥⌘T", PanelId::Timeline),
    ("Tools", "⌘F2", PanelId::Tools),
    ("Properties", "⌘F3", PanelId::Properties),
    ("Library", "⌘L", PanelId::Library),
    ("Actions", "F9", PanelId::Actions),
    ("Align", "⌘K", PanelId::Align),
    ("Color", "⇧⌘F9", PanelId::Color),
    ("Swatches", "⌘F9", PanelId::Swatches),
    ("Transform", "⌘T", PanelId::Transform),
    ("Info", "⌘I", PanelId::Info),
    ("Ease", "", PanelId::Ease),
    ("History", "⌘F10", PanelId::History),
    ("Scene", "⇧F2", PanelId::Scene),
    ("Output", "F2", PanelId::Output),
    ("Accessibility", "⇧F11", PanelId::Accessibility),
    ("Motion editor", "", PanelId::MotionEditor),
    ("Project", "", PanelId::Project),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConvertedTrigger {
    DocumentSettings,
    AddSwatch,
}

fn converted_triggers_id() -> egui::Id {
    egui::Id::new("nf2-converted-entry-triggers")
}

impl Editor {
    /// Open the preview registered for `path` (buttons, shortcuts and menus share this).
    /// Entry points whose panel/dialog has become real route through a frame trigger so call sites
    /// that only hold `&egui::Context` still open the real feature.
    pub(crate) fn open_planned(ctx: &egui::Context, path: &str) {
        let converted = match path {
            "Modify ▸ Document…" => Some(ConvertedTrigger::DocumentSettings),
            "Color menu ▸ Add swatch" | "Swatches popover ▸ Add swatch" => {
                Some(ConvertedTrigger::AddSwatch)
            }
            _ => None,
        };
        if let Some(trigger) = converted {
            let id = converted_triggers_id();
            ctx.data_mut(|d| {
                let mut queue = d.get_temp::<Vec<ConvertedTrigger>>(id).unwrap_or_default();
                queue.push(trigger);
                d.insert_temp(id, queue);
            });
            return;
        }
        if let Some(panel) = planned_entry(path) {
            planned::set_open(ctx, panel, true);
        }
    }

    /// Execute any converted entry-point triggers queued during the frame.
    pub(crate) fn drain_converted_entry_triggers(&mut self, ctx: &egui::Context) {
        let id = converted_triggers_id();
        let triggers = ctx
            .data_mut(|d| d.remove_temp::<Vec<ConvertedTrigger>>(id))
            .unwrap_or_default();
        for trigger in triggers {
            match trigger {
                ConvertedTrigger::DocumentSettings => self.open_document_settings(),
                ConvertedTrigger::AddSwatch => {
                    self.add_active_swatch(ctx);
                    self.show_panel(PanelId::Swatches);
                }
            }
        }
    }

    pub(crate) fn undo_menu_label(&self) -> String {
        if self.vector.draft.as_ref().is_some_and(|d| !d.anchors.is_empty()) {
            return "Undo".to_owned();
        }
        match self.history.undo_label() {
            Some(step) if !step.is_empty() => format!("Undo {step}"),
            _ => "Undo".to_owned(),
        }
    }

    pub(crate) fn redo_menu_label(&self) -> String {
        match self.history.redo_label() {
            Some(step) if !step.is_empty() => format!("Redo {step}"),
            _ => "Redo".to_owned(),
        }
    }

    /// Show or hide the preview registered for `path` (Window shortcuts).
    pub(crate) fn toggle_planned(ctx: &egui::Context, path: &str) {
        if let Some(panel) = planned_entry(path) {
            planned::set_open(ctx, panel, !planned::is_open(ctx, panel));
        }
    }

    /// Menu row that opens the preview registered for `path`.
    pub(crate) fn planned_menu_item(ui: &mut egui::Ui, path: &str, shortcut: &str) {
        if menu_item(ui, entry_label(path), shortcut).clicked() {
            Self::open_planned(ui.ctx(), path);
            ui.close();
        }
    }

    /// Window-menu toggle for a preview panel (checked while the preview is open).
    fn planned_menu_toggle(ui: &mut egui::Ui, path: &str, shortcut: &str) {
        let Some(panel) = planned_entry(path) else {
            return;
        };
        let mut open = planned::is_open(ui.ctx(), panel);
        if menu_toggle(ui, &mut open, entry_label(path), shortcut).changed() {
            planned::set_open(ui.ctx(), panel, open);
            ui.close();
        }
    }

    /// Checked menu row for a view setting; true when clicked (the caller toggles the setting).
    fn view_toggle(ui: &mut egui::Ui, on: bool, label: &str, shortcut: &str) -> bool {
        let mut value = on;
        let clicked = menu_toggle(ui, &mut value, label, shortcut).changed();
        if clicked {
            ui.close();
        }
        clicked
    }

    /// Disabled row for a designed command the editor cannot perform yet. Its shortcut does nothing.
    pub(crate) fn unavailable_item(ui: &mut egui::Ui, label: &str, shortcut: &str) {
        menu_item_enabled(ui, false, label, shortcut).on_disabled_hover_text(NOT_AVAILABLE);
    }

    /// Disabled submenu row (▸) whose commands are all unavailable.
    pub(crate) fn unavailable_submenu(ui: &mut egui::Ui, label: &str) {
        ui.add_enabled(false, MenuItem::new(label).submenu(true))
            .on_disabled_hover_text(NOT_AVAILABLE);
    }

    /// Symbol of the single selected instance, if any (Edit in place, Show in library).
    pub(crate) fn selected_instance_symbol(&self) -> Option<String> {
        match self.selected_element().map(|e| &e.kind) {
            Some(ElementKind::Instance { symbol_id, .. }) if self.selection.len() == 1 => {
                Some(symbol_id.clone())
            }
            _ => None,
        }
    }

    /// Control ▸ Rewind / Go to end / step: move the playhead without playing.
    pub(crate) fn seek_frame(&mut self, frame: u32) {
        self.finish_edits();
        self.timeline_key_drag = None;
        self.reset_authored_preview();
        self.frame = frame.min(self.symbol().timeline.frame_count.saturating_sub(1));
    }

    /// Edit ▸ Edit symbols (⌘E): enter the selected symbol, or return to the scene.
    pub(crate) fn toggle_edit_symbols(&mut self) {
        if let Some(id) = self.selected_instance_symbol() {
            self.enter_symbol(id);
        } else if self.editing.len() > 1 {
            self.leave_to_level(0);
        }
    }

    pub fn menu(&mut self, ctx: &egui::Context) {
        let clipboard_target = self.clipboard_target(ctx);
        let mut edit_open = false;
        // NF2 top bar (48 pt): logo · menu bar · centred title + Unsaved badge · workspace · Test Movie · Publish.
        let nf = neoflash_ui::palette(ctx);
        egui::TopBottomPanel::top("menu")
            .exact_height(neoflash_ui::metrics::TOP_BAR)
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(nf.bg_app)
                    .inner_margin(egui::Margin::symmetric(12, 0)),
            )
            .show(ctx, |ui| {
                let bar = ui.max_rect();
                ui.painter().hline(
                    bar.x_range(),
                    bar.bottom() - 0.5,
                    egui::Stroke::new(1.0_f32, nf.border_subtle),
                );
                self.top_bar_title(ui, bar);
                ui.horizontal_centered(|ui| {
                    // Figma: logo, 4 pt, product name (Title), then the menu titles.
                    ui.spacing_mut().item_spacing.x = 4.;
                    neoflash_ui::components::logo(ui, 28.);
                    ui.label(
                        neoflash_ui::typography::rich("NeoFlash", neoflash_ui::TextStyle::Title)
                            .color(nf.text_primary),
                    );
                    ui.add_space(12.);
                    neoflash_ui::components::menu_bar(ui, |ui| {
                        menu_popup(ui, "File", "Publish settings…", "⇧F12", |ui| {
                            self.file_menu(ui, ctx);
                        });
                        let undo_w = self.undo_menu_label();
                        let redo_w = self.redo_menu_label();
                        let edit_widest = ["Keyboard shortcuts…", undo_w.as_str(), redo_w.as_str()]
                            .into_iter()
                            .max_by_key(|s| s.len())
                            .unwrap_or("Keyboard shortcuts…");
                        menu_popup(ui, "Edit", edit_widest, "⌥⇧⌘K", |ui| {
                            edit_open = true;
                            if !self.clipboard.menu_open
                                || self.clipboard.menu_viewport != Some(ctx.viewport_id())
                            {
                                self.clipboard.menu_target = Some(clipboard_target.clone());
                            }
                            self.edit_menu(ui, ctx);
                        });
                        menu_popup(ui, "View", "Rulers, guides & grid…", "⌥⇧⌘R", |ui| {
                            self.view_menu(ui);
                        });
                        menu_popup(ui, "Insert", "Blank keyframe", "⌘F8", |ui| {
                            self.insert_menu(ui);
                        });
                        self.modify_menu_ui(ui);
                        menu_popup(ui, "Control", "Enable simple buttons", "⌥⌘B", |ui| {
                            self.control_menu(ui);
                        });
                        menu_popup(ui, "Window", "Compiler errors", "⇧⌘F9", |ui| {
                            self.window_menu(ui, ctx);
                        });
                        menu_popup(ui, "Help", "About NeoFlash", "", |ui| {
                            if menu_item(ui, "Open example", "").clicked() {
                                self.request(Pending::Demo, ctx);
                                ui.close();
                            }
                            nf::menu_separator(ui);
                            if menu_item(ui, "About NeoFlash", "").clicked() {
                                self.show_about = true;
                                ui.close();
                            }
                        });
                    });
                    self.top_bar_actions(ui);
                });
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

    /// File (§8).
    fn file_menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if menu_item(ui, "New", "⌘N").clicked() {
            self.request(Pending::New, ctx);
            ui.close();
        }
        if menu_item(ui, "Open…", "⌘O").clicked() {
            self.request(Pending::Open, ctx);
            ui.close();
        }
        Self::unavailable_submenu(ui, "Open recent");
        Self::unavailable_item(ui, "Close", "⌘W");
        nf::menu_separator(ui);
        if menu_item(ui, "Save", "⌘S").clicked() {
            self.save(false);
            ui.close();
        }
        if menu_item(ui, "Save as…", "⇧⌘S").clicked() {
            self.save(true);
            ui.close();
        }
        Self::unavailable_item(ui, "Revert", "");
        nf::menu_separator(ui);
        menu_popup(ui, "Import", "Import graphics to Stage…", "", |ui| {
            if menu_item(ui, "Import graphics to Stage…", "").clicked() {
                self.import_graphics_dialog();
                ui.close();
            }
            if menu_item(ui, "Import legacy Flash…", "").clicked() {
                self.request(Pending::Import, ctx);
                ui.close();
            }
            Self::planned_menu_item(ui, "File ▸ Import ▸ Import video…", "");
            if menu_item_enabled(ui, self.layer_editable(), "Video placeholder", "").clicked() {
                self.finish_edits();
                self.add_element(
                    ElementKind::Video {
                        width: 320.,
                        height: 240.,
                    },
                    80.,
                    80.,
                );
                ui.close();
            }
        });
        menu_popup(ui, "Export", "Export image…", "", |ui| {
            Self::planned_menu_item(ui, "File ▸ Export ▸ Export image…", "");
            Self::planned_menu_item(ui, "File ▸ Export ▸ Export video…", "");
            Self::planned_menu_item(ui, "File ▸ Export ▸ Export audio…", "");
        });
        nf::menu_separator(ui);
        Self::planned_menu_item(ui, "File ▸ Publish settings…", "⇧F12");
        if menu_item(ui, "Publish", "⇧⌥F12")
            .on_hover_text("Choose an output folder and publish the project as HTML5")
            .clicked()
        {
            self.export(false);
            ui.close();
        }
        nf::menu_separator(ui);
        Self::unavailable_item(ui, "Print…", "⌘P");
    }

    /// Edit (§8) with the Edit ▸ Timeline submenu.
    fn edit_menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let undo_label = self.undo_menu_label();
        let redo_label = self.redo_menu_label();
        if menu_item_enabled(
            ui,
            self.history.can_undo() || self.vector.draft.is_some(),
            &undo_label,
            "⌘Z",
        )
        .clicked()
        {
            self.undo();
            ui.close();
        }
        if menu_item_enabled(ui, self.history.can_redo(), &redo_label, "⇧⌘Z").clicked() {
            self.redo();
            ui.close();
        }
        nf::menu_separator(ui);
        let text_target = self
            .clipboard
            .menu_target
            .as_ref()
            .is_some_and(|target| target.is_text());
        let can_copy = text_target || !self.selection.is_empty();
        let can_paste = text_target || self.layer_editable();
        for (label, shortcut, command, enabled) in [
            ("Cut", "⌘X", ClipboardCommand::Cut, can_copy),
            ("Copy", "⌘C", ClipboardCommand::Copy, can_copy),
            ("Paste in centre", "⌘V", ClipboardCommand::Paste, can_paste),
            (
                "Paste in place",
                "⇧⌘V",
                ClipboardCommand::PasteInPlace,
                !text_target && self.layer_editable(),
            ),
        ] {
            if menu_item_enabled(ui, enabled, label, shortcut).clicked() {
                self.request_clipboard(ctx, command);
                ui.close();
            }
        }
        if menu_item_enabled(ui, !self.selection.is_empty(), "Clear", "⌫").clicked() {
            self.delete_selected();
            ui.close();
        }
        nf::menu_separator(ui);
        if menu_item_enabled(ui, !self.selection.is_empty(), "Duplicate", "⌘D").clicked() {
            self.duplicate();
            ui.close();
        }
        if menu_item(ui, "Select all", "⌘A").clicked() {
            self.select_all();
            ui.close();
        }
        if menu_item_enabled(ui, !self.selection.is_empty(), "Deselect", "⇧⌘A").clicked() {
            self.finish_edits();
            self.clear_selection();
            ui.close();
        }
        nf::menu_separator(ui);
        Self::planned_menu_item(ui, "Edit ▸ Find and replace", "⌘F");
        menu_popup(ui, "Timeline", "Synchronise symbols", "⌥⌘X", |ui| {
            self.edit_timeline_menu(ui);
        });
        nf::menu_separator(ui);
        let instance = self.selected_instance_symbol();
        if menu_item_enabled(
            ui,
            instance.is_some() || self.editing.len() > 1,
            "Edit symbols",
            "⌘E",
        )
        .clicked()
        {
            self.toggle_edit_symbols();
            ui.close();
        }
        for label in ["Edit selected", "Edit in place"] {
            if menu_item_enabled(ui, instance.is_some(), label, "").clicked() {
                if let Some(id) = instance.clone() {
                    self.enter_symbol(id);
                }
                ui.close();
            }
        }
        nf::menu_separator(ui);
        Self::planned_menu_item(ui, "Edit ▸ Preferences…", "⌘U");
        Self::planned_menu_item(ui, "Edit ▸ Keyboard shortcuts…", "⌥⇧⌘K");
    }

    /// Edit ▸ Timeline (§8).
    fn edit_timeline_menu(&mut self, ui: &mut egui::Ui) {
        use crate::timeline_edit::FrameCommand as F;
        self.frame_command_item(ui, "Cut frames", "⌥⌘X", F::Cut);
        self.frame_command_item(ui, "Copy frames", "⌥⌘C", F::Copy);
        self.frame_command_item(ui, "Paste frames", "⌥⌘V", F::Paste);
        self.frame_command_item(ui, "Clear frames", "⌥⌫", F::Clear);
        self.frame_command_item(ui, "Remove frames", "⇧F5", F::Remove);
        nf::menu_separator(ui);
        self.frame_command_item(ui, "Select all frames", "⌥⌘A", F::SelectAll);
        if menu_item_enabled(ui, self.can_reverse_frames(), "Reverse frames", "").clicked() {
            self.reverse_frames();
            ui.close();
        }
        if menu_item_enabled(ui, self.can_sync_symbols(), "Synchronise symbols", "").clicked() {
            self.sync_symbols();
            ui.close();
        }
    }

    /// View (§8) with Magnification, Preview mode, Grid, Guides and Snapping submenus.
    fn view_menu(&mut self, ui: &mut egui::Ui) {
        if menu_item(ui, "Zoom in", "⌘=").clicked() {
            self.zoom_in();
            ui.close();
        }
        if menu_item(ui, "Zoom out", "⌘−").clicked() {
            self.zoom_out();
            ui.close();
        }
        menu_popup(ui, "Magnification", "Fit in window", "⌘0", |ui| {
            for (label, zoom) in [("25 %", 0.25), ("50 %", 0.5)] {
                if menu_item(ui, label, "").clicked() {
                    self.finish_edits();
                    self.zoom = zoom;
                    ui.close();
                }
            }
            if menu_item(ui, "100 %", "⌘1").clicked() {
                self.zoom_actual();
                ui.close();
            }
            for (label, zoom) in [("200 %", 2.), ("400 %", 4.)] {
                if menu_item(ui, label, "").clicked() {
                    self.finish_edits();
                    self.zoom = zoom;
                    ui.close();
                }
            }
            nf::menu_separator(ui);
            if menu_item(ui, "Fit in window", "⌘0").clicked() {
                self.fit_stage();
                ui.close();
            }
        });
        nf::menu_separator(ui);
        Self::unavailable_submenu(ui, "Preview mode");
        // The pasteboard is always shown; hiding it is not available yet.
        ui.add_enabled(
            false,
            MenuItem::new("Pasteboard").shortcut("⇧⌘W").checked(true),
        )
        .on_disabled_hover_text(NOT_AVAILABLE);
        nf::menu_separator(ui);
        // Rulers, Grid and Guides toggle the same view settings as the zoom control and the View
        // settings popover (stage_view.rs); the "…" items open that popover.
        let v = self.stage_view.prefs.clone();
        if Self::view_toggle(ui, v.rulers, "Rulers", "⌥⇧⌘R") {
            self.toggle_rulers();
        }
        menu_popup(ui, "Grid", "Show over objects", "⇧⌘'", |ui| {
            if Self::view_toggle(ui, v.grid_show, "Show grid", "⌘'") {
                self.toggle_grid();
            }
            if Self::view_toggle(ui, v.grid_snap, "Snap to grid", "⇧⌘'") {
                self.toggle_grid_snap();
            }
            if Self::view_toggle(ui, v.grid_over_objects, "Show over objects", "") {
                self.toggle_grid_over_objects();
            }
            nf::menu_separator(ui);
            if menu_item(ui, "Edit grid…", "⌥⌘G").clicked() {
                self.open_view_settings();
                ui.close();
            }
        });
        menu_popup(ui, "Guides", "Snap to guides", "⇧⌘;", |ui| {
            if Self::view_toggle(ui, v.guides_show, "Show guides", "⌘;") {
                self.toggle_guides();
            }
            if Self::view_toggle(ui, v.guides_lock, "Lock guides", "") {
                self.toggle_guide_lock();
            }
            if Self::view_toggle(ui, v.guides_snap, "Snap to guides", "⇧⌘;") {
                self.toggle_guide_snap();
            }
            nf::menu_separator(ui);
            if menu_item(ui, "Edit guides…", "").clicked() {
                self.open_view_settings();
                ui.close();
            }
        });
        Self::unavailable_submenu(ui, "Snapping");
        nf::menu_separator(ui);
        if menu_item(ui, "Rulers, guides & grid…", "").clicked() {
            self.open_view_settings();
            ui.close();
        }
        Self::unavailable_item(ui, "Show shape hints", "⌥⌘H");
    }

    /// Insert (§8).
    fn insert_menu(&mut self, ui: &mut egui::Ui) {
        if menu_item(ui, "New symbol…", "⌘F8").clicked() {
            self.begin_new_symbol();
            ui.close();
        }
        nf::menu_separator(ui);
        if menu_item(ui, "Layer", "").clicked() {
            self.insert_timeline_layer();
            ui.close();
        }
        if menu_item(ui, "Layer folder", "").clicked() {
            self.insert_layer_folder(self.layer);
            ui.close();
        }
        if menu_item_enabled(
            ui,
            self.can_add_motion_guide_to(self.layer),
            "Motion guide",
            "",
        )
        .clicked()
        {
            self.add_motion_guide();
            ui.close();
        }
        nf::menu_separator(ui);
        let editable = self.layer_editable();
        if menu_item_enabled(ui, editable, "Frame", "F5").clicked() {
            self.finish_edits();
            self.insert_frame();
            ui.close();
        }
        if menu_item_enabled(ui, editable, "Keyframe", "F6").clicked() {
            self.finish_edits();
            self.make_key(false);
            ui.close();
        }
        if menu_item_enabled(ui, editable, "Blank keyframe", "F7").clicked() {
            self.finish_edits();
            self.make_key(true);
            ui.close();
        }
        nf::menu_separator(ui);
        let tweenable = self.can_create_tween();
        for (label, kind) in [
            ("Classic tween", TweenKind::Classic),
            ("Shape tween", TweenKind::Shape),
            ("Motion tween", TweenKind::Motion),
        ] {
            if menu_item_enabled(
                ui,
                tweenable && self.current_tween() != Some(kind),
                label,
                "",
            )
            .clicked()
            {
                self.create_tween(kind);
                ui.close();
            }
        }
        nf::menu_separator(ui);
        if menu_item(ui, "Scene", "").clicked() {
            self.new_scene();
            ui.close();
        }
    }

    /// Control (§8).
    fn control_menu(&mut self, ui: &mut egui::Ui) {
        if menu_item(ui, if self.playing { "Stop" } else { "Play" }, "↵").clicked() {
            self.set_playing(!self.playing);
            ui.close();
        }
        let last = self.symbol().timeline.frame_count.saturating_sub(1);
        for (label, shortcut, frame) in [
            ("Rewind", "⌥⇧R", 0),
            ("Go to end", "⌥⇧.", last),
            ("Step forward one frame", ".", (self.frame + 1).min(last)),
            ("Step back one frame", ",", self.frame.saturating_sub(1)),
        ] {
            if menu_item(ui, label, shortcut).clicked() {
                self.seek_frame(frame);
                ui.close();
            }
        }
        nf::menu_separator(ui);
        if menu_item(ui, "Test movie", "⌘↵").clicked() {
            self.export(true);
            ui.close();
        }
        Self::planned_menu_item(ui, "Control ▸ Test scene", "⌥⌘↵");
        nf::menu_separator(ui);
        menu_toggle(
            ui,
            &mut self.workspace.timeline.loop_playback,
            "Loop playback",
            "",
        );
        menu_toggle(
            ui,
            &mut self.timeline_edit.play_all_scenes,
            "Play all scenes",
            "",
        );
        let mut muted = self.timeline_audio.muted;
        if menu_toggle(ui, &mut muted, "Mute sounds", "⌥⌘M").changed() {
            self.toggle_mute_sounds();
        }
        nf::menu_separator(ui);
        Self::unavailable_item(ui, "Enable simple buttons", "⌥⌘B");
    }

    /// Window (§8), then the §6 panels §8 does not list, then workspace placement and appearance.
    fn window_menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        Self::unavailable_item(ui, "Duplicate window", "⌥⌘K");
        nf::menu_separator(ui);
        let toggle = |editor: &mut Self, ui: &mut egui::Ui, index: usize| {
            let (name, shortcut, panel) = WINDOW_PANELS[index];
            let mut visible = editor.workspace.is_panel_visible(panel);
            if menu_toggle(ui, &mut visible, name, shortcut).changed() {
                editor.set_panel_visible(panel, visible);
                ui.close();
            }
        };
        // Timeline · Tools · Properties · Library · Actions · Align · Color · Swatches ·
        // Transform · Info · Ease · History · Scene · Output.
        for index in 0..14 {
            toggle(self, ui, index);
        }
        Self::planned_menu_toggle(ui, "Window ▸ Compiler errors", "⌥F2");
        toggle(self, ui, 14);
        nf::menu_separator(ui);
        toggle(self, ui, 15);
        Self::planned_menu_toggle(ui, "Window ▸ Debugger", "");
        toggle(self, ui, 16);
        Self::planned_menu_toggle(ui, "Window ▸ Code snippets", "");
        Self::planned_menu_toggle(ui, "Window ▸ Movie Explorer", "");
        Self::planned_menu_toggle(ui, "Window ▸ Filters", "");
        nf::menu_separator(ui);
        menu_popup(ui, "Workspaces", "Reset Developer workspace", "", |ui| {
            self.workspace_menu(ui, ctx);
        });
        Self::unavailable_item(ui, "Hide panels", "F4");
        nf::menu_separator(ui);
        for (label, placement, target) in [
            (
                "Detach panel",
                PanelPlacement::Docked,
                PanelPlacement::Detached,
            ),
            (
                "Dock panel",
                PanelPlacement::Detached,
                PanelPlacement::Docked,
            ),
        ] {
            let any = WINDOW_PANELS
                .iter()
                .any(|(_, _, panel)| self.workspace.placement(*panel) == placement);
            ui.add_enabled_ui_semantic(any, |ui| {
                menu_popup(ui, label, "Accessibility", "", |ui| {
                    for (name, _, panel) in WINDOW_PANELS {
                        if self.workspace.placement(panel) == placement
                            && menu_item(ui, name, "").clicked()
                        {
                            self.set_panel_placement(ctx, panel, target);
                            ui.close();
                        }
                    }
                })
            });
        }
        menu_popup(ui, "Appearance", "Light", "", |ui| {
            for (name, theme) in [("Light", ThemeMode::Light), ("Dark", ThemeMode::Dark)] {
                let mut selected = self.workspace.theme == theme;
                if menu_toggle(ui, &mut selected, name, "").clicked() {
                    self.set_theme(ctx, theme);
                    ui.close();
                }
            }
        });
        menu_popup(
            ui,
            "Planned panels",
            "Properties — Rectangle primitive",
            "",
            |ui| {
                planned::menu_items(ui);
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Source files that draw an entry point (menus, context menus, buttons, popovers, shortcuts).
    const SOURCES: [&str; 12] = [
        include_str!("menu.rs"),
        include_str!("timeline.rs"),
        include_str!("timeline_controls.rs"),
        include_str!("tools.rs"),
        include_str!("library.rs"),
        include_str!("properties.rs"),
        include_str!("../shell.rs"),
        include_str!("../layer_controls.rs"),
        include_str!("../authoring_modify.rs"),
        include_str!("../stage.rs"),
        include_str!("../app.rs"),
        include_str!("../dock_grid.rs"),
    ];

    #[test]
    fn every_planned_panel_has_a_natural_entry_point() {
        for panel in PlannedPanel::ALL {
            if panel == PlannedPanel::GraphicProperties {
                // Working commands; see PLANNED_ENTRY_POINTS.
                continue;
            }
            assert!(
                PLANNED_ENTRY_POINTS
                    .iter()
                    .any(|(path, p)| *p == panel && !path.contains("Planned panels")),
                "{panel:?} is reachable only from Window ▸ Planned panels"
            );
        }
    }

    #[test]
    fn every_entry_point_is_drawn_by_the_ui() {
        let source = SOURCES.concat();
        for (path, panel) in PLANNED_ENTRY_POINTS {
            // Once in the table above, at least once where the UI opens it.
            let uses = source.matches(&format!("\"{path}\"")).count();
            assert!(uses >= 2, "entry point {path:?} ({panel:?}) is not drawn");
        }
        let mut seen = std::collections::HashSet::new();
        for (path, _) in PLANNED_ENTRY_POINTS {
            assert!(seen.insert(*path), "duplicate entry point {path:?}");
        }
    }

    #[test]
    fn entry_labels_are_the_last_path_segment() {
        assert_eq!(entry_label("View ▸ Grid ▸ Edit grid…"), "Edit grid…");
        assert_eq!(
            planned_entry("Edit ▸ Find and replace"),
            Some(PlannedPanel::FindReplace)
        );
    }
}
