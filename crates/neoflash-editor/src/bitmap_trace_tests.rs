use super::*;
use crate::{app::Tool, theme::ThemeMode};
use std::{
    io::Cursor,
    time::{Duration, Instant},
};

fn fixture() -> Project {
    let mut project = Project::blank();
    project.format_version = 3;
    let image = image::RgbaImage::from_fn(24, 20, |x, y| {
        image::Rgba(if x == 0 || y == 0 || x == 23 || y == 19 {
            [0; 4]
        } else if x < 12 {
            [230, 60, 30, 255]
        } else {
            [20, 120, 230, 255]
        })
    });
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    let resource =
        ResourceUnit::from_bytes(new_id("bitmap"), "trace.png", "image/png", bytes.get_ref())
            .unwrap();
    let element = Element {
        id: new_id("bitmap"),
        name: "Trace sample".into(),
        kind: ElementKind::Bitmap {
            resource_id: resource.id.clone(),
            width: 24,
            height: 20,
            smoothing: true,
        },
        transform: Transform {
            x: 37.,
            y: 41.,
            scale_x: 1.5,
            scale_y: 0.75,
            rotation: 24.,
            alpha: 0.8,
            ..Default::default()
        },
        ..Default::default()
    };
    project.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .push(element);
    project.resources.push(resource);
    project.validate().unwrap();
    project
}
fn editor(ctx: &egui::Context) -> Editor {
    let mut e = Editor::from_project(ctx, fixture());
    let id = e.project.symbols[0].timeline.layers[0].keyframes[0].elements[0]
        .id
        .clone();
    e.set_selection(vec![e.target_for_id(&id).unwrap()]);
    e.tool = Tool::Brush;
    e
}
fn wait(e: &mut Editor, ctx: &egui::Context) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while e.bitmap_trace.busy() {
        e.poll_bitmap_trace(ctx);
        assert!(Instant::now() < deadline, "trace worker timed out");
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn frame(
    e: &mut Editor,
    ctx: &egui::Context,
    time: f64,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1920., 1080.),
            )),
            time: Some(time),
            events,
            ..Default::default()
        },
        |ctx| e.trace_bitmap_dialog(ctx),
    )
}
fn bounds(output: &egui::FullOutput, label: &str) -> Option<egui::Rect> {
    output
        .platform_output
        .accesskit_update
        .as_ref()?
        .nodes
        .iter()
        .find_map(|(_, n)| {
            if n.label() != Some(label) {
                return None;
            }
            n.bounds().map(|b| {
                egui::Rect::from_min_max(
                    egui::pos2(b.x0 as f32, b.y0 as f32),
                    egui::pos2(b.x1 as f32, b.y1 as f32),
                )
            })
        })
}
#[test]
fn trace_bitmap_family_controls_cancel_stale_and_one_undo() {
    for theme in [ThemeMode::Dark, ThemeMode::Light] {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut e = editor(&ctx);
        e.set_theme(&ctx, theme);
        let before = e.project.clone();
        let selected = e.selection.clone();
        e.open_trace_bitmap(&ctx);
        assert!(e.has_modal_dialog());
        wait(&mut e, &ctx);
        assert!(e.bitmap_trace.dialog.as_ref().unwrap().preview.is_some());
        assert_eq!(e.project, before);
        frame(&mut e, &ctx, 0.1, vec![]);
        let out = frame(&mut e, &ctx, 0.2, vec![]);
        fn dialog_rect(shape: &egui::Shape) -> Option<egui::Rect> {
            match shape {
                egui::Shape::Rect(r)
                    if (679.0..=681.).contains(&r.rect.width()) && r.rect.height() > 500. =>
                {
                    Some(r.rect)
                }
                egui::Shape::Vec(shapes) => shapes.iter().find_map(dialog_rect),
                _ => None,
            }
        }
        let dialog = out
            .shapes
            .iter()
            .find_map(|s| dialog_rect(&s.shape))
            .expect("Trace dialog surface");
        assert!(
            (683.0..=685.).contains(&dialog.height()),
            "approved 680 × 684 dialog: {dialog:?}"
        );
        for label in ["Colours", "Paths", "Corners", "Noise"] {
            let widths: Vec<_> = out
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .filter(|(_, node)| node.label() == Some(label))
                .filter_map(|(_, node)| node.bounds().map(|r| r.x1 - r.x0))
                .collect();
            assert!(
                widths.iter().any(|w| (133.0..=135.).contains(w)),
                "{label} field must use outline width134: {widths:?}"
            );
        }
        for label in [
            "Live preview",
            "Colours",
            "Paths",
            "Corners",
            "Noise",
            "Fills",
            "Strokes",
            "Cancel",
            "Trace",
        ] {
            let rect = bounds(&out, label).unwrap_or_else(|| panic!("missing {label}"));
            assert!(dialog.contains_rect(rect), "clipped {label}: {rect:?}");
        }
        // An actual control event changes only the draft, then Cancel discards it.
        let checkbox = bounds(&out, "Ignore white").unwrap().center();
        frame(
            &mut e,
            &ctx,
            0.3,
            vec![
                egui::Event::PointerMoved(checkbox),
                crate::tests::primary_button(checkbox, true),
                crate::tests::primary_button(checkbox, false),
            ],
        );
        assert!(e.bitmap_trace.dialog.as_ref().unwrap().options.ignore_white);
        e.bitmap_trace.request(true);
        e.bitmap_trace.cancel();
        wait(&mut e, &ctx);
        assert_eq!(e.project, before);
        assert_eq!(e.selection, selected);
        assert_eq!(e.history.undo_len(), 0);
        // A newer preview always wins, even when queued behind a cancelled request.
        e.open_trace_bitmap(&ctx);
        e.bitmap_trace.dialog.as_mut().unwrap().options.mode = TraceMode::Greyscale;
        e.bitmap_trace.request(false);
        wait(&mut e, &ctx);
        assert_eq!(
            e.bitmap_trace
                .dialog
                .as_ref()
                .unwrap()
                .preview
                .as_ref()
                .unwrap()
                .options
                .mode,
            TraceMode::Greyscale
        );
        e.bitmap_trace.dialog.as_mut().unwrap().options = TraceOptions {
            noise: 1,
            paths: 100.,
            corners: 100.,
            ..Default::default()
        };
        e.bitmap_trace.request(true);
        wait(&mut e, &ctx);
        assert!(!e.bitmap_trace.is_open());
        assert_eq!(e.tool, Tool::Brush);
        assert_eq!(e.history.undo_len(), 1);
        e.project.validate().unwrap();
        let layer = &e.symbol().timeline.layers[0];
        assert_eq!(layer.properties.groups.len(), 1);
        assert_eq!(e.selection.targets.len(), 2);
        assert!(layer.keyframes[0].elements.iter().all(|el| matches!(
            el.kind,
            ElementKind::Path { .. }
        ) && el.transform
            == before.symbols[0].timeline.layers[0].keyframes[0].elements[0].transform));
        for point in [[5.5, 10.5], [18.5, 10.5]] {
            let covering = layer.keyframes[0]
                .elements
                .iter()
                .filter(|element| {
                    let ElementKind::Path {
                        commands,
                        fill_rule,
                        ..
                    } = &element.kind
                    else {
                        return false;
                    };
                    hit_test_path(
                        commands,
                        *fill_rule,
                        point,
                        true,
                        0.,
                        FlattenOptions::default(),
                    )
                    .unwrap()
                })
                .count();
            assert_eq!(
                covering, 1,
                "translucent placement must not receive double underpaint alpha"
            );
        }
        e.undo();
        assert_eq!(e.project, before);
        let id = e.symbol().timeline.layers[0].keyframes[0].elements[0]
            .id
            .clone();
        e.set_selection(vec![e.target_for_id(&id).unwrap()]);
        e.open_trace_bitmap(&ctx);
        e.bitmap_trace.dialog.as_mut().unwrap().options = TraceOptions {
            fills: false,
            strokes: true,
            stroke_width: 3.,
            ..Default::default()
        };
        e.bitmap_trace.request(true);
        wait(&mut e, &ctx);
        assert!(!e.bitmap_trace.is_open());
        assert!(
            e.symbol().timeline.layers[0].keyframes[0]
                .elements
                .iter()
                .all(|el| matches!(
                    el.kind,
                    ElementKind::Path {
                        fill: [0, 0, 0, 0],
                        stroke: [_, _, _, 255],
                        stroke_width: 3.,
                        ..
                    }
                ))
        );
        e.project.validate().unwrap();
        e.undo();
        assert_eq!(e.project, before);
        e.set_selection(vec![e.target_for_id(&id).unwrap()]);
        e.open_trace_bitmap(&ctx);
        e.bitmap_trace.request(true);
        e.project.symbols[0].timeline.layers[0].keyframes[0].elements[0]
            .transform
            .x += 1.;
        let changed = e.project.clone();
        wait(&mut e, &ctx);
        assert_eq!(e.project, changed);
        assert!(
            e.bitmap_trace
                .dialog
                .as_ref()
                .unwrap()
                .error
                .as_deref()
                .unwrap()
                .contains("changed")
        );
        e.bitmap_trace.cancel();
    }
}
