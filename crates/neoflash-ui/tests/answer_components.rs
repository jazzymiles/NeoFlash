//! Headless layout checks for the shared components added for the 5 November 2026 design answers
//! (R-022, R-032, R-033, R-035, R-036, R-038): they lay out at the Figma sizes in both themes.
use eframe::egui;
use neoflash_ui::components::{self as c, headers, weight};
use neoflash_ui::icons::i20;
use neoflash_ui::{Theme, theme, typography};

/// Run `content` for a few frames (fonts are installed on the first one) and return its last value.
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
fn document_tabs_clamp_between_96_and_220() {
    for t in [Theme::Dark, Theme::Light] {
        let widths = run(t, |ui| {
            ui.horizontal(|ui| {
                [
                    c::DocumentTab::new("A").show(ui),
                    c::DocumentTab::new("Ideas in motion")
                        .active(true)
                        .dirty(true)
                        .path("/Users/me/Ideas in motion.nflash")
                        .show(ui),
                    c::DocumentTab::new("A very long document name that never fits in a tab")
                        .folder("Archive")
                        .show(ui),
                ]
                .map(|r| (r.response.rect.width(), r.response.rect.height()))
            })
            .inner
        });
        assert_eq!(widths[0], (headers::DOCUMENT_TAB_MIN, 32.0));
        assert!(widths[1].0 > 96.0 && widths[1].0 < 220.0, "{widths:?}");
        assert_eq!(widths[2].0, headers::DOCUMENT_TAB_MAX);
    }
    use headers::DocumentTabTrailing as T;
    assert_eq!(T::of(true, false, false), T::Close);
    assert_eq!(T::of(true, false, true), T::Dirty);
    assert_eq!(T::of(false, false, true), T::Dirty);
    assert_eq!(T::of(false, true, true), T::Close);
    assert_eq!(T::of(false, false, false), T::Empty);
}

#[test]
fn inline_message_wraps_and_peek_hugs() {
    for t in [Theme::Dark, Theme::Light] {
        let (one, two, peek) = run(t, |ui| {
            let one = ui
                .add(c::InlineMessage::info("Needs HTTP(S) hosting.").width(360.0))
                .rect;
            let two = ui
                .add(
                    c::InlineMessage::warning(
                        "“My animation” already exists. Replace overwrites its saved layout, \
                         including panel sizes, floating panels and the active tabs.",
                    )
                    .width(240.0),
                )
                .rect;
            let peek = c::ErrorPeek::new(
                c::Severity::Error,
                "1120: Access of undefined property orbit_mc.",
            )
            .action("2 fixes ⌘.")
            .show(ui);
            (one, two, peek.response.rect)
        });
        assert_eq!(one.size(), egui::vec2(360.0, 16.0));
        assert!(two.height() >= 32.0, "warning should wrap: {two:?}");
        // Figma `Error peek`: 30 pt high; it hugs the text (~370 pt with the v2.2 13 pt scale,
        // ~310 pt with the owner-approved type scale B, NF2/Small 10).
        assert_eq!(peek.height(), 30.0);
        assert!((280.0..420.0).contains(&peek.width()), "{peek:?}");
    }
}

#[test]
fn weight_bar_legend_and_totals_lay_out() {
    let segments = [
        (c::WeightPart::WebAssembly, 1_620_000, "1.62 MB"),
        (c::WeightPart::RuntimeScripts, 410_000, "410 KB"),
        (c::WeightPart::ProjectScripts, 38_000, "38 KB"),
        (c::WeightPart::Content, 120_000, "120 KB"),
        (c::WeightPart::Resources, 196_000, "196 KB"),
        (c::WeightPart::Fonts, 84_000, "84 KB"),
        (c::WeightPart::Page, 6_000, "6 KB"),
    ]
    .map(|(part, bytes, value)| c::WeightSegment { part, bytes, value });
    for t in [Theme::Dark, Theme::Light] {
        let (bar, legend, totals) = run(t, |ui| {
            let bar = c::WeightBar::new(&segments).show(ui, 566.0).rect;
            let legend = c::weight_legend(ui, &segments, 566.0).rect;
            let totals = c::weight_totals(
                ui,
                &[
                    ("On disk", "2.47 MB"),
                    ("Transfer (gzip)", "864 KB"),
                    ("Files", "7"),
                ],
            )
            .rect;
            (bar, legend, totals)
        });
        assert_eq!(bar.size(), egui::vec2(566.0, weight::WEIGHT_BAR));
        // Two legend rows as in the reference (36 pt).
        assert_eq!(legend.height(), 38.0);
        assert!(totals.width() > 200.0 && totals.height() == 16.0);
    }
}

#[test]
fn v22_components_have_figma_sizes() {
    for t in [Theme::Dark, Theme::Light] {
        let sizes = run(t, |ui| {
            ui.set_width(272.0);
            let flyout = ui
                .add(
                    c::ToolFlyoutItem::new(i20::TOOL_SELECT, "Selection tool")
                        .shortcut("V")
                        .active(true)
                        .width(260.0),
                )
                .rect
                .size();
            let preset = ui
                .add(
                    c::BrushPreset::new("Round", "12")
                        .stroke(c::BrushStroke::Round)
                        .selected(true)
                        .width(272.0),
                )
                .rect
                .size();
            let shape = ui
                .add(c::BrushShape::new(c::BrushShapeKind::Slash).selected(true))
                .rect
                .size();
            let mut value = 12.0;
            let field = ui
                .add(
                    c::Slider::new(&mut value, 1.0..=200.0, "Size")
                        .value(c::SliderValue::Field)
                        .unit("px"),
                )
                .rect
                .size();
            let inline = ui
                .add(c::Slider::new(&mut value, 1.0..=200.0, "Size").value(c::SliderValue::Inline))
                .rect
                .size();
            let painter = ui.painter();
            for (i, state) in [
                c::BrushCursorState::Idle,
                c::BrushCursorState::Resize,
                c::BrushCursorState::Eraser,
            ]
            .into_iter()
            .enumerate()
            {
                c::brush_cursor(
                    painter,
                    egui::pos2(100.0 + i as f32 * 100.0, 600.0),
                    c::BrushShapeKind::Round,
                    [26.0, 56.0, 500.0][i],
                    state,
                    Some("56 px"),
                );
            }
            for (i, stroke) in c::brushes::BrushStroke::ALL.into_iter().enumerate() {
                c::brush_stroke_sample(
                    painter,
                    egui::Rect::from_min_size(
                        egui::pos2(i as f32 * 100.0, 700.0),
                        egui::vec2(96.0, 20.0),
                    ),
                    stroke,
                    12.0,
                );
            }
            (flyout, preset, shape, field, inline)
        });
        assert_eq!(sizes.0, egui::vec2(260.0, 28.0));
        assert_eq!(sizes.1, egui::vec2(272.0, 36.0));
        assert_eq!(sizes.2, egui::vec2(24.0, 24.0));
        // Slider 140 + 10 gap + 74 field, 32 pt high (field height).
        assert_eq!(sizes.3, egui::vec2(224.0, 32.0));
        assert_eq!(sizes.4.y, 20.0);
    }
}

#[test]
fn keyboard_highlighted_menu_item_lays_out_like_a_row() {
    let h = run(Theme::Dark, |ui| {
        c::menu::menu_style(ui.style_mut());
        c::menu_section(ui, "2 fixes for 1120");
        ui.add(
            c::MenuItem::new("Use instance “orbit” (Library: orbit_mc)")
                .shortcut("↩")
                .highlighted(true),
        )
        .rect
        .height()
    });
    assert_eq!(h, 28.0);
}
