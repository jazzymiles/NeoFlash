//! Panel integration checks use real egui layout, input, clipping and AX output.
use crate::{
    app::Editor,
    tests::primary_button,
    theme::{ThemeMode, UiPalette},
    ui_icons::{Icon, IconButton},
    workspace::PanelId,
};
use eframe::egui::{self, Event, Pos2, Rect, Vec2};
use neoflash_core::*;

fn frame(
    editor: &mut Editor,
    ctx: &egui::Context,
    size: Vec2,
    time: f64,
    events: Vec<Event>,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
            time: Some(time),
            events,
            ..Default::default()
        },
        |ctx| {
            editor.shortcuts(ctx);
            editor.menu(ctx);
            editor.timeline(ctx);
            editor.tools(ctx);
            editor.library(ctx);
            editor.bottom_panel(ctx);
            editor.stage(ctx);
        },
    )
}
fn named_bounds(output: &egui::FullOutput, label: &str) -> Option<Rect> {
    output
        .platform_output
        .accesskit_update
        .as_ref()?
        .nodes
        .iter()
        .find_map(|(_, node)| {
            if node.label() != Some(label) {
                return None;
            }
            node.bounds().map(|r| {
                Rect::from_min_max(
                    Pos2::new(r.x0 as f32, r.y0 as f32),
                    Pos2::new(r.x1 as f32, r.y1 as f32),
                )
            })
        })
}
fn wheel(point: Pos2, delta: Vec2) -> Vec<Event> {
    vec![
        Event::PointerMoved(point),
        Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}

#[test]
fn tools_scroll_to_fit_and_library_scroll_to_last_symbol_at_minimum_window() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut project = Project::blank();
    for i in 0..100 {
        let mut symbol = project.symbols[0].clone();
        symbol.id = format!("symbol-{i}");
        symbol.name = format!("Symbol {i:03}");
        symbol.timeline.layers[0].id = format!("layer-{i}");
        project.symbols.push(symbol);
    }
    let mut editor = Editor::from_project(&ctx, project);
    editor.set_theme(&ctx, ThemeMode::Light);
    let before = editor.project.clone();
    let size = Vec2::new(1024., 720.);
    frame(&mut editor, &ctx, size, 0.1, vec![]);
    // Scroll actual panels, not their state objects: a user must be able to
    // reach both the final view tool and a long library's last entry.
    frame(
        &mut editor,
        &ctx,
        size,
        0.2,
        wheel(Pos2::new(24., 400.), Vec2::new(0., -2000.)),
    );
    // Wheel input changes the next layout; inspect the following settled paints.
    let mut out = frame(&mut editor, &ctx, size, 0.21, vec![]);
    for step in 1..5 {
        out = frame(&mut editor, &ctx, size, 0.21 + step as f64 / 60., vec![]);
    }
    let fit = named_bounds(&out, "Fit stage").expect("Fit must have an accessible button");
    assert!(
        Rect::from_min_size(Pos2::ZERO, size).contains_rect(fit),
        "{fit:?}"
    );
    editor.zoom = 1.5;
    frame(
        &mut editor,
        &ctx,
        size,
        0.3,
        vec![
            Event::PointerMoved(fit.center()),
            primary_button(fit.center(), true),
        ],
    );
    frame(
        &mut editor,
        &ctx,
        size,
        0.4,
        vec![primary_button(fit.center(), false)],
    );
    assert!(
        editor.zoom > 0. && editor.zoom < 1.5,
        "Fit must recalculate stage zoom"
    );
    assert_eq!(editor.pan, Vec2::ZERO);
    frame(
        &mut editor,
        &ctx,
        size,
        0.5,
        wheel(Pos2::new(900., 400.), Vec2::new(0., -10000.)),
    );
    let mut out = frame(&mut editor, &ctx, size, 0.51, vec![]);
    for step in 1..10 {
        out = frame(&mut editor, &ctx, size, 0.51 + step as f64 / 60., vec![]);
    }
    let last = named_bounds(&out, "Symbol 099").expect("last symbol remains accessible");
    assert!(last.bottom() <= 720. && last.top() > 200., "{last:?}");
    frame(
        &mut editor,
        &ctx,
        size,
        0.8,
        vec![
            Event::PointerMoved(last.center()),
            primary_button(last.center(), true),
        ],
    );
    frame(
        &mut editor,
        &ctx,
        size,
        0.9,
        vec![primary_button(last.center(), false)],
    );
    assert_eq!(editor.library_selected.as_deref(), Some("symbol-99"));
    assert_eq!(editor.project, before);
    assert!(!editor.dirty());
}

#[test]
fn layer_labels_and_keys_share_exact_rows_after_vertical_scroll() {
    for theme in [ThemeMode::Light, ThemeMode::Dark] {
        let ctx = egui::Context::default();
        let mut project = Project::demo();
        let root = project.root.clone();
        let template = project.symbol(&root).unwrap().timeline.layers[0].clone();
        project.symbol_mut(&root).unwrap().timeline.layers = (0..30)
            .map(|i| {
                let mut layer = template.clone();
                layer.id = format!("test-layer-{i}");
                layer.name = format!("Row {i:02}");
                let mut last_key = layer.keyframes[0].clone();
                last_key.frame = 15;
                layer.keyframes.push(last_key);
                for key in &mut layer.keyframes {
                    for element in &mut key.elements {
                        element.id = format!("{i}-{}", element.id);
                    }
                }
                layer
            })
            .collect();
        let mut editor = Editor::from_project(&ctx, project);
        editor.set_theme(&ctx, theme);
        frame(&mut editor, &ctx, Vec2::new(1024., 720.), 0.1, vec![]);
        frame(
            &mut editor,
            &ctx,
            Vec2::new(1024., 720.),
            0.2,
            wheel(Pos2::new(400., 130.), Vec2::new(0., -260.)),
        );
        let mut out = frame(&mut editor, &ctx, Vec2::new(1024., 720.), 0.3, vec![]);
        for step in 1..8 {
            out = frame(
                &mut editor,
                &ctx,
                Vec2::new(1024., 720.),
                0.3 + step as f64 / 60.,
                vec![],
            );
        }
        let keys: Vec<_> = out
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Circle(c)
                    if c.radius == 3.
                        && c.fill == UiPalette::current(&ctx).timeline_key
                        && s.clip_rect.contains(c.center) =>
                {
                    Some(c.center.y)
                }
                _ => None,
            })
            .collect();
        let row_top = keys.iter().copied().fold(f32::INFINITY, f32::min)
            - crate::timeline_paint::ROW_HEIGHT / 2.;
        let row_bottom = keys.iter().copied().fold(f32::NEG_INFINITY, f32::max)
            + crate::timeline_paint::ROW_HEIGHT / 2.;
        let mut visible = 0;
        let mut first_row = usize::MAX;
        for shape in &out.shapes {
            if let egui::Shape::Text(text) = &shape.shape
                && text.galley.job.text.starts_with("Row ")
                && shape.clip_rect.height() <= crate::timeline_paint::ROW_HEIGHT + 1.
            {
                let y = text.pos.y + text.galley.size().y / 2.;
                if shape.clip_rect.contains(Pos2::new(text.pos.x + 2., y))
                    && text.pos.x < 210.
                    && y >= row_top
                    && y <= row_bottom
                {
                    visible += 1;
                    first_row = first_row.min(text.galley.job.text[4..].parse::<usize>().unwrap());
                    assert!(
                        keys.iter().any(|key| (*key - y).abs() < 1.),
                        "unaligned label at {y}, keys {keys:?}"
                    );
                }
            }
        }
        assert!(visible >= 2);
        assert!(
            first_row >= 5,
            "wheel must actually scroll shared rows, first={first_row}"
        );
        let from = out
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Circle(circle)
                    if circle.radius == 3.
                        && circle.fill == UiPalette::current(&ctx).timeline_key
                        && shape.clip_rect.contains(circle.center)
                        && (circle.center.y - row_top - crate::timeline_paint::ROW_HEIGHT / 2.)
                            .abs()
                            < 1. =>
                {
                    Some(circle.center)
                }
                _ => None,
            })
            .max_by(|a, b| a.x.total_cmp(&b.x))
            .unwrap();
        let to = from - Vec2::new(3. * editor.timeline_cell, 0.);
        let before = editor.project.clone();
        frame(
            &mut editor,
            &ctx,
            Vec2::new(1024., 720.),
            0.6,
            vec![
                Event::PointerMoved(from),
                primary_button(from, true),
                Event::PointerMoved(to),
                primary_button(to, false),
            ],
        );
        assert_eq!(editor.frame, 12, "batched key drag selects its destination");
        let layer = &editor.symbol().timeline.layers[editor.layer];
        assert!(layer.keyframes.iter().any(|key| key.frame == 12));
        assert!(!layer.keyframes.iter().any(|key| key.frame == 15));
        editor.undo();
        assert_eq!(editor.project, before);
        assert!(
            !editor.history.can_undo(),
            "a complete key drag is one undo step"
        );
    }
}

#[test]
fn visibility_restore_and_theme_keep_document_and_history_unchanged() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut editor = Editor::from_project(&ctx, Project::demo());
    let before = editor.project.clone();
    for panel in PanelId::ALL {
        editor.set_panel_visible(panel, false);
    }
    frame(&mut editor, &ctx, Vec2::new(1024., 720.), 0.1, vec![]);
    editor.set_theme(&ctx, ThemeMode::Dark);
    editor.restore_workspace(&ctx);
    let out = frame(&mut editor, &ctx, Vec2::new(1024., 720.), 0.2, vec![]);
    assert!(named_bounds(&out, "Selection (V)").is_some());
    for panel in PanelId::ALL {
        assert!(editor.workspace.is_panel_visible(panel));
    }
    assert!(editor.workspace.theme == ThemeMode::Dark);
    assert_eq!(editor.project, before);
    assert!(!editor.dirty());
    assert!(!editor.history.can_undo());
}

#[test]
fn icon_button_has_real_name_selection_focus_and_disabled_behavior() {
    fn render(
        ctx: &egui::Context,
        events: Vec<Event>,
        enabled: bool,
    ) -> (egui::FullOutput, egui::Response) {
        let mut response = None;
        let output = ctx.run(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    response = Some(ui.add_enabled(
                        enabled,
                        IconButton::new(Icon::Pen, "Pen (P)").selected(true),
                    ));
                });
            },
        );
        (output, response.unwrap())
    }
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let (output, response) = render(&ctx, vec![], true);
    assert!(named_bounds(&output, "Pen (P)").is_some());
    let rect = response.rect;
    ctx.memory_mut(|m| m.request_focus(response.id));
    let (_, response) = render(&ctx, vec![], true);
    assert!(response.has_focus());
    render(
        &ctx,
        vec![
            Event::PointerMoved(rect.center()),
            primary_button(rect.center(), true),
        ],
        true,
    );
    let (_, response) = render(&ctx, vec![primary_button(rect.center(), false)], true);
    assert!(response.clicked());
    let (output, response) = render(
        &ctx,
        vec![
            Event::PointerMoved(rect.center()),
            primary_button(rect.center(), true),
            primary_button(rect.center(), false),
        ],
        false,
    );
    assert!(!response.clicked());
    let node = output
        .platform_output
        .accesskit_update
        .unwrap()
        .nodes
        .into_iter()
        .find(|(_, node)| node.label() == Some("Pen (P)"))
        .unwrap()
        .1;
    assert!(node.is_disabled());
    assert_eq!(node.toggled(), Some(egui::accesskit::Toggled::True));
}

#[test]
fn actions_reparent_keeps_cursor_and_visibility_alone_does_not_pause_preview() {
    fn render(editor: &mut Editor, ctx: &egui::Context, parent: &str, focused: bool) {
        let _ = ctx.run(
            egui::RawInput {
                focused,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.scope_builder(egui::UiBuilder::new().id(parent), |ui| {
                        editor.actions_contents(ui)
                    });
                });
            },
        );
    }
    let ctx = egui::Context::default();
    let mut project = Project::blank();
    project.symbols[0].timeline.scripts = vec![FrameScript {
        frame: 0,
        script: Script {
            language: ScriptLanguage::JavaScript,
            source: "var longText = 123;".into(),
        },
    }];
    let mut editor = Editor::from_project(&ctx, project);
    editor.show_panel(PanelId::Actions);
    editor.set_playing(true);
    render(&mut editor, &ctx, "docked-parent", true);
    assert!(
        editor.playing,
        "an open Actions panel is not an editing gesture"
    );
    let id = editor.actions_editor_id();
    let mut state = egui::text_edit::TextEditState::load(&ctx, id).unwrap();
    state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::two(
            egui::text::CCursor::new(3),
            egui::text::CCursor::new(8),
        )));
    state.store(&ctx, id);
    render(&mut editor, &ctx, "detached-parent", false);
    assert!(editor.playing);
    let range = egui::text_edit::TextEditState::load(&ctx, id)
        .unwrap()
        .cursor
        .char_range()
        .unwrap();
    assert_eq!((range.primary.index, range.secondary.index), (8, 3));
    ctx.memory_mut(|memory| memory.request_focus(id));
    render(&mut editor, &ctx, "detached-parent", true);
    assert!(
        !editor.playing,
        "focused Actions editor pauses the authoring preview"
    );
    editor.frame = 1;
    assert_ne!(id, editor.actions_editor_id());
    assert!(!editor.dirty());
    assert!(!editor.history.can_undo());
}

fn detached_timeline_frame(
    editor: &mut Editor,
    ctx: &egui::Context,
    size: Vec2,
    time: f64,
    events: Vec<Event>,
) -> egui::FullOutput {
    let child = egui::ViewportId::from_hash_of("timeline-footer-detached-test");
    let mut input = egui::RawInput {
        viewport_id: child,
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
        time: Some(time),
        events,
        ..Default::default()
    };
    input.viewports.insert(
        child,
        egui::ViewportInfo {
            parent: Some(egui::ViewportId::ROOT),
            ..Default::default()
        },
    );
    ctx.run(input, |ctx| {
        egui::TopBottomPanel::top("detached-test-header")
            .exact_height(28.)
            .show(ctx, |ui| {
                ui.label("Timeline — Dock");
            });
        egui::CentralPanel::default().show(ctx, |ui| editor.timeline_contents(ui));
    })
}

fn fully_visible_text(output: &egui::FullOutput, label: &str, viewport: Rect) -> bool {
    output.shapes.iter().any(|shape| {
        if let egui::Shape::Text(text) = &shape.shape
            && text.galley.job.text == label
        {
            let rect = Rect::from_min_size(text.pos, text.galley.size());
            return viewport.contains_rect(rect) && shape.clip_rect.contains_rect(rect);
        }
        false
    })
}

#[test]
fn detached_timeline_at_750_wraps_complete_opacity_control_inside_viewport() {
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut editor = Editor::from_project(&ctx, Project::demo());
        editor.set_theme(&ctx, mode);
        let before = editor.project.clone();
        let size = Vec2::new(750., 380.);
        let viewport = Rect::from_min_size(Pos2::ZERO, size);
        let mut output = detached_timeline_frame(&mut editor, &ctx, size, 0., vec![]);
        for pass in 1..4 {
            output = detached_timeline_frame(&mut editor, &ctx, size, pass as f64 / 60., vec![]);
        }
        for label in [
            "Add layer",
            "Rewind",
            "Play preview",
            "Onion Skin",
            "Outlines",
            "Anchor",
            "Opacity",
        ] {
            let bounds = named_bounds(&output, label)
                .unwrap_or_else(|| panic!("missing AX control {label}"));
            assert!(
                viewport.contains_rect(bounds),
                "{mode:?} {label}: {bounds:?}"
            );
        }
        assert!(
            fully_visible_text(&output, "Opacity", viewport),
            "Opacity label must not be clipped in {mode:?}"
        );
        assert_eq!(editor.project, before);
        assert!(!editor.history.can_undo());
    }
}

#[test]
fn detached_timeline_minimum_width_scrolls_to_working_opacity_control() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut editor = Editor::from_project(&ctx, Project::demo());
    editor.set_theme(&ctx, ThemeMode::Light);
    let before = editor.project.clone();
    let size = Vec2::new(320., 180.);
    let viewport = Rect::from_min_size(Pos2::ZERO, size);
    detached_timeline_frame(&mut editor, &ctx, size, 0., vec![]);
    let initial = detached_timeline_frame(&mut editor, &ctx, size, 0.1, vec![]);
    assert!(
        !fully_visible_text(&initial, "Opacity", viewport),
        "narrow footer should need scrolling"
    );
    let pointer = Pos2::new(160., 155.);
    let mut output = detached_timeline_frame(
        &mut editor,
        &ctx,
        size,
        0.2,
        wheel(pointer, Vec2::new(0., -1000.)),
    );
    for pass in 1..12 {
        output = detached_timeline_frame(&mut editor, &ctx, size, 0.2 + pass as f64 / 60., vec![]);
    }
    assert!(
        fully_visible_text(&output, "Opacity", viewport),
        "wheel must reveal complete Opacity label"
    );
    let bounds = named_bounds(&output, "Opacity").expect("Opacity retains an AX control");
    assert!(viewport.contains_rect(bounds), "{bounds:?}");
    let opacity = editor.onion_opacity;
    let point = Pos2::new(bounds.right() - 4., bounds.center().y);
    detached_timeline_frame(
        &mut editor,
        &ctx,
        size,
        0.5,
        vec![Event::PointerMoved(point), primary_button(point, true)],
    );
    detached_timeline_frame(
        &mut editor,
        &ctx,
        size,
        0.6,
        vec![primary_button(point, false)],
    );
    assert_ne!(
        editor.onion_opacity, opacity,
        "revealed slider must accept pointer input"
    );
    assert_eq!(editor.project, before);
    assert!(!editor.dirty());
    assert!(!editor.history.can_undo());
}

#[test]
fn bounded_timeline_footer_preserves_stage_at_minimum_root_window() {
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::demo());
    editor.set_theme(&ctx, ThemeMode::Light);
    let size = Vec2::new(1024., 720.);
    let viewport = Rect::from_min_size(Pos2::ZERO, size);
    for pass in 0..20 {
        let mut stage = Rect::NOTHING;
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(viewport),
                time: Some(pass as f64 / 60.),
                ..Default::default()
            },
            |ctx| {
                editor.menu(ctx);
                editor.timeline(ctx);
                editor.tools(ctx);
                editor.library(ctx);
                editor.bottom_panel(ctx);
                stage = ctx.available_rect();
                editor.stage(ctx);
            },
        );
        assert!(stage.width() >= 600. && stage.height() >= 200., "{stage:?}");
        assert!(fully_visible_text(&output, "Opacity", viewport));
        assert!(
            editor.workspace.sizes.timeline_height <= 208.,
            "timeline must not grow on repaint: {}",
            editor.workspace.sizes.timeline_height
        );
    }
    assert!(!editor.dirty());
}
