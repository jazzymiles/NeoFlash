//! NF2 component gallery — the code twin of the `Shelf / …` frames in Figma.
//!
//! ```bash
//! cargo run -p neoflash-ui --example gallery              # interactive, Dark
//! cargo run -p neoflash-ui --example gallery -- light     # Light
//! cargo run -p neoflash-ui --example gallery -- dark  target/ui-shots/gallery-dark.png   # save a PNG and quit
//! ```
//! Compare the PNG with `interface/design-system/docs/illustrations/*.png`.
use eframe::egui;
use neoflash_ui::components::{self as c, timeline as tl};
use neoflash_ui::icons::{i16, i20};
use neoflash_ui::metrics::FrameView;
use neoflash_ui::tokens::{TextStyle, space};
use neoflash_ui::{Theme, palette, typography};

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    let theme = if args.get(1).is_some_and(|a| a == "light") {
        Theme::Light
    } else {
        Theme::Dark
    };
    let shot = args.get(2).cloned();
    eframe::run_native(
        "NF2 gallery",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 1340.0]),
            ..Default::default()
        },
        Box::new(move |cc| {
            cc.egui_ctx.set_fonts(typography::font_definitions());
            neoflash_ui::theme::install(&cc.egui_ctx, theme);
            Ok(Box::new(Gallery {
                shot,
                frames: 0,
                ..Gallery::default()
            }))
        }),
    )
}

#[derive(Default)]
struct Gallery {
    shot: Option<String>,
    frames: u32,
    check: bool,
    switch: bool,
    slider: f64,
    width: f64,
    size: f64,
    stroke: f64,
    name: String,
    search: String,
    segment: u8,
    radio: u8,
    color: egui::Color32,
    line: c::LineStyle,
    reg: c::RegistrationPoint,
    dock_tab: usize,
}

impl eframe::App for Gallery {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.frames == 0 {
            self.check = true;
            self.slider = 50.0;
            self.width = 960.0;
            self.size = 2.0;
            self.stroke = 2.0;
            self.name = "Ideas in motion".into();
            self.search = String::new();
            self.color = egui::Color32::from_rgb(0x23, 0x30, 0x43);
        }
        self.frames += 1;
        let p = palette(ctx);
        egui::CentralPanel::default()
            .frame(c::Surface::App.frame(ctx).inner_margin(24))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(space::S4, space::S4);
                    let title = |ui: &mut egui::Ui, t: &str| {
                        ui.label(typography::rich(t, TextStyle::Title).color(p.text_primary));
                    };
                    ui.columns(2, |cols| {
                        let ui = &mut cols[0];
                        title(ui, "Button");
                        ui.horizontal(|ui| {
                            ui.add(c::Button::primary("Publish").icon(i16::UPLOAD));
                            ui.add(c::Button::new("Test Movie").icon(i16::PLAY).shortcut("⌘↵"));
                            ui.add(c::Button::ghost("Ghost"));
                            ui.add(c::Button::danger("Discard"));
                            ui.add_enabled(false, c::Button::new("Disabled"));
                        });
                        title(ui, "Icon button · Tool button");
                        ui.horizontal(|ui| {
                            ui.add(c::IconButton::new(i16::SELECT, "Selection").shortcut("V"));
                            ui.add(c::IconButton::new(i16::LOOP, "Loop").selected(true));
                            ui.add(
                                c::IconButton::new(i16::PLAY, "Play")
                                    .kind(c::IconButtonType::Primary)
                                    .size(c::IconButtonSize::MD)
                                    .shortcut("Enter"),
                            );
                            ui.add(
                                c::ToolButton::new(i20::TOOL_SELECT, "Selection")
                                    .key("V")
                                    .selected(true),
                            );
                            ui.add(
                                c::ToolButton::new(i20::TOOL_RECT, "Rectangle")
                                    .key("R")
                                    .flyout(true),
                            );
                            ui.add(c::ToolButton::new(i20::TOOL_PEN, "Pen").key("P"));
                        });
                        title(ui, "Segmented control · Radio");
                        ui.horizontal(|ui| {
                            c::Segmented::show(
                                ui,
                                &mut self.segment,
                                &[(0, "Selection"), (1, "Tool")],
                                false,
                            );
                            ui.add(c::Radio::new(self.radio == 0, "Option"))
                                .clicked()
                                .then(|| self.radio = 0);
                            ui.add(c::Radio::new(self.radio == 1, "Option"))
                                .clicked()
                                .then(|| self.radio = 1);
                        });
                        title(ui, "Field · Search · Select");
                        ui.horizontal(|ui| {
                            ui.add(
                                c::Field::new(&mut self.width, "Width")
                                    .prefix("W")
                                    .width(140.0),
                            );
                            ui.add(
                                c::Field::new(&mut self.size, "Stroke size")
                                    .prefix("Size")
                                    .decimals(1)
                                    .step(0.1)
                                    .stepper()
                                    .width(120.0),
                            );
                            ui.add(
                                c::Field::new(&mut self.stroke, "Stroke")
                                    .presets(&[0.5, 1.0, 2.0, 4.0, 8.0])
                                    .width(96.0),
                            );
                        });
                        ui.add(c::TextField::new(&mut self.name, "Name").prefix("Name"));
                        ui.add(c::Search::new(&mut self.search, "Search library"));
                        ui.horizontal(|ui| {
                            c::Select::options(
                                ui,
                                "scene",
                                &mut self.segment,
                                &[(0, "Scene 1"), (1, "Scene 2")],
                                160.0,
                            );
                            c::LineStyleSelect::show(ui, "line", &mut self.line, 200.0);
                        });
                        c::ColorField::new(&mut self.color, "Background")
                            .width(200.0)
                            .show(ui);
                        title(ui, "Checkbox · Switch · Slider · Badge");
                        ui.horizontal(|ui| {
                            ui.add(c::Checkbox::new(&mut self.check, "Checkbox"));
                            ui.add(c::Switch::new(&mut self.switch, "Switch"));
                            ui.add(c::Slider::new(&mut self.slider, 0.0..=100.0, "Opacity"));
                        });
                        ui.horizontal(|ui| {
                            ui.add(c::Badge::new("Badge", c::BadgeType::Neutral));
                            ui.add(c::Badge::new(
                                "HTML5 · Rust / WebAssembly",
                                c::BadgeType::Accent,
                            ));
                            ui.add(c::Badge::new("Unsaved", c::BadgeType::Warning));
                            c::kbd(ui, "F6", false);
                            c::logo(ui, 28.0);
                            c::spinner(ui);
                        });
                        c::progress(ui, "Exporting frames 72 / 120", Some(0.6), 280.0);
                        c::Toast {
                            kind: c::ToastType::Error,
                            message: "Test Movie failed — 1 compiler error",
                            action: Some("Show"),
                        }
                        .show(ui, 420.0);

                        let ui = &mut cols[1];
                        title(ui, "Dock tab · Panel header · Section header");
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            for (i, (l, icon)) in [
                                ("Timeline", i16::FILM),
                                ("Actions", i16::CODE),
                                ("Output", i16::TERMINAL),
                            ]
                            .into_iter()
                            .enumerate()
                            {
                                if ui
                                    .add(c::DockTab {
                                        label: l,
                                        icon: Some(icon),
                                        active: self.dock_tab == i,
                                    })
                                    .clicked()
                                {
                                    self.dock_tab = i;
                                }
                            }
                        });
                        c::Surface::PanelFlush.frame(ui.ctx()).show(ui, |ui| {
                            ui.set_width(320.0);
                            c::PanelHeader {
                                title: "Properties",
                            }
                            .show(ui, |_| {});
                            egui::Frame::new().inner_margin(12).show(ui, |ui| {
                                c::SectionHeader::new("Stage").show(ui, |ui| {
                                    ui.add(
                                        c::TextField::new(&mut self.name, "Name").prefix("Name"),
                                    );
                                    ui.horizontal(|ui| {
                                        ui.add(
                                            c::Field::new(&mut self.width, "Width")
                                                .prefix("W")
                                                .width(130.0),
                                        );
                                        ui.add(
                                            c::Field::new(&mut self.width, "Height")
                                                .prefix("H")
                                                .width(130.0),
                                        );
                                    });
                                });
                                c::SectionHeader::new("Playback")
                                    .default_open(false)
                                    .show(ui, |_| {});
                            });
                        });
                        title(ui, "Tree item · Breadcrumb · Menu");
                        c::Surface::PanelFlush.frame(ui.ctx()).show(ui, |ui| {
                            ui.set_width(320.0);
                            c::TreeItem::new("Characters", c::TreeItemType::FolderOpen).show(ui);
                            c::TreeItem::new("orbit", c::TreeItemType::Symbol(i16::MOVIECLIP))
                                .depth(1)
                                .selected(true)
                                .meta("Orbit")
                                .show(ui);
                            c::TreeItem::new("planet", c::TreeItemType::Symbol(i16::GRAPHIC))
                                .depth(1)
                                .show(ui);
                        });
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            ui.add(c::BreadcrumbItem {
                                label: "Scene 1",
                                kind: c::BreadcrumbType::Scene,
                                current: false,
                                menu: false,
                            });
                            c::breadcrumb_separator(ui);
                            ui.add(c::BreadcrumbItem {
                                label: "orbit",
                                kind: c::BreadcrumbType::MovieClip,
                                current: false,
                                menu: false,
                            });
                            c::breadcrumb_separator(ui);
                            ui.add(c::BreadcrumbItem {
                                label: "planet",
                                kind: c::BreadcrumbType::Graphic,
                                current: true,
                                menu: true,
                            });
                        });
                        c::menu::popover_frame(ui.ctx()).show(ui, |ui| {
                            ui.set_width(240.0);
                            c::menu::menu_style(ui.style_mut());
                            c::menu_section(ui, "Edit");
                            ui.add(c::MenuItem::new("Undo").shortcut("⌘Z"));
                            ui.add(c::MenuItem::new("Snap to grid").checked(true));
                            c::menu_separator(ui);
                            ui.add(c::MenuItem::new("Timeline").submenu(true));
                            ui.add_enabled(
                                false,
                                c::MenuItem::new("Paste in place").shortcut("⇧⌘V"),
                            );
                        });
                        title(ui, "Colour · registration · icon group");
                        ui.horizontal(|ui| {
                            c::FillStrokeChip {
                                fill: Some(egui::Color32::from_rgb(0x2c, 0x5c, 0xc5)),
                                stroke: Some(egui::Color32::BLACK),
                            }
                            .show(ui);
                            ui.add(c::ColorWell {
                                target: c::ColorWellTarget::Stroke,
                                color: Some(egui::Color32::BLACK),
                                active: true,
                            });
                            ui.add(c::ColorWell {
                                target: c::ColorWellTarget::Fill,
                                color: None,
                                active: false,
                            });
                            ui.add(c::RegistrationPicker {
                                point: &mut self.reg,
                            });
                        });
                        c::IconGroup {
                            items: &[
                                (i16::ALIGN_LEFT, "Align left"),
                                (i16::ALIGN_HCENTER, "Align centre"),
                                (i16::ALIGN_RIGHT, "Align right"),
                            ],
                            selected: Some(1),
                        }
                        .show(ui);
                    });

                    title(ui, "Timeline 2.0");
                    let geo_view = FrameView::Normal;
                    c::Surface::PanelFlush.frame(ui.ctx()).show(ui, |ui| {
                        let top = ui.cursor().min;
                        let width = ui.available_width();
                        let lc = 280.0;
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                                tl::layers_header(ui, lc);
                                let rows = [
                                    ("labels", tl::LayerKind::Layer, 0, false),
                                    ("character", tl::LayerKind::FolderOpen, 0, false),
                                    ("Guide: path", tl::LayerKind::MotionGuide, 1, false),
                                    ("ball", tl::LayerKind::Layer, 2, false),
                                    ("hand", tl::LayerKind::Masked, 1, true),
                                    ("music.mp3", tl::LayerKind::Sound, 0, false),
                                ];
                                for (l, k, ind, sel) in rows {
                                    tl::LayerRow {
                                        label: l,
                                        kind: k,
                                        selected: sel,
                                        indent: ind,
                                        editing: sel,
                                        visible: l != "labels",
                                        locked: l == "character",
                                        outline: false,
                                        color: p.layer_blue,
                                    }
                                    .show(ui, lc, 28.0);
                                }
                            });
                            let (area, _) = ui.allocate_exact_size(
                                egui::vec2(width - lc, 32.0 + 6.0 * 28.0),
                                egui::Sense::hover(),
                            );
                            let painter = ui.painter_at(area);
                            let geo = tl::Geometry::new(area.left(), geo_view, false);
                            let ruler =
                                egui::Rect::from_min_size(area.min, egui::vec2(area.width(), 32.0));
                            tl::paint_ruler(&painter, ui.ctx(), ruler, &geo, 24.0, true);
                            for i in 0..6 {
                                let row = egui::Rect::from_min_size(
                                    egui::pos2(area.left(), ruler.bottom() + i as f32 * 28.0),
                                    egui::vec2(area.width(), 28.0),
                                );
                                tl::paint_grid(
                                    &painter,
                                    ui.ctx(),
                                    row,
                                    &geo,
                                    if i == 1 {
                                        tl::GridKind::Folder
                                    } else {
                                        tl::GridKind::Layer
                                    },
                                );
                                match i {
                                    0 => {
                                        tl::paint_marker(
                                            &painter,
                                            ui.ctx(),
                                            geo.cell(row, 1),
                                            tl::MarkerKind::ActionKeyframe,
                                            false,
                                        );
                                        tl::paint_frame_label(
                                            &painter,
                                            ui.ctx(),
                                            geo.cell(row, 1),
                                            tl::FrameLabelKind::Label,
                                            "intro",
                                            geo.frame_x(30),
                                        );
                                        tl::paint_marker(
                                            &painter,
                                            ui.ctx(),
                                            geo.cell(row, 40),
                                            tl::MarkerKind::Keyframe,
                                            false,
                                        );
                                        tl::paint_frame_label(
                                            &painter,
                                            ui.ctx(),
                                            geo.cell(row, 40),
                                            tl::FrameLabelKind::Comment,
                                            "loop",
                                            geo.frame_x(80),
                                        );
                                    }
                                    2 => {
                                        tl::paint_span(
                                            &painter,
                                            ui.ctx(),
                                            geo.span(row, 1, 60),
                                            tl::SpanType::Static,
                                            true,
                                        );
                                        tl::paint_marker(
                                            &painter,
                                            ui.ctx(),
                                            geo.cell(row, 1),
                                            tl::MarkerKind::Keyframe,
                                            false,
                                        );
                                        tl::paint_marker(
                                            &painter,
                                            ui.ctx(),
                                            geo.cell(row, 60),
                                            tl::MarkerKind::SpanEnd,
                                            false,
                                        );
                                    }
                                    3 => {
                                        tl::paint_span(
                                            &painter,
                                            ui.ctx(),
                                            geo.span(row, 1, 24),
                                            tl::SpanType::ClassicTween,
                                            true,
                                        );
                                        tl::paint_span(
                                            &painter,
                                            ui.ctx(),
                                            geo.span(row, 25, 48),
                                            tl::SpanType::BrokenTween,
                                            true,
                                        );
                                        tl::paint_grid_lines(
                                            &painter,
                                            ui.ctx(),
                                            row,
                                            &geo,
                                            p.timeline_grid,
                                        );
                                        tl::paint_marker(
                                            &painter,
                                            ui.ctx(),
                                            geo.cell(row, 1),
                                            tl::MarkerKind::Keyframe,
                                            false,
                                        );
                                        tl::paint_marker(
                                            &painter,
                                            ui.ctx(),
                                            geo.cell(row, 25),
                                            tl::MarkerKind::Keyframe,
                                            false,
                                        );
                                    }
                                    4 => {
                                        tl::paint_span(
                                            &painter,
                                            ui.ctx(),
                                            geo.span(row, 1, 50),
                                            tl::SpanType::MotionTween,
                                            true,
                                        );
                                        tl::paint_span(
                                            &painter,
                                            ui.ctx(),
                                            geo.span(row, 28, 34),
                                            tl::SpanType::Selected,
                                            true,
                                        );
                                        tl::paint_grid_lines(
                                            &painter,
                                            ui.ctx(),
                                            row,
                                            &geo,
                                            p.timeline_grid,
                                        );
                                        tl::paint_marker(
                                            &painter,
                                            ui.ctx(),
                                            geo.cell(row, 1),
                                            tl::MarkerKind::Keyframe,
                                            false,
                                        );
                                        tl::paint_marker(
                                            &painter,
                                            ui.ctx(),
                                            geo.cell(row, 30),
                                            tl::MarkerKind::PropertyKeyframe,
                                            true,
                                        );
                                        tl::paint_span(
                                            &painter,
                                            ui.ctx(),
                                            geo.span(row, 60, 80),
                                            tl::SpanType::ShapeTween,
                                            true,
                                        );
                                    }
                                    5 => tl::paint_span(
                                        &painter,
                                        ui.ctx(),
                                        geo.span(row, 1, 100),
                                        tl::SpanType::Sound,
                                        true,
                                    ),
                                    _ => {}
                                }
                            }
                            tl::paint_onion_range(
                                &painter,
                                ui.ctx(),
                                ruler,
                                geo.frame_x(26),
                                geo.frame_x(35),
                            );
                            tl::paint_playhead(
                                &painter,
                                ui.ctx(),
                                area,
                                ruler,
                                &geo,
                                geo.frame_x(30),
                                30,
                                Some("1.21s"),
                            );
                            let _ = top;
                        });
                    });
                    c::surface::status_bar(
                        ui,
                        true,
                        "Ready",
                        "Stage 960 × 600 · 24 fps · 120 frames",
                        &[
                            ("F5", "frame"),
                            ("F6", "keyframe"),
                            ("F7", "blank keyframe"),
                            ("⌘↵", "Test Movie"),
                        ],
                    );

                    // ── Planned-panel components: Ease preset / graph / preview track, motion lanes, swatches, view settings.
                    // `NF2_GALLERY_SECTION=planned` scrolls here so the PNG capture shows this section.
                    title(ui, "Ease · Motion lanes · Swatches · View settings");
                    ui.horizontal_top(|ui| {
                        ui.vertical(|ui| {
                            ui.set_width(360.0);
                            ui.spacing_mut().item_spacing = egui::vec2(space::S2, space::S2);
                            for row in c::EasePresetKind::ALL.chunks(4) {
                                ui.horizontal(|ui| {
                                    for preset in row {
                                        ui.add(c::EasePreset {
                                            preset: *preset,
                                            selected: *preset == c::EasePresetKind::Custom,
                                        });
                                    }
                                });
                            }
                            ui.add(c::EaseGraph {
                                playhead: Some(30),
                                ..Default::default()
                            });
                            ui.add(c::EaseGraph {
                                range: c::EaseRange::Overshoot,
                                bezier: [0.3, 0.0, 0.55, 1.3],
                                frames: 40,
                                playhead: None,
                                width: 336.0,
                            });
                            c::ease_preview_track(ui, [0.6, 0.02, 0.4, 0.98], 0.75, 336.0);
                        });
                        ui.vertical(|ui| {
                            ui.set_width(420.0);
                            let (rect, _) = ui.allocate_exact_size(
                                egui::vec2(420.0, 28.0 + 3.0 * 48.0),
                                egui::Sense::hover(),
                            );
                            let scale =
                                egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), 28.0));
                            c::paint_frame_scale(ui.painter(), ui.ctx(), scale, 60, 5);
                            let lanes: [&[(f32, f32)]; 3] = [
                                &[(0.0, 0.05), (17.0, 0.8), (36.0, 0.5), (59.0, 0.95)],
                                &[(0.0, 0.65), (30.0, 0.4), (59.0, 0.65)],
                                &[(0.0, 0.1), (11.0, 0.85), (59.0, 0.85)],
                            ];
                            for (i, keys) in lanes.iter().enumerate() {
                                let lane = egui::Rect::from_min_size(
                                    egui::pos2(rect.left(), scale.bottom() + i as f32 * 48.0),
                                    egui::vec2(rect.width(), 48.0),
                                );
                                let keys: Vec<c::LaneKey> = keys
                                    .iter()
                                    .map(|(frame, value)| c::LaneKey {
                                        frame: *frame,
                                        value: *value,
                                    })
                                    .collect();
                                c::paint_curve_lane(
                                    ui.painter(),
                                    ui.ctx(),
                                    lane,
                                    60,
                                    &keys,
                                    i == 0,
                                );
                            }
                            c::paint_lane_playhead(
                                ui.painter(),
                                ui.ctx(),
                                rect.left() + 12.0 + 29.0 / 59.0 * (rect.width() - 24.0),
                                scale,
                                rect.bottom(),
                                30,
                            );
                            c::Surface::Popover.frame(ui.ctx()).show(ui, |ui| {
                                ui.set_width(300.0);
                                let palette = c::default_palette();
                                let pick = |i: usize| palette[i % palette.len()];
                                let recent: Vec<_> = [65, 26, 67, 25, 0, 11, 24, 64, 62, 33, 1, 7]
                                    .into_iter()
                                    .map(pick)
                                    .collect();
                                let document: Vec<_> =
                                    [1, 67, 25, 10, 7, 79].into_iter().map(pick).collect();
                                let gradients = [
                                    c::GradientSwatch {
                                        from: pick(0),
                                        to: pick(11),
                                        radial: false,
                                    },
                                    c::GradientSwatch {
                                        from: pick(48),
                                        to: pick(84),
                                        radial: true,
                                    },
                                    c::GradientSwatch {
                                        from: pick(50),
                                        to: pick(48),
                                        radial: false,
                                    },
                                ];
                                let mut color = recent[0];
                                c::SwatchesPopover {
                                    color: &mut color,
                                    recent: &recent,
                                    document: &document,
                                    palette: &palette,
                                    gradients: &gradients,
                                }
                                .show(ui);
                            });
                        });
                        c::Surface::Popover.frame(ui.ctx()).show(ui, |ui| {
                            ui.set_width(320.0);
                            let mut state = c::ViewSettingsState::defaults(p);
                            c::ViewSettings { state: &mut state }.show(ui);
                        });
                    });
                    // ── Planned-panel components (2): Color picker, Gradient bar / stop, Anchor point,
                    // Edit envelope lane. Sample colours are document data (HSV), not tokens.
                    title(
                        ui,
                        "Color picker · Gradient bar · Anchor point · Envelope lane",
                    );
                    ui.horizontal_top(|ui| {
                        let id = egui::Id::new("gallery-planned-2");
                        type Planned2 = (
                            c::color_picker::Hsb,
                            Vec<(f32, egui::Color32)>,
                            usize,
                            Vec<(f32, f32)>,
                        );
                        let (mut color, mut stops, mut stop, mut envelope) =
                            ui.data(|d| d.get_temp::<Planned2>(id)).unwrap_or_else(|| {
                                let h = 36.0 / 360.0;
                                let hsv = c::color_picker::hsv;
                                (
                                    hsv(h, 1.0, 1.0, 1.0),
                                    vec![
                                        (0.0, hsv(h, 0.0, 1.0, 1.0).into()),
                                        (0.5, hsv(h, 0.6, 1.0, 1.0).into()),
                                        (1.0, hsv(h, 1.0, 1.0, 1.0).into()),
                                    ],
                                    1,
                                    vec![(0.0, 0.0), (0.19, 1.0), (0.75, 1.0), (1.0, 0.4)],
                                )
                            });
                        ui.vertical(|ui| {
                            ui.set_width(260.0);
                            ui.spacing_mut().item_spacing = egui::vec2(space::S2, space::S3);
                            ui.add(c::ColorPicker { color: &mut color });
                            ui.add(c::GradientBar {
                                stops: &mut stops,
                                selected: &mut stop,
                                width: 232.0,
                            });
                            ui.horizontal(|ui| {
                                for selected in [false, true] {
                                    ui.add(c::GradientStop {
                                        color: stops[stops.len() - 1].1,
                                        selected,
                                    });
                                }
                                for selected in [false, true] {
                                    ui.add(c::AnchorPoint { selected });
                                }
                            });
                        });
                        ui.vertical(|ui| {
                            ui.set_width(632.0);
                            ui.add(c::EnvelopeLane {
                                channel: "L",
                                points: &mut envelope,
                                selected: Some(1),
                                height: 96.0,
                            });
                        });
                        ui.data_mut(|d| d.insert_temp(id, (color, stops, stop, envelope)));
                    });
                    title(ui, "Rulers · guides · grid · canvas editing chrome");
                    {
                        use c::canvas as cv;
                        let (area, _) =
                            ui.allocate_exact_size(egui::vec2(1180.0, 380.0), egui::Sense::hover());
                        let painter = ui.painter_at(area);
                        painter.rect_filled(area, 0.0, p.bg_canvas);
                        let zoom = 0.75;
                        let corner =
                            egui::Rect::from_min_size(area.min, egui::vec2(cv::RULER, cv::RULER));
                        let page = egui::Rect::from_min_size(
                            area.min + egui::vec2(cv::RULER + 40.0, cv::RULER + 30.0),
                            egui::vec2(480.0 * zoom, 300.0 * zoom),
                        );
                        let h = egui::Rect::from_min_max(
                            egui::pos2(corner.right(), area.top()),
                            egui::pos2(area.right(), area.top() + cv::RULER),
                        );
                        let v = egui::Rect::from_min_max(
                            egui::pos2(area.left(), corner.bottom()),
                            egui::pos2(area.left() + cv::RULER, area.bottom()),
                        );
                        painter.rect_filled(page, 0.0, p.bg_stage);
                        cv::paint_stage_grid(
                            &painter,
                            ui.ctx(),
                            page,
                            zoom,
                            egui::vec2(20.0, 20.0),
                            None,
                        );
                        cv::paint_stage_ruler(
                            &painter,
                            ui.ctx(),
                            h,
                            cv::Orientation::Horizontal,
                            page.left(),
                            zoom,
                            None,
                        );
                        cv::paint_stage_ruler(
                            &painter,
                            ui.ctx(),
                            v,
                            cv::Orientation::Vertical,
                            page.top(),
                            zoom,
                            None,
                        );
                        cv::paint_ruler_corner(&painter, ui.ctx(), corner);
                        let canvas = egui::Rect::from_min_max(corner.right_bottom(), area.max);
                        cv::paint_guide(
                            &painter,
                            ui.ctx(),
                            cv::Orientation::Horizontal,
                            page.top() + 60.0,
                            canvas,
                            cv::GuideState::Default,
                            None,
                        );
                        cv::paint_guide(
                            &painter,
                            ui.ctx(),
                            cv::Orientation::Vertical,
                            page.left() + 120.0,
                            canvas,
                            cv::GuideState::Selected,
                            None,
                        );
                        cv::paint_guide(
                            &painter,
                            ui.ctx(),
                            cv::Orientation::Vertical,
                            page.left() + 200.0,
                            canvas,
                            cv::GuideState::Locked,
                            None,
                        );
                        let x0 = page.right() + 40.0;
                        let boxes = [
                            cv::SelectionKind::Group,
                            cv::SelectionKind::Symbol,
                            cv::SelectionKind::TextStatic,
                            cv::SelectionKind::TextDynamic,
                            cv::SelectionKind::Primitive,
                        ];
                        for (i, kind) in boxes.into_iter().enumerate() {
                            let r = egui::Rect::from_min_size(
                                egui::pos2(x0 + i as f32 * 120.0, area.top() + 40.0),
                                egui::vec2(100.0, 64.0),
                            );
                            cv::paint_selection_box(&painter, ui.ctx(), r, kind);
                        }
                        let t1 = egui::Rect::from_min_size(
                            egui::pos2(x0, area.top() + 140.0),
                            egui::vec2(110.0, 70.0),
                        );
                        cv::paint_transform_box(&painter, ui.ctx(), t1, cv::TransformMode::Scale);
                        let t2 = t1.translate(egui::vec2(150.0, 0.0));
                        cv::paint_transform_box(
                            &painter,
                            ui.ctx(),
                            t2,
                            cv::TransformMode::RotateSkew,
                        );
                        let m = t2.translate(egui::vec2(150.0, 0.0));
                        cv::paint_marquee(&painter, ui.ctx(), m);
                        let sh = m.translate(egui::vec2(140.0, 0.0));
                        cv::paint_shape_selection(&painter, ui.ctx(), sh);
                        let y = area.top() + 270.0;
                        for (i, st) in [
                            cv::AnchorState::Unselected,
                            cv::AnchorState::Selected,
                            cv::AnchorState::Hover,
                        ]
                        .into_iter()
                        .enumerate()
                        {
                            cv::paint_anchor(
                                &painter,
                                ui.ctx(),
                                egui::pos2(x0 + i as f32 * 30.0, y),
                                st,
                            );
                        }
                        cv::paint_bezier_handle(
                            &painter,
                            ui.ctx(),
                            egui::pos2(x0 + 100.0, y),
                            egui::pos2(x0 + 150.0, y - 20.0),
                        );
                        cv::paint_gradient_transform(
                            &painter,
                            ui.ctx(),
                            egui::pos2(x0 + 260.0, y + 20.0),
                            50.0,
                            0.5,
                        );
                        cv::paint_snap_ring(&painter, ui.ctx(), egui::pos2(x0 + 360.0, y), false);
                        cv::paint_snap_ring(&painter, ui.ctx(), egui::pos2(x0 + 400.0, y), true);
                    }
                    if std::env::var("NF2_GALLERY_SECTION").is_ok_and(|v| v == "planned") {
                        ui.scroll_to_cursor_animation(
                            Some(egui::Align::BOTTOM),
                            egui::style::ScrollAnimation::none(),
                        );
                    }
                });
            });

        if let Some(path) = &self.shot {
            if self.frames == 4 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
            let image = ctx.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
            });
            if let Some(image) = image {
                let [w, h] = image.size;
                let bytes: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
                if let Some(dir) = std::path::Path::new(path).parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                image::save_buffer(path, &bytes, w as u32, h as u32, image::ColorType::Rgba8)
                    .expect("save screenshot");
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ctx.request_repaint();
        }
    }
}
