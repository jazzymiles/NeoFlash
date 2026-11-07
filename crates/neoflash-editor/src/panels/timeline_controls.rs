//! Timeline controls bar (reference §4 "Timeline controls bar", audit #2 and #4; R-052): one 44 pt row
//! above the grid. Left → right: layer actions · transport group (Play = square Primary Icon Button SM)
//! · loop · frame number · timecode · Scene select · FPS / Duration fields · frame insert group
//! (F5 F6 F7) · tween group · ease · sound; right-aligned: onion-skin group · centre playhead · zoom −
//! slider + · More ⋯ (hidden controls and frame view options).
//!
//! Controls whose feature the engine lacks are shown disabled with "not available yet" in their name
//! and tooltip, never as working-looking fakes; ease opens its planned preview (Ease panel). Sound
//! attaches a library sound to the current keyframe (`timeline_sound.rs`). Tweens, motion guides,
//! scenes, loop and edit multiple frames run the commands in `timeline_commands.rs`. When the bar is
//! narrower than its content, controls hide by the owner-approved priority in [`CONTROLS`] and appear
//! at the top of the More menu; nothing is clipped or scrolled out of sight.
use super::menu::NOT_AVAILABLE;
use crate::app::*;
use crate::workspace::{DockDestination, DockEdge, FramePreview, PanelId, PanelPlacement};
use eframe::egui::{self, Sense, Stroke, Vec2};
use neoflash_core::*;
use neoflash_ui::components::{self as nf, IconButton, IconButtonType, IconGroup, MenuItem};
use neoflash_ui::icons::i16;
use neoflash_ui::metrics::FrameView;
use neoflash_ui::tokens::space;
use neoflash_ui::{TextStyle, typography};
use std::collections::HashMap;

/// Narrowest and widest frame cell of the continuous zoom (Tiny is 6; overview zoom may go lower).
pub(crate) const MIN_CELL: f32 = 3.;
pub(crate) const MAX_CELL: f32 = 52.;
/// Gap between groups (`Timeline controls` auto-layout, gap 8) and inside groups (gap 2).
const GAP: f32 = space::S2;
const GROUP_GAP: f32 = 2.;
const SLIDER: f32 = 140.;

/// Requests from the bar that the frame area applies after layout.
#[derive(Default)]
pub(crate) struct ControlsOutcome {
    /// Scroll the frames so the playhead is centred (Centre playhead button).
    pub recenter: bool,
    /// Scroll the frames so this 0-based frame is at the left edge (Fit selection / Fit content).
    pub scroll_to: Option<u32>,
}

/// `mm:ss.hh` of a 0-based frame (frame 30 at 24 fps → `00:01.21`).
pub(crate) fn timecode(frame: u32, fps: u32) -> String {
    let t = frame as f64 / fps.max(1) as f64;
    let minutes = (t / 60.).floor();
    format!("{:02}:{:05.2}", minutes as u32, t - minutes * 60.)
}

/// Controls of the bar in visual order (R-052).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Ctl {
    AddLayer,
    Folder,
    Guide,
    Delete,
    Div1,
    Transport,
    Loop,
    Frame,
    Timecode,
    Div2,
    Scene,
    Fps,
    Duration,
    Insert,
    Tween,
    Ease,
    Sound,
    Flex,
    Onion,
    OnionOutline,
    EditMulti,
    OnionOptions,
    Div3,
    Centre,
    ZoomOut,
    Slider,
    ZoomIn,
    More,
}

/// Always shown: Add layer, transport and the More menu (which lists everything hidden).
pub(crate) const ESSENTIAL: u8 = u8::MAX;
/// Hidden only when the essential controls would not fit otherwise (Delete layer, frame number,
/// dividers), e.g. in a detached Timeline at its minimum width.
pub(crate) const LAST_RESORT: u8 = 100;

/// Bar order and hide priority (owner-approved, T-101 / R-052): when the bar is too narrow the lowest
/// priority hides first (the rightmost on ties), then smaller hidden controls that still fit come back.
/// Hidden controls are listed at the top of the More menu; nothing is clipped or scrolled away.
pub(crate) const CONTROLS: [(Ctl, u8); 28] = [
    (Ctl::AddLayer, ESSENTIAL),
    (Ctl::Folder, 4),
    (Ctl::Guide, 3),
    (Ctl::Delete, LAST_RESORT),
    (Ctl::Div1, LAST_RESORT),
    (Ctl::Transport, ESSENTIAL),
    (Ctl::Loop, 6),
    (Ctl::Frame, LAST_RESORT),
    (Ctl::Timecode, 5),
    (Ctl::Div2, LAST_RESORT),
    (Ctl::Scene, 2),
    (Ctl::Fps, 3),
    (Ctl::Duration, 1),
    (Ctl::Insert, 8),
    (Ctl::Tween, 7),
    (Ctl::Ease, 4),
    (Ctl::Sound, 4),
    (Ctl::Flex, ESSENTIAL),
    (Ctl::Onion, 9),
    (Ctl::OnionOutline, 5),
    (Ctl::EditMulti, 5),
    (Ctl::OnionOptions, 5),
    (Ctl::Div3, LAST_RESORT),
    (Ctl::Centre, 1),
    (Ctl::ZoomOut, 6),
    (Ctl::Slider, 0),
    (Ctl::ZoomIn, 6),
    (Ctl::More, ESSENTIAL),
];

const ONION_OPTIONS_POPUP: &str = "timeline-onion-options-popup";

/// More ▸ Hidden controls lists rows flat up to this count, otherwise one submenu per group.
const HIDDEN_FLAT_ROWS: usize = 8;

/// Submenus of More ▸ Hidden controls when many controls are hidden.
const HIDDEN_GROUPS: [(&str, &[Ctl]); 5] = [
    ("Layers", &[Ctl::Folder, Ctl::Guide, Ctl::Delete]),
    (
        "Playback",
        &[
            Ctl::Loop,
            Ctl::Frame,
            Ctl::Timecode,
            Ctl::Scene,
            Ctl::Fps,
            Ctl::Duration,
        ],
    ),
    (
        "Frames and tweens",
        &[Ctl::Insert, Ctl::Tween, Ctl::Ease, Ctl::Sound],
    ),
    (
        "Onion skin",
        &[
            Ctl::Onion,
            Ctl::OnionOutline,
            Ctl::EditMulti,
            Ctl::OnionOptions,
        ],
    ),
    ("View", &[Ctl::Centre, Ctl::ZoomOut, Ctl::ZoomIn]),
];

/// Menu rows a hidden control takes in More ▸ Hidden controls.
fn hidden_menu_rows(c: Ctl) -> usize {
    match c {
        Ctl::Insert | Ctl::Tween => 3,
        Ctl::Slider | Ctl::Flex | Ctl::Div1 | Ctl::Div2 | Ctl::Div3 => 0,
        _ => 1,
    }
}

/// Width before the first measurement (later frames use the measured width).
fn estimated_width(c: Ctl) -> f32 {
    match c {
        Ctl::Div1 | Ctl::Div2 | Ctl::Div3 => 1. + space::S2,
        Ctl::Transport => 5. * neoflash_ui::metrics::control() + 4. * GROUP_GAP,
        Ctl::Frame => 72.,
        Ctl::Timecode => 60.,
        Ctl::Scene => 120.,
        Ctl::Fps => 92.,
        Ctl::Duration => 140.,
        Ctl::Insert | Ctl::Tween => 3. * 28. + 6.,
        Ctl::Slider => SLIDER,
        Ctl::Flex => 0.,
        _ => neoflash_ui::metrics::control(),
    }
}

/// Controls of one visual group sit 2 pt apart; other neighbours 8 pt.
fn same_group(a: Ctl, b: Ctl) -> bool {
    let group = |c| match c {
        Ctl::AddLayer | Ctl::Folder | Ctl::Guide | Ctl::Delete => 1,
        Ctl::Onion | Ctl::OnionOutline | Ctl::EditMulti | Ctl::OnionOptions => 2,
        Ctl::ZoomOut | Ctl::Slider | Ctl::ZoomIn => 3,
        _ => 0,
    };
    group(a) != 0 && group(a) == group(b)
}

/// Total width of a row of controls including the gaps between them.
pub(crate) fn controls_width(row: &[(Ctl, f32)]) -> f32 {
    let mut width = 0.;
    for (i, (c, w)) in row.iter().enumerate() {
        width += w;
        if i > 0 && *c != Ctl::Flex {
            width += if same_group(row[i - 1].0, *c) {
                GROUP_GAP
            } else {
                GAP
            };
        }
    }
    width
}

/// Visible controls, in bar order, for `available` points (see [`CONTROLS`]).
pub(crate) fn fit_controls(items: &[(Ctl, u8, f32)], available: f32) -> Vec<Ctl> {
    let width = |visible: &[usize]| {
        controls_width(
            &visible
                .iter()
                .map(|&i| (items[i].0, items[i].2))
                .collect::<Vec<_>>(),
        )
    };
    let mut visible: Vec<usize> = (0..items.len()).collect();
    let mut hidden = Vec::new();
    while width(&visible) > available {
        let Some(k) = visible
            .iter()
            .enumerate()
            .filter(|(_, i)| items[**i].1 != ESSENTIAL)
            .min_by_key(|(_, i)| (items[**i].1, std::cmp::Reverse(**i)))
            .map(|(k, _)| k)
        else {
            break;
        };
        hidden.push(visible.remove(k));
    }
    // Give back: highest priority first (leftmost on ties), whatever still fits.
    hidden.sort_by_key(|&i| std::cmp::Reverse((items[i].1, std::cmp::Reverse(i))));
    for i in hidden {
        let mut trial = visible.clone();
        trial.insert(trial.partition_point(|&j| j < i), i);
        if width(&trial) <= available {
            visible = trial;
        }
    }
    // A divider only separates two visible controls on the same side of the flexible gap.
    let is_div = |c: Ctl| matches!(c, Ctl::Div1 | Ctl::Div2 | Ctl::Div3);
    let row: Vec<Ctl> = visible.into_iter().map(|i| items[i].0).collect();
    row.iter()
        .enumerate()
        .filter(|&(k, &c)| {
            !is_div(c)
                || (k > 0
                    && k + 1 < row.len()
                    && !is_div(row[k - 1])
                    && !is_div(row[k + 1])
                    && row[k - 1] != Ctl::Flex
                    && row[k + 1] != Ctl::Flex)
        })
        .map(|(_, &c)| c)
        .collect()
}

impl Editor {
    pub(crate) fn timeline_controls_bar(
        &mut self,
        ui: &mut egui::Ui,
        layer_column: f32,
        frames_width: f32,
    ) -> ControlsOutcome {
        let p = neoflash_ui::palette(ui.ctx());
        let width = ui.available_width();
        let (bar, _) = ui.allocate_exact_size(
            Vec2::new(width, neoflash_ui::metrics::timeline_controls()),
            Sense::hover(),
        );
        ui.painter().hline(
            bar.x_range(),
            bar.bottom() - 0.5,
            Stroke::new(1.0_f32, p.border_subtle),
        );
        let mut out = ControlsOutcome::default();
        let button = self.symbol().kind == SymbolKind::Button;
        let widths_id = egui::Id::new("timeline-controls-widths");
        let mut measured: HashMap<Ctl, f32> =
            ui.data(|d| d.get_temp(widths_id)).unwrap_or_default();
        let items: Vec<(Ctl, u8, f32)> = CONTROLS
            .iter()
            .filter(|(c, _)| !(button && matches!(c, Ctl::Frame | Ctl::Timecode | Ctl::Duration)))
            .map(|&(c, priority)| {
                let w = measured
                    .get(&c)
                    .copied()
                    .unwrap_or_else(|| estimated_width(c));
                (c, priority, w)
            })
            .collect();
        let inner = bar.shrink2(Vec2::new(GAP, 0.));
        let visible = fit_controls(&items, inner.width());
        let hidden: Vec<Ctl> = items
            .iter()
            .map(|i| i.0)
            .filter(|c| !visible.contains(c))
            .collect();
        let row: Vec<(Ctl, f32)> = items
            .iter()
            .filter(|i| visible.contains(&i.0))
            .map(|i| (i.0, i.2))
            .collect();
        let used = controls_width(&row);
        // Adaptive alignment: the transport starts above frame 1 only when everything fits with it.
        let compact_actions = 4. * neoflash_ui::metrics::control() + 3. * GROUP_GAP;
        let align = (layer_column - 2. * GAP - compact_actions).max(0.);
        let align = if hidden.is_empty() && used + align <= inner.width() {
            align
        } else {
            0.
        };
        let flex = (inner.width() - used - align).max(0.);
        let mut remeasure = false;
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                ui.set_clip_rect(bar.intersect(ui.clip_rect()));
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                let mut prev = None;
                for c in visible.iter().copied() {
                    if let Some(prev) = prev
                        && c != Ctl::Flex
                    {
                        ui.add_space(if same_group(prev, c) { GROUP_GAP } else { GAP });
                    }
                    prev = Some(c);
                    if c == Ctl::Flex {
                        ui.add_space(flex);
                        continue;
                    }
                    if c == Ctl::Delete {
                        ui.add_space(align);
                    }
                    let x0 = ui.cursor().left();
                    self.timeline_control(ui, c, &hidden, frames_width, &mut out);
                    let w = ui.cursor().left() - x0;
                    if w > 0. && measured.get(&c).is_none_or(|old| (old - w).abs() > 0.5) {
                        measured.insert(c, w);
                        remeasure = true;
                    }
                }
            },
        );
        ui.data_mut(|d| d.insert_temp(widths_id, measured));
        if remeasure {
            ui.ctx().request_repaint();
        }
        out
    }

    /// One control of the bar (see [`CONTROLS`]).
    fn timeline_control(
        &mut self,
        ui: &mut egui::Ui,
        c: Ctl,
        hidden: &[Ctl],
        frames_width: f32,
        out: &mut ControlsOutcome,
    ) {
        let p = neoflash_ui::palette(ui.ctx());
        match c {
            Ctl::AddLayer => {
                if ui
                    .add(IconButton::new(i16::LAYERPLUS, "Add layer"))
                    .clicked()
                {
                    self.insert_timeline_layer();
                }
            }
            Ctl::Folder => {
                if ui
                    .add(IconButton::new(i16::FOLDERPLUS, "New folder"))
                    .clicked()
                {
                    self.insert_layer_folder(self.layer);
                }
            }
            Ctl::Guide => {
                if ui
                    .add_enabled(
                        self.can_add_motion_guide_to(self.layer),
                        IconButton::new(i16::GUIDEPLUS, "Add motion guide"),
                    )
                    .on_disabled_hover_text("Select a normal layer to add a motion guide")
                    .clicked()
                {
                    self.add_motion_guide();
                }
            }
            Ctl::Delete => {
                if ui
                    .add_enabled(
                        self.symbol().timeline.layers.len() > 1,
                        IconButton::new(i16::TRASH, "Delete selected layer"),
                    )
                    .clicked()
                {
                    self.delete_timeline_layer(self.layer);
                }
            }
            Ctl::Div1 | Ctl::Div2 | Ctl::Div3 => {
                nf::divider(ui, true);
            }
            Ctl::Transport => self.timeline_transport(ui),
            Ctl::Loop => {
                let looping = self.workspace.timeline.loop_playback;
                if ui
                    .add(IconButton::new(i16::LOOP, "Loop playback").selected(looping))
                    .clicked()
                {
                    self.workspace.timeline.loop_playback = !looping;
                }
            }
            Ctl::Frame => {
                let frame = self.frame;
                let count = self.symbol().timeline.frame_count;
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = space::S1;
                    ui.label(
                        typography::rich(format!("{}", frame + 1), TextStyle::Mono)
                            .color(p.text_primary),
                    );
                    ui.label(
                        typography::rich(format!("/ {count}"), TextStyle::MonoSmall)
                            .color(p.text_tertiary),
                    );
                })
                .response
                .on_hover_text("Current frame / frame count");
            }
            Ctl::Timecode => {
                ui.label(
                    typography::rich(timecode(self.frame, self.project.fps), TextStyle::MonoSmall)
                        .color(p.text_secondary),
                )
                .on_hover_text("Current time");
            }
            Ctl::Scene => self.timeline_scene_select(ui),
            Ctl::Fps => self.timeline_timing_fields(ui, true, false),
            Ctl::Duration => self.timeline_timing_fields(ui, false, true),
            Ctl::Insert => self.timeline_insert_group(ui),
            Ctl::Tween => self.timeline_tween_group(ui),
            Ctl::Ease => {
                // Ease opens the Ease panel (§6) on the tween under the playhead.
                if ui
                    .add_enabled(
                        self.tween_span().is_some(),
                        IconButton::new(i16::EASE, "Ease"),
                    )
                    .clicked()
                {
                    self.show_panel(PanelId::Ease);
                }
            }
            Ctl::Sound => self.timeline_sound_button(ui),
            Ctl::Onion => {
                if ui
                    .add(
                        IconButton::new(i16::ONION, "Onion skin")
                            .selected(self.onion && !self.onion_outlines),
                    )
                    .clicked()
                {
                    self.toggle_onion();
                }
            }
            Ctl::OnionOutline => {
                if ui
                    .add(
                        IconButton::new(i16::ONIONOUTLINE, "Onion skin outlines")
                            .selected(self.onion && self.onion_outlines),
                    )
                    .clicked()
                {
                    self.toggle_onion_outlines();
                }
            }
            Ctl::EditMulti => {
                let multiple = self.timeline_edit.edit_multiple;
                if ui
                    .add(IconButton::new(i16::EDITMULTI, "Edit multiple frames").selected(multiple))
                    .clicked()
                {
                    self.timeline_edit.edit_multiple = !multiple;
                }
            }
            Ctl::OnionOptions => {
                let popup_id = egui::Id::new(ONION_OPTIONS_POPUP);
                let open = egui::Popup::is_id_open(ui.ctx(), popup_id);
                let options =
                    ui.add(IconButton::new(i16::MARKERS, "Onion skin options").selected(open));
                egui::Popup::menu(&options)
                    .id(popup_id)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .gap(space::S1)
                    .frame(nf::popover_frame(ui.ctx()))
                    .show(|ui| self.onion_options(ui));
            }
            Ctl::Centre => {
                if ui
                    .add(IconButton::new(i16::CENTERFRAME, "Center playhead"))
                    .clicked()
                {
                    out.recenter = true;
                }
            }
            Ctl::ZoomOut => {
                if ui
                    .add(IconButton::new(i16::MINUS, "Show more frames"))
                    .clicked()
                {
                    self.timeline_zoom_step(false);
                }
            }
            Ctl::Slider => {
                let mut zoom = (self.timeline_cell as f64).ln();
                if ui
                    .add(
                        nf::Slider::new(
                            &mut zoom,
                            (MIN_CELL as f64).ln()..=(MAX_CELL as f64).ln(),
                            "Timeline zoom",
                        )
                        .step(0.01)
                        .width(SLIDER),
                    )
                    .on_hover_text(
                        "Change the width of timeline cells without changing the animation",
                    )
                    .changed()
                {
                    self.timeline_cell = (zoom.exp() as f32).clamp(MIN_CELL, MAX_CELL);
                }
            }
            Ctl::ZoomIn => {
                if ui
                    .add(IconButton::new(i16::PLUS, "Widen frame cells"))
                    .clicked()
                {
                    self.timeline_zoom_step(true);
                }
            }
            Ctl::More => {
                let shown: Vec<Ctl> = hidden
                    .iter()
                    .copied()
                    .filter(|c| !matches!(c, Ctl::Slider))
                    .collect();
                let (kind, label) = if shown.is_empty() {
                    (IconButtonType::Ghost, "Frame view options")
                } else {
                    (
                        IconButtonType::Selected,
                        "Hidden controls and frame view options",
                    )
                };
                let more = ui.add(IconButton::new(i16::MORE, label).kind(kind));
                if shown.contains(&Ctl::OnionOptions) {
                    // Opened from More ▸ Onion skin options…: the same popover, anchored here.
                    egui::Popup::menu(&more)
                        .id(egui::Id::new(ONION_OPTIONS_POPUP))
                        .open_memory(None)
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                        .gap(space::S1)
                        .frame(nf::popover_frame(ui.ctx()))
                        .show(|ui| self.onion_options(ui));
                }
                nf::popup_below(&more, 260., |ui| {
                    if !shown.is_empty() {
                        nf::menu_section(ui, "Hidden controls");
                        let rows: usize = shown.iter().map(|c| hidden_menu_rows(*c)).sum();
                        if rows <= HIDDEN_FLAT_ROWS {
                            for c in shown {
                                self.timeline_hidden_control(ui, c, out);
                            }
                        } else {
                            // Many hidden controls (narrow detached Timeline): one submenu per group.
                            for (group, members) in HIDDEN_GROUPS {
                                let in_group: Vec<Ctl> = shown
                                    .iter()
                                    .copied()
                                    .filter(|c| members.contains(c))
                                    .collect();
                                if !in_group.is_empty() {
                                    nf::submenu(ui, group, |ui| {
                                        for c in in_group {
                                            self.timeline_hidden_control(ui, c, out);
                                        }
                                    });
                                }
                            }
                        }
                        nf::menu_separator(ui);
                    }
                    self.timeline_frame_view_menu(ui, frames_width, out);
                });
            }
            Ctl::Flex => {}
        }
    }

    /// Menu entry of a control the bar had to hide (More menu ▸ Hidden controls).
    fn timeline_hidden_control(&mut self, ui: &mut egui::Ui, c: Ctl, out: &mut ControlsOutcome) {
        let editable = self.layer_editable();
        match c {
            Ctl::Folder => {
                if ui
                    .add(MenuItem::new("New folder").icon(i16::FOLDERPLUS))
                    .clicked()
                {
                    self.insert_layer_folder(self.layer);
                    ui.close();
                }
            }
            Ctl::Guide => {
                if ui
                    .add_enabled(
                        self.can_add_motion_guide_to(self.layer),
                        MenuItem::new("Add motion guide").icon(i16::GUIDEPLUS),
                    )
                    .clicked()
                {
                    self.add_motion_guide();
                    ui.close();
                }
            }
            Ctl::Loop => {
                let looping = self.workspace.timeline.loop_playback;
                if ui
                    .add(
                        MenuItem::new("Loop playback")
                            .icon(i16::LOOP)
                            .checked(looping),
                    )
                    .clicked()
                {
                    self.workspace.timeline.loop_playback = !looping;
                }
            }
            Ctl::Timecode => {
                let time = format!("Time {}", timecode(self.frame, self.project.fps));
                ui.add_enabled(false, MenuItem::new(&time).icon(i16::SPEED));
            }
            Ctl::Scene => {
                nf::submenu(ui, "Scene", |ui| self.scene_menu_items(ui));
            }
            Ctl::Fps | Ctl::Duration => {
                ui.horizontal(|ui| {
                    ui.add_space(space::S2);
                    ui.spacing_mut().item_spacing.x = GAP;
                    self.timeline_timing_fields(ui, c == Ctl::Fps, c == Ctl::Duration);
                });
            }
            Ctl::Insert => {
                for (icon, label, key, blank) in [
                    (i16::FRAME_ADD, "Insert frame", "F5", None),
                    (i16::KEYFRAME_ADD, "Insert keyframe", "F6", Some(false)),
                    (i16::BLANKKEY_ADD, "Insert blank keyframe", "F7", Some(true)),
                ] {
                    if ui
                        .add_enabled(editable, MenuItem::new(label).icon(icon).shortcut(key))
                        .clicked()
                    {
                        match blank {
                            None => self.insert_frame(),
                            Some(blank) => self.make_key(blank),
                        }
                        ui.close();
                    }
                }
            }
            Ctl::Tween => {
                let tween = self.current_tween();
                let can_tween = self.can_create_tween();
                for (icon, label, kind) in [
                    (i16::TWEEN_CLASSIC, "Classic tween", TweenKind::Classic),
                    (i16::TWEEN_SHAPE, "Shape tween", TweenKind::Shape),
                    (i16::TWEEN_MOTION, "Motion tween", TweenKind::Motion),
                ] {
                    if ui
                        .add_enabled(
                            can_tween,
                            MenuItem::new(label).icon(icon).checked(tween == Some(kind)),
                        )
                        .clicked()
                    {
                        self.toggle_tween(kind);
                        ui.close();
                    }
                }
            }
            Ctl::Ease => {
                if ui
                    .add_enabled(
                        self.tween_span().is_some(),
                        MenuItem::new("Ease…").icon(i16::EASE),
                    )
                    .clicked()
                {
                    self.show_panel(PanelId::Ease);
                    ui.close();
                }
            }
            Ctl::Sound => {
                nf::submenu(ui, "Sound", |ui| self.sound_menu_items(ui));
            }
            Ctl::Onion => {
                if ui
                    .add(
                        MenuItem::new("Onion skin")
                            .icon(i16::ONION)
                            .checked(self.onion && !self.onion_outlines),
                    )
                    .clicked()
                {
                    self.toggle_onion();
                }
            }
            Ctl::OnionOutline => {
                if ui
                    .add(
                        MenuItem::new("Onion skin outlines")
                            .icon(i16::ONIONOUTLINE)
                            .checked(self.onion && self.onion_outlines),
                    )
                    .clicked()
                {
                    self.toggle_onion_outlines();
                }
            }
            Ctl::EditMulti => {
                let multiple = self.timeline_edit.edit_multiple;
                if ui
                    .add(
                        MenuItem::new("Edit multiple frames")
                            .icon(i16::EDITMULTI)
                            .checked(multiple),
                    )
                    .clicked()
                {
                    self.timeline_edit.edit_multiple = !multiple;
                }
            }
            Ctl::OnionOptions => {
                if ui
                    .add(MenuItem::new("Onion skin options…").icon(i16::MARKERS))
                    .clicked()
                {
                    ui.close();
                    egui::Popup::open_id(ui.ctx(), egui::Id::new(ONION_OPTIONS_POPUP));
                }
            }
            Ctl::Delete => {
                if ui
                    .add_enabled(
                        self.symbol().timeline.layers.len() > 1,
                        MenuItem::new("Delete selected layer").icon(i16::TRASH),
                    )
                    .clicked()
                {
                    self.delete_timeline_layer(self.layer);
                    ui.close();
                }
            }
            Ctl::Frame => {
                let frame = format!(
                    "Frame {} / {}",
                    self.frame + 1,
                    self.symbol().timeline.frame_count
                );
                ui.add_enabled(false, MenuItem::new(&frame).icon(i16::FRAME));
            }
            Ctl::Centre => {
                if ui
                    .add(MenuItem::new("Center playhead").icon(i16::CENTERFRAME))
                    .clicked()
                {
                    out.recenter = true;
                    ui.close();
                }
            }
            Ctl::ZoomOut | Ctl::ZoomIn => {
                let wider = c == Ctl::ZoomIn;
                let (label, icon) = if wider {
                    ("Widen frame cells", i16::PLUS)
                } else {
                    ("Show more frames", i16::MINUS)
                };
                if ui.add(MenuItem::new(label).icon(icon)).clicked() {
                    self.timeline_zoom_step(wider);
                }
            }
            _ => {}
        }
    }

    fn timeline_zoom_step(&mut self, wider: bool) {
        self.timeline_cell = if wider {
            (self.timeline_cell * 1.4).min(MAX_CELL)
        } else {
            (self.timeline_cell / 1.4).max(MIN_CELL)
        };
    }

    fn toggle_onion(&mut self) {
        if !self.onion {
            self.onion = true;
            self.onion_outlines = false;
        } else if self.onion_outlines {
            self.onion_outlines = false;
        } else {
            self.onion = false;
        }
    }

    fn toggle_onion_outlines(&mut self) {
        if !self.onion {
            self.onion = true;
            self.onion_outlines = true;
        } else if !self.onion_outlines {
            self.onion_outlines = true;
        } else {
            self.onion = false;
            self.onion_outlines = false;
        }
    }

    /// Fit frames `first..=last` (0-based) into the visible frame area and scroll to `first`.
    fn timeline_fit_frames(
        &mut self,
        first: u32,
        last: u32,
        frames_width: f32,
        out: &mut ControlsOutcome,
    ) {
        let count = last.saturating_sub(first) + 1;
        self.timeline_cell = (frames_width.max(1.) / count as f32).clamp(MIN_CELL, MAX_CELL);
        out.scroll_to = Some(first);
    }

    /// First and last frame (0-based) that hold content or a frame sound on any layer; the whole
    /// timeline when it is empty.
    pub(crate) fn content_frame_range(&self) -> (u32, u32) {
        let timeline = &self.symbol().timeline;
        let end = timeline.frame_count.saturating_sub(1);
        let mut range: Option<(u32, u32)> = None;
        for layer in &timeline.layers {
            for (i, key) in layer.keyframes.iter().enumerate() {
                if key.elements.is_empty() && key.sound.is_none() {
                    continue;
                }
                let last = layer
                    .keyframes
                    .get(i + 1)
                    .map_or(end, |next| next.frame.saturating_sub(1))
                    .min(end);
                range =
                    Some(range.map_or((key.frame, last), |(a, b)| (a.min(key.frame), b.max(last))));
            }
        }
        match range {
            Some((a, b)) if a <= b => (a, b),
            _ => (0, end),
        }
    }

    fn timeline_seek(&mut self, frame: u32) {
        self.finish_edits();
        self.timeline_key_drag = None;
        self.reset_authored_preview();
        self.frame = frame.min(self.symbol().timeline.frame_count.saturating_sub(1));
    }

    /// Transport group: first, step back, Play (Primary SM, square), step forward, last — 2 pt apart.
    fn timeline_transport(&mut self, ui: &mut egui::Ui) {
        use crate::commands::{CommandId as C, transport_step_label};
        let last = self.symbol().timeline.frame_count.saturating_sub(1);
        let show_sc = self.workspace.general.tooltip_shortcuts;
        let prev_sc = show_sc
            .then(|| transport_step_label(&self.workspace.shortcuts, C::ControlStepBack))
            .filter(|s| !s.is_empty());
        let play_sc = self.tooltip_sc(C::ControlPlay);
        let next_sc = show_sc
            .then(|| transport_step_label(&self.workspace.shortcuts, C::ControlStepForward))
            .filter(|s| !s.is_empty());

        if ui.add(IconButton::new(i16::FIRST, "Rewind")).clicked() {
            self.timeline_seek(0);
        }
        ui.add_space(GROUP_GAP);
        let mut prev_btn = IconButton::new(i16::STEPBACK, "Previous frame");
        if let Some(sc) = prev_sc.as_deref() {
            prev_btn = prev_btn.shortcut(sc);
        }
        if ui.add(prev_btn).clicked() {
            self.timeline_seek(self.frame.saturating_sub(1));
        }
        ui.add_space(GROUP_GAP);
        let (icon, label) = if self.playing {
            (i16::PAUSE, "Pause preview")
        } else {
            (i16::PLAY, "Play preview")
        };
        let mut play_btn = IconButton::new(icon, label).kind(IconButtonType::Primary);
        if let Some(sc) = play_sc.as_deref() {
            play_btn = play_btn.shortcut(sc);
        }
        if ui.add(play_btn).clicked() {
            self.set_playing(!self.playing);
        }
        ui.add_space(GROUP_GAP);
        let mut next_btn = IconButton::new(i16::STEPFWD, "Next frame");
        if let Some(sc) = next_sc.as_deref() {
            next_btn = next_btn.shortcut(sc);
        }
        if ui.add(next_btn).clicked() {
            self.timeline_seek(self.frame + 1);
        }
        ui.add_space(GROUP_GAP);
        if ui
            .add(IconButton::new(i16::LAST, "Go to last frame"))
            .clicked()
        {
            self.timeline_seek(last);
        }
    }

    /// Scene `Select`: the document's scenes in play order (checked = edited scene) + New scene.
    /// Choosing a scene leaves symbol editing and shows that scene's timeline.
    fn timeline_scene_select(&mut self, ui: &mut egui::Ui) {
        let scenes = self.scene_list();
        let current = self.current_scene().to_owned();
        let name = scenes
            .iter()
            .find(|(id, _)| id == &current)
            .map(|(_, name)| name.clone())
            .unwrap_or_default();
        let (response, _) = nf::Select::new("timeline-scene", &name)
            .width(120.)
            .show(ui, |ui| self.scene_menu_items(ui));
        response.on_hover_text("Scene");
    }

    /// Scene menu rows (Scene select and More ▸ Hidden controls ▸ Scene).
    fn scene_menu_items(&mut self, ui: &mut egui::Ui) {
        let scenes = self.scene_list();
        let current = self.current_scene().to_owned();
        let at_scene = self.editing.len() <= 1;
        let mut chosen = None;
        let mut create = false;
        for (id, scene) in &scenes {
            if ui
                .add(MenuItem::new(scene).checked(at_scene && id == &current))
                .clicked()
            {
                chosen = Some(id.clone());
            }
        }
        nf::menu_separator(ui);
        if ui.add(MenuItem::new("New scene")).clicked() {
            create = true;
        }
        if create {
            self.new_scene();
            ui.close();
        } else if let Some(id) = chosen {
            if id != current || !at_scene {
                self.switch_scene(&id);
            }
            ui.close();
        }
    }

    /// Frame insert group (F5 / F6 / F7).
    fn timeline_insert_group(&mut self, ui: &mut egui::Ui) {
        let editable = self.layer_editable();
        let inserted = IconGroup {
            items: &[
                (i16::FRAME_ADD, "Insert frame"),
                (i16::KEYFRAME_ADD, "Insert keyframe"),
                (i16::BLANKKEY_ADD, "Insert blank keyframe"),
            ],
            selected: None,
        }
        .show_with(ui, &[editable; 3], &[Some("F5"), Some("F6"), Some("F7")]);
        match inserted {
            Some(0) => self.insert_frame(),
            Some(1) => self.make_key(false),
            Some(2) => self.make_key(true),
            _ => {}
        }
    }

    /// Tween group: classic, shape, motion (the current kind is selected; clicking it removes it).
    fn timeline_tween_group(&mut self, ui: &mut egui::Ui) {
        let tween = self.current_tween();
        let classic = if tween == Some(TweenKind::Classic) {
            "Remove classic tween"
        } else {
            "Create classic tween"
        };
        let shape = if tween == Some(TweenKind::Shape) {
            "Remove shape tween"
        } else {
            "Shape tween"
        };
        let motion = if tween == Some(TweenKind::Motion) {
            "Remove motion tween"
        } else {
            "Motion tween"
        };
        let can_tween = self.can_create_tween();
        let clicked = IconGroup {
            items: &[
                (i16::TWEEN_CLASSIC, classic),
                (i16::TWEEN_SHAPE, shape),
                (i16::TWEEN_MOTION, motion),
            ],
            selected: tween.map(|kind| match kind {
                TweenKind::Classic => 0,
                TweenKind::Shape => 1,
                TweenKind::Motion => 2,
            }),
        }
        .show_with(ui, &[can_tween; 3], &[]);
        match clicked {
            Some(0) => self.toggle_tween(TweenKind::Classic),
            Some(1) => self.toggle_tween(TweenKind::Shape),
            Some(2) => self.toggle_tween(TweenKind::Motion),
            _ => {}
        }
    }

    /// Sound (TL-07): library sounds and "Import sound…" for the current keyframe.
    fn timeline_sound_button(&mut self, ui: &mut egui::Ui) {
        let button = ui.add(IconButton::new(i16::SOUND, "Sound"));
        nf::popup_below(&button, 220., |ui| self.sound_menu_items(ui));
    }

    /// Sound menu rows (Sound button and More ▸ Hidden controls ▸ Sound).
    fn sound_menu_items(&mut self, ui: &mut egui::Ui) {
        let current = self.frame_sound().map(|(_, sound)| sound.resource);
        let editable = self.layer_editable();
        let sounds = self.audio_resources();
        let mut chosen = None;
        let mut import = false;
        for (id, path) in &sounds {
            let item = MenuItem::new(path).checked(current.as_ref() == Some(id));
            if ui.add_enabled(editable, item).clicked() {
                chosen = Some(id.clone());
            }
        }
        if !sounds.is_empty() {
            nf::menu_separator(ui);
        }
        if ui
            .add_enabled(editable, MenuItem::new("Import sound…"))
            .clicked()
        {
            import = true;
        }
        if let Some(id) = chosen {
            self.set_frame_sound(Some(id));
            ui.close();
        } else if import {
            self.import_frame_sound_dialog();
            ui.close();
        }
    }

    /// New layer above the selection (layer actions, Insert ▸ Layer).
    pub(crate) fn insert_timeline_layer(&mut self) {
        self.finish_edits();
        self.transact(|s| {
            let n = s.symbol().timeline.layers.len() + 1;
            s.symbol_mut().timeline.layers.push(Layer {
                properties: Default::default(),
                id: new_id("layer"),
                name: format!("Layer {n}"),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![],
                    tween: None,
                    label: String::new(),
                    ..Default::default()
                }],
            });
            s.layer = s.symbol().timeline.layers.len() - 1;
        });
    }

    /// Onion-skin popover: outlines, anchor, before / after range and opacity.
    fn onion_options(&mut self, ui: &mut egui::Ui) {
        let p = neoflash_ui::palette(ui.ctx());
        egui::Frame::new()
            .inner_margin(egui::Margin::same(space::S2 as i8))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(space::S3, space::S2);
                ui.set_width(232.);
                nf::menu_section(ui, "Onion skin");
                let last = self.symbol().timeline.frame_count.saturating_sub(1);
                ui.horizontal(|ui| {
                    if ui
                        .add(nf::Checkbox::new(&mut self.onion_outlines, "Outlines"))
                        .changed()
                        && self.onion_outlines
                    {
                        self.onion = true;
                    }
                    if ui
                        .add(nf::Checkbox::new(&mut self.onion_anchor, "Anchor"))
                        .changed()
                    {
                        if self.onion_anchor {
                            self.anchored_range = (
                                self.frame.saturating_sub(self.onion_before),
                                (self.frame + self.onion_after).min(last),
                            );
                        } else {
                            self.onion_before =
                                self.frame.saturating_sub(self.anchored_range.0.min(last));
                            self.onion_after =
                                self.anchored_range.1.min(last).saturating_sub(self.frame);
                            self.workspace.onion_range = [self.onion_before, self.onion_after];
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = space::S2;
                    let mut before = if self.onion_anchor {
                        self.frame.saturating_sub(self.anchored_range.0.min(last))
                    } else {
                        self.onion_before
                    } as f64;
                    if ui
                        .add(
                            nf::Field::new(&mut before, "Onion frames before")
                                .prefix("Before")
                                .unit("fr")
                                .range(0.0..=120.0)
                                .width(112.),
                        )
                        .changed()
                    {
                        let v = before.round().max(0.0) as u32;
                        self.onion_before = v;
                        self.workspace.onion_range = [self.onion_before, self.onion_after];
                        if self.onion_anchor {
                            self.anchored_range.0 = self
                                .frame
                                .saturating_sub(v)
                                .min(self.anchored_range.1.min(last));
                        }
                    }
                    let mut after = if self.onion_anchor {
                        self.anchored_range.1.min(last).saturating_sub(self.frame)
                    } else {
                        self.onion_after
                    } as f64;
                    if ui
                        .add(
                            nf::Field::new(&mut after, "Onion frames after")
                                .prefix("After")
                                .unit("fr")
                                .range(0.0..=120.0)
                                .width(112.),
                        )
                        .changed()
                    {
                        let v = after.round().max(0.0) as u32;
                        self.onion_after = v;
                        self.workspace.onion_range = [self.onion_before, self.onion_after];
                        if self.onion_anchor {
                            self.anchored_range.1 = (self.frame + v)
                                .min(last)
                                .max(self.anchored_range.0.min(last));
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(typography::rich("Opacity", TextStyle::Small).color(p.text_secondary));
                    let mut opacity = f64::from(self.workspace.timeline_prefs.onion_falloff);
                    if ui
                        .add(
                            nf::Slider::new(&mut opacity, 15.0..=100.0, "Onion opacity")
                                .width(SLIDER),
                        )
                        .changed()
                    {
                        self.workspace.timeline_prefs.onion_falloff = opacity as f32;
                    }
                });
            });
    }

    /// Frame view ⋯ (§5.4): placement, frame sizes, fit, tinted frames, seconds on the ruler, help, close.
    fn timeline_frame_view_menu(
        &mut self,
        ui: &mut egui::Ui,
        frames_width: f32,
        out: &mut ControlsOutcome,
    ) {
        let ctx = ui.ctx().clone();
        crate::ui_icons::menu_popup(ui, "Placement", "Floating window", "", |ui| {
            let docked = self.workspace.placement(PanelId::Timeline) == PanelPlacement::Docked;
            let edge = self
                .workspace
                .dock
                .location(PanelId::Timeline)
                .map(|location| location.edge);
            for (label, target) in [
                ("Bottom (docked)", Some(DockEdge::Bottom)),
                ("Top", Some(DockEdge::Top)),
                ("Left", Some(DockEdge::Left)),
                ("Right", Some(DockEdge::Right)),
                ("Floating window", None),
            ] {
                let current = match target {
                    Some(target) => docked && edge == Some(target),
                    None => !docked,
                };
                if ui.add(MenuItem::new(label).checked(current)).clicked() && !current {
                    match target {
                        Some(target) => {
                            if !docked {
                                self.set_panel_placement(
                                    &ctx,
                                    PanelId::Timeline,
                                    PanelPlacement::Docked,
                                );
                            }
                            self.dock_panel_at(
                                &ctx,
                                PanelId::Timeline,
                                DockDestination::Edge(target),
                            );
                        }
                        None => self.set_panel_placement(
                            &ctx,
                            PanelId::Timeline,
                            PanelPlacement::Detached,
                        ),
                    }
                    ui.close();
                }
            }
        });
        nf::menu_separator(ui);
        for view in FrameView::ALL {
            let current = (self.timeline_cell - view.frame_width()).abs() < 0.01;
            if ui
                .add(MenuItem::new(view.label()).checked(current))
                .clicked()
            {
                self.timeline_cell = view.frame_width();
            }
        }
        if ui.add(MenuItem::new("Fit whole timeline")).clicked() {
            self.timeline_cell = (frames_width.max(1.)
                / self.symbol().timeline.frame_count.max(1) as f32)
                .clamp(MIN_CELL, MAX_CELL);
        }
        let selection = self.selected_frame_range();
        if ui
            .add_enabled(selection.is_some(), MenuItem::new("Fit selection"))
            .on_disabled_hover_text("Select a frame range first")
            .clicked()
            && let Some((first, last, _)) = selection
        {
            self.timeline_fit_frames(first, last, frames_width, out);
            ui.close();
        }
        if ui.add(MenuItem::new("Fit content")).clicked() {
            let (first, last) = self.content_frame_range();
            self.timeline_fit_frames(first, last, frames_width, out);
            ui.close();
        }
        let view = &mut self.workspace.timeline;
        for (label, mode) in [
            ("Preview", FramePreview::Layer),
            ("Preview in context", FramePreview::InContext),
        ] {
            if ui
                .add(MenuItem::new(label).checked(view.preview == mode))
                .clicked()
            {
                if view.preview == mode {
                    view.preview = FramePreview::Off;
                } else {
                    view.preview = mode;
                    view.short_rows = false;
                    view.compact_rows = false;
                }
            }
        }
        let short_active =
            view.preview == FramePreview::Off && view.short_rows && !view.compact_rows;
        if ui
            .add(MenuItem::new("Short").checked(short_active))
            .clicked()
        {
            if short_active {
                view.short_rows = false;
            } else {
                view.preview = FramePreview::Off;
                view.short_rows = true;
                view.compact_rows = false;
            }
        }
        let compact_active = view.preview == FramePreview::Off && view.compact_rows;
        if ui
            .add(MenuItem::new("Compact").checked(compact_active))
            .clicked()
        {
            if compact_active {
                view.compact_rows = false;
            } else {
                view.preview = FramePreview::Off;
                view.compact_rows = true;
                view.short_rows = false;
            }
        }
        nf::menu_separator(ui);
        if ui
            .add(MenuItem::new("Tinted frames").checked(view.tinted))
            .clicked()
        {
            view.tinted = !view.tinted;
        }
        if ui
            .add(MenuItem::new("Show seconds on ruler").checked(view.show_seconds))
            .clicked()
        {
            view.show_seconds = !view.show_seconds;
        }
        if ui
            .add(MenuItem::new("Snap playhead to keyframes").checked(view.snap_playhead))
            .clicked()
        {
            view.snap_playhead = !view.snap_playhead;
        }
        let can_sync_layer = self
            .symbol()
            .timeline
            .layers
            .get(self.layer)
            .is_some_and(|l| l.properties.role != LayerRole::Guide);
        let synced = self.layer_sync_timeline(self.layer);
        if ui
            .add_enabled(
                can_sync_layer,
                MenuItem::new("Sync layer to timeline").checked(synced),
            )
            .clicked()
        {
            self.set_layer_sync_timeline(self.layer, !synced);
            ui.close();
        }
        nf::menu_separator(ui);
        ui.add_enabled(false, MenuItem::new("Help").shortcut("F1"))
            .on_disabled_hover_text(NOT_AVAILABLE);
        if ui.add(MenuItem::new("Close panel")).clicked() {
            self.set_panel_visible(PanelId::Timeline, false);
            ui.close();
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn timecode_matches_the_reference() {
        assert_eq!(super::timecode(29, 24), "00:01.21");
        assert_eq!(super::timecode(0, 24), "00:00.00");
        assert_eq!(super::timecode(24 * 75, 24), "01:15.00");
    }

    /// Every control that can hide has a menu row and a group in More ▸ Hidden controls.
    #[test]
    fn hidden_controls_have_menu_groups() {
        use super::{CONTROLS, Ctl, ESSENTIAL, HIDDEN_GROUPS, hidden_menu_rows};
        for (c, priority) in CONTROLS {
            let structural = matches!(c, Ctl::Slider | Ctl::Div1 | Ctl::Div2 | Ctl::Div3);
            if priority == ESSENTIAL || structural {
                continue;
            }
            assert!(hidden_menu_rows(c) > 0, "{c:?}");
            assert_eq!(
                HIDDEN_GROUPS.iter().filter(|(_, m)| m.contains(&c)).count(),
                1,
                "{c:?}"
            );
        }
    }

    /// R-052: controls hide by priority instead of clipping; the essential set survives any width.
    #[test]
    fn controls_hide_by_priority_and_give_back_what_fits() {
        use super::{CONTROLS, Ctl, ESSENTIAL, controls_width, estimated_width, fit_controls};
        let items: Vec<(Ctl, u8, f32)> = CONTROLS
            .iter()
            .map(|&(c, p)| (c, p, estimated_width(c)))
            .collect();
        let full = controls_width(&items.iter().map(|i| (i.0, i.2)).collect::<Vec<_>>());
        assert_eq!(fit_controls(&items, full).len(), CONTROLS.len());
        // Slightly narrower: the zoom slider (priority 0) goes first.
        let visible = fit_controls(&items, full - 1.);
        assert!(!visible.contains(&Ctl::Slider));
        assert!(visible.contains(&Ctl::Insert) && visible.contains(&Ctl::Tween));
        // Every width keeps the never-hidden controls, in bar order, and fits when possible.
        let never: Vec<Ctl> = CONTROLS
            .iter()
            .filter(|c| c.1 == ESSENTIAL)
            .map(|c| c.0)
            .collect();
        let minimum = controls_width(
            &items
                .iter()
                .filter(|i| i.1 == ESSENTIAL)
                .map(|i| (i.0, i.2))
                .collect::<Vec<_>>(),
        );
        for width in [minimum, 320., 560., 780., 1000., 1240., 1500.] {
            let visible = fit_controls(&items, width);
            assert!(never.iter().all(|c| visible.contains(c)), "{width}");
            let row: Vec<(Ctl, f32)> = items
                .iter()
                .filter(|i| visible.contains(&i.0))
                .map(|i| (i.0, i.2))
                .collect();
            assert!(controls_width(&row) <= width, "{width}");
            let order: Vec<usize> = visible
                .iter()
                .map(|c| CONTROLS.iter().position(|i| i.0 == *c).unwrap())
                .collect();
            assert!(order.windows(2).all(|w| w[0] < w[1]), "{width}");
        }
        // At 1000 pt the lowest priorities are hidden, the frame tools stay.
        let visible = fit_controls(&items, 1000.);
        for gone in [Ctl::Slider, Ctl::Duration, Ctl::Scene] {
            assert!(!visible.contains(&gone));
        }
        for kept in [
            Ctl::Insert,
            Ctl::Tween,
            Ctl::Onion,
            Ctl::Loop,
            Ctl::Delete,
            Ctl::Frame,
        ] {
            assert!(visible.contains(&kept));
        }
        // Last-resort controls (Delete layer, frame number, dividers) go only when nothing else is left.
        let visible = fit_controls(&items, 330.);
        assert!(!visible.contains(&Ctl::Frame) && !visible.contains(&Ctl::Insert));
        let visible = fit_controls(&items, 270.);
        assert!(!visible.contains(&Ctl::Delete));
        // No divider is left at an edge, next to another divider or next to the flexible gap.
        for width in [270., 330., 450., 560., 780.] {
            let row = fit_controls(&items, width);
            for (k, c) in row.iter().enumerate() {
                if matches!(c, Ctl::Div1 | Ctl::Div2 | Ctl::Div3) {
                    assert!(k > 0 && k + 1 < row.len(), "{width}: {row:?}");
                    for n in [row[k - 1], row[k + 1]] {
                        assert!(
                            !matches!(n, Ctl::Div1 | Ctl::Div2 | Ctl::Div3 | Ctl::Flex),
                            "{width}: {row:?}"
                        );
                    }
                }
            }
        }
    }
}
