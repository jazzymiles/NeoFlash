//! Preferences ▸ Shortcuts pieces (R-046, owner-approved 5 November 2026): row, recorder and
//! conflict strip lay out at the Figma sizes in both themes.
use eframe::egui;
use neoflash_ui::components::{self as c, shortcuts};
use neoflash_ui::{Theme, theme, typography};

fn run<R>(theme_: Theme, mut content: impl FnMut(&mut egui::Ui) -> R) -> R {
    let ctx = egui::Context::default();
    ctx.set_fonts(typography::font_definitions());
    theme::install(&ctx, theme_);
    let mut out = None;
    for _ in 0..3 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 900.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| out = Some(content(ui)));
        });
    }
    out.expect("ran")
}

#[test]
fn rows_are_32_pt_with_a_140_pt_cell_on_the_right() {
    for t in [Theme::Dark, Theme::Light] {
        let (assigned, empty) = run(t, |ui| {
            let a = c::ShortcutRow::new("Keyframe")
                .chords(&[&["F6"]])
                .conflict("Also on Convert to keyframes")
                .show(ui, 650.0);
            let e = c::ShortcutRow::new("Motion tween").show(ui, 650.0);
            ((a.row.rect, a.cell.rect), (e.row.rect, e.cell.rect))
        });
        for (row, cell) in [assigned, empty] {
            assert_eq!(row.size(), egui::vec2(650.0, shortcuts::SHORTCUT_ROW));
            assert_eq!(cell.width(), shortcuts::SHORTCUT_CELL);
            assert!((row.right() - cell.right() - 12.0).abs() < 0.5);
        }
    }
}

#[test]
fn recorder_and_conflict_strip_report_nothing_without_clicks() {
    for t in [Theme::Dark, Theme::Light] {
        let (waiting, captured, conflict) = run(t, |ui| {
            (
                c::ShortcutRecorder::new("Classic tween").show(ui, 650.0),
                c::ShortcutRecorder::new("Classic tween")
                    .captured(&["⌥", "⌘", "T"])
                    .show(ui, 650.0),
                c::ShortcutRecorder::new("Classic tween")
                    .captured(&["F6"])
                    .conflict_with("Insert ▸ Keyframe")
                    .show(ui, 650.0),
            )
        });
        for r in [waiting, captured, conflict] {
            assert_eq!(r, c::ShortcutRecorderResponse::default());
        }
        let h = run(t, |ui| {
            let top = ui.cursor().top();
            c::ShortcutRecorder::new("Classic tween")
                .captured(&["F6"])
                .conflict_with("Insert ▸ Keyframe")
                .show(ui, 650.0);
            ui.cursor().top() - top
        });
        // Row (40 pt) + 4 pt + strip (at least one 32 pt row).
        assert!(h >= 40.0 + 4.0 + 32.0, "{h}");
    }
}
