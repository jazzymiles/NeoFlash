//! Timeline controls bar (reference §4 "Timeline controls bar", audit #2 and #4): one 44 pt row above
//! the grid. Left → right: layer actions · transport (Play = primary Icon Button MD) · loop · timecode
//! · Scene select · FPS / Duration fields · frame insert group (F5 F6 F7) · tween group · ease · sound;
//! pinned on the right: onion-skin group (its options open in a popover) · centre playhead · frame view ⋯
//! · zoom − slider +.
//!
//! Controls whose feature the engine lacks are shown disabled with "not available yet" in their name
//! and tooltip, never as working-looking fakes; ease, sound and motion tween open their planned
//! previews (Ease panel, Properties — Sound, Properties — Motion tween). When the bar is narrower than its content the left
//! part scrolls horizontally while the onion group and frame view menu stay pinned and visible.
use super::menu::NOT_AVAILABLE;
use crate::app::*;
use crate::workspace::{PanelId, PanelPlacement};
use eframe::egui::{self, Rect, Sense, Stroke, Vec2};
use neoflash_core::*;
use neoflash_ui::components::{
    self as nf, IconButton, IconButtonSize, IconButtonType, IconGroup, MenuItem,
};
use neoflash_ui::icons::i16;
use neoflash_ui::metrics::{CONTROL, FrameView, TIMELINE_CONTROLS};
use neoflash_ui::tokens::space;
use neoflash_ui::{TextStyle, typography};

/// Narrowest and widest frame cell of the continuous zoom (Tiny is 6; overview zoom may go lower).
pub(crate) const MIN_CELL: f32 = 3.;
pub(crate) const MAX_CELL: f32 = 52.;
/// Gap between groups (`Timeline controls` auto-layout, gap 8) and inside groups (gap 2).
const GAP: f32 = space::S2;
const GROUP_GAP: f32 = 2.;
const SLIDER: f32 = 140.;

/// Frame view ⋯ options that only change presentation. Workspace-only, never in the document.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FrameViewOptions {
    /// Off = spans are not colour-coded, only arrows (§5.4 "Tinted frames").
    pub tinted: bool,
    pub show_seconds: bool,
}

impl Default for FrameViewOptions {
    fn default() -> Self {
        Self {
            tinted: true,
            show_seconds: true,
        }
    }
}

fn options_id() -> egui::Id {
    egui::Id::new("neoflash-timeline-frame-view")
}

pub(crate) fn frame_view_options(ctx: &egui::Context) -> FrameViewOptions {
    ctx.data(|data| data.get_temp(options_id()))
        .unwrap_or_default()
}

/// Requests from the bar that the frame area applies after layout.
#[derive(Default)]
pub(crate) struct ControlsOutcome {
    /// Scroll the frames so the playhead is centred (Centre playhead button).
    pub recenter: bool,
}

/// `mm:ss.hh` of a 0-based frame (frame 30 at 24 fps → `00:01.21`).
pub(crate) fn timecode(frame: u32, fps: u32) -> String {
    let t = frame as f64 / fps.max(1) as f64;
    let minutes = (t / 60.).floor();
    format!("{:02}:{:05.2}", minutes as u32, t - minutes * 60.)
}

fn not_available(label: &str) -> String {
    format!("{label} — not available yet")
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
        let (bar, _) = ui.allocate_exact_size(Vec2::new(width, TIMELINE_CONTROLS), Sense::hover());
        ui.painter().hline(
            bar.x_range(),
            bar.bottom() - 0.5,
            Stroke::new(1.0_f32, p.border_subtle),
        );
        let mut out = ControlsOutcome::default();
        // Pinned right cluster; lower-priority pieces drop first as the panel narrows.
        let full_zoom = width >= 1100.;
        let zoom_buttons = width >= 600.;
        let centre = width >= 900.;
        let onion_w = 4. * CONTROL + 3. * GROUP_GAP;
        let zoom_w = if full_zoom {
            2. * CONTROL + SLIDER + 2. * GAP
        } else if zoom_buttons {
            2. * CONTROL + GAP
        } else {
            0.
        };
        // + the 9 pt divider after the onion group.
        let right_w = onion_w
            + GAP
            + 9.
            + GAP
            + if centre { CONTROL + GAP } else { 0. }
            + CONTROL
            + if zoom_w > 0. { GAP + zoom_w } else { 0. };
        let inner = bar.shrink2(Vec2::new(GAP, 0.));
        let right = Rect::from_min_max(
            egui::pos2((inner.right() - right_w).max(inner.left()), inner.top()),
            inner.max,
        );
        let left = Rect::from_min_max(
            inner.min,
            egui::pos2((right.left() - GAP).max(inner.left()), inner.bottom()),
        );
        let wide = width >= 1400.;
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(left)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                egui::ScrollArea::horizontal()
                    .id_salt("timeline_controls")
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_min_height(left.height());
                        ui.horizontal_centered(|ui| {
                            ui.spacing_mut().item_spacing = Vec2::new(GAP, 0.);
                            // Figma controls bar: dividers between layer actions | transport,
                            // timecode | scene, and onion | view (NF2 Divider, vertical).
                            self.timeline_layer_actions(ui, layer_column, wide);
                            nf::divider(ui, true);
                            self.timeline_transport(ui);
                            if self.symbol().kind != SymbolKind::Button {
                                let frame = self.frame;
                                let count = self.symbol().timeline.frame_count;
                                ui.horizontal_centered(|ui| {
                                    ui.spacing_mut().item_spacing.x = GAP;
                                    ui.label(
                                        typography::rich(
                                            timecode(frame, self.project.fps),
                                            TextStyle::Mono,
                                        )
                                        .color(p.text_primary),
                                    );
                                    ui.label(
                                        typography::rich(
                                            format!("{} / {} F", frame + 1, count),
                                            TextStyle::MonoSmall,
                                        )
                                        .color(p.text_tertiary),
                                    );
                                })
                                .response
                                .on_hover_text("Current time and frame");
                                nf::divider(ui, true);
                            }
                            self.timeline_scene_select(ui);
                            self.timeline_timing_ui(ui);
                            self.timeline_frame_tools(ui);
                        });
                    });
            },
        );
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(right)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                ui.set_clip_rect(right.intersect(ui.clip_rect()));
                ui.spacing_mut().item_spacing = Vec2::new(GAP, 0.);
                self.timeline_onion_group(ui);
                nf::divider(ui, true);
                if centre
                    && ui
                        .add(IconButton::new(i16::CENTERFRAME, "Center playhead"))
                        .clicked()
                {
                    out.recenter = true;
                }
                let more = ui.add(IconButton::new(i16::MORE, "Frame view options"));
                nf::popup_below(&more, 240., |ui| {
                    self.timeline_frame_view_menu(ui, frames_width)
                });
                if zoom_w > 0. {
                    self.timeline_zoom(ui, full_zoom);
                }
            },
        );
        out
    }

    /// `Layer actions` (264 = layer column − 2 × 8 when wide, so transport starts over frame 1).
    fn timeline_layer_actions(&mut self, ui: &mut egui::Ui, layer_column: f32, wide: bool) {
        let compact = 4. * CONTROL + 3. * GROUP_GAP;
        let w = if wide {
            (layer_column - 2. * GAP).max(compact)
        } else {
            compact
        };
        ui.allocate_ui_with_layout(
            Vec2::new(w, CONTROL),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.spacing_mut().item_spacing.x = GROUP_GAP;
                if ui
                    .add(IconButton::new(i16::LAYERPLUS, "Add layer"))
                    .clicked()
                {
                    self.insert_timeline_layer();
                }
                if ui
                    .add(IconButton::new(i16::FOLDERPLUS, "New folder"))
                    .clicked()
                {
                    self.insert_layer_folder(self.layer);
                }
                ui.add_enabled(
                    false,
                    IconButton::new(i16::GUIDEPLUS, &not_available("Add motion guide")),
                );
                let rest = ui.available_width() - CONTROL;
                if rest > 0. {
                    ui.add_space(rest);
                }
                if ui
                    .add_enabled(
                        self.symbol().timeline.layers.len() > 1,
                        IconButton::new(i16::TRASH, "Delete selected layer"),
                    )
                    .clicked()
                {
                    self.delete_timeline_layer(self.layer);
                }
            },
        );
    }

    fn timeline_seek(&mut self, frame: u32) {
        self.finish_edits();
        self.timeline_key_drag = None;
        self.reset_authored_preview();
        self.frame = frame.min(self.symbol().timeline.frame_count.saturating_sub(1));
    }

    /// Transport and loop: first, step back, Play (primary MD 40), step forward, last.
    fn timeline_transport(&mut self, ui: &mut egui::Ui) {
        let last = self.symbol().timeline.frame_count.saturating_sub(1);
        if ui.add(IconButton::new(i16::FIRST, "Rewind")).clicked() {
            self.timeline_seek(0);
        }
        if ui
            .add(IconButton::new(i16::STEPBACK, "Previous frame").shortcut("←"))
            .clicked()
        {
            self.timeline_seek(self.frame.saturating_sub(1));
        }
        let (icon, label) = if self.playing {
            (i16::PAUSE, "Pause preview")
        } else {
            (i16::PLAY, "Play preview")
        };
        if ui
            .add(
                IconButton::new(icon, label)
                    .kind(IconButtonType::Primary)
                    .size(IconButtonSize::MD)
                    .shortcut("↵"),
            )
            .clicked()
        {
            self.set_playing(!self.playing);
        }
        if ui
            .add(IconButton::new(i16::STEPFWD, "Next frame").shortcut("→"))
            .clicked()
        {
            self.timeline_seek(self.frame + 1);
        }
        if ui
            .add(IconButton::new(i16::LAST, "Go to last frame"))
            .clicked()
        {
            self.timeline_seek(last);
        }
        ui.add_enabled(
            false,
            IconButton::new(i16::LOOP, &not_available("Loop playback")),
        );
    }

    /// Scene `Select`. The engine has one main timeline per document; the menu returns to it.
    fn timeline_scene_select(&mut self, ui: &mut egui::Ui) {
        let root = self.project.root.clone();
        let name = self
            .project
            .symbol(&root)
            .map(|s| s.name.clone())
            .unwrap_or_default();
        let at_root = self.editing.len() <= 1;
        let mut go_root = false;
        let (response, _) = nf::Select::new("timeline-scene", &name)
            .width(120.)
            .show(ui, |ui| {
                if ui.add(MenuItem::new(&name).checked(at_root)).clicked() {
                    go_root = true;
                }
                nf::menu_separator(ui);
                ui.add_enabled(false, MenuItem::new(&not_available("New scene")));
            });
        response.on_hover_text("Main timeline. Multiple scenes are not available yet.");
        if go_root && !at_root {
            self.finish_edits();
            self.timeline_key_drag = None;
            self.editing.truncate(1);
            self.reset_authored_preview();
            self.frame = 0;
            self.layer = 0;
            self.clear_selection();
        }
    }

    /// Frame insert group (F5 / F6 / F7), tween group, ease, sound.
    fn timeline_frame_tools(&mut self, ui: &mut egui::Ui) {
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
        let tweened = self
            .symbol()
            .timeline
            .layers
            .get(self.layer)
            .and_then(|layer| layer.key_at(self.frame))
            .map(|key| key.tween.is_some());
        let shape = not_available("Shape tween");
        let motion = "Motion tween";
        let classic = if tweened == Some(true) {
            "Remove classic tween"
        } else {
            "Create classic tween"
        };
        let tween = IconGroup {
            items: &[
                (i16::TWEEN_CLASSIC, classic),
                (i16::TWEEN_SHAPE, &shape),
                (i16::TWEEN_MOTION, motion),
            ],
            selected: (tweened == Some(true)).then_some(0),
        }
        .show_with(ui, &[editable && tweened.is_some(), false, true], &[]);
        if tween == Some(2) {
            Self::open_planned(ui.ctx(), "Timeline controls ▸ Motion tween");
        }
        if tween == Some(0) {
            self.finish_edits();
            self.transact(|s| {
                let (f, l) = (s.frame, s.layer);
                if let Some(k) = s.symbol_mut().timeline.layers[l].key_at_mut(f) {
                    k.tween = if k.tween.is_some() {
                        None
                    } else {
                        Some(Easing::EaseInOut)
                    };
                }
            });
        }
        // Ease opens the Ease panel (§6); the classic tween's preset stays editable in Properties.
        if ui.add(IconButton::new(i16::EASE, "Ease")).clicked() {
            Self::open_planned(ui.ctx(), "Timeline controls ▸ Ease");
        }
        if ui.add(IconButton::new(i16::SOUND, "Sound")).clicked() {
            Self::open_planned(ui.ctx(), "Timeline controls ▸ Sound");
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
                }],
            });
            s.layer = s.symbol().timeline.layers.len() - 1;
        });
    }

    /// Onion-skin group: on/off, outlines, edit multiple frames, modify markers (popover, audit #4).
    fn timeline_onion_group(&mut self, ui: &mut egui::Ui) {
        ui.allocate_ui_with_layout(
            Vec2::new(4. * CONTROL + 3. * GROUP_GAP, CONTROL),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.spacing_mut().item_spacing.x = GROUP_GAP;
                if ui
                    .add(IconButton::new(i16::ONION, "Onion skin").selected(self.onion))
                    .clicked()
                {
                    self.onion = !self.onion;
                }
                if ui
                    .add(
                        IconButton::new(i16::ONIONOUTLINE, "Onion skin outlines")
                            .selected(self.onion_outlines),
                    )
                    .clicked()
                {
                    self.onion_outlines = !self.onion_outlines;
                }
                ui.add_enabled(
                    false,
                    IconButton::new(i16::EDITMULTI, &not_available("Edit multiple frames")),
                );
                let popup_id = ui.id().with("onion-options-popup");
                let open = egui::Popup::is_id_open(ui.ctx(), popup_id);
                let options =
                    ui.add(IconButton::new(i16::MARKERS, "Onion skin options").selected(open));
                egui::Popup::menu(&options)
                    .id(popup_id)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .gap(space::S1)
                    .frame(nf::popover_frame(ui.ctx()))
                    .show(|ui| self.onion_options(ui));
            },
        );
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
                ui.horizontal(|ui| {
                    ui.add(nf::Checkbox::new(&mut self.onion_outlines, "Outlines"));
                    if ui
                        .add(nf::Checkbox::new(&mut self.onion_anchor, "Anchor"))
                        .changed()
                    {
                        self.anchored_range = (
                            self.frame.saturating_sub(self.onion_before),
                            (self.frame + self.onion_after)
                                .min(self.symbol().timeline.frame_count.saturating_sub(1)),
                        );
                    }
                });
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = space::S2;
                    let mut before = self.onion_before as f64;
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
                        self.onion_before = before.round() as u32;
                    }
                    let mut after = self.onion_after as f64;
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
                        self.onion_after = after.round() as u32;
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(typography::rich("Opacity", TextStyle::Small).color(p.text_secondary));
                    let mut opacity = self.onion_opacity as f64;
                    if ui
                        .add(
                            nf::Slider::new(&mut opacity, 0.05..=0.7, "Onion opacity")
                                .step(0.01)
                                .width(SLIDER),
                        )
                        .changed()
                    {
                        self.onion_opacity = opacity as f32;
                    }
                });
            });
    }

    /// Frame view ⋯ (§5.4): placement, frame sizes, fit, tinted frames, seconds on the ruler, help, close.
    fn timeline_frame_view_menu(&mut self, ui: &mut egui::Ui, frames_width: f32) {
        let ctx = ui.ctx().clone();
        let mut options = frame_view_options(&ctx);
        crate::ui_icons::menu_popup(ui, "Placement", "Floating window", "", |ui| {
            let docked = self.workspace.placement(PanelId::Timeline) == PanelPlacement::Docked;
            for (label, placement, current) in [
                ("Bottom (docked)", PanelPlacement::Docked, docked),
                ("Floating window", PanelPlacement::Detached, !docked),
            ] {
                if ui.add(MenuItem::new(label).checked(current)).clicked() && !current {
                    self.set_panel_placement(&ctx, PanelId::Timeline, placement);
                    ui.close();
                }
                if label.starts_with("Bottom") {
                    for edge in ["Top", "Left", "Right"] {
                        Self::unavailable_item(ui, edge, "");
                    }
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
        ui.add_enabled(false, MenuItem::new("Preview"))
            .on_disabled_hover_text(NOT_AVAILABLE);
        ui.add_enabled(false, MenuItem::new("Preview in context"))
            .on_disabled_hover_text(NOT_AVAILABLE);
        ui.add_enabled(false, MenuItem::new("Short"))
            .on_disabled_hover_text(NOT_AVAILABLE);
        nf::menu_separator(ui);
        if ui
            .add(MenuItem::new("Tinted frames").checked(options.tinted))
            .clicked()
        {
            options.tinted = !options.tinted;
        }
        if ui
            .add(MenuItem::new("Show seconds on ruler").checked(options.show_seconds))
            .clicked()
        {
            options.show_seconds = !options.show_seconds;
        }
        ui.add_enabled(false, MenuItem::new("Snap playhead to keyframes"))
            .on_disabled_hover_text(NOT_AVAILABLE);
        nf::menu_separator(ui);
        ui.add_enabled(false, MenuItem::new("Help").shortcut("F1"))
            .on_disabled_hover_text(NOT_AVAILABLE);
        if ui.add(MenuItem::new("Close panel")).clicked() {
            self.set_panel_visible(PanelId::Timeline, false);
            ui.close();
        }
        ctx.data_mut(|data| data.insert_temp(options_id(), options));
    }

    /// Zoom − slider +. The slider is logarithmic over the frame width; − / + step by 1.4×.
    fn timeline_zoom(&mut self, ui: &mut egui::Ui, slider: bool) {
        if ui
            .add(IconButton::new(i16::MINUS, "Show more frames"))
            .clicked()
        {
            self.timeline_cell = (self.timeline_cell / 1.4).max(MIN_CELL);
        }
        if slider {
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
                .on_hover_text("Change the width of timeline cells without changing the animation")
                .changed()
            {
                self.timeline_cell = (zoom.exp() as f32).clamp(MIN_CELL, MAX_CELL);
            }
        }
        if ui
            .add(IconButton::new(i16::PLUS, "Widen frame cells"))
            .clicked()
        {
            self.timeline_cell = (self.timeline_cell * 1.4).min(MAX_CELL);
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
}
