//! Planned previews of symbol, layer, stroke, font and document surfaces: Properties — Graphic looping and
//! Layer properties (`symbols-panels.png`), Stroke style (`dialogs-sound-stroke.png`), Font
//! embedding and the Test Movie window (`publish-misc-dialogs.png`), Find and replace and Movie Explorer
//! (`document-dialogs.png`), Symbols & scenes popover (`interface/*/png/editor-editing-symbol-in-place.png`).
//! Fixed sample values; sample artwork / document colours are HSV data, never UI tokens.
use super::planned::{caption, check, mono, select, stepper, switch, text};
use super::planned_dialogs::group;
use super::planned_drawing::buttons;
use eframe::egui::{self, Color32, Ui};
use neoflash_ui::components as c;
use neoflash_ui::components::color_picker::hsv;
use neoflash_ui::icons::i16;
use neoflash_ui::tokens::{TextStyle, radius, space};
use neoflash_ui::typography::{rich, rich_block};

fn half(ui: &Ui) -> f32 {
    (ui.available_width() - space::S2) / 2.0
}

fn search(ui: &mut Ui, value: &str, hint: &str) {
    let mut s = value.to_owned();
    let w = ui.available_width();
    ui.add(c::Search::new(&mut s, hint).width(w));
}

/// Sample planet artwork colour (orange dot in thumbnails / Test Movie).
fn planet_orange() -> Color32 {
    hsv(0.06, 0.7, 0.95, 1.0).into()
}

/// Panel / Properties — Graphic (looping options the real Properties panel does not offer yet: reverse
/// modes, Last frame, frame picker).
pub(super) fn graphic_properties(ui: &mut Ui) {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.add(c::TypeHeader {
        icon: i16::GRAPHIC,
        title: "Graphic",
        subtitle: "Instance of \"planet\" · 24 frames",
    });
    ui.horizontal(|ui| {
        let w = ui.available_width() - 72.0 - space::S2;
        ui.allocate_ui_with_layout(
            egui::vec2(w, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(w);
                ui.add(
                    egui::Label::new(
                        rich_block(
                            "Graphics have no instance name — their timeline is synced to the parent.",
                            TextStyle::Small,
                        )
                        .color(nf.text_secondary),
                    )
                    .wrap(),
                );
            },
        );
        let _ = ui.add(c::Button::new("Swap…"));
    });
    for label in ["Position and size", "Colour effect"] {
        c::SectionHeader::new(label)
            .id_salt(("planned-graphic", label))
            .default_open(false)
            .show(ui, |_| {});
    }
    c::SectionHeader::new("Looping")
        .id_salt("planned-graphic-looping")
        .show(ui, |ui| {
            for (i, label) in [
                "Loop",
                "Play once",
                "Single frame",
                "Loop reverse",
                "Play once reverse",
            ]
            .into_iter()
            .enumerate()
            {
                ui.add(c::Radio::new(i == 0, label));
            }
            let w = half(ui);
            ui.horizontal(|ui| {
                stepper(ui, 1.0, "First frame", "First", "", 0, w);
                stepper(ui, 24.0, "Last frame", "Last", "", 0, w);
            });
            // Frame picker: 36 pt thumbnails on a field strip; the first frame is selected.
            let (strip, _) = ui
                .allocate_exact_size(egui::vec2(ui.available_width(), 44.0), egui::Sense::hover());
            ui.painter().rect_filled(
                strip,
                egui::CornerRadius::same(radius::MD as u8),
                nf.bg_field,
            );
            for i in 0..7 {
                let cell = egui::Rect::from_min_size(
                    egui::pos2(strip.left() + 4.0 + i as f32 * 40.0, strip.top() + 4.0),
                    egui::vec2(36.0, 36.0),
                );
                ui.painter().rect_filled(
                    cell,
                    egui::CornerRadius::same(radius::SM as u8),
                    nf.bg_raised,
                );
                let (w, color) = if i == 0 {
                    (2.0_f32, nf.accent_default)
                } else {
                    (1.0, nf.border_subtle)
                };
                ui.painter().rect_stroke(
                    cell,
                    egui::CornerRadius::same(radius::SM as u8),
                    egui::Stroke::new(w, color),
                    egui::StrokeKind::Inside,
                );
                // The moon travels across the thumbnails like the planet symbol's timeline.
                let x = cell.left() + 8.0 + (i % 2) as f32 * 2.0;
                ui.painter().circle_filled(
                    egui::pos2(x + 2.0, cell.center().y),
                    4.0,
                    planet_orange(),
                );
            }
            caption(ui, "Click a thumbnail to set the first frame");
        });
}

/// Dialog / Layer properties.
pub(super) fn layer_properties(ui: &mut Ui) {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.spacing_mut().item_spacing.y = space::S3;
    text(ui, "hand", "Name", ui.available_width());
    ui.horizontal(|ui| {
        switch(ui, true, "Visible");
        switch(ui, false, "Locked");
    });
    group(ui, "Type");
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(space::S4, space::S2);
        for label in [
            "Normal", "Guide", "Guided", "Mask", "Masked", "Folder", "Armature",
        ] {
            ui.add(c::Radio::new(label == "Masked", label));
        }
    });
    group(ui, "Outline");
    ui.horizontal(|ui| {
        ui.label(rich("Colour", TextStyle::Body).color(nf.text_secondary));
        ui.add(c::SwatchCell {
            color: nf.layer_blue,
            selected: true,
        });
        check(ui, false, "View layer as outlines");
    });
    select(ui, "Layer height: 100 %", 200.0);
    buttons(ui, &["Cancel", "OK"]);
}

/// Dialog / Stroke style.
pub(super) fn stroke_style(ui: &mut Ui) {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.spacing_mut().item_spacing.y = space::S3;
    // 4× zoom of a 4 pt dashed stroke (dash 12, gap 8) in the sample stroke colour.
    let (preview, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 96.0), egui::Sense::hover());
    // Paper is sample document data (white), like the stroke colour, so the preview reads the same in both themes.
    ui.painter().rect_filled(
        preview,
        egui::CornerRadius::same(radius::MD as u8),
        Color32::from(hsv(0.0, 0.0, 1.0, 1.0)),
    );
    ui.painter().rect_stroke(
        preview,
        egui::CornerRadius::same(radius::MD as u8),
        egui::Stroke::new(1.0_f32, nf.border_subtle),
        egui::StrokeKind::Inside,
    );
    let y = preview.center().y;
    ui.painter().extend(egui::Shape::dashed_line(
        &[
            egui::pos2(preview.left() + 48.0, y),
            egui::pos2(preview.right() - 48.0, y),
        ],
        egui::Stroke::new(4.0_f32, Color32::from(hsv(0.0, 0.0, 0.0, 1.0))),
        12.0,
        8.0,
    ));
    ui.horizontal(|ui| {
        select(ui, "Type: Dashed", 196.0);
        let mut size = 4.0;
        ui.add(
            c::Field::new(&mut size, "Thickness")
                .unit("pt")
                .presets(&[0.25, 0.5, 1.0, 2.0, 4.0, 8.0])
                .width(108.0),
        );
        switch(ui, true, "4× zoom");
    });
    ui.horizontal(|ui| {
        stepper(ui, 12.0, "Dash", "Dash", "pt", 0, 140.0);
        stepper(ui, 8.0, "Gap", "Gap", "pt", 0, 140.0);
        check(ui, false, "Sharp corners");
    });
    ui.horizontal(|ui| {
        let _ = ui.add(c::Button::ghost("Reset"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let _ = ui.add(c::Button::primary("OK"));
            let _ = ui.add(c::Button::new("Cancel"));
        });
    });
}

/// Dialog / Font embedding.
pub(super) fn font_embedding(ui: &mut Ui) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = space::S5;
        ui.allocate_ui_with_layout(
            egui::vec2(220.0, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(220.0);
                ui.spacing_mut().item_spacing.y = 2.0;
                for (i, font) in ["Inter Regular", "Inter Semi Bold", "Roboto Mono"]
                    .into_iter()
                    .enumerate()
                {
                    c::TreeItem::new(font, c::TreeItemType::Symbol(i16::FONT))
                        .selected(i == 0)
                        .show(ui);
                }
                ui.horizontal(|ui| {
                    let _ = ui.add(c::IconButton::new(i16::PLUS, "Add font"));
                    let _ = ui.add(c::IconButton::new(i16::MINUS, "Remove font"));
                });
            },
        );
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = space::S3;
            ui.horizontal(|ui| {
                text(ui, "Inter Regular", "Name", 220.0);
                select(ui, "Family: Inter", 200.0);
            });
            select(ui, "Style: Regular", 220.0);
            group(ui, "Character ranges");
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(space::S4, space::S2);
                for (label, on) in [
                    ("Uppercase [A–Z] (27)", true),
                    ("Lowercase [a–z] (27)", true),
                    ("Numerals [0–9] (11)", true),
                    ("Punctuation (52)", true),
                    ("Basic Latin (95)", false),
                    ("Cyrillic (256)", true),
                    ("Latin Extended A (128)", false),
                    ("Greek (117)", false),
                ] {
                    check(ui, on, label);
                }
            });
            text(ui, "→ ← ★ © ®", "Also include", ui.available_width());
            ui.horizontal(|ui| {
                check(ui, true, "Export for ActionScript");
                text(ui, "InterRegular", "Class", 200.0);
            });
        });
    });
    ui.add_space(space::S2);
    ui.horizontal(|ui| {
        ui.add(c::Badge::new("378 glyphs · ~ 46 kB", c::BadgeType::Accent));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let _ = ui.add(c::Button::primary("OK"));
            let _ = ui.add(c::Button::new("Cancel"));
        });
    });
}

/// Window / Test Movie (title bar drawn by `planned.rs`): stage, transport, output drawer.
pub(super) fn test_movie(ui: &mut Ui) {
    let nf = neoflash_ui::palette(ui.ctx());
    // Stage area: pasteboard with the 640 × 400 document stage (document background #131824 as HSV data).
    let (area, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 448.0),
        egui::Sense::hover(),
    );
    ui.painter().rect_filled(area, 0.0, nf.bg_canvas);
    let stage = egui::Rect::from_center_size(area.center(), egui::vec2(640.0, 400.0));
    ui.painter()
        .rect_filled(stage, 0.0, Color32::from(hsv(0.59, 0.46, 0.094, 1.0)));
    let ink: Color32 = hsv(0.0, 0.0, 0.95, 1.0).into();
    ui.painter().text(
        stage.left_center() + egui::vec2(40.0, -90.0),
        egui::Align2::LEFT_CENTER,
        "Ideas in motion.",
        neoflash_ui::typography::font(TextStyle::DialogTitle),
        ink,
    );
    let orbit = stage.center() + egui::vec2(140.0, 20.0);
    ui.painter().circle_stroke(
        orbit,
        66.0,
        egui::Stroke::new(1.0_f32, Color32::from(hsv(0.0, 0.0, 0.4, 1.0))),
    );
    ui.painter()
        .circle_filled(orbit, 20.0, Color32::from(hsv(0.62, 0.6, 1.0, 1.0)));
    ui.painter()
        .circle_filled(orbit + egui::vec2(52.0, -34.0), 7.0, planet_orange());

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let _ = ui.add(c::IconButton::new(i16::FIRST, "First frame"));
        let _ = ui.add(c::IconButton::new(i16::STEPBACK, "Previous frame"));
        let _ = ui.add(
            c::IconButton::new(i16::PAUSE, "Pause")
                .kind(c::IconButtonType::Primary)
                .shortcut("↵"),
        );
        let _ = ui.add(c::IconButton::new(i16::STEPFWD, "Next frame"));
        let _ = ui.add(c::IconButton::new(i16::LAST, "Last frame"));
        let _ = ui.add(c::IconButton::new(i16::LOOP, "Loop").selected(true));
        let (rail, _) = ui.allocate_exact_size(egui::vec2(420.0, 6.0), egui::Sense::hover());
        let r = egui::CornerRadius::same(radius::FULL.min(127.0) as u8);
        ui.painter().rect_filled(rail, r, nf.bg_hover);
        let done = egui::Rect::from_min_size(
            rail.min,
            egui::vec2(rail.width() * 30.0 / 120.0, rail.height()),
        );
        ui.painter().rect_filled(done, r, nf.accent_default);
        mono(ui, "Frame 30 / 120 · 00:01.21");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let _ = ui.add(c::IconButton::new(i16::ROTATE_CW, "Restart"));
            select(ui, "Speed 1×", 110.0);
        });
    });
    ui.spacing_mut().item_spacing.y = 2.0;
    for (severity, line) in [
        (c::Severity::Info, "trace: NeoFlash: the stage is ready"),
        (c::Severity::Info, "trace: animator started (60 frames)"),
        (
            c::Severity::Warning,
            "Font \"Inter Semi Bold\" not embedded — fallback",
        ),
    ] {
        ui.add(c::LogLine {
            severity,
            time: "",
            text: line,
        });
    }
}

/// Title-bar trailing items of the Test Movie window: performance badge, browser preview, output drawer.
pub(super) fn test_movie_title_actions(ui: &mut Ui) {
    let _ = ui.add(c::IconButton::new(i16::SIDEBAR, "Output drawer").selected(true));
    let _ = ui.add(c::IconButton::new(i16::GLOBE, "Open in browser"));
    ui.add(c::Badge::new("24 / 24 fps · 38 MB", c::BadgeType::Neutral));
}

/// Dialog / Find and replace.
pub(super) fn find_replace(ui: &mut Ui) {
    ui.spacing_mut().item_spacing.y = space::S3;
    let mut kind = 0;
    c::Segmented::show(
        ui,
        &mut kind,
        &[
            (0, "Text"),
            (1, "Font"),
            (2, "Colour"),
            (3, "Symbol"),
            (4, "Sound"),
            (5, "Video"),
            (6, "Bitmap"),
        ],
        false,
    );
    ui.horizontal(|ui| {
        select(ui, "In: Current document", 220.0);
        select(ui, "Scene: All scenes", 200.0);
    });
    let w = ui.available_width();
    text(ui, "orbit_mc", "Find", w);
    text(ui, "orbitClip", "Replace", w);
    ui.horizontal(|ui| {
        check(ui, true, "Whole word");
        check(ui, false, "Match case");
        check(ui, false, "Regular expression");
        check(ui, false, "Live edit");
    });
    ui.horizontal(|ui| {
        check(ui, true, "Text field contents");
        check(ui, true, "Frames / layers / scenes");
        check(ui, true, "ActionScript");
    });
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let cols = &[0.0, 0.42];
        ui.add(c::TableRow {
            cells: &["Location", "Found"],
            kind: c::TableRowKind::Header,
            columns: cols,
        });
        for (i, (location, found)) in [
            (
                "Scene 1 › orbit › Frame 1 · line 17",
                "new Animator(orbitXML, orbit_mc)",
            ),
            (
                "Scene 1 › orbit › Frame 1 · line 18",
                "orbit_mc.rotation += speed",
            ),
            ("Orbit.as · line 42", "public var orbit_mc:MovieClip"),
            ("Library › orbit · instance name", "orbit_mc"),
        ]
        .into_iter()
        .enumerate()
        {
            ui.add(c::TableRow {
                cells: &[location, found],
                kind: if i == 0 {
                    c::TableRowKind::Selected
                } else {
                    c::TableRowKind::Row
                },
                columns: cols,
            });
        }
    });
    ui.horizontal(|ui| {
        caption(ui, "4 results in 3 places");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let _ = ui.add(c::Button::primary("Replace all"));
            let _ = ui.add(c::Button::new("Replace"));
            let _ = ui.add(c::Button::ghost("Find all"));
        });
    });
}

/// Panel / Movie Explorer.
pub(super) fn movie_explorer(ui: &mut Ui) {
    ui.spacing_mut().item_spacing.y = space::S1;
    ui.horizontal(|ui| {
        c::IconGroup {
            items: &[
                (i16::FONT, "Show text"),
                (i16::MOVIECLIP, "Show buttons, movie clips and graphics"),
                (i16::CODE, "Show ActionScript"),
                (i16::VIDEO, "Show video, sound and bitmaps"),
            ],
            selected: Some(1),
        }
        .show(ui);
        c::IconGroup {
            items: &[
                (i16::FRAME, "Show frames and layers"),
                (i16::SETTINGS, "Customise items to show"),
            ],
            selected: None,
        }
        .show(ui);
    });
    search(ui, "", "Find in movie");
    for (label, kind, depth, selected) in [
        ("Ideas in motion.nfl", c::TreeItemType::FolderOpen, 0, false),
        ("Scene 1", c::TreeItemType::FolderOpen, 1, false),
        ("Layer \"orbit\"", c::TreeItemType::FolderOpen, 2, false),
        ("Frame 1", c::TreeItemType::FolderOpen, 3, false),
        (
            "orbit_mc  ·  orbit",
            c::TreeItemType::Symbol(i16::MOVIECLIP),
            4,
            true,
        ),
        (
            "Actions for Frame 1",
            c::TreeItemType::Symbol(i16::CODE),
            4,
            false,
        ),
        ("Layer \"labels\"", c::TreeItemType::FolderClosed, 2, false),
        (
            "Layer \"music.mp3\"",
            c::TreeItemType::FolderClosed,
            2,
            false,
        ),
        (
            "Symbol definitions",
            c::TreeItemType::FolderClosed,
            1,
            false,
        ),
    ] {
        c::TreeItem::new(label, kind)
            .depth(depth)
            .selected(selected)
            .show(ui);
    }
    mono(ui, "Scene 1 › orbit › Frame 1 › orbit_mc");
}

/// Popover / Symbols & scenes (breadcrumb ▾): search, scenes, matching symbols, recent, browse, key hints.
pub(super) fn symbols_popover(ui: &mut Ui) {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.add_space(space::S1);
    search(ui, "pla", "Find symbol or scene");
    ui.add_space(space::S1);
    c::menu_section(ui, "Scenes · 4");
    c::TreeItem::new("Scene 1", c::TreeItemType::Symbol(i16::SCENE)).show(ui);
    c::menu_section(ui, "Matches in symbols · 3 of 248");
    c::TreeItem::new("planet", c::TreeItemType::Symbol(i16::GRAPHIC))
        .selected(true)
        .show(ui);
    c::TreeItem::new("planet_shadow", c::TreeItemType::Symbol(i16::GRAPHIC)).show(ui);
    c::TreeItem::new("planets_bg", c::TreeItemType::Symbol(i16::MOVIECLIP)).show(ui);
    c::menu_section(ui, "Recent");
    c::TreeItem::new("orbit", c::TreeItemType::Symbol(i16::MOVIECLIP)).show(ui);
    c::TreeItem::new("cta_btn", c::TreeItemType::Symbol(i16::BUTTON_SYMBOL)).show(ui);
    c::menu_section(ui, "Browse");
    for folder in ["Backgrounds  (41)", "Characters  (96)", "UI  (111)"] {
        c::TreeItem::new(folder, c::TreeItemType::FolderClosed).show(ui);
    }
    c::menu_separator(ui);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(space::S1, space::S1);
        ui.add_space(space::S2);
        for (key, action) in [
            ("↑↓", "move"),
            ("↵", "edit"),
            ("⌥↵", "in place"),
            ("⌘↵", "new window"),
        ] {
            c::kbd(ui, key, false);
            ui.label(rich(action, TextStyle::Small).color(nf.text_secondary));
            ui.add_space(space::S1);
        }
    });
}
