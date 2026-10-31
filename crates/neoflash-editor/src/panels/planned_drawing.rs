//! Planned previews of drawing-tool panels: Properties — Oval primitive (`panel-properties-oval-primitive.png`),
//! Rectangle primitive and PolyStar tool settings (`drawing-tools-panels.png`), Deco tool, Spray brush and
//! IK bone (`drawing-tools-panels-2.png`). Fixed sample values; sample artwork colours are HSV data.
use super::planned::{caption, check, form_label, num, select, stepper, switch, text};
use eframe::egui::{self, Color32, Ui};
use neoflash_ui::components as c;
use neoflash_ui::components::color_picker::hsv;
use neoflash_ui::icons::{i16, i20};
use neoflash_ui::tokens::{TextStyle, radius, space};
use neoflash_ui::typography::rich;

fn half(ui: &Ui) -> f32 {
    (ui.available_width() - space::S2) / 2.0
}

fn section(ui: &mut Ui, label: &str, open: bool, body: impl FnOnce(&mut Ui)) {
    c::SectionHeader::new(label)
        .id_salt(("planned-drawing", label))
        .default_open(open)
        .show(ui, body);
}

fn pair_num(ui: &mut Ui, a: (&str, f64, &str), b: (&str, f64, &str), decimals: usize, w: f32) {
    ui.horizontal(|ui| {
        num(ui, a.1, a.0, a.0, a.2, decimals, w);
        num(ui, b.1, b.0, b.0, b.2, decimals, w);
    });
}

fn pair_stepper(ui: &mut Ui, a: (&str, f64, &str), b: (&str, f64, &str), decimals: usize) {
    let w = half(ui);
    ui.horizontal(|ui| {
        stepper(ui, a.1, a.0, a.0, a.2, decimals, w);
        stepper(ui, b.1, b.0, b.0, b.2, decimals, w);
    });
}

/// Fill and Stroke wells (fill first, as in the primitive references).
fn wells(ui: &mut Ui, fill: Color32) {
    ui.horizontal(|ui| {
        ui.add(c::ColorWell {
            target: c::ColorWellTarget::Fill,
            color: Some(fill),
            active: false,
        });
        ui.add(c::ColorWell {
            target: c::ColorWellTarget::Stroke,
            color: Some(hsv(0.0, 0.0, 0.0, 1.0).into()),
            active: false,
        });
    });
}

/// Stroke size (`NF2/Combo Field`) + `NF2/Line Style Select`.
fn stroke_row(ui: &mut Ui, size: f64) {
    ui.horizontal(|ui| {
        let mut v = size;
        ui.add(
            c::Field::new(&mut v, "Stroke size")
                .presets(&[0.25, 0.5, 1.0, 2.0, 4.0, 8.0])
                .width(80.0),
        );
        let mut style = c::LineStyle::Solid;
        let w = ui.available_width();
        c::LineStyleSelect::show(ui, "planned-line-style", &mut style, w);
    });
}

fn link_button(ui: &mut Ui) {
    let _ = ui.add(c::IconButton::new(i16::LINK, "Lock proportions").selected(true));
}

/// Panel / Properties — Oval primitive.
pub(super) fn properties_oval(ui: &mut Ui) {
    ui.add(c::TypeHeader {
        icon: i20::TOOL_OVAL,
        title: "Oval primitive",
        subtitle: "Drawing object · layer \"ball\"",
    });
    let field = (ui.available_width() - 32.0 - 2.0 * space::S2) / 2.0;
    section(ui, "Position and size", true, |ui| {
        ui.horizontal(|ui| {
            num(ui, 100.0, "X", "X", "", 1, field);
            num(ui, 100.0, "Y", "Y", "", 1, field);
            link_button(ui);
        });
        pair_num(ui, ("W", 50.0, ""), ("H", 50.0, ""), 1, field);
    });
    section(ui, "Fill and stroke", true, |ui| {
        wells(ui, hsv(0.6, 0.75, 0.75, 1.0).into());
        stroke_row(ui, 2.0);
        ui.horizontal(|ui| {
            let _ = ui.add(c::Button::new("Custom…").icon(i16::SETTINGS));
            switch(ui, false, "Stroke hinting");
        });
        let w = half(ui);
        ui.horizontal(|ui| {
            select(ui, "Scale: Normal", w);
            num(ui, 3.0, "Miter", "Miter", "", 0, w);
        });
        for (label, items) in [
            (
                "Cap",
                [
                    (i16::CAP_NONE, "No cap"),
                    (i16::CAP_ROUND, "Round cap"),
                    (i16::CAP_SQUARE, "Square cap"),
                ],
            ),
            (
                "Join",
                [
                    (i16::JOIN_MITER, "Miter join"),
                    (i16::JOIN_ROUND, "Round join"),
                    (i16::JOIN_BEVEL, "Bevel join"),
                ],
            ),
        ] {
            ui.horizontal(|ui| {
                form_label(ui, label, 48.0);
                c::IconGroup {
                    items: &items,
                    selected: Some(1),
                }
                .show(ui);
            });
        }
    });
    section(ui, "Oval options", true, |ui| {
        pair_stepper(ui, ("Start", 90.0, "°"), ("End", 0.0, "°"), 0);
        let w = half(ui);
        ui.horizontal(|ui| {
            stepper(ui, 0.0, "Inner", "Inner", "%", 0, w);
            check(ui, true, "Close path");
        });
        let _ = ui.add(c::Button::ghost("Reset").icon(i16::RESET_TRANSFORM));
    });
}

/// Panel / Properties — Rectangle primitive.
pub(super) fn properties_rect(ui: &mut Ui) {
    ui.add(c::TypeHeader {
        icon: i20::TOOL_RECT_PRIMITIVE,
        title: "Rectangle primitive",
        subtitle: "Drawing object · editable corners",
    });
    section(ui, "Position and size", true, |ui| {
        let w = half(ui);
        pair_num(ui, ("X", 140.0, ""), ("Y", 96.0, ""), 1, w);
        pair_num(ui, ("W", 240.0, ""), ("H", 120.0, ""), 1, w);
    });
    section(ui, "Fill and stroke", true, |ui| {
        wells(ui, hsv(48.0 / 360.0, 1.0, 1.0, 1.0).into());
        stroke_row(ui, 1.0);
    });
    section(ui, "Rectangle options", true, |ui| {
        let w = (ui.available_width() - 32.0 - 2.0 * space::S2) / 2.0;
        ui.horizontal(|ui| {
            stepper(ui, 12.0, "Top-left radius", "↖", "", 0, w);
            stepper(ui, 12.0, "Top-right radius", "↗", "", 0, w);
            link_button(ui);
        });
        ui.horizontal(|ui| {
            stepper(ui, 12.0, "Bottom-left radius", "↙", "", 0, w);
            stepper(ui, 12.0, "Bottom-right radius", "↘", "", 0, w);
        });
        let mut v = 50.0;
        ui.add(c::Slider::new(&mut v, 0.0..=100.0, "Corner radius").width(2.0 * w + space::S2));
        let _ = ui.add(c::Button::ghost("Reset"));
    });
}

/// Dialog / PolyStar tool settings.
pub(super) fn polystar(ui: &mut Ui) {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 20.0;
        let (frame, _) = ui.allocate_exact_size(egui::vec2(110.0, 110.0), egui::Sense::hover());
        ui.painter().rect_filled(
            frame,
            egui::CornerRadius::same(radius::LG as u8),
            nf.bg_canvas,
        );
        // Sample star (5 points, point size 0.5) in the 90 × 90 artwork box.
        let center = frame.center();
        let points: Vec<egui::Pos2> = (0..10)
            .map(|i| {
                let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
                let r = if i % 2 == 0 { 45.0 } else { 22.5 };
                center + egui::vec2(a.cos(), a.sin()) * r
            })
            .collect();
        let fill: Color32 = hsv(0.6, 0.75, 0.75, 1.0).into();
        // Concave star: fill as triangles from the centre, then outline.
        for i in 0..10 {
            ui.painter().add(egui::Shape::convex_polygon(
                vec![center, points[i], points[(i + 1) % 10]],
                fill,
                egui::Stroke::NONE,
            ));
        }
        ui.painter().add(egui::Shape::closed_line(
            points,
            egui::Stroke::new(2.0_f32, Color32::from(hsv(0.0, 0.0, 0.0, 1.0))),
        ));
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 10.0;
            let w = ui.available_width();
            select(ui, "Style: Star", w);
            stepper(ui, 5.0, "Sides", "Sides", "", 0, w);
            stepper(ui, 0.5, "Point size", "Point size", "", 2, w);
            caption(ui, "Point size 0–1: smaller = sharper points");
        });
    });
    buttons(ui, &["Cancel", "OK"]);
}

/// Right-aligned dialog buttons (last = primary), no separator.
pub(super) fn buttons(ui: &mut Ui, labels: &[&str]) {
    ui.add_space(space::S2);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        for (i, b) in labels.iter().rev().enumerate() {
            let _ = ui.add(if i == 0 {
                c::Button::primary(b)
            } else {
                c::Button::new(b)
            });
        }
    });
}

/// Deco "Leaf" / "Flower" row: label, colour chip, shape name, Edit….
fn deco_row(ui: &mut Ui, label: &str, color: Color32) {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.horizontal(|ui| {
        form_label(ui, label, 64.0);
        let (chip, _) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::hover());
        neoflash_ui::components::display::paint_swatch(ui, chip, color, radius::SM);
        ui.label(rich("Default shape", TextStyle::Body).color(nf.text_secondary));
        let _ = ui.add(c::Button::ghost("Edit…"));
    });
}

/// Panel / Properties — Deco tool.
pub(super) fn deco_tool(ui: &mut Ui) {
    ui.add(c::TypeHeader {
        icon: i20::TOOL_DECO,
        title: "Deco tool",
        subtitle: "Procedural patterns from symbols",
    });
    section(ui, "Drawing effect", true, |ui| {
        select(ui, "Vine fill", ui.available_width());
        deco_row(ui, "Leaf", hsv(0.42, 0.6, 0.8, 1.0).into());
        deco_row(ui, "Flower", hsv(0.06, 0.6, 1.0, 1.0).into());
    });
    section(ui, "Advanced options", true, |ui| {
        pair_stepper(ui, ("Branch", 0.0, "°"), ("Scale", 100.0, "%"), 0);
        pair_stepper(ui, ("Segment", 6.0, "px"), ("Step", 1.0, "f"), 0);
        check(ui, true, "Animate pattern (one keyframe per step)");
    });
}

/// Panel / Properties — Spray brush.
pub(super) fn spray_brush(ui: &mut Ui) {
    ui.add(c::TypeHeader {
        icon: i20::TOOL_SPRAY,
        title: "Spray brush",
        subtitle: "Sprays copies of a symbol or dots",
    });
    section(ui, "Symbol", true, |ui| {
        ui.horizontal(|ui| {
            text(ui, "leaf_mc", "", 200.0);
            let _ = ui.add(c::Button::new("Edit…"));
        });
        check(ui, false, "Use default shape (dots)");
        pair_stepper(ui, ("W", 100.0, "%"), ("H", 100.0, "%"), 0);
        check(ui, true, "Random scaling");
        ui.horizontal(|ui| {
            check(ui, true, "Rotate symbol");
            check(ui, true, "Random rotation");
        });
    });
    section(ui, "Brush", true, |ui| {
        pair_stepper(ui, ("Width", 24.0, "px"), ("Height", 24.0, "px"), 0);
        let w = half(ui);
        stepper(ui, 0.0, "Angle", "Angle", "°", 0, w);
    });
}

/// Panel / Properties — IK bone.
pub(super) fn bone(ui: &mut Ui) {
    ui.add(c::TypeHeader {
        icon: i16::ARMATURE,
        title: "IK bone",
        subtitle: "Armature \"arm\" · bone 2 of 3",
    });
    ui.horizontal(|ui| {
        text(ui, "arm_lower", "", 200.0);
        let _ = ui.add(c::IconButton::new(i16::CHEVUP, "Parent bone"));
        let _ = ui.add(c::IconButton::new(i16::CHEVDOWN, "Child bone"));
    });
    section(ui, "Position", true, |ui| {
        let w = half(ui);
        pair_num(ui, ("X", 210.4, ""), ("Y", 188.0, ""), 1, w);
        pair_num(ui, ("Length", 96.0, "px"), ("Angle", -32.0, "°"), 0, w);
    });
    section(ui, "Joint: rotation", true, |ui| {
        ui.horizontal(|ui| {
            switch(ui, true, "Enable");
            switch(ui, true, "Constrain");
        });
        pair_stepper(ui, ("Min", -45.0, "°"), ("Max", 90.0, "°"), 0);
    });
    section(ui, "Joint: X / Y translation", false, |ui| {
        ui.horizontal(|ui| {
            switch(ui, false, "Enable");
            switch(ui, false, "Constrain");
        });
    });
    section(ui, "Spring", true, |ui| {
        pair_stepper(ui, ("Strength", 20.0, ""), ("Damping", 40.0, ""), 0);
    });
    section(ui, "Armature", true, |ui| {
        let w = half(ui);
        ui.horizontal(|ui| {
            select(ui, "Type: Authortime", w);
            select(ui, "Style: Solid", w);
        });
        ui.horizontal(|ui| {
            stepper(ui, 100.0, "Speed", "Speed", "%", 0, w);
            switch(ui, true, "Enable spring");
        });
    });
}
