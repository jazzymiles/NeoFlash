//! Application menu bar inside the NF2 top bar (`NeoFlash · File · Edit · View · Insert · Modify ·
//! Text · Control · Debug · Window · Help`) and the table of converted entry-point routes.
//!
//! Driven directly by [`crate::macos_menu::NativeMenuSpec`] so the in-window NF2 menu bar and the
//! native macOS system menu bar share 100% 1:1 top-level menus, items, submenus, separators,
//! dynamic `Undo`/`Redo` labels, shortcuts, enablement, and checkmarks (`T-377`).
use crate::app::*;
use crate::commands::CommandId;
use crate::macos_menu::{NativeMenuAction, NativeMenuItemSpec, NativeMenuSpec, NativeSubmenuRole};
use crate::theme::DesignUi as _;
use crate::ui_icons::{menu_check_enabled, menu_item_enabled, menu_popup};
#[cfg(test)]
use crate::workspace::PanelId;
use eframe::egui;
use neoflash_core::*;
use neoflash_ui::components::{self as nf, MenuItem};

/// Tooltip of every designed command the editor cannot perform yet.
pub(crate) const NOT_AVAILABLE: &str = "Not available yet";

/// Real panels in Window, in §8 order, with their shortcuts.
#[cfg(test)]
const WINDOW_PANELS: [(&str, &str, PanelId); 22] = [
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
    ("Debugger", "", PanelId::Debugger),
    ("Project", "", PanelId::Project),
    ("Code snippets", "", PanelId::CodeSnippets),
    ("Movie Explorer", "", PanelId::MovieExplorer),
    ("Filters", "", PanelId::Filters),
];

#[cfg(test)]
fn window_detachable_panels() -> impl Iterator<Item = (&'static str, PanelId)> {
    std::iter::once(("Brushes", PanelId::Brushes))
        .chain(WINDOW_PANELS.iter().map(|(name, _, panel)| (*name, *panel)))
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConvertedTrigger {
    DocumentSettings,
    PublishSettings,
    ExportImage,
    ExportVideo,
    ExportAudio,
    ImportVideo,
    StrokeStyle,
    FontEmbedding,
    /// Preferences opened on a page (`panels/preferences.rs`).
    Preferences(usize),
    AddSwatch,
    BitmapFill,
    EditEnvelope,
    SoundProperties,
    LayerProperties,
    SymbolsPopover,
    TestScene,
    TestMoviePreview,
    ToolOptions(Tool),
    FindReplace,
    ShowPanel(PanelId),
}

#[cfg(test)]
impl ConvertedTrigger {
    fn for_path(path: &str) -> Option<Self> {
        match path {
            "Document ▸ Document settings…" | "Modify ▸ Document…" => {
                Some(Self::DocumentSettings)
            }
            "File ▸ Publish settings…" | "Properties ▸ Publish settings…" => {
                Some(Self::PublishSettings)
            }
            "File ▸ Export ▸ Export image…" => Some(Self::ExportImage),
            "File ▸ Export ▸ Export video…" => Some(Self::ExportVideo),
            "File ▸ Export ▸ Export audio…" => Some(Self::ExportAudio),
            "File ▸ Import ▸ Import video…" => Some(Self::ImportVideo),
            "Tool options ▸ Stroke style…" => Some(Self::StrokeStyle),
            "Tool options ▸ Rectangle primitive…" | "Tool options ▸ Oval primitive…" => {
                Some(Self::ShowPanel(PanelId::Properties))
            }
            "Tool options ▸ Spray brush…" => Some(Self::ToolOptions(Tool::SprayBrush)),
            "Tool options ▸ Deco tool…" => Some(Self::ToolOptions(Tool::Deco)),
            "Tool options ▸ Bone tool…" => Some(Self::ToolOptions(Tool::Bone)),
            "Library menu ▸ Font embedding…" => Some(Self::FontEmbedding),
            "Edit ▸ Preferences…" | "NeoFlash ▸ Preferences…" => {
                Some(Self::Preferences(crate::panels::preferences::PAGE_GENERAL))
            }
            "Edit ▸ Keyboard shortcuts…" | "NeoFlash ▸ Keyboard shortcuts…" => Some(
                Self::Preferences(crate::panels::preferences::PAGE_SHORTCUTS),
            ),
            "Edit ▸ Find and replace" => Some(Self::FindReplace),
            "Window ▸ Panels ▸ Debugger" | "Window ▸ Debugger" => {
                Some(Self::ShowPanel(PanelId::Debugger))
            }
            "Window ▸ Panels ▸ Compiler errors" | "Window ▸ Compiler errors" => {
                Some(Self::ShowPanel(PanelId::CompilerErrors))
            }
            "Window ▸ Panels ▸ Code snippets" | "Window ▸ Code snippets" => {
                Some(Self::ShowPanel(PanelId::CodeSnippets))
            }
            "Window ▸ Panels ▸ Movie Explorer" | "Window ▸ Movie Explorer" => {
                Some(Self::ShowPanel(PanelId::MovieExplorer))
            }
            "Window ▸ Panels ▸ Filters" | "Window ▸ Filters" => {
                Some(Self::ShowPanel(PanelId::Filters))
            }
            "Color menu ▸ Add swatch" | "Swatches popover ▸ Add swatch" => {
                Some(Self::AddSwatch)
            }
            "Color fill type ▸ Bitmap fill" => Some(Self::BitmapFill),
            "Library menu ▸ Edit envelope…" => Some(Self::EditEnvelope),
            "Library menu ▸ Sound properties…" => Some(Self::SoundProperties),
            "Modify ▸ Timeline ▸ Layer properties…" | "Layer menu ▸ Properties…" => {
                Some(Self::LayerProperties)
            }
            "Breadcrumb ▸ Symbols & scenes" => Some(Self::SymbolsPopover),
            "Control ▸ Test scene" => Some(Self::TestScene),
            "Control ▸ Test movie preview" => Some(Self::TestMoviePreview),
            _ => None,
        }
    }
}

fn widest_menu_entry(items: &[NativeMenuItemSpec]) -> (&str, &str) {
    let mut best: (&str, &str, usize) = ("", "", 0);
    for item in items {
        let (label, sc): (&str, &str) = match item {
            NativeMenuItemSpec::Action {
                label,
                shortcut_label,
                ..
            } => (label.as_str(), shortcut_label.as_str()),
            NativeMenuItemSpec::System {
                label,
                shortcut_label,
                ..
            } => (label.as_str(), shortcut_label.as_str()),
            NativeMenuItemSpec::Submenu { label, .. } => (label.as_str(), "▸"),
            NativeMenuItemSpec::Section(title) => (title.as_str(), ""),
            NativeMenuItemSpec::Separator => continue,
        };
        let score = label.chars().count() + sc.chars().count();
        if score > best.2 {
            best = (label, sc, score);
        }
    }
    (best.0, best.1)
}

#[cfg(test)]
fn converted_triggers_id() -> egui::Id {
    egui::Id::new("nf2-converted-entry-triggers")
}

impl Editor {
    /// Route a contextual menu or button entry point to its live editor dialog, panel, or tool.
    #[cfg(test)]
    pub(crate) fn open_entry_point(ctx: &egui::Context, path: &str) {
        let Some(trigger) = ConvertedTrigger::for_path(path) else {
            debug_assert!(false, "unknown entry point {path:?}");
            return;
        };
        let id = converted_triggers_id();
        ctx.data_mut(|d| {
            let mut queue = d.get_temp::<Vec<ConvertedTrigger>>(id).unwrap_or_default();
            queue.push(trigger);
            d.insert_temp(id, queue);
        });
    }

    /// Execute any converted entry-point triggers queued during the frame.
    #[cfg(test)]
    pub(crate) fn drain_converted_entry_triggers(&mut self, ctx: &egui::Context) {
        let id = converted_triggers_id();
        let triggers = ctx
            .data_mut(|d| d.remove_temp::<Vec<ConvertedTrigger>>(id))
            .unwrap_or_default();
        for trigger in triggers {
            match trigger {
                ConvertedTrigger::DocumentSettings => self.open_document_settings(),
                ConvertedTrigger::PublishSettings => self.open_publish_settings(),
                ConvertedTrigger::ExportImage => self.open_export_image(ctx),
                ConvertedTrigger::ExportVideo => self.open_export_video(None),
                ConvertedTrigger::ExportAudio => self.open_export_audio(ctx, None),
                ConvertedTrigger::ImportVideo => self.open_import_video(ctx),
                ConvertedTrigger::StrokeStyle => self.open_stroke_style(ctx),
                ConvertedTrigger::FontEmbedding => self.open_font_embedding(ctx),
                ConvertedTrigger::Preferences(page) => self.open_preferences(page),
                ConvertedTrigger::AddSwatch => {
                    self.add_active_swatch(ctx);
                    self.show_panel(PanelId::Swatches);
                }
                ConvertedTrigger::BitmapFill => {
                    self.set_selection_fill_type(
                        false,
                        crate::authoring_modify::PaintFillType::BitmapFill,
                    );
                    self.show_panel(PanelId::Color);
                }
                ConvertedTrigger::EditEnvelope => self.open_edit_envelope(),
                ConvertedTrigger::SoundProperties => self.open_sound_properties(ctx, None),
                ConvertedTrigger::LayerProperties => self.open_layer_properties(ctx, self.layer),
                ConvertedTrigger::SymbolsPopover => self.open_symbols_popover(ctx, None),
                ConvertedTrigger::TestScene => self.test_scene(),
                ConvertedTrigger::TestMoviePreview => self.open_test_movie_preview(),
                ConvertedTrigger::ToolOptions(tool) => {
                    self.set_tool(tool);
                    self.show_panel(PanelId::Properties);
                }
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

    /// Disabled row for a designed command the editor cannot perform yet. Its shortcut does nothing.
    pub(crate) fn unavailable_item(ui: &mut egui::Ui, label: &str, shortcut: &str) {
        crate::ui_icons::hover_tooltip(
            menu_item_enabled(ui, false, label, shortcut),
            label,
            (!shortcut.is_empty()).then_some(shortcut),
            Some(NOT_AVAILABLE),
        );
    }

    /// Disabled submenu row (▸) whose commands are all unavailable.
    pub(crate) fn unavailable_submenu(ui: &mut egui::Ui, label: &str) {
        crate::ui_icons::hover_tooltip(
            ui.add_enabled(false, MenuItem::new(label).submenu(true)),
            label,
            None,
            Some(NOT_AVAILABLE),
        );
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
        if self.live_script_preview
            && self.editing_scene()
            && self
                .authored_preview
                .as_ref()
                .is_some_and(|p| p.swf_player.is_some())
            && self.seek_swf_preview(frame).is_some()
        {
            return;
        }
        self.reset_authored_preview();
        self.frame = frame.min(self.symbol().timeline.frame_count.saturating_sub(1));
    }

    /// Edit ▸ Edit symbols (⌘E): enter the selected symbol, or return to the scene.
    pub(crate) fn toggle_edit_symbols(&mut self) {
        if self.selected_instance_symbol().is_some() {
            self.enter_selected_symbol_in_place();
        } else if !self.editing_scene() {
            self.leave_to_level(0);
        }
    }

    fn render_top_menu_popup(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        title: &str,
        clipboard_target: &crate::clipboard::ClipboardTarget,
        edit_open: &mut bool,
    ) {
        if false {
            self.modify_menu_ui(ui);
        }
        if title == "Edit" {
            *edit_open = true;
            if !self.clipboard.menu_open || self.clipboard.menu_viewport != Some(ctx.viewport_id())
            {
                self.clipboard.menu_target = Some(clipboard_target.clone());
            }
        }
        let Some(top_spec) = NativeMenuSpec::build_top_menu(self, Some(ctx), title) else {
            return;
        };
        let (widest_label, widest_shortcut) = widest_menu_entry(&top_spec.items);
        let width = crate::ui_icons::menu_width(ui, widest_label, widest_shortcut);
        crate::ui_icons::menu_contents(ui, width, |ui| {
            self.render_menu_items_ui(ui, ctx, &top_spec.items);
        });
    }

    fn render_menu_items_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        items: &[NativeMenuItemSpec],
    ) {
        for item in items {
            match item {
                NativeMenuItemSpec::Separator => {
                    nf::menu_separator(ui);
                }
                NativeMenuItemSpec::Section(title) => {
                    nf::menu_section(ui, title);
                }
                NativeMenuItemSpec::System {
                    label,
                    role,
                    shortcut_label,
                    enabled,
                    ..
                } => {
                    if menu_item_enabled(ui, *enabled, label, shortcut_label).clicked() {
                        self.execute_native_system_role(ctx, *role);
                        ui.close();
                    }
                }
                NativeMenuItemSpec::Action {
                    label,
                    shortcut_label,
                    enabled,
                    checkable,
                    checked,
                    tooltip,
                    action,
                    ..
                } => {
                    if *action == NativeMenuAction::Unavailable {
                        if *checkable {
                            let resp =
                                menu_check_enabled(ui, false, label, shortcut_label, *checked);
                            crate::ui_icons::hover_tooltip(
                                resp,
                                label,
                                (!shortcut_label.is_empty()).then_some(shortcut_label.as_str()),
                                tooltip.as_deref().or(Some(NOT_AVAILABLE)),
                            );
                        } else {
                            Self::unavailable_item(ui, label, shortcut_label);
                        }
                        continue;
                    }
                    let resp = if *checkable {
                        menu_check_enabled(ui, *enabled, label, shortcut_label, *checked)
                    } else {
                        menu_item_enabled(ui, *enabled, label, shortcut_label)
                    };
                    let resp = if let Some(tip) = tooltip {
                        nf::tooltip(
                            resp,
                            tip,
                            (!shortcut_label.is_empty()).then_some(shortcut_label.as_str()),
                        )
                    } else {
                        resp
                    };
                    if resp.clicked() {
                        if *action == NativeMenuAction::Command(CommandId::HelpSearchFunctions) {
                            self.open_function_search_at(ctx, Some(resp.rect));
                        } else {
                            self.execute_native_menu_action(ctx, action.clone());
                        }
                        ui.close();
                    }
                }
                NativeMenuItemSpec::Submenu {
                    label,
                    enabled,
                    role,
                    items: sub_items,
                } => {
                    if matches!(role, Some(NativeSubmenuRole::Services))
                        || (!*enabled && sub_items.is_empty())
                    {
                        Self::unavailable_submenu(ui, label);
                    } else {
                        let (widest_label, widest_shortcut) = widest_menu_entry(sub_items);
                        ui.add_enabled_ui_semantic(*enabled, |ui| {
                            menu_popup(ui, label, widest_label, widest_shortcut, |ui| {
                                self.render_menu_items_ui(ui, ctx, sub_items);
                            });
                        });
                    }
                }
            }
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
                if self.has_modal_dialog_in(ctx) {
                    ui.disable_semantic();
                }
                let bar = ui.max_rect();
                ui.painter().hline(
                    bar.x_range(),
                    bar.bottom() - 0.5,
                    egui::Stroke::new(1.0_f32, nf.border_subtle),
                );
                ui.horizontal_centered(|ui| {
                    // Figma: logo, 4 pt, product name (Title, interactive NeoFlash app menu), then the menu titles.
                    ui.spacing_mut().item_spacing.x = 4.;
                    let logo_side = neoflash_ui::metrics::density().pick(28.0, 22.0, 18.0);
                    neoflash_ui::components::logo(ui, logo_side);
                    let menus_end = neoflash_ui::components::menu_bar(ui, |ui| {
                        for title in NativeMenuSpec::TOP_LEVEL_TITLES {
                            if title == "NeoFlash" {
                                ui.menu_button(
                                    neoflash_ui::typography::rich(
                                        "NeoFlash",
                                        neoflash_ui::TextStyle::Title,
                                    )
                                    .color(nf.text_primary),
                                    |ui| {
                                        self.render_top_menu_popup(
                                            ui,
                                            ctx,
                                            title,
                                            &clipboard_target,
                                            &mut edit_open,
                                        );
                                    },
                                );
                                ui.add_space(8.);
                            } else {
                                ui.menu_button(title, |ui| {
                                    self.render_top_menu_popup(
                                        ui,
                                        ctx,
                                        title,
                                        &clipboard_target,
                                        &mut edit_open,
                                    );
                                });
                            }
                        }
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
        self.function_search_popover(ctx);
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
    fn window_panels_include_debugger_and_unique_ids() {
        let mut seen = std::collections::HashSet::new();
        for (_, _, panel) in WINDOW_PANELS {
            assert!(
                seen.insert(panel),
                "duplicate panel in WINDOW_PANELS: {panel:?}"
            );
        }
        assert!(
            WINDOW_PANELS
                .iter()
                .any(|(label, _, panel)| *label == "Debugger" && *panel == PanelId::Debugger)
        );
        let detachable: std::collections::HashSet<_> =
            window_detachable_panels().map(|(_, panel)| panel).collect();
        assert_eq!(
            detachable,
            PanelId::DETACHABLE.into_iter().collect(),
            "Window detach/dock routes must cover every detachable panel"
        );
    }

    #[test]
    fn converted_entry_points_route_all_promoted_panels_and_dialogs() {
        for path in [
            "File ▸ Import ▸ Import video…",
            "File ▸ Export ▸ Export image…",
            "Library menu ▸ Font embedding…",
            "Color fill type ▸ Bitmap fill",
            "Window ▸ Panels ▸ Debugger",
            "Tool options ▸ Rectangle primitive…",
            "Tool options ▸ Oval primitive…",
            "Tool options ▸ Stroke style…",
            "Tool options ▸ Spray brush…",
            "Tool options ▸ Deco tool…",
            "Tool options ▸ Bone tool…",
            "Control ▸ Test movie preview",
        ] {
            assert!(
                ConvertedTrigger::for_path(path).is_some(),
                "expected converted trigger for {path:?}"
            );
        }
        for (index, src) in SOURCES.iter().enumerate() {
            let prod = src.split("#[cfg(test)]").next().unwrap_or(src);
            assert!(
                !prod.contains("Planned panels") && !prod.contains("PlannedPanel"),
                "stale planned-panel reference in SOURCES[{index}]"
            );
        }
    }
}
