//! Planned preview of Debugger (`scripting-panels.png`). All values are fixed samples from the reference.
use super::planned::check;
use eframe::egui::{self, Ui};
use neoflash_ui::components as c;
use neoflash_ui::icons::{self, i16};
use neoflash_ui::tokens::{TextStyle, space};

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
