//! Ease panel (NF2 `Panel / Ease`, PP-01; references `panel-ease.png`, `ease-range-states.png`).
//!
//! Edits the ease of the tween span under the playhead on the current layer: a classic or shape
//! tween's keyframe, or a motion tween's start key (plus optional per-property overrides on motion
//! tracks). The panel edits a draft (egui temp memory, never the document); Apply writes
//! `Keyframe.tween` and `Keyframe.property_eases` as one Undo step, Cancel discards the draft.
use super::planned::{caption, mono};
use crate::app::Editor;
use crate::theme::ScrollTheme as _;
use eframe::egui::{self, Ui};
use neoflash_core::*;
use neoflash_ui::components as c;
use neoflash_ui::icons::i16;
use neoflash_ui::tokens::space;

/// Ease-out "Back" preset: overshoots to about 110 % before settling (CSS `easeOutBack`).
pub(crate) const BACK_OUT: [f64; 4] = [0.34, 1.56, 0.64, 1.0];
/// Range the graph handles can be dragged in (the Overshoot / Undershoot graph shows −30…130 %).
const HANDLE_Y: std::ops::RangeInclusive<f32> = -0.3..=1.3;

const EASE_TARGETS: &[(Option<TweenProperty>, &str)] = &[
    (None, "All"),
    (Some(TweenProperty::X), "X"),
    (Some(TweenProperty::Y), "Y"),
    (Some(TweenProperty::Rotation), "Rotation Z"),
    (Some(TweenProperty::ScaleX), "Scale X"),
    (Some(TweenProperty::ScaleY), "Scale Y"),
    (Some(TweenProperty::Skew), "Skew"),
    (Some(TweenProperty::Alpha), "Alpha"),
    (Some(TweenProperty::Color), "Color"),
];

fn ease_target_label(target: Option<TweenProperty>) -> &'static str {
    EASE_TARGETS
        .iter()
        .find(|(t, _)| *t == target)
        .map_or("All", |(_, label)| *label)
}

/// The tween span under the playhead on the current layer.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TweenSpan {
    pub layer_id: Id,
    pub layer_name: String,
    pub kind: TweenKind,
    /// Frame of the keyframe holding the ease (a motion tween's start key).
    pub start: u32,
    /// Exclusive end frame of the span.
    pub end: u32,
    pub easing: Easing,
    /// Per-property ease overrides on a motion tween's start key.
    pub property_eases: Vec<(TweenProperty, Easing)>,
}

impl TweenSpan {
    /// Frames the tween covers (the graph's X axis).
    pub(crate) fn frames(&self) -> u32 {
        self.end.saturating_sub(self.start).max(1)
    }
    /// "frames a–b" in one-based frame numbers.
    pub(crate) fn frame_label(&self) -> String {
        format!("frames {}–{}", self.start + 1, self.end)
    }
    /// Per-property override for `property`, if any.
    pub(crate) fn property_override(&self, property: TweenProperty) -> Option<Easing> {
        self.property_eases
            .iter()
            .find(|(p, _)| *p == property)
            .map(|(_, e)| *e)
    }
}

/// Graph curve (`X1 Y1 X2 Y2`) that draws `easing`. Flash acceleration and the quadratic presets
/// are exact cubic forms of their quadratic curves; Ease in-out, Bounce and Elastic are
/// approximated for display while `evaluate::easing` computes their exact curves.
pub(crate) fn easing_bezier(easing: Easing) -> [f32; 4] {
    let quadratic = |acceleration: f64| {
        let a = (acceleration / 100.).clamp(-1., 1.) as f32;
        [1. / 3., (1. + a) / 3., 2. / 3., (2. + a) / 3.]
    };
    match easing {
        Easing::Linear => quadratic(0.),
        Easing::EaseIn => quadratic(-100.),
        Easing::EaseOut => quadratic(100.),
        Easing::EaseInOut => [0.45, 0., 0.55, 1.],
        Easing::Bounce => [0.34, 1.25, 0.64, 1.0],
        Easing::Elastic => [0.22, 1.35, 0.36, 1.0],
        Easing::CubicBezier { x1, y1, x2, y2 } => [x1 as f32, y1 as f32, x2 as f32, y2 as f32],
        Easing::Flash { acceleration } => quadratic(acceleration),
    }
}

/// Preset card that shows `easing` as selected.
pub(crate) fn preset_of(easing: Easing) -> c::EasePresetKind {
    match easing {
        Easing::Linear => c::EasePresetKind::Linear,
        Easing::EaseIn => c::EasePresetKind::EaseIn,
        Easing::EaseOut => c::EasePresetKind::EaseOut,
        Easing::EaseInOut => c::EasePresetKind::EaseInOut,
        Easing::Bounce => c::EasePresetKind::Bounce,
        Easing::Elastic => c::EasePresetKind::Elastic,
        Easing::CubicBezier { x1, y1, x2, y2 } if [x1, y1, x2, y2] == BACK_OUT => {
            c::EasePresetKind::Back
        }
        _ => c::EasePresetKind::Custom,
    }
}

/// Easing written for a preset card.
pub(crate) fn preset_easing(preset: c::EasePresetKind, current: Easing) -> Option<Easing> {
    Some(match preset {
        c::EasePresetKind::Linear => Easing::Linear,
        c::EasePresetKind::EaseIn => Easing::EaseIn,
        c::EasePresetKind::EaseOut => Easing::EaseOut,
        c::EasePresetKind::EaseInOut => Easing::EaseInOut,
        c::EasePresetKind::Back => {
            let [x1, y1, x2, y2] = BACK_OUT;
            Easing::CubicBezier { x1, y1, x2, y2 }
        }
        c::EasePresetKind::Bounce => Easing::Bounce,
        c::EasePresetKind::Elastic => Easing::Elastic,
        c::EasePresetKind::Custom => {
            let [x1, y1, x2, y2] = easing_bezier(current).map(f64::from);
            Easing::CubicBezier { x1, y1, x2, y2 }
        }
    })
}

fn range_of(bezier: [f32; 4]) -> c::EaseRange {
    if bezier[1].min(bezier[3]) < 0. {
        c::EaseRange::Undershoot
    } else if bezier[1].max(bezier[3]) > 1. {
        c::EaseRange::Overshoot
    } else {
        c::EaseRange::Normal
    }
}

/// Draft of the Ease panel: the span it belongs to and the edited curve(s).
#[derive(Clone, Debug, PartialEq)]
struct EaseDraft {
    symbol: Id,
    span: TweenSpan,
    easing: Easing,
    property_eases: Vec<(TweenProperty, Easing)>,
    target: Option<TweenProperty>,
    one_curve_for_all: bool,
    /// The curve differs from the document and waits for Apply.
    dirty: bool,
    /// Preview playback start (egui time), if playing.
    playing_since: Option<f64>,
}

impl EaseDraft {
    fn active_easing(&self) -> Easing {
        match self.target {
            Some(prop) => self
                .property_eases
                .iter()
                .find(|(p, _)| *p == prop)
                .map_or(self.easing, |(_, e)| *e),
            None => self.easing,
        }
    }

    fn set_property_override(&mut self, prop: TweenProperty, easing: Easing) {
        if let Some((_, existing)) = self.property_eases.iter_mut().find(|(p, _)| *p == prop) {
            *existing = easing;
        } else {
            self.property_eases.push((prop, easing));
        }
    }

    fn recompute_dirty(&mut self) {
        self.dirty =
            self.easing != self.span.easing || self.property_eases != self.span.property_eases;
    }
}

fn draft_id() -> egui::Id {
    egui::Id::new("nf2-ease-draft")
}

impl Editor {
    /// Tween span under the playhead on the current layer, if the frame is tweened.
    pub(crate) fn tween_span(&self) -> Option<TweenSpan> {
        let timeline = &self.symbol().timeline;
        let layer = timeline.layers.get(self.layer)?;
        let (key, end) = if let Some((start, properties, end)) = motion_span(layer, self.frame) {
            let last = properties.last().map_or(start.frame, |k| k.frame);
            let end = end.unwrap_or(timeline.frame_count).max(last + 1);
            (start, end)
        } else {
            let key = layer.key_at(self.frame)?;
            if key.keyed_properties.is_some() {
                return None;
            }
            let end = layer
                .keyframes
                .iter()
                .filter(|k| k.frame > key.frame)
                .map(|k| k.frame)
                .min()
                .or(layer.properties.end_frame)
                .unwrap_or(timeline.frame_count);
            (key, end)
        };
        Some(TweenSpan {
            layer_id: layer.id.clone(),
            layer_name: layer.name.clone(),
            kind: key.tween_kind,
            start: key.frame,
            end: end.max(key.frame + 1),
            easing: key.tween?,
            property_eases: if key.tween_kind == TweenKind::Motion {
                key.property_eases.clone()
            } else {
                vec![]
            },
        })
    }

    /// Open the Ease panel focused on `property` of the current motion tween (`Motion editor ▸ Edit curve…`).
    pub(crate) fn open_ease_for_property(&mut self, ctx: &egui::Context, property: TweenProperty) {
        self.show_panel(crate::workspace::PanelId::Ease);
        let Some(span) = self.tween_span().filter(|s| s.kind == TweenKind::Motion) else {
            return;
        };
        let symbol = self.symbol_id().to_owned();
        let stored: Option<EaseDraft> = ctx.data(|d| d.get_temp(draft_id()));
        let draft = match stored {
            Some(mut draft)
                if draft.symbol == symbol
                    && draft.span.layer_id == span.layer_id
                    && draft.span.start == span.start =>
            {
                if !draft.dirty {
                    draft.easing = span.easing;
                    draft.property_eases = span.property_eases.clone();
                }
                draft.span = span;
                draft.target = Some(property);
                draft.one_curve_for_all = false;
                draft
            }
            _ => EaseDraft {
                symbol,
                easing: span.easing,
                property_eases: span.property_eases.clone(),
                span,
                target: Some(property),
                one_curve_for_all: false,
                dirty: false,
                playing_since: None,
            },
        };
        ctx.data_mut(|d| d.insert_temp(draft_id(), draft));
    }

    fn span_editable(&self, span: &TweenSpan) -> bool {
        self.symbol()
            .timeline
            .layers
            .iter()
            .position(|l| l.id == span.layer_id)
            .is_some_and(|index| {
                let timeline = &self.symbol().timeline;
                let layer = &timeline.layers[index];
                !layer_effectively_locked(timeline, layer)
                    && layer_effectively_visible(timeline, layer)
                    && layer
                        .keyframes
                        .iter()
                        .any(|k| k.frame == span.start && k.tween.is_some())
            })
    }

    /// Write the span ease of a tween span on the current symbol (preserving any per-property eases).
    /// One Undo step; `false` when the span's layer is not editable or its keyframe is gone.
    pub(crate) fn set_tween_easing(&mut self, span: &TweenSpan, easing: Easing) -> bool {
        self.set_tween_eases(span, easing, span.property_eases.clone())
    }

    /// Write both the span ease and (for motion tweens) per-property ease overrides on the current
    /// symbol in one Undo step.
    pub(crate) fn set_tween_eases(
        &mut self,
        span: &TweenSpan,
        easing: Easing,
        property_eases: Vec<(TweenProperty, Easing)>,
    ) -> bool {
        self.finish_edits();
        if !self.span_editable(span) {
            return false;
        }
        let mut deduped: Vec<(TweenProperty, Easing)> = Vec::new();
        for (prop, ease) in property_eases {
            if let Some((_, existing)) = deduped.iter_mut().find(|(p, _)| *p == prop) {
                *existing = ease;
            } else {
                deduped.push((prop, ease));
            }
        }
        let uses_flash = matches!(easing, Easing::Flash { .. })
            || deduped
                .iter()
                .any(|(_, e)| matches!(e, Easing::Flash { .. }));
        self.transact_named("Edit ease", |s| {
            if let Some(key) = s
                .symbol_mut()
                .timeline
                .layers
                .iter_mut()
                .find(|l| l.id == span.layer_id)
                .and_then(|l| l.keyframes.iter_mut().find(|k| k.frame == span.start))
            {
                key.tween = Some(easing);
                key.property_eases = if key.tween_kind == TweenKind::Motion {
                    deduped
                } else {
                    vec![]
                };
            }
            if uses_flash {
                s.project.format_version = s.project.format_version.max(5);
            }
        });
        true
    }

    /// Window ▸ Ease (and the Timeline controls Ease button, Frame menu ▸ Ease…).
    pub(crate) fn ease_panel_ui(&mut self, ui: &mut Ui) {
        self.panel_title(ui, crate::workspace::PanelId::Ease, "Ease");
        egui::ScrollArea::vertical()
            .id_salt("ease-content")
            .auto_shrink([false, false])
            .themed()
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(space::S2, space::S2);
                self.ease_panel(ui);
            });
    }

    /// Panel / Ease body (moved from the planned preview; bound to the span under the playhead).
    fn ease_panel(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let span = self.tween_span();
        let symbol = self.symbol_id().to_owned();
        let stored: Option<EaseDraft> = ctx.data(|d| d.get_temp(draft_id()));
        let mut draft = match (stored, &span) {
            (Some(draft), Some(span))
                if draft.symbol == symbol
                    && draft.span.layer_id == span.layer_id
                    && draft.span.start == span.start =>
            {
                let mut draft = draft;
                if !draft.dirty {
                    draft.easing = span.easing;
                    draft.property_eases = span.property_eases.clone();
                    if draft.target.is_none() {
                        draft.one_curve_for_all = draft.property_eases.is_empty();
                    }
                }
                if span.kind != TweenKind::Motion {
                    draft.target = None;
                    draft.property_eases.clear();
                    draft.one_curve_for_all = true;
                }
                draft.span = span.clone();
                draft
            }
            (_, Some(span)) => EaseDraft {
                symbol: symbol.clone(),
                span: span.clone(),
                easing: span.easing,
                property_eases: span.property_eases.clone(),
                target: None,
                one_curve_for_all: span.property_eases.is_empty(),
                dirty: false,
                playing_since: None,
            },
            (_, None) => EaseDraft {
                symbol: symbol.clone(),
                span: TweenSpan {
                    layer_id: String::new(),
                    layer_name: String::new(),
                    kind: TweenKind::Classic,
                    start: 0,
                    end: 1,
                    easing: Easing::Linear,
                    property_eases: vec![],
                },
                easing: Easing::Linear,
                property_eases: vec![],
                target: None,
                one_curve_for_all: true,
                dirty: false,
                playing_since: None,
            },
        };
        let editable = span.is_some() && self.layer_editable();
        let is_motion = span.as_ref().is_some_and(|s| s.kind == TweenKind::Motion);
        let active_easing = draft.active_easing();
        let bezier = easing_bezier(active_easing);
        let preset = preset_of(active_easing);
        let frames = draft.span.frames();
        let fps = self.project.fps.max(1) as f64;
        let now = ctx.input(|i| i.time);
        let progress = match draft.playing_since {
            Some(since) => {
                let p = ((now - since) * fps / frames as f64) as f32;
                if p >= 1. {
                    draft.playing_since = None;
                } else {
                    ctx.request_repaint();
                }
                p.clamp(0., 1.)
            }
            None => {
                (self.frame.saturating_sub(draft.span.start) as f32 / frames as f32).clamp(0., 1.)
            }
        };
        let playhead = (progress * frames as f32).round() as u32;
        let mut new_easing = None;
        let mut picked_target = None;
        let mut toggled_one_curve = None;
        let mut action = None;
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = space::S3;
            let context = match &span {
                Some(span) => format!(
                    "{} tween · {} · {}",
                    match span.kind {
                        TweenKind::Classic => "Classic",
                        TweenKind::Shape => "Shape",
                        TweenKind::Motion => "Motion",
                    },
                    span.layer_name,
                    span.frame_label()
                ),
                None => "No tween at the playhead · move it into a tweened span".to_owned(),
            };
            caption(ui, &context);
            ui.add_enabled_ui(editable, |ui| {
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(is_motion, |ui| {
                        let label = format!("Property: {}", ease_target_label(draft.target));
                        c::Select::new("ease-property", &label)
                            .width(176.0)
                            .show(ui, |ui| {
                                for &(target, item_label) in EASE_TARGETS {
                                    if ui
                                        .add(
                                            c::MenuItem::new(item_label)
                                                .checked(draft.target == target),
                                        )
                                        .clicked()
                                    {
                                        picked_target = Some(target);
                                        ui.close();
                                    }
                                }
                            });
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let mut one_curve = if is_motion {
                            draft.one_curve_for_all
                        } else {
                            true
                        };
                        if ui
                            .add_enabled(
                                is_motion,
                                c::Checkbox::new(&mut one_curve, "One curve for all"),
                            )
                            .changed()
                        {
                            toggled_one_curve = Some(one_curve);
                        }
                    });
                });
                for row in c::EasePresetKind::ALL.chunks(4) {
                    ui.horizontal(|ui| {
                        for kind in row {
                            let supported = preset_easing(*kind, active_easing).is_some();
                            let response = ui.add_enabled(
                                supported,
                                c::EasePreset {
                                    preset: *kind,
                                    selected: *kind == preset,
                                },
                            );
                            c::not_available(response.clone(), kind.label());
                            if response.clicked() {
                                new_easing = preset_easing(*kind, active_easing);
                            }
                        }
                    });
                }
                let graph = ui.add(c::EaseGraph {
                    range: range_of(bezier),
                    bezier,
                    frames,
                    playhead: span.as_ref().map(|_| playhead),
                    width: ui.available_width(),
                });
                if let Some(curve) = drag_handles(ui, &graph, bezier) {
                    new_easing = Some(curve);
                }
                ui.horizontal(|ui| {
                    let w = (ui.available_width() - 3.0 * space::S2) / 4.0;
                    let mut values = bezier.map(f64::from);
                    let mut changed = false;
                    for (i, prefix) in ["X1", "Y1", "X2", "Y2"].into_iter().enumerate() {
                        let range = if i % 2 == 0 {
                            0.0..=1.0
                        } else {
                            -100.0..=100.0
                        };
                        changed |= ui
                            .add(
                                c::Field::new(&mut values[i], prefix)
                                    .prefix(prefix)
                                    .decimals(2)
                                    .range(range)
                                    .step(0.01)
                                    .width(w),
                            )
                            .changed();
                    }
                    if changed {
                        let [x1, y1, x2, y2] = values;
                        new_easing = Some(Easing::CubicBezier { x1, y1, x2, y2 });
                    }
                });
                c::ease_preview_track(ui, bezier, progress, ui.available_width());
                ui.horizontal(|ui| {
                    if ui
                        .add(c::IconButton::new(i16::PLAY, "Play preview"))
                        .clicked()
                    {
                        draft.playing_since = Some(now);
                        ctx.request_repaint();
                    }
                    if ui
                        .add(c::IconButton::new(i16::STOP, "Stop preview"))
                        .clicked()
                    {
                        draft.playing_since = None;
                    }
                    let eased = easing(f64::from(progress), active_easing);
                    mono(
                        ui,
                        &format!(
                            "Frame {playhead} / {frames}  ·  {} %",
                            (eased * 100.).round() as i32
                        ),
                    );
                });
                action = footer(ui, draft.dirty);
            });
        });
        if let Some(target) = picked_target {
            draft.target = target;
            if target.is_some() {
                draft.one_curve_for_all = false;
            }
        }
        if let Some(one_curve) = toggled_one_curve {
            draft.one_curve_for_all = one_curve;
            if one_curve {
                draft.target = None;
                draft.property_eases.clear();
            }
            draft.recompute_dirty();
        }
        if let Some(easing) = new_easing.filter(|e| *e != draft.active_easing()) {
            if let Some(prop) = draft.target {
                draft.one_curve_for_all = false;
                draft.set_property_override(prop, easing);
            } else {
                draft.easing = easing;
                if draft.one_curve_for_all {
                    draft.property_eases.clear();
                }
            }
            draft.recompute_dirty();
        }
        match action {
            Some(FooterAction::Reset) => {
                if let Some(prop) = draft.target {
                    if draft.easing == Easing::Linear {
                        draft.property_eases.retain(|(p, _)| *p != prop);
                    } else {
                        draft.set_property_override(prop, Easing::Linear);
                    }
                } else {
                    draft.easing = Easing::Linear;
                    if draft.one_curve_for_all {
                        draft.property_eases.clear();
                    }
                }
                draft.recompute_dirty();
            }
            Some(FooterAction::Cancel) => {
                draft.easing = draft.span.easing;
                draft.property_eases = draft.span.property_eases.clone();
                draft.one_curve_for_all =
                    draft.property_eases.is_empty() && draft.target.is_none();
                draft.dirty = false;
            }
            Some(FooterAction::Apply) => {
                if let Some(span) = &span
                    && self.set_tween_eases(span, draft.easing, draft.property_eases.clone())
                {
                    draft.span.easing = draft.easing;
                    draft.span.property_eases = draft.property_eases.clone();
                    draft.dirty = false;
                }
            }
            None => {}
        }
        ctx.data_mut(|d| d.insert_temp(draft_id(), draft));
    }
}

/// Drag the two Bézier handles drawn by `NF2/Ease Graph` (same plot geometry as the component).
fn drag_handles(ui: &mut Ui, graph: &egui::Response, bezier: [f32; 4]) -> Option<Easing> {
    if !ui.is_enabled() {
        return None;
    }
    let rect = graph.rect;
    let plot = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 40.0, rect.top() + space::S2),
        egui::pos2(rect.right() - space::S2, rect.bottom() - 24.0),
    );
    let (lo, hi) = match range_of(bezier) {
        c::EaseRange::Normal => (0.0_f32, 1.0_f32),
        _ => (-0.3, 1.3),
    };
    let y_of = |v: f32| plot.bottom() - (v - lo) / (hi - lo) * plot.height();
    let x_of = |t: f32| plot.left() + t * plot.width();
    let mut result = None;
    for (index, (x, y)) in [(bezier[0], bezier[1]), (bezier[2], bezier[3])]
        .into_iter()
        .enumerate()
    {
        let handle =
            egui::Rect::from_center_size(egui::pos2(x_of(x), y_of(y)), egui::vec2(16., 16.));
        let response = ui.interact(
            handle,
            graph.id.with(("ease-handle", index)),
            egui::Sense::drag(),
        );
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        if response.dragged()
            && let Some(pointer) = response.interact_pointer_pos()
        {
            let t = ((pointer.x - plot.left()) / plot.width()).clamp(0., 1.);
            let v = (lo + (plot.bottom() - pointer.y) / plot.height() * (hi - lo))
                .clamp(*HANDLE_Y.start(), *HANDLE_Y.end());
            let mut b = bezier;
            b[index * 2] = (t * 100.).round() / 100.;
            b[index * 2 + 1] = (v * 100.).round() / 100.;
            let [x1, y1, x2, y2] = b.map(f64::from);
            result = Some(Easing::CubicBezier { x1, y1, x2, y2 });
        }
    }
    result
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FooterAction {
    Reset,
    Cancel,
    Apply,
}

/// Panel footer of the preview (`planned::footer`): Reset ghost left, Cancel · Apply right.
fn footer(ui: &mut Ui, dirty: bool) -> Option<FooterAction> {
    let nf = neoflash_ui::palette(ui.ctx());
    let mut action = None;
    ui.add_space(space::S2);
    let r = ui.available_rect_before_wrap();
    ui.painter().hline(
        r.x_range(),
        r.top() - space::S1,
        egui::Stroke::new(1.0_f32, nf.border_subtle),
    );
    ui.add_space(space::S2);
    ui.horizontal(|ui| {
        if ui.add(c::Button::ghost("Reset")).clicked() {
            action = Some(FooterAction::Reset);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add_enabled(dirty, c::Button::primary("Apply")).clicked() {
                action = Some(FooterAction::Apply);
            }
            if ui.add_enabled(dirty, c::Button::new("Cancel")).clicked() {
                action = Some(FooterAction::Cancel);
            }
        });
    });
    action
}
