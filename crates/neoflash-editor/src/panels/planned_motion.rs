//! Planned previews: Motion editor, Properties — Motion tween, Ease panel (references `motion-panels.png`,
//! `panel-ease.png`, `ease-range-states.png`). Motion tweens and the Ease panel are not supported by the
//! engine yet; classic-tween easing stays editable in Properties ▸ Easing.
use super::planned::{caption, check, mono, num, select, stepper, type_header};
use eframe::egui::{self, Ui};
use neoflash_ui::components as c;
use neoflash_ui::icons::i16;
use neoflash_ui::metrics;
use neoflash_ui::tokens::{TextStyle, space};
use neoflash_ui::typography::rich;

struct EaseState {
    context: &'static str,
    preset: c::EasePresetKind,
    range: c::EaseRange,
    bezier: [f32; 4],
}

const EASE_STATES: [EaseState; 3] = [
    EaseState {
        context: "Classic tween · ball · frames 1–40",
        preset: c::EasePresetKind::Custom,
        range: c::EaseRange::Normal,
        bezier: [0.6, 0.02, 0.4, 0.98],
    },
    EaseState {
        context: "Back out · overshoots to 112 %",
        preset: c::EasePresetKind::Back,
        range: c::EaseRange::Overshoot,
        bezier: [0.3, 0.0, 0.55, 1.3],
    },
    EaseState {
        context: "Anticipation · dips to −8 % first",
        preset: c::EasePresetKind::Back,
        range: c::EaseRange::Undershoot,
        bezier: [0.35, -0.3, 0.7, 1.0],
    },
];

/// Panel / Ease. `range` switches between the three reference states (preview navigation only).
pub(super) fn ease(ui: &mut Ui, range: &mut usize) {
    ui.horizontal(|ui| {
        caption(ui, "Preview state");
        c::Segmented::show(
            ui,
            range,
            &[(0, "0–100 %"), (1, "Overshoot"), (2, "Undershoot")],
            false,
        );
    });
    let s = &EASE_STATES[(*range).min(2)];
    ui.scope(|ui| {
        ui.disable();
        ui.spacing_mut().item_spacing.y = space::S3;
        caption(ui, s.context);
        ui.horizontal(|ui| {
            select(ui, "Property: All", 176.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                check(ui, true, "One curve for all")
            });
        });
        for row in c::EasePresetKind::ALL.chunks(4) {
            ui.horizontal(|ui| {
                for preset in row {
                    ui.add(c::EasePreset {
                        preset: *preset,
                        selected: *preset == s.preset,
                    });
                }
            });
        }
        ui.add(c::EaseGraph {
            range: s.range,
            bezier: s.bezier,
            frames: 40,
            playhead: Some(30),
            width: ui.available_width(),
        });
        ui.horizontal(|ui| {
            let w = (ui.available_width() - 3.0 * space::S2) / 4.0;
            for (i, prefix) in ["X1", "Y1", "X2", "Y2"].into_iter().enumerate() {
                num(ui, s.bezier[i] as f64, prefix, prefix, "", 2, w);
            }
        });
        c::ease_preview_track(ui, s.bezier, 0.75, ui.available_width());
        ui.horizontal(|ui| {
            let _ = ui.add(c::IconButton::new(i16::PLAY, "Play preview"));
            let _ = ui.add(c::IconButton::new(i16::STOP, "Stop preview"));
            mono(ui, "Frame 30 / 40  ·  88 %");
        });
        super::planned::footer(ui, "", Some("Reset"), &["Cancel", "Apply"]);
    });
}

/// Panel / Properties — Motion tween.
pub(super) fn motion_tween(ui: &mut Ui) {
    type_header(
        ui,
        i16::TWEEN_MOTION,
        "Motion tween",
        "Layer \"hand\" · frames 1–60",
    );
    let half = (ui.available_width() - space::S2) / 2.0;
    c::SectionHeader::new("Ease")
        .id_salt("planned-mt-ease")
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                stepper(ui, -40.0, "Ease", "Ease", "(in)", 0, 160.0);
                let _ = ui.add(c::Button::ghost("Edit curve…"));
            });
        });
    c::SectionHeader::new("Rotation")
        .id_salt("planned-mt-rotation")
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                stepper(ui, 1.0, "Rotate", "Rotate", "×", 0, half);
                select(ui, "Direction: CW", half);
            });
            check(ui, true, "Orient to path");
        });
    c::SectionHeader::new("Path")
        .id_salt("planned-mt-path")
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                stepper(ui, 412.0, "X", "X", "", 1, half);
                stepper(ui, 160.0, "Y", "Y", "", 1, half);
            });
            ui.horizontal(|ui| {
                stepper(ui, 220.0, "W", "W", "", 1, half);
                stepper(ui, 120.0, "H", "H", "", 1, half);
            });
            ui.horizontal(|ui| {
                check(ui, true, "Show path");
                check(ui, true, "Show frame dots");
            });
        });
    c::SectionHeader::new("Options")
        .id_salt("planned-mt-options")
        .show(ui, |ui| {
            check(ui, true, "Sync graphic symbols");
            check(ui, false, "Snap to motion guide");
        });
}

enum MotionRow {
    Section(&'static str),
    Property {
        name: &'static str,
        value: f64,
        unit: &'static str,
        decimals: usize,
        ease: &'static str,
        keys: &'static [(f32, f32)],
        selected: bool,
    },
}

const MOTION_ROWS: &[MotionRow] = &[
    MotionRow::Section("Basic motion"),
    MotionRow::Property {
        name: "X",
        value: 412.0,
        unit: "px",
        decimals: 1,
        ease: "Simple (slow)",
        keys: &[(0.0, 0.05), (17.0, 0.8), (36.0, 0.5), (59.0, 0.95)],
        selected: true,
    },
    MotionRow::Property {
        name: "Y",
        value: 160.0,
        unit: "px",
        decimals: 1,
        ease: "No ease",
        keys: &[(0.0, 0.65), (30.0, 0.4), (59.0, 0.65)],
        selected: false,
    },
    MotionRow::Property {
        name: "Rotation Z",
        value: 0.0,
        unit: "°",
        decimals: 0,
        ease: "No ease",
        keys: &[(0.0, 0.5), (59.0, 0.5)],
        selected: false,
    },
    MotionRow::Section("Transformation"),
    MotionRow::Property {
        name: "Scale X",
        value: 100.0,
        unit: "%",
        decimals: 0,
        ease: "Bounce",
        keys: &[(0.0, 0.3), (24.0, 0.65), (59.0, 0.3)],
        selected: false,
    },
    MotionRow::Property {
        name: "Scale Y",
        value: 100.0,
        unit: "%",
        decimals: 0,
        ease: "Bounce",
        keys: &[(0.0, 0.3), (24.0, 0.65), (59.0, 0.3)],
        selected: false,
    },
    MotionRow::Section("Colour effect"),
    MotionRow::Property {
        name: "Alpha",
        value: 100.0,
        unit: "%",
        decimals: 0,
        ease: "No ease",
        keys: &[(0.0, 0.1), (11.0, 0.85), (59.0, 0.85)],
        selected: false,
    },
    MotionRow::Section("Filters"),
];

/// Panel / Motion editor: property column (448) + one curve lane per property over 60 frames.
pub(super) fn motion_editor(ui: &mut Ui) {
    const FRAMES: u32 = 60;
    const LEFT: f32 = 448.0;
    let nf = neoflash_ui::palette(ui.ctx());
    ui.spacing_mut().item_spacing.y = 0.0;
    let width = ui.available_width();
    let top = ui.cursor().min.y;
    let (header, _) = ui.allocate_exact_size(egui::vec2(width, metrics::ROW), egui::Sense::hover());
    // Children via `new_child` so the row allocations above stay the only layout advance.
    ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
        header.min,
        egui::vec2(LEFT, metrics::ROW),
    )))
    .add(c::TableRow {
        cells: &["Property", "Value", "Ease", "Keyframe"],
        kind: c::TableRowKind::Header,
        columns: &[0.0, 0.3, 0.52, 0.76],
    });
    let lanes_x = header.left() + LEFT;
    let scale = egui::Rect::from_min_max(egui::pos2(lanes_x, header.top()), header.max);
    ui.painter().rect_filled(scale, 0.0, nf.bg_raised);
    c::paint_frame_scale(ui.painter(), ui.ctx(), scale, FRAMES, 5);
    for row in MOTION_ROWS {
        match row {
            MotionRow::Section(label) => {
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(width, metrics::SECTION_HEADER),
                    egui::Sense::hover(),
                );
                ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                    rect.min,
                    egui::vec2(LEFT - space::S2, rect.height()),
                )))
                .horizontal(|ui| {
                    ui.allocate_ui(egui::vec2(LEFT - 48.0, rect.height()), |ui| {
                        c::SectionHeader::new(label)
                            .id_salt(("planned-me", *label))
                            .show(ui, |_| {});
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let _ = ui.add(c::IconButton::new(i16::PLUS, "Add property").side(24.0));
                    });
                });
                ui.painter().hline(
                    rect.x_range(),
                    rect.bottom() - 0.5,
                    egui::Stroke::new(1.0_f32, nf.border_subtle),
                );
            }
            MotionRow::Property {
                name,
                value,
                unit,
                decimals,
                ease,
                keys,
                selected,
            } => {
                let h = if *selected {
                    120.0
                } else {
                    metrics::CONTROL + space::S2
                };
                let (rect, _) = ui.allocate_exact_size(egui::vec2(width, h), egui::Sense::hover());
                let lane = egui::Rect::from_min_max(egui::pos2(lanes_x, rect.top()), rect.max);
                let lane_keys: Vec<c::LaneKey> = keys
                    .iter()
                    .map(|(frame, value)| c::LaneKey {
                        frame: *frame,
                        value: *value,
                    })
                    .collect();
                if *selected {
                    ui.painter().rect_filled(rect, 0.0, nf.bg_selected);
                }
                c::paint_curve_lane(ui.painter(), ui.ctx(), lane, FRAMES, &lane_keys, *selected);
                let left = egui::Rect::from_min_size(
                    rect.min,
                    egui::vec2(LEFT, metrics::CONTROL + space::S2),
                );
                ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(left.shrink2(egui::vec2(space::S3, space::S1)))
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                )
                .scope(|ui| {
                    let (label, _) = ui.allocate_exact_size(
                        egui::vec2(120.0, metrics::TOGGLE_ROW),
                        egui::Sense::hover(),
                    );
                    ui.painter().text(
                        label.left_center() + egui::vec2(space::S4, 0.0),
                        egui::Align2::LEFT_CENTER,
                        *name,
                        neoflash_ui::typography::font(TextStyle::Body),
                        nf.text_primary,
                    );
                    num(ui, *value, name, "", unit, *decimals, 96.0);
                    select(ui, ease, 120.0);
                    let _ =
                        ui.add(c::IconButton::new(i16::STEPBACK, "Previous keyframe").side(24.0));
                    let (diamond, _) =
                        ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                    c::motion::paint_key_diamond(
                        ui.painter(),
                        diamond.center(),
                        if *selected {
                            nf.accent_default
                        } else {
                            nf.text_tertiary
                        },
                    );
                    let _ = ui.add(c::IconButton::new(i16::STEPFWD, "Next keyframe").side(24.0));
                });
            }
        }
    }
    let bottom = ui.cursor().min.y;
    ui.painter().vline(
        lanes_x,
        scale.bottom()..=bottom,
        egui::Stroke::new(1.0_f32, nf.border_subtle),
    );
    let playhead =
        lanes_x + space::S3 + 29.0 / (FRAMES - 1) as f32 * (width - LEFT - 2.0 * space::S3);
    c::paint_lane_playhead(
        ui.painter(),
        ui.ctx(),
        playhead,
        egui::Rect::from_min_max(egui::pos2(lanes_x, top), scale.max),
        bottom,
        30,
    );
    ui.add_space(space::S3);
    ui.spacing_mut().item_spacing = egui::vec2(space::S3, space::S2);
    ui.horizontal(|ui| {
        caption(ui, "Graph size");
        super::planned::slider(ui, 55.0, "Graph size", 120.0);
        caption(ui, "Expanded");
        super::planned::slider(ui, 70.0, "Expanded graph size", 120.0);
        stepper(ui, 60.0, "Frames", "Frames", "", 0, 120.0);
        ui.label(
            rich(
                "Click a curve to edit · ⌥-drag a keyframe to copy · ⇧ snaps to frames",
                TextStyle::Small,
            )
            .color(nf.text_secondary),
        );
    });
}
