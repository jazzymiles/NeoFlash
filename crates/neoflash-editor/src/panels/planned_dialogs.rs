//! Planned dialog previews: Preferences (`dialog-preferences-*.png`), Document settings (`document-dialogs.png`),
//! Publish settings (`publish-misc-dialogs.png`), Export image / video / audio (`dialogs-export.png`),
//! Import video (`dialog-import-video.png`), Sound properties (`dialogs-sound-stroke.png`).
//! Preferences categories and export formats switch preview pages; every setting stays disabled.
use super::planned::{
    body_text, caption, check, footer, form_label, num, segmented, select, slider, stepper, switch,
    text,
};
use eframe::egui::{self, Color32, Ui};
use neoflash_ui::components as c;
use neoflash_ui::components::timeline as tl;
use neoflash_ui::icons::{self, i16};
use neoflash_ui::tokens::{TextStyle, radius, space};
use neoflash_ui::typography::rich;

const LABEL: f32 = 150.0;

/// Overline caption of a dialog group ("OUTPUT", "RANGE").
pub(super) fn group(ui: &mut Ui, s: &str) {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.add_space(space::S1);
    ui.label(rich(s, TextStyle::Overline).color(nf.text_tertiary));
}

/// Preferences page section title.
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

/// Stage-preview placeholder (export / import dialogs): `bg/canvas` pasteboard with a `bg/stage` page.
pub(super) fn preview_box(ui: &mut Ui, width: f32, height: f32, label: &str) {
    let nf = neoflash_ui::palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let r = egui::CornerRadius::same(radius::LG as u8);
    ui.painter().rect_filled(rect, r, nf.bg_canvas);
    let stage = egui::Rect::from_center_size(rect.center(), rect.size() * 0.8);
    ui.painter().rect_filled(stage, 0.0, nf.bg_stage);
    ui.painter().text(
        stage.center(),
        egui::Align2::CENTER_CENTER,
        label,
        neoflash_ui::typography::font(TextStyle::Small),
        nf.text_tertiary,
    );
    ui.painter().rect_stroke(
        rect,
        r,
        egui::Stroke::new(1.0_f32, nf.border_subtle),
        egui::StrokeKind::Inside,
    );
}

fn disabled(ui: &mut Ui, content: impl FnOnce(&mut Ui)) {
    ui.scope(|ui| {
        ui.disable();
        content(ui);
    });
}

// ── Preferences ──

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

/// Dialog / Preferences: category list (preview navigation) + the selected page.
pub(super) fn preferences(ui: &mut Ui, page: &mut usize) {
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
                        .selected(*page == i)
                        .show(ui)
                        .row
                        .clicked()
                    {
                        *page = i;
                    }
                }
            },
        );
        ui.add_space(space::S5);
        ui.vertical(|ui| {
            disabled(ui, |ui| match *page {
                0 => prefs_general(ui),
                1 => prefs_code_editor(ui),
                2 => prefs_timeline(ui),
                3 => prefs_drawing(ui),
                4 => prefs_text(ui),
                5 => prefs_clipboard(ui),
                6 => prefs_shortcuts(ui),
                7 => prefs_publish(ui),
                _ => prefs_updates(ui),
            });
        });
    });
    disabled(ui, |ui| {
        footer(ui, "", Some("Reset to defaults"), &["Cancel", "Save"])
    });
}

fn prefs_general(ui: &mut Ui) {
    let nf = neoflash_ui::palette(ui.ctx());
    section(ui, "Interface");
    row(ui, "Theme", |ui| {
        segmented(ui, &["Dark", "Light"], 0);
        caption(ui, "or follow system");
    });
    row(ui, "UI scale", |ui| select(ui, "100 %", 140.0));
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
    row(ui, "Undo levels", |ui| {
        stepper(ui, 100.0, "Undo levels", "Steps", "", 0, 140.0)
    });
    row(ui, "Autosave", |ui| {
        switch(ui, true, "Every");
        stepper(ui, 5.0, "Autosave minutes", "Min", "", 0, 100.0);
    });
    row(ui, "Tooltips", |ui| {
        switch(ui, true, "Show tooltips with shortcuts")
    });
    section(ui, "Selection");
    row(ui, "Shift-select", |ui| {
        switch(ui, true, "Add with ⇧ (otherwise click adds)")
    });
    row(ui, "Highlight", |ui| select(ui, "Layer colour", 200.0));
}

fn prefs_code_editor(ui: &mut Ui) {
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
    egui::Grid::new("planned-prefs-syntax")
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

fn prefs_timeline(ui: &mut Ui) {
    let nf = neoflash_ui::palette(ui.ctx());
    section(ui, "Selection & view");
    row(ui, "Selection", |ui| {
        switch(ui, true, "Span-based selection")
    });
    row(ui, "Default frame view", |ui| {
        select(ui, "Normal", 160.0);
        select(ui, "Row: Normal", 160.0);
    });
    row(ui, "Ruler", |ui| {
        switch(ui, true, "Show seconds");
        switch(ui, false, "Snap playhead to keyframes");
    });
    row(ui, "Scenes", |ui| {
        switch(ui, false, "Named anchor on each scene")
    });
    section(ui, "Onion skin defaults");
    row(ui, "Range", |ui| {
        stepper(ui, 3.0, "Onion frames before", "Before", "", 0, 120.0);
        stepper(ui, 3.0, "Onion frames after", "After", "", 0, 120.0);
    });
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
    row(ui, "Default ease", |ui| select(ui, "Ease In-Out", 180.0));
    row(ui, "Tween type on drag", |ui| {
        select(ui, "Classic tween", 180.0)
    });
}

fn prefs_drawing(ui: &mut Ui) {
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

fn prefs_text(ui: &mut Ui) {
    section(ui, "Defaults");
    row(ui, "Font", |ui| {
        select(ui, "Inter", 220.0);
        stepper(ui, 16.0, "Font size", "Size", "pt", 0, 130.0);
    });
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
}

fn prefs_clipboard(ui: &mut Ui) {
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
    row(ui, "Default paste", |ui| {
        segmented(ui, &["In centre", "In place"], 0)
    });
    ui.horizontal(|ui| {
        switch(ui, true, "Import vector paste as editable paths");
        switch(ui, true, "Ask before pasting large bitmaps");
    });
}

fn prefs_shortcuts(ui: &mut Ui) {
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

fn prefs_publish(ui: &mut Ui) {
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

// ── Document and publish ──

/// Dialog / Document settings.
pub(super) fn document_settings(ui: &mut Ui) {
    let nf = neoflash_ui::palette(ui.ctx());
    const L: f32 = 100.0;
    ui.spacing_mut().item_spacing.y = space::S3;
    ui.horizontal(|ui| {
        form_label(ui, "Stage size", L);
        num(ui, 960.0, "Stage width", "W", "", 0, 120.0);
        num(ui, 600.0, "Stage height", "H", "", 0, 120.0);
        let _ = ui.add(c::IconButton::new(i16::LINK, "Constrain proportions").selected(true));
    });
    ui.horizontal(|ui| {
        form_label(ui, "", L);
        let _ = ui.add(c::Button::ghost("Match contents"));
        let _ = ui.add(c::Button::ghost("Match printer"));
    });
    ui.horizontal(|ui| {
        form_label(ui, "Scale", L);
        check(ui, true, "Scale content with stage");
        let mut point = c::RegistrationPoint::TopLeft;
        ui.add(c::RegistrationPicker { point: &mut point });
    });
    ui.horizontal(|ui| {
        form_label(ui, "Units", L);
        select(ui, "Pixels", 160.0);
    });
    ui.horizontal(|ui| {
        form_label(ui, "Background", L);
        let mut color = nf.bg_stage;
        c::ColorField::new(&mut color, "Background")
            .width(200.0)
            .show(ui);
    });
    ui.horizontal(|ui| {
        form_label(ui, "Frame rate", L);
        stepper(ui, 24.0, "Frame rate", "fps", "", 2, 140.0);
        caption(ui, "120 frames · 5.00 s");
    });
    ui.horizontal(|ui| {
        form_label(ui, "Script", L);
        select(ui, "ActionScript 3.0 · strict", 220.0);
    });
    caption(
        ui,
        "Stage size, background and frame rate are editable now in Properties › Document.",
    );
    footer(ui, "", Some("Make default"), &["Cancel", "OK"]);
}

/// Dialog / Publish settings.
pub(super) fn publish_settings(ui: &mut Ui) {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(260.0, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(260.0);
                ui.spacing_mut().item_spacing.y = 2.0;
                select(ui, "Profile: Default", 260.0);
                for (on, icon, label) in [
                    (true, i16::GLOBE, "HTML5 · Rust / WebAssembly"),
                    (false, i16::DESKTOP, "Desktop app (macOS / Windows)"),
                    (false, i16::VIDEO, "Video (MP4)"),
                    (true, i16::GRID, "Image sequence / sprite sheet"),
                    (false, i16::CODE, "SVG animation"),
                ] {
                    ui.horizontal(|ui| {
                        ui.set_min_height(32.0);
                        check(ui, on, "");
                        icons::show(ui, icon, nf.text_secondary);
                        body_text(ui, label);
                    });
                }
            },
        );
        ui.add_space(space::S5);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = space::S2;
            ui.label(rich("HTML5 · Rust / WebAssembly", TextStyle::Title).color(nf.text_primary));
            group(ui, "Output");
            ui.horizontal(|ui| {
                text(ui, "publish/ideas.html", "File", 380.0);
                let _ = ui.add(c::Button::new("…"));
            });
            ui.horizontal(|ui| {
                check(ui, true, "Include HTML wrapper");
                select(ui, "Template: Responsive", 200.0);
            });
            group(ui, "Runtime");
            ui.horizontal(|ui| {
                select(ui, "WASM: optimised (−O3)", 220.0);
                select(ui, "Scale: Fit, keep ratio", 200.0);
            });
            ui.horizontal(|ui| {
                check(ui, true, "Loop");
                check(ui, true, "Autoplay");
                check(ui, true, "Preloader");
                check(ui, false, "Source maps");
            });
            group(ui, "Assets");
            ui.horizontal(|ui| {
                caption(ui, "JPEG quality");
                slider(ui, 80.0, "JPEG quality", 160.0);
                caption(ui, "80");
                select(ui, "Audio: MP3 128 kbps", 180.0);
            });
            ui.horizontal(|ui| {
                check(ui, true, "Sprite sheets for bitmaps");
                check(ui, true, "Embed fonts (subset)");
            });
            group(ui, "Scripts");
            ui.horizontal(|ui| {
                switch(ui, true, "Strict mode");
                switch(ui, false, "Include trace()");
                switch(ui, true, "Minify");
            });
        });
    });
    footer(
        ui,
        "Estimated size ~ 1.8 MB · 6 files",
        None,
        &["Cancel", "OK", "Publish"],
    );
}

// ── Export / import ──

const IMAGE_FORMATS: [&str; 5] = ["PNG", "SVG", "PSD", "AI", "PDF"];

/// Dialog / Export image. The format segments switch preview pages.
pub(super) fn export_image(ui: &mut Ui, format: &mut usize) {
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(300.0);
            preview_box(ui, 300.0, 188.0, "Stage preview");
            caption(ui, "Frame 30 of 120 · 960 × 600");
        });
        ui.add_space(space::S5);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = space::S2;
            let opts: Vec<(usize, &str)> = IMAGE_FORMATS.iter().copied().enumerate().collect();
            c::Segmented::show(ui, format, &opts, false);
            disabled(ui, |ui| {
                group(ui, "Range");
                ui.horizontal(|ui| {
                    select(ui, "Current frame", 200.0);
                    num(ui, 30.0, "Frames", "Frames", "", 0, 160.0);
                });
                group(ui, "Size");
                ui.horizontal(|ui| {
                    num(ui, 960.0, "Width", "W", "", 0, 120.0);
                    num(ui, 600.0, "Height", "H", "", 0, 120.0);
                    let _ = ui
                        .add(c::IconButton::new(i16::LINK, "Constrain proportions").selected(true));
                    select(ui, "1×", 80.0);
                });
                group(ui, "Background");
                ui.horizontal(|ui| {
                    ui.add(c::Radio::new(*format != 1, "Stage colour"));
                    ui.add(c::Radio::new(*format == 1, "Transparent"));
                });
                match *format {
                    1 => {
                        group(ui, "SVG options");
                        ui.horizontal(|ui| {
                            select(ui, "Images: Embed", 180.0);
                            select(ui, "Text: Convert to outlines", 200.0);
                        });
                        switch(ui, true, "Minify output");
                        switch(ui, false, "Use CSS classes for styles");
                        check(ui, false, "Include hidden & guide layers");
                        ui.horizontal(|ui| {
                            num(ui, 2.0, "Precision", "Precision", "", 0, 120.0);
                            text(ui, "nf-", "ID prefix", 180.0);
                        });
                    }
                    2 => {
                        group(ui, "PSD options");
                        ui.add(c::Radio::new(
                            true,
                            "Keep timeline layers as Photoshop layers",
                        ));
                        ui.add(c::Radio::new(false, "Flatten to one layer"));
                        check(ui, true, "Folders → layer groups");
                        check(ui, true, "Symbols → smart objects");
                        check(ui, true, "Keep text editable");
                        ui.horizontal(|ui| {
                            select(ui, "Colour: sRGB 8-bit", 200.0);
                            select(ui, "Resolution: 144 ppi", 180.0);
                        });
                    }
                    3 => {
                        group(ui, "AI options");
                        select(ui, "Version: Illustrator CS6+", 240.0);
                        ui.add(c::Radio::new(true, "Each layer → AI layer"));
                        ui.add(c::Radio::new(false, "Each keyframe → artboard"));
                        check(ui, true, "Strokes as strokes (not outlines)");
                        check(ui, false, "Convert text to outlines");
                    }
                    _ => caption(ui, "No format-specific options in the reference design."),
                }
            });
        });
    });
    let (size, button) = match *format {
        1 => ("~ 18 kB · 42 paths", "Export SVG"),
        2 => ("~ 4.2 MB · 6 layers", "Export PSD"),
        3 => ("~ 260 kB · 6 layers", "Export AI"),
        4 => ("", "Export PDF"),
        _ => ("", "Export PNG"),
    };
    disabled(ui, |ui| footer(ui, size, None, &["Cancel", button]));
}

/// Dialog / Export video.
pub(super) fn export_video(ui: &mut Ui) {
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(300.0);
            preview_box(ui, 300.0, 188.0, "Stage preview");
            c::progress(ui, "Exporting frames 72 / 120", Some(0.6), 300.0);
            caption(ui, "Preview while rendering · ⏸ to pause");
        });
        ui.add_space(space::S5);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = space::S2;
            segmented(ui, &["MP4", "WebM", "MOV", "GIF", "PNG seq."], 0);
            group(ui, "Range");
            ui.horizontal(|ui| {
                select(ui, "Whole movie (all scenes)", 220.0);
                num(ui, 1.0, "From frame", "From", "", 0, 90.0);
                num(ui, 120.0, "To frame", "To", "", 0, 90.0);
            });
            group(ui, "Video");
            ui.horizontal(|ui| {
                num(ui, 1920.0, "Width", "W", "", 0, 110.0);
                num(ui, 1200.0, "Height", "H", "", 0, 110.0);
                let _ =
                    ui.add(c::IconButton::new(i16::LINK, "Constrain proportions").selected(true));
                select(ui, "2×", 76.0);
            });
            ui.horizontal(|ui| {
                select(ui, "24 fps (document)", 160.0);
                select(ui, "Codec: H.264", 160.0);
            });
            ui.horizontal(|ui| {
                caption(ui, "Quality");
                slider(ui, 75.0, "Quality", 180.0);
                caption(ui, "High · ~8 Mbps");
            });
            group(ui, "Content");
            check(
                ui,
                true,
                "Render nested movie clip animation and scripts (runtime capture)",
            );
            ui.horizontal(|ui| {
                ui.add(c::Radio::new(true, "Stop at last frame"));
                ui.add(c::Radio::new(false, "Stop after"));
                num(ui, 10.0, "Stop after seconds", "", "s", 0, 72.0);
            });
            ui.horizontal(|ui| {
                switch(ui, true, "Include audio");
                select(ui, "AAC 192 kbps", 150.0);
                check(ui, false, "Transparent bg (MOV/WebM)");
            });
        });
    });
    footer(
        ui,
        "~ 12.1 MB · 00:05 · 1920×1200",
        None,
        &["Cancel", "Export"],
    );
}

/// Dialog / Export audio.
pub(super) fn export_audio(ui: &mut Ui) {
    let w = ui.available_width();
    segmented(ui, &["WAV", "MP3", "OGG", "AAC"], 1);
    let (span, _) = ui.allocate_exact_size(egui::vec2(w, 40.0), egui::Sense::hover());
    tl::paint_span(ui.painter(), ui.ctx(), span, tl::SpanType::Sound, true);
    group(ui, "Source");
    ui.horizontal(|ui| {
        select(ui, "Timeline mix (all sound layers)", 300.0);
        select(ui, "Range: whole movie", 280.0);
    });
    group(ui, "Format");
    ui.horizontal(|ui| {
        select(ui, "44.1 kHz", 140.0);
        select(ui, "16-bit", 120.0);
        select(ui, "Stereo", 120.0);
        select(ui, "192 kbps CBR", 180.0);
    });
    group(ui, "Processing");
    ui.horizontal(|ui| {
        switch(ui, true, "Normalize to −1 dB");
        stepper(ui, 1.0, "Fade out", "Fade out", "s", 1, 160.0);
        check(ui, false, "Trim silence");
    });
    footer(
        ui,
        "music.mp3 + sfx · 00:32.4 · ~ 760 kB",
        None,
        &["Cancel", "Export"],
    );
}

/// Dialog / Import video.
pub(super) fn import_video(ui: &mut Ui) {
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(300.0);
            preview_box(ui, 300.0, 169.0, "Video preview");
            let (span, _) = ui.allocate_exact_size(egui::vec2(300.0, 27.0), egui::Sense::hover());
            tl::paint_span(ui.painter(), ui.ctx(), span, tl::SpanType::Video, true);
            ui.horizontal(|ui| {
                text(ui, "00:00.00", "In", 146.0);
                text(ui, "00:24.00", "Out", 146.0);
            });
            caption(ui, "Drag the in / out handles on the strip to trim.");
        });
        ui.add_space(space::S5);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = space::S2;
            ui.horizontal(|ui| {
                text(ui, "~/Movies/intro.mp4", "File", 260.0);
                let _ = ui.add(c::Button::new("Browse…"));
            });
            group(ui, "Deployment");
            for (on, label, hint) in [
                (
                    true,
                    "Embed in timeline",
                    "Frames sync with the timeline; best for short clips",
                ),
                (
                    false,
                    "Load external video",
                    "Streams with a player component; keeps .nfl small",
                ),
                (
                    false,
                    "Progressive download",
                    "Plays while downloading from a URL",
                ),
            ] {
                ui.add(c::Radio::new(on, label));
                caption(ui, hint);
            }
            group(ui, "Embedding");
            ui.horizontal(|ui| {
                select(ui, "Symbol: Movie clip", 172.0);
                select(ui, "Codec: H.264", 172.0);
            });
            ui.horizontal(|ui| {
                check(ui, true, "Place on stage");
                check(ui, true, "Expand timeline if needed");
            });
            ui.horizontal(|ui| {
                check(ui, true, "Include audio");
                check(ui, true, "Match document frame rate");
            });
        });
    });
    footer(
        ui,
        "1280×720 · 30 fps → 24 fps · 24.0 s · ~6.1 MB",
        None,
        &["Cancel", "Import"],
    );
}

/// Dialog / Sound properties.
pub(super) fn sound_properties(ui: &mut Ui) {
    let w = ui.available_width();
    ui.horizontal(|ui| {
        let (span, _) = ui.allocate_exact_size(egui::vec2(120.0, 64.0), egui::Sense::hover());
        tl::paint_span(ui.painter(), ui.ctx(), span, tl::SpanType::Sound, true);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = space::S1;
            text(ui, "music.mp3", "", w - 120.0 - space::S2);
            caption(ui, "~/Projects/ideas/audio/music.mp3");
            caption(ui, "44 kHz · Stereo · 16 bit · 32.4 s · 5.7 MB");
        });
    });
    group(ui, "Export");
    select(ui, "Compression: MP3", w);
    ui.horizontal(|ui| {
        let half = (w - space::S2) / 2.0;
        select(ui, "Bit rate: 128 kbps", half);
        select(ui, "Quality: Best", half);
    });
    check(ui, false, "Convert stereo to mono");
    caption(ui, "Result: 128 kbps stereo, 518.6 kB — 9 % of original");
    ui.horizontal(|ui| {
        let _ = ui.add(c::Button::ghost("Update"));
        let _ = ui.add(c::Button::ghost("Import…"));
        let _ = ui.add(c::Button::ghost("Test").icon(i16::PLAY));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let _ = ui.add(c::Button::primary("OK"));
            let _ = ui.add(c::Button::new("Cancel"));
        });
    });
}
