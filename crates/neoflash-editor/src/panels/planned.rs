//! Planned panels: read-only NF2 previews of panels and dialogs the engine does not support yet
//! (docs/DESIGN-SYSTEM.md rule 9 "Engine boundary").
//!
//! Every preview is a floating NF2 surface (`Surface::Floating` + `FloatingPanelHeader`, or `Surface::Dialog` +
//! `DialogHeader` for dialogs) with a "Planned feature" banner. Bodies show the reference layout with fixed
//! sample values; their controls are disabled (`ui.disable()`), so a preview never reads or writes the
//! project. Only preview chrome reacts: move, collapse, close and — where the reference has them — page or
//! format tabs that switch between preview pages. The one wired setting is Preferences ▸ General ▸ Theme:
//! Save applies it to the editor theme preference (never the project), Cancel discards it. Open previews live
//! in egui temp memory, not in the document or the workspace prefs.
//!
//! Bodies: `planned_panels.rs` (History, Swatches, Info, Compiler errors, Debugger, Project, Code snippets and
//! movie clip display/filter properties), `planned_dialogs.rs` (Preferences, Document settings, Publish
//! settings, Export image / video / audio, Import video, Sound properties), `planned_color.rs` (Color — radial
//! gradient / solid mixer, bitmap fill), `planned_drawing.rs` (oval / rectangle primitive, PolyStar, Deco, Spray
//! brush, IK bone), `planned_documents.rs` (Graphic looping, Layer properties, Stroke style, Font
//! embedding, Test Movie window, Find and replace, Movie Explorer, Symbols & scenes popover).
use crate::app::Editor;
use crate::theme::ThemeMode;
use eframe::egui::{self, Ui};
use neoflash_ui::components as c;
use neoflash_ui::metrics;
use neoflash_ui::tokens::{TextStyle, radius, space};
use neoflash_ui::typography::rich;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum PlannedPanel {
    Debugger,
    CodeSnippets,
    CompilerErrors,
    PropertiesFilters,
    Preferences,
    PublishSettings,
    ExportImage,
    ExportVideo,
    ExportAudio,
    ImportVideo,
    SoundProperties,
    BitmapFill,
    PropertiesOvalPrimitive,
    PropertiesRectPrimitive,
    DecoTool,
    SprayBrush,
    BoneProperties,
    GraphicProperties,
    LayerProperties,
    StrokeStyle,
    FontEmbedding,
    TestMovie,
    FindReplace,
    MovieExplorer,
    SymbolsPopover,
}

/// How the preview is framed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Chrome {
    /// Floating panel with up to three header tabs.
    Panel(&'static [&'static str]),
    /// Dialog: `Surface::Dialog` + `DialogHeader` (non-modal while it is a preview).
    Dialog,
    /// Popover: menu/popover frame, no header; dragged by its banner row.
    Popover,
    /// Separate window (Test Movie): macOS title bar with window controls, title and trailing actions.
    Window,
}

/// Title bar of a preview window (`Window / Test Movie` › `Title bar`, 52 pt in the outline).
const WINDOW_TITLE_BAR: f32 = 52.0;

impl PlannedPanel {
    pub(crate) const ALL: [Self; 25] = [
        Self::Debugger,
        Self::CodeSnippets,
        Self::CompilerErrors,
        Self::PropertiesFilters,
        Self::Preferences,
        Self::PublishSettings,
        Self::ExportImage,
        Self::ExportVideo,
        Self::ExportAudio,
        Self::ImportVideo,
        Self::SoundProperties,
        Self::BitmapFill,
        Self::PropertiesOvalPrimitive,
        Self::PropertiesRectPrimitive,
        Self::DecoTool,
        Self::SprayBrush,
        Self::BoneProperties,
        Self::GraphicProperties,
        Self::LayerProperties,
        Self::StrokeStyle,
        Self::FontEmbedding,
        Self::TestMovie,
        Self::FindReplace,
        Self::MovieExplorer,
        Self::SymbolsPopover,
    ];

    /// Menu label (Window ▸ Planned panels ▸ …).
    pub(crate) const fn title(self) -> &'static str {
        match self {
            Self::Debugger => "Debugger",
            Self::CodeSnippets => "Code snippets",
            Self::CompilerErrors => "Compiler errors",
            Self::PropertiesFilters => "Properties — Display & filters",
            Self::Preferences => "Preferences",
            Self::PublishSettings => "Publish settings",
            Self::ExportImage => "Export image",
            Self::ExportVideo => "Export video",
            Self::ExportAudio => "Export audio",
            Self::ImportVideo => "Import video",
            Self::SoundProperties => "Sound properties",
            Self::BitmapFill => "Color — Bitmap fill",
            Self::PropertiesOvalPrimitive => "Properties — Oval primitive",
            Self::PropertiesRectPrimitive => "Properties — Rectangle primitive",
            Self::DecoTool => "Properties — Deco tool",
            Self::SprayBrush => "Properties — Spray brush",
            Self::BoneProperties => "Properties — IK bone",
            Self::GraphicProperties => "Properties — Graphic looping",
            Self::LayerProperties => "Layer properties",
            Self::StrokeStyle => "Stroke style",
            Self::FontEmbedding => "Font embedding",
            Self::TestMovie => "Test Movie",
            Self::FindReplace => "Find and replace",
            Self::MovieExplorer => "Movie Explorer",
            Self::SymbolsPopover => "Symbols & scenes",
        }
    }

    /// Reference PNG under `interface/design-system/exports/panels/{dark,light}/png/`.
    pub(crate) const fn reference(self) -> &'static str {
        match self {
            Self::Debugger | Self::CodeSnippets => "scripting-panels.png",
            Self::CompilerErrors => "panels-more-2.png",
            Self::PropertiesFilters => "panel-properties-movie-clip.png",
            Self::Preferences => "dialog-preferences-*.png",
            Self::PublishSettings => "publish-misc-dialogs.png",
            Self::ExportImage | Self::ExportVideo | Self::ExportAudio => "dialogs-export.png",
            Self::ImportVideo => "dialog-import-video.png",
            Self::SoundProperties | Self::StrokeStyle => "dialogs-sound-stroke.png",
            Self::PropertiesOvalPrimitive => "panel-properties-oval-primitive.png",
            Self::PropertiesRectPrimitive => "drawing-tools-panels.png",
            Self::DecoTool | Self::SprayBrush | Self::BoneProperties => {
                "drawing-tools-panels-2.png"
            }
            Self::GraphicProperties | Self::LayerProperties => "symbols-panels.png",
            Self::BitmapFill | Self::FontEmbedding | Self::TestMovie => "publish-misc-dialogs.png",
            Self::FindReplace | Self::MovieExplorer => "document-dialogs.png",
            Self::SymbolsPopover => "interface/{dark,light}/png/editor-editing-symbol-in-place.png",
        }
    }

    pub(crate) const fn chrome(self) -> Chrome {
        match self {
            Self::PropertiesFilters => Chrome::Panel(&["Properties", "Filters"]),
            Self::Debugger => Chrome::Panel(&["Debugger"]),
            Self::CodeSnippets => Chrome::Panel(&["Code snippets"]),
            Self::CompilerErrors => Chrome::Panel(&["Compiler errors"]),
            Self::BitmapFill => Chrome::Panel(&["Color", "Swatches"]),
            Self::PropertiesOvalPrimitive | Self::PropertiesRectPrimitive => {
                Chrome::Panel(&["Properties", "Filters"])
            }
            Self::DecoTool | Self::SprayBrush | Self::BoneProperties | Self::GraphicProperties => {
                Chrome::Panel(&["Properties"])
            }
            Self::MovieExplorer => Chrome::Panel(&["Movie Explorer"]),
            Self::SymbolsPopover => Chrome::Popover,
            Self::TestMovie => Chrome::Window,
            _ => Chrome::Dialog,
        }
    }

    /// Active header tab: the first tab for the NF2 v2 previews whose reference shows the first tab
    /// active (Color, primitives); the last tab for the earlier previews (Properties ▸ Filters).
    pub(crate) const fn active_tab(self, tabs: usize) -> usize {
        match self {
            Self::BitmapFill
            | Self::PropertiesOvalPrimitive
            | Self::PropertiesRectPrimitive => 0,
            _ => tabs.saturating_sub(1),
        }
    }

    /// Outer width from the reference frame.
    pub(crate) const fn width(self) -> f32 {
        match self {
            Self::Debugger => 960.,
            Self::Preferences => 920.,
            Self::PublishSettings => 900.,
            Self::ExportVideo => 840.,
            Self::ExportImage => 760.,
            Self::ImportVideo => 720.,
            Self::ExportAudio => 640.,
            Self::CompilerErrors => 560.,
            Self::SoundProperties => 520.,
            Self::CodeSnippets => 340.,
            Self::PropertiesFilters => 324.,
            Self::TestMovie => 1000.,
            Self::FindReplace => 640.,
            Self::FontEmbedding => 760.,
            Self::StrokeStyle => 480.,
            Self::LayerProperties => 440.,
            Self::MovieExplorer => 340.,
            Self::PropertiesOvalPrimitive
            | Self::PropertiesRectPrimitive
            | Self::DecoTool
            | Self::SprayBrush
            | Self::BoneProperties
            | Self::GraphicProperties => 324.,
            // Popover frame adds a 4 pt margin: 312 + 2 × 4 = the 320 pt reference.
            Self::SymbolsPopover => 312.,
            Self::BitmapFill => 284.,
        }
    }
}

/// Open previews and their preview-only navigation (egui temp memory, never saved).
#[derive(Clone, Debug, Default)]
pub(crate) struct PlannedState {
    pub(crate) open: Vec<(PlannedPanel, egui::Pos2)>,
    pub(crate) collapsed: Vec<PlannedPanel>,
    /// Preferences category index.
    pub(crate) preferences_page: usize,
    /// Export image format index (PNG, SVG, PSD, AI, PDF).
    pub(crate) export_format: usize,
    /// Preferences ▸ General ▸ Theme draft (`None` = the current theme); dropped when Preferences closes.
    pub(crate) theme_draft: Option<ThemeMode>,
    /// Theme confirmed with Preferences ▸ Save; [`Editor::planned_panels`] applies and clears it.
    pub(crate) theme_save: Option<ThemeMode>,
}

fn state_id() -> egui::Id {
    egui::Id::new("nf2-planned-panels")
}

pub(crate) fn planned_state(ctx: &egui::Context) -> PlannedState {
    ctx.data(|d| d.get_temp::<PlannedState>(state_id()))
        .unwrap_or_default()
}

fn store_state(ctx: &egui::Context, state: PlannedState) {
    ctx.data_mut(|d| d.insert_temp(state_id(), state));
}

pub(crate) fn is_open(ctx: &egui::Context, panel: PlannedPanel) -> bool {
    planned_state(ctx).open.iter().any(|(p, _)| *p == panel)
}

/// Open or close a preview (Window ▸ Planned panels). New previews cascade from the top-left of the content.
pub(crate) fn set_open(ctx: &egui::Context, panel: PlannedPanel, open: bool) {
    let mut state = planned_state(ctx);
    state.open.retain(|(p, _)| *p != panel);
    if open {
        let area = ctx.content_rect();
        let step = (state.open.len() % 8) as f32 * space::S6;
        let pos = egui::pos2(
            area.left() + metrics::TOOLS_RAIL + space::S6 + step,
            area.top() + metrics::TOP_BAR + space::S6 + step,
        );
        state.open.push((panel, pos));
    }
    store_state(ctx, state);
}

/// Window ▸ Planned panels submenu body: one toggle per preview.
pub(crate) fn menu_items(ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    for panel in PlannedPanel::ALL {
        let mut open = is_open(&ctx, panel);
        if crate::ui_icons::menu_toggle(ui, &mut open, panel.title(), "").changed() {
            set_open(&ctx, panel, open);
            ui.close();
        }
    }
}

impl Editor {
    /// Draw open planned-panel previews. They never touch the project; the only wired setting,
    /// Preferences ▸ General ▸ Theme, changes the editor theme preference when Save is clicked.
    pub(crate) fn planned_panels(&mut self, ctx: &egui::Context) {
        self.drain_converted_entry_triggers(ctx);
        open_from_env(ctx);
        show_planned_panels(ctx);
        let mut state = planned_state(ctx);
        if let Some(theme) = state.theme_save.take() {
            store_state(ctx, state);
            self.set_theme(ctx, theme);
        }
    }
}

/// Screenshot aid: `NEOFLASH_SCREENSHOT_PLANNED=ColorGradient,TestMovie` (variant names, or `all`) opens those
/// previews once at start-up, laid out left to right in rows instead of cascading, for comparison with the
/// reference PNGs. Read-only like every preview.
fn open_from_env(ctx: &egui::Context) {
    let flag = egui::Id::new("nf2-planned-env");
    if ctx.data(|d| d.get_temp::<bool>(flag)).unwrap_or(false) {
        return;
    }
    ctx.data_mut(|d| d.insert_temp(flag, true));
    let Ok(list) = std::env::var("NEOFLASH_SCREENSHOT_PLANNED") else {
        return;
    };
    let wanted: Vec<PlannedPanel> = PlannedPanel::ALL
        .into_iter()
        .filter(|p| list == "all" || list.split(',').any(|name| name.trim() == format!("{p:?}")))
        .collect();
    let area = ctx.content_rect();
    let mut state = planned_state(ctx);
    let (mut x, mut y, mut row) = (area.left() + space::S4, area.top() + space::S4, 0.0_f32);
    for panel in wanted {
        if x + panel.width() > area.right() && x > area.left() + space::S4 {
            x = area.left() + space::S4;
            y += row + space::S4;
            row = 0.0;
        }
        state.open.retain(|(p, _)| *p != panel);
        state.open.push((panel, egui::pos2(x, y)));
        x += panel.width() + space::S4;
        row = row.max(area.height() / 2.0);
    }
    store_state(ctx, state);
}

/// Draw every open preview (free function so tests can run it without an editor).
pub(crate) fn show_planned_panels(ctx: &egui::Context) {
    let mut state = planned_state(ctx);
    if state.open.is_empty() {
        return;
    }
    let mut closed = Vec::new();
    let open = state.open.clone();
    for (i, (panel, pos)) in open.into_iter().enumerate() {
        let (close, new_pos) = preview_window(ctx, panel, pos, &mut state);
        state.open[i].1 = new_pos;
        if close {
            closed.push(panel);
        }
    }
    state.open.retain(|(p, _)| !closed.contains(p));
    state.collapsed.retain(|p| !closed.contains(p));
    if closed.contains(&PlannedPanel::Preferences) {
        state.theme_draft = None;
    }
    // Keep previews opened from inside a preview this frame (e.g. Color ▸ Bitmap fill).
    for (p, pos) in planned_state(ctx).open {
        if !state.open.iter().any(|(q, _)| *q == p) && !closed.contains(&p) {
            state.open.push((p, pos));
        }
    }
    store_state(ctx, state);
}

/// One preview window. Returns (close requested, new position).
fn preview_window(
    ctx: &egui::Context,
    panel: PlannedPanel,
    pos: egui::Pos2,
    state: &mut PlannedState,
) -> (bool, egui::Pos2) {
    let nf = neoflash_ui::palette(ctx);
    let mut close = false;
    let mut delta = egui::Vec2::ZERO;
    let width = panel.width();
    let frame = match panel.chrome() {
        Chrome::Panel(_) => c::Surface::Floating.frame(ctx),
        Chrome::Dialog => c::Surface::Dialog.frame(ctx),
        Chrome::Popover => c::popover_frame(ctx),
        Chrome::Window => c::Surface::Dialog.frame(ctx),
    };
    let area = egui::Area::new(egui::Id::new(("nf2-planned", panel)))
        .order(egui::Order::Foreground)
        .current_pos(pos)
        .movable(false)
        .constrain(true)
        .show(ctx, |ui| {
            frame.show(ui, |ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing = egui::vec2(space::S2, space::S2);
                let collapsed = state.collapsed.contains(&panel);
                match panel.chrome() {
                    Chrome::Panel(tabs) => {
                        let header = c::FloatingPanelHeader {
                            tabs,
                            active: panel.active_tab(tabs.len()),
                            collapsed,
                        }
                        .show(ui);
                        delta = header.drag.drag_delta();
                        close = header.close;
                        if header.collapse {
                            if collapsed {
                                state.collapsed.retain(|p| *p != panel);
                            } else {
                                state.collapsed.push(panel);
                            }
                        }
                        c::popup_below(&header.menu, 200.0, |ui| {
                            ui.add_enabled(
                                false,
                                c::MenuItem::new(&format!("Reference: {}", panel.reference())),
                            );
                            c::menu_separator(ui);
                            if ui.add(c::MenuItem::new("Close preview")).clicked() {
                                close = true;
                                ui.close();
                            }
                        });
                    }
                    Chrome::Dialog => {
                        let top = ui.cursor().min;
                        let rect = egui::Rect::from_min_size(
                            top,
                            egui::vec2(width, metrics::DIALOG_HEADER),
                        );
                        // Register the drag zone before the header so the header's ✕ lies above it
                        // and receives the click (the later widget wins the hit test).
                        let drag = ui.interact(
                            rect,
                            ui.id().with("planned-dialog-drag"),
                            egui::Sense::drag(),
                        );
                        close = c::DialogHeader {
                            title: panel.title(),
                        }
                        .show(ui);
                        if drag.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                        }
                        delta = drag.drag_delta();
                        ui.painter().hline(
                            rect.x_range(),
                            rect.bottom() - 0.5,
                            egui::Stroke::new(1.0_f32, nf.border_subtle),
                        );
                    }
                    Chrome::Popover => {}
                    Chrome::Window => {
                        let (c2, d) = window_title_bar(ui, panel, width);
                        close = c2;
                        delta = d;
                    }
                }
                if collapsed {
                    return;
                }
                let pad = match panel.chrome() {
                    Chrome::Panel(_) => space::S3,
                    Chrome::Dialog => space::S5,
                    Chrome::Popover => 0.0,
                    Chrome::Window => space::S3,
                };
                egui::Frame::new()
                    .inner_margin(egui::Margin::same(pad as i8))
                    .show(ui, |ui| {
                        ui.set_width(width - 2.0 * pad);
                        if panel.chrome() == Chrome::Popover {
                            // Popovers have no header: the banner row is the drag handle.
                            let top = ui.cursor().min;
                            banner(ui);
                            let rect = egui::Rect::from_min_max(
                                top,
                                egui::pos2(top.x + width - 2.0 * pad, ui.cursor().min.y),
                            );
                            let drag = ui.interact(
                                rect,
                                ui.id().with("planned-popover-drag"),
                                egui::Sense::drag(),
                            );
                            delta = drag.drag_delta();
                        } else {
                            banner(ui);
                        }
                        ui.add_space(space::S1);
                        close |= body(ui, panel, state);
                    });
            });
        });
    // Popovers have no close button: like real popovers, a click outside dismisses them. The click
    // that opened the popover (it was not shown last frame) does not count.
    if panel.chrome() == Chrome::Popover
        && ctx.memory(|m| m.areas().visible_last_frame(&area.response.layer_id))
        && area.response.clicked_elsewhere()
    {
        close = true;
    }
    (close, pos + delta)
}

/// macOS-style title bar of a preview window: close / minimise / zoom dots (`danger`, `warning`,
/// `success` tokens; only close reacts), title, panel-specific trailing actions. Returns (close, drag delta).
fn window_title_bar(ui: &mut Ui, panel: PlannedPanel, width: f32) -> (bool, egui::Vec2) {
    let nf = neoflash_ui::palette(ui.ctx());
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(width, WINDOW_TITLE_BAR), egui::Sense::hover());
    let drag = ui.interact(
        rect,
        ui.id().with("planned-window-drag"),
        egui::Sense::drag(),
    );
    let mut close = false;
    let mut x = rect.left() + space::S4;
    for (i, color) in [nf.danger_default, nf.warning_default, nf.success_default]
        .into_iter()
        .enumerate()
    {
        let c = egui::pos2(x + 6.0, rect.center().y);
        ui.painter().circle_filled(c, 6.0, color);
        if i == 0 {
            let hit = egui::Rect::from_center_size(c, egui::vec2(16.0, 16.0));
            let r = ui.interact(
                hit,
                ui.id().with("planned-window-close"),
                egui::Sense::click(),
            );
            r.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Close preview")
            });
            close = c::tooltip(r, "Close preview", None).clicked();
        }
        x += 20.0;
    }
    ui.painter().text(
        egui::pos2(x + space::S2, rect.center().y),
        egui::Align2::LEFT_CENTER,
        match panel {
            PlannedPanel::TestMovie => "Ideas in motion — Test Movie",
            other => other.title(),
        },
        neoflash_ui::typography::font(TextStyle::BodyStrong),
        nf.text_primary,
    );
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(space::S3, 0.0)))
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
        |ui| {
            if panel == PlannedPanel::TestMovie {
                ui.scope(|ui| {
                    ui.disable();
                    super::planned_documents::test_movie_title_actions(ui);
                });
            }
        },
    );
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        egui::Stroke::new(1.0_f32, nf.border_subtle),
    );
    if drag.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    (close, drag.drag_delta())
}

/// Banner "Planned feature · Preview — not available in this build.": NF2 Badge (Warning) + one line.
pub(crate) fn banner(ui: &mut Ui) {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.horizontal_wrapped(|ui| {
        ui.add(c::Badge::new("Planned feature", c::BadgeType::Warning));
        ui.label(
            rich("Preview — not available in this build.", TextStyle::Small)
                .color(nf.text_secondary),
        );
    });
}

/// Preview body. Returns true when the body closes its window (Preferences Save / Cancel).
fn body(ui: &mut Ui, panel: PlannedPanel, state: &mut PlannedState) -> bool {
    use super::planned_dialogs::PreferencesAction;
    use PlannedPanel as P;
    // Pages with preview navigation disable their own content; everything else is disabled here.
    match panel {
        P::Preferences => {
            let action = super::planned_dialogs::preferences(
                ui,
                &mut state.preferences_page,
                &mut state.theme_draft,
            );
            if action == Some(PreferencesAction::Save) {
                state.theme_save = state.theme_draft;
            }
            return action.is_some();
        }
        P::ExportImage => {
            super::planned_dialogs::export_image(ui, &mut state.export_format);
            return false;
        }
        _ => {}
    }
    ui.scope(|ui| {
        ui.disable();
        match panel {
            P::Debugger => super::planned_panels::debugger(ui),
            P::CodeSnippets => super::planned_panels::code_snippets(ui),
            P::CompilerErrors => super::planned_panels::compiler_errors(ui),
            P::PropertiesFilters => super::planned_panels::properties_filters(ui),
            P::PublishSettings => super::planned_dialogs::publish_settings(ui),
            P::ExportVideo => super::planned_dialogs::export_video(ui),
            P::ExportAudio => super::planned_dialogs::export_audio(ui),
            P::ImportVideo => super::planned_dialogs::import_video(ui),
            P::SoundProperties => super::planned_dialogs::sound_properties(ui),
            P::BitmapFill => super::planned_color::bitmap_fill(ui),
            P::PropertiesOvalPrimitive => super::planned_drawing::properties_oval(ui),
            P::PropertiesRectPrimitive => super::planned_drawing::properties_rect(ui),
            P::DecoTool => super::planned_drawing::deco_tool(ui),
            P::SprayBrush => super::planned_drawing::spray_brush(ui),
            P::BoneProperties => super::planned_drawing::bone(ui),
            P::GraphicProperties => super::planned_documents::graphic_properties(ui),
            P::LayerProperties => super::planned_documents::layer_properties(ui),
            P::StrokeStyle => super::planned_documents::stroke_style(ui),
            P::FontEmbedding => super::planned_documents::font_embedding(ui),
            P::TestMovie => super::planned_documents::test_movie(ui),
            P::FindReplace => super::planned_documents::find_replace(ui),
            P::MovieExplorer => super::planned_documents::movie_explorer(ui),
            P::SymbolsPopover => super::planned_documents::symbols_popover(ui),
            P::Preferences | P::ExportImage => {}
        }
    });
    false
}

// ── Shared display helpers: NF2 components showing fixed sample values (never project data). ──

pub(crate) fn num(
    ui: &mut Ui,
    value: f64,
    label: &str,
    prefix: &str,
    unit: &str,
    decimals: usize,
    width: f32,
) {
    let mut v = value;
    ui.add(
        c::Field::new(&mut v, label)
            .prefix(prefix)
            .unit(unit)
            .decimals(decimals)
            .width(width),
    );
}

pub(crate) fn stepper(
    ui: &mut Ui,
    value: f64,
    label: &str,
    prefix: &str,
    unit: &str,
    decimals: usize,
    width: f32,
) {
    let mut v = value;
    ui.add(
        c::Field::new(&mut v, label)
            .prefix(prefix)
            .unit(unit)
            .decimals(decimals)
            .stepper()
            .width(width),
    );
}

pub(crate) fn text(ui: &mut Ui, value: &str, prefix: &str, width: f32) {
    let mut s = value.to_owned();
    ui.add(
        c::TextField::new(&mut s, if prefix.is_empty() { value } else { prefix })
            .prefix(prefix)
            .width(width),
    );
}

pub(crate) fn select(ui: &mut Ui, label: &str, width: f32) {
    c::Select::new(ui.next_auto_id(), label)
        .width(width)
        .show(ui, |_| {});
}

pub(crate) fn check(ui: &mut Ui, on: bool, label: &str) {
    let mut v = on;
    ui.add(c::Checkbox::new(&mut v, label));
}

pub(crate) fn switch(ui: &mut Ui, on: bool, label: &str) {
    let mut v = on;
    ui.add(c::Switch::new(&mut v, label));
}

pub(crate) fn slider(ui: &mut Ui, value: f64, label: &str, width: f32) {
    let mut v = value;
    ui.add(c::Slider::new(&mut v, 0.0..=100.0, label).width(width));
}

pub(crate) fn segmented(ui: &mut Ui, options: &[&str], selected: usize) {
    let mut v = selected;
    let opts: Vec<(usize, &str)> = options.iter().copied().enumerate().collect();
    c::Segmented::show(ui, &mut v, &opts, false);
}

pub(crate) fn caption(ui: &mut Ui, s: &str) {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.label(rich(s, TextStyle::Small).color(nf.text_secondary));
}

pub(crate) fn mono(ui: &mut Ui, s: &str) {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.label(rich(s, TextStyle::MonoSmall).color(nf.text_secondary));
}

pub(crate) fn body_text(ui: &mut Ui, s: &str) {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.label(rich(s, TextStyle::Body).color(nf.text_primary));
}

/// Label column of a dialog form row (Preferences, Document settings).
pub(crate) fn form_label(ui: &mut Ui, s: &str, width: f32) {
    let nf = neoflash_ui::palette(ui.ctx());
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(width, metrics::TOGGLE_ROW), egui::Sense::hover());
    ui.painter().text(
        rect.left_center(),
        egui::Align2::LEFT_CENTER,
        s,
        neoflash_ui::typography::font(TextStyle::Body),
        nf.text_secondary,
    );
}

/// Properties type header: 48 × 48 type-icon tile, title, subtitle (Properties panels).
pub(crate) fn type_header(ui: &mut Ui, icon: neoflash_ui::Icon, title: &str, subtitle: &str) {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.horizontal(|ui| {
        let (tile, _) = ui.allocate_exact_size(egui::vec2(48.0, 48.0), egui::Sense::hover());
        ui.painter().rect_filled(
            tile,
            egui::CornerRadius::same(radius::LG as u8),
            nf.bg_field,
        );
        neoflash_ui::icons::paint(ui.painter(), ui.ctx(), tile, icon, nf.text_secondary);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.add_space(space::S1);
            ui.label(rich(title, TextStyle::BodyStrong).color(nf.text_primary));
            ui.label(rich(subtitle, TextStyle::Small).color(nf.text_secondary));
        });
    });
}

/// Dialog footer: optional left caption/ghost action, right-aligned buttons (last = primary).
pub(crate) fn footer(ui: &mut Ui, left: &str, ghost: Option<&str>, buttons: &[&str]) {
    let _ = footer_clicked(ui, left, ghost.map(|g| (g, true)), buttons);
}

/// Footer button clicked in [`footer_clicked`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FooterClick {
    Ghost,
    /// Index into `buttons`.
    Button(usize),
}

/// [`footer`] that reports clicks; `ghost` carries its enabled state.
pub(crate) fn footer_clicked(
    ui: &mut Ui,
    left: &str,
    ghost: Option<(&str, bool)>,
    buttons: &[&str],
) -> Option<FooterClick> {
    let nf = neoflash_ui::palette(ui.ctx());
    let mut clicked = None;
    ui.add_space(space::S2);
    let r = ui.available_rect_before_wrap();
    ui.painter().hline(
        r.x_range(),
        r.top() - space::S1,
        egui::Stroke::new(1.0_f32, nf.border_subtle),
    );
    ui.add_space(space::S2);
    ui.horizontal(|ui| {
        if let Some((g, enabled)) = ghost
            && ui.add_enabled(enabled, c::Button::ghost(g)).clicked()
        {
            clicked = Some(FooterClick::Ghost);
        }
        if !left.is_empty() {
            caption(ui, left);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            for (i, b) in buttons.iter().rev().enumerate() {
                let response = ui.add(if i == 0 {
                    c::Button::primary(b)
                } else {
                    c::Button::new(b)
                });
                if response.clicked() {
                    clicked = Some(FooterClick::Button(buttons.len() - 1 - i));
                }
            }
        });
    });
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(ctx: &egui::Context, theme: neoflash_ui::Theme, frames: usize) -> usize {
        ctx.set_fonts(neoflash_ui::typography::font_definitions());
        neoflash_ui::theme::install(ctx, theme);
        let mut shapes = 0;
        for _ in 0..frames {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(2400.0, 1600.0),
                )),
                ..Default::default()
            };
            let out = ctx.run(input, show_planned_panels);
            shapes = out.shapes.len();
        }
        shapes
    }

    #[test]
    fn every_preview_paints_in_both_themes() {
        for theme in [neoflash_ui::Theme::Dark, neoflash_ui::Theme::Light] {
            for panel in PlannedPanel::ALL {
                let ctx = egui::Context::default();
                let empty = render(&ctx, theme, 1);
                set_open(&ctx, panel, true);
                let painted = render(&ctx, theme, 2);
                assert!(
                    painted > empty + 10,
                    "{panel:?} painted only {painted} shapes"
                );
                assert!(is_open(&ctx, panel));
            }
        }
    }

    #[test]
    fn preferences_pages_and_export_formats_paint() {
        let ctx = egui::Context::default();
        set_open(&ctx, PlannedPanel::Preferences, true);
        set_open(&ctx, PlannedPanel::ExportImage, true);
        for page in 0..super::super::planned_dialogs::PREFERENCE_PAGES.len() {
            let mut state = planned_state(&ctx);
            state.preferences_page = page;
            state.export_format = page % 5;
            store_state(&ctx, state);
            assert!(render(&ctx, neoflash_ui::Theme::Dark, 1) > 50);
        }
    }

    #[test]
    fn every_chrome_kind_is_covered() {
        let kinds = |f: fn(Chrome) -> bool| PlannedPanel::ALL.iter().any(|p| f(p.chrome()));
        assert!(kinds(|c| matches!(c, Chrome::Panel(_))));
        assert!(kinds(|c| c == Chrome::Dialog));
        assert!(kinds(|c| c == Chrome::Popover));
        assert!(kinds(|c| c == Chrome::Window));
        // Titles are unique so the Window ▸ Planned panels menu stays unambiguous.
        let mut titles: Vec<_> = PlannedPanel::ALL.iter().map(|p| p.title()).collect();
        titles.sort_unstable();
        titles.dedup();
        assert_eq!(titles.len(), PlannedPanel::ALL.len());
    }

    #[test]
    fn previews_leave_the_project_unchanged() {
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, neoflash_core::Project::blank());
        let before = editor.project.clone();
        for panel in PlannedPanel::ALL {
            set_open(&ctx, panel, true);
        }
        for _ in 0..2 {
            let _ = ctx.run(egui::RawInput::default(), |ctx| editor.planned_panels(ctx));
        }
        assert_eq!(editor.project, before);
        assert_eq!(planned_state(&ctx).open.len(), PlannedPanel::ALL.len());
        for panel in PlannedPanel::ALL {
            set_open(&ctx, panel, false);
        }
        assert!(planned_state(&ctx).open.is_empty());
    }
}
