//! Preserve macOS pen pressure alongside the pointer events delivered by winit.
//!
//! AppKit tablet mouse events carry pressure, but egui-winit currently forwards only their
//! pointer position/buttons. A local, root-window-only monitor records each event's own sample.
//! It never consumes or rewrites a native event. The raw-input hook inserts supplemental Touch
//! events immediately before their matching pointer events; ordinary mouse/trackpad input stays
//! unchanged. Authoring owns how pressure changes a brush, not this platform adapter.
use eframe::egui::{self, Event, Pos2, TouchDeviceId, TouchId, TouchPhase};
use std::cell::RefCell;
use std::rc::Rc;

const DEVICE: TouchDeviceId = TouchDeviceId(0x4e46_5441_424c_4554);
const MAX_SAMPLES: usize = 8192;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum TabletPointerKind {
    #[default]
    PenTip,
    Eraser,
}

/// Maps normalized tablet tilt (`tilt_x`, `tilt_y` in `-1.0..=1.0`) to a calligraphic nib
/// azimuth angle in radians (`[-PI, PI]`). Returns `None` when the stylus is upright or inputs
/// are non-finite.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn tilt_to_nib_angle_rad(tilt_x: f32, tilt_y: f32) -> Option<f32> {
    if !tilt_x.is_finite() || !tilt_y.is_finite() {
        return None;
    }
    let tx = tilt_x.clamp(-1.0, 1.0);
    let ty = tilt_y.clamp(-1.0, 1.0);
    if tx.hypot(ty) < 1e-4 {
        return None;
    }
    Some(ty.atan2(tx))
}

/// Maps normalized tablet tilt (`tilt_x`, `tilt_y` in `-1.0..=1.0`) to a nib angle in degrees
/// (`[0.0, 360.0)`). Returns `None` when the stylus is upright or inputs are non-finite.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn tilt_to_nib_angle_deg(tilt_x: f32, tilt_y: f32) -> Option<f32> {
    tilt_to_nib_angle_rad(tilt_x, tilt_y).map(|rad| rad.to_degrees().rem_euclid(360.0))
}

/// Maps normalized tablet tilt (`tilt_x`, `tilt_y`) to the nearest NF2 calligraphic brush nib
/// orientation, falling back to `BrushShapeKind::Round` when the pen is held upright.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn tilt_to_brush_shape(
    tilt_x: f32,
    tilt_y: f32,
) -> neoflash_ui::components::BrushShapeKind {
    use neoflash_ui::components::BrushShapeKind;
    let Some(deg) = tilt_to_nib_angle_deg(tilt_x, tilt_y) else {
        return BrushShapeKind::Round;
    };
    let folded = deg.rem_euclid(180.0);
    if !(22.5..157.5).contains(&folded) {
        BrushShapeKind::OvalH
    } else if folded < 67.5 {
        BrushShapeKind::Slash
    } else if folded < 112.5 {
        BrushShapeKind::OvalV
    } else {
        BrushShapeKind::Backslash
    }
}

#[derive(Clone, Copy, Debug)]
struct Sample {
    id: TouchId,
    phase: TouchPhase,
    /// Top-left content-view coordinates, in native logical points (before egui UI zoom).
    position: Pos2,
    force: f32,
    #[cfg_attr(not(test), allow(dead_code))]
    tilt: [f32; 2],
    #[cfg_attr(not(test), allow(dead_code))]
    pointer_kind: TabletPointerKind,
}

#[derive(Default)]
struct Samples {
    queued: Vec<Sample>,
    contact: Option<Sample>,
    next_id: u64,
}

impl Samples {
    fn record(&mut self, phase: TouchPhase, position: Pos2, force: f32) {
        self.record_with_pen_state(
            phase,
            position,
            force,
            [0.0, 0.0],
            TabletPointerKind::PenTip,
        );
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn record_with_pen_state(
        &mut self,
        phase: TouchPhase,
        position: Pos2,
        force: f32,
        tilt: [f32; 2],
        pointer_kind: TabletPointerKind,
    ) {
        if !position.is_finite()
            || !force.is_finite()
            || !tilt[0].is_finite()
            || !tilt[1].is_finite()
        {
            return;
        }
        let id = if phase == TouchPhase::Start {
            self.next_id = self.next_id.wrapping_add(1);
            TouchId(self.next_id)
        } else if let Some(contact) = self.contact {
            contact.id
        } else {
            // A pen already held down when the monitor was installed must not begin a gesture.
            return;
        };
        let sample = Sample {
            id,
            phase,
            position,
            force: force.clamp(0., 1.),
            tilt: [tilt[0].clamp(-1., 1.), tilt[1].clamp(-1., 1.)],
            pointer_kind,
        };
        if self.queued.len() >= MAX_SAMPLES {
            self.abort(sample);
            return;
        }
        self.contact = match phase {
            TouchPhase::Start | TouchPhase::Move => Some(sample),
            TouchPhase::End | TouchPhase::Cancel => None,
        };
        self.queued.push(sample);
    }

    fn cancel(&mut self) {
        // The final mouse-up may already be queued when focus changes, so contact can be None
        // although the authoring gesture has not received End yet.
        let pending = self.contact.take().or_else(|| self.queued.last().copied());
        if let Some(sample) = pending {
            self.abort(sample);
        }
    }

    fn abort(&mut self, sample: Sample) {
        // A queued Start has not reached authoring yet. Keep it so Cancel follows its matching
        // pointer-down; otherwise the remaining raw pointer stream would become mouse ink.
        let start = if sample.phase == TouchPhase::Start {
            Some(sample)
        } else {
            self.queued
                .iter()
                .find(|pending| pending.id == sample.id && pending.phase == TouchPhase::Start)
                .copied()
        };
        self.queued.clear();
        self.queued.extend(start);
        self.queued.push(Sample {
            phase: TouchPhase::Cancel,
            ..sample
        });
        self.contact = None;
    }
}

/// Owned by the editor so dropping it removes the AppKit monitor on the main thread.
/// Rc makes the bridge !Send/!Sync, just like the AppKit monitor itself.
#[derive(Default)]
pub(crate) struct TabletInput {
    samples: Rc<RefCell<Samples>>,
    forwarded_contact: Option<Sample>,
    #[cfg(all(target_os = "macos", not(test)))]
    _monitor: Option<native::Monitor>,
}

impl TabletInput {
    /// Install during Editor::new, while only the main editor window exists.
    pub(crate) fn install() -> Self {
        let input = Self::default();
        #[cfg(all(target_os = "macos", not(test)))]
        let input = Self {
            _monitor: native::install(Rc::clone(&input.samples)),
            ..input
        };
        input
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn active_tilt(&self) -> Option<[f32; 2]> {
        self.forwarded_contact
            .or_else(|| self.samples.borrow().contact)
            .map(|c| c.tilt)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn active_nib_angle_deg(&self) -> Option<f32> {
        let [tx, ty] = self.active_tilt()?;
        tilt_to_nib_angle_deg(tx, ty)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn active_pointer_kind(&self) -> TabletPointerKind {
        self.forwarded_contact
            .or_else(|| self.samples.borrow().contact)
            .map_or(TabletPointerKind::PenTip, |c| c.pointer_kind)
    }

    /// Maps the tablet's active pointer end (`PenTip` vs `Eraser`) to the matching drawing tool.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn tool_for_pointer_kind(kind: TabletPointerKind) -> crate::app::Tool {
        match kind {
            TabletPointerKind::PenTip => crate::app::Tool::Brush,
            TabletPointerKind::Eraser => crate::app::Tool::Eraser,
        }
    }

    /// Called from eframe::App::raw_input_hook, before egui processes this frame's input.
    pub(crate) fn append_events(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        if raw.viewport_id != egui::ViewportId::ROOT {
            return;
        }
        let mut samples = self.samples.borrow_mut();
        if !raw.focused {
            samples.cancel();
        }
        let pending = std::mem::take(&mut samples.queued);
        drop(samples);
        insert_samples(
            &mut raw.events,
            &pending,
            ctx.zoom_factor(),
            &mut self.forwarded_contact,
        );
    }
}

fn touch(sample: Sample, zoom: f32) -> Event {
    Event::Touch {
        device_id: DEVICE,
        id: sample.id,
        phase: sample.phase,
        pos: Pos2::ZERO + sample.position.to_vec2() / zoom,
        force: Some(sample.force),
    }
}

fn matches_pointer(sample: Sample, event: &Event, zoom: f32) -> bool {
    let position = match (sample.phase, event) {
        (TouchPhase::Move, Event::PointerMoved(position)) => position,
        (
            TouchPhase::Start | TouchPhase::End,
            Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                ..
            },
        ) if *pressed == (sample.phase == TouchPhase::Start) => pos,
        _ => return false,
    };
    let sample_position = Pos2::ZERO + sample.position.to_vec2() / zoom;
    // Winit converts through physical pixels and f32; permit only that small rounding difference.
    position.distance(sample_position) <= 0.25 / zoom
}

fn insert_samples(
    events: &mut Vec<Event>,
    samples: &[Sample],
    zoom: f32,
    forwarded_contact: &mut Option<Sample>,
) {
    if !zoom.is_finite() || zoom <= 0. {
        return;
    }
    let original = std::mem::take(events);
    let mut cursor = 0;
    for sample in samples {
        if sample.phase == TouchPhase::Cancel {
            events.push(touch(*sample, zoom));
            continue;
        }
        // A local monitor can see an event intercepted by another native modal handler. Do not
        // invent a pointer gesture or associate its pressure with an unrelated later mouse click.
        let Some(offset) = original[cursor..]
            .iter()
            .position(|event| matches_pointer(*sample, event, zoom))
        else {
            if sample.phase == TouchPhase::End {
                // Winit can omit mouse-up after CursorLeft cleared its pointer position. A pen
                // release without the matching pointer release must abort, never leave ink live.
                events.push(touch(
                    Sample {
                        phase: TouchPhase::Cancel,
                        ..*sample
                    },
                    zoom,
                ));
            }
            continue;
        };
        let index = cursor + offset;
        events.extend(original[cursor..index].iter().cloned());
        events.push(touch(*sample, zoom));
        events.push(original[index].clone());
        cursor = index + 1;
    }
    events.extend(original[cursor..].iter().cloned());

    // PointerGone can arrive without a native release at all (e.g. window focus/dispatch changes).
    // Cancel only the pen that this adapter actually forwarded; ordinary mouse input is untouched.
    let merged = std::mem::take(events);
    for event in merged {
        if let Event::Touch {
            device_id,
            id,
            phase,
            pos,
            force,
        } = &event
            && *device_id == DEVICE
        {
            match phase {
                TouchPhase::Start | TouchPhase::Move => {
                    if *phase == TouchPhase::Start
                        || forwarded_contact.is_some_and(|contact| contact.id == *id)
                    {
                        let matched = samples
                            .iter()
                            .rfind(|s| s.id == *id && s.phase == *phase)
                            .copied();
                        *forwarded_contact = Some(Sample {
                            id: *id,
                            phase: *phase,
                            position: *pos * zoom,
                            force: force.unwrap_or(1.),
                            tilt: matched
                                .or(*forwarded_contact)
                                .map_or([0.0, 0.0], |s| s.tilt),
                            pointer_kind: matched
                                .or(*forwarded_contact)
                                .map_or(TabletPointerKind::PenTip, |s| s.pointer_kind),
                        });
                    }
                }
                TouchPhase::End | TouchPhase::Cancel => {
                    if forwarded_contact.is_some_and(|contact| contact.id == *id) {
                        *forwarded_contact = None;
                    }
                }
            }
        }
        if matches!(event, Event::PointerGone)
            && let Some(sample) = forwarded_contact.take()
        {
            events.push(touch(
                Sample {
                    phase: TouchPhase::Cancel,
                    ..sample
                },
                zoom,
            ));
        }
        events.push(event);
    }
}

#[cfg(all(target_os = "macos", not(test)))]
mod native {
    use super::*;
    use block2::RcBlock;
    use objc2::{MainThreadMarker, rc::Retained, runtime::AnyObject};
    use objc2_app_kit::{NSApplication, NSEvent, NSEventMask, NSEventSubtype, NSEventType};
    use std::ptr::NonNull;

    pub(super) struct Monitor {
        token: Retained<AnyObject>,
        // Enforces main-thread ownership, including removal in Drop.
        _main_thread: MainThreadMarker,
    }

    impl Drop for Monitor {
        fn drop(&mut self) {
            // SAFETY: token is exactly the retained object returned by addLocalMonitor below.
            unsafe { NSEvent::removeMonitor(&self.token) };
        }
    }

    pub(super) fn install(samples: Rc<RefCell<Samples>>) -> Option<Monitor> {
        let main_thread = MainThreadMarker::new()?;
        let app = NSApplication::sharedApplication(main_thread);
        let root = app.windows().iter().next()?;
        let root_number = root.windowNumber();
        let handler = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
            // SAFETY: AppKit supplies a live NSEvent for this synchronous local callback.
            let native = unsafe { event.as_ref() };
            if native.windowNumber() == root_number
                && native.subtype() == NSEventSubtype::TabletPoint
            {
                let phase = match native.r#type() {
                    NSEventType::LeftMouseDown => Some(TouchPhase::Start),
                    NSEventType::LeftMouseDragged => Some(TouchPhase::Move),
                    NSEventType::LeftMouseUp => Some(TouchPhase::End),
                    _ => None,
                };
                if let Some(phase) = phase
                    && let Some(window) = native.window(main_thread)
                    && let Some(view) = window.contentView()
                {
                    let point = view.convertPoint_fromView(native.locationInWindow(), None);
                    let bounds = view.bounds();
                    let y = if view.isFlipped() {
                        point.y - bounds.origin.y
                    } else {
                        bounds.origin.y + bounds.size.height - point.y
                    };
                    let position = egui::pos2((point.x - bounds.origin.x) as f32, y as f32);
                    samples
                        .borrow_mut()
                        .record(phase, position, native.pressure());
                }
            }
            // Pass the original event back unchanged; winit still owns pointer/button handling.
            event.as_ptr()
        });
        let mask =
            NSEventMask::LeftMouseDown | NSEventMask::LeftMouseDragged | NSEventMask::LeftMouseUp;
        // SAFETY: the block returns its original live NSEvent pointer, never a borrowed temporary.
        // AppKit copies the block; Monitor owns and removes its returned registration token.
        let token =
            unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &handler) }?;
        Some(Monitor {
            token,
            _main_thread: main_thread,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pointer(position: Pos2, pressed: bool) -> Event {
        Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    fn pen_samples_keep_each_pressure_position_and_precede_their_pointer_events() {
        let mut samples = Samples::default();
        let a = egui::pos2(20., 40.);
        let b = egui::pos2(22., 44.);
        samples.record(TouchPhase::Start, a, 0.2);
        samples.record(TouchPhase::Move, b, 0.8);
        // Stationary pressure changes are separate samples, not last-event pressure broadcast.
        samples.record(TouchPhase::Move, b, 0.4);
        samples.record(TouchPhase::End, b, 0.);
        let a = a / 2.;
        let b = b / 2.;
        let original = vec![
            Event::PointerMoved(a),
            pointer(a, true),
            Event::PointerMoved(b),
            Event::PointerMoved(b),
            Event::PointerMoved(b),
            pointer(b, false),
        ];
        let mut events = original.clone();
        insert_samples(&mut events, &samples.queued, 2., &mut None);
        let touches: Vec<_> = events
            .iter()
            .enumerate()
            .filter_map(|(index, event)| match event {
                Event::Touch {
                    id,
                    phase,
                    pos,
                    force,
                    ..
                } => Some((index, *id, *phase, *pos, *force)),
                _ => None,
            })
            .collect();
        assert_eq!(touches.len(), 4);
        assert_eq!(
            touches.iter().map(|t| t.4.unwrap()).collect::<Vec<_>>(),
            [0.2, 0.8, 0.4, 0.]
        );
        assert!(touches.iter().all(|t| t.1 == touches[0].1));
        for (index, _, phase, pos, _) in touches {
            match phase {
                TouchPhase::Start => assert_eq!(events[index + 1], pointer(pos, true)),
                TouchPhase::Move => assert_eq!(events[index + 1], Event::PointerMoved(pos)),
                TouchPhase::End => assert_eq!(events[index + 1], pointer(pos, false)),
                TouchPhase::Cancel => panic!("unexpected cancellation"),
            }
        }
        events.retain(|event| !matches!(event, Event::Touch { .. }));
        assert_eq!(
            events, original,
            "supplementing pressure must preserve mouse input"
        );
    }

    #[test]
    fn unmatched_mouse_input_stays_unchanged_and_focus_loss_cancels_the_pen() {
        let mut input = TabletInput::default();
        input
            .samples
            .borrow_mut()
            .record(TouchPhase::Start, egui::pos2(10., 20.), 0.5);
        let ordinary = vec![pointer(egui::pos2(100., 200.), true)];
        let mut raw = egui::RawInput {
            events: ordinary.clone(),
            ..Default::default()
        };
        input.append_events(&egui::Context::default(), &mut raw);
        assert_eq!(raw.events, ordinary);

        input
            .samples
            .borrow_mut()
            .record(TouchPhase::Move, egui::pos2(12., 22.), 0.8);
        let mut floating = egui::RawInput {
            viewport_id: egui::ViewportId::from_hash_of("floating"),
            ..Default::default()
        };
        input.append_events(&egui::Context::default(), &mut floating);
        assert!(floating.events.is_empty());
        let mut lost_focus = egui::RawInput {
            focused: false,
            ..Default::default()
        };
        input.append_events(&egui::Context::default(), &mut lost_focus);
        assert!(matches!(
            lost_focus.events.as_slice(),
            [Event::Touch {
                phase: TouchPhase::Cancel,
                ..
            }]
        ));
        assert!(input.samples.borrow().contact.is_none());
    }

    #[test]
    fn invalid_or_overflowed_samples_never_finish_a_partial_stroke() {
        let mut samples = Samples::default();
        samples.record(TouchPhase::Move, Pos2::ZERO, 0.5);
        samples.record(TouchPhase::Start, Pos2::ZERO, f32::NAN);
        assert!(samples.queued.is_empty());
        samples.record(TouchPhase::Start, Pos2::ZERO, 0.5);
        for _ in 0..MAX_SAMPLES {
            samples.record(TouchPhase::Move, Pos2::ZERO, 0.5);
        }
        assert_eq!(samples.queued.len(), 2);
        assert_eq!(samples.queued[0].phase, TouchPhase::Start);
        assert_eq!(samples.queued[1].phase, TouchPhase::Cancel);
        samples.record(TouchPhase::End, Pos2::ZERO, 0.);
        assert_eq!(samples.queued.len(), 2);
        assert!(samples.contact.is_none());

        let original = vec![
            pointer(Pos2::ZERO, true),
            Event::PointerMoved(egui::pos2(20., 0.)),
            pointer(egui::pos2(20., 0.), false),
            // A separate ordinary mouse gesture after the aborted pen must remain untouched.
            pointer(egui::pos2(100., 100.), true),
            pointer(egui::pos2(100., 100.), false),
        ];
        let mut events = original.clone();
        let mut forwarded = None;
        insert_samples(&mut events, &samples.queued, 1., &mut forwarded);
        assert!(matches!(
            events[0],
            Event::Touch {
                phase: TouchPhase::Start,
                ..
            }
        ));
        assert_eq!(
            events[1], original[0],
            "native down must establish the pen before cancellation"
        );
        assert!(matches!(
            events[2],
            Event::Touch {
                phase: TouchPhase::Cancel,
                ..
            }
        ));
        assert!(forwarded.is_none());
        events.retain(|event| !matches!(event, Event::Touch { .. }));
        assert_eq!(events, original);

        samples.record(TouchPhase::Start, Pos2::ZERO, 0.5);
        samples.record(TouchPhase::End, Pos2::ZERO, 0.);
        samples.cancel();
        assert_eq!(samples.queued.len(), 2);
        assert_eq!(samples.queued[1].phase, TouchPhase::Cancel);

        // Drive the real authoring loop too: overflow may not fall back to a committed mouse
        // brush stroke merely because its native Start and Cancel arrived in the same frame.
        let (mut editor, ctx, page) = crate::tests::rectangle_stage();
        editor.set_tool(crate::app::Tool::Brush);
        let before = editor.project.clone();
        let start = page.min + egui::vec2(100., 100.);
        let end = start + egui::vec2(80., 20.);
        let mut input = TabletInput::default();
        input
            .samples
            .borrow_mut()
            .record(TouchPhase::Start, start, 0.5);
        for _ in 0..MAX_SAMPLES {
            input
                .samples
                .borrow_mut()
                .record(TouchPhase::Move, end, 0.8);
        }
        let mut raw = egui::RawInput {
            events: vec![
                Event::PointerMoved(start),
                pointer(start, true),
                Event::PointerMoved(end),
                pointer(end, false),
            ],
            ..Default::default()
        };
        input.append_events(&ctx, &mut raw);
        crate::tests::stage_input_frame(&mut editor, &ctx, 1., raw.events);
        assert_eq!(editor.project, before);
        assert!(!editor.history.can_undo());
        assert!(!editor.authoring_tools.is_drawing());
        editor.project.validate().unwrap();

        // A prior frame delivered Start. The native release either lacks a winit pointer-up,
        // or the window only reports PointerGone. Both must cancel the captured pen.
        for native_end in [false, true] {
            let mut input = TabletInput::default();
            input
                .samples
                .borrow_mut()
                .record(TouchPhase::Start, Pos2::ZERO, 0.5);
            let mut down = egui::RawInput {
                events: vec![pointer(Pos2::ZERO, true)],
                ..Default::default()
            };
            input.append_events(&egui::Context::default(), &mut down);
            assert!(input.forwarded_contact.is_some());
            if native_end {
                input
                    .samples
                    .borrow_mut()
                    .record(TouchPhase::End, Pos2::ZERO, 0.);
            }
            let mut gone = egui::RawInput {
                events: vec![Event::PointerGone],
                ..Default::default()
            };
            input.append_events(&egui::Context::default(), &mut gone);
            assert!(matches!(
                gone.events.as_slice(),
                [
                    Event::Touch {
                        phase: TouchPhase::Cancel,
                        ..
                    },
                    Event::PointerGone
                ]
            ));
            assert!(input.forwarded_contact.is_none());
            // PointerGone in an ordinary mouse frame does not gain a synthetic touch.
            let mut mouse = egui::RawInput {
                events: vec![Event::PointerGone],
                ..Default::default()
            };
            input.append_events(&egui::Context::default(), &mut mouse);
            assert_eq!(mouse.events, [Event::PointerGone]);
        }
    }

    fn path_y_span_near_x(
        commands: &[neoflash_core::PathCommand],
        origin_x: f64,
        origin_y: f64,
        target_x: f64,
        tolerance: f64,
    ) -> f64 {
        let mut min_y = f64::INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for cmd in commands {
            let (x, y) = match *cmd {
                neoflash_core::PathCommand::MoveTo { x, y }
                | neoflash_core::PathCommand::LineTo { x, y }
                | neoflash_core::PathCommand::QuadraticTo { x, y, .. }
                | neoflash_core::PathCommand::CubicTo { x, y, .. } => (origin_x + x, origin_y + y),
                neoflash_core::PathCommand::Close => continue,
            };
            if (x - target_x).abs() <= tolerance {
                min_y = min_y.min(y);
                max_y = max_y.max(y);
            }
        }
        if min_y.is_finite() && max_y.is_finite() {
            max_y - min_y
        } else {
            0.0
        }
    }

    fn path_max_x(commands: &[neoflash_core::PathCommand], origin_x: f64) -> f64 {
        let mut max_x = f64::NEG_INFINITY;
        for cmd in commands {
            match *cmd {
                neoflash_core::PathCommand::MoveTo { x, .. }
                | neoflash_core::PathCommand::LineTo { x, .. }
                | neoflash_core::PathCommand::QuadraticTo { x, .. }
                | neoflash_core::PathCommand::CubicTo { x, .. } => {
                    max_x = max_x.max(origin_x + x);
                }
                neoflash_core::PathCommand::Close => {}
            }
        }
        max_x
    }

    #[test]
    fn t235_tablet_pressure_tilt_consecutive_strokes_eraser_switch_undo_and_save_reopen() {
        use neoflash_ui::components::BrushShapeKind;

        // 1. Tilt-to-angle mapping (`tilt_x`, `tilt_y` -> nib angle and BrushShapeKind).
        assert_eq!(tilt_to_nib_angle_rad(0.0, 0.0), None);
        assert_eq!(tilt_to_nib_angle_deg(0.0, 0.0), None);
        assert_eq!(tilt_to_nib_angle_rad(f32::NAN, 0.5), None);
        assert_eq!(tilt_to_brush_shape(0.0, 0.0), BrushShapeKind::Round);

        let deg_h = tilt_to_nib_angle_deg(0.8, 0.0).expect("horizontal tilt angle");
        assert!((deg_h - 0.0).abs() < 1e-3);
        assert_eq!(tilt_to_brush_shape(0.8, 0.0), BrushShapeKind::OvalH);

        let deg_dr = tilt_to_nib_angle_deg(0.6, 0.6).expect("diagonal right tilt angle");
        assert!((deg_dr - 45.0).abs() < 1e-3);
        assert_eq!(tilt_to_brush_shape(0.6, 0.6), BrushShapeKind::Slash);

        let deg_v = tilt_to_nib_angle_deg(0.0, 0.9).expect("vertical tilt angle");
        assert!((deg_v - 90.0).abs() < 1e-3);
        assert_eq!(tilt_to_brush_shape(0.0, 0.9), BrushShapeKind::OvalV);

        let deg_dl = tilt_to_nib_angle_deg(-0.7, 0.7).expect("diagonal left tilt angle");
        assert!((deg_dl - 135.0).abs() < 1e-3);
        assert_eq!(tilt_to_brush_shape(-0.7, 0.7), BrushShapeKind::Backslash);

        // 2. Pressure retention across a multi-sample stroke (`pressure` in `0.05..=1.0`).
        let (mut editor, ctx, page) = crate::tests::rectangle_stage();
        editor.set_tool(crate::app::Tool::Brush);
        editor.authoring_tools.brush_pressure = true;
        editor.authoring_tools.brush_size = 24.0;
        let baseline_count = editor.symbol().timeline.layers[0].keyframes[0]
            .elements
            .len();

        let mut input = TabletInput::default();
        let stroke1_samples = [
            (
                TouchPhase::Start,
                page.min + egui::vec2(60.0, 120.0),
                0.05_f32,
                [0.5_f32, 0.5_f32],
            ),
            (
                TouchPhase::Move,
                page.min + egui::vec2(100.0, 120.0),
                0.25_f32,
                [0.5_f32, 0.5_f32],
            ),
            (
                TouchPhase::Move,
                page.min + egui::vec2(140.0, 120.0),
                0.60_f32,
                [0.5_f32, 0.5_f32],
            ),
            (
                TouchPhase::Move,
                page.min + egui::vec2(180.0, 120.0),
                1.00_f32,
                [0.5_f32, 0.5_f32],
            ),
            (
                TouchPhase::End,
                page.min + egui::vec2(220.0, 120.0),
                0.35_f32,
                [0.5_f32, 0.5_f32],
            ),
        ];

        let mut raw_events = Vec::new();
        for (phase, pos, force, tilt) in stroke1_samples {
            input.samples.borrow_mut().record_with_pen_state(
                phase,
                pos,
                force,
                tilt,
                TabletPointerKind::PenTip,
            );
            match phase {
                TouchPhase::Start => {
                    raw_events.push(Event::PointerMoved(pos));
                    raw_events.push(pointer(pos, true));
                }
                TouchPhase::Move => {
                    raw_events.push(Event::PointerMoved(pos));
                }
                TouchPhase::End => {
                    raw_events.push(Event::PointerMoved(pos));
                    raw_events.push(pointer(pos, false));
                }
                TouchPhase::Cancel => {}
            }
        }

        // Before End is processed, contact carries tilt and PenTip state.
        let mut raw = egui::RawInput {
            events: raw_events,
            ..Default::default()
        };
        input.append_events(&ctx, &mut raw);

        let recorded_forces: Vec<f32> = raw
            .events
            .iter()
            .filter_map(|ev| match ev {
                Event::Touch { force, .. } => *force,
                _ => None,
            })
            .collect();
        assert_eq!(recorded_forces, vec![0.05, 0.25, 0.60, 1.00, 0.35]);
        assert!(
            input.forwarded_contact.is_none(),
            "completed stroke must release forwarded tablet contact"
        );

        crate::tests::stage_input_frame(&mut editor, &ctx, 1.0, raw.events);
        assert!(
            !editor.authoring_tools.is_drawing(),
            "completed tablet stroke must not stay in drawing state"
        );

        let elements = &editor.symbol().timeline.layers[0].keyframes[0].elements;
        assert_eq!(elements.len(), baseline_count + 1);
        let first_stroke = elements.last().expect("first pressure brush stroke");
        let neoflash_core::ElementKind::Path { commands, .. } = &first_stroke.kind else {
            panic!("expected vector Path element from pressure brush stroke");
        };
        // Verify pressure profile tapered the stroke: span near x=180 (force=1.0) > span near x=60 (force=0.05).
        let thin_span = path_y_span_near_x(
            commands,
            first_stroke.transform.x,
            first_stroke.transform.y,
            60.0,
            12.0,
        );
        let thick_span = path_y_span_near_x(
            commands,
            first_stroke.transform.x,
            first_stroke.transform.y,
            180.0,
            12.0,
        );
        assert!(
            thick_span > thin_span * 1.8,
            "expected pressure=1.0 segment height ({thick_span:.2}) to exceed pressure=0.05 segment height ({thin_span:.2})"
        );
        assert!(
            path_max_x(commands, first_stroke.transform.x) >= 218.0,
            "final segment at x=220 must not be dropped"
        );

        // 3. Fast consecutive strokes without dropped final segments or hangs.
        for (stroke_idx, y_off) in [(1_usize, 180.0_f32), (2_usize, 240.0_f32)] {
            let p0 = page.min + egui::vec2(60.0, y_off);
            let p1 = page.min + egui::vec2(150.0, y_off);
            let p2 = page.min + egui::vec2(260.0, y_off);
            input.samples.borrow_mut().record_with_pen_state(
                TouchPhase::Start,
                p0,
                0.15,
                [0.0, 0.8],
                TabletPointerKind::PenTip,
            );
            input.samples.borrow_mut().record_with_pen_state(
                TouchPhase::Move,
                p1,
                0.75,
                [0.0, 0.8],
                TabletPointerKind::PenTip,
            );
            assert_eq!(input.active_pointer_kind(), TabletPointerKind::PenTip);
            assert!((input.active_nib_angle_deg().unwrap() - 90.0).abs() < 1e-3);
            input.samples.borrow_mut().record_with_pen_state(
                TouchPhase::End,
                p2,
                0.95,
                [0.0, 0.8],
                TabletPointerKind::PenTip,
            );
            let mut stroke_raw = egui::RawInput {
                events: vec![
                    Event::PointerMoved(p0),
                    pointer(p0, true),
                    Event::PointerMoved(p1),
                    Event::PointerMoved(p2),
                    pointer(p2, false),
                ],
                ..Default::default()
            };
            input.append_events(&ctx, &mut stroke_raw);
            crate::tests::stage_input_frame(&mut editor, &ctx, 1.0, stroke_raw.events);
            assert!(!editor.authoring_tools.is_drawing());
            assert!(input.forwarded_contact.is_none());

            let els = &editor.symbol().timeline.layers[0].keyframes[0].elements;
            assert_eq!(els.len(), baseline_count + 1 + stroke_idx);
            let latest = els.last().unwrap();
            let neoflash_core::ElementKind::Path { commands: cmds, .. } = &latest.kind else {
                panic!("expected Path element for consecutive stroke {stroke_idx}");
            };
            let end_x = path_max_x(cmds, latest.transform.x);
            assert!(
                end_x >= 258.0,
                "consecutive stroke {stroke_idx} dropped its final segment (max_x={end_x:.2})"
            );
        }

        // 4. Pen-tip to Eraser switching and pointer-capture release cleanup.
        // First verify mid-stroke PointerGone + tool switch cleans up without committing garbage.
        let count_before_abort = editor.symbol().timeline.layers[0].keyframes[0]
            .elements
            .len();
        let abort_pos = page.min + egui::vec2(80.0, 300.0);
        input.samples.borrow_mut().record_with_pen_state(
            TouchPhase::Start,
            abort_pos,
            0.6,
            [-0.5, 0.5],
            TabletPointerKind::PenTip,
        );
        let mut down_raw = egui::RawInput {
            events: vec![Event::PointerMoved(abort_pos), pointer(abort_pos, true)],
            ..Default::default()
        };
        input.append_events(&ctx, &mut down_raw);
        assert!(input.forwarded_contact.is_some());
        crate::tests::stage_input_frame(&mut editor, &ctx, 1.0, down_raw.events);
        assert!(editor.authoring_tools.is_drawing());

        // PointerGone releases tablet contact and cancels the active stroke cleanly.
        let mut gone_raw = egui::RawInput {
            events: vec![Event::PointerGone],
            ..Default::default()
        };
        input.append_events(&ctx, &mut gone_raw);
        assert!(input.forwarded_contact.is_none());
        crate::tests::stage_input_frame(&mut editor, &ctx, 1.0, gone_raw.events);
        assert!(!editor.authoring_tools.is_drawing());
        assert_eq!(
            editor.symbol().timeline.layers[0].keyframes[0]
                .elements
                .len(),
            count_before_abort
        );

        // Switch from PenTip -> Eraser end and run an eraser pass across the third stroke (y=240).
        editor.set_tool(TabletInput::tool_for_pointer_kind(
            TabletPointerKind::Eraser,
        ));
        assert_eq!(editor.tool, crate::app::Tool::Eraser);
        let before_erase = editor.project.clone();
        let e0 = page.min + egui::vec2(150.0, 210.0);
        let e1 = page.min + egui::vec2(150.0, 240.0);
        let e2 = page.min + egui::vec2(150.0, 270.0);
        input.samples.borrow_mut().record_with_pen_state(
            TouchPhase::Start,
            e0,
            0.8,
            [0.0, 0.0],
            TabletPointerKind::Eraser,
        );
        input.samples.borrow_mut().record_with_pen_state(
            TouchPhase::Move,
            e1,
            0.9,
            [0.0, 0.0],
            TabletPointerKind::Eraser,
        );
        assert_eq!(input.active_pointer_kind(), TabletPointerKind::Eraser);
        input.samples.borrow_mut().record_with_pen_state(
            TouchPhase::End,
            e2,
            0.4,
            [0.0, 0.0],
            TabletPointerKind::Eraser,
        );
        let mut erase_raw = egui::RawInput {
            events: vec![
                Event::PointerMoved(e0),
                pointer(e0, true),
                Event::PointerMoved(e1),
                Event::PointerMoved(e2),
                pointer(e2, false),
            ],
            ..Default::default()
        };
        input.append_events(&ctx, &mut erase_raw);
        crate::tests::stage_input_frame(&mut editor, &ctx, 1.0, erase_raw.events);
        assert!(!editor.authoring_tools.is_drawing());
        assert!(input.forwarded_contact.is_none());
        assert_ne!(
            editor.project, before_erase,
            "eraser stroke must modify the stage artwork"
        );

        // Switch back to PenTip (`Tool::Brush`).
        editor.set_tool(TabletInput::tool_for_pointer_kind(
            TabletPointerKind::PenTip,
        ));
        assert_eq!(editor.tool, crate::app::Tool::Brush);

        // 5. Single-step Undo and `.nflash` save/reopen round-trip of pressure-sensitive strokes.
        editor.undo();
        assert_eq!(
            editor.project, before_erase,
            "single-step Undo must revert the eraser pass"
        );

        let before_third_stroke_undo = editor.project.clone();
        editor.undo();
        assert_eq!(
            editor.symbol().timeline.layers[0].keyframes[0]
                .elements
                .len(),
            baseline_count + 2,
            "single-step Undo must remove the third pressure brush stroke"
        );
        editor.redo();
        assert_eq!(editor.project, before_third_stroke_undo);

        let saved_json =
            serde_json::to_string_pretty(&editor.project).expect("serialize .nflash project");
        let reopened_project: neoflash_core::Project =
            serde_json::from_str(&saved_json).expect("deserialize .nflash project");
        reopened_project
            .validate()
            .expect("reopened .nflash project must validate");
        let orig_els = &editor.project.symbols[0].timeline.layers[0].keyframes[0].elements;
        let reop_els = &reopened_project.symbols[0].timeline.layers[0].keyframes[0].elements;
        assert_eq!(reop_els.len(), orig_els.len());
        for (a, b) in orig_els.iter().zip(reop_els.iter()) {
            assert_eq!(a.id, b.id);
            assert_eq!(a.name, b.name);
            if let (
                neoflash_core::ElementKind::Path { commands: ca, .. },
                neoflash_core::ElementKind::Path { commands: cb, .. },
            ) = (&a.kind, &b.kind)
            {
                assert_eq!(ca.len(), cb.len());
            }
        }

        let reopened_editor = crate::app::Editor::from_project(&ctx, reopened_project.clone());
        assert_eq!(reopened_editor.project, reopened_project);
    }
}
