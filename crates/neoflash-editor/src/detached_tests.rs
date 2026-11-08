//! Exercise real egui immediate-viewport callbacks with simulated window input.
//! OS move/resize and native clipboard acceptance remain live native checks.
use crate::{
    app::{BottomTab, Editor},
    workspace::{DockSurface, PanelId, PanelPlacement},
};
use eframe::egui;
use neoflash_core::Project;
use std::{cell::RefCell, rc::Rc};

#[test]
fn panel_drag_previews_actual_contents_including_inactive_tabs_and_cancels_cleanly() {
    // Bottom docks are deliberately much wider than any floating panel. Include
    // the huge geometry remembered by the old implementation and a valid user
    // size, plus a whole-group drag and an unchanged side-panel drag.
    for (panel, saved_size, group_drag) in [
        (PanelId::Properties, None, false),
        (PanelId::Timeline, None, false),
        (PanelId::Actions, None, false),
        (PanelId::Output, Some([3000., 800.]), false),
        (PanelId::Accessibility, None, false),
        (PanelId::Accessibility, Some([480., 300.]), false),
        (PanelId::Timeline, None, true),
    ] {
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        editor.set_panel_visible(panel, true);
        editor.output.clear();
        if let Some(inner_size) = saved_size {
            editor.workspace.remember_geometry(
                panel,
                crate::workspace::WindowGeometry {
                    outer_position: [120., 100.],
                    inner_size,
                },
            );
        }
        let body_label = match panel {
            PanelId::Properties => "HTML5 · Rust / WebAssembly".to_owned(),
            PanelId::Timeline => editor.symbol().timeline.layers[0].name.clone(),
            PanelId::Actions => format!("{} · Frame 1", editor.symbol().name),
            PanelId::Output => "No output yet".to_owned(),
            _ => "No object selected".to_owned(),
        };
        let origin = egui::pos2(100., 80.);
        let screen_size = egui::vec2(1920., 1080.);
        let render = |editor: &mut Editor, events| {
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, screen_size)),
                events,
                ..Default::default()
            };
            input
                .viewports
                .get_mut(&egui::ViewportId::ROOT)
                .unwrap()
                .inner_rect = Some(egui::Rect::from_min_size(origin, screen_size));
            ctx.run(input, |ctx| {
                editor.dock_grid(ctx);
                editor.detached_panels(ctx);
            })
        };
        let text_shapes = |output: &egui::FullOutput, label: &str, area: egui::Rect| {
            output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text)
                        if text.galley.job.text == label
                            && shape.clip_rect.contains(text.pos)
                            && area.contains(text.pos) =>
                    {
                        Some(text.clone())
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        render(&mut editor, vec![]);
        let source = editor
            .detached
            .dock_groups
            .iter()
            .find(|group| group.tabs.iter().any(|(id, _)| *id == panel))
            .unwrap()
            .clone();
        let source_group = source.rect.translate(-origin.to_vec2());
        let tab_rect = source
            .tabs
            .iter()
            .find(|(id, _)| *id == panel)
            .unwrap()
            .1
            .translate(-origin.to_vec2());
        let start = if group_drag {
            // Empty internal header, away from the tab strip and menu button.
            egui::pos2(source_group.right() - 80., tab_rect.center().y)
        } else {
            tab_rect.center()
        };
        let compact = panel != PanelId::Properties;
        let floating = crate::workspace::default_window_size(panel);
        let preview_size = if compact {
            let saved = saved_size.unwrap_or(floating);
            egui::vec2(saved[0].min(floating[0]), saved[1].min(floating[1]))
        } else {
            source_group.size()
        };
        let grab_offset = if group_drag {
            let offset = start - source_group.min;
            egui::vec2(offset.x * preview_size.x / source_group.width(), offset.y)
        } else if compact {
            start - tab_rect.min
        } else {
            start - source_group.min
        };
        let before = editor.workspace.clone();
        let project_before = serde_json::to_value(&editor.project).unwrap();
        let press = render(
            &mut editor,
            vec![
                egui::Event::PointerMoved(start),
                crate::tests::primary_button(start, true),
            ],
        );
        let source_body = text_shapes(&press, &body_label, source_group)
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("{panel:?}: body must be visible before movement"));
        let stage = egui::pos2(if group_drag { 1200. } else { 650. }, 240.);
        let expected_rect = egui::Rect::from_min_size(stage - grab_offset, preview_size);
        let assert_preview = |output: &egui::FullOutput| {
            assert!(
                output.shapes.iter().any(|shape| matches!(&shape.shape,
                    egui::Shape::Rect(rect)
                        if rect.rect.min.distance(expected_rect.min) < 0.1
                            && rect.rect.max.distance(expected_rect.max) < 0.1
                            && shape.clip_rect.contains_rect(expected_rect)
                )),
                "{panel:?}: actual preview background must use {expected_rect:?}"
            );
            let bodies = text_shapes(output, &body_label, expected_rect);
            assert_eq!(
                bodies.len(),
                1,
                "{panel:?}: preview must contain its body once"
            );
            let body = &bodies[0];
            assert!(expected_rect.contains_rect(body.galley.rect.translate(body.pos.to_vec2())));
            assert!(
                (body.galley.size().y - source_body.galley.size().y).abs() < 0.1,
                "{panel:?}: narrowing must relayout the body, not shrink its text"
            );
            assert_eq!(
                body.galley
                    .job
                    .sections
                    .iter()
                    .map(|section| &section.format.font_id)
                    .collect::<Vec<_>>(),
                source_body
                    .galley
                    .job
                    .sections
                    .iter()
                    .map(|section| &section.format.font_id)
                    .collect::<Vec<_>>()
            );
            let preview_header = egui::Rect::from_min_size(
                expected_rect.min,
                egui::vec2(expected_rect.width(), neoflash_ui::metrics::PANEL_HEADER),
            );
            assert_eq!(text_shapes(output, panel.title(), preview_header).len(), 1);
            if compact {
                assert!(expected_rect.width() <= floating[0]);
                assert!(expected_rect.width() < source_group.width());
                for (other, _) in &source.tabs {
                    if *other != panel {
                        assert_eq!(
                            text_shapes(output, other.title(), preview_header).len(),
                            usize::from(group_drag),
                            "{panel:?}: a single-tab preview must not copy the other dock tabs"
                        );
                    }
                }
            } else {
                assert!(body.pos.distance(source_body.pos + (stage - start)) < 0.1);
            }
        };
        let output = render(&mut editor, vec![egui::Event::PointerMoved(stage)]);
        assert!(editor.dock_hit(origin + stage.to_vec2(), 1.).is_none());
        assert_preview(&output);
        assert!(editor.panel_drag_preview_visible());
        assert_eq!(
            editor.workspace, before,
            "preview must preserve saved tab/dock state"
        );
        assert_eq!(
            serde_json::to_value(&editor.project).unwrap(),
            project_before
        );
        assert!(!editor.dirty());
        assert!(!editor.history.can_undo());

        // Content clips travel with the panel between dock targets.
        let edge = egui::pos2(40., 400.);
        let output = render(&mut editor, vec![egui::Event::PointerMoved(edge)]);
        assert!(
            output
                .shapes
                .iter()
                .any(|shape| matches!(&shape.shape, egui::Shape::Text(text)
                if text.galley.job.text == format!("{} · Dock Left", panel.title())))
        );
        let output = render(&mut editor, vec![egui::Event::PointerMoved(stage)]);
        assert_preview(&output);
        let output = render(
            &mut editor,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(!editor.panel_drag_preview_visible());
        assert!(text_shapes(&output, &body_label, expected_rect).is_empty());
        assert_eq!(editor.workspace, before);
        assert_eq!(
            serde_json::to_value(&editor.project).unwrap(),
            project_before
        );
        assert!(!editor.dirty());
        assert!(!editor.history.can_undo());

        // A later tab stays under the pointer and keeps its compact footprint
        // when release creates the native window.
        render(
            &mut editor,
            vec![crate::tests::primary_button(stage, false)],
        );
        render(
            &mut editor,
            vec![
                egui::Event::PointerMoved(start),
                crate::tests::primary_button(start, true),
            ],
        );
        let output = render(&mut editor, vec![egui::Event::PointerMoved(stage)]);
        assert_preview(&output);
        render(
            &mut editor,
            vec![crate::tests::primary_button(stage, false)],
        );
        let geometry = editor.workspace.detached_geometry(panel).unwrap();
        let expected_position = origin + expected_rect.min.to_vec2();
        assert_eq!(
            geometry.outer_position,
            [expected_position.x, expected_position.y]
        );
        // Group weights are fractions (0.68 / 0.32), so compare sizes to a hundredth of a point.
        let [width, height] = geometry.inner_size;
        assert!(
            (width - preview_size.x).abs() < 0.01 && (height - preview_size.y).abs() < 0.01,
            "{:?} vs {preview_size:?}",
            geometry.inner_size
        );
        assert!(!editor.panel_drag_preview_visible());
        assert!(!editor.dirty());
        assert!(!editor.history.can_undo());
    }
}

#[test]
fn immediate_panels_share_state_close_reopen_and_dock_without_duplicate_contents() {
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(false);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let close = Rc::new(RefCell::new(None));
    let outputs = Rc::new(RefCell::new(Vec::new()));
    let (seen_child, close_child, outputs_child) = (seen.clone(), close.clone(), outputs.clone());
    egui::Context::set_immediate_viewport_renderer(move |ctx, mut viewport| {
        let id = viewport.ids.this;
        assert_eq!(viewport.builder.decorations, Some(false));
        assert_eq!(viewport.builder.title_shown, Some(false));
        assert_eq!(viewport.builder.titlebar_shown, Some(false));
        assert_eq!(viewport.builder.titlebar_buttons_shown, Some(false));
        assert_eq!(viewport.builder.close_button, Some(false));
        assert_eq!(viewport.builder.minimize_button, Some(false));
        assert_eq!(viewport.builder.maximize_button, Some(false));
        seen_child.borrow_mut().push(id);
        let mut input = egui::RawInput {
            viewport_id: id,
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900., 600.),
            )),
            ..Default::default()
        };
        input.viewports.insert(
            id,
            egui::ViewportInfo {
                parent: Some(egui::ViewportId::ROOT),
                outer_rect: Some(egui::Rect::from_min_size(
                    if *close_child.borrow() == Some(id) {
                        egui::pos2(360., 280.)
                    } else {
                        egui::pos2(60., 80.)
                    },
                    egui::vec2(900., 630.),
                )),
                inner_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(60., 110.),
                    egui::vec2(900., 600.),
                )),
                events: if *close_child.borrow() == Some(id) {
                    vec![egui::ViewportEvent::Close]
                } else {
                    vec![]
                },
                ..Default::default()
            },
        );
        outputs_child
            .borrow_mut()
            .push(ctx.run(input, |ctx| (viewport.viewport_ui_cb)(ctx)));
    });
    let mut editor = Editor::from_project(&ctx, Project::demo());
    editor.frame = 7;
    editor.select_all();
    let original = editor.project.clone();
    let selection = editor.selection.clone();
    for panel in PanelId::DETACHABLE {
        editor.set_panel_placement(&ctx, panel, PanelPlacement::Detached);
    }
    let render = |editor: &mut Editor, events| {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1440., 960.),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                editor.shortcuts(ctx);
                editor.prepare_docked_panel_focus(ctx);
                editor.dock_grid(ctx);
                editor.stage(ctx);
                editor.detached_panels(ctx);
            },
        );
    };
    render(&mut editor, vec![]);
    assert_eq!(seen.borrow().len(), PanelId::DETACHABLE.len());
    assert_eq!(
        seen.borrow()
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        PanelId::DETACHABLE.len()
    );
    assert!(outputs.borrow().iter().all(|out| !out.shapes.is_empty()));
    let actions_id = egui::ViewportId::from_hash_of(PanelId::Actions.viewport_key().unwrap());
    *close.borrow_mut() = Some(actions_id);
    seen.borrow_mut().clear();
    render(&mut editor, vec![]);
    assert!(!editor.workspace.is_panel_visible(PanelId::Actions));
    assert_eq!(
        editor.workspace.placement(PanelId::Actions),
        PanelPlacement::Detached
    );
    assert_eq!(
        editor
            .workspace
            .detached_geometry(PanelId::Actions)
            .unwrap()
            .outer_position,
        [360., 280.]
    );
    *close.borrow_mut() = None;
    seen.borrow_mut().clear();
    render(
        &mut editor,
        vec![egui::Event::Key {
            key: egui::Key::F9,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(editor.workspace.is_panel_visible(PanelId::Actions));
    assert_eq!(
        seen.borrow().iter().filter(|id| **id == actions_id).count(),
        1
    );
    editor.set_panel_placement(&ctx, PanelId::Actions, PanelPlacement::Docked);
    seen.borrow_mut().clear();
    render(&mut editor, vec![]);
    assert_eq!(seen.borrow().len(), PanelId::DETACHABLE.len() - 1);
    assert_eq!(editor.bottom, BottomTab::Actions);
    editor.reset_active_workspace(&ctx);
    seen.borrow_mut().clear();
    render(&mut editor, vec![]);
    assert!(seen.borrow().is_empty());

    // Exercise the same header handler with a complete gesture batched between
    // paints, then with a release outside the floating viewport over the root
    // dock. Native screen positions stay separate from viewport-local input.
    let library_id = egui::ViewportId::from_hash_of(PanelId::Library.viewport_key().unwrap());
    let header_frame = |editor: &mut Editor, id, origin: egui::Pos2, events| {
        let mut input = egui::RawInput {
            viewport_id: id,
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1440., 960.),
            )),
            events,
            ..Default::default()
        };
        input.viewports.insert(
            id,
            egui::ViewportInfo {
                parent: (id != egui::ViewportId::ROOT).then_some(egui::ViewportId::ROOT),
                inner_rect: Some(egui::Rect::from_min_size(origin, egui::vec2(1440., 960.))),
                ..Default::default()
            },
        );
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                editor.panel_title(ui, PanelId::Library, "Library")
            });
            editor.finalize_panel_drag(ctx);
        });
    };
    let gesture = |to| {
        vec![
            egui::Event::PointerMoved(egui::pos2(20., 14.)),
            crate::tests::primary_button(egui::pos2(20., 14.), true),
            egui::Event::PointerMoved(to),
            crate::tests::primary_button(to, false),
        ]
    };
    header_frame(
        &mut editor,
        egui::ViewportId::ROOT,
        egui::pos2(100., 100.),
        vec![],
    );
    header_frame(
        &mut editor,
        egui::ViewportId::ROOT,
        egui::pos2(100., 100.),
        gesture(egui::pos2(400., 300.)),
    );
    assert!(!editor.workspace.is_panel_docked(PanelId::Library));
    header_frame(&mut editor, library_id, egui::pos2(500., 400.), vec![]);
    header_frame(
        &mut editor,
        library_id,
        egui::pos2(500., 400.),
        vec![
            egui::Event::PointerMoved(egui::pos2(20., 14.)),
            crate::tests::primary_button(egui::pos2(20., 14.), true),
            egui::Event::PointerGone,
        ],
    );
    // The source gets CursorLeft but no release; the receiving root window
    // must still complete the captured drag using its own local coordinates.
    let mut input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1440., 960.),
        )),
        events: vec![
            egui::Event::PointerMoved(egui::pos2(1380., 100.)),
            crate::tests::primary_button(egui::pos2(1380., 100.), false),
        ],
        ..Default::default()
    };
    input
        .viewports
        .get_mut(&egui::ViewportId::ROOT)
        .unwrap()
        .inner_rect = Some(egui::Rect::from_min_size(
        egui::pos2(100., 100.),
        egui::vec2(1440., 960.),
    ));
    let _ = ctx.run(input, |ctx| editor.finalize_panel_drag(ctx));
    assert!(editor.workspace.is_panel_docked(PanelId::Library));

    // The same gesture moves a root tab into a native window and then between
    // two windows. Native identities remain stable when their original panel
    // is no longer the selected tab; each moved panel has exactly one owner.
    editor.set_panel_placement(&ctx, PanelId::Properties, PanelPlacement::Detached);
    render(&mut editor, vec![]);
    header_frame(
        &mut editor,
        egui::ViewportId::ROOT,
        egui::Pos2::ZERO,
        vec![],
    );
    header_frame(
        &mut editor,
        egui::ViewportId::ROOT,
        egui::Pos2::ZERO,
        gesture(egui::pos2(510., 410.)),
    );
    assert_eq!(
        editor.workspace.panel_surface(PanelId::Library),
        DockSurface::Floating(PanelId::Properties)
    );
    editor.set_panel_placement(&ctx, PanelId::Actions, PanelPlacement::Detached);
    render(&mut editor, vec![]);
    let properties_id = egui::ViewportId::from_hash_of(PanelId::Properties.viewport_key().unwrap());
    header_frame(&mut editor, properties_id, egui::pos2(60., 110.), vec![]);
    header_frame(
        &mut editor,
        properties_id,
        egui::pos2(60., 110.),
        gesture(egui::pos2(450., 300.)),
    );
    assert_eq!(
        editor.workspace.panel_surface(PanelId::Library),
        DockSurface::Floating(PanelId::Actions)
    );
    assert_eq!(editor.workspace.floating.len(), 2);
    let before_cancel = editor.workspace.clone();
    header_frame(&mut editor, actions_id, egui::pos2(60., 110.), vec![]);
    header_frame(
        &mut editor,
        actions_id,
        egui::pos2(60., 110.),
        vec![
            egui::Event::PointerMoved(egui::pos2(20., 14.)),
            crate::tests::primary_button(egui::pos2(20., 14.), true),
            egui::Event::PointerMoved(egui::pos2(1300., 800.)),
            egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    assert_eq!(editor.workspace, before_cancel);
    *close.borrow_mut() = Some(actions_id);
    render(&mut editor, vec![]);
    assert!(!editor.workspace.is_panel_visible(PanelId::Actions));
    assert!(!editor.workspace.is_panel_visible(PanelId::Library));
    *close.borrow_mut() = None;
    editor.show_panel(PanelId::Library);
    seen.borrow_mut().clear();
    render(&mut editor, vec![]);
    assert!(seen.borrow().contains(&actions_id));
    assert_eq!(
        editor.workspace.panel_surface(PanelId::Library),
        DockSurface::Floating(PanelId::Actions)
    );
    assert_eq!(editor.project, original);
    assert_eq!(editor.selection, selection);
    assert_eq!(editor.frame, 7);
    assert!(!editor.dirty());
    assert!(!editor.history.can_undo());
}

#[test]
fn borderless_floating_window_moves_by_header_without_self_docking_and_resizes_from_edges() {
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(false);
    let mut editor = Editor::from_project(&ctx, Project::blank());
    editor.show_panel(PanelId::Align);
    editor.set_panel_placement(&ctx, PanelId::Align, PanelPlacement::Detached);
    editor.workspace.remember_floating_geometry(
        PanelId::Align,
        crate::workspace::WindowGeometry {
            outer_position: [300., 200.],
            inner_size: [260., 240.],
        },
    );
    let align_id = egui::ViewportId::from_hash_of(PanelId::Align.viewport_key().unwrap());
    let run_floating_frame = |editor: &mut Editor, origin: egui::Pos2, size: egui::Vec2, events| {
        let mut input = egui::RawInput {
            viewport_id: align_id,
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        };
        let info = egui::ViewportInfo {
            parent: Some(egui::ViewportId::ROOT),
            outer_rect: Some(egui::Rect::from_min_size(origin, size)),
            inner_rect: Some(egui::Rect::from_min_size(origin, size)),
            ..Default::default()
        };
        input.viewports.insert(egui::ViewportId::ROOT, info.clone());
        input.viewports.insert(align_id, info);
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default()
                .frame(egui::Frame::central_panel(&ctx.style()).inner_margin(0.))
                .show(ctx, |ui| editor.floating_dock_contents(ui, PanelId::Align));
            editor.floating_window_resize_borders(
                ctx,
                PanelId::Align,
                &[PanelId::Align],
                [200., 160.],
                1.0,
            );
            editor.finalize_panel_drag(ctx);
        });
    };
    run_floating_frame(
        &mut editor,
        egui::pos2(300., 200.),
        egui::vec2(260., 240.),
        vec![],
    );
    // Dragging the header to (80, 54) — still well inside the 260x240 floating window —
    // must move the floating window to a new outer_position and never self-dock it.
    run_floating_frame(
        &mut editor,
        egui::pos2(300., 200.),
        egui::vec2(260., 240.),
        vec![
            egui::Event::PointerMoved(egui::pos2(30., 14.)),
            crate::tests::primary_button(egui::pos2(30., 14.), true),
            egui::Event::PointerMoved(egui::pos2(80., 54.)),
            crate::tests::primary_button(egui::pos2(80., 54.), false),
        ],
    );
    assert_eq!(
        editor.workspace.panel_surface(PanelId::Align),
        DockSurface::Floating(PanelId::Align)
    );
    let moved = editor
        .workspace
        .detached_geometry(PanelId::Align)
        .expect("Align floating geometry");
    assert_eq!(moved.outer_position, [350., 240.]);
    assert_eq!(
        editor
            .workspace
            .floating_window(PanelId::Align)
            .unwrap()
            .panels(),
        vec![PanelId::Align]
    );
    // Dragging the bottom-right corner of the borderless floating window resizes its inner_size.
    run_floating_frame(
        &mut editor,
        egui::pos2(350., 240.),
        egui::vec2(260., 240.),
        vec![
            egui::Event::PointerMoved(egui::pos2(259., 239.)),
            crate::tests::primary_button(egui::pos2(259., 239.), true),
        ],
    );
    run_floating_frame(
        &mut editor,
        egui::pos2(350., 240.),
        egui::vec2(260., 240.),
        vec![
            egui::Event::PointerMoved(egui::pos2(299., 269.)),
            crate::tests::primary_button(egui::pos2(299., 269.), false),
        ],
    );
    let resized = editor
        .workspace
        .detached_geometry(PanelId::Align)
        .expect("Align resized geometry");
    assert_eq!(resized.outer_position, [350., 240.]);
    assert_eq!(resized.inner_size, [300., 270.]);
}
