#![allow(unused_imports)]
use super::*;
use crate::theme::{ScrollTheme as _, ThemeMode};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use neoflash_ui::components as c;
use neoflash_ui::components::timeline::{
    self as tl, GridKind, LayerColumns, LayerKind, LayerRow, LayersHeaderColumns, ModeCell,
};
use neoflash_ui::icons::{self, Icon, i16};
use neoflash_ui::tokens::{Palette, TextStyle, radius, space};
use neoflash_ui::typography::rich;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

// ─────────────────────────────────────────────────────────────────────────────
// 6. Preloaded Real NeoFlash Panel Presets + JSON & Rust Codegen Export
// ─────────────────────────────────────────────────────────────────────────────

/// Built-in templates modeling real NeoFlash surfaces, workspaces, timelines, and panels for immediate visual editing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StudioPanelPreset {
    #[default]
    PropertiesMovieClip,
    FullWorkspaceShell,
    TimelineStudio,
    LibraryAndAssetsStudio,
    ColorAndGradientStudio,
    AlignAndTransform,
    PropertiesText,
    ColorPanel,
    ToolsPanel,
    FiltersPanel,
    TimelineHeader,
}

impl StudioPanelPreset {
    pub const ALL: [Self; 11] = [
        Self::PropertiesMovieClip,
        Self::LibraryAndAssetsStudio,
        Self::ColorAndGradientStudio,
        Self::AlignAndTransform,
        Self::ToolsPanel,
        Self::TimelineStudio,
        Self::FullWorkspaceShell,
        Self::PropertiesText,
        Self::ColorPanel,
        Self::FiltersPanel,
        Self::TimelineHeader,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::FullWorkspaceShell => "Full Editor Workspace — Live Shell",
            Self::TimelineStudio => "Timeline & Tween Geometry Studio",
            Self::LibraryAndAssetsStudio => "Library Split: Preview + Tree",
            Self::ColorAndGradientStudio => "Color Mixer, Gradient Bar & Swatches",
            Self::AlignAndTransform => "Align & Transform",
            Self::PropertiesMovieClip => "Properties — Movie Clip",
            Self::PropertiesText => "Properties — Text",
            Self::ColorPanel => "Color",
            Self::ToolsPanel => "Tools",
            Self::FiltersPanel => "Filters",
            Self::TimelineHeader => "Timeline Header",
        }
    }

    pub const fn is_full_shell(self) -> bool {
        matches!(self, Self::FullWorkspaceShell)
    }

    pub const fn is_timeline_surface(self) -> bool {
        matches!(self, Self::TimelineStudio)
    }

    pub fn build_document(self) -> UiStudioDocument {
        let mut next_id = 1_u64;
        let mut alloc = || {
            let id = next_id;
            next_id += 1;
            id
        };

        let root = match self {
            Self::FullWorkspaceShell => {
                let root_id = alloc(); // 1
                let top_bars_id = alloc(); // 2
                let left_tools_id = alloc(); // 3
                let center_stage_id = alloc(); // 4
                let right_dock_id = alloc(); // 5
                let bottom_timeline_id = alloc(); // 6
                let tl_layers_col_id = alloc(); // 7
                let tl_ruler_playhead_id = alloc(); // 8
                let tl_classic_tween_id = alloc(); // 9
                let tl_shape_tween_id = alloc(); // 10
                let tl_motion_tween_id = alloc(); // 11
                let tl_broken_tween_id = alloc(); // 12
                let tl_keyframes_badges_id = alloc(); // 13
                let tl_audio_waveform_id = alloc(); // 14

                UiStudioNode::new(
                    root_id,
                    "Full Editor Workspace — Live Shell",
                    UiNodeKind::Panel {
                        tabs: vec![
                            "Animator Workspace".to_owned(),
                            "Classic Flash".to_owned(),
                            "Developer".to_owned(),
                        ],
                        active_tab: 0,
                    },
                )
                .with_width(UiSizeUnit::Percent(100.0))
                .with_system_binding(SharedSystemKind::PanelHeader)
                .with_children(vec![
                    UiStudioNode::new(
                        top_bars_id,
                        "Top Menu Bar + Document Tabs + Edit Breadcrumb Bar",
                        UiNodeKind::Row,
                    )
                    .with_width(UiSizeUnit::Percent(100.0))
                    .with_height(UiSizeUnit::Px(78.0)),
                    UiStudioNode::new(
                        left_tools_id,
                        "Left Tools Dock Rail (1-Col / 2-Col)",
                        UiNodeKind::Column,
                    )
                    .with_width(UiSizeUnit::Px(46.0))
                    .with_system_binding(SharedSystemKind::ToolRail),
                    UiStudioNode::new(
                        center_stage_id,
                        "Center Stage Viewport (Rulers, Cyan Guides & Vector Instance)",
                        UiNodeKind::Section {
                            collapsible: false,
                            expanded: true,
                        },
                    )
                    .with_width(UiSizeUnit::Percent(70.0)),
                    UiStudioNode::new(
                        right_dock_id,
                        "Right Multi-Panel Inspector Dock (Properties + Library + Color)",
                        UiNodeKind::Column,
                    )
                    .with_width(UiSizeUnit::Percent(25.0))
                    .with_system_binding(SharedSystemKind::TwoColumnForm),
                    UiStudioNode::new(
                        bottom_timeline_id,
                        "Bottom Timeline Dock (Layers + Frame Grid + Tweens)",
                        UiNodeKind::Section {
                            collapsible: false,
                            expanded: true,
                        },
                    )
                    .with_width(UiSizeUnit::Percent(100.0))
                    .with_height(UiSizeUnit::Percent(34.0))
                    .with_children(vec![
                        UiStudioNode::new(
                            tl_layers_col_id,
                            "Layer Tree Column (Folder / Mask / Masked / Guide / Audio)",
                            UiNodeKind::Column,
                        )
                        .with_width(UiSizeUnit::Px(220.0)),
                        UiStudioNode::new(
                            tl_ruler_playhead_id,
                            "Frame Ruler, Red Playhead & Onion Skin Brackets",
                            UiNodeKind::Row,
                        )
                        .with_width(UiSizeUnit::Percent(100.0))
                        .with_height(UiSizeUnit::Px(22.0)),
                        UiStudioNode::new(
                            tl_classic_tween_id,
                            "Classic Tween Span (Purple + Arrow ──────►)",
                            UiNodeKind::Row,
                        )
                        .with_width(UiSizeUnit::Percent(100.0))
                        .with_height(UiSizeUnit::Px(24.0)),
                        UiStudioNode::new(
                            tl_shape_tween_id,
                            "Shape Tween Span (Green + Arrow ──────►)",
                            UiNodeKind::Row,
                        )
                        .with_width(UiSizeUnit::Percent(100.0))
                        .with_height(UiSizeUnit::Px(24.0)),
                        UiStudioNode::new(
                            tl_motion_tween_id,
                            "Motion Tween Span (Blue + Property Diamonds ◆)",
                            UiNodeKind::Row,
                        )
                        .with_width(UiSizeUnit::Percent(100.0))
                        .with_height(UiSizeUnit::Px(24.0)),
                        UiStudioNode::new(
                            tl_broken_tween_id,
                            "Broken Tween Span (Dashed Line - - - - -)",
                            UiNodeKind::Row,
                        )
                        .with_width(UiSizeUnit::Percent(100.0))
                        .with_height(UiSizeUnit::Px(24.0)),
                        UiStudioNode::new(
                            tl_keyframes_badges_id,
                            "Keyframe Dots (●/○), Span End ([ ]), Label Flag & Action (a)",
                            UiNodeKind::Row,
                        )
                        .with_width(UiSizeUnit::Percent(100.0))
                        .with_height(UiSizeUnit::Px(24.0)),
                        UiStudioNode::new(
                            tl_audio_waveform_id,
                            "Soundtrack Audio Waveform Span",
                            UiNodeKind::Row,
                        )
                        .with_width(UiSizeUnit::Percent(100.0))
                        .with_height(UiSizeUnit::Px(24.0)),
                    ]),
                ])
            }
            Self::TimelineStudio => {
                let root_id = alloc(); // 1
                let bar_row = alloc(); // 2
                let layers_col_id = alloc(); // 3
                let ruler_playhead_id = alloc(); // 4
                let classic_tween_id = alloc(); // 5
                let shape_tween_id = alloc(); // 6
                let motion_tween_id = alloc(); // 7
                let broken_tween_id = alloc(); // 8
                let keyframes_badges_id = alloc(); // 9
                let audio_waveform_id = alloc(); // 10

                let new_layer_btn = alloc();
                let folder_btn = alloc();
                let trash_btn = alloc();
                let play_btn = alloc();
                let loop_btn = alloc();
                let onion_sw = alloc();
                let frame_fld = alloc();
                let fps_fld = alloc();

                UiStudioNode::new(
                    root_id,
                    "Timeline & Tween Geometry Studio",
                    UiNodeKind::Panel {
                        tabs: vec!["Timeline".to_owned(), "Motion Editor".to_owned()],
                        active_tab: 0,
                    },
                )
                .with_width(UiSizeUnit::Percent(100.0))
                .with_system_binding(SharedSystemKind::PanelHeader)
                .with_children(vec![
                    UiStudioNode::new(bar_row, "Timeline Header Toolbar", UiNodeKind::Row)
                        .with_children(vec![
                            UiStudioNode::new(
                                new_layer_btn,
                                "New Layer",
                                UiNodeKind::IconButton {
                                    icon_name: "layerplus".to_owned(),
                                    selected: false,
                                    md_size: false,
                                },
                            )
                            .with_width(UiSizeUnit::Px(28.0)),
                            UiStudioNode::new(
                                folder_btn,
                                "New Folder",
                                UiNodeKind::IconButton {
                                    icon_name: "folderplus".to_owned(),
                                    selected: false,
                                    md_size: false,
                                },
                            )
                            .with_width(UiSizeUnit::Px(28.0)),
                            UiStudioNode::new(
                                trash_btn,
                                "Delete Layer",
                                UiNodeKind::IconButton {
                                    icon_name: "trash".to_owned(),
                                    selected: false,
                                    md_size: false,
                                },
                            )
                            .with_width(UiSizeUnit::Px(28.0)),
                            UiStudioNode::new(
                                play_btn,
                                "Play",
                                UiNodeKind::IconButton {
                                    icon_name: "play".to_owned(),
                                    selected: true,
                                    md_size: false,
                                },
                            )
                            .with_width(UiSizeUnit::Px(28.0)),
                            UiStudioNode::new(
                                loop_btn,
                                "Loop",
                                UiNodeKind::IconButton {
                                    icon_name: "loop".to_owned(),
                                    selected: false,
                                    md_size: false,
                                },
                            )
                            .with_width(UiSizeUnit::Px(28.0)),
                            UiStudioNode::new(onion_sw, "Onion", UiNodeKind::Switch { on: true })
                                .with_width(UiSizeUnit::Px(82.0)),
                            UiStudioNode::new(
                                frame_fld,
                                "Current Frame",
                                UiNodeKind::Field {
                                    value: 14.0,
                                    prefix: "F".to_owned(),
                                    unit: "".to_owned(),
                                    decimals: 0,
                                    stepper: false,
                                },
                            )
                            .with_width(UiSizeUnit::Px(72.0)),
                            UiStudioNode::new(
                                fps_fld,
                                "Frame Rate",
                                UiNodeKind::Field {
                                    value: 24.0,
                                    prefix: "FPS".to_owned(),
                                    unit: "fps".to_owned(),
                                    decimals: 0,
                                    stepper: false,
                                },
                            )
                            .with_width(UiSizeUnit::Px(88.0)),
                        ]),
                    UiStudioNode::new(
                        layers_col_id,
                        "Layer Tree Column (6 Layers: Folder, Mask, Masked, Guide, Normal, Audio)",
                        UiNodeKind::Column,
                    )
                    .with_width(UiSizeUnit::Px(220.0)),
                    UiStudioNode::new(
                        ruler_playhead_id,
                        "Frame Ruler (1..35), Red Playhead & Onion Skin Brackets",
                        UiNodeKind::Row,
                    )
                    .with_width(UiSizeUnit::Percent(100.0))
                    .with_height(UiSizeUnit::Px(22.0)),
                    UiStudioNode::new(
                        classic_tween_id,
                        "Classic Tween Span (Purple + Arrow ──────►)",
                        UiNodeKind::Row,
                    )
                    .with_width(UiSizeUnit::Percent(100.0))
                    .with_height(UiSizeUnit::Px(24.0)),
                    UiStudioNode::new(
                        shape_tween_id,
                        "Shape Tween Span (Green + Arrow ──────►)",
                        UiNodeKind::Row,
                    )
                    .with_width(UiSizeUnit::Percent(100.0))
                    .with_height(UiSizeUnit::Px(24.0)),
                    UiStudioNode::new(
                        motion_tween_id,
                        "Motion Tween Span (Blue + Property Diamonds ◆)",
                        UiNodeKind::Row,
                    )
                    .with_width(UiSizeUnit::Percent(100.0))
                    .with_height(UiSizeUnit::Px(24.0)),
                    UiStudioNode::new(
                        broken_tween_id,
                        "Broken Tween Span (Dashed Line - - - - -)",
                        UiNodeKind::Row,
                    )
                    .with_width(UiSizeUnit::Percent(100.0))
                    .with_height(UiSizeUnit::Px(24.0)),
                    UiStudioNode::new(
                        keyframes_badges_id,
                        "Keyframe Dots (●/○), Span End ([ ]), Label Flag & Action (a)",
                        UiNodeKind::Row,
                    )
                    .with_width(UiSizeUnit::Percent(100.0))
                    .with_height(UiSizeUnit::Px(24.0)),
                    UiStudioNode::new(
                        audio_waveform_id,
                        "Soundtrack Audio Waveform Span",
                        UiNodeKind::Row,
                    )
                    .with_width(UiSizeUnit::Percent(100.0))
                    .with_height(UiSizeUnit::Px(24.0)),
                ])
            }
            Self::LibraryAndAssetsStudio => {
                let root_id = alloc();
                let preview_sec = alloc();
                let search_fld = alloc();
                let hdr_row = alloc();
                let sym1 = alloc();
                let sym2 = alloc();
                let sym3 = alloc();
                let sym4 = alloc();
                let footer_row = alloc();
                let new_sym_btn = alloc();
                let folder_btn = alloc();
                let props_btn = alloc();
                let del_btn = alloc();

                UiStudioNode::new(
                    root_id,
                    "Library Split: Preview + Tree",
                    UiNodeKind::Panel {
                        tabs: vec!["Library".to_owned(), "Assets".to_owned()],
                        active_tab: 0,
                    },
                )
                .with_width(UiSizeUnit::Px(320.0))
                .with_system_binding(SharedSystemKind::PanelHeader)
                .with_children(vec![
                    UiStudioNode::new(
                        preview_sec,
                        "SYMBOL PREVIEW STAGE (Hero_MC · Movie Clip)",
                        UiNodeKind::Section {
                            collapsible: true,
                            expanded: true,
                        },
                    )
                    .with_children(vec![UiStudioNode::new(
                        search_fld,
                        "Filter Symbols",
                        UiNodeKind::TextField {
                            text: "".to_owned(),
                            prefix: "Find".to_owned(),
                            hint: "Search symbols, bitmaps, sounds…".to_owned(),
                            mono: false,
                        },
                    )]),
                    UiStudioNode::new(
                        hdr_row,
                        "Library Columns Header",
                        UiNodeKind::TableRow {
                            cells: vec!["NAME".to_owned(), "KIND".to_owned(), "USE".to_owned()],
                            column_fractions: vec![0.0, 0.56, 0.84],
                            is_header: true,
                            selected: false,
                        },
                    ),
                    UiStudioNode::new(
                        sym1,
                        "Hero_MC Row",
                        UiNodeKind::TableRow {
                            cells: vec![
                                "Hero_MC".to_owned(),
                                "Movie Clip".to_owned(),
                                "4".to_owned(),
                            ],
                            column_fractions: vec![0.0, 0.56, 0.84],
                            is_header: false,
                            selected: true,
                        },
                    ),
                    UiStudioNode::new(
                        sym2,
                        "CTA_Button Row",
                        UiNodeKind::TableRow {
                            cells: vec![
                                "CTA_Button".to_owned(),
                                "Button".to_owned(),
                                "2".to_owned(),
                            ],
                            column_fractions: vec![0.0, 0.56, 0.84],
                            is_header: false,
                            selected: false,
                        },
                    ),
                    UiStudioNode::new(
                        sym3,
                        "Cloud_Graphic Row",
                        UiNodeKind::TableRow {
                            cells: vec![
                                "Cloud_Graphic".to_owned(),
                                "Graphic".to_owned(),
                                "9".to_owned(),
                            ],
                            column_fractions: vec![0.0, 0.56, 0.84],
                            is_header: false,
                            selected: false,
                        },
                    ),
                    UiStudioNode::new(
                        sym4,
                        "Theme_Loop.mp3 Row",
                        UiNodeKind::TableRow {
                            cells: vec![
                                "Theme_Loop.mp3".to_owned(),
                                "Sound".to_owned(),
                                "1".to_owned(),
                            ],
                            column_fractions: vec![0.0, 0.56, 0.84],
                            is_header: false,
                            selected: false,
                        },
                    ),
                    UiStudioNode::new(
                        footer_row,
                        "Library Footer Actions (4-Col Grid)",
                        UiNodeKind::Grid {
                            columns: vec![
                                UiSizeUnit::Percent(25.0),
                                UiSizeUnit::Percent(25.0),
                                UiSizeUnit::Percent(25.0),
                                UiSizeUnit::Percent(25.0),
                            ],
                        },
                    )
                    .with_children(vec![
                        UiStudioNode::new(
                            new_sym_btn,
                            "New Symbol",
                            UiNodeKind::IconButton {
                                icon_name: "plus".to_owned(),
                                selected: false,
                                md_size: false,
                            },
                        )
                        .with_width(UiSizeUnit::Percent(100.0)),
                        UiStudioNode::new(
                            folder_btn,
                            "New Folder",
                            UiNodeKind::IconButton {
                                icon_name: "folderplus".to_owned(),
                                selected: false,
                                md_size: false,
                            },
                        )
                        .with_width(UiSizeUnit::Percent(100.0)),
                        UiStudioNode::new(
                            props_btn,
                            "Symbol Properties",
                            UiNodeKind::IconButton {
                                icon_name: "settings".to_owned(),
                                selected: false,
                                md_size: false,
                            },
                        )
                        .with_width(UiSizeUnit::Percent(100.0)),
                        UiStudioNode::new(
                            del_btn,
                            "Delete Symbol",
                            UiNodeKind::IconButton {
                                icon_name: "trash".to_owned(),
                                selected: false,
                                md_size: false,
                            },
                        )
                        .with_width(UiSizeUnit::Percent(100.0)),
                    ]),
                ])
            }
            Self::ColorAndGradientStudio => {
                let root_id = alloc();
                let top_row = alloc();
                let stroke_well = alloc();
                let fill_well = alloc();
                let mode_sel = alloc();
                let grad_sec = alloc();
                let stop_grid = alloc();
                let stop1_pos = alloc();
                let stop2_pos = alloc();
                let rgba_grid = alloc();
                let r_fld = alloc();
                let g_fld = alloc();
                let b_fld = alloc();
                let a_fld = alloc();
                let swatches_sec = alloc();
                let sw_grid = alloc();

                let mut sw_children = Vec::new();
                for (lbl, rgba) in [
                    ("Crimson", [235, 68, 90, 255]),
                    ("Amber", [255, 168, 52, 255]),
                    ("Emerald", [52, 208, 132, 255]),
                    ("Cobalt", [76, 132, 255, 255]),
                ] {
                    sw_children.push(
                        UiStudioNode::new(
                            alloc(),
                            lbl,
                            UiNodeKind::ColorSwatch {
                                rgba,
                                is_stroke: false,
                            },
                        )
                        .with_width(UiSizeUnit::Percent(100.0)),
                    );
                }

                UiStudioNode::new(
                    root_id,
                    "Color Mixer, Gradient Bar & Swatches",
                    UiNodeKind::Panel {
                        tabs: vec![
                            "Color Mixer".to_owned(),
                            "Gradient".to_owned(),
                            "Swatches".to_owned(),
                        ],
                        active_tab: 0,
                    },
                )
                .with_width(UiSizeUnit::Px(320.0))
                .with_system_binding(SharedSystemKind::PanelHeader)
                .with_children(vec![
                    UiStudioNode::new(top_row, "Stroke / Fill Wells & Paint Mode", UiNodeKind::Row)
                        .with_children(vec![
                            UiStudioNode::new(
                                stroke_well,
                                "Stroke Well",
                                UiNodeKind::ColorSwatch {
                                    rgba: [108, 147, 255, 255],
                                    is_stroke: true,
                                },
                            )
                            .with_width(UiSizeUnit::Px(74.0)),
                            UiStudioNode::new(
                                fill_well,
                                "Fill Well",
                                UiNodeKind::ColorSwatch {
                                    rgba: [255, 114, 92, 255],
                                    is_stroke: false,
                                },
                            )
                            .with_width(UiSizeUnit::Px(74.0)),
                            UiStudioNode::new(
                                mode_sel,
                                "Gradient Mode",
                                UiNodeKind::Select {
                                    selected: 1,
                                    options: vec![
                                        "Solid".to_owned(),
                                        "Linear Gradient".to_owned(),
                                        "Radial Gradient".to_owned(),
                                        "Bitmap Fill".to_owned(),
                                    ],
                                },
                            )
                            .with_width(UiSizeUnit::Fill(1.0)),
                        ]),
                    UiStudioNode::new(
                        grad_sec,
                        "GRADIENT RAMP & COLOR STOPS",
                        UiNodeKind::Section {
                            collapsible: true,
                            expanded: true,
                        },
                    )
                    .with_children(vec![
                        UiStudioNode::new(
                            stop_grid,
                            "Gradient Stop Positions (2-Col Grid)",
                            UiNodeKind::Grid {
                                columns: vec![UiSizeUnit::Percent(50.0), UiSizeUnit::Percent(50.0)],
                            },
                        )
                        .with_system_binding(SharedSystemKind::TwoColumnForm)
                        .with_children(vec![
                            UiStudioNode::new(
                                stop1_pos,
                                "Stop 1 Pos",
                                UiNodeKind::Field {
                                    value: 0.0,
                                    prefix: "S1".to_owned(),
                                    unit: "%".to_owned(),
                                    decimals: 0,
                                    stepper: false,
                                },
                            ),
                            UiStudioNode::new(
                                stop2_pos,
                                "Stop 2 Pos",
                                UiNodeKind::Field {
                                    value: 100.0,
                                    prefix: "S2".to_owned(),
                                    unit: "%".to_owned(),
                                    decimals: 0,
                                    stepper: false,
                                },
                            ),
                        ]),
                    ]),
                    UiStudioNode::new(
                        rgba_grid,
                        "RGBA Channels (4-Col Grid)",
                        UiNodeKind::Grid {
                            columns: vec![
                                UiSizeUnit::Percent(25.0),
                                UiSizeUnit::Percent(25.0),
                                UiSizeUnit::Percent(25.0),
                                UiSizeUnit::Percent(25.0),
                            ],
                        },
                    )
                    .with_children(vec![
                        UiStudioNode::new(
                            r_fld,
                            "Red",
                            UiNodeKind::Field {
                                value: 255.0,
                                prefix: "R".to_owned(),
                                unit: "".to_owned(),
                                decimals: 0,
                                stepper: false,
                            },
                        ),
                        UiStudioNode::new(
                            g_fld,
                            "Green",
                            UiNodeKind::Field {
                                value: 114.0,
                                prefix: "G".to_owned(),
                                unit: "".to_owned(),
                                decimals: 0,
                                stepper: false,
                            },
                        ),
                        UiStudioNode::new(
                            b_fld,
                            "Blue",
                            UiNodeKind::Field {
                                value: 92.0,
                                prefix: "B".to_owned(),
                                unit: "".to_owned(),
                                decimals: 0,
                                stepper: false,
                            },
                        ),
                        UiStudioNode::new(
                            a_fld,
                            "Alpha",
                            UiNodeKind::Field {
                                value: 100.0,
                                prefix: "A".to_owned(),
                                unit: "%".to_owned(),
                                decimals: 0,
                                stepper: false,
                            },
                        ),
                    ]),
                    UiStudioNode::new(
                        swatches_sec,
                        "SWATCH PALETTE (4-COL GRID)",
                        UiNodeKind::Section {
                            collapsible: true,
                            expanded: true,
                        },
                    )
                    .with_children(vec![
                        UiStudioNode::new(
                            sw_grid,
                            "Swatches 4-Col Row",
                            UiNodeKind::Grid {
                                columns: vec![
                                    UiSizeUnit::Percent(25.0),
                                    UiSizeUnit::Percent(25.0),
                                    UiSizeUnit::Percent(25.0),
                                    UiSizeUnit::Percent(25.0),
                                ],
                            },
                        )
                        .with_children(sw_children),
                    ]),
                ])
            }
            Self::AlignAndTransform => {
                let root_id = alloc();
                let stage_sw = alloc();
                let align_sec = alloc();
                let align_grid = alloc();
                let dist_sec = alloc();
                let dist_grid = alloc();
                let xform_sec = alloc();
                let xform_grid = alloc();
                let sx_id = alloc();
                let sy_id = alloc();
                let rot_id = alloc();
                let skew_id = alloc();

                let align_btns = [
                    ("Align Left", "align-left"),
                    ("Align Center H", "align-center-h"),
                    ("Align Right", "align-right"),
                    ("Align Top", "align-top"),
                    ("Align Center V", "align-center-v"),
                    ("Align Bottom", "align-bottom"),
                ]
                .into_iter()
                .map(|(lbl, ico)| {
                    UiStudioNode::new(
                        alloc(),
                        lbl,
                        UiNodeKind::IconButton {
                            icon_name: ico.to_owned(),
                            selected: false,
                            md_size: false,
                        },
                    )
                    .with_width(UiSizeUnit::Percent(100.0))
                })
                .collect();

                let dist_btns = [
                    ("Distribute Left", "distribute-h"),
                    ("Distribute Center H", "space-h"),
                    ("Distribute Top", "distribute-v"),
                    ("Distribute Center V", "space-v"),
                ]
                .into_iter()
                .map(|(lbl, ico)| {
                    UiStudioNode::new(
                        alloc(),
                        lbl,
                        UiNodeKind::IconButton {
                            icon_name: ico.to_owned(),
                            selected: false,
                            md_size: false,
                        },
                    )
                    .with_width(UiSizeUnit::Percent(100.0))
                })
                .collect();

                UiStudioNode::new(
                    root_id,
                    "Align & Transform",
                    UiNodeKind::Panel {
                        tabs: vec!["Align".to_owned(), "Transform".to_owned()],
                        active_tab: 0,
                    },
                )
                .with_width(UiSizeUnit::Px(268.0))
                .with_system_binding(SharedSystemKind::PanelHeader)
                .with_children(vec![
                    UiStudioNode::new(stage_sw, "Align to Stage", UiNodeKind::Switch { on: true }),
                    UiStudioNode::new(
                        align_sec,
                        "ALIGN OBJECTS (3-COL GRID)",
                        UiNodeKind::Section {
                            collapsible: true,
                            expanded: true,
                        },
                    )
                    .with_children(vec![
                        UiStudioNode::new(
                            align_grid,
                            "Align 3-Column Grid",
                            UiNodeKind::Grid {
                                columns: vec![
                                    UiSizeUnit::Percent(33.3333),
                                    UiSizeUnit::Percent(33.3333),
                                    UiSizeUnit::Percent(33.3334),
                                ],
                            },
                        )
                        .with_children(align_btns),
                    ]),
                    UiStudioNode::new(
                        dist_sec,
                        "DISTRIBUTE & SPACE (4-COL GRID)",
                        UiNodeKind::Section {
                            collapsible: true,
                            expanded: true,
                        },
                    )
                    .with_children(vec![
                        UiStudioNode::new(
                            dist_grid,
                            "Distribute 4-Column Grid",
                            UiNodeKind::Grid {
                                columns: vec![
                                    UiSizeUnit::Percent(25.0),
                                    UiSizeUnit::Percent(25.0),
                                    UiSizeUnit::Percent(25.0),
                                    UiSizeUnit::Percent(25.0),
                                ],
                            },
                        )
                        .with_children(dist_btns),
                    ]),
                    UiStudioNode::new(
                        xform_sec,
                        "TRANSFORM MATRIX (2-COL GRID)",
                        UiNodeKind::Section {
                            collapsible: true,
                            expanded: true,
                        },
                    )
                    .with_children(vec![
                        UiStudioNode::new(
                            xform_grid,
                            "Transform 2-Col Grid",
                            UiNodeKind::Grid {
                                columns: vec![UiSizeUnit::Percent(50.0), UiSizeUnit::Percent(50.0)],
                            },
                        )
                        .with_system_binding(SharedSystemKind::TwoColumnForm)
                        .with_children(vec![
                            UiStudioNode::new(
                                sx_id,
                                "Scale X",
                                UiNodeKind::Field {
                                    value: 100.0,
                                    prefix: "SX".to_owned(),
                                    unit: "%".to_owned(),
                                    decimals: 1,
                                    stepper: false,
                                },
                            ),
                            UiStudioNode::new(
                                sy_id,
                                "Scale Y",
                                UiNodeKind::Field {
                                    value: 100.0,
                                    prefix: "SY".to_owned(),
                                    unit: "%".to_owned(),
                                    decimals: 1,
                                    stepper: false,
                                },
                            ),
                            UiStudioNode::new(
                                rot_id,
                                "Rotate",
                                UiNodeKind::Field {
                                    value: 0.0,
                                    prefix: "Rot".to_owned(),
                                    unit: "°".to_owned(),
                                    decimals: 1,
                                    stepper: false,
                                },
                            ),
                            UiStudioNode::new(
                                skew_id,
                                "Skew X",
                                UiNodeKind::Field {
                                    value: 0.0,
                                    prefix: "SkX".to_owned(),
                                    unit: "°".to_owned(),
                                    decimals: 1,
                                    stepper: false,
                                },
                            ),
                        ]),
                    ]),
                ])
            }
            Self::PropertiesMovieClip => {
                let root_id = alloc();
                let instance_id = alloc();
                let pos_sec_id = alloc();
                let pos_grid_id = alloc();
                let x_id = alloc();
                let y_id = alloc();
                let w_id = alloc();
                let h_id = alloc();
                let rot_id = alloc();
                let skew_id = alloc();
                let disp_sec_id = alloc();
                let blend_id = alloc();
                let alpha_id = alloc();
                let vis_row_id = alloc();
                let vis_chk_id = alloc();
                let cache_chk_id = alloc();

                UiStudioNode::new(
                    root_id,
                    "Properties — Movie Clip",
                    UiNodeKind::Panel {
                        tabs: vec!["Properties".to_owned(), "Filters".to_owned()],
                        active_tab: 0,
                    },
                )
                .with_width(UiSizeUnit::Px(320.0))
                .with_system_binding(SharedSystemKind::PanelHeader)
                .with_children(vec![
                    UiStudioNode::new(
                        instance_id,
                        "Instance Name",
                        UiNodeKind::TextField {
                            text: "hero_mc".to_owned(),
                            prefix: "Name".to_owned(),
                            hint: "<Instance Name>".to_owned(),
                            mono: true,
                        },
                    ),
                    UiStudioNode::new(
                        pos_sec_id,
                        "POSITION AND SIZE",
                        UiNodeKind::Section {
                            collapsible: true,
                            expanded: true,
                        },
                    )
                    .with_children(vec![
                        UiStudioNode::new(
                            pos_grid_id,
                            "Transform 2-Col Grid",
                            UiNodeKind::Grid {
                                columns: vec![UiSizeUnit::Percent(50.0), UiSizeUnit::Percent(50.0)],
                            },
                        )
                        .with_system_binding(SharedSystemKind::TwoColumnForm)
                        .with_children(vec![
                            UiStudioNode::new(
                                x_id,
                                "Position X",
                                UiNodeKind::Field {
                                    value: 140.0,
                                    prefix: "X".to_owned(),
                                    unit: "px".to_owned(),
                                    decimals: 1,
                                    stepper: false,
                                },
                            ),
                            UiStudioNode::new(
                                y_id,
                                "Position Y",
                                UiNodeKind::Field {
                                    value: 96.0,
                                    prefix: "Y".to_owned(),
                                    unit: "px".to_owned(),
                                    decimals: 1,
                                    stepper: false,
                                },
                            ),
                            UiStudioNode::new(
                                w_id,
                                "Width",
                                UiNodeKind::Field {
                                    value: 240.0,
                                    prefix: "W".to_owned(),
                                    unit: "px".to_owned(),
                                    decimals: 1,
                                    stepper: false,
                                },
                            ),
                            UiStudioNode::new(
                                h_id,
                                "Height",
                                UiNodeKind::Field {
                                    value: 180.0,
                                    prefix: "H".to_owned(),
                                    unit: "px".to_owned(),
                                    decimals: 1,
                                    stepper: false,
                                },
                            ),
                            UiStudioNode::new(
                                rot_id,
                                "Rotation",
                                UiNodeKind::Field {
                                    value: 0.0,
                                    prefix: "R".to_owned(),
                                    unit: "°".to_owned(),
                                    decimals: 1,
                                    stepper: false,
                                },
                            ),
                            UiStudioNode::new(
                                skew_id,
                                "Scale X",
                                UiNodeKind::Field {
                                    value: 100.0,
                                    prefix: "SX".to_owned(),
                                    unit: "%".to_owned(),
                                    decimals: 0,
                                    stepper: false,
                                },
                            ),
                        ]),
                    ]),
                    UiStudioNode::new(
                        disp_sec_id,
                        "DISPLAY",
                        UiNodeKind::Section {
                            collapsible: true,
                            expanded: true,
                        },
                    )
                    .with_children(vec![
                        UiStudioNode::new(
                            blend_id,
                            "Blend Mode",
                            UiNodeKind::Select {
                                selected: 0,
                                options: vec![
                                    "Normal".to_owned(),
                                    "Multiply".to_owned(),
                                    "Screen".to_owned(),
                                    "Overlay".to_owned(),
                                    "Add".to_owned(),
                                ],
                            },
                        ),
                        UiStudioNode::new(
                            alpha_id,
                            "Alpha (%)",
                            UiNodeKind::Slider {
                                value: 100.0,
                                min: 0.0,
                                max: 100.0,
                                show_field: true,
                            },
                        ),
                        UiStudioNode::new(vis_row_id, "Display Flags Row", UiNodeKind::Row)
                            .with_children(vec![
                                UiStudioNode::new(
                                    vis_chk_id,
                                    "Visible",
                                    UiNodeKind::Checkbox { checked: true },
                                )
                                .with_width(UiSizeUnit::Percent(50.0)),
                                UiStudioNode::new(
                                    cache_chk_id,
                                    "Cache as bitmap",
                                    UiNodeKind::Checkbox { checked: false },
                                )
                                .with_width(UiSizeUnit::Percent(50.0)),
                            ]),
                    ]),
                ])
            }
            Self::PropertiesText => {
                let root_id = alloc();
                let char_sec = alloc();
                let family_sel = alloc();
                let style_row = alloc();
                let weight_sel = alloc();
                let size_fld = alloc();
                let metrics_grid = alloc();
                let line_fld = alloc();
                let track_fld = alloc();
                let align_grid = alloc();
                let al_l = alloc();
                let al_c = alloc();
                let al_r = alloc();
                let al_j = alloc();

                UiStudioNode::new(
                    root_id,
                    "Properties — Text",
                    UiNodeKind::Panel {
                        tabs: vec!["Properties".to_owned()],
                        active_tab: 0,
                    },
                )
                .with_width(UiSizeUnit::Px(320.0))
                .with_system_binding(SharedSystemKind::PanelHeader)
                .with_children(vec![
                    UiStudioNode::new(
                        char_sec,
                        "CHARACTER",
                        UiNodeKind::Section {
                            collapsible: true,
                            expanded: true,
                        },
                    )
                    .with_children(vec![
                        UiStudioNode::new(
                            family_sel,
                            "Font Family",
                            UiNodeKind::Select {
                                selected: 0,
                                options: vec![
                                    "Inter".to_owned(),
                                    "Roboto Mono".to_owned(),
                                    "Helvetica Neue".to_owned(),
                                ],
                            },
                        ),
                        UiStudioNode::new(style_row, "Weight & Size Row", UiNodeKind::Row)
                            .with_children(vec![
                                UiStudioNode::new(
                                    weight_sel,
                                    "Font Weight",
                                    UiNodeKind::Select {
                                        selected: 1,
                                        options: vec![
                                            "Regular".to_owned(),
                                            "Medium".to_owned(),
                                            "Semi Bold".to_owned(),
                                        ],
                                    },
                                )
                                .with_width(UiSizeUnit::Percent(60.0)),
                                UiStudioNode::new(
                                    size_fld,
                                    "Font Size",
                                    UiNodeKind::Field {
                                        value: 16.0,
                                        prefix: "Sz".to_owned(),
                                        unit: "pt".to_owned(),
                                        decimals: 0,
                                        stepper: true,
                                    },
                                )
                                .with_width(UiSizeUnit::Percent(40.0)),
                            ]),
                        UiStudioNode::new(
                            metrics_grid,
                            "Spacing 2-Col Grid",
                            UiNodeKind::Grid {
                                columns: vec![UiSizeUnit::Percent(50.0), UiSizeUnit::Percent(50.0)],
                            },
                        )
                        .with_system_binding(SharedSystemKind::TwoColumnForm)
                        .with_children(vec![
                            UiStudioNode::new(
                                line_fld,
                                "Line Spacing",
                                UiNodeKind::Field {
                                    value: 20.0,
                                    prefix: "LH".to_owned(),
                                    unit: "pt".to_owned(),
                                    decimals: 0,
                                    stepper: false,
                                },
                            ),
                            UiStudioNode::new(
                                track_fld,
                                "Letter Spacing",
                                UiNodeKind::Field {
                                    value: 0.0,
                                    prefix: "AV".to_owned(),
                                    unit: "em".to_owned(),
                                    decimals: 2,
                                    stepper: false,
                                },
                            ),
                        ]),
                        UiStudioNode::new(
                            align_grid,
                            "Paragraph Align (4-Col Grid)",
                            UiNodeKind::Grid {
                                columns: vec![
                                    UiSizeUnit::Percent(25.0),
                                    UiSizeUnit::Percent(25.0),
                                    UiSizeUnit::Percent(25.0),
                                    UiSizeUnit::Percent(25.0),
                                ],
                            },
                        )
                        .with_children(vec![
                            UiStudioNode::new(
                                al_l,
                                "Align Left",
                                UiNodeKind::IconButton {
                                    icon_name: "talign-left".to_owned(),
                                    selected: true,
                                    md_size: false,
                                },
                            )
                            .with_width(UiSizeUnit::Percent(100.0)),
                            UiStudioNode::new(
                                al_c,
                                "Align Center",
                                UiNodeKind::IconButton {
                                    icon_name: "talign-center".to_owned(),
                                    selected: false,
                                    md_size: false,
                                },
                            )
                            .with_width(UiSizeUnit::Percent(100.0)),
                            UiStudioNode::new(
                                al_r,
                                "Align Right",
                                UiNodeKind::IconButton {
                                    icon_name: "talign-right".to_owned(),
                                    selected: false,
                                    md_size: false,
                                },
                            )
                            .with_width(UiSizeUnit::Percent(100.0)),
                            UiStudioNode::new(
                                al_j,
                                "Justify",
                                UiNodeKind::IconButton {
                                    icon_name: "talign-justify".to_owned(),
                                    selected: false,
                                    md_size: false,
                                },
                            )
                            .with_width(UiSizeUnit::Percent(100.0)),
                        ]),
                    ]),
                ])
            }
            Self::ColorPanel => {
                let root_id = alloc();
                let top_row = alloc();
                let stroke_well = alloc();
                let fill_well = alloc();
                let mode_sel = alloc();
                let rgba_grid = alloc();
                let r_fld = alloc();
                let g_fld = alloc();
                let b_fld = alloc();
                let a_fld = alloc();
                let alpha_slider = alloc();

                UiStudioNode::new(
                    root_id,
                    "Color",
                    UiNodeKind::Panel {
                        tabs: vec!["Color".to_owned(), "Swatches".to_owned()],
                        active_tab: 0,
                    },
                )
                .with_width(UiSizeUnit::Px(288.0))
                .with_system_binding(SharedSystemKind::PanelHeader)
                .with_children(vec![
                    UiStudioNode::new(top_row, "Wells & Mode Row", UiNodeKind::Row).with_children(
                        vec![
                            UiStudioNode::new(
                                stroke_well,
                                "Stroke Well",
                                UiNodeKind::ColorSwatch {
                                    rgba: [108, 147, 255, 255],
                                    is_stroke: true,
                                },
                            )
                            .with_width(UiSizeUnit::Px(74.0)),
                            UiStudioNode::new(
                                fill_well,
                                "Fill Well",
                                UiNodeKind::ColorSwatch {
                                    rgba: [255, 154, 92, 255],
                                    is_stroke: false,
                                },
                            )
                            .with_width(UiSizeUnit::Px(74.0)),
                            UiStudioNode::new(
                                mode_sel,
                                "Paint Type",
                                UiNodeKind::Select {
                                    selected: 0,
                                    options: vec![
                                        "Solid".to_owned(),
                                        "Linear".to_owned(),
                                        "Radial".to_owned(),
                                    ],
                                },
                            )
                            .with_width(UiSizeUnit::Fill(1.0)),
                        ],
                    ),
                    UiStudioNode::new(
                        rgba_grid,
                        "RGBA Channels (4-Col Grid)",
                        UiNodeKind::Grid {
                            columns: vec![
                                UiSizeUnit::Percent(25.0),
                                UiSizeUnit::Percent(25.0),
                                UiSizeUnit::Percent(25.0),
                                UiSizeUnit::Percent(25.0),
                            ],
                        },
                    )
                    .with_children(vec![
                        UiStudioNode::new(
                            r_fld,
                            "Red",
                            UiNodeKind::Field {
                                value: 108.0,
                                prefix: "R".to_owned(),
                                unit: "".to_owned(),
                                decimals: 0,
                                stepper: false,
                            },
                        ),
                        UiStudioNode::new(
                            g_fld,
                            "Green",
                            UiNodeKind::Field {
                                value: 147.0,
                                prefix: "G".to_owned(),
                                unit: "".to_owned(),
                                decimals: 0,
                                stepper: false,
                            },
                        ),
                        UiStudioNode::new(
                            b_fld,
                            "Blue",
                            UiNodeKind::Field {
                                value: 255.0,
                                prefix: "B".to_owned(),
                                unit: "".to_owned(),
                                decimals: 0,
                                stepper: false,
                            },
                        ),
                        UiStudioNode::new(
                            a_fld,
                            "Alpha",
                            UiNodeKind::Field {
                                value: 100.0,
                                prefix: "A".to_owned(),
                                unit: "%".to_owned(),
                                decimals: 0,
                                stepper: false,
                            },
                        ),
                    ]),
                    UiStudioNode::new(
                        alpha_slider,
                        "Opacity Slider",
                        UiNodeKind::Slider {
                            value: 100.0,
                            min: 0.0,
                            max: 100.0,
                            show_field: true,
                        },
                    ),
                ])
            }
            Self::ToolsPanel => {
                let root_id = alloc();
                let grid_id = alloc();
                let tools = [
                    ("Select (V)", "select", true),
                    ("Subselect (A)", "subselect", false),
                    ("Transform (Q)", "transform", false),
                    ("Pen (P)", "pen", false),
                    ("Text (T)", "text", false),
                    ("Line (N)", "line", false),
                    ("Rectangle (R)", "rect", false),
                    ("Oval (O)", "oval", false),
                    ("Pencil (Y)", "pencil", false),
                    ("Brush (B)", "brush", false),
                    ("Paint Bucket (K)", "bucket", false),
                    ("Eyedropper (I)", "eyedropper", false),
                ];
                let tool_nodes = tools
                    .into_iter()
                    .map(|(lbl, ico, sel)| {
                        UiStudioNode::new(
                            alloc(),
                            lbl,
                            UiNodeKind::IconButton {
                                icon_name: ico.to_owned(),
                                selected: sel,
                                md_size: false,
                            },
                        )
                    })
                    .collect();

                UiStudioNode::new(
                    root_id,
                    "Tools",
                    UiNodeKind::Panel {
                        tabs: Vec::new(),
                        active_tab: 0,
                    },
                )
                .with_width(UiSizeUnit::Px(80.0))
                .with_padding(Padding::all_px(7.0))
                .with_children(vec![
                    UiStudioNode::new(
                        grid_id,
                        "Tools 2-Column Rail",
                        UiNodeKind::Grid {
                            columns: vec![UiSizeUnit::Px(32.0), UiSizeUnit::Px(32.0)],
                        },
                    )
                    .with_system_binding(SharedSystemKind::ToolRail)
                    .with_gap(GapPair::both_px(2.0))
                    .with_children(tool_nodes),
                ])
            }
            Self::FiltersPanel => {
                let root_id = alloc();
                let hdr_row = alloc();
                let row1 = alloc();
                let row2 = alloc();
                let params_sec = alloc();
                let params_grid = alloc();
                let bx = alloc();
                let by = alloc();
                let str_fld = alloc();
                let qual_sel = alloc();

                UiStudioNode::new(
                    root_id,
                    "Filters",
                    UiNodeKind::Panel {
                        tabs: vec!["Properties".to_owned(), "Filters".to_owned()],
                        active_tab: 1,
                    },
                )
                .with_width(UiSizeUnit::Px(320.0))
                .with_system_binding(SharedSystemKind::PanelHeader)
                .with_children(vec![
                    UiStudioNode::new(
                        hdr_row,
                        "Filter Table Header",
                        UiNodeKind::TableRow {
                            cells: vec!["FILTER".to_owned(), "QUALITY".to_owned()],
                            column_fractions: vec![0.0, 0.65],
                            is_header: true,
                            selected: false,
                        },
                    ),
                    UiStudioNode::new(
                        row1,
                        "Drop Shadow Row",
                        UiNodeKind::TableRow {
                            cells: vec!["Drop Shadow".to_owned(), "High".to_owned()],
                            column_fractions: vec![0.0, 0.65],
                            is_header: false,
                            selected: true,
                        },
                    ),
                    UiStudioNode::new(
                        row2,
                        "Blur Filter Row",
                        UiNodeKind::TableRow {
                            cells: vec!["Blur".to_owned(), "Medium".to_owned()],
                            column_fractions: vec![0.0, 0.65],
                            is_header: false,
                            selected: false,
                        },
                    ),
                    UiStudioNode::new(
                        params_sec,
                        "DROP SHADOW PARAMETERS",
                        UiNodeKind::Section {
                            collapsible: true,
                            expanded: true,
                        },
                    )
                    .with_children(vec![
                        UiStudioNode::new(
                            params_grid,
                            "Filter Params 2-Col Grid",
                            UiNodeKind::Grid {
                                columns: vec![UiSizeUnit::Percent(50.0), UiSizeUnit::Percent(50.0)],
                            },
                        )
                        .with_system_binding(SharedSystemKind::TwoColumnForm)
                        .with_children(vec![
                            UiStudioNode::new(
                                bx,
                                "Blur X",
                                UiNodeKind::Field {
                                    value: 8.0,
                                    prefix: "BX".to_owned(),
                                    unit: "px".to_owned(),
                                    decimals: 1,
                                    stepper: false,
                                },
                            ),
                            UiStudioNode::new(
                                by,
                                "Blur Y",
                                UiNodeKind::Field {
                                    value: 8.0,
                                    prefix: "BY".to_owned(),
                                    unit: "px".to_owned(),
                                    decimals: 1,
                                    stepper: false,
                                },
                            ),
                            UiStudioNode::new(
                                str_fld,
                                "Strength",
                                UiNodeKind::Field {
                                    value: 100.0,
                                    prefix: "Str".to_owned(),
                                    unit: "%".to_owned(),
                                    decimals: 0,
                                    stepper: false,
                                },
                            ),
                            UiStudioNode::new(
                                qual_sel,
                                "Quality",
                                UiNodeKind::Select {
                                    selected: 2,
                                    options: vec![
                                        "Low".to_owned(),
                                        "Medium".to_owned(),
                                        "High".to_owned(),
                                    ],
                                },
                            ),
                        ]),
                    ]),
                ])
            }
            Self::TimelineHeader => {
                let root_id = alloc();
                let bar_row = alloc();
                let new_layer_btn = alloc();
                let folder_btn = alloc();
                let trash_btn = alloc();
                let sep1 = alloc();
                let play_btn = alloc();
                let loop_btn = alloc();
                let onion_sw = alloc();
                let frame_fld = alloc();
                let fps_fld = alloc();

                UiStudioNode::new(
                    root_id,
                    "Timeline Header",
                    UiNodeKind::Panel {
                        tabs: vec!["Timeline".to_owned(), "Motion Editor".to_owned()],
                        active_tab: 0,
                    },
                )
                .with_width(UiSizeUnit::Px(560.0))
                .with_system_binding(SharedSystemKind::PanelHeader)
                .with_children(vec![
                    UiStudioNode::new(bar_row, "Timeline Controls Row", UiNodeKind::Row)
                        .with_children(vec![
                            UiStudioNode::new(
                                new_layer_btn,
                                "New Layer",
                                UiNodeKind::IconButton {
                                    icon_name: "layerplus".to_owned(),
                                    selected: false,
                                    md_size: false,
                                },
                            )
                            .with_width(UiSizeUnit::Px(32.0)),
                            UiStudioNode::new(
                                folder_btn,
                                "New Folder",
                                UiNodeKind::IconButton {
                                    icon_name: "folderplus".to_owned(),
                                    selected: false,
                                    md_size: false,
                                },
                            )
                            .with_width(UiSizeUnit::Px(32.0)),
                            UiStudioNode::new(
                                trash_btn,
                                "Delete Layer",
                                UiNodeKind::IconButton {
                                    icon_name: "trash".to_owned(),
                                    selected: false,
                                    md_size: false,
                                },
                            )
                            .with_width(UiSizeUnit::Px(32.0)),
                            UiStudioNode::new(
                                sep1,
                                "Divider",
                                UiNodeKind::Separator { vertical: true },
                            )
                            .with_width(UiSizeUnit::Px(8.0)),
                            UiStudioNode::new(
                                play_btn,
                                "Play",
                                UiNodeKind::IconButton {
                                    icon_name: "play".to_owned(),
                                    selected: true,
                                    md_size: false,
                                },
                            )
                            .with_width(UiSizeUnit::Px(32.0)),
                            UiStudioNode::new(
                                loop_btn,
                                "Loop Playback",
                                UiNodeKind::IconButton {
                                    icon_name: "loop".to_owned(),
                                    selected: false,
                                    md_size: false,
                                },
                            )
                            .with_width(UiSizeUnit::Px(32.0)),
                            UiStudioNode::new(onion_sw, "Onion", UiNodeKind::Switch { on: true })
                                .with_width(UiSizeUnit::Px(86.0)),
                            UiStudioNode::new(
                                frame_fld,
                                "Current Frame",
                                UiNodeKind::Field {
                                    value: 1.0,
                                    prefix: "F".to_owned(),
                                    unit: "".to_owned(),
                                    decimals: 0,
                                    stepper: false,
                                },
                            )
                            .with_width(UiSizeUnit::Px(76.0)),
                            UiStudioNode::new(
                                fps_fld,
                                "Frame Rate",
                                UiNodeKind::Field {
                                    value: 24.0,
                                    prefix: "FPS".to_owned(),
                                    unit: "fps".to_owned(),
                                    decimals: 0,
                                    stepper: false,
                                },
                            )
                            .with_width(UiSizeUnit::Fill(1.0)),
                        ]),
                ])
            }
        };

        UiStudioDocument {
            name: self.label().to_owned(),
            root,
            next_id,
        }
    }
}
