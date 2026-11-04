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

#[derive(Clone, Copy, Debug)]
struct Sample {
    id: TouchId,
    phase: TouchPhase,
    /// Top-left content-view coordinates, in native logical points (before egui UI zoom).
    position: Pos2,
    force: f32,
}

#[derive(Default)]
struct Samples {
    queued: Vec<Sample>,
    contact: Option<Sample>,
    next_id: u64,
}

impl Samples {
    fn record(&mut self, phase: TouchPhase, position: Pos2, force: f32) {
        if !position.is_finite() || !force.is_finite() {
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
                        *forwarded_contact = Some(Sample {
                            id: *id,
                            phase: *phase,
                            position: *pos * zoom,
                            force: force.unwrap_or(1.),
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
}
