//! Motion editor (NF2 `Panel / Motion editor`, PP-02; reference `motion-panels.png`): one row per
//! tweened property of the motion tween under the playhead — value at the playhead, ease, ◀ ◆ ▶ key
//! navigation — and a curve lane through the property's keys over the span.
//!
//! ◆ toggles the property's key at the playhead; the value field keys the property there; dragging a
//! key on the selected (expanded) lane edits its value and frame (◆ interior keys); clicking a lane
//! moves the playhead; the Ease column edits per-property ease overrides. Each change is one Undo
//! step (one per drag).
use super::planned::caption;
use crate::app::Editor;
use crate::theme::ScrollTheme as _;
use crate::workspace::PanelId;
use eframe::egui::{self, Ui};
use neoflash_core::*;
use neoflash_ui::components as c;
use neoflash_ui::icons::{i16, i20};
use neoflash_ui::metrics;
use neoflash_ui::tokens::{TextStyle, space};
use neoflash_ui::typography::rich;

enum MotionRow {
    Section(&'static str),
    Property {
        name: &'static str,
        property: TweenProperty,
        unit: &'static str,
        decimals: usize,
    },
}

const MOTION_ROWS: &[MotionRow] = &[
    MotionRow::Section("Basic motion"),
    MotionRow::Property {
        name: "X",
        property: TweenProperty::X,
        unit: "px",
        decimals: 1,
    },
    MotionRow::Property {
        name: "Y",
        property: TweenProperty::Y,
        unit: "px",
        decimals: 1,
    },
    MotionRow::Property {
        name: "Rotation Z",
        property: TweenProperty::Rotation,
        unit: "°",
        decimals: 0,
    },
    MotionRow::Section("Transformation"),
    MotionRow::Property {
        name: "Scale X",
        property: TweenProperty::ScaleX,
        unit: "%",
        decimals: 0,
    },
    MotionRow::Property {
        name: "Scale Y",
        property: TweenProperty::ScaleY,
        unit: "%",
        decimals: 0,
    },
    MotionRow::Section("Colour effect"),
    MotionRow::Property {
        name: "Alpha",
        property: TweenProperty::Alpha,
        unit: "%",
        decimals: 0,
    },
    MotionRow::Section("Filters"),
];

/// A key being dragged on the selected lane, with the value range captured at the press.
#[derive(Clone, Copy, Debug, PartialEq)]
struct LaneDrag {
    property: TweenProperty,
    frame: u32,
    range: (f64, f64),
}

/// View state of the Motion editor (egui temp memory, never saved).
#[derive(Clone, Copy, Debug, PartialEq)]
struct MotionEditorState {
    selected: TweenProperty,
    graph_size: f64,
    expanded: f64,
    drag: Option<LaneDrag>,
}

impl Default for MotionEditorState {
    fn default() -> Self {
        Self {
            selected: TweenProperty::X,
            graph_size: 55.,
            expanded: 70.,
            drag: None,
        }
    }
}

fn state_id() -> egui::Id {
    egui::Id::new("nf2-motion-editor")
}

/// Name of an ease in the Ease column.
fn ease_name(easing: Easing) -> &'static str {
    match easing {
        Easing::Linear => "No ease",
        Easing::Flash { acceleration: 0. } => "No ease",
        Easing::Flash { .. } => "Simple",
        Easing::EaseIn => "Ease in",
        Easing::EaseOut => "Ease out",
        Easing::EaseInOut => "Ease in-out",
        Easing::Bounce => "Bounce",
        Easing::Elastic => "Elastic",
        Easing::CubicBezier { x1, y1, x2, y2 } if [x1, y1, x2, y2] == super::ease::BACK_OUT => {
            "Back"
        }
        Easing::CubicBezier { .. } => "Custom",
    }
}

/// Lane value range of a property: its key values, or a default span around a flat track.
fn lane_range(property: TweenProperty, values: &[f64]) -> (f64, f64) {
    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if !min.is_finite() {
        return (0., 1.);
    }
    if max - min > 1e-9 {
        return (min, max);
    }
    let half = match property {
        TweenProperty::Rotation | TweenProperty::Skew => 45.,
        _ => 50.,
    };
    (min - half, max + half)
}

impl Editor {
    /// Window ▸ Motion editor.
    pub(crate) fn motion_editor_panel_ui(&mut self, ui: &mut Ui) {
        self.panel_title(ui, PanelId::MotionEditor, "Motion editor");
        let ctx = ui.ctx().clone();
        egui::ScrollArea::both()
            .id_salt("motion-editor-content")
            .auto_shrink([false, false])
            .themed()
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(space::S2, space::S2);
                self.motion_editor(ui);
            });
        self.end_property_interaction(&ctx);
    }

    /// Panel / Motion editor body: property column (448) + one curve lane per property over the span.
    fn motion_editor(&mut self, ui: &mut Ui) {
        const LEFT: f32 = 448.0;
        let Some(span) = self.motion_tween_span() else {
            c::EmptyState {
                icon: i20::FILM,
                title: "No motion tween",
                body: "Move the playhead into a motion tween to edit its property curves.",
                action: None,
            }
            .show(ui);
            return;
        };
        let ctx = ui.ctx().clone();
        let mut state: MotionEditorState = ctx.data(|d| d.get_temp(state_id())).unwrap_or_default();
        let frames = span.frames();
        let editable = self.layer_editable();
        let nf = neoflash_ui::palette(ui.ctx());
        ui.set_min_width(LEFT + 240.);
        ui.spacing_mut().item_spacing.y = 0.0;
        let width = ui.available_width();
        let top = ui.cursor().min.y;
        let (header, _) =
            ui.allocate_exact_size(egui::vec2(width, metrics::ROW), egui::Sense::hover());
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
        c::paint_frame_scale(ui.painter(), ui.ctx(), scale, frames, 5);
        let collapsed_h =
            (metrics::CONTROL + space::S2) * (1. + ((state.graph_size - 55.).max(0.) / 45.) as f32);
        let expanded_h = (120. * state.expanded / 70.).clamp(f64::from(collapsed_h), 240.) as f32;
        let last = frames.saturating_sub(1).max(1) as f32;
        let mut seek = None;
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
                                .id_salt(("motion-editor", *label))
                                .show(ui, |_| {});
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // Every track of a motion tween is always present; nothing to add yet.
                            let add = ui.add_enabled(
                                false,
                                c::IconButton::new(i16::PLUS, "Add property").side(24.0),
                            );
                            c::not_available(add, "Add property");
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
                    property,
                    unit,
                    decimals,
                } => {
                    let property = *property;
                    let selected = state.selected == property;
                    let h = if selected { expanded_h } else { collapsed_h };
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(width, h), egui::Sense::hover());
                    let lane = egui::Rect::from_min_max(egui::pos2(lanes_x, rect.top()), rect.max);
                    let key_frames = self.motion_key_frames(&span, property);
                    let values: Vec<(u32, f64)> = key_frames
                        .iter()
                        .filter_map(|f| self.motion_key_value(&span, property, *f).map(|v| (*f, v)))
                        .collect();
                    let range = state.drag.filter(|d| d.property == property).map_or_else(
                        || lane_range(property, &values.iter().map(|v| v.1).collect::<Vec<_>>()),
                        |d| d.range,
                    );
                    let norm = |v: f64| ((v - range.0) / (range.1 - range.0)) as f32;
                    let lane_keys: Vec<c::LaneKey> = values
                        .iter()
                        .map(|(frame, value)| c::LaneKey {
                            frame: (frame - span.start) as f32,
                            value: norm(*value),
                        })
                        .collect();
                    if selected {
                        ui.painter().rect_filled(rect, 0.0, nf.bg_selected);
                    }
                    c::paint_curve_lane(ui.painter(), ui.ctx(), lane, frames, &lane_keys, selected);
                    // Lane: click moves the playhead; on the selected lane a key drags its value.
                    let inner = lane.shrink2(egui::vec2(space::S3, space::S3));
                    let lane_response = ui.interact(
                        lane,
                        ui.id().with(("motion-lane", name)),
                        if editable {
                            egui::Sense::click_and_drag()
                        } else {
                            egui::Sense::click()
                        },
                    );
                    let frame_at = |x: f32| {
                        span.start
                            + (((x - inner.left()) / inner.width()).clamp(0., 1.) * last).round()
                                as u32
                    };
                    if lane_response.clicked()
                        && let Some(p) = lane_response.interact_pointer_pos()
                    {
                        state.selected = property;
                        seek = Some(frame_at(p.x).min(span.end - 1));
                    }
                    if lane_response.drag_started()
                        && let Some(p) = lane_response.interact_pointer_pos()
                    {
                        state.selected = property;
                        let hit = lane_keys.iter().zip(&values).find(|(k, _)| {
                            let x = inner.left() + k.frame / last * inner.width();
                            let y = inner.bottom() - k.value.clamp(0., 1.) * inner.height();
                            (x - p.x).abs() <= 8. && (y - p.y).abs() <= 8.
                        });
                        state.drag = hit.filter(|_| selected).map(|(_, (frame, _))| LaneDrag {
                            property,
                            frame: *frame,
                            range,
                        });
                    }
                    if let Some(drag) = state.drag.filter(|d| d.property == property) {
                        if lane_response.dragged()
                            && let Some(p) = lane_response.interact_pointer_pos()
                        {
                            let active_frame =
                                if drag.frame > span.start && span.end > span.start + 1 {
                                    let target = frame_at(p.x).clamp(span.start + 1, span.end - 1);
                                    if target != drag.frame {
                                        self.move_motion_key(
                                            &span,
                                            property,
                                            drag.frame,
                                            target,
                                            Some(lane_response.id),
                                        )
                                        .unwrap_or(drag.frame)
                                    } else {
                                        drag.frame
                                    }
                                } else {
                                    drag.frame
                                };
                            if let Some(d) = &mut state.drag {
                                d.frame = active_frame;
                            }
                            let t = f64::from((inner.bottom() - p.y) / inner.height());
                            let value = drag.range.0 + t * (drag.range.1 - drag.range.0);
                            self.set_motion_value(
                                &span,
                                property,
                                active_frame,
                                value,
                                Some(lane_response.id),
                            );
                        }
                        if !lane_response.dragged() && !ctx.input(|i| i.pointer.any_down()) {
                            state.drag = None;
                        }
                    }
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
                        let (label, label_response) = ui.allocate_exact_size(
                            egui::vec2(120.0, metrics::TOGGLE_ROW),
                            egui::Sense::click(),
                        );
                        if label_response.clicked() {
                            state.selected = property;
                        }
                        ui.painter().text(
                            label.left_center() + egui::vec2(space::S4, 0.0),
                            egui::Align2::LEFT_CENTER,
                            *name,
                            neoflash_ui::typography::font(TextStyle::Body),
                            nf.text_primary,
                        );
                        let mut value =
                            self.motion_value(&span, property, self.frame).unwrap_or(0.);
                        let range = match property {
                            TweenProperty::Alpha => 0.0..=100.0,
                            _ => -1e6..=1e6,
                        };
                        let response = ui.add_enabled(
                            editable,
                            c::Field::new(&mut value, name)
                                .unit(unit)
                                .decimals(*decimals)
                                .range(range)
                                .width(96.0),
                        );
                        if response.changed() {
                            self.set_motion_value(
                                &span,
                                property,
                                self.frame,
                                value,
                                Some(response.id),
                            );
                        }
                        let override_ease = span.property_override(property);
                        let shown_ease = override_ease.unwrap_or(span.easing);
                        let mut picked_ease: Option<Option<Easing>> = None;
                        let mut open_curve = false;
                        ui.add_enabled_ui(editable, |ui| {
                            c::Select::new(("motion-editor-ease", *name), ease_name(shown_ease))
                                .width(120.0)
                                .show(ui, |ui| {
                                    if ui
                                        .add(
                                            c::MenuItem::new("Same as span")
                                                .checked(override_ease.is_none()),
                                        )
                                        .clicked()
                                    {
                                        picked_ease = Some(None);
                                        ui.close();
                                    }
                                    c::menu_separator(ui);
                                    let [bx1, by1, bx2, by2] = super::ease::BACK_OUT;
                                    for (choice, label) in [
                                        (Easing::Linear, "No ease"),
                                        (Easing::EaseIn, "Ease in"),
                                        (Easing::EaseOut, "Ease out"),
                                        (Easing::EaseInOut, "Ease in-out"),
                                        (
                                            Easing::CubicBezier {
                                                x1: bx1,
                                                y1: by1,
                                                x2: bx2,
                                                y2: by2,
                                            },
                                            "Back",
                                        ),
                                        (Easing::Bounce, "Bounce"),
                                        (Easing::Elastic, "Elastic"),
                                    ] {
                                        if ui
                                            .add(
                                                c::MenuItem::new(label)
                                                    .checked(override_ease == Some(choice)),
                                            )
                                            .clicked()
                                        {
                                            picked_ease = Some(Some(choice));
                                            ui.close();
                                        }
                                    }
                                    c::menu_separator(ui);
                                    if ui.add(c::MenuItem::new("Edit curve…")).clicked() {
                                        open_curve = true;
                                        ui.close();
                                    }
                                });
                        });
                        if let Some(choice) = picked_ease {
                            state.selected = property;
                            self.set_motion_property_ease(&span, property, choice);
                        }
                        if open_curve {
                            state.selected = property;
                            self.open_ease_for_property(&ctx, property);
                        }
                        let previous = key_frames.iter().rev().find(|f| **f < self.frame).copied();
                        let next = key_frames.iter().find(|f| **f > self.frame).copied();
                        if ui
                            .add_enabled(
                                previous.is_some(),
                                c::IconButton::new(i16::STEPBACK, "Previous keyframe").side(24.0),
                            )
                            .clicked()
                        {
                            seek = previous;
                        }
                        let (diamond, diamond_response) =
                            ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::click());
                        let keyed_here = key_frames.contains(&self.frame);
                        c::motion::paint_key_diamond(
                            ui.painter(),
                            diamond.center(),
                            if keyed_here {
                                nf.accent_default
                            } else {
                                nf.text_tertiary
                            },
                        );
                        diamond_response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                editable,
                                "Add or remove keyframe",
                            )
                        });
                        if editable && diamond_response.clicked() {
                            state.selected = property;
                            self.toggle_motion_key(&span, property);
                        }
                        if ui
                            .add_enabled(
                                next.is_some(),
                                c::IconButton::new(i16::STEPFWD, "Next keyframe").side(24.0),
                            )
                            .clicked()
                        {
                            seek = next;
                        }
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
        let playhead = lanes_x
            + space::S3
            + self.frame.saturating_sub(span.start) as f32 / last
                * (width - LEFT - 2.0 * space::S3);
        c::paint_lane_playhead(
            ui.painter(),
            ui.ctx(),
            playhead,
            egui::Rect::from_min_max(egui::pos2(lanes_x, top), scale.max),
            bottom,
            self.frame + 1,
        );
        ui.add_space(space::S3);
        ui.spacing_mut().item_spacing = egui::vec2(space::S3, space::S2);
        ui.horizontal(|ui| {
            caption(ui, "Graph size");
            ui.add(c::Slider::new(&mut state.graph_size, 0.0..=100.0, "Graph size").width(120.0));
            caption(ui, "Expanded");
            ui.add(
                c::Slider::new(&mut state.expanded, 0.0..=100.0, "Expanded graph size")
                    .width(120.0),
            );
            // The span length is set on the Timeline; the field shows it.
            let mut shown = f64::from(frames);
            let response = ui.add_enabled(
                false,
                c::Field::new(&mut shown, "Frames")
                    .prefix("Frames")
                    .decimals(0)
                    .stepper()
                    .width(120.0),
            );
            c::not_available(response, "Frames");
            ui.label(
                rich(
                    "Click a curve to edit · ⌥-drag a keyframe to copy · ⇧ snaps to frames",
                    TextStyle::Small,
                )
                .color(nf.text_secondary),
            );
        });
        ctx.data_mut(|d| d.insert_temp(state_id(), state));
        if let Some(frame) = seek {
            self.seek_frame(frame);
        }
    }
}
