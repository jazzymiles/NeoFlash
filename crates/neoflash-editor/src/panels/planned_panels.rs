//! Planned previews of small panels: Debugger, Code snippets (`scripting-panels.png`), Compiler errors
//! (`panels-more-2.png`) and movie clip Display & Filters (`panel-properties-movie-clip.png`).
//! All values are fixed samples from the reference.
use super::planned::{caption, check, num, select, switch, type_header};
use eframe::egui::{self, Ui};
use neoflash_ui::components as c;
use neoflash_ui::icons::{self, i16};
use neoflash_ui::tokens::{TextStyle, radius, space};
use neoflash_ui::typography::rich;

fn search(ui: &mut Ui, hint: &str) {
    let mut s = String::new();
    let w = ui.available_width();
    ui.add(c::Search::new(&mut s, hint).width(w));
}

fn column_title(ui: &mut Ui, s: &str) {
    let nf = neoflash_ui::palette(ui.ctx());
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 30.0), egui::Sense::hover());
    ui.painter().text(
        rect.left_center() + egui::vec2(space::S3, 0.0),
        egui::Align2::LEFT_CENTER,
        s,
        neoflash_ui::typography::font(TextStyle::SmallStrong),
        nf.text_secondary,
    );
}

/// Icon-button row at the bottom of list panels: `left` icons, flexible gap, `right` icons.
fn action_row(
    ui: &mut Ui,
    left: &[(neoflash_ui::Icon, &str)],
    right: &[(neoflash_ui::Icon, &str)],
) {
    ui.horizontal(|ui| {
        for (icon, name) in left {
            let _ = ui.add(c::IconButton::new(*icon, name));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            for (icon, name) in right {
                let _ = ui.add(c::IconButton::new(*icon, name));
            }
        });
    });
}

fn column<R>(ui: &mut Ui, width: f32, content: impl FnOnce(&mut Ui) -> R) {
    ui.allocate_ui_with_layout(
        egui::vec2(width, 0.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing.y = 0.0;
            content(ui);
        },
    );
}

/// Panel / Debugger.
pub(super) fn debugger(ui: &mut Ui) {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let _ = ui.add(
            c::IconButton::new(i16::DEBUG_CONTINUE, "Continue")
                .kind(c::IconButtonType::Primary)
                .shortcut("F8"),
        );
        let _ = ui.add(c::IconButton::new(i16::STEP_OVER, "Step over").shortcut("F10"));
        let _ = ui.add(c::IconButton::new(i16::STEP_INTO, "Step in").shortcut("F11"));
        let _ = ui.add(c::IconButton::new(i16::STEP_OUT, "Step out").shortcut("⇧F11"));
        let _ = ui.add(c::IconButton::new(i16::STOP, "Stop debugging"));
        let _ = ui.add(c::IconButton::new(i16::BREAKPOINT, "Toggle breakpoint").shortcut("⌘B"));
        let _ = ui.add(c::Button::ghost("Remove all breakpoints"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add(c::Badge::new(
                "Paused · orbit : Frame 1 · line 18 (breakpoint)",
                c::BadgeType::Warning,
            ));
        });
    });
    let total = ui.available_width();
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        column(ui, 240.0, |ui| {
            column_title(ui, "Call stack");
            c::TreeItem::new(
                "orbit : Frame 1 · line 18",
                c::TreeItemType::Symbol(i16::EXEC_ARROW),
            )
            .selected(true)
            .show(ui);
            for frame in [
                "Animator.play() · 204",
                "MotionBase.update() · 88",
                "[global code]",
            ] {
                c::TreeItem::new(frame, c::TreeItemType::Symbol(i16::CALLSTACK)).show(ui);
            }
        });
        column(ui, 380.0, |ui| {
            column_title(ui, "Variables");
            let cols = &[0.0, 0.45];
            ui.add(c::TableRow {
                cells: &["Name", "Value"],
                kind: c::TableRowKind::Header,
                columns: cols,
            });
            for (name, value, kind) in [
                ("+ this", "MovieClip \"orbit\"", c::TableRowKind::Row),
                ("− animator", "Animator", c::TableRowKind::Row),
                ("    target", "orbit_mc → null", c::TableRowKind::Row),
                ("    speed", "2.5  (changed)", c::TableRowKind::Selected),
                ("+ orbitXML", "XML (14 nodes)", c::TableRowKind::Row),
                ("  frame", "1", c::TableRowKind::Row),
            ] {
                ui.add(c::TableRow {
                    cells: &[name, value],
                    kind,
                    columns: cols,
                });
            }
        });
        column(ui, (total - 620.0).max(240.0), |ui| {
            column_title(ui, "Watch");
            let cols = &[0.0, 0.5];
            ui.add(c::TableRow {
                cells: &["Expression", "Value"],
                kind: c::TableRowKind::Header,
                columns: cols,
            });
            for (expr, value) in [
                ("orbit_mc.rotation", "Error: null reference"),
                ("animator.time", "0.75"),
                ("+ add expression", ""),
            ] {
                ui.add(c::TableRow {
                    cells: &[expr, value],
                    kind: c::TableRowKind::Row,
                    columns: cols,
                });
            }
            column_title(ui, "Breakpoints");
            ui.spacing_mut().item_spacing.y = space::S1;
            for (label, on) in [
                ("orbit : Frame 1 · line 18", true),
                ("Orbit.as · line 42", true),
                ("Main.as · line 7", false),
            ] {
                ui.horizontal(|ui| {
                    ui.add_space(space::S3);
                    icons::show(ui, i16::BREAKPOINT, nf.syntax_breakpoint);
                    check(ui, on, label);
                });
            }
        });
    });
}

/// Panel / Code snippets.
pub(super) fn code_snippets(ui: &mut Ui) {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.spacing_mut().item_spacing.y = space::S1;
    search(ui, "Search snippets");
    c::TreeItem::new("Actions", c::TreeItemType::FolderOpen).show(ui);
    for s in [
        "Click to go to frame and stop",
        "Click to go to web page",
        "Drag and drop",
    ] {
        c::TreeItem::new(s, c::TreeItemType::Symbol(i16::SNIPPET))
            .depth(1)
            .show(ui);
    }
    c::TreeItem::new("Timeline navigation", c::TreeItemType::FolderClosed).show(ui);
    c::TreeItem::new("Animation", c::TreeItemType::FolderOpen).show(ui);
    c::TreeItem::new("Rotate continuously", c::TreeItemType::Symbol(i16::SNIPPET))
        .depth(1)
        .selected(true)
        .show(ui);
    c::TreeItem::new("Fade in movie clip", c::TreeItemType::Symbol(i16::SNIPPET))
        .depth(1)
        .show(ui);
    for f in ["Load and unload", "Audio and video", "Event handlers"] {
        c::TreeItem::new(f, c::TreeItemType::FolderClosed).show(ui);
    }
    ui.add_space(space::S1);
    egui::Frame::new()
        .fill(nf.bg_field)
        .stroke(egui::Stroke::new(1.0_f32, nf.border_subtle))
        .corner_radius(egui::CornerRadius::same(radius::MD as u8))
        .inner_margin(egui::Margin::same(space::S3 as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 2.0;
            caption(
                ui,
                "Rotates the selected movie clip every frame. Change 10 to adjust speed.",
            );
            ui.add_space(space::S1);
            for line in [
                "orbit_mc.addEventListener(",
                "  Event.ENTER_FRAME, spin);",
                "function spin(e:Event):void {",
                "  orbit_mc.rotation += 10;",
                "}",
            ] {
                ui.label(rich(line, TextStyle::Mono).color(nf.text_primary));
            }
        });
    ui.add_space(space::S1);
    ui.horizontal(|ui| {
        let _ = ui.add(c::Button::ghost("Copy"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let _ = ui.add(c::Button::primary("Insert at frame 1"));
        });
    });
}

/// Panel / Compiler errors.
pub(super) fn compiler_errors(ui: &mut Ui) {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.horizontal(|ui| {
        ui.add(c::Badge::new("1 warning", c::BadgeType::Warning));
        ui.add(c::Badge::new("1 error", c::BadgeType::Neutral));
    });
    for (icon, color, message, location) in [
        (
            i16::ERROR,
            nf.danger_default,
            "1120: Access of undefined property orbit_mc.",
            "Scene 1 · layer \"orbit\" · frame 1 · Line 19",
        ),
        (
            i16::WARNING,
            nf.warning_default,
            "1090: Migration issue — frameRate is read-only at runtime.",
            "Scene 1 · layer \"orbit\" · frame 1 · Line 4",
        ),
    ] {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            icons::show(ui, icon, color);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(rich(message, TextStyle::Body).color(nf.text_primary));
                ui.label(rich(location, TextStyle::Small).color(nf.text_secondary));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let _ = ui.add(c::IconButton::new(i16::EXTERNAL, "Go to source"));
            });
        });
    }
}

/// Panel / Properties — Movie clip, planned sections only (Display, Filters). Position, size and colour
/// effect already work in the real Properties panel.
pub(super) fn properties_filters(ui: &mut Ui) {
    type_header(ui, i16::MOVIECLIP, "Movie clip", "Instance of \"orbit\"");
    let half = (ui.available_width() - space::S2) / 2.0;
    c::SectionHeader::new("Display")
        .id_salt("planned-mc-display")
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                switch(ui, true, "Visible");
                check(ui, false, "Cache as bitmap");
            });
            ui.horizontal(|ui| {
                select(ui, "Blending: Normal", half);
                select(ui, "Render: Original", half);
            });
        });
    c::SectionHeader::new("Filters")
        .id_salt("planned-mc-filters")
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let cols = &[0.0, 0.5];
            ui.add(c::TableRow {
                cells: &["Filter", "Value"],
                kind: c::TableRowKind::Header,
                columns: cols,
            });
            ui.add(c::TableRow {
                cells: &["Drop shadow", "4 px · 45° · 60 %"],
                kind: c::TableRowKind::Row,
                columns: cols,
            });
            ui.add(c::TableRow {
                cells: &["Blur", "5 × 5 px"],
                kind: c::TableRowKind::Selected,
                columns: cols,
            });
            ui.add(c::TableRow {
                cells: &["Glow", "Off"],
                kind: c::TableRowKind::Row,
                columns: cols,
            });
            ui.add_space(space::S2);
            ui.spacing_mut().item_spacing.y = space::S2;
            ui.horizontal(|ui| {
                num(ui, 5.0, "Blur X", "Blur X", "px", 0, half);
                num(ui, 5.0, "Blur Y", "Blur Y", "px", 0, half);
            });
            ui.horizontal(|ui| {
                num(ui, 100.0, "Strength", "Strength", "%", 0, half);
                select(ui, "Quality: Low", half);
            });
            action_row(
                ui,
                &[(i16::PLUS, "Add filter"), (i16::COPY, "Duplicate filter")],
                &[(i16::TRASH, "Remove filter")],
            );
        });
}
