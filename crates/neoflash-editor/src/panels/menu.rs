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
    // §6 panels the §8 Window list does not name.
    ("Window ▸ Debugger", P::Debugger),
    // Panel menus (§6).
    ("Color fill type ▸ Bitmap fill", P::BitmapFill),
    ("Library menu ▸ Font embedding…", P::FontEmbedding),
    // Buttons and popovers (§4–§6).
    (
        "Tool options ▸ Rectangle primitive…",
        P::PropertiesRectPrimitive,
    ),
    ("Tool options ▸ Oval primitive…", P::PropertiesOvalPrimitive),
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
const WINDOW_PANELS: [(&str, &str, PanelId); 21] = [
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
    ("Compiler errors", "⌥F2", PanelId::CompilerErrors),
    ("Accessibility", "⇧F11", PanelId::Accessibility),
    ("Motion editor", "", PanelId::MotionEditor),
    ("Project", "", PanelId::Project),
    ("Code snippets", "", PanelId::CodeSnippets),
    ("Movie Explorer", "", PanelId::MovieExplorer),
    ("Filters", "", PanelId::Filters),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConvertedTrigger {
    DocumentSettings,
    PublishSettings,
    ExportVideo,
    ExportAudio,
    /// Preferences opened on a page (`panels/preferences.rs`).
    Preferences(usize),
    AddSwatch,
    EditEnvelope,
    SoundProperties,
    LayerProperties,
    SymbolsPopover,
    TestScene,
    FindReplace,
    ShowPanel(PanelId),
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
            "File ▸ Publish settings…" | "Properties ▸ Publish settings…" => {
                Some(ConvertedTrigger::PublishSettings)
            }
            "File ▸ Export ▸ Export video…" => Some(ConvertedTrigger::ExportVideo),
            "File ▸ Export ▸ Export audio…" => Some(ConvertedTrigger::ExportAudio),
            "Edit ▸ Preferences…" => Some(ConvertedTrigger::Preferences(
                crate::panels::preferences::PAGE_GENERAL,
            )),
            "Edit ▸ Keyboard shortcuts…" => Some(ConvertedTrigger::Preferences(
                crate::panels::preferences::PAGE_SHORTCUTS,
            )),
            "Edit ▸ Find and replace" => Some(ConvertedTrigger::FindReplace),
            "Window ▸ Compiler errors" => {
                Some(ConvertedTrigger::ShowPanel(PanelId::CompilerErrors))
            }
            "Window ▸ Code snippets" => Some(ConvertedTrigger::ShowPanel(PanelId::CodeSnippets)),
            "Window ▸ Movie Explorer" => {
                Some(ConvertedTrigger::ShowPanel(PanelId::MovieExplorer))
            }
            "Window ▸ Filters" => Some(ConvertedTrigger::ShowPanel(PanelId::Filters)),
            "Color menu ▸ Add swatch" | "Swatches popover ▸ Add swatch" => {
                Some(ConvertedTrigger::AddSwatch)
            }
            "Library menu ▸ Edit envelope…" => Some(ConvertedTrigger::EditEnvelope),
            "Library menu ▸ Sound properties…" => Some(ConvertedTrigger::SoundProperties),
            "Modify ▸ Timeline ▸ Layer properties…" | "Layer menu ▸ Properties…" => {
                Some(ConvertedTrigger::LayerProperties)
            }
            "Breadcrumb ▸ Symbols & scenes" => Some(ConvertedTrigger::SymbolsPopover),
            "Control ▸ Test scene" => Some(ConvertedTrigger::TestScene),
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
                ConvertedTrigger::PublishSettings => self.open_publish_settings(),
                ConvertedTrigger::ExportVideo => self.open_export_video(None),
                ConvertedTrigger::ExportAudio => self.open_export_audio(ctx, None),
                ConvertedTrigger::Preferences(page) => self.open_preferences(page),
                ConvertedTrigger::AddSwatch => {
                    self.add_active_swatch(ctx);
                    self.show_panel(PanelId::Swatches);
                }
                ConvertedTrigger::EditEnvelope => self.open_edit_envelope(),
                ConvertedTrigger::SoundProperties => self.open_sound_properties(ctx, None),
                ConvertedTrigger::LayerProperties => self.open_layer_properties(ctx, self.layer),
                ConvertedTrigger::SymbolsPopover => self.open_symbols_popover(ctx, None),
                ConvertedTrigger::TestScene => self.test_scene(),
                ConvertedTrigger::FindReplace => self.open_find_replace(),
                ConvertedTrigger::ShowPanel(panel) => self.show_panel(panel),
            }
        }
    }

    pub(crate) fn undo_menu_label(&self) -> String {
        if self
            .vector
            .draft
            .as_ref()
            .is_some_and(|d| !d.anchors.is_empty())
        {
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
            .exact_height(neoflash_ui::metrics::top_bar())
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
                ui.horizontal_centered(|ui| {
                    // Figma: logo, 4 pt, product name (Title), then the menu titles.
                    ui.spacing_mut().item_spacing.x = 4.;
                    neoflash_ui::components::logo(ui, 28.);
                    ui.label(
                        neoflash_ui::typography::rich("NeoFlash", neoflash_ui::TextStyle::Title)
                            .color(nf.text_primary),
                    );
                    ui.add_space(12.);
                    let menus_end = neoflash_ui::components::menu_bar(ui, |ui| {
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
                            ui.add_enabled_ui_semantic(self.has_document(), |ui| {
                                self.view_menu(ui)
                            });
                        });
                        menu_popup(ui, "Insert", "Blank keyframe", "⌘F8", |ui| {
                            ui.add_enabled_ui_semantic(self.has_document(), |ui| {
                                self.insert_menu(ui)
                            });
                        });
                        ui.add_enabled_ui_semantic(self.has_document(), |ui| {
                            self.modify_menu_ui(ui)
                        });
                        menu_popup(ui, "Control", "Enable simple buttons", "⌥⌘B", |ui| {
                            ui.add_enabled_ui_semantic(self.has_document(), |ui| {
                                self.control_menu(ui)
                            });
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
                        ui.cursor().left()
                    });
                    let actions = egui::Rect::from_min_max(
                        egui::pos2(menus_end + neoflash_ui::tokens::space::S2, bar.top()),
                        bar.right_bottom(),
                    );
                    let mut actions_ui = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(actions)
                            .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    );
                    self.top_bar_actions(&mut actions_ui);
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

    fn recent_file_label(path: &std::path::Path) -> String {
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| path.display().to_string())
    }

    /// File (§8).
    fn file_menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        use crate::commands::CommandId as C;
        if menu_item(ui, "New", &self.sc(C::FileNew)).clicked() {
            self.request(Pending::New, ctx);
            ui.close();
        }
        if menu_item(ui, "Open…", &self.sc(C::FileOpen)).clicked() {
            self.request(Pending::Open, ctx);
            ui.close();
        }
        let recent = self.workspace.general.existing_recent_files();
        let widest_recent = recent
            .iter()
            .map(|p| Self::recent_file_label(p))
            .chain(std::iter::once("Clear recent".to_owned()))
            .max_by_key(|s| s.len())
            .unwrap_or_else(|| "Clear recent".to_owned());
        ui.add_enabled_ui_semantic(!recent.is_empty(), |ui| {
            menu_popup(ui, "Open recent", &widest_recent, "", |ui| {
                for path in recent {
                    let label = Self::recent_file_label(&path);
                    let tooltip = path.display().to_string();
                    if menu_item(ui, &label, "").on_hover_text(tooltip).clicked() {
                        self.request(Pending::OpenPath(path), ctx);
                        ui.close();
                    }
                }
                nf::menu_separator(ui);
                if menu_item(ui, "Clear recent", "").clicked() {
                    self.workspace.clear_recent_files();
                    ui.close();
                }
            });
        });
        ui.add_enabled_ui_semantic(self.has_document(), |ui| {
            if menu_item(ui, "Close", &self.sc(C::FileClose)).clicked() {
                self.request(Pending::CloseDocument, ctx);
                ui.close();
            }
            nf::menu_separator(ui);
            if menu_item(ui, "Save", &self.sc(C::FileSave)).clicked() {
                self.save(false);
                ui.close();
            }
            if menu_item(ui, "Save as…", &self.sc(C::FileSaveAs)).clicked() {
                self.save(true);
                ui.close();
            }
            if menu_item_enabled(ui, self.can_revert(), "Revert", &self.sc(C::FileRevert)).clicked()
            {
                self.request_revert();
                ui.close();
            }
            nf::menu_separator(ui);
            menu_popup(ui, "Import", "Import graphics to Stage…", "", |ui| {
                if menu_item(
                    ui,
                    "Import graphics to Stage…",
                    &self.sc(C::FileImportGraphics),
                )
                .clicked()
                {
                    self.import_graphics_dialog();
                    ui.close();
                }
                if menu_item(
                    ui,
                    "Import legacy Flash…",
                    &self.sc(C::FileImportLegacyFlash),
                )
                .clicked()
                {
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
            menu_popup(
                ui,
                "Export",
                "Export Lottie…",
                &self.sc(C::FileExportLottie),
                |ui| {
                    Self::planned_menu_item(ui, "File ▸ Export ▸ Export image…", "");
                    Self::planned_menu_item(
                        ui,
                        "File ▸ Export ▸ Export video…",
                        &self.sc(C::FileExportVideo),
                    );
                    Self::planned_menu_item(
                        ui,
                        "File ▸ Export ▸ Export audio…",
                        &self.sc(C::FileExportAudio),
                    );
                    nf::menu_separator(ui);
                    if menu_item(ui, "Export Lottie…", &self.sc(C::FileExportLottie)).clicked() {
                        self.export_lottie_dialog(None);
                        ui.close();
                    }
                },
            );
            nf::menu_separator(ui);
            Self::planned_menu_item(
                ui,
                "File ▸ Publish settings…",
                &self.sc(C::FilePublishSettings),
            );
            if menu_item(ui, "Publish", &self.sc(C::FilePublish))
                .on_hover_text("Choose an output folder and publish the project as HTML5")
                .clicked()
            {
                self.export(false);
                ui.close();
            }
            nf::menu_separator(ui);
            Self::unavailable_item(ui, "Print…", "⌘P");
        });
    }

    /// Edit (§8) with the Edit ▸ Timeline submenu.
    fn edit_menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        use crate::commands::CommandId as C;
        ui.add_enabled_ui_semantic(self.has_document(), |ui| {
            let undo_label = self.undo_menu_label();
            let redo_label = self.redo_menu_label();
            if menu_item_enabled(
                ui,
                self.history.can_undo() || self.vector.draft.is_some(),
                &undo_label,
                &self.sc(C::EditUndo),
            )
            .clicked()
            {
                self.undo();
                ui.close();
            }
            if menu_item_enabled(
                ui,
                self.history.can_redo(),
                &redo_label,
                &self.sc(C::EditRedo),
            )
            .clicked()
            {
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
            for (label, cmd_id, command, enabled) in [
                ("Cut", C::EditCut, ClipboardCommand::Cut, can_copy),
                ("Copy", C::EditCopy, ClipboardCommand::Copy, can_copy),
                (
                    "Paste in centre",
                    C::EditPaste,
                    ClipboardCommand::Paste,
                    can_paste,
                ),
                (
                    "Paste in place",
                    C::EditPasteInPlace,
                    ClipboardCommand::PasteInPlace,
                    !text_target && self.layer_editable(),
                ),
            ] {
                if menu_item_enabled(ui, enabled, label, &self.sc(cmd_id)).clicked() {
                    self.request_clipboard(ctx, command);
                    ui.close();
                }
            }
            if menu_item_enabled(
                ui,
                !self.selection.is_empty(),
                "Clear",
                &self.sc(C::EditClear),
            )
            .clicked()
            {
                self.delete_selected();
                ui.close();
            }
            nf::menu_separator(ui);
            if menu_item_enabled(
                ui,
                !self.selection.is_empty(),
                "Duplicate",
                &self.sc(C::EditDuplicate),
            )
            .clicked()
            {
                self.duplicate();
                ui.close();
            }
            if menu_item(ui, "Select all", &self.sc(C::EditSelectAll)).clicked() {
                self.select_all();
                ui.close();
            }
            if menu_item_enabled(
                ui,
                !self.selection.is_empty(),
                "Deselect",
                &self.sc(C::EditDeselect),
            )
            .clicked()
            {
                self.finish_edits();
                self.clear_selection();
                ui.close();
            }
            nf::menu_separator(ui);
            if menu_item(ui, "Find and replace", &self.sc(C::EditFindAndReplace)).clicked() {
                self.open_find_replace();
                ui.close();
            }
            menu_popup(ui, "Timeline", "Synchronise symbols", "⌥⌘X", |ui| {
                self.edit_timeline_menu(ui);
            });
            nf::menu_separator(ui);
            let instance = self.selected_instance_symbol();
            if menu_item_enabled(
                ui,
                instance.is_some() || self.editing.len() > 1,
                "Edit symbols",
                &self.sc(C::EditSymbols),
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
        });
        Self::planned_menu_item(ui, "Edit ▸ Preferences…", &self.sc(C::EditPreferences));
        Self::planned_menu_item(
            ui,
            "Edit ▸ Keyboard shortcuts…",
            &self.sc(C::EditKeyboardShortcuts),
        );
    }

    /// Edit ▸ Timeline (§8).
    fn edit_timeline_menu(&mut self, ui: &mut egui::Ui) {
        use crate::commands::CommandId as C;
        use crate::timeline_edit::FrameCommand as F;
        for (label, cmd) in [
            ("Cut frames", F::Cut),
            ("Copy frames", F::Copy),
            ("Paste frames", F::Paste),
            ("Clear frames", F::Clear),
            ("Remove frames", F::Remove),
        ] {
            let sc = self.sc(C::for_frame_command(cmd));
            self.frame_command_item(ui, label, &sc, cmd);
        }
        nf::menu_separator(ui);
        if menu_item_enabled(ui, self.can_paste_motion(), "Paste motion special…", "").clicked() {
            self.open_paste_motion_special();
            ui.close();
        }
        let sel_all_sc = self.sc(C::EditTimelineSelectAllFrames);
        self.frame_command_item(ui, "Select all frames", &sel_all_sc, F::SelectAll);
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
    }

    /// View (§8) with Magnification, Preview mode, Grid, Guides and Snapping submenus.
    fn view_menu(&mut self, ui: &mut egui::Ui) {
        use crate::commands::{CommandId as C, has_override};
        if menu_item(ui, "Zoom in", &self.sc(C::ViewZoomIn)).clicked() {
            self.zoom_in();
            ui.close();
        }
        if menu_item(ui, "Zoom out", &self.sc(C::ViewZoomOut)).clicked() {
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
            if menu_item(ui, "100 %", &self.sc(C::ViewZoom100)).clicked() {
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
            if menu_item(ui, "Fit in window", &self.sc(C::ViewFitInWindow)).clicked() {
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
        if Self::view_toggle(ui, v.rulers, "Rulers", &self.sc(C::ViewRulers)) {
            self.toggle_rulers();
        }
        menu_popup(ui, "Grid", "Show over objects", "⇧⌘'", |ui| {
            if Self::view_toggle(ui, v.grid_show, "Show grid", &self.sc(C::ViewGridShow)) {
                self.toggle_grid();
            }
            if Self::view_toggle(ui, v.grid_snap, "Snap to grid", &self.sc(C::ViewGridSnap)) {
                self.toggle_grid_snap();
            }
            if Self::view_toggle(ui, v.grid_over_objects, "Show over objects", "") {
                self.toggle_grid_over_objects();
            }
            nf::menu_separator(ui);
            if menu_item(ui, "Edit grid…", &self.sc(C::ViewGridEdit)).clicked() {
                self.open_view_settings();
                ui.close();
            }
        });
        menu_popup(ui, "Guides", "Snap to guides", "⇧⌘;", |ui| {
            if Self::view_toggle(
                ui,
                v.guides_show,
                "Show guides",
                &self.sc(C::ViewGuidesShow),
            ) {
                self.toggle_guides();
            }
            let lock_sc = if has_override(&self.workspace.shortcuts, C::ViewGuidesLock) {
                self.sc(C::ViewGuidesLock)
            } else {
                String::new()
            };
            if Self::view_toggle(ui, v.guides_lock, "Lock guides", &lock_sc) {
                self.toggle_guide_lock();
            }
            if Self::view_toggle(
                ui,
                v.guides_snap,
                "Snap to guides",
                &self.sc(C::ViewGuidesSnap),
            ) {
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
        let mut show_hints = self.timeline_shape_hints.show;
        if menu_toggle(ui, &mut show_hints, "Show shape hints", "⌥⌘H").changed() {
            self.toggle_shape_hints();
            ui.close();
        }
    }

    /// Insert (§8).
    fn insert_menu(&mut self, ui: &mut egui::Ui) {
        use crate::commands::CommandId as C;
        if menu_item(ui, "New symbol…", &self.sc(C::InsertNewSymbol)).clicked() {
            self.begin_new_symbol();
            ui.close();
        }
        nf::menu_separator(ui);
        if menu_item(ui, "Layer", &self.sc(C::InsertLayer)).clicked() {
            self.insert_timeline_layer();
            ui.close();
        }
        if menu_item(ui, "Layer folder", &self.sc(C::InsertLayerFolder)).clicked() {
            self.insert_layer_folder(self.layer);
            ui.close();
        }
        if menu_item_enabled(
            ui,
            self.can_add_motion_guide_to(self.layer),
            "Motion guide",
            &self.sc(C::InsertMotionGuide),
        )
        .clicked()
        {
            self.add_motion_guide();
            ui.close();
        }
        nf::menu_separator(ui);
        let editable = self.layer_editable();
        if menu_item_enabled(ui, editable, "Frame", &self.sc(C::InsertFrame)).clicked() {
            self.finish_edits();
            self.insert_frame();
            ui.close();
        }
        if menu_item_enabled(ui, editable, "Keyframe", &self.sc(C::InsertKeyframe)).clicked() {
            self.finish_edits();
            self.make_key(false);
            ui.close();
        }
        if menu_item_enabled(
            ui,
            editable,
            "Blank keyframe",
            &self.sc(C::InsertBlankKeyframe),
        )
        .clicked()
        {
            self.finish_edits();
            self.make_key(true);
            ui.close();
        }
        nf::menu_separator(ui);
        let tweenable = self.can_create_tween();
        for (label, cmd_id, kind) in [
            ("Classic tween", C::InsertClassicTween, TweenKind::Classic),
            ("Shape tween", C::InsertShapeTween, TweenKind::Shape),
            ("Motion tween", C::InsertMotionTween, TweenKind::Motion),
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
        if menu_item(ui, "Scene", &self.sc(C::InsertScene)).clicked() {
            self.new_scene();
            ui.close();
        }
    }

    /// Control (§8).
    fn control_menu(&mut self, ui: &mut egui::Ui) {
        use crate::commands::CommandId as C;
        if menu_item(
            ui,
            if self.playing { "Stop" } else { "Play" },
            &self.sc(C::ControlPlay),
        )
        .clicked()
        {
            self.set_playing(!self.playing);
            ui.close();
        }
        let last = self.symbol().timeline.frame_count.saturating_sub(1);
        for (label, cmd_id, frame) in [
            ("Rewind", C::ControlRewind, 0),
            ("Go to end", C::ControlGoToEnd, last),
            (
                "Step forward one frame",
                C::ControlStepForward,
                (self.frame + 1).min(last),
            ),
            (
                "Step back one frame",
                C::ControlStepBack,
                self.frame.saturating_sub(1),
            ),
        ] {
            if menu_item(ui, label, &self.sc(cmd_id)).clicked() {
                self.seek_frame(frame);
                ui.close();
            }
        }
        nf::menu_separator(ui);
        if menu_item(ui, "Test movie", &self.sc(C::ControlTestMovie)).clicked() {
            self.export(true);
            ui.close();
        }
        if menu_item(ui, "Test scene", &self.sc(C::ControlTestScene)).clicked() {
            self.test_scene();
            ui.close();
        }
        nf::menu_separator(ui);
        let loop_sc = self.sc(C::ControlLoopPlayback);
        menu_toggle(
            ui,
            &mut self.workspace.timeline.loop_playback,
            "Loop playback",
            &loop_sc,
        );
        let all_scenes_sc = self.sc(C::ControlPlayAllScenes);
        menu_toggle(
            ui,
            &mut self.timeline_edit.play_all_scenes,
            "Play all scenes",
            &all_scenes_sc,
        );
        let mut muted = self.timeline_audio.muted;
        if menu_toggle(
            ui,
            &mut muted,
            "Mute sounds",
            &self.sc(C::ControlMuteSounds),
        )
        .changed()
        {
            self.toggle_mute_sounds();
        }
        nf::menu_separator(ui);
        let mut simple = self.enable_simple_buttons;
        let simple_sc = self.sc(C::ControlEnableSimpleButtons);
        if menu_toggle(ui, &mut simple, "Enable simple buttons", &simple_sc).changed() {
            self.set_enable_simple_buttons(simple);
            ui.close();
        }
    }

    /// Window (§8), then the §6 panels §8 does not list, then workspace placement and appearance.
    fn window_menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        use crate::commands::CommandId as C;
        Self::unavailable_item(ui, "Duplicate window", "⌥⌘K");
        let mut brushes = self.workspace.panels.brushes;
        if menu_toggle(ui, &mut brushes, "Brushes", "").changed() {
            self.set_panel_visible(PanelId::Brushes, brushes);
            ui.close();
        }
        if menu_item(ui, "Customize tools…", "").clicked() {
            self.open_customize_tools(ctx);
            ui.close();
        }
        nf::menu_separator(ui);
        let toggle = |editor: &mut Self, ui: &mut egui::Ui, index: usize| {
            let (name, _, panel) = WINDOW_PANELS[index];
            let shortcut = editor.sc(C::for_panel(panel));
            let mut visible = !editor.panels_hidden && editor.workspace.is_panel_active(panel);
            if menu_toggle(ui, &mut visible, name, &shortcut).changed() {
                editor.set_panel_visible(panel, visible);
                ui.close();
            }
        };
        // Timeline · Tools · Properties · Library · Actions · Align · Color · Swatches ·
        // Transform · Info · Ease · History · Scene · Output · Compiler errors · Accessibility.
        for index in 0..16 {
            toggle(self, ui, index);
        }
        nf::menu_separator(ui);
        toggle(self, ui, 16);
        Self::planned_menu_toggle(ui, "Window ▸ Debugger", "");
        for index in 17..21 {
            toggle(self, ui, index);
        }
        nf::menu_separator(ui);
        menu_popup(ui, "Workspaces", "Reset Developer workspace", "", |ui| {
            self.workspace_menu(ui, ctx);
        });
        let mut hidden = self.panels_hidden;
        let hide_sc = self.sc(C::WindowHidePanels);
        if menu_toggle(ui, &mut hidden, "Hide panels", &hide_sc).changed() {
            self.set_panels_hidden(hidden);
            ui.close();
        }
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
        let density = self.workspace.density;
        menu_popup(ui, "Density", "Classic Flash", "", |ui| {
            for option in crate::workspace::DensityPref::ALL {
                let mut selected = density == option;
                if menu_toggle(ui, &mut selected, option.label(), "").clicked() {
                    self.set_density(ctx, option);
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
        nf::menu_separator(ui);
        self.document_list(ui, ctx);
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
            if matches!(
                panel,
                PlannedPanel::GraphicProperties | PlannedPanel::TestMovie
            ) {
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
            planned_entry("File ▸ Export ▸ Export image…"),
            Some(PlannedPanel::ExportImage)
        );
    }
}
