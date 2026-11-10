//! Preferences dialog (NF2 `Dialog / Preferences`, DL-01 / W3.6; references `dialog-preferences-*.png`),
//! opened by Edit ▸ Preferences… (⌘U) and, on the Shortcuts page, Edit ▸ Keyboard shortcuts… (⌥⇧⌘K).
//!
//! Values are editor preferences (`WorkspacePrefs`), never document data. Edits live on a draft:
//! the theme previews live while the dialog is open; `Save` / `Enter` applies and persists the draft,
//! `Cancel` / `Esc` / ✕ restores the previous theme and discards it; `Reset to defaults` resets the
//! current page. Controls whose engine or NF2 design role is not yet defined stay disabled (see
//! `docs/NF2-WIRING-PLAN.md` W3.6/W3.7 and `docs/NF2-REQUESTS.md`).
use super::planned::{
    FooterClick, body_text, caption, footer_clicked, form_label, segmented, select, switch,
};
use crate::app::Editor;
use crate::theme::{ScrollTheme as _, ThemeMode};
use crate::workspace::{
    ClipboardPrefs, CodeEditorPrefs, DrawingPrefs, FramePreview, GeneralPrefs, PublishPrefs,
    PublishTarget, ShortcutPrefs, TextPrefs, TimelinePrefs, TimelineView, WorkspacePrefs,
};
use eframe::egui::{self, Color32, Ui};
use neoflash_export::AudioExportPolicy;
use neoflash_ui::components as c;
use neoflash_ui::icons::{self, i16};
use neoflash_ui::tokens::{TextStyle, radius, space};
use neoflash_ui::typography::rich;

const LABEL: f32 = 120.0;
/// Dialog width of the reference (`dialog-preferences-*.png`).
/// Narrower Preferences composition (R-055): 720 pt dialog, 160 pt category list, 120 pt labels.
const WIDTH: f32 = 720.0;
const SIDEBAR: f32 = 160.0;

pub(crate) const PREFERENCE_PAGES: [&str; 9] = [
    "General",
    "Code editor",
    "Timeline",
    "Drawing",
    "Text",
    "Clipboard",
    "Shortcuts",
    "Publish",
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
pub(crate) const PAGE_UPDATES: usize = 8;

/// Editable copy of the wired preferences while the dialog is open.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PreferencesDraft {
    pub(crate) page: usize,
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
    pub(crate) shortcuts: ShortcutPrefs,
    /// Stage view settings edited by Drawing ▸ Snap to pixels (`StageView::prefs`, one state).
    pub(crate) pixels_snap: bool,
}

impl PreferencesDraft {
    fn from_prefs(
        prefs: &WorkspacePrefs,
        page: usize,
        font_families: Vec<String>,
        pixels_snap: bool,
    ) -> Self {
        Self {
            page,
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
            shortcuts: prefs.shortcuts.clone(),
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
                // Recent files are history, not a setting.
                let recent = std::mem::take(&mut self.general.recent_files);
                self.general = GeneralPrefs {
                    recent_files: recent,
                    ..defaults.general
                };
            }
            PAGE_CODE_EDITOR => self.code_editor = defaults.code_editor,
            PAGE_DRAWING => {
                self.drawing = defaults.drawing;
                self.pixels_snap = false;
            }
            PAGE_SHORTCUTS => self.shortcuts = defaults.shortcuts,
            PAGE_PUBLISH => self.publish = defaults.publish,
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
            _ => {}
        }
    }

    fn page_has_settings(&self) -> bool {
        self.page != PAGE_UPDATES
    }
}

fn section(ui: &mut Ui, s: &str) {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.add_space(space::S2);
    ui.label(rich(s, TextStyle::BodyStrong).color(nf.text_primary));
}

fn row(ui: &mut Ui, label: &str, content: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        form_label(ui, label, LABEL);
        content(ui);
    });
}

fn disabled(ui: &mut Ui, content: impl FnOnce(&mut Ui)) {
    ui.scope(|ui| {
        ui.disable();
        content(ui);
    });
}

fn bound_switch(ui: &mut Ui, value: &mut bool, label: &str) {
    ui.add(c::Switch::new(value, label));
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
        self.preferences = Some(PreferencesDraft::from_prefs(
            &self.workspace,
            page.min(PREFERENCE_PAGES.len() - 1),
            self.fonts.families.clone(),
            self.stage_view.prefs.pixels_snap,
        ));
    }

    /// Apply and persist a confirmed draft (Save).
    pub(crate) fn save_preferences(&mut self, ctx: &egui::Context, draft: &PreferencesDraft) {
        self.set_theme(ctx, draft.theme);
        let w = &mut self.workspace;
        w.ui_scale = draft.ui_scale;
        w.undo_levels = draft.undo_levels.round() as u32;
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
        w.onion_range = draft.onion.map(|n| n.round().max(0.0) as u32);
        w.default_ease = draft.default_ease;
        w.text_font = draft.text_font.clone();
        w.text_size = draft.text_size;
        w.paste_in_place = draft.paste_in_place;
        w.general = draft.general.clone();
        w.code_editor = draft.code_editor.clone();
        w.timeline_prefs = draft.timeline_prefs.clone();
        w.drawing = draft.drawing.clone();
        w.text_prefs = draft.text_prefs.clone();
        w.clipboard_prefs = draft.clipboard_prefs.clone();
        w.publish = draft.publish.clone();
        w.shortcuts = draft.shortcuts.clone();
        // Snap to pixels is the one stage view setting: change it now and in the saved view defaults.
        w.view_defaults
            .get_or_insert_with(Default::default)
            .pixels_snap = draft.pixels_snap;
        w.sanitize();
        self.stage_view.prefs.pixels_snap = draft.pixels_snap;
        self.apply_editor_preferences(ctx);
    }

    pub(crate) fn preferences_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.preferences.clone() else {
            clear_shortcuts_ui_state(ctx);
            return;
        };
        if draft.page != PAGE_SHORTCUTS {
            cancel_shortcut_recording(ctx);
        }
        let was_recording = is_recording_shortcut(ctx, draft.page);
        let p = neoflash_ui::palette(ctx);
        let mut close_cancel = false;
        let mut close_save = false;
        let modal_id = egui::Id::new("preferences");
        let modal = egui::Modal::new(modal_id)
            .area(
                egui::Modal::default_area(modal_id)
                    .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, space::S5)),
            )
            .frame(c::Surface::Dialog.frame(ctx))
            .backdrop_color(p.overlay)
            .show(ctx, |ui| {
                ui.set_width(WIDTH);
                let top = ui.cursor().min;
                close_cancel |= c::DialogHeader {
                    title: "Preferences",
                }
                .show(ui);
                ui.painter().hline(
                    top.x..=top.x + WIDTH,
                    top.y + neoflash_ui::metrics::dialog_header() - 0.5,
                    egui::Stroke::new(1.0_f32, p.border_subtle),
                );
                egui::Frame::new()
                    .inner_margin(egui::Margin::same(space::S5 as i8))
                    .show(ui, |ui| {
                        ui.set_width(WIDTH - 2.0 * space::S5);
                        ui.spacing_mut().item_spacing = egui::vec2(space::S2, space::S2);
                        let footer_height = neoflash_ui::metrics::control() + 2.0 * space::S2;
                        let body_height = (ctx.content_rect().height()
                            - 2.0 * space::S5
                            - neoflash_ui::metrics::dialog_header()
                            - 2.0 * space::S5
                            - footer_height)
                            .max(neoflash_ui::metrics::control());
                        egui::ScrollArea::vertical()
                            .id_salt("preferences-body")
                            .max_height(body_height)
                            .scroll_bar_visibility(
                                egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded,
                            )
                            .auto_shrink([false, true])
                            .themed()
                            .show(ui, |ui| body(ui, &mut draft));
                        match footer_clicked(
                            ui,
                            "",
                            Some(("Reset to defaults", draft.page_has_settings())),
                            &["Cancel", "Save"],
                        ) {
                            Some(FooterClick::Ghost) => {
                                draft.reset_page();
                                if draft.page == PAGE_SHORTCUTS {
                                    cancel_shortcut_recording(ctx);
                                }
                            }
                            Some(FooterClick::Button(0)) => close_cancel = true,
                            Some(FooterClick::Button(_)) => close_save = true,
                            None => {}
                        }
                    });
            });
        if !close_cancel && !close_save && !was_recording {
            let typing = ctx.wants_keyboard_input();
            let popup_open = egui::Popup::is_any_open(ctx);
            if !popup_open
                && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
            {
                close_cancel = true;
            } else if !typing
                && !popup_open
                && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
            {
                close_save = true;
            } else if modal.should_close() {
                close_cancel = true;
            }
        }
        // Live theme preview: the stage and panels behind the dialog follow the draft.
        let shown = ThemeMode::current(ctx);
        if close_cancel {
            clear_shortcuts_ui_state(ctx);
            self.preferences = None;
            draft.original_theme.apply(ctx);
        } else if close_save {
            clear_shortcuts_ui_state(ctx);
            self.preferences = None;
            self.save_preferences(ctx, &draft);
        } else {
            if draft.theme != shown {
                draft.theme.apply(ctx);
            }
            self.preferences = Some(draft);
        }
    }
}

/// Category list + the selected page.
fn body(ui: &mut Ui, draft: &mut PreferencesDraft) {
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(SIDEBAR, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(SIDEBAR);
                ui.spacing_mut().item_spacing.y = 2.0;
                let icons = [
                    i16::SETTINGS,
                    i16::CODE,
                    i16::FILM,
                    i16::PEN,
                    i16::TEXT,
                    i16::PASTE,
                    i16::KEYBOARD,
                    i16::PUBLISH,
                    i16::RECENT,
                ];
                for (i, (name, icon)) in PREFERENCE_PAGES.iter().zip(icons).enumerate() {
                    if c::TreeItem::new(name, c::TreeItemType::Symbol(icon))
                        .selected(draft.page == i)
                        .show(ui)
                        .row
                        .clicked()
                    {
                        draft.page = i;
                    }
                }
            },
        );
        ui.add_space(space::S5);
        ui.vertical(|ui| match draft.page {
            0 => prefs_general(ui, draft),
            1 => prefs_code_editor(ui, draft),
            2 => prefs_timeline(ui, draft),
            3 => prefs_drawing(ui, draft),
            4 => prefs_text(ui, draft),
            5 => prefs_clipboard(ui, draft),
            6 => prefs_shortcuts(ui, draft),
            7 => prefs_publish(ui, draft),
            _ => disabled(ui, prefs_updates),
        });
    });
}

fn prefs_general(ui: &mut Ui, draft: &mut PreferencesDraft) {
    let nf = neoflash_ui::palette(ui.ctx());
    disabled(ui, |ui| section(ui, "Interface"));
    row(ui, "Theme", |ui| {
        c::Select::options(
            ui,
            "prefs-theme",
            &mut draft.theme,
            &ThemeMode::ALL.map(|theme| (theme, theme.label())),
            220.0,
        );
        // Follow system has no control in the design (caption only): NF2-REQUESTS R-012.
        disabled(ui, |ui| caption(ui, "or follow system"));
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
            140.0,
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
        row(ui, "Language", |ui| select(ui, "English", 200.0));
        section(ui, "Workspace");
    });
    row(ui, "On launch", |ui| {
        c::Select::options(
            ui,
            "prefs-on-launch",
            &mut draft.general.on_launch,
            &crate::workspace::OnLaunch::OPTIONS,
            260.0,
        );
    });
    row(ui, "Undo levels", |ui| {
        let range = WorkspacePrefs::UNDO_LEVELS;
        bound_stepper(
            ui,
            &mut draft.undo_levels,
            "Undo levels",
            "Steps",
            *range.start() as f64..=*range.end() as f64,
            140.0,
        );
    });
    row(ui, "Autosave", |ui| {
        bound_switch(ui, &mut draft.general.autosave, "Every");
        let mut minutes = f64::from(draft.general.autosave_minutes);
        ui.add_enabled(
            draft.general.autosave,
            c::Field::new(&mut minutes, "Autosave minutes")
                .prefix("Min")
                .decimals(0)
                .range(1.0..=120.0)
                .stepper()
                .width(100.0),
        );
        draft.general.autosave_minutes = minutes.round().clamp(1.0, 120.0) as u32;
    });
    row(ui, "Tooltips", |ui| {
        bound_switch(
            ui,
            &mut draft.general.tooltip_shortcuts,
            "Show tooltips with shortcuts",
        )
    });
    disabled(ui, |ui| {
        section(ui, "Selection");
    });
    row(ui, "Shift-select", |ui| {
        bound_switch(
            ui,
            &mut draft.shift_select,
            "Add with ⇧ (otherwise click adds)",
        )
    });
    disabled(ui, |ui| {
        row(ui, "Highlight", |ui| select(ui, "Layer colour", 200.0))
    });
}

fn prefs_code_editor(ui: &mut Ui, draft: &mut PreferencesDraft) {
    const BUNDLED_MONO: &str = "Roboto Mono";
    const TAB_SIZES: [(u8, &str); 3] = [
        (2, "Tab: 2 spaces"),
        (4, "Tab: 4 spaces"),
        (8, "Tab: 8 spaces"),
    ];

    let nf = neoflash_ui::palette(ui.ctx());
    section(ui, "Font");
    ui.horizontal(|ui| {
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
            .width(220.0)
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
                    .unit("px")
                    .decimals(0)
                    .range(8.0..=32.0)
                    .step(1.0)
                    .stepper()
                    .width(120.0),
            )
            .changed()
        {
            draft.code_editor.font_size = font_size.clamp(8.0, 32.0) as f32;
        }
        let mut line_height = f64::from(draft.code_editor.line_height);
        if ui
            .add(
                c::Field::new(&mut line_height, "Line height")
                    .prefix("Line")
                    .decimals(1)
                    .range(1.0..=3.0)
                    .step(0.1)
                    .stepper()
                    .width(110.0),
            )
            .changed()
        {
            draft.code_editor.line_height = line_height.clamp(1.0, 3.0) as f32;
        }
    });
    section(ui, "Editing");
    ui.horizontal(|ui| {
        bound_switch(ui, &mut draft.code_editor.line_numbers, "Line numbers");
        bound_switch(ui, &mut draft.code_editor.word_wrap, "Word wrap");
        bound_switch(
            ui,
            &mut draft.code_editor.auto_close_brackets,
            "Auto-close brackets",
        );
    });
    ui.horizontal(|ui| {
        bound_switch(ui, &mut draft.code_editor.code_hints, "Code hints");
        caption(ui, "Delay");
        let mut delay = f64::from(draft.code_editor.hint_delay_ms);
        if ui
            .add(
                c::Slider::new(&mut delay, 0.0..=1000.0, "Code hint delay")
                    .step(10.0)
                    .width(160.0),
            )
            .changed()
        {
            draft.code_editor.hint_delay_ms = delay.round().clamp(0.0, 1000.0) as u32;
        }
        caption(ui, &format!("{} ms", draft.code_editor.hint_delay_ms));
    });
    ui.horizontal(|ui| {
        let tab_label = TAB_SIZES
            .iter()
            .find(|(n, _)| *n == draft.code_editor.tab_size)
            .map(|(_, label)| (*label).to_owned())
            .unwrap_or_else(|| format!("Tab: {} spaces", draft.code_editor.tab_size));
        c::Select::new("prefs-code-tab-size", &tab_label)
            .width(180.0)
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
        bound_switch(ui, &mut draft.code_editor.format_on_save, "Format on save");
        bound_switch(
            ui,
            &mut draft.code_editor.highlight_current_line,
            "Highlight current line",
        );
    });
    // Custom syntax colour schemes are not supported by the NF2 token palette yet.
    disabled(ui, |ui| {
        section(ui, "Colour scheme");
        ui.horizontal(|ui| {
            select(ui, "Scheme: NF2 Night", 220.0);
            let _ = ui.add(c::Button::ghost("Duplicate"));
            let _ = ui.add(c::Button::ghost("Import…"));
        });
        let tokens: [(&str, Color32); 13] = [
            ("Keyword", nf.syntax_keyword),
            ("Type / class", nf.syntax_type),
            ("Function", nf.syntax_function),
            ("String", nf.syntax_string),
            ("Number", nf.syntax_number),
            ("Comment", nf.syntax_comment),
            ("XML tag", nf.syntax_tag),
            ("Attribute", nf.syntax_attr),
            ("Error", nf.syntax_error),
            ("Warning", nf.syntax_warning),
            ("Breakpoint", nf.syntax_breakpoint),
            ("Selection", nf.syntax_selection),
            ("Current line", nf.syntax_current_line),
        ];
        // Three columns fit the narrower 720 pt dialog (R-055).
        egui::Grid::new("prefs-syntax")
            .num_columns(3)
            .spacing(egui::vec2(space::S3, space::S1))
            .show(ui, |ui| {
                for (i, (name, color)) in tokens.iter().enumerate() {
                    ui.horizontal(|ui| {
                        c::swatch(ui, *color, 18.0);
                        form_label(ui, name, 76.0);
                        let [r, g, b, _] = color.to_srgba_unmultiplied();
                        ui.label(
                            rich(format!("{r:02X}{g:02X}{b:02X}"), TextStyle::MonoSmall)
                                .color(nf.text_tertiary),
                        );
                    });
                    if i % 3 == 2 {
                        ui.end_row();
                    }
                }
            });
        caption(
            ui,
            "Red is reserved for errors only. Breakpoints use orange, warnings amber.",
        );
    });
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
        .corner_radius(egui::CornerRadius::same(radius::MD as u8))
        .stroke(egui::Stroke::new(1.0_f32, nf.border_subtle))
        .inner_margin(egui::Margin::symmetric(space::S2 as i8, space::S2 as i8))
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
                (None, &[("// orbit setup", nf.syntax_comment)]),
                (
                    Some((i16::BREAKPOINT, nf.syntax_breakpoint)),
                    &[
                        ("var ", nf.syntax_keyword),
                        ("speed", nf.text_primary),
                        (":", nf.text_primary),
                        ("Number", nf.syntax_type),
                        (" = ", nf.text_primary),
                        ("2.5", nf.syntax_number),
                        (";", nf.text_primary),
                    ],
                ),
                (
                    None,
                    &[
                        ("animator.", nf.text_primary),
                        ("play", nf.syntax_function),
                        ("();", nf.text_primary),
                    ],
                ),
                (
                    Some((i16::WARNING, nf.syntax_warning)),
                    &[
                        ("<Keyframe ", nf.syntax_tag),
                        ("index", nf.syntax_attr),
                        ("=", nf.text_primary),
                        ("\"0\"", nf.syntax_string),
                        ("/>", nf.syntax_tag),
                    ],
                ),
                (
                    Some((i16::ERROR, nf.syntax_error)),
                    &[("orbit_mc.rotation += speed;", nf.text_primary)],
                ),
            ];
            for (n, (marker, parts)) in lines.iter().enumerate() {
                let band = ui.painter().add(egui::Shape::Noop);
                let mut gutter_right = 0.0;
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
                            gutter.right_center() - egui::vec2(space::S2, 0.0),
                            egui::Align2::RIGHT_CENTER,
                            (n + 1).to_string(),
                            neoflash_ui::typography::font(TextStyle::MonoSmall),
                            nf.text_tertiary,
                        );
                    }
                    let mut job = egui::text::LayoutJob::default();
                    for (t, color) in parts.iter() {
                        let mut fmt = neoflash_ui::typography::format(TextStyle::Mono, *color);
                        fmt.font_id = preview_font.clone();
                        fmt.line_height = Some(row_h);
                        job.append(t, 0.0, fmt);
                    }
                    ui.label(job);
                });
                if draft.code_editor.highlight_current_line && n == 2 {
                    let band_rect = egui::Rect::from_min_max(
                        egui::pos2(gutter_right, row_out.response.rect.top()),
                        egui::pos2(preview_right, row_out.response.rect.bottom()),
                    );
                    ui.painter().set(
                        band,
                        egui::Shape::rect_filled(band_rect, 0.0, nf.syntax_current_line),
                    );
                }
            }
        });
}

fn prefs_timeline(ui: &mut Ui, draft: &mut PreferencesDraft) {
    let nf = neoflash_ui::palette(ui.ctx());
    section(ui, "Selection & view");
    row(ui, "Selection", |ui| {
        bound_switch(
            ui,
            &mut draft.timeline_prefs.span_selection,
            "Span-based selection",
        )
    });
    row(ui, "Default frame view", |ui| {
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
            160.0,
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
            160.0,
        );
        if row_mode != prev_row_mode {
            draft.timeline.preview = FramePreview::Off;
        }
        draft.timeline.short_rows = row_mode == 1;
        draft.timeline.compact_rows = row_mode == 2;
    });
    row(ui, "Ruler", |ui| {
        bound_switch(ui, &mut draft.timeline.show_seconds, "Show seconds");
        bound_switch(
            ui,
            &mut draft.timeline.snap_playhead,
            "Snap playhead to keyframes",
        );
    });
    disabled(ui, |ui| {
        row(ui, "Scenes", |ui| {
            switch(ui, false, "Named anchor on each scene")
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
            120.0,
        );
        bound_stepper(
            ui,
            &mut draft.onion[1],
            "Onion frames after",
            "After",
            0.0..=max,
            120.0,
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
            .add(c::Slider::new(&mut falloff, 15.0..=100.0, "Opacity falloff").width(200.0))
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
        bound_switch(
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
            180.0,
        );
    });
    row(ui, "Tween type on drag", |ui| {
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
            180.0,
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
    ui.horizontal(|ui| {
        bound_switch(ui, &mut draft.drawing.pen_preview, "Show pen preview");
        let mut solid_points = false;
        ui.add_enabled(
            false,
            c::Switch::new(&mut solid_points, "Show solid points"),
        );
        bound_switch(ui, &mut draft.drawing.precise_cursors, "Precise cursors");
    });
    section(ui, "Recognition");
    row(ui, "Connect lines", |ui| {
        c::Select::options(
            ui,
            "prefs-drawing-connect-lines",
            &mut draft.drawing.connect_lines,
            &crate::workspace::Tolerance::OPTIONS,
            160.0,
        );
    });
    row(ui, "Smooth curves", |ui| {
        c::Select::options(
            ui,
            "prefs-drawing-smooth-curves",
            &mut draft.drawing.smooth_curves,
            &crate::workspace::Tolerance::SMOOTH_OPTIONS,
            160.0,
        );
    });
    row(ui, "Recognise lines", |ui| {
        c::Select::options(
            ui,
            "prefs-drawing-recognise-lines",
            &mut draft.drawing.recognise_lines,
            &crate::workspace::Tolerance::OPTIONS,
            160.0,
        );
    });
    row(ui, "Recognise shapes", |ui| {
        c::Select::options(
            ui,
            "prefs-drawing-recognise-shapes",
            &mut draft.drawing.recognise_shapes,
            &crate::workspace::Tolerance::OPTIONS,
            160.0,
        );
    });
    row(ui, "Click accuracy", |ui| {
        c::Select::options(
            ui,
            "prefs-drawing-click-accuracy",
            &mut draft.drawing.click_accuracy,
            &crate::workspace::Tolerance::OPTIONS,
            160.0,
        );
    });
    section(ui, "Snapping");
    row(ui, "Tolerance", |ui| {
        let mut snap_tolerance = f64::from(draft.drawing.snap_tolerance);
        bound_stepper(
            ui,
            &mut snap_tolerance,
            "Snap tolerance",
            "px",
            1.0..=32.0,
            120.0,
        );
        draft.drawing.snap_tolerance = snap_tolerance as f32;
        bound_switch(ui, &mut draft.pixels_snap, "Snap to pixels");
        let mut snap_align = true;
        ui.add_enabled(false, c::Switch::new(&mut snap_align, "Snap align"));
    });
}

fn prefs_text(ui: &mut Ui, draft: &mut PreferencesDraft) {
    disabled(ui, |ui| section(ui, "Defaults"));
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
            .width(220.0)
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
            130.0,
        );
    });
    disabled(ui, |ui| {
        row(ui, "Anti-alias", |ui| select(ui, "For readability", 220.0));
    });
    row(ui, "Font substitution", |ui| {
        c::Select::options(
            ui,
            "prefs-font-substitution",
            &mut draft.text_prefs.substitution_warning,
            &[(true, "Show warning"), (false, "Substitute silently")],
            220.0,
        );
    });
    disabled(ui, |ui| {
        section(ui, "Behaviour");
        ui.horizontal(|ui| {
            switch(ui, true, "Embed used glyphs automatically");
            switch(ui, true, "Spell check");
        });
        ui.horizontal(|ui| {
            switch(ui, false, "Vertical text: right-to-left flow");
            switch(ui, true, "No kerning for vertical");
        });
        row(ui, "Input method", |ui| select(ui, "System default", 220.0));
    });
}

fn prefs_clipboard(ui: &mut Ui, draft: &mut PreferencesDraft) {
    disabled(ui, |ui| {
        section(ui, "Copying");
        row(ui, "Bitmaps", |ui| {
            select(ui, "Match screen (144 ppi)", 220.0)
        });
        row(ui, "Gradient quality", |ui| select(ui, "Normal", 160.0));
        ui.horizontal(|ui| {
            switch(ui, true, "Copy as SVG to other apps");
            switch(ui, true, "Keep layer names");
        });
        section(ui, "Pasting");
    });
    row(ui, "Default paste", |ui| {
        c::Segmented::show(
            ui,
            &mut draft.paste_in_place,
            &[(false, "In centre"), (true, "In place")],
            false,
        );
    });
    ui.horizontal(|ui| {
        bound_switch(
            ui,
            &mut draft.clipboard_prefs.editable_vector_paste,
            "Import vector paste as editable paths",
        );
        disabled(ui, |ui| {
            switch(ui, true, "Ask before pasting large bitmaps")
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

fn prefs_shortcuts(ui: &mut Ui, draft: &mut PreferencesDraft) {
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
    ui.horizontal(|ui| {
        let active_label = draft.shortcuts.active_set_label().to_owned();
        let set_names: Vec<String> = draft.shortcuts.sets.keys().cloned().collect();
        let (set_resp, _) = c::Select::new("prefs-shortcut-set", &active_label)
            .width(240.0)
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
            ui.add(c::Button::ghost("Rename"))
                .on_disabled_hover_text(super::menu::NOT_AVAILABLE);
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
                .add(c::Switch::new(&mut state.only_assigned, "Only assigned"))
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

    egui::ScrollArea::vertical()
        .id_salt("prefs-shortcuts-list")
        .max_height(232.0)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            for category in crate::commands::CommandCategory::ALL {
                let all_in_cat: Vec<crate::commands::CommandId> = crate::commands::CommandId::ALL
                    .iter()
                    .copied()
                    .filter(|cmd| cmd.category() == category)
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
                ui.add_space(space::S3);
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

    store_shortcuts_ui_state(ui.ctx(), state);
}

fn prefs_publish(ui: &mut Ui, draft: &mut PreferencesDraft) {
    section(ui, "Target");
    row(ui, "Default target", |ui| {
        let prev = draft.publish.default_target;
        c::Select::options(
            ui,
            "prefs-publish-default-target",
            &mut draft.publish.default_target,
            &PublishTarget::OPTIONS,
            280.0,
        );
        if draft.publish.default_target != prev {
            draft.publish.enable_target(draft.publish.default_target);
        }
    });
    row(ui, "Output folder", |ui| {
        let mut folder = draft.publish.output_folder.clone().unwrap_or_default();
        if ui
            .add(
                c::TextField::new(&mut folder, "Output folder")
                    .hint("~/Projects/ideas/publish")
                    .width(240.0),
            )
            .changed()
        {
            draft.publish.output_folder = (!folder.trim().is_empty()).then_some(folder);
        }
        if ui.add(c::Button::new("Browse…")).clicked()
            && let Some(dir) = rfd::FileDialog::new()
                .set_title("Choose HTML5 output folder")
                .pick_folder()
        {
            draft.publish.output_folder = Some(dir.to_string_lossy().into_owned());
        }
    });
    ui.horizontal(|ui| {
        ui.add(c::Checkbox::new(
            &mut draft.publish.html_wrapper,
            "HTML wrapper",
        ));
        ui.add(c::Checkbox::new(
            &mut draft.publish.assets_folder,
            "Assets folder",
        ));
        ui.add(c::Checkbox::new(
            &mut draft.publish.source_maps,
            "Source maps",
        ));
    });
    section(ui, "Preview & publish");
    row(ui, "Preview mode", |ui| {
        c::Segmented::show(
            ui,
            &mut draft.publish.preview_chrome.clean,
            &[(false, "Technical bar"), (true, "Clean stage")],
            false,
        );
        ui.add(c::Checkbox::new(
            &mut draft.publish.publish_clean,
            "Clean publish output",
        ));
    });
    ui.add_enabled_ui(!draft.publish.preview_chrome.clean, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.add(c::Checkbox::new(
                &mut draft.publish.preview_chrome.show_fps,
                "FPS & frames",
            ));
            ui.add(c::Checkbox::new(
                &mut draft.publish.preview_chrome.show_dimensions,
                "Stage size (W×H)",
            ));
            ui.add(c::Checkbox::new(
                &mut draft.publish.preview_chrome.show_controls,
                "Playback controls",
            ));
            ui.add(c::Checkbox::new(
                &mut draft.publish.preview_chrome.show_load_summary,
                "Weight & size report",
            ));
            ui.add(c::Checkbox::new(
                &mut draft.publish.preview_chrome.show_trace,
                "Trace & diagnostics",
            ));
        });
    });
    section(ui, "Compression");
    row(ui, "Images", |ui| {
        let mut q = f64::from(draft.publish.jpeg_quality.clamp(1, 100));
        if ui
            .add(c::Slider::new(&mut q, 1.0..=100.0, "JPEG quality").width(200.0))
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
        .width(180.0)
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
        .width(180.0)
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
    section(ui, "Scripts");
    ui.horizontal(|ui| {
        bound_switch(ui, &mut draft.publish.strict, "Strict mode");
        bound_switch(
            ui,
            &mut draft.publish.warnings_as_errors,
            "Treat warnings as errors",
        );
        bound_switch(ui, &mut draft.publish.publish_on_save, "Publish on save");
    });
}

fn prefs_updates(ui: &mut Ui) {
    section(ui, "Version");
    ui.horizontal(|ui| {
        body_text(ui, "NeoFlash 2.0.0 · build 412 · up to date");
        let _ = ui.add(c::Button::new("Check now"));
        let _ = ui.add(c::Button::ghost("Release notes"));
    });
    row(ui, "Channel", |ui| segmented(ui, &["Stable", "Beta"], 0));
    ui.horizontal(|ui| {
        switch(ui, true, "Check for updates automatically");
        switch(ui, true, "Download in background");
    });
    section(ui, "Privacy");
    switch(ui, false, "Send anonymous usage statistics");
    switch(ui, true, "Send crash reports");
    section(ui, "Extensions");
    row(ui, "Installed", |ui| {
        body_text(ui, "3 extensions · 1 update available");
        let _ = ui.add(c::Button::new("Manage…"));
    });
}
