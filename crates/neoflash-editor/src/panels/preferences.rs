//! Preferences dialog (`Dialog / Preferences`, DL-01 / W3.6; v3 composition `S.preferences`),
//! opened by Edit ▸ Preferences… (⌘U) and, on the Shortcuts page, Edit ▸ Keyboard shortcuts… (⌥⇧⌘K).
//!
//! One size for every category (`dialog_l()` × `dialog_l_height()` of the density): a category
//! sidebar, one scrolling page on the form grid (label column + controls at their natural width)
//! and a one-row footer. Options are checkboxes; only Autosave is a switch.
//!
//! Values are editor preferences (`WorkspacePrefs`), never document data. Edits live on a draft:
//! the theme previews live while the dialog is open; `Save` / `Enter` applies and persists the draft,
//! `Cancel` / `Esc` / ✕ restores the previous theme and discards it; `Reset to defaults` resets the
//! current page. Controls whose engine or NF2 design role is not yet defined stay disabled (see
//! `docs/NF2-WIRING-PLAN.md` W3.6/W3.7 and `docs/NF2-REQUESTS.md`).
use super::dialog_helpers::{
    DialogShell, PageArea, fixed_dialog_size, form_block, form_hint, form_row as row,
    group_caption as section, nav_row,
};
use super::planned::{body_text, caption, select};
use crate::app::Editor;
use crate::build_info::BuildIdentity;
use crate::theme::{ScrollTheme as _, ThemeMode};
use crate::updater::{
    AvailableUpdate, UpdateState, UpdaterController, format_byte_progress, format_last_checked,
};
use crate::workspace::{
    ClipboardPrefs, CodeEditorPrefs, DrawingPrefs, FramePreview, GeneralPrefs, PublishPrefs,
    PublishTarget, ShortcutPrefs, TextPrefs, TimelinePrefs, TimelineView, UpdateChannel,
    UpdatePrefs, WorkspacePrefs,
};
use eframe::egui::{self, Color32, Ui};
use neoflash_export::AudioExportPolicy;
use neoflash_ui::components as c;
use neoflash_ui::icons::{self, i16};
use neoflash_ui::metrics;
use neoflash_ui::tokens::{TextStyle, radius, space};
use neoflash_ui::typography::rich;

pub(crate) const PREFERENCE_PAGES: [&str; 10] = [
    "General",
    "Code editor",
    "Timeline",
    "Drawing",
    "Text",
    "Clipboard",
    "Shortcuts",
    "Publish",
    "AI",
    "Updates",
];
pub(crate) const PAGE_GENERAL: usize = 0;
pub(crate) const PAGE_CODE_EDITOR: usize = 1;
pub(crate) const PAGE_TIMELINE: usize = 2;
pub(crate) const PAGE_DRAWING: usize = 3;
pub(crate) const PAGE_TEXT: usize = 4;
pub(crate) const PAGE_CLIPBOARD: usize = 5;
pub(crate) const PAGE_SHORTCUTS: usize = 6;
pub(crate) const PAGE_PUBLISH: usize = 7;
pub(crate) const PAGE_AI: usize = 8;
pub(crate) const PAGE_UPDATES: usize = 9;

/// Editable copy of the wired preferences while the dialog is open.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PreferencesDraft {
    pub(crate) page: usize,
    /// A newly opened or selected category starts at the top before its first layout.
    displayed_page: Option<usize>,
    /// Theme to restore on Cancel (the draft theme is previewed live).
    pub(crate) original_theme: ThemeMode,
    pub(crate) theme: ThemeMode,
    /// Density profile; previewed live while the dialog is open (the editor applies the draft's).
    pub(crate) density: crate::workspace::DensityPref,
    pub(crate) ui_scale: f32,
    pub(crate) undo_levels: f64,
    pub(crate) shift_select: bool,
    pub(crate) timeline: TimelineView,
    pub(crate) onion: [f64; 2],
    pub(crate) default_ease: neoflash_core::Easing,
    pub(crate) text_font: String,
    pub(crate) text_size: f64,
    pub(crate) paste_in_place: bool,
    /// System font families for the Text ▸ Font menu (snapshot when the dialog opened).
    pub(crate) font_families: Vec<String>,
    /// Page groups, copied back as a whole on Save (`WorkspacePrefs` fields of the same name).
    pub(crate) general: GeneralPrefs,
    pub(crate) code_editor: CodeEditorPrefs,
    pub(crate) timeline_prefs: TimelinePrefs,
    pub(crate) drawing: DrawingPrefs,
    pub(crate) text_prefs: TextPrefs,
    pub(crate) clipboard_prefs: ClipboardPrefs,
    pub(crate) publish: PublishPrefs,
    pub(crate) sequence_import_options: neoflash_import::ImageSequenceImportOptions,
    pub(crate) shortcuts: ShortcutPrefs,
    pub(crate) ai: crate::ai_connections::AiPrefs,
    pub(crate) selected_ai_profile: Option<String>,
    pub(crate) removed_ai_secrets: Vec<String>,
    pub(crate) has_ready_ai: bool,
    pub(crate) updates: UpdatePrefs,
    /// Stage view settings edited by Drawing ▸ Snap to pixels (`StageView::prefs`, one state).
    pub(crate) pixels_snap: bool,
}

impl PreferencesDraft {
    fn from_prefs(
        prefs: &WorkspacePrefs,
        page: usize,
        font_families: Vec<String>,
        pixels_snap: bool,
        has_ready_ai: bool,
    ) -> Self {
        let selected_ai_profile = prefs
            .ai
            .default_profile_id
            .clone()
            .or_else(|| prefs.ai.profiles.first().map(|p| p.id.clone()));
        Self {
            page,
            displayed_page: None,
            original_theme: prefs.theme,
            theme: prefs.theme,
            density: prefs.density,
            ui_scale: prefs.ui_scale,
            undo_levels: prefs.undo_levels as f64,
            shift_select: prefs.shift_select,
            timeline: prefs.timeline,
            onion: prefs.onion_range.map(f64::from),
            default_ease: prefs.default_ease,
            text_font: prefs.text_font.clone(),
            text_size: prefs.text_size,
            paste_in_place: prefs.paste_in_place,
            font_families,
            general: prefs.general.clone(),
            code_editor: prefs.code_editor.clone(),
            timeline_prefs: prefs.timeline_prefs.clone(),
            drawing: prefs.drawing.clone(),
            text_prefs: prefs.text_prefs.clone(),
            clipboard_prefs: prefs.clipboard_prefs.clone(),
            publish: prefs.publish.clone(),
            sequence_import_options: prefs.sequence_import_options.clone(),
            shortcuts: prefs.shortcuts.clone(),
            ai: prefs.ai.clone(),
            selected_ai_profile,
            removed_ai_secrets: Vec::new(),
            has_ready_ai,
            updates: prefs.updates.clone(),
            pixels_snap,
        }
    }

    /// `Reset to defaults` restores the current page only; pages without wired settings have none.
    fn reset_page(&mut self) {
        let defaults = WorkspacePrefs::default();
        match self.page {
            PAGE_GENERAL => {
                self.theme = defaults.theme;
                self.density = defaults.density;
                self.ui_scale = defaults.ui_scale;
                self.undo_levels = defaults.undo_levels as f64;
                self.shift_select = defaults.shift_select;
                // Recent files and open-session tab history are history, not settings.
                let recent = std::mem::take(&mut self.general.recent_files);
                let session_files = std::mem::take(&mut self.general.session_files);
                let active_session_file = self.general.active_session_file.take();
                self.general = GeneralPrefs {
                    recent_files: recent,
                    session_files,
                    active_session_file,
                    ..defaults.general
                };
            }
            PAGE_CODE_EDITOR => {
                // Keep the Actions panel's sidebar collapse state across Code Editor page resets.
                let actions_sidebar = self.code_editor.actions_sidebar;
                self.code_editor = CodeEditorPrefs {
                    actions_sidebar,
                    ..defaults.code_editor
                };
            }
            PAGE_DRAWING => {
                self.drawing = defaults.drawing;
                self.pixels_snap = false;
            }
            PAGE_SHORTCUTS => self.shortcuts = defaults.shortcuts,
            PAGE_PUBLISH => {
                self.publish = defaults.publish;
                self.sequence_import_options = defaults.sequence_import_options;
            }
            PAGE_TIMELINE => {
                let t = TimelineView::default();
                self.timeline.preview = t.preview;
                self.timeline.short_rows = t.short_rows;
                self.timeline.compact_rows = t.compact_rows;
                self.timeline.show_seconds = t.show_seconds;
                self.timeline.snap_playhead = t.snap_playhead;
                self.onion = defaults.onion_range.map(f64::from);
                self.default_ease = defaults.default_ease;
                self.timeline_prefs = defaults.timeline_prefs;
            }
            PAGE_TEXT => {
                self.text_font = defaults.text_font;
                self.text_size = defaults.text_size;
                self.text_prefs = defaults.text_prefs;
            }
            PAGE_CLIPBOARD => {
                self.paste_in_place = defaults.paste_in_place;
                self.clipboard_prefs = defaults.clipboard_prefs;
            }
            PAGE_AI => {
                for p in &self.ai.profiles {
                    if !self.removed_ai_secrets.contains(&p.id) {
                        self.removed_ai_secrets.push(p.id.clone());
                    }
                }
                self.ai = defaults.ai;
                self.selected_ai_profile = None;
                self.has_ready_ai = false;
            }
            PAGE_UPDATES => {
                let last_check = self.updates.last_check_unix;
                self.updates = UpdatePrefs {
                    last_check_unix: last_check,
                    ..defaults.updates
                };
            }
            _ => {}
        }
    }

    fn page_has_settings(&self) -> bool {
        true
    }
}

fn disabled(ui: &mut Ui, content: impl FnOnce(&mut Ui)) {
    ui.scope(|ui| {
        ui.disable();
        content(ui);
    });
}

/// On/off option of a page: a checkbox bound to the draft.
fn option(ui: &mut Ui, value: &mut bool, label: &str) {
    ui.add(c::Checkbox::new(value, label));
}

/// Checkbox of an option that is not wired yet; call inside [`disabled`].
fn fixed_option(ui: &mut Ui, on: bool, label: &str) {
    let mut value = on;
    ui.add(c::Checkbox::new(&mut value, label));
}

#[allow(clippy::too_many_arguments)]
fn bound_stepper(
    ui: &mut Ui,
    value: &mut f64,
    label: &str,
    prefix: &str,
    range: std::ops::RangeInclusive<f64>,
    width: f32,
) {
    ui.add(
        c::Field::new(value, label)
            .prefix(prefix)
            .decimals(0)
            .range(range)
            .stepper()
            .width(width),
    );
}

impl Editor {
    /// Open Preferences on `page` (a new draft from the saved preferences).
    pub(crate) fn open_preferences(&mut self, page: usize) {
        let has_ready_ai = self.has_ready_ai_connection();
        self.preferences = Some(PreferencesDraft::from_prefs(
            &self.workspace,
            page.min(PREFERENCE_PAGES.len() - 1),
            self.fonts.families.clone(),
            self.stage_view.prefs.pixels_snap,
            has_ready_ai,
        ));
    }

    /// Apply and persist a confirmed draft (Save).
    pub(crate) fn save_preferences(&mut self, ctx: &egui::Context, draft: &PreferencesDraft) {
        for removed_id in &draft.removed_ai_secrets {
            if draft.ai.profile(removed_id).is_none() {
                let _ = self.ai_manager.delete_secret(removed_id);
            }
        }
        self.set_theme(ctx, draft.theme);
        let w = &mut self.workspace;
        if draft.ui_scale.is_finite() {
            w.ui_scale = draft.ui_scale;
        }
        if draft.undo_levels.is_finite() {
            w.undo_levels = draft.undo_levels.round().clamp(1.0, 10_000.0) as u32;
        }
        w.shift_select = draft.shift_select;
        w.timeline.preview = draft.timeline.preview;
        w.timeline.short_rows = draft.timeline.short_rows;
        w.timeline.compact_rows = draft.timeline.compact_rows;
        w.timeline.show_seconds = draft.timeline.show_seconds;
        w.timeline.snap_playhead = draft.timeline.snap_playhead;
        // A new density brings its own layer-row preset and layer-column width (after the
        // Timeline page's row choice, which still described the previous profile).
        if draft.density != w.density {
            w.set_density(draft.density);
        }
        w.onion_range = draft.onion.map(|n| {
            if n.is_finite() {
                n.round().clamp(0.0, 100.0) as u32
            } else {
                0
            }
        });
        w.default_ease = draft.default_ease;
        w.text_font = draft.text_font.clone();
        if draft.text_size.is_finite() {
            w.text_size = draft.text_size;
        }
        w.paste_in_place = draft.paste_in_place;
        w.general = draft.general.clone();
        w.code_editor = draft.code_editor.clone();
        w.timeline_prefs = draft.timeline_prefs.clone();
        w.drawing = draft.drawing.clone();
        w.text_prefs = draft.text_prefs.clone();
        w.clipboard_prefs = draft.clipboard_prefs.clone();
        w.publish = draft.publish.clone();
        w.sequence_import_options = draft.sequence_import_options.clone();
        w.shortcuts = draft.shortcuts.clone();
        w.ai = draft.ai.clone();
        let preserved_last_check = w.updates.last_check_unix;
        w.updates = draft.updates.clone();
        w.updates.last_check_unix = match (preserved_last_check, draft.updates.last_check_unix) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        // Snap to pixels is the one stage view setting: change it now and in the saved view defaults.
        w.view_defaults
            .get_or_insert_with(Default::default)
            .pixels_snap = draft.pixels_snap;
        w.sanitize();
        self.stage_view.prefs.pixels_snap = draft.pixels_snap;
        self.apply_editor_preferences(ctx);
        self.sync_session_tabs_to_prefs();
        self.request_workspace_persist();
    }

    pub(crate) fn preferences_dialog(&mut self, ctx: &egui::Context) {
        self.poll_updater(ctx, false);
        let Some(mut draft) = self.preferences.clone() else {
            clear_shortcuts_ui_state(ctx);
            return;
        };
        let installed_ext_count = self.installed_extensions().len();
        if draft.page != PAGE_SHORTCUTS {
            cancel_shortcut_recording(ctx);
        }
        let was_recording = is_recording_shortcut(ctx, draft.page);
        let kb = super::dialog_helpers::DialogKeyboardState::capture(ctx);
        let p = neoflash_ui::palette(ctx);
        let mut close_cancel = false;
        let mut close_save = false;
        let modal_id = egui::Id::new("preferences");
        // v3: one size for every category; pages scroll inside it and the footer never moves.
        let size = fixed_dialog_size(ctx, metrics::dialog_l(), metrics::dialog_l_height());
        let modal = egui::Modal::new(modal_id)
            .area(
                egui::Modal::default_area(modal_id)
                    .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, space::s5())),
            )
            .frame(c::Surface::Dialog.frame(ctx))
            .backdrop_color(p.overlay)
            .show(ctx, |ui| {
                let shell = DialogShell::show(ui, "Preferences", size, metrics::sidebar());
                close_cancel |= shell.close;
                shell.sidebar(ui, |ui| {
                    for (i, (name, icon)) in PREFERENCE_PAGES.iter().zip(PAGE_ICONS).enumerate() {
                        if nav_row(ui, icon, name, draft.page == i, None).clicked() {
                            draft.page = i;
                        }
                    }
                });
                let entering_page = draft.displayed_page.replace(draft.page) != Some(draft.page);
                shell.page_with_scroll_reset(ui, "preferences-body", entering_page, |ui, area| {
                    match draft.page {
                        PAGE_GENERAL => prefs_general(ui, &mut draft),
                        PAGE_CODE_EDITOR => prefs_code_editor(ui, &mut draft),
                        PAGE_TIMELINE => prefs_timeline(ui, &mut draft),
                        PAGE_DRAWING => prefs_drawing(ui, &mut draft),
                        PAGE_TEXT => prefs_text(ui, &mut draft),
                        PAGE_CLIPBOARD => prefs_clipboard(ui, &mut draft),
                        PAGE_SHORTCUTS => prefs_shortcuts(ui, area, &mut draft),
                        PAGE_PUBLISH => prefs_publish(ui, &mut draft),
                        PAGE_AI => prefs_ai(
                            ui,
                            area,
                            &mut draft,
                            &mut self.ai_manager,
                            &mut self.ai_connection_modal,
                        ),
                        _ => prefs_updates(ui, &mut draft, &mut self.updater, installed_ext_count),
                    }
                });
                shell.footer(
                    ui,
                    |ui| {
                        if ui
                            .add_enabled(
                                draft.page_has_settings(),
                                c::Button::ghost("Reset to defaults"),
                            )
                            .clicked()
                        {
                            draft.reset_page();
                            if draft.page == PAGE_SHORTCUTS {
                                cancel_shortcut_recording(ctx);
                            }
                        }
                    },
                    |ui| {
                        close_save |= ui.add(c::Button::primary("Save")).clicked();
                        close_cancel |= ui.add(c::Button::new("Cancel")).clicked();
                    },
                );
            });
        if !close_cancel && !close_save && !was_recording && self.ai_connection_modal.is_none() {
            match kb.resolve(ctx, &modal, true) {
                super::dialog_helpers::DialogKeyboardOutcome::Cancel => close_cancel = true,
                super::dialog_helpers::DialogKeyboardOutcome::Confirm => close_save = true,
                super::dialog_helpers::DialogKeyboardOutcome::None => {}
            }
        }
        // Live theme & density preview: the stage and panels behind the dialog follow the draft.
        let shown = ThemeMode::current(ctx);
        if close_cancel {
            clear_shortcuts_ui_state(ctx);
            self.preferences = None;
            draft.original_theme.apply(ctx);
            self.apply_density(ctx);
        } else if close_save {
            clear_shortcuts_ui_state(ctx);
            self.preferences = None;
            self.save_preferences(ctx, &draft);
            self.apply_density(ctx);
        } else {
            if draft.theme != shown {
                draft.theme.apply(ctx);
            }
            self.preferences = Some(draft);
            self.apply_density(ctx);
        }
    }
}

/// Sidebar icons, in the order of [`PREFERENCE_PAGES`].
const PAGE_ICONS: [neoflash_ui::Icon; 10] = [
    i16::SETTINGS,
    i16::CODE,
    i16::FILM,
    i16::PEN,
    i16::TEXT,
    i16::PASTE,
    i16::KEYBOARD,
    i16::PUBLISH,
    i16::WAND,
    i16::RECENT,
];

fn prefs_general(ui: &mut Ui, draft: &mut PreferencesDraft) {
    let nf = neoflash_ui::palette(ui.ctx());
    section(ui, "Interface");
    row(ui, "Theme", |ui| {
        c::Select::options(
            ui,
            "prefs-theme",
            &mut draft.theme,
            &ThemeMode::ALL.map(|theme| (theme, theme.label())),
            metrics::field_m(),
        );
        // Following the system theme is not wired yet (NF2-REQUESTS R-012).
        disabled(ui, |ui| {
            let mut follow = false;
            c::not_available(
                ui.add(c::Checkbox::new(&mut follow, "Follow system")),
                "Follow the system theme",
            );
        });
    });
    row(ui, "Density", |ui| {
        c::Segmented::show(
            ui,
            &mut draft.density,
            &crate::workspace::DensityPref::ALL.map(|d| (d, d.label())),
            false,
        );
    });
    row(ui, "UI scale", |ui| {
        c::Select::options(
            ui,
            "prefs-ui-scale",
            &mut draft.ui_scale,
            &WorkspacePrefs::UI_SCALES,
            metrics::field_s(),
        );
    });
    disabled(ui, |ui| {
        row(ui, "Accent", |ui| {
            for (i, color) in [
                nf.accent_default,
                nf.success_default,
                nf.brand_orange,
                nf.layer_red,
                nf.syntax_keyword,
            ]
            .into_iter()
            .enumerate()
            {
                ui.add(c::SwatchCell {
                    color,
                    selected: i == 0,
                });
            }
        });
        row(ui, "Language", |ui| {
            select(ui, "English", metrics::field_m())
        });
    });
    section(ui, "Workspace");
    row(ui, "On launch", |ui| {
        c::Select::options(
            ui,
            "prefs-on-launch",
            &mut draft.general.on_launch,
            &crate::workspace::OnLaunch::OPTIONS,
            metrics::field_l(),
        );
    });
    row(ui, "Undo levels", |ui| {
        let range = WorkspacePrefs::UNDO_LEVELS;
        bound_stepper(
            ui,
            &mut draft.undo_levels,
            "Undo levels",
            "",
            *range.start() as f64..=*range.end() as f64,
            metrics::field_s(),
        );
    });
    row(ui, "Autosave", |ui| {
        // Autosave applies at once, so it is the one switch of the dialog.
        ui.add(c::Switch::new(&mut draft.general.autosave, "Every"));
        let mut minutes = f64::from(draft.general.autosave_minutes);
        ui.add_enabled(
            draft.general.autosave,
            c::Field::new(&mut minutes, "Autosave minutes")
                .unit("min")
                .decimals(0)
                .range(1.0..=120.0)
                .stepper()
                .width(metrics::field_s()),
        );
        draft.general.autosave_minutes = minutes.round().clamp(1.0, 120.0) as u32;
    });
    row(ui, "Tooltips", |ui| {
        option(
            ui,
            &mut draft.general.tooltip_shortcuts,
            "Show tooltips with shortcuts",
        )
    });
    section(ui, "Selection");
    row(ui, "Shift-select", |ui| {
        option(
            ui,
            &mut draft.shift_select,
            "Add with Shift (otherwise click adds)",
        )
    });
    disabled(ui, |ui| {
        row(ui, "Highlight", |ui| {
            select(ui, "Layer colour", metrics::field_m())
        })
    });
}

fn prefs_code_editor(ui: &mut Ui, draft: &mut PreferencesDraft) {
    const BUNDLED_MONO: &str = "Roboto Mono";
    const TAB_SIZES: [(u8, &str); 3] = [(2, "2 spaces"), (4, "4 spaces"), (8, "8 spaces")];

    let nf = neoflash_ui::palette(ui.ctx());
    section(ui, "Font");
    row(ui, "Font", |ui| {
        let is_bundled = draft.code_editor.font.trim().is_empty()
            || draft
                .code_editor
                .font
                .trim()
                .eq_ignore_ascii_case(BUNDLED_MONO);
        let current_font = if is_bundled {
            BUNDLED_MONO.to_owned()
        } else {
            draft.code_editor.font.clone()
        };
        c::Select::new("prefs-code-font", &current_font)
            .width(metrics::field_l())
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("prefs-code-font-list")
                    .max_height(320.0)
                    .show(ui, |ui| {
                        if ui
                            .add(c::MenuItem::new(BUNDLED_MONO).checked(is_bundled))
                            .clicked()
                        {
                            draft.code_editor.font.clear();
                            ui.close();
                        }
                        for family in &draft.font_families {
                            if family.eq_ignore_ascii_case(BUNDLED_MONO) {
                                continue;
                            }
                            let selected = !is_bundled && *family == current_font;
                            if ui.add(c::MenuItem::new(family).checked(selected)).clicked() {
                                draft.code_editor.font = family.clone();
                                ui.close();
                            }
                        }
                    });
            });
        let mut font_size = f64::from(draft.code_editor.font_size);
        if ui
            .add(
                c::Field::new(&mut font_size, "Font size")
                    .prefix("Size")
                    .decimals(0)
                    .range(8.0..=32.0)
                    .step(1.0)
                    .stepper()
                    .width(metrics::field_s()),
            )
            .changed()
        {
            draft.code_editor.font_size = font_size.clamp(8.0, 32.0) as f32;
        }
    });
    row(ui, "Line height", |ui| {
        let mut line_height = f64::from(draft.code_editor.line_height);
        if ui
            .add(
                c::Field::new(&mut line_height, "Line height")
                    .decimals(1)
                    .range(1.0..=3.0)
                    .step(0.1)
                    .stepper()
                    .width(metrics::field_s()),
            )
            .changed()
        {
            draft.code_editor.line_height = line_height.clamp(1.0, 3.0) as f32;
        }
    });
    section(ui, "Editing");
    row(ui, "Display", |ui| {
        option(ui, &mut draft.code_editor.line_numbers, "Line numbers");
        option(ui, &mut draft.code_editor.word_wrap, "Word wrap");
        option(
            ui,
            &mut draft.code_editor.highlight_current_line,
            "Highlight current line",
        );
    });
    row(ui, "Typing", |ui| {
        option(
            ui,
            &mut draft.code_editor.auto_close_brackets,
            "Auto-close brackets",
        );
        option(ui, &mut draft.code_editor.format_on_save, "Format on save");
    });
    row(ui, "Hints", |ui| {
        option(ui, &mut draft.code_editor.code_hints, "Code hints");
        caption(ui, "Delay");
        let mut delay = f64::from(draft.code_editor.hint_delay_ms);
        if ui
            .add(
                c::Slider::new(&mut delay, 0.0..=1000.0, "Code hint delay")
                    .step(10.0)
                    .width(metrics::field_m()),
            )
            .changed()
        {
            draft.code_editor.hint_delay_ms = delay.round().clamp(0.0, 1000.0) as u32;
        }
        caption(ui, &format!("{} ms", draft.code_editor.hint_delay_ms));
    });
    row(ui, "Indentation", |ui| {
        let tab_label = TAB_SIZES
            .iter()
            .find(|(n, _)| *n == draft.code_editor.tab_size)
            .map(|(_, label)| (*label).to_owned())
            .unwrap_or_else(|| format!("{} spaces", draft.code_editor.tab_size));
        c::Select::new("prefs-code-tab-size", &tab_label)
            .prefix("Tab")
            .width(metrics::field_m())
            .show(ui, |ui| {
                for (size, label) in TAB_SIZES {
                    if ui
                        .add(c::MenuItem::new(label).checked(draft.code_editor.tab_size == size))
                        .clicked()
                    {
                        draft.code_editor.tab_size = size;
                        ui.close();
                    }
                }
            });
    });
    section(ui, "Colour scheme");
    row(ui, "Scheme", |ui| {
        disabled(ui, |ui| {
            select(ui, "Scheme: NF2 Night", metrics::field_m());
            let _ = ui.add(c::Button::ghost("Duplicate"));
            let _ = ui.add(c::Button::ghost("Import…"));
        });
    });
    row(ui, "Syntax colours", |ui| {
        if ui.add(c::Button::ghost("Reset colours")).clicked() {
            draft.code_editor.syntax_colors.clear();
        }
    });
    // Three columns share the page width; a cell is swatch · name · hex.
    let column_gap = space::s3();
    let column = ((ui.available_width() - 2.0 * column_gap) / 3.0).floor();
    egui::Grid::new("prefs-syntax")
        .num_columns(3)
        .min_col_width(column)
        .max_col_width(column)
        .spacing(egui::vec2(column_gap, space::s1()))
        .show(ui, |ui| {
            for (i, slot) in crate::sources::SyntaxColorSlot::ALL.into_iter().enumerate() {
                ui.push_id(slot, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = space::s2();
                        let default_color = slot.default_color(nf);
                        let color = draft.code_editor.syntax_colors.effective_color(slot, nf);
                        let swatch_resp = c::swatch(ui, color, metrics::swatch());
                        let a11y_label = format!("{} colour", slot.label());
                        let enabled = swatch_resp.enabled();
                        swatch_resp.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::ColorButton,
                                enabled,
                                &a11y_label,
                            )
                        });
                        let swatch_resp =
                            swatch_resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                        let swatch_resp = c::tooltip(swatch_resp, &a11y_label, None);
                        let [r, g, b, _] = color.to_srgba_unmultiplied();
                        let hex = format!("{r:02X}{g:02X}{b:02X}");
                        let hex_width = ui.fonts_mut(|f| {
                            f.layout_no_wrap(
                                hex.clone(),
                                neoflash_ui::typography::font(TextStyle::MonoSmall),
                                Color32::WHITE,
                            )
                            .size()
                            .x
                        });
                        let name_width =
                            (column - metrics::swatch() - hex_width - 2.0 * space::s2()).max(0.0);
                        let (name_rect, _) = ui.allocate_exact_size(
                            egui::vec2(name_width, metrics::toggle_row()),
                            egui::Sense::hover(),
                        );
                        ui.painter().with_clip_rect(name_rect).text(
                            name_rect.left_center(),
                            egui::Align2::LEFT_CENTER,
                            slot.label(),
                            neoflash_ui::typography::font(TextStyle::Body),
                            nf.text_secondary,
                        );
                        let hex_fg = if draft.code_editor.syntax_colors.get(slot).is_some() {
                            nf.text_primary
                        } else {
                            nf.text_tertiary
                        };
                        let hex_resp = ui
                            .add(
                                egui::Label::new(rich(hex, TextStyle::MonoSmall).color(hex_fg))
                                    .sense(egui::Sense::click()),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        super::dialog_helpers::track_rgba_popup(ui.ctx(), swatch_resp.id);
                        show_syntax_color_popup(
                            ui,
                            &swatch_resp,
                            swatch_resp.clicked() || hex_resp.clicked(),
                            slot,
                            &mut draft.code_editor.syntax_colors,
                            default_color,
                        );
                    });
                });
                if i % 3 == 2 {
                    ui.end_row();
                }
            }
        });
    ui.add(
        egui::Label::new(
            rich(
                "Red is reserved for errors by default. Click any swatch to customise; Reset colours restores theme defaults.",
                TextStyle::Small,
            )
            .color(nf.text_tertiary),
        )
        .wrap(),
    );
    let syn = draft.code_editor.syntax_colors.resolve_palette(nf);
    let preview_font = crate::sources::code_font_id(ui.ctx(), &draft.code_editor);
    let row_h = (preview_font.size * draft.code_editor.line_height.clamp(1.0, 3.0))
        .floor()
        .max(14.0);
    let gutter_w = if draft.code_editor.line_numbers {
        40.0
    } else {
        20.0
    };
    egui::Frame::new()
        .fill(nf.bg_field)
        .corner_radius(egui::CornerRadius::same(radius::md() as u8))
        .stroke(egui::Stroke::new(1.0_f32, nf.border_subtle))
        .inner_margin(egui::Margin::symmetric(
            space::s2() as i8,
            space::s2() as i8,
        ))
        .show(ui, |ui| {
            let preview_width = ui.available_width();
            ui.set_width(preview_width);
            let preview_right = ui.cursor().left() + preview_width;
            ui.spacing_mut().item_spacing.y = 0.0;
            type Line<'a> = (
                Option<(neoflash_ui::Icon, Color32)>,
                &'a [(&'a str, Color32)],
            );
            let lines: [Line; 5] = [
                (None, &[("// orbit setup", syn.syntax_comment)]),
                (
                    Some((i16::BREAKPOINT, syn.syntax_breakpoint)),
                    &[
                        ("var ", syn.syntax_keyword),
                        ("speed", syn.syntax_identifier),
                        (":", nf.text_secondary),
                        ("Number", syn.syntax_type),
                        (" = ", nf.text_secondary),
                        ("2.5", syn.syntax_number),
                        (";", nf.text_secondary),
                    ],
                ),
                (
                    None,
                    &[
                        ("animator", syn.syntax_identifier),
                        (".", nf.text_secondary),
                        ("play", syn.syntax_function),
                        ("();", nf.text_secondary),
                    ],
                ),
                (
                    Some((i16::WARNING, syn.syntax_warning)),
                    &[
                        ("<Keyframe ", syn.syntax_tag),
                        ("index", syn.syntax_attr),
                        ("=", nf.text_secondary),
                        ("\"0\"", syn.syntax_string),
                        ("/>", syn.syntax_tag),
                    ],
                ),
                (
                    Some((i16::ERROR, syn.syntax_error)),
                    &[
                        ("orbit_mc", syn.syntax_identifier),
                        (".", nf.text_secondary),
                        ("rotation", syn.syntax_identifier),
                        (" += ", nf.text_secondary),
                        ("speed", syn.syntax_identifier),
                        (";", nf.text_secondary),
                    ],
                ),
            ];
            for (n, (marker, parts)) in lines.iter().enumerate() {
                let band = ui.painter().add(egui::Shape::Noop);
                let sel_band = ui.painter().add(egui::Shape::Noop);
                let mut gutter_right = 0.0;
                let mut code_left = 0.0;
                let row_out = ui.horizontal(|ui| {
                    let (gutter, _) =
                        ui.allocate_exact_size(egui::vec2(gutter_w, row_h), egui::Sense::hover());
                    gutter_right = gutter.right();
                    if let Some((icon, color)) = marker {
                        icons::paint_sized(
                            ui.painter(),
                            ui.ctx(),
                            egui::Rect::from_center_size(
                                gutter.left_center() + egui::vec2(8.0, 0.0),
                                egui::vec2(12.0, 12.0),
                            ),
                            *icon,
                            12.0,
                            *color,
                        );
                    }
                    if draft.code_editor.line_numbers {
                        ui.painter().text(
                            gutter.right_center() - egui::vec2(space::s2(), 0.0),
                            egui::Align2::RIGHT_CENTER,
                            (n + 1).to_string(),
                            neoflash_ui::typography::font(TextStyle::MonoSmall),
                            nf.text_tertiary,
                        );
                    }
                    code_left = ui.cursor().left();
                    let mut job = egui::text::LayoutJob::default();
                    for (t, color) in parts.iter() {
                        let mut fmt = neoflash_ui::typography::format(TextStyle::Mono, *color);
                        fmt.font_id = preview_font.clone();
                        fmt.line_height = Some(row_h);
                        job.append(t, 0.0, fmt);
                    }
                    ui.label(job);
                });
                if n == 1 {
                    let prefix_w = ui.fonts_mut(|f| {
                        f.layout_no_wrap("var ".to_owned(), preview_font.clone(), Color32::WHITE)
                            .size()
                            .x
                    });
                    let sel_w = ui.fonts_mut(|f| {
                        f.layout_no_wrap("speed".to_owned(), preview_font.clone(), Color32::WHITE)
                            .size()
                            .x
                    });
                    let sel_rect = egui::Rect::from_min_max(
                        egui::pos2(code_left + prefix_w, row_out.response.rect.top()),
                        egui::pos2(code_left + prefix_w + sel_w, row_out.response.rect.bottom()),
                    );
                    ui.painter().set(
                        sel_band,
                        egui::Shape::rect_filled(sel_rect, 2.0, syn.syntax_selection),
                    );
                }
                if draft.code_editor.highlight_current_line && n == 2 {
                    let band_rect = egui::Rect::from_min_max(
                        egui::pos2(gutter_right, row_out.response.rect.top()),
                        egui::pos2(preview_right, row_out.response.rect.bottom()),
                    );
                    ui.painter().set(
                        band,
                        egui::Shape::rect_filled(band_rect, 0.0, syn.syntax_current_line),
                    );
                }
            }
        });
}

fn color32_to_hsb(color: Color32) -> neoflash_ui::components::color_picker::Hsb {
    let [r, g, b, a] = color.to_srgba_unmultiplied();
    let (rf, gf, bf, af) = (
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
        a as f32 / 255.0,
    );
    let max = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let d = max - min;
    let h = if d <= 1e-6 {
        0.0
    } else if (max - rf).abs() <= 1e-6 {
        ((gf - bf) / d).rem_euclid(6.0) / 6.0
    } else if (max - gf).abs() <= 1e-6 {
        ((bf - rf) / d + 2.0) / 6.0
    } else {
        ((rf - gf) / d + 4.0) / 6.0
    };
    let s = if max <= 1e-6 { 0.0 } else { d / max };
    neoflash_ui::components::color_picker::hsv(h, s, max, af)
}

fn show_syntax_color_popup(
    ui: &mut Ui,
    trigger: &egui::Response,
    clicked: bool,
    slot: crate::sources::SyntaxColorSlot,
    overrides: &mut crate::sources::SyntaxColorOverrides,
    default_color: Color32,
) {
    let ctx = ui.ctx().clone();
    let open_id = trigger.id.with("nf2-rgba-popup-open");
    let active_slot_key = egui::Id::new("nf2-prefs-active-syntax-slot");
    let mut is_open = ctx.data(|d| d.get_temp::<bool>(open_id)).unwrap_or(false);
    if is_open && ctx.data(|d| d.get_temp::<egui::Id>(active_slot_key)) != Some(open_id) {
        is_open = false;
        ctx.data_mut(|d| d.insert_temp(open_id, false));
    }
    if clicked {
        is_open = !is_open;
        ctx.data_mut(|d| {
            d.insert_temp(open_id, is_open);
            if is_open {
                d.insert_temp(active_slot_key, open_id);
            }
        });
    }
    if !is_open {
        return;
    }

    let parent_layer = ui.layer_id();
    let area_id = trigger.id.with("nf2-syntax-color-area");
    ctx.set_sublayer(
        parent_layer,
        egui::LayerId::new(egui::Order::Foreground, area_id),
    );
    let anchor = trigger.rect;
    let nf = neoflash_ui::palette(&ctx);
    let mut current = overrides
        .get(slot)
        .map(|[r, g, b, a]| Color32::from_rgba_unmultiplied(r, g, b, a))
        .unwrap_or(default_color);
    let hsb_id = trigger.id.with("nf2-syntax-hsb");
    let mut close_now = false;

    let area_resp = egui::Area::new(area_id)
        .order(egui::Order::Foreground)
        .constrain(true)
        .fixed_pos(egui::pos2(anchor.left(), anchor.bottom() + 4.0))
        .show(&ctx, |ui| {
            c::popover_frame(&ctx).show(ui, |ui| {
                let width = neoflash_ui::components::color_picker::COLOR_PICKER.x;
                ui.set_min_width(width);
                ui.set_max_width(width);
                ui.spacing_mut().item_spacing.y = space::s2();

                ui.horizontal(|ui| {
                    ui.label(rich(slot.label(), TextStyle::BodyStrong).color(nf.text_primary));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(c::Button::ghost("Reset")).clicked() {
                            overrides.set(slot, None);
                            current = default_color;
                            ui.data_mut(|d| {
                                d.remove::<neoflash_ui::components::color_picker::Hsb>(hsb_id)
                            });
                            close_now = true;
                        }
                    });
                });

                let cf = c::ColorField::new(&mut current, slot.label())
                    .width(width)
                    .show(ui);
                if cf.response.changed() {
                    overrides.set(slot, Some(current.to_srgba_unmultiplied()));
                    ui.data_mut(|d| d.insert_temp(hsb_id, color32_to_hsb(current)));
                }

                let mut hsb = ui
                    .data(|d| d.get_temp::<neoflash_ui::components::color_picker::Hsb>(hsb_id))
                    .filter(|cached| Color32::from(*cached) == current)
                    .unwrap_or_else(|| color32_to_hsb(current));
                if ui.add(c::ColorPicker { color: &mut hsb }).changed() {
                    current = Color32::from(hsb);
                    overrides.set(slot, Some(current.to_srgba_unmultiplied()));
                }
                ui.data_mut(|d| d.insert_temp(hsb_id, hsb));
            });
        });

    let popup_rect = area_resp.response.rect;
    let (pressed, pointer_pos, escape) = ctx.input(|i| {
        (
            i.pointer.any_pressed(),
            i.pointer.interact_pos(),
            i.key_pressed(egui::Key::Escape),
        )
    });
    let clicked_outside = !clicked
        && pressed
        && pointer_pos.is_some_and(|pos| {
            !popup_rect.contains(pos)
                && !anchor.contains(pos)
                && ctx.layer_id_at(pos).is_none_or(|layer| {
                    layer == parent_layer || layer.order < egui::Order::Foreground
                })
        });
    if close_now || clicked_outside || escape {
        ctx.data_mut(|d| d.insert_temp(open_id, false));
    }
}

fn prefs_timeline(ui: &mut Ui, draft: &mut PreferencesDraft) {
    let nf = neoflash_ui::palette(ui.ctx());
    section(ui, "Selection & view");
    row(ui, "Selection", |ui| {
        option(
            ui,
            &mut draft.timeline_prefs.span_selection,
            "Span-based selection",
        )
    });
    row(ui, "Frame view", |ui| {
        let prev_preview = draft.timeline.preview;
        c::Select::options(
            ui,
            "prefs-frame-view",
            &mut draft.timeline.preview,
            &[
                (FramePreview::Off, "Normal"),
                (FramePreview::Layer, "Preview"),
                (FramePreview::InContext, "Preview in context"),
            ],
            metrics::field_m(),
        );
        if draft.timeline.preview != prev_preview && draft.timeline.preview != FramePreview::Off {
            draft.timeline.short_rows = false;
            draft.timeline.compact_rows = false;
        }
        let prev_row_mode = if draft.timeline.compact_rows {
            2_u8
        } else if draft.timeline.short_rows {
            1_u8
        } else {
            0_u8
        };
        let mut row_mode = prev_row_mode;
        c::Select::options(
            ui,
            "prefs-frame-rows",
            &mut row_mode,
            &[(0, "Row: Normal"), (1, "Row: Short"), (2, "Row: Compact")],
            metrics::field_m(),
        );
        if row_mode != prev_row_mode {
            draft.timeline.preview = FramePreview::Off;
        }
        draft.timeline.short_rows = row_mode == 1;
        draft.timeline.compact_rows = row_mode == 2;
    });
    row(ui, "Ruler", |ui| {
        option(ui, &mut draft.timeline.show_seconds, "Show seconds");
        option(
            ui,
            &mut draft.timeline.snap_playhead,
            "Snap playhead to keyframes",
        );
    });
    disabled(ui, |ui| {
        row(ui, "Scenes", |ui| {
            fixed_option(ui, false, "Named anchor on each scene")
        });
    });
    section(ui, "Onion skin defaults");
    row(ui, "Range", |ui| {
        let max = WorkspacePrefs::ONION_MAX as f64;
        bound_stepper(
            ui,
            &mut draft.onion[0],
            "Onion frames before",
            "Before",
            0.0..=max,
            metrics::field_s(),
        );
        bound_stepper(
            ui,
            &mut draft.onion[1],
            "Onion frames after",
            "After",
            0.0..=max,
            metrics::field_s(),
        );
    });
    disabled(ui, |ui| {
        row(ui, "Tint", |ui| {
            caption(ui, "Before");
            ui.add(c::SwatchCell {
                color: nf.layer_blue,
                selected: true,
            });
            caption(ui, "After");
            ui.add(c::SwatchCell {
                color: nf.layer_green,
                selected: false,
            });
        });
    });
    row(ui, "Opacity falloff", |ui| {
        let mut falloff = f64::from(draft.timeline_prefs.onion_falloff);
        if ui
            .add(
                c::Slider::new(&mut falloff, 15.0..=100.0, "Opacity falloff")
                    .width(metrics::field_m()),
            )
            .changed()
        {
            draft.timeline_prefs.onion_falloff = falloff as f32;
        }
        caption(
            ui,
            &format!("{:.0} → 15 %", draft.timeline_prefs.onion_falloff),
        );
    });
    section(ui, "Animation");
    row(ui, "Auto keyframe", |ui| {
        option(
            ui,
            &mut draft.timeline_prefs.auto_keyframe,
            "Create keyframe on transform",
        )
    });
    row(ui, "Default ease", |ui| {
        c::Select::options(
            ui,
            "prefs-default-ease",
            &mut draft.default_ease,
            &WorkspacePrefs::EASES,
            metrics::field_m(),
        );
    });
    row(ui, "Tween on drag", |ui| {
        use crate::workspace::TweenOnDrag;
        c::Select::options(
            ui,
            "prefs-tween-on-drag",
            &mut draft.timeline_prefs.tween_on_drag,
            &[
                (TweenOnDrag::None, "None"),
                (TweenOnDrag::Classic, "Classic tween"),
                (TweenOnDrag::Motion, "Motion tween"),
            ],
            metrics::field_m(),
        );
    });
}

fn prefs_drawing(ui: &mut Ui, draft: &mut PreferencesDraft) {
    section(ui, "Default mode");
    row(ui, "New shapes", |ui| {
        c::Segmented::show(
            ui,
            &mut draft.drawing.object_drawing,
            &[(false, "Merge drawing"), (true, "Object drawing")],
            false,
        );
    });
    section(ui, "Pen tool");
    row(ui, "Pen", |ui| {
        option(ui, &mut draft.drawing.pen_preview, "Show pen preview");
        let mut solid_points = false;
        ui.add_enabled(
            false,
            c::Checkbox::new(&mut solid_points, "Show solid points"),
        );
    });
    row(ui, "Cursors", |ui| {
        option(ui, &mut draft.drawing.precise_cursors, "Precise cursors");
    });
    section(ui, "Recognition");
    fn tolerance(
        ui: &mut Ui,
        label: &str,
        id: &str,
        value: &mut crate::workspace::Tolerance,
        options: &[(crate::workspace::Tolerance, &str)],
    ) {
        row(ui, label, |ui| {
            c::Select::options(ui, id, value, options, metrics::field_m());
        });
    }
    tolerance(
        ui,
        "Connect lines",
        "prefs-drawing-connect-lines",
        &mut draft.drawing.connect_lines,
        &crate::workspace::Tolerance::OPTIONS,
    );
    tolerance(
        ui,
        "Smooth curves",
        "prefs-drawing-smooth-curves",
        &mut draft.drawing.smooth_curves,
        &crate::workspace::Tolerance::SMOOTH_OPTIONS,
    );
    tolerance(
        ui,
        "Lines",
        "prefs-drawing-recognise-lines",
        &mut draft.drawing.recognise_lines,
        &crate::workspace::Tolerance::OPTIONS,
    );
    tolerance(
        ui,
        "Shapes",
        "prefs-drawing-recognise-shapes",
        &mut draft.drawing.recognise_shapes,
        &crate::workspace::Tolerance::OPTIONS,
    );
    tolerance(
        ui,
        "Click accuracy",
        "prefs-drawing-click-accuracy",
        &mut draft.drawing.click_accuracy,
        &crate::workspace::Tolerance::OPTIONS,
    );
    section(ui, "Snapping");
    row(ui, "Tolerance", |ui| {
        let mut snap_tolerance = f64::from(draft.drawing.snap_tolerance);
        ui.add(
            c::Field::new(&mut snap_tolerance, "Snap tolerance")
                .unit("px")
                .decimals(0)
                .range(1.0..=32.0)
                .stepper()
                .width(metrics::field_s()),
        );
        draft.drawing.snap_tolerance = snap_tolerance as f32;
    });
    row(ui, "Snap", |ui| {
        option(ui, &mut draft.pixels_snap, "Snap to pixels");
        let mut snap_align = true;
        ui.add_enabled(false, c::Checkbox::new(&mut snap_align, "Snap align"));
    });
}

fn prefs_text(ui: &mut Ui, draft: &mut PreferencesDraft) {
    section(ui, "Defaults");
    row(ui, "Font", |ui| {
        let current = if draft.text_font.is_empty() {
            if draft.font_families.iter().any(|f| f == "Arial") {
                "Arial".to_owned()
            } else {
                "Helvetica".to_owned()
            }
        } else {
            draft.text_font.clone()
        };
        c::Select::new("prefs-text-font", &current)
            .width(metrics::field_l())
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("prefs-text-font-list")
                    .max_height(320.0)
                    .show(ui, |ui| {
                        for family in &draft.font_families {
                            if ui
                                .add(c::MenuItem::new(family).checked(*family == current))
                                .clicked()
                            {
                                draft.text_font = family.clone();
                                ui.close();
                            }
                        }
                    });
            });
        bound_stepper(
            ui,
            &mut draft.text_size,
            "Font size",
            "Size",
            WorkspacePrefs::TEXT_SIZES,
            metrics::field_s(),
        );
    });
    disabled(ui, |ui| {
        row(ui, "Anti-alias", |ui| {
            select(ui, "For readability", metrics::field_l())
        });
    });
    row(ui, "Substitution", |ui| {
        c::Select::options(
            ui,
            "prefs-font-substitution",
            &mut draft.text_prefs.substitution_warning,
            &[(true, "Show warning"), (false, "Substitute silently")],
            metrics::field_l(),
        );
    });
    section(ui, "Behaviour");
    disabled(ui, |ui| {
        row(ui, "Fonts", |ui| {
            fixed_option(ui, true, "Embed used glyphs automatically")
        });
        row(ui, "Editing", |ui| fixed_option(ui, true, "Spell check"));
        row(ui, "Vertical text", |ui| {
            fixed_option(ui, false, "Right-to-left flow");
            fixed_option(ui, true, "No kerning");
        });
        row(ui, "Input method", |ui| {
            select(ui, "System default", metrics::field_l())
        });
    });
}

fn prefs_clipboard(ui: &mut Ui, draft: &mut PreferencesDraft) {
    section(ui, "Copying");
    disabled(ui, |ui| {
        row(ui, "Bitmaps", |ui| {
            select(ui, "Match screen (144 ppi)", metrics::field_l())
        });
        row(ui, "Gradients", |ui| {
            select(ui, "Normal", metrics::field_m())
        });
        row(ui, "Other apps", |ui| {
            fixed_option(ui, true, "Copy as SVG to other apps");
            fixed_option(ui, true, "Keep layer names");
        });
    });
    section(ui, "Pasting");
    row(ui, "Default paste", |ui| {
        c::Segmented::show(
            ui,
            &mut draft.paste_in_place,
            &[(false, "In centre"), (true, "In place")],
            false,
        );
    });
    row(ui, "Vectors", |ui| {
        option(
            ui,
            &mut draft.clipboard_prefs.editable_vector_paste,
            "Import vector paste as editable paths",
        );
    });
    disabled(ui, |ui| {
        row(ui, "Bitmaps", |ui| {
            fixed_option(ui, true, "Ask before pasting large bitmaps")
        });
    });
}

#[derive(Clone, Debug, Default)]
struct ShortcutsUiState {
    search: String,
    only_assigned: bool,
    only_conflicts: bool,
    selected: Option<crate::commands::CommandId>,
    recording: Option<crate::commands::CommandId>,
    captured: Option<crate::commands::Shortcut>,
    append_mode: bool,
    scroll_to: Option<crate::commands::CommandId>,
}

impl ShortcutsUiState {
    fn stop_recording(&mut self) {
        self.recording = None;
        self.captured = None;
        self.append_mode = false;
    }
}

fn shortcuts_ui_state_id() -> egui::Id {
    egui::Id::new("prefs-shortcuts-ui-state")
}

fn shortcuts_search_focus_request_id() -> egui::Id {
    egui::Id::new("prefs-shortcuts-search-focus-request")
}

fn load_shortcuts_ui_state(ctx: &egui::Context) -> ShortcutsUiState {
    ctx.data(|d| d.get_temp::<ShortcutsUiState>(shortcuts_ui_state_id()))
        .unwrap_or_default()
}

fn store_shortcuts_ui_state(ctx: &egui::Context, state: ShortcutsUiState) {
    ctx.data_mut(|d| d.insert_temp(shortcuts_ui_state_id(), state));
}

fn clear_shortcuts_ui_state(ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove_temp::<ShortcutsUiState>(shortcuts_ui_state_id()));
}

fn cancel_shortcut_recording(ctx: &egui::Context) {
    let mut state = load_shortcuts_ui_state(ctx);
    if state.recording.is_some() {
        state.stop_recording();
        store_shortcuts_ui_state(ctx, state);
    }
}

fn is_recording_shortcut(ctx: &egui::Context, page: usize) -> bool {
    page == PAGE_SHORTCUTS && load_shortcuts_ui_state(ctx).recording.is_some()
}

fn prefs_shortcuts(ui: &mut Ui, area: PageArea, draft: &mut PreferencesDraft) {
    #[derive(Clone, Copy)]
    enum RecordedAction {
        Cancel,
        Clear,
        Capture(crate::commands::Shortcut),
    }

    let nf = neoflash_ui::palette(ui.ctx());
    let mut state = load_shortcuts_ui_state(ui.ctx());
    let was_recording = state.recording;

    if let Some(recording_cmd) = state.recording {
        let mut action = None;
        ui.ctx().input_mut(|i| {
            let mut remaining = Vec::with_capacity(i.events.len());
            for event in i.events.drain(..) {
                if action.is_some() {
                    if !matches!(event, egui::Event::Text(_)) {
                        remaining.push(event);
                    }
                    continue;
                }
                match event {
                    egui::Event::Text(_) => {}
                    egui::Event::Cut => {
                        action = Some(RecordedAction::Capture(crate::commands::Shortcut::cmd(
                            egui::Key::X,
                        )));
                    }
                    egui::Event::Copy => {
                        action = Some(RecordedAction::Capture(crate::commands::Shortcut::cmd(
                            egui::Key::C,
                        )));
                    }
                    egui::Event::Paste(_) => {
                        action = Some(RecordedAction::Capture(crate::commands::Shortcut::cmd(
                            egui::Key::V,
                        )));
                    }
                    egui::Event::Key {
                        key,
                        physical_key,
                        pressed: true,
                        modifiers,
                        ..
                    } => {
                        if key == egui::Key::Escape && crate::commands::modifiers_is_none(modifiers)
                        {
                            action = Some(RecordedAction::Cancel);
                        } else if matches!(key, egui::Key::Backspace | egui::Key::Delete)
                            && crate::commands::modifiers_is_none(modifiers)
                        {
                            action = Some(RecordedAction::Clear);
                        } else {
                            action = Some(RecordedAction::Capture(
                                crate::commands::Shortcut::from_key_event(
                                    modifiers,
                                    key,
                                    physical_key,
                                ),
                            ));
                        }
                    }
                    other => remaining.push(other),
                }
            }
            i.events = remaining;
        });
        if let Some(act) = action {
            match act {
                RecordedAction::Cancel => {
                    state.stop_recording();
                }
                RecordedAction::Clear => {
                    draft.shortcuts.set_shortcuts(recording_cmd, Vec::new());
                    state.stop_recording();
                }
                RecordedAction::Capture(chord) => {
                    state.captured = Some(chord);
                }
            }
            if let Some(focused) = ui.memory(|m| m.focused()) {
                ui.memory_mut(|m| m.surrender_focus(focused));
            }
        }
    } else if let Some(selected_cmd) = state.selected
        && !ui.ctx().wants_keyboard_input()
        && !egui::Popup::is_any_open(ui.ctx())
        && ui
            .ctx()
            .input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
    {
        state.recording = Some(selected_cmd);
        state.captured = None;
        state.append_mode = false;
    }

    section(ui, "Shortcut set");
    row(ui, "Set", |ui| {
        let active_label = draft.shortcuts.active_set_label().to_owned();
        let set_names: Vec<String> = draft.shortcuts.sets.keys().cloned().collect();
        let (set_resp, _) = c::Select::new("prefs-shortcut-set", &active_label)
            .width(metrics::field_m())
            .show(ui, |ui| {
                let is_default = draft.shortcuts.is_default_set();
                if ui
                    .add(c::MenuItem::new(ShortcutPrefs::DEFAULT_SET_NAME).checked(is_default))
                    .clicked()
                {
                    draft.shortcuts.select_set(ShortcutPrefs::DEFAULT_SET_NAME);
                    state.stop_recording();
                }
                for name in &set_names {
                    let checked = !is_default && draft.shortcuts.set == *name;
                    if ui.add(c::MenuItem::new(name).checked(checked)).clicked() {
                        draft.shortcuts.select_set(name);
                        state.stop_recording();
                    }
                }
            });
        if set_resp.clicked() {
            state.stop_recording();
        }
        if ui.add(c::Button::ghost("Duplicate")).clicked() {
            draft.shortcuts.duplicate_active_set();
            state.stop_recording();
        }
        disabled(ui, |ui| {
            c::not_available(ui.add(c::Button::ghost("Rename")), "Renaming shortcut sets");
        });
        if ui
            .add_enabled(
                !draft.shortcuts.is_default_set(),
                c::Button::ghost("Delete"),
            )
            .clicked()
        {
            draft.shortcuts.delete_active_set();
            state.stop_recording();
        }
    });

    let conflict_map = crate::commands::conflicts(&draft.shortcuts);
    let mut chord_owners: std::collections::BTreeMap<crate::commands::Shortcut, usize> =
        std::collections::BTreeMap::new();
    for &cmd in crate::commands::CommandId::ALL {
        if matches!(cmd, crate::commands::CommandId::EditSendToAi) && !draft.has_ready_ai {
            continue;
        }
        if matches!(cmd, crate::commands::CommandId::HelpSendBetaFeedback)
            && !crate::panels::beta_feedback::beta_feedback_available()
        {
            continue;
        }
        for sc in crate::commands::effective_shortcuts(&draft.shortcuts, cmd) {
            *chord_owners.entry(sc).or_insert(0) += 1;
        }
    }
    let conflict_count = chord_owners.values().filter(|&&n| n > 1).count();
    if conflict_count == 0 {
        state.only_conflicts = false;
    }

    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), neoflash_ui::metrics::control()),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            if conflict_count > 0 {
                let badge_label = if conflict_count == 1 {
                    "1 conflict".to_owned()
                } else {
                    format!("{conflict_count} conflicts")
                };
                let badge_resp = ui
                    .add(c::Badge::new(&badge_label, c::BadgeType::Warning))
                    .interact(egui::Sense::click());
                badge_resp.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::Button,
                        true,
                        state.only_conflicts,
                        &badge_label,
                    )
                });
                if badge_resp.clicked() {
                    state.only_conflicts = !state.only_conflicts;
                    state.stop_recording();
                }
            }
            if ui
                .add(c::Checkbox::new(&mut state.only_assigned, "Only assigned"))
                .changed()
            {
                state.stop_recording();
            }
            let search_w = ui.available_width();
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                let search_was_focused = ui
                    .data(|d| d.get_temp::<bool>(egui::Id::new("prefs-shortcuts-search-focused")))
                    .unwrap_or(false);
                let search_resp =
                    ui.add(c::Search::new(&mut state.search, "Search commands").width(search_w));
                if ui.ctx().data_mut(|data| {
                    data.remove_temp::<bool>(shortcuts_search_focus_request_id())
                        .unwrap_or(false)
                }) {
                    ui.memory_mut(|memory| memory.request_focus(search_resp.id.with("edit")));
                }
                let search_now_focused = ui.memory(|m| m.has_focus(search_resp.id.with("edit")));
                ui.data_mut(|d| {
                    d.insert_temp(
                        egui::Id::new("prefs-shortcuts-search-focused"),
                        search_now_focused,
                    );
                });
                if (search_now_focused && !search_was_focused) || search_resp.changed() {
                    state.stop_recording();
                }
            });
        },
    );

    let query = state.search.trim().to_lowercase();
    let any_title_or_path_match = !query.is_empty()
        && crate::commands::CommandId::ALL.iter().any(|&cmd| {
            if matches!(cmd, crate::commands::CommandId::EditSendToAi) && !draft.has_ready_ai {
                return false;
            }
            if matches!(cmd, crate::commands::CommandId::HelpSendBetaFeedback)
                && !crate::panels::beta_feedback::beta_feedback_available()
            {
                return false;
            }
            cmd.title().to_lowercase().contains(&query)
                || cmd.menu_path().to_lowercase().contains(&query)
                || cmd.row_title().to_lowercase().contains(&query)
        });
    let matches_cmd = |cmd: crate::commands::CommandId,
                       shortcuts: &ShortcutPrefs,
                       only_assigned: bool,
                       only_conflicts: bool,
                       recording: Option<crate::commands::CommandId>|
     -> bool {
        if recording == Some(cmd) {
            return true;
        }
        let effective = crate::commands::effective_shortcuts(shortcuts, cmd);
        if only_assigned && effective.is_empty() {
            return false;
        }
        if only_conflicts && !conflict_map.contains_key(&cmd) {
            return false;
        }
        if query.is_empty() {
            return true;
        }
        if cmd.title().to_lowercase().contains(&query)
            || cmd.menu_path().to_lowercase().contains(&query)
            || cmd.row_title().to_lowercase().contains(&query)
        {
            return true;
        }
        if any_title_or_path_match {
            return false;
        }
        if cmd.category().label().to_lowercase().contains(&query)
            || cmd.search_keywords().to_lowercase().contains(&query)
        {
            return true;
        }
        effective
            .iter()
            .any(|sc| sc.format().to_lowercase().contains(&query))
    };

    let mut recorder_rect: Option<egui::Rect> = None;
    let mut started_recording = false;
    let mut total_matches = 0usize;

    // The list fills the page down to the command row under it; one row per shortcut.
    let list_height = area.fill_height(
        ui,
        "prefs-shortcuts",
        metrics::control() + metrics::row_gap(),
        6.0 * metrics::row(),
    );
    let list = egui::ScrollArea::vertical()
        .id_salt("prefs-shortcuts-list")
        .max_height(list_height)
        .auto_shrink([false, false])
        .themed()
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            for category in crate::commands::CommandCategory::ALL {
                let all_in_cat: Vec<crate::commands::CommandId> = crate::commands::CommandId::ALL
                    .iter()
                    .copied()
                    .filter(|cmd| {
                        cmd.category() == category
                            && (!matches!(cmd, crate::commands::CommandId::EditSendToAi)
                                || draft.has_ready_ai)
                            && (!matches!(cmd, crate::commands::CommandId::HelpSendBetaFeedback)
                                || crate::panels::beta_feedback::beta_feedback_available())
                    })
                    .collect();
                let total = all_in_cat.len();
                let assigned = all_in_cat
                    .iter()
                    .filter(|&&cmd| {
                        !crate::commands::effective_shortcuts(&draft.shortcuts, cmd).is_empty()
                    })
                    .count();
                let matching: Vec<crate::commands::CommandId> = all_in_cat
                    .into_iter()
                    .filter(|&cmd| {
                        matches_cmd(
                            cmd,
                            &draft.shortcuts,
                            state.only_assigned,
                            state.only_conflicts,
                            state.recording,
                        )
                    })
                    .collect();
                if matching.is_empty() {
                    continue;
                }
                total_matches += matching.len();

                let salt = ("prefs-shortcuts-cat", category.label());
                if state
                    .scroll_to
                    .is_some_and(|target| target.category() == category)
                {
                    let section_id = ui.id().with(egui::Id::new(("nf2-section", salt)));
                    ui.data_mut(|d| d.insert_persisted(section_id, true));
                }

                let header_label = if query.is_empty() {
                    format!("{} · {assigned} of {total} assigned", category.label())
                } else if matching.len() == 1 {
                    format!("{} · 1 match", category.label())
                } else {
                    format!("{} · {} matches", category.label(), matching.len())
                };

                c::SectionHeader::new(&header_label)
                    .id_salt(salt)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                        let w = ui.available_width();
                        for cmd in matching {
                            if state.recording == Some(cmd) {
                                let top = ui.cursor().min;
                                let captured_caps: Vec<String> =
                                    state.captured.map(|sc| sc.key_caps()).unwrap_or_default();
                                let captured_refs: Vec<&str> =
                                    captured_caps.iter().map(String::as_str).collect();
                                let conflict_other = state.captured.and_then(|sc| {
                                    crate::commands::first_conflict_for_chord(
                                        &draft.shortcuts,
                                        cmd,
                                        sc,
                                    )
                                });
                                let mut rec = c::ShortcutRecorder::new(cmd.row_title());
                                if state.captured.is_some() {
                                    rec = rec.captured(&captured_refs);
                                }
                                if let Some(other) = conflict_other {
                                    rec = rec.conflict_with(other.menu_path());
                                }
                                let out = rec.show(ui, w);
                                let bottom = ui.cursor().min.y;
                                let r_rect =
                                    egui::Rect::from_min_max(top, egui::pos2(top.x + w, bottom));
                                recorder_rect = Some(r_rect);
                                if state.scroll_to == Some(cmd) {
                                    ui.scroll_to_rect(r_rect, Some(egui::Align::Center));
                                    state.scroll_to = None;
                                }
                                if out.accept {
                                    if let Some(chord) = state.captured {
                                        if state.append_mode {
                                            let mut list = crate::commands::effective_shortcuts(
                                                &draft.shortcuts,
                                                cmd,
                                            );
                                            if !list.contains(&chord) {
                                                list.push(chord);
                                            }
                                            draft.shortcuts.set_shortcuts(cmd, list);
                                        } else {
                                            draft.shortcuts.set_shortcuts(cmd, vec![chord]);
                                        }
                                    }
                                    state.stop_recording();
                                } else if out.cancel {
                                    state.stop_recording();
                                } else if out.go_to
                                    && let Some(other) = conflict_other
                                {
                                    if !matches_cmd(
                                        other,
                                        &draft.shortcuts,
                                        state.only_assigned,
                                        state.only_conflicts,
                                        Some(other),
                                    ) {
                                        state.search.clear();
                                        state.only_conflicts = false;
                                    }
                                    state.selected = Some(other);
                                    state.recording = Some(other);
                                    state.captured = None;
                                    state.append_mode = false;
                                    state.scroll_to = Some(other);
                                    started_recording = true;
                                }
                            } else {
                                let effective =
                                    crate::commands::effective_shortcuts(&draft.shortcuts, cmd);
                                let caps_owned: Vec<Vec<String>> = effective
                                    .iter()
                                    .map(crate::commands::Shortcut::key_caps)
                                    .collect();
                                let caps_slices: Vec<Vec<&str>> = caps_owned
                                    .iter()
                                    .map(|v| v.iter().map(String::as_str).collect())
                                    .collect();
                                let chords_ref: Vec<&[&str]> =
                                    caps_slices.iter().map(Vec::as_slice).collect();
                                let conflict_note = conflict_map
                                    .get(&cmd)
                                    .map(|other| format!("Also on {}", other.title()));
                                let mut row = c::ShortcutRow::new(cmd.row_title())
                                    .chords(&chords_ref)
                                    .selected(state.selected == Some(cmd));
                                if let Some(note) = conflict_note.as_deref() {
                                    row = row.conflict(note);
                                }
                                let resp = row.show(ui, w);
                                for control in [&resp.row, &resp.cell] {
                                    neoflash_ui::theme::focus_ring_inside(
                                        ui,
                                        control,
                                        radius::sm(),
                                    );
                                }
                                if state.scroll_to == Some(cmd) {
                                    resp.row.scroll_to_me(Some(egui::Align::Center));
                                    state.scroll_to = None;
                                }
                                if resp.row.clicked() || resp.cell.clicked() {
                                    if let Some(focused) = ui.memory(|m| m.focused()) {
                                        ui.memory_mut(|m| m.surrender_focus(focused));
                                    }
                                    state.selected = Some(cmd);
                                    state.recording = Some(cmd);
                                    state.captured = None;
                                    state.append_mode = false;
                                    started_recording = true;
                                }
                                c::context_menu(&resp.row, |ui| {
                                    if ui.add(c::MenuItem::new("Add another shortcut")).clicked() {
                                        state.selected = Some(cmd);
                                        state.recording = Some(cmd);
                                        state.captured = None;
                                        state.append_mode = true;
                                        started_recording = true;
                                    }
                                    if ui
                                        .add_enabled(
                                            !effective.is_empty(),
                                            c::MenuItem::new("Remove shortcut"),
                                        )
                                        .clicked()
                                    {
                                        state.selected = Some(cmd);
                                        draft.shortcuts.set_shortcuts(cmd, Vec::new());
                                        state.stop_recording();
                                    }
                                    if ui
                                        .add_enabled(
                                            crate::commands::has_override(&draft.shortcuts, cmd),
                                            c::MenuItem::new("Reset to default"),
                                        )
                                        .clicked()
                                    {
                                        state.selected = Some(cmd);
                                        draft.shortcuts.reset_command(cmd);
                                        state.stop_recording();
                                    }
                                });
                            }
                        }
                    });
            }
            if total_matches == 0 {
                let msg = format!(
                    "No commands match “{}”. Clear the search or turn off Only assigned.",
                    state.search.trim()
                );
                ui.add_space(space::s3());
                ui.label(rich(msg, TextStyle::Body).color(nf.text_tertiary));
            }
        });

    if was_recording.is_some()
        && state.recording.is_some()
        && !started_recording
        && let Some(pos) = ui.input(|i| {
            i.pointer
                .primary_clicked()
                .then(|| i.pointer.interact_pos())
                .flatten()
        })
        && recorder_rect.is_none_or(|r| !r.contains(pos))
    {
        state.stop_recording();
    }

    ui.horizontal(|ui| {
        let can_reset = state
            .selected
            .is_some_and(|cmd| crate::commands::has_override(&draft.shortcuts, cmd));
        if ui
            .add_enabled(can_reset, c::Button::ghost("Reset command"))
            .clicked()
        {
            if let Some(cmd) = state.selected {
                draft.shortcuts.reset_command(cmd);
            }
            state.recording = None;
        }
        if ui.add(c::Button::ghost("Export as HTML…")).clicked() {
            state.recording = None;
            if let Some(path) = rfd::FileDialog::new()
                .set_title("Export Keyboard Shortcuts")
                .add_filter("HTML", &["html"])
                .set_file_name("neoflash-shortcuts.html")
                .save_file()
            {
                let html = crate::commands::export_shortcuts_html(&draft.shortcuts);
                let _ = std::fs::write(path, html);
            }
        }
    });

    area.remember_below(ui, "prefs-shortcuts", list.inner_rect.bottom());
    store_shortcuts_ui_state(ui.ctx(), state);
}

fn prefs_publish(ui: &mut Ui, draft: &mut PreferencesDraft) {
    section(ui, "Target");
    row(ui, "Default target", |ui| {
        let prev = draft.publish.default_target;
        // Same short names as the Publish settings sidebar.
        let options = PublishTarget::OPTIONS
            .map(|(target, _)| (target, super::publish_settings::target_short_name(target)));
        c::Select::options(
            ui,
            "prefs-publish-default-target",
            &mut draft.publish.default_target,
            &options,
            metrics::field_m(),
        );
        if draft.publish.default_target != prev {
            draft.publish.enable_target(draft.publish.default_target);
        }
    });
    row(ui, "Output folder", |ui| {
        // The path field is the only control that fills the row.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add(c::Button::new("Browse…")).clicked()
                && let Some(dir) = rfd::FileDialog::new()
                    .set_title("Choose HTML5 output folder")
                    .pick_folder()
            {
                draft.publish.output_folder = Some(dir.to_string_lossy().into_owned());
            }
            let mut folder = draft.publish.output_folder.clone().unwrap_or_default();
            let width = ui.available_width();
            if ui
                .add(
                    c::TextField::new(&mut folder, "Output folder")
                        .hint("~/Projects/ideas/publish")
                        .width(width),
                )
                .changed()
            {
                draft.publish.output_folder = (!folder.trim().is_empty()).then_some(folder);
            }
        });
    });
    row(ui, "Files", |ui| {
        option(ui, &mut draft.publish.html_wrapper, "HTML wrapper");
        option(ui, &mut draft.publish.assets_folder, "Assets folder");
        option(ui, &mut draft.publish.source_maps, "Source maps");
    });
    // Scripts share the first group so that the whole page fits the dialog without scrolling.
    row(ui, "Scripts", |ui| {
        option(ui, &mut draft.publish.strict, "Strict mode");
        option(
            ui,
            &mut draft.publish.warnings_as_errors,
            "Treat warnings as errors",
        );
        option(ui, &mut draft.publish.publish_on_save, "Publish on save");
    });
    section(ui, "Preview & publish");
    row(ui, "Preview mode", |ui| {
        c::Segmented::show(
            ui,
            &mut draft.publish.preview_chrome.clean,
            &[(false, "Technical bar"), (true, "Clean stage")],
            false,
        );
        option(ui, &mut draft.publish.publish_clean, "Clean publish output");
    });
    ui.add_enabled_ui(!draft.publish.preview_chrome.clean, |ui| {
        row(ui, "Technical bar", |ui| {
            option(
                ui,
                &mut draft.publish.preview_chrome.show_fps,
                "FPS & frames",
            );
            option(
                ui,
                &mut draft.publish.preview_chrome.show_dimensions,
                "Stage size (W×H)",
            );
            option(
                ui,
                &mut draft.publish.preview_chrome.show_controls,
                "Playback controls",
            );
            option(
                ui,
                &mut draft.publish.preview_chrome.show_load_summary,
                "Weight & size report",
            );
            option(
                ui,
                &mut draft.publish.preview_chrome.show_trace,
                "Trace & diagnostics",
            );
        });
    });
    section(ui, "Compression");
    row(ui, "Images", |ui| {
        let mut q = f64::from(draft.publish.jpeg_quality.clamp(1, 100));
        if ui
            .add(c::Slider::new(&mut q, 1.0..=100.0, "JPEG quality").width(metrics::field_m()))
            .changed()
        {
            draft.publish.jpeg_quality = q.round().clamp(1.0, 100.0) as u8;
        }
        caption(ui, &format!("JPEG {}", draft.publish.jpeg_quality));
    });
    row(ui, "Audio", |ui| {
        c::Select::new(
            ui.id().with("prefs-publish-audio"),
            draft.publish.options.audio.label(),
        )
        .width(metrics::field_m())
        .show(ui, |ui| {
            for policy in AudioExportPolicy::ALL {
                if ui
                    .add(
                        c::MenuItem::new(policy.label())
                            .checked(draft.publish.options.audio == policy),
                    )
                    .clicked()
                {
                    draft.publish.options.audio = policy;
                    ui.close();
                }
            }
        });
    });
    row(ui, "Acceleration", |ui| {
        c::Select::new(
            ui.id().with("prefs-publish-acceleration"),
            draft.publish.acceleration.label(),
        )
        .width(metrics::field_m())
        .show(ui, |ui| {
            for accel in neoflash_export::HtmlAcceleration::ALL {
                if ui
                    .add(
                        c::MenuItem::new(accel.label())
                            .checked(draft.publish.acceleration == accel),
                    )
                    .clicked()
                {
                    draft.publish.acceleration = accel;
                    ui.close();
                }
            }
        });
    });
    row(ui, "Sequences", |ui| {
        c::Select::new(
            ui.id().with("prefs-publish-seq-policy"),
            draft.publish.sequence_policy.label(),
        )
        .width(metrics::field_l())
        .show(ui, |ui| {
            for policy in neoflash_export::SequencePublishPolicy::ALL {
                if ui
                    .add(
                        c::MenuItem::new(policy.label())
                            .checked(draft.publish.sequence_policy == policy),
                    )
                    .clicked()
                {
                    draft.publish.sequence_policy = policy;
                    ui.close();
                }
            }
        });
    });
    row(ui, "Import", |ui| {
        let placement_lbl = match draft.sequence_import_options.placement {
            neoflash_import::ImageSequencePlacement::KeyframesOnSingleLayer => "Keyframes on layer",
            neoflash_import::ImageSequencePlacement::SeparateLayers => "Separate layers",
            neoflash_import::ImageSequencePlacement::ConvertToMovieClip => "MovieClip symbol",
            neoflash_import::ImageSequencePlacement::ConvertToGraphicSymbol => "Graphic symbol",
        };
        c::Select::new(ui.id().with("prefs-seq-import-placement"), placement_lbl)
            .width(metrics::field_l())
            .show(ui, |ui| {
                for (val, lbl) in [
                    (
                        neoflash_import::ImageSequencePlacement::ConvertToGraphicSymbol,
                        "Graphic symbol",
                    ),
                    (
                        neoflash_import::ImageSequencePlacement::ConvertToMovieClip,
                        "MovieClip symbol",
                    ),
                    (
                        neoflash_import::ImageSequencePlacement::KeyframesOnSingleLayer,
                        "Keyframes on layer",
                    ),
                    (
                        neoflash_import::ImageSequencePlacement::SeparateLayers,
                        "Separate layers",
                    ),
                ] {
                    if ui
                        .add(
                            c::MenuItem::new(lbl)
                                .checked(draft.sequence_import_options.placement == val),
                        )
                        .clicked()
                    {
                        draft.sequence_import_options.placement = val;
                        ui.close();
                    }
                }
            });
        let mut step = f64::from(draft.sequence_import_options.frame_step.clamp(1, 60));
        if ui
            .add(
                c::Field::new(&mut step, "Step")
                    .prefix("Step")
                    .decimals(0)
                    .range(1.0..=60.0)
                    .width(metrics::field_s()),
            )
            .changed()
        {
            draft.sequence_import_options.frame_step = step.round().clamp(1.0, 60.0) as u32;
        }
    });
}

enum UpdateUiAction {
    CheckNow,
    OpenReleaseNotes,
    Download(AvailableUpdate),
    CancelDownload,
    RevealFile(std::path::PathBuf),
}

fn reveal_button_label() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "Reveal in Finder"
    }
    #[cfg(target_os = "windows")]
    {
        "Show in Explorer"
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        "Open folder"
    }
}

fn prefs_updates(
    ui: &mut Ui,
    draft: &mut PreferencesDraft,
    updater: &mut UpdaterController,
    installed_ext_count: usize,
) {
    updater.sync_selected_channel(draft.updates.channel);
    let id = BuildIdentity::current();
    let now_unix = crate::recovery::unix_now_secs();
    let checking = updater.is_checking();
    let downloading = updater.is_downloading();
    let mut action: Option<UpdateUiAction> = None;

    section(ui, "Version");
    row(ui, "Version", |ui| {
        body_text(ui, &id.version_prefix());
    });
    row(ui, "Updates", |ui| {
        if ui
            .add_enabled(!checking && !downloading, c::Button::new("Check now"))
            .clicked()
        {
            action = Some(UpdateUiAction::CheckNow);
        }
        if ui.add(c::Button::ghost("Release notes")).clicked() {
            action = Some(UpdateUiAction::OpenReleaseNotes);
        }
    });

    // Status and messages start on the control axis.
    form_block(ui, |ui| {
        match &updater.state {
            UpdateState::Idle => {
                caption(
                    ui,
                    &format!(
                        "Status: {}",
                        format_last_checked(draft.updates.last_check_unix, now_unix)
                    ),
                );
            }
            UpdateState::Checking { .. } => {
                ui.horizontal(|ui| {
                    c::spinner(ui);
                    caption(ui, "Checking GitHub Releases for updates…");
                });
            }
            UpdateState::UpToDate { latest_version, .. } => {
                let status = match latest_version {
                    Some(v) => format!(
                        "Up to date (latest {v}) · {}",
                        format_last_checked(draft.updates.last_check_unix, now_unix)
                    ),
                    None => format!(
                        "Up to date · {}",
                        format_last_checked(draft.updates.last_check_unix, now_unix)
                    ),
                };
                caption(ui, &status);
            }
            UpdateState::UpdateAvailable { update, .. } => {
                ui.add(c::InlineMessage::info(&format!(
                    "Update {} is available for {}.",
                    update.version,
                    updater.config.platform.label()
                )));
                match &update.download {
                    Ok(_) => {
                        ui.horizontal(|ui| {
                            if ui.add(c::Button::primary("Download")).clicked() {
                                action = Some(UpdateUiAction::Download(update.clone()));
                            }
                            caption(ui, "Verified via SHA256SUMS before saving; manual install.");
                        });
                    }
                    Err(reason) => {
                        ui.add(c::InlineMessage::warning(reason));
                    }
                }
            }
            UpdateState::Downloading {
                update,
                received_bytes,
                total_bytes,
                ..
            } => {
                let fraction = total_bytes
                    .filter(|&tot| tot > 0)
                    .map(|tot| (*received_bytes as f32 / tot as f32).clamp(0.0, 1.0));
                let label = format!(
                    "Downloading {} ({})",
                    update.version,
                    format_byte_progress(*received_bytes, *total_bytes)
                );
                ui.horizontal(|ui| {
                    let bar_w = (ui.available_width() - 140.0).max(180.0);
                    c::progress(ui, &label, fraction, bar_w);
                    if ui.add(c::Button::new("Cancel download")).clicked() {
                        action = Some(UpdateUiAction::CancelDownload);
                    }
                });
            }
            UpdateState::VerifiedDownloaded {
                version,
                path,
                reveal_note,
                ..
            } => {
                ui.add(c::InlineMessage::info(&format!(
                    "Verified {version} (SHA-256) at {}. Quit NeoFlash and replace the application manually; Beta 1 does not auto-install.",
                    path.display()
                )));
                ui.horizontal(|ui| {
                    if ui.add(c::Button::new(reveal_button_label())).clicked() {
                        action = Some(UpdateUiAction::RevealFile(path.clone()));
                    }
                    if let Some(note) = reveal_note {
                        caption(ui, note);
                    }
                });
            }
            UpdateState::RateLimited { message, .. } => {
                ui.add(c::InlineMessage::warning(message));
            }
            UpdateState::Error {
                message,
                retry_update,
                ..
            } => {
                ui.add(c::InlineMessage::error(message));
                if let Some(retry) = retry_update
                    && retry.download.is_ok()
                    && ui.add(c::Button::new("Download")).clicked()
                {
                    action = Some(UpdateUiAction::Download(retry.clone()));
                }
            }
        }
        if let Some(err) = &updater.action_error {
            ui.add(c::InlineMessage::error(err));
        }
    });

    row(ui, "Channel", |ui| {
        ui.add_enabled_ui(!checking && !downloading, |ui| {
            c::Segmented::show(
                ui,
                &mut draft.updates.channel,
                &UpdateChannel::OPTIONS,
                false,
            );
        });
    });
    updater.sync_selected_channel(draft.updates.channel);
    row(ui, "Automatic", |ui| {
        option(
            ui,
            &mut draft.updates.check_automatically,
            "Check for updates automatically",
        );
    });
    disabled(ui, |ui| {
        row(ui, "Background", |ui| {
            let mut background = false;
            c::not_available(
                ui.add(c::Checkbox::new(&mut background, "Download in background")),
                "Download in background",
            );
        });
    });

    section(ui, "Privacy");
    disabled(ui, |ui| {
        row(ui, "Statistics", |ui| {
            let mut usage = false;
            c::not_available(
                ui.add(c::Checkbox::new(
                    &mut usage,
                    "Send anonymous usage statistics",
                )),
                "Send anonymous usage statistics",
            );
        });
        row(ui, "Crash reports", |ui| {
            let mut crashes = false;
            c::not_available(
                ui.add(c::Checkbox::new(&mut crashes, "Send crash reports")),
                "Send crash reports",
            );
        });
    });
    form_hint(
        ui,
        "Usage telemetry and crash reports are disabled; no data is collected or transmitted.",
    );

    section(ui, "Extensions");
    let ext_label = match installed_ext_count {
        1 => "1 extension installed".to_owned(),
        n => format!("{n} extensions installed"),
    };
    row(ui, "Installed", |ui| {
        body_text(ui, &ext_label);
        disabled(ui, |ui| {
            c::not_available(ui.add(c::Button::new("Manage…")), "Extension management");
        });
    });
    form_hint(ui, "Extension update checks are not available yet.");

    match action {
        Some(UpdateUiAction::CheckNow) => {
            updater.start_check(draft.updates.channel, ui.ctx());
        }
        Some(UpdateUiAction::OpenReleaseNotes) => {
            let _ = updater.open_release_notes();
        }
        Some(UpdateUiAction::Download(update)) => {
            if let Ok(target) = &update.download
                && let Some(dest) = updater.choose_download_destination(&target.asset_name)
            {
                updater.start_download_to_path(draft.updates.channel, update, dest, ui.ctx());
            }
        }
        Some(UpdateUiAction::CancelDownload) => {
            updater.cancel_download();
        }
        Some(UpdateUiAction::RevealFile(path)) => {
            let _ = updater.reveal_downloaded_file(&path);
        }
        None => {}
    }
}

fn prefs_ai(
    ui: &mut Ui,
    area: PageArea,
    draft: &mut PreferencesDraft,
    ai_manager: &mut crate::ai_connections::AiConnectionManager,
    ai_connection_modal: &mut Option<super::ai_dialogs::AiConnectionModal>,
) {
    use super::ai_dialogs::{AiConnectionModal, AiConnectionModalTarget};
    use crate::ai_connections::AiConnectionStatus;

    let nf = neoflash_ui::palette(ui.ctx());
    draft.has_ready_ai = ai_manager.has_ready_connection(&draft.ai);

    section(ui, "Connections");
    let valid_selected = draft
        .selected_ai_profile
        .as_deref()
        .is_some_and(|id| draft.ai.profiles.iter().any(|p| p.id == id));
    if !valid_selected {
        draft.selected_ai_profile = draft.ai.profiles.first().map(|p| p.id.clone());
    }

    let total = draft.ai.profiles.len();
    let mut clicked_id: Option<String> = None;
    let mut double_clicked_id: Option<String> = None;

    // The list fills the page above its command row and the Default / Privacy groups; one row
    // per connection. The 2 pt are the frame's border.
    let list_height = area.fill_height(
        ui,
        "prefs-ai",
        9.0 * metrics::control(),
        4.0 * metrics::row() + 2.0,
    ) - 2.0;
    let table = egui::Frame::new()
        .fill(nf.bg_panel)
        .stroke(egui::Stroke::new(1.0_f32, nf.border_subtle))
        .corner_radius(egui::CornerRadius::same(radius::md() as u8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical()
                .id_salt("prefs-ai-list")
                .max_height(list_height)
                .auto_shrink([false, false])
                .themed()
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    if draft.ai.profiles.is_empty() {
                        // `Add connection…` is the first button of the command row below.
                        let _ = c::EmptyState {
                            icon: i16::WAND,
                            title: "No AI connections",
                            body: "Add an OpenAI, Anthropic, Google Gemini or local provider to review and send prompts.",
                            action: None,
                        }
                        .show(ui);
                    }
                    let table_w = ui.available_width();
                    for (row_idx, profile) in draft.ai.profiles.iter().enumerate() {
                        let is_selected =
                            draft.selected_ai_profile.as_deref() == Some(profile.id.as_str());
                        let status = ai_manager.profile_status(profile);
                        let badge_type = match status {
                            AiConnectionStatus::Ready => c::BadgeType::Accent,
                            AiConnectionStatus::NotTested => c::BadgeType::Neutral,
                            AiConnectionStatus::NoKey | AiConnectionStatus::Failed => {
                                c::BadgeType::Warning
                            }
                        };
                        let (rect, resp) = ui.allocate_exact_size(
                            egui::vec2(table_w, metrics::row()),
                            egui::Sense::click(),
                        );
                        if is_selected {
                            ui.painter().rect_filled(rect, 0.0, nf.bg_selected);
                        } else if resp.hovered() {
                            ui.painter().rect_filled(rect, 0.0, nf.bg_hover);
                        }
                        if row_idx + 1 < total {
                            ui.painter().hline(
                                rect.x_range(),
                                rect.bottom() - 0.5,
                                egui::Stroke::new(1.0_f32, nf.border_subtle),
                            );
                        }
                        let inner = rect.shrink2(egui::vec2(space::s3(), 0.0));
                        ui.scope_builder(
                            egui::UiBuilder::new()
                                .max_rect(inner)
                                .layout(egui::Layout::left_to_right(egui::Align::Center)),
                            |ui| {
                                ui.spacing_mut().item_spacing.x = space::s2();
                                ui.label(
                                    rich(&profile.display_name, TextStyle::Body)
                                        .color(nf.text_primary),
                                );
                                let sub = format!(
                                    "{} · {}",
                                    profile.kind.display_name(),
                                    profile.default_model
                                );
                                ui.label(rich(&sub, TextStyle::Small).color(nf.text_secondary));
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.add(c::Badge::new(status.label(), badge_type));
                                    },
                                );
                            },
                        );
                        if resp.clicked() {
                            clicked_id = Some(profile.id.clone());
                        }
                        if resp.double_clicked() {
                            double_clicked_id = Some(profile.id.clone());
                        }
                    }
                });
        });
    let table_bottom = table.response.rect.bottom();

    if let Some(id) = clicked_id {
        draft.selected_ai_profile = Some(id);
    }
    if let Some(id) = double_clicked_id
        && let Some(prof) = draft.ai.profile(&id)
    {
        let has_key = ai_manager.has_secret(&prof.id);
        *ai_connection_modal = Some(AiConnectionModal::new_edit(
            prof,
            has_key,
            AiConnectionModalTarget::Preferences,
        ));
    }

    let selected_profile = draft
        .selected_ai_profile
        .as_deref()
        .and_then(|id| draft.ai.profile(id))
        .cloned();
    let has_sel = selected_profile.is_some();

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::s2();
        if ui.add(c::Button::new("Add connection…")).clicked() {
            *ai_connection_modal = Some(AiConnectionModal::new_add(
                &draft.ai,
                AiConnectionModalTarget::Preferences,
            ));
        }
        if ui.add_enabled(has_sel, c::Button::ghost("Edit…")).clicked()
            && let Some(prof) = &selected_profile
        {
            let has_key = ai_manager.has_secret(&prof.id);
            *ai_connection_modal = Some(AiConnectionModal::new_edit(
                prof,
                has_key,
                AiConnectionModalTarget::Preferences,
            ));
        }
        if ui.add_enabled(has_sel, c::Button::ghost("Test")).clicked()
            && let Some(prof) = &selected_profile
        {
            let _ = ai_manager.test_profile(prof);
            draft.has_ready_ai = ai_manager.has_ready_connection(&draft.ai);
        }
        if ui
            .add_enabled(has_sel, c::Button::ghost("Remove"))
            .clicked()
            && let Some(prof) = &selected_profile
        {
            draft.ai.remove_profile(&prof.id);
            ai_manager.clear_test_state(&prof.id);
            if !draft.removed_ai_secrets.contains(&prof.id) {
                draft.removed_ai_secrets.push(prof.id.clone());
            }
            draft.selected_ai_profile = draft.ai.profiles.first().map(|p| p.id.clone());
            draft.has_ready_ai = ai_manager.has_ready_connection(&draft.ai);
        }
    });

    section(ui, "Default");
    row(ui, "Connection", |ui| {
        if draft.ai.profiles.is_empty() {
            disabled(ui, |ui| select(ui, "None", metrics::field_l()));
        } else {
            let current_label = draft
                .ai
                .default_profile_id
                .as_deref()
                .and_then(|id| draft.ai.profile(id))
                .or_else(|| draft.ai.profiles.first())
                .map_or("None", |p| p.display_name.as_str())
                .to_owned();
            c::Select::new("prefs-ai-default", &current_label)
                .width(metrics::field_l())
                .show(ui, |ui| {
                    let ids_and_names: Vec<(String, String)> = draft
                        .ai
                        .profiles
                        .iter()
                        .map(|p| (p.id.clone(), p.display_name.clone()))
                        .collect();
                    for (id, name) in ids_and_names {
                        let is_sel = draft.ai.default_profile_id.as_deref() == Some(id.as_str());
                        if ui.add(c::MenuItem::new(&name).checked(is_sel)).clicked() {
                            draft.ai.default_profile_id = Some(id);
                            ui.close();
                        }
                    }
                });
        }
    });

    section(ui, "Privacy");
    ui.add(c::InlineMessage::info(
        "Nothing is sent to a provider until you press Send in the review dialog. Every request lists what will leave this computer.",
    ));
    ui.add(c::InlineMessage::info(
        "API keys are kept in the system keychain. They are never written to the project, to preferences or to logs.",
    ));
    area.remember_below(ui, "prefs-ai", table_bottom);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::{SyntaxColorOverrides, SyntaxColorSlot};
    use neoflash_core::{FrameScript, Project, Script, ScriptLanguage};

    fn find_center(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
        let b = output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .find_map(|(_, n)| {
                ((n.label() == Some(label) || n.value() == Some(label)) && !n.is_disabled())
                    .then(|| n.bounds())
                    .flatten()
            })
            .unwrap_or_else(|| panic!("expected enabled control `{label}`"));
        egui::pos2(((b.x0 + b.x1) * 0.5) as f32, ((b.y0 + b.y1) * 0.5) as f32)
    }

    #[test]
    fn preferences_category_switch_starts_at_top_after_scrolling_either_page() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        editor.open_preferences(PAGE_GENERAL);
        let mut time = 0.0;
        let mut frame = |events: Vec<egui::Event>| {
            time += 0.1;
            ctx.run(
                egui::RawInput {
                    // Both General and Drawing need vertical scrolling at this height.
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1024.0, 460.0),
                    )),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ctx| editor.preferences_dialog(ctx),
            )
        };
        frame(vec![]);
        let mut out = frame(vec![]);
        let top = find_center(&out, "Interface").y;
        for (heading, next, next_heading) in [
            ("Interface", "Drawing", "Default mode"),
            ("Default mode", "General", "Interface"),
        ] {
            let at = find_center(&out, heading);
            frame(vec![
                egui::Event::PointerMoved(at),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -12.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
            out = frame(vec![]);
            assert!(
                find_center(&out, heading).y < top,
                "{heading} should scroll"
            );
            let tab = find_center(&out, next);
            frame(vec![
                egui::Event::PointerMoved(tab),
                crate::tests::primary_button(tab, true),
            ]);
            out = frame(vec![crate::tests::primary_button(tab, false)]);
            assert_eq!(
                find_center(&out, next_heading).y,
                top,
                "{next} should start at the top"
            );
            out = frame(vec![]);
            assert_eq!(
                find_center(&out, next_heading).y,
                top,
                "{next} should stay at the top"
            );
        }
    }

    /// v3 "nothing jumps": the dialog rect and the footer are the same for every category, in
    /// every density.
    #[test]
    fn preferences_dialog_rect_is_identical_for_every_category_in_each_density() {
        use crate::workspace::DensityPref;

        for density in DensityPref::ALL {
            let ctx = egui::Context::default();
            ctx.enable_accesskit();
            let mut editor = Editor::from_project(&ctx, Project::blank());
            editor.workspace.density = density;
            editor.open_preferences(PAGE_GENERAL);
            let mut time = 0.0_f64;
            let mut frame = |editor: &mut Editor| {
                time += 0.1;
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1920.0, 1080.0),
                        )),
                        time: Some(time),
                        ..Default::default()
                    },
                    |ctx| editor.preferences_dialog(ctx),
                )
            };
            let mut reference = None;
            for (page, page_name) in PREFERENCE_PAGES.iter().enumerate() {
                editor.preferences.as_mut().unwrap().page = page;
                frame(&mut editor);
                frame(&mut editor);
                let out = frame(&mut editor);
                let rect = ctx
                    .memory(|m| m.area_rect(egui::Id::new("preferences")))
                    .expect("Preferences modal area");
                // The modal frame adds its 1 pt border on every side.
                let expected = egui::vec2(metrics::dialog_l(), metrics::dialog_l_height());
                assert!(
                    (rect.width() - expected.x).abs() <= 2.0
                        && (rect.height() - expected.y).abs() <= 2.0,
                    "{density:?} · {page_name}: dialog is {:?}, expected {expected:?}",
                    rect.size()
                );
                // "Save" is also a row of the Shortcuts page; the reset button is unique.
                let footer = out
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .unwrap()
                    .nodes
                    .iter()
                    .find_map(|(_, n)| {
                        (n.label() == Some("Reset to defaults"))
                            .then(|| n.bounds())
                            .flatten()
                    })
                    .expect("Reset to defaults button");
                match &reference {
                    None => reference = Some((rect, footer)),
                    Some((first, first_footer)) => {
                        assert_eq!(
                            rect, *first,
                            "{density:?} · {}: dialog rect changed",
                            PREFERENCE_PAGES[page]
                        );
                        assert_eq!(
                            footer, *first_footer,
                            "{density:?} · {}: footer moved",
                            PREFERENCE_PAGES[page]
                        );
                    }
                }
            }
        }
        metrics::set_density(Default::default());
    }

    #[test]
    fn t376_syntax_color_pickers_update_live_persist_and_reset_cleanly() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut project = Project::blank();
        project.symbols[0].timeline.scripts.push(FrameScript {
            layer: None,
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript2,
                source: "TFigure = new Array([0, 1, 0]);\nlFigure = new Array([1, 1]);\n".into(),
            },
        });
        let mut editor = Editor::from_project(&ctx, project);
        editor.open_preferences(PAGE_CODE_EDITOR);

        let mut tick = 0.0_f64;
        let mut step = |ed: &mut Editor, events: Vec<egui::Event>| -> egui::FullOutput {
            tick += 0.1;
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280.0, 1200.0),
                    )),
                    time: Some(tick),
                    focused: true,
                    events,
                    ..Default::default()
                },
                |ctx| {
                    ed.preferences_dialog(ctx);
                    super::super::dialog_helpers::record_end_of_frame_focus(ctx);
                },
            )
        };

        let _ = step(&mut editor, vec![]);
        let out = step(&mut editor, vec![]);

        // 1. Every one of the 14 syntax colour swatches is enabled and exposed via AccessKit.
        for slot in SyntaxColorSlot::ALL {
            let _ = find_center(&out, &format!("{} colour", slot.label()));
        }
        let _ = find_center(&out, "Reset colours");

        // 2. Click "Identifier colour" swatch -> opens NF2 colour picker popover (`Colour mixer`),
        //    click inside the picker to change the colour, and verify Escape closes ONLY the popover
        //    while keeping Preferences open!
        let id_swatch = find_center(&out, "Identifier colour");
        let _ = step(
            &mut editor,
            vec![
                egui::Event::PointerMoved(id_swatch),
                egui::Event::PointerButton {
                    pos: id_swatch,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        let _ = step(
            &mut editor,
            vec![egui::Event::PointerButton {
                pos: id_swatch,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        let _ = step(&mut editor, vec![]);
        let out_popup = step(&mut editor, vec![]);
        let mixer_center = find_center(&out_popup, "Colour mixer");
        let pick_pos = mixer_center + egui::vec2(-40.0, -30.0);
        let _ = step(
            &mut editor,
            vec![
                egui::Event::PointerMoved(pick_pos),
                egui::Event::PointerButton {
                    pos: pick_pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        let _ = step(
            &mut editor,
            vec![egui::Event::PointerButton {
                pos: pick_pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        let picked_ident = editor
            .preferences
            .as_ref()
            .unwrap()
            .code_editor
            .syntax_colors
            .identifier
            .expect("picking on Colour mixer must set identifier colour override");

        // Press Escape -> closes the colour popover without closing Preferences!
        let _ = step(
            &mut editor,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: Some(egui::Key::Escape),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        let _ = step(
            &mut editor,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: Some(egui::Key::Escape),
                pressed: false,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(
            editor.preferences.is_some(),
            "Escape with colour popover open must dismiss only the popover"
        );

        // Also set custom overrides for Keyword, Type, Current line, Selection, Error, Warning, Breakpoint.
        {
            let sc = &mut editor
                .preferences
                .as_mut()
                .unwrap()
                .code_editor
                .syntax_colors;
            sc.keyword = Some([255, 120, 200, 255]);
            sc.type_class = Some([80, 240, 180, 255]);
            sc.current_line = Some([32, 40, 64, 255]);
            sc.selection = Some([70, 100, 180, 255]);
            sc.error = Some([255, 60, 60, 255]);
            sc.warning = Some([240, 190, 40, 255]);
            sc.breakpoint = Some([255, 140, 40, 255]);
        }

        // Save Preferences -> persists `syntax_colors` into `editor.workspace.code_editor` and applies to Actions!
        let out_ready_save = step(&mut editor, vec![]);
        let save_pos = find_center(&out_ready_save, "Save");
        let _ = step(
            &mut editor,
            vec![
                egui::Event::PointerMoved(save_pos),
                egui::Event::PointerButton {
                    pos: save_pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        let _ = step(
            &mut editor,
            vec![egui::Event::PointerButton {
                pos: save_pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(editor.preferences.is_none());
        assert_eq!(
            editor.workspace.code_editor.syntax_colors.identifier,
            Some(picked_ident)
        );

        // Verify JSON persistence round-trip AND live syntax palette propagation in `code_layout_with_prefs`!
        let saved_json = serde_json::to_string(&editor.workspace).unwrap();
        let reloaded = WorkspacePrefs::from_json(&saved_json);
        assert_eq!(
            reloaded.code_editor.syntax_colors,
            editor.workspace.code_editor.syntax_colors
        );

        let mut t_fig_color = Color32::TRANSPARENT;
        let mut l_fig_color = Color32::TRANSPARENT;
        let mut arr_color = Color32::TRANSPARENT;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let sample = "TFigure = new Array(); lFigure = 1;";
                let galley = crate::sources::code_layout_with_prefs(
                    ui,
                    &sample,
                    800.0,
                    &editor.workspace.code_editor,
                );
                let job = &galley.job;
                for sec in &job.sections {
                    let s = &job.text[sec.byte_range.clone()];
                    if s == "TFigure" {
                        t_fig_color = sec.format.color;
                    } else if s == "lFigure" {
                        l_fig_color = sec.format.color;
                    } else if s == "Array" {
                        arr_color = sec.format.color;
                    }
                }
            });
        });
        let expected_ident = Color32::from_rgba_unmultiplied(
            picked_ident[0],
            picked_ident[1],
            picked_ident[2],
            picked_ident[3],
        );
        assert_eq!(t_fig_color, expected_ident);
        assert_eq!(l_fig_color, expected_ident);
        assert_eq!(arr_color, Color32::from_rgb(80, 240, 180));

        // 3. Reopen Preferences and click "Reset colours" -> clears all overrides back to empty defaults!
        editor.open_preferences(PAGE_CODE_EDITOR);
        let _ = step(&mut editor, vec![]);
        let out_reopen = step(&mut editor, vec![]);
        let reset_pos = find_center(&out_reopen, "Reset colours");
        let _ = step(
            &mut editor,
            vec![
                egui::Event::PointerMoved(reset_pos),
                egui::Event::PointerButton {
                    pos: reset_pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        let _ = step(
            &mut editor,
            vec![egui::Event::PointerButton {
                pos: reset_pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert_eq!(
            editor
                .preferences
                .as_ref()
                .unwrap()
                .code_editor
                .syntax_colors,
            SyntaxColorOverrides::default()
        );
        assert!(
            editor
                .preferences
                .as_ref()
                .unwrap()
                .code_editor
                .syntax_colors
                .is_empty()
        );

        // 4. Collapsing the Actions sidebar (`actions_sidebar = false`) must survive
        //    `Reset to defaults` on `PAGE_CODE_EDITOR` while other code-editor settings reset.
        editor.preferences = None;
        editor.workspace.code_editor.actions_sidebar = false;
        editor.workspace.code_editor.tab_size = 2;
        editor.workspace.code_editor.syntax_colors.keyword = Some([200, 100, 255, 255]);
        editor.open_preferences(PAGE_CODE_EDITOR);
        let _ = step(&mut editor, vec![]);
        let out_page_reset = step(&mut editor, vec![]);
        let reset_page_pos = find_center(&out_page_reset, "Reset to defaults");
        let _ = step(
            &mut editor,
            vec![
                egui::Event::PointerMoved(reset_page_pos),
                egui::Event::PointerButton {
                    pos: reset_page_pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        let _ = step(
            &mut editor,
            vec![egui::Event::PointerButton {
                pos: reset_page_pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        let reset_ce = &editor.preferences.as_ref().unwrap().code_editor;
        assert!(
            !reset_ce.actions_sidebar,
            "Reset to defaults on Code editor page must preserve collapsed actions_sidebar"
        );
        assert_eq!(reset_ce.tab_size, 4);
        assert!(reset_ce.syntax_colors.is_empty());
    }
}
