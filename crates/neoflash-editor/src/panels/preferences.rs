//! Preferences dialog (NF2 `Dialog / Preferences`, DL-01 / W3.6; references `dialog-preferences-*.png`),
//! opened by Edit ▸ Preferences… (⌘U) and, on the Shortcuts page, Edit ▸ Keyboard shortcuts… (⌥⇧⌘K).
//!
//! Values are editor preferences (`WorkspacePrefs`), never document data. Edits live on a draft:
//! the theme previews live while the dialog is open; `Save` / `Enter` applies and persists the draft,
//! `Cancel` / `Esc` / ✕ restores the previous theme and discards it; `Reset to defaults` resets the
//! current page. Wired: General ▸ Theme, UI scale, Undo levels, Shift-select; Timeline ▸ Default frame
//! view and row, Show seconds, Snap playhead, Onion range, Default ease; Text ▸ Font and size;
//! Clipboard ▸ Default paste. Every other control stays disabled until its engine exists (plan in
//! `docs/NF2-WIRING-PLAN.md` W3.6).
use super::planned::{
    FooterClick, body_text, caption, check, footer_clicked, form_label, segmented, select, slider,
    stepper, switch, text,
};
use crate::app::Editor;
use crate::theme::ThemeMode;
use crate::workspace::{
    ClipboardPrefs, CodeEditorPrefs, DrawingPrefs, FramePreview, GeneralPrefs, PublishPrefs,
    ShortcutPrefs, TextPrefs, TimelinePrefs, TimelineView, WorkspacePrefs,
};
use eframe::egui::{self, Color32, Ui};
use neoflash_ui::components as c;
use neoflash_ui::icons::{self, i16};
use neoflash_ui::metrics;
use neoflash_ui::tokens::{TextStyle, radius, space};
use neoflash_ui::typography::rich;

const LABEL: f32 = 150.0;
/// Dialog width of the reference (`dialog-preferences-*.png`).
const WIDTH: f32 = 920.0;

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
        w.timeline.show_seconds = draft.timeline.show_seconds;
        w.timeline.snap_playhead = draft.timeline.snap_playhead;
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
            return;
        };
        let p = neoflash_ui::palette(ctx);
        let mut close_cancel = false;
        let mut close_save = false;
        let modal = egui::Modal::new(egui::Id::new("preferences"))
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
                    top.y + metrics::DIALOG_HEADER - 0.5,
                    egui::Stroke::new(1.0_f32, p.border_subtle),
                );
                egui::Frame::new()
                    .inner_margin(egui::Margin::same(space::S5 as i8))
                    .show(ui, |ui| {
                        ui.set_width(WIDTH - 2.0 * space::S5);
                        ui.spacing_mut().item_spacing = egui::vec2(space::S2, space::S2);
                        match body(ui, &mut draft) {
                            Some(FooterClick::Ghost) => draft.reset_page(),
                            Some(FooterClick::Button(0)) => close_cancel = true,
                            Some(FooterClick::Button(_)) => close_save = true,
                            None => {}
                        }
                    });
            });
        if !close_cancel && !close_save {
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
        let shown = if ctx.theme() == egui::Theme::Light {
            ThemeMode::Light
        } else {
            ThemeMode::Dark
        };
        if close_cancel {
            self.preferences = None;
            draft.original_theme.apply(ctx);
        } else if close_save {
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

/// Category list + the selected page + footer.
fn body(ui: &mut Ui, draft: &mut PreferencesDraft) -> Option<FooterClick> {
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(196.0, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(196.0);
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
    footer_clicked(
        ui,
        "",
        Some(("Reset to defaults", draft.page_has_settings())),
        &["Cancel", "Save"],
    )
}

fn prefs_general(ui: &mut Ui, draft: &mut PreferencesDraft) {
    let nf = neoflash_ui::palette(ui.ctx());
    disabled(ui, |ui| section(ui, "Interface"));
    row(ui, "Theme", |ui| {
        c::Segmented::show(
            ui,
            &mut draft.theme,
            &[(ThemeMode::Dark, "Dark"), (ThemeMode::Light, "Light")],
            false,
        );
        // Follow system has no control in the design (caption only): NF2-REQUESTS R-012.
        disabled(ui, |ui| caption(ui, "or follow system"));
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
        row(ui, "On launch", |ui| select(ui, "Welcome screen", 260.0));
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
    disabled(ui, |ui| {
        row(ui, "Autosave", |ui| {
            switch(ui, true, "Every");
            stepper(ui, 5.0, "Autosave minutes", "Min", "", 0, 100.0);
        });
        row(ui, "Tooltips", |ui| {
            switch(ui, true, "Show tooltips with shortcuts")
        });
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

/// Not wired yet: the reference layout, disabled.
fn prefs_code_editor(ui: &mut Ui, _draft: &mut PreferencesDraft) {
    disabled(ui, prefs_code_editor_reference);
}

fn prefs_code_editor_reference(ui: &mut Ui) {
    let nf = neoflash_ui::palette(ui.ctx());
    section(ui, "Font");
    ui.horizontal(|ui| {
        select(ui, "Roboto Mono", 260.0);
        stepper(ui, 13.0, "Font size", "Size", "px", 0, 120.0);
        stepper(ui, 1.6, "Line height", "Line", "", 1, 110.0);
    });
    section(ui, "Editing");
    ui.horizontal(|ui| {
        switch(ui, true, "Line numbers");
        switch(ui, false, "Word wrap");
        switch(ui, true, "Auto-close brackets");
    });
    ui.horizontal(|ui| {
        switch(ui, true, "Code hints");
        caption(ui, "Delay");
        slider(ui, 30.0, "Code hint delay", 160.0);
        caption(ui, "300 ms");
    });
    ui.horizontal(|ui| {
        select(ui, "Tab: 4 spaces", 180.0);
        switch(ui, true, "Format on save");
        switch(ui, true, "Highlight current line");
    });
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
    egui::Grid::new("prefs-syntax")
        .num_columns(3)
        .spacing(egui::vec2(space::S5, space::S1))
        .show(ui, |ui| {
            for (i, (name, color)) in tokens.iter().enumerate() {
                ui.horizontal(|ui| {
                    c::swatch(ui, *color, 18.0);
                    form_label(ui, name, 96.0);
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
    egui::Frame::new()
        .fill(nf.bg_field)
        .corner_radius(egui::CornerRadius::same(radius::MD as u8))
        .stroke(egui::Stroke::new(1.0_f32, nf.border_subtle))
        .inner_margin(egui::Margin::symmetric(space::S2 as i8, space::S2 as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
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
                ui.horizontal(|ui| {
                    let (gutter, _) =
                        ui.allocate_exact_size(egui::vec2(40.0, 20.0), egui::Sense::hover());
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
                    ui.painter().text(
                        gutter.right_center() - egui::vec2(space::S2, 0.0),
                        egui::Align2::RIGHT_CENTER,
                        (n + 1).to_string(),
                        neoflash_ui::typography::font(TextStyle::MonoSmall),
                        nf.text_tertiary,
                    );
                    let mut job = egui::text::LayoutJob::default();
                    for (t, color) in parts.iter() {
                        job.append(
                            t,
                            0.0,
                            neoflash_ui::typography::format(TextStyle::Mono, *color),
                        );
                    }
                    ui.label(job);
                });
            }
        });
}

fn prefs_timeline(ui: &mut Ui, draft: &mut PreferencesDraft) {
    let nf = neoflash_ui::palette(ui.ctx());
    disabled(ui, |ui| {
        section(ui, "Selection & view");
        row(ui, "Selection", |ui| {
            switch(ui, true, "Span-based selection")
        });
    });
    row(ui, "Default frame view", |ui| {
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
        c::Select::options(
            ui,
            "prefs-frame-rows",
            &mut draft.timeline.short_rows,
            &[(false, "Row: Normal"), (true, "Row: Short")],
            160.0,
        );
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
        section(ui, "Onion skin defaults");
    });
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
        row(ui, "Opacity falloff", |ui| {
            slider(ui, 70.0, "Opacity falloff", 200.0);
            caption(ui, "70 → 15 %");
        });
        section(ui, "Animation");
        row(ui, "Auto keyframe", |ui| {
            switch(ui, true, "Create keyframe on transform")
        });
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
    disabled(ui, |ui| {
        row(ui, "Tween type on drag", |ui| {
            select(ui, "Classic tween", 180.0)
        });
    });
}

/// Not wired yet: the reference layout, disabled.
fn prefs_drawing(ui: &mut Ui, _draft: &mut PreferencesDraft) {
    disabled(ui, prefs_drawing_reference);
}

fn prefs_drawing_reference(ui: &mut Ui) {
    section(ui, "Default mode");
    row(ui, "New shapes", |ui| {
        segmented(ui, &["Merge drawing", "Object drawing"], 1)
    });
    section(ui, "Pen tool");
    ui.horizontal(|ui| {
        switch(ui, true, "Show pen preview");
        switch(ui, false, "Show solid points");
        switch(ui, false, "Precise cursors");
    });
    section(ui, "Recognition");
    for label in [
        "Connect lines",
        "Smooth curves",
        "Recognise lines",
        "Recognise shapes",
        "Click accuracy",
    ] {
        row(ui, label, |ui| select(ui, "Normal", 160.0));
    }
    section(ui, "Snapping");
    row(ui, "Tolerance", |ui| {
        stepper(ui, 8.0, "Snap tolerance", "px", "", 0, 120.0);
        switch(ui, false, "Snap to pixels");
        switch(ui, true, "Snap align");
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
        row(ui, "Font substitution", |ui| {
            select(ui, "Show warning", 220.0)
        });
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
    disabled(ui, |ui| {
        ui.horizontal(|ui| {
            switch(ui, true, "Import vector paste as editable paths");
            switch(ui, true, "Ask before pasting large bitmaps");
        });
    });
}

/// Not wired yet: the reference layout, disabled.
fn prefs_shortcuts(ui: &mut Ui, _draft: &mut PreferencesDraft) {
    disabled(ui, prefs_shortcuts_reference);
}

fn prefs_shortcuts_reference(ui: &mut Ui) {
    let nf = neoflash_ui::palette(ui.ctx());
    section(ui, "Shortcut set");
    ui.horizontal(|ui| {
        select(ui, "NeoFlash default", 240.0);
        for b in ["Duplicate", "Rename", "Delete"] {
            let _ = ui.add(c::Button::ghost(b));
        }
    });
    ui.horizontal(|ui| {
        let mut s = "keyframe".to_owned();
        ui.add(c::Search::new(&mut s, "Search commands").width(400.0));
        select(ui, "All commands", 200.0);
    });
    let rows: [(&str, &[&str], Option<&str>); 6] = [
        ("Insert › Frame", &["F5"], None),
        ("Insert › Keyframe", &[], None),
        ("Insert › Blank keyframe", &["F7"], None),
        ("Clear keyframe", &["⇧", "F6"], None),
        (
            "Convert to keyframes",
            &["⌥", "F6"],
            Some("Conflicts with Insert › Keyframe"),
        ),
        ("Edit › Timeline › Remove frames", &["⇧", "F5"], None),
    ];
    for (command, keys, warning) in rows {
        let top = ui.cursor().min;
        ui.horizontal(|ui| {
            ui.set_min_height(32.0);
            ui.add_sized(
                [260.0, 20.0],
                egui::Label::new(rich(command, TextStyle::Body).color(nf.text_primary)),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if keys.is_empty() {
                    text(ui, "Press keys…", "", 140.0);
                }
                for k in keys.iter().rev() {
                    c::kbd(ui, k, false);
                }
                if let Some(w) = warning {
                    ui.add(c::Badge::new(w, c::BadgeType::Warning));
                }
            });
        });
        let w = ui.available_width();
        ui.painter().hline(
            top.x..=top.x + w,
            ui.cursor().min.y - 2.0,
            egui::Stroke::new(1.0_f32, nf.border_subtle),
        );
    }
    ui.horizontal(|ui| {
        let _ = ui.add(c::Button::ghost("Reset command"));
        let _ = ui.add(c::Button::ghost("Export as HTML…"));
    });
}

/// Not wired yet: the reference layout, disabled.
fn prefs_publish(ui: &mut Ui, _draft: &mut PreferencesDraft) {
    disabled(ui, prefs_publish_reference);
}

fn prefs_publish_reference(ui: &mut Ui) {
    section(ui, "Target");
    row(ui, "Default target", |ui| {
        select(ui, "HTML5 · Rust / WebAssembly", 280.0)
    });
    row(ui, "Output folder", |ui| {
        text(ui, "~/Projects/ideas/publish", "", 240.0);
        let _ = ui.add(c::Button::new("Browse…"));
    });
    ui.horizontal(|ui| {
        check(ui, true, "HTML wrapper");
        check(ui, true, "Assets folder");
        check(ui, false, "Source maps");
    });
    section(ui, "Compression");
    row(ui, "Images", |ui| {
        slider(ui, 80.0, "JPEG quality", 200.0);
        caption(ui, "JPEG 80");
    });
    row(ui, "Audio", |ui| select(ui, "MP3 128 kbps", 180.0));
    row(ui, "Acceleration", |ui| select(ui, "GPU (WebGL)", 180.0));
    section(ui, "Scripts");
    ui.horizontal(|ui| {
        switch(ui, true, "Strict mode");
        switch(ui, false, "Treat warnings as errors");
        switch(ui, false, "Publish on save");
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
