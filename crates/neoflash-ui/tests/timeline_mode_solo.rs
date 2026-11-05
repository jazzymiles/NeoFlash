//! Layer-row Mode cell (R-040) and Solo toggle (R-045), owner-approved 5 November 2026: the
//! classic row is unchanged by default, and the optional columns sit where the Figma frame puts them.
use eframe::egui;
use neoflash_ui::components::timeline::{
    self as tl, BLEND_MODE_GROUPS, LayerColumns, LayersHeaderColumns, ModeCell,
};
use neoflash_ui::metrics::{LAYER_COLUMN, LAYER_COLUMN_MODE_SOLO, LAYER_MODE, LAYER_TOGGLE};
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

fn row(label: &str) -> tl::LayerRow<'_> {
    tl::LayerRow {
        label,
        kind: tl::LayerKind::Layer,
        selected: false,
        indent: 0,
        editing: false,
        visible: true,
        locked: false,
        outline: false,
        color: egui::Color32::LIGHT_BLUE,
    }
}

#[test]
fn default_columns_keep_the_classic_row() {
    for t in [Theme::Dark, Theme::Light] {
        let (classic, explicit) = run(t, |ui| {
            let a = row("hand").show(ui, LAYER_COLUMN, 28.0);
            let b = row("hand").show_with(ui, LAYER_COLUMN, 28.0, LayerColumns::default());
            (
                (a.label_rect.width(), a.mode_rect),
                (b.label_rect.width(), b.mode_rect),
            )
        });
        assert_eq!(classic, explicit);
        assert!(classic.1.is_none());
    }
}

#[test]
fn mode_cell_and_solo_take_the_extra_92_pt() {
    assert_eq!(
        LAYER_COLUMN_MODE_SOLO,
        LAYER_COLUMN + LAYER_MODE + LAYER_TOGGLE + 8.0
    );
    for t in [Theme::Dark, Theme::Light] {
        let (classic, wide, guide) = run(t, |ui| {
            let classic = row("hand").show(ui, LAYER_COLUMN, 28.0);
            let cols = LayerColumns {
                mode: ModeCell::Mode("Multiply"),
                solo: Some(true),
            };
            let wide = row("hand").show_with(ui, LAYER_COLUMN_MODE_SOLO, 28.0, cols);
            let guide = row("Guide").show_with(
                ui,
                LAYER_COLUMN_MODE_SOLO,
                28.0,
                LayerColumns {
                    mode: ModeCell::Empty,
                    solo: Some(false),
                },
            );
            (
                classic.label_rect,
                (wide.row.rect, wide.label_rect, wide.mode_rect),
                guide.mode_rect,
            )
        });
        let (row_rect, label, mode) = wide;
        // The label keeps the classic width: the 92 pt go to Mode + Solo.
        assert!(
            (label.width() - classic.width()).abs() < 0.5,
            "{label:?} {classic:?}"
        );
        let mode = mode.expect("mode cell");
        assert_eq!(mode.size(), egui::vec2(LAYER_MODE, 20.0));
        // Mode cell ends 4 pt before the four toggles (eye, solo, lock, outline) and the 8 pt inset.
        let toggles_left = row_rect.right() - 8.0 - 4.0 * LAYER_TOGGLE;
        assert!((mode.right() - (toggles_left - 4.0)).abs() < 0.5);
        // Empty rows reserve the same cell so columns line up.
        let guide = guide.expect("reserved cell");
        assert_eq!((guide.left(), guide.right()), (mode.left(), mode.right()));
    }
}

#[test]
fn header_with_columns_and_blend_mode_menu_groups() {
    for t in [Theme::Dark, Theme::Light] {
        let r = run(t, |ui| {
            tl::layers_header_with(
                ui,
                LAYER_COLUMN_MODE_SOLO,
                LayersHeaderColumns {
                    mode: true,
                    solo: true,
                },
            )
        });
        assert!(!r.clear_solo && !r.all_visibility);
    }
    let all: Vec<&str> = BLEND_MODE_GROUPS
        .iter()
        .flat_map(|g| g.iter().copied())
        .collect();
    assert_eq!(all.len(), 14);
    assert_eq!(all[0], "Normal");
    assert!(all.contains(&"Multiply") && all.contains(&"Erase"));
}
