//! Every atom takes its size from the density metrics: the same layout is checked in Normal,
//! Compact and Classic Flash, with expectations derived from `neoflash_ui::metrics`.
use eframe::egui;
use neoflash_ui::components::{self as c, headers};
use neoflash_ui::icons::i16;
use neoflash_ui::metrics::{self, Density};
use neoflash_ui::tokens::space;
use neoflash_ui::{Theme, theme, typography};

/// Lay `content` out for a few frames in `density` and return its last value.
fn run<R>(density: Density, mut content: impl FnMut(&mut egui::Ui) -> R) -> R {
    metrics::set_density(density);
    let ctx = egui::Context::default();
    ctx.set_fonts(typography::font_definitions());
    theme::install(&ctx, Theme::Light);
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

fn each_density(mut check: impl FnMut(Density)) {
    for density in Density::ALL {
        check(density);
    }
    metrics::set_density(Density::Comfortable);
}

#[test]
fn buttons_fields_and_selects_are_one_control_high() {
    each_density(|density| {
        let (button, icon, field, select, search, color) = run(density, |ui| {
            let (mut value, mut text, mut choice) = (640.0, String::new(), 0);
            let mut colour = egui::Color32::from_rgb(0x23, 0x30, 0x43);
            let button = ui.add(c::Button::new("Swap…")).rect;
            let icon = ui.add(c::IconButton::new(i16::MORE, "More")).rect;
            let field = ui.add(c::Field::new(&mut value, "X").prefix("X")).rect;
            let select = c::Select::new("s", "Normal").show(ui, |_| {}).0.rect;
            let search = ui
                .add(c::Search::new(&mut text, "Search").width(200.0))
                .rect;
            let color = c::ColorField::new(&mut colour, "Fill")
                .show(ui)
                .response
                .rect;
            let _ = c::Select::options(ui, "o", &mut choice, &[(0, "A")], 90.0);
            (button, icon, field, select, search, color)
        });
        let control = metrics::control();
        for rect in [button, field, select, search, color] {
            assert_eq!(rect.height(), control, "{density:?}");
        }
        assert_eq!(icon.size(), egui::Vec2::splat(control), "{density:?}");
        // Defaults when the caller gives no width (the Ui is wider than every class).
        assert_eq!(field.width(), metrics::field_m(), "{density:?}");
        assert_eq!(select.width(), metrics::field_m(), "{density:?}");
        assert_eq!(color.width(), metrics::field_l(), "{density:?}");
        assert_eq!(c::button::button_padding(), space::s3() + 2.0);
    });
}

#[test]
fn icon_groups_and_segmented_controls_are_one_control_high() {
    each_density(|density| {
        let (group, segmented) = run(density, |ui| {
            let top = ui.cursor().left_top();
            let items = [
                (i16::ALIGN_LEFT, "Left"),
                (i16::ALIGN_HCENTER, "Centre"),
                (i16::ALIGN_RIGHT, "Right"),
            ];
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            c::IconGroup {
                items: &items,
                selected: Some(1),
            }
            .show(ui);
            let group = ui.cursor().left_top().y - top.y;
            let mut value = 0;
            c::Segmented::show(ui, &mut value, &[(0, "RGB"), (1, "HSB")], false);
            (group, ui.cursor().left_top().y - top.y - group)
        });
        assert_eq!(group, metrics::control(), "{density:?}");
        assert_eq!(segmented, metrics::control(), "{density:?}");
        assert_eq!(
            c::IconGroup::width(3),
            3.0 * (metrics::control() - 4.0) + 4.0
        );
    });
}

#[test]
fn a_narrow_ui_keeps_a_numeric_field_at_the_num_width() {
    each_density(|density| {
        let width = run(density, |ui| {
            ui.allocate_ui(egui::vec2(20.0, 40.0), |ui| {
                let mut value = 1.0;
                ui.add(c::Field::new(&mut value, "X")).rect.width()
            })
            .inner
        });
        assert_eq!(width, metrics::field_num(), "{density:?}");
    });
}

#[test]
fn toggles_use_the_toggle_metrics() {
    each_density(|density| {
        let (check, switch, radio, slider, badge, kbd) = run(density, |ui| {
            let (mut on, mut value) = (true, 50.0);
            (
                ui.add(c::Checkbox::new(&mut on, "")).rect,
                ui.add(c::Switch::new(&mut on, "")).rect,
                ui.add(c::Radio::new(true, "")).rect,
                ui.add(c::Slider::new(&mut value, 0.0..=100.0, "Opacity"))
                    .rect,
                ui.add(c::Badge::new("Badge", c::BadgeType::Neutral)).rect,
                c::kbd(ui, "F8", false).rect,
            )
        });
        let row = metrics::toggle_row();
        assert_eq!(
            check.size(),
            egui::vec2(metrics::check_box(), row),
            "{density:?}"
        );
        assert_eq!(
            switch.size(),
            egui::vec2(metrics::switch_width(), row),
            "{density:?}"
        );
        assert_eq!(
            radio.size(),
            egui::vec2(metrics::check_box(), row),
            "{density:?}"
        );
        assert_eq!(
            slider.size(),
            egui::vec2(metrics::field_m(), row),
            "{density:?}"
        );
        assert_eq!(badge.height(), metrics::badge(), "{density:?}");
        assert_eq!(kbd.height(), metrics::kbd(), "{density:?}");
        // The marks fit their rows on whole points.
        assert!(metrics::check_box() <= row && metrics::switch_height() <= row);
        assert!(metrics::slider_thumb() <= row);
    });
}

#[test]
fn headers_tabs_rows_and_menus_follow_the_density() {
    each_density(|density| {
        let sizes = run(density, |ui| {
            ui.spacing_mut().item_spacing.y = 17.0;
            let panel_tab = ui
                .add(c::PanelTab {
                    label: "Properties",
                    active: true,
                })
                .rect;
            let dock_tab = ui
                .add(c::DockTab {
                    label: "Timeline",
                    icon: None,
                    active: false,
                })
                .rect;
            let tab_width = headers::panel_tab_width(ui, "Properties");
            let section_top = ui.cursor().top();
            let body_top = c::SectionHeader::new("Position and size")
                .show(ui, |ui| ui.cursor().top())
                .expect("open by default");
            let type_header = ui
                .add(c::TypeHeader {
                    icon: i16::MOVIECLIP,
                    title: "Movie clip",
                    subtitle: "Instance of “Orbit”",
                })
                .rect;
            let dialog_top = ui.cursor().top();
            c::DialogHeader { title: "Dialog" }.show(ui);
            let dialog = ui.cursor().top() - dialog_top - 17.0;
            let tree = c::TreeItem::new("orbit", c::TreeItemType::Symbol(i16::MOVIECLIP))
                .show(ui)
                .row
                .rect;
            let menu = ui.add(c::MenuItem::new("Undo").shortcut("⌘Z")).rect;
            let separator_top = ui.cursor().top();
            c::menu_separator(ui);
            let separator = ui.cursor().top() - separator_top - 17.0;
            (
                panel_tab,
                dock_tab,
                tab_width,
                body_top - section_top,
                type_header,
                dialog,
                tree,
                menu,
                separator,
            )
        });
        let (panel_tab, dock_tab, tab_width, body, type_header, dialog, tree, menu, separator) =
            sizes;
        assert_eq!(panel_tab.height(), metrics::panel_tab(), "{density:?}");
        assert_eq!(panel_tab.width(), tab_width, "{density:?}");
        assert_eq!(dock_tab.height(), metrics::dock_tab(), "{density:?}");
        // The body starts one row gap below the header, whatever the host's row spacing is.
        assert_eq!(
            body,
            metrics::section_header() + metrics::row_gap(),
            "{density:?}"
        );
        assert_eq!(type_header.height(), metrics::panel_tab(), "{density:?}");
        assert_eq!(dialog, metrics::dialog_header(), "{density:?}");
        assert_eq!(tree.height(), metrics::row(), "{density:?}");
        assert_eq!(menu.height(), metrics::row(), "{density:?}");
        assert_eq!(separator, metrics::menu_separator(), "{density:?}");
    });
}

#[test]
fn floating_header_drops_tabs_and_window_buttons_below_120_pt() {
    each_density(|density| {
        let (wide, narrow) = run(density, |ui| {
            let show = |ui: &mut egui::Ui, width: f32| {
                ui.allocate_ui(egui::vec2(width, 40.0), |ui| {
                    ui.set_width(width);
                    let r = c::FloatingPanelHeader {
                        tabs: &["Tools"],
                        active: 0,
                        collapsed: false,
                    }
                    .show(ui);
                    (r.narrow, r.drag.rect, r.menu.rect)
                })
                .inner
            };
            (
                show(ui, metrics::inspector()),
                show(ui, metrics::tools_two_columns()),
            )
        });
        let control = metrics::control();
        assert!(!wide.0 && narrow.0, "{density:?}");
        for (_, header, menu) in [wide, narrow] {
            assert_eq!(header.height(), metrics::panel_tab(), "{density:?}");
            assert_eq!(menu.size(), egui::Vec2::splat(control), "{density:?}");
            assert!(
                header.contains_rect(menu),
                "{density:?}: {menu:?} in {header:?}"
            );
        }
        // Full header: ⋯ is the leftmost of three control squares at the right edge.
        let actions = headers::FloatingPanelHeader::actions_width();
        assert!(
            (wide.1.right() - wide.2.left() - actions).abs() < 0.5,
            "{density:?}"
        );
        assert!(headers::FloatingPanelHeader::chrome_width() < metrics::inspector());
    });
}

#[test]
fn stage_toolbar_is_zoom_control_high_and_keeps_its_width_while_zooming() {
    each_density(|density| {
        let bars = [0.08_f32, 1.0, 64.0].map(|zoom| {
            run(density, |ui| {
                c::ZoomControl::bar_frame(ui.ctx())
                    .show(ui, |ui| {
                        c::ZoomControl {
                            zoom,
                            hand: false,
                            rulers: Some(true),
                            guides: Some(false),
                            grid: Some(false),
                            snap: Some(true),
                        }
                        .contents(ui);
                    })
                    .response
                    .rect
                    .size()
            })
        });
        assert_eq!(bars[0].y, metrics::zoom_control(), "{density:?}");
        assert_eq!(bars[0], bars[1], "{density:?}");
        assert_eq!(bars[1], bars[2], "{density:?}");
    });
}

#[test]
fn stepper_arrows_stay_inside_the_field_in_every_density() {
    each_density(|_| {
        // Two 12 px chevrons whose centres are 8 pt apart need 20 pt.
        assert!(metrics::control() >= 20.0);
    });
}

/// T-389 F: the atoms added for the panel packages — `Outlined` icon button, row-high icon cell,
/// `Success` tones, the Select label slot, the row-high text field, the brush preset row, the colour
/// mixer and the swatch grid that follow the inspector width.
#[test]
fn finishing_atoms_follow_the_density_and_the_inspector_width() {
    each_density(|density| {
        let content = metrics::inspector() - 2.0 * space::s4();
        let out = run(density, |ui| {
            // A column as wide as the inspector content (a panel Ui cannot be narrowed in place).
            let column = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(content, 800.0));
            let ui = &mut ui.new_child(egui::UiBuilder::new().max_rect(column));
            let outlined = ui.add(c::IconButton::outlined(i16::MORE, "More")).rect;
            let cell = ui
                .add(
                    c::IconButton::new(i16::MORE, "Options")
                        .cell(metrics::ICON_UI, metrics::section_header()),
                )
                .rect;
            let badge = ui.add(c::Badge::new("Working", c::BadgeType::Success)).rect;
            let message = ui.add(c::InlineMessage::success("Working.")).rect;
            let select = c::Select::new("labelled", "Default")
                .prefix("Profile")
                .show(ui, |_| {})
                .0
                .rect;
            let mut name = String::from("Layer 1");
            let inline = ui
                .add(
                    c::TextField::new(&mut name, "Layer name")
                        .compact(metrics::LAYER_ROW_COMPACT)
                        .width(120.0),
                )
                .rect;
            let preset = ui.add(c::BrushPreset::new("Round", "12")).rect;
            let mut colour = c::color_picker::hsv(0.6, 0.5, 0.5, 1.0);
            let picker = ui.add(c::ColorPicker { color: &mut colour }).rect;
            (
                outlined, cell, badge, message, select, inline, preset, picker,
            )
        });
        let (outlined, cell, badge, message, select, inline, preset, picker) = out;
        assert_eq!(outlined.size(), egui::Vec2::splat(metrics::control()));
        assert_eq!(
            cell.size(),
            egui::vec2(metrics::ICON_UI, metrics::section_header())
        );
        assert_eq!(badge.height(), metrics::badge());
        assert!(message.height() >= 16.0);
        assert_eq!(select.height(), metrics::control());
        assert_eq!(inline.height(), metrics::LAYER_ROW_COMPACT);
        assert_eq!(preset.height(), metrics::control() + space::s2());
        assert_eq!(preset.height(), c::brushes::brush_preset_height());
        // The mixer fills the inspector content width (never the old fixed 248 pt) in proportion.
        assert_eq!(picker.width(), content.min(248.0), "{density:?}");
        assert_eq!(
            picker.height(),
            (picker.width() * 150.0 / 248.0).round(),
            "{density:?}"
        );
        assert_eq!(c::ColorPicker::size_for(10_000.0), egui::vec2(248.0, 150.0));
        let (area, hue, alpha) = c::ColorPicker::layout(picker);
        assert!(area.right() < hue.left() && hue.right() < alpha.left());
        assert!((alpha.right() - picker.right()).abs() < 0.01, "{density:?}");
        // Twelve palette columns fit the inspector; a narrower column reflows instead of clipping.
        let (columns, pitch) = c::swatches::grid_columns(content);
        assert_eq!(columns, c::swatches::PALETTE_COLUMNS, "{density:?}");
        let last_right = (columns - 1) as f32 * pitch + metrics::swatch();
        assert!(last_right <= content + 0.01, "{density:?}: {last_right}");
        let (narrow, narrow_pitch) = c::swatches::grid_columns(100.0);
        assert!(narrow < c::swatches::PALETTE_COLUMNS);
        assert!((narrow - 1) as f32 * narrow_pitch + metrics::swatch() <= 100.01);
    });
    assert_eq!(
        c::Select::accessible_name("Profile", "Default"),
        "Profile, Default"
    );
    assert_eq!(c::Select::accessible_name("", "Default"), "Default");
}

/// T-389 F: the swatches popover keeps every cell and header control inside its width at the
/// inspector content width of each density.
#[test]
fn swatches_popover_fits_the_inspector_content_width() {
    each_density(|density| {
        metrics::set_density(density);
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        ctx.set_fonts(typography::font_definitions());
        theme::install(&ctx, Theme::Light);
        let width = metrics::inspector() - 2.0 * space::s4();
        let palette = c::default_palette();
        let document: Vec<egui::Color32> = palette.iter().copied().take(12).collect();
        let mut colour = palette[20];
        let mut output = None;
        for _ in 0..3 {
            output = Some(ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 900.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ctx, |ui| {
                            c::SwatchesPopover {
                                color: &mut colour,
                                recent: &palette[..5],
                                document: &document,
                                palette: &palette,
                                gradients: &[c::GradientSwatch {
                                    from: egui::Color32::BLACK,
                                    to: egui::Color32::WHITE,
                                    radial: false,
                                }],
                            }
                            .show(ui);
                        });
                },
            ));
        }
        let output = output.unwrap();
        let nodes = &output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes;
        let mut seen = 0;
        for (_, node) in nodes {
            let (Some(bounds), Some(label)) = (node.bounds(), node.label()) else {
                continue;
            };
            if bounds.x1 - bounds.x0 > f64::from(width) * 2.0 {
                continue;
            }
            seen += 1;
            assert!(
                bounds.x0 >= -0.5 && bounds.x1 <= f64::from(width) + 0.5,
                "{density:?}: {label:?} {bounds:?} leaves the {width} pt popover"
            );
        }
        // Header (field, three icons), 5 + 12 + 108 colours, the Add cell and a gradient.
        assert!(seen >= 3 + 5 + 12 + 108 + 2, "{density:?}: {seen} nodes");
        let labelled = |label: &str| nodes.iter().any(|(_, node)| node.label() == Some(label));
        for label in ["No colour", "Eyedropper", "Open colour mixer", "Add swatch"] {
            assert!(labelled(label), "{density:?}: {label} missing");
        }
    });
}
