//! Exercise real egui immediate-viewport callbacks with simulated window input.
//! OS move/resize and native clipboard acceptance remain live native checks.
use crate::{
    app::{BottomTab, Editor},
    workspace::{DockEdge, DockSurface, PanelId, PanelPlacement, ToolsStageAnchor, WindowGeometry},
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

#[test]
fn floating_minima_scale_once_and_dock_edges_have_a_larger_invisible_halo() {
    assert_eq!(
        crate::detached::scaled_size([320., 220.], 1.0),
        [320., 220.]
    );
    assert_eq!(
        crate::detached::scaled_size([320., 220.], 1.25),
        [400., 275.]
    );
    assert_eq!(
        crate::detached::scaled_size([320., 220.], 1.5),
        [480., 330.]
    );

    let root = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000., 800.));
    let pointer = egui::pos2(500., 70.);
    let visible = crate::dock_grid::edge_targets(root, 1.0)
        .into_iter()
        .find(|(edge, _)| *edge == DockEdge::Top)
        .unwrap()
        .1;
    let acceptance = crate::dock_grid::edge_acceptance_targets(root, 1.0)
        .into_iter()
        .find(|(edge, _)| *edge == DockEdge::Top)
        .unwrap()
        .1;
    assert!(
        !visible.contains(pointer),
        "the approved overlay stays unchanged"
    );
    assert!(
        acceptance.contains(pointer),
        "the invisible return-to-dock halo is deliberately easier to hit"
    );
}

#[test]
fn tools_snap_to_all_eight_stage_anchors_reflow_and_roundtrip() {
    for zoom in [1.0_f32, 1.25, 1.5] {
        let stage =
            egui::Rect::from_min_size(egui::pos2(100., 80.) * zoom, egui::vec2(1200., 760.) * zoom);
        for anchor in ToolsStageAnchor::ALL {
            let logical_width = if anchor.vertical() {
                crate::detached::floating_tools_width()
            } else {
                (2. * neoflash_ui::metrics::tools_two_columns())
                    .max(crate::detached::floating_tools_width())
            };
            let target = crate::detached::apply_tools_stage_anchor(
                anchor,
                stage,
                WindowGeometry {
                    outer_position: [0., 0.],
                    // Simulate geometry saved at a different scale; the anchor owns width.
                    inner_size: [logical_width, 240. * zoom],
                },
                zoom,
            );
            assert_eq!(target.inner_size[0], logical_width * zoom);
            let near = WindowGeometry {
                outer_position: [target.outer_position[0] + 4., target.outer_position[1] + 4.],
                inner_size: target.inner_size,
            };
            let (snapped, geometry) =
                crate::detached::snap_tools_stage_anchor(stage, near, zoom).unwrap();
            assert_eq!(snapped, anchor);
            assert_eq!(geometry.inner_size[0], logical_width * zoom);
            assert_eq!(
                geometry,
                crate::detached::apply_tools_stage_anchor(anchor, stage, geometry, zoom)
            );
        }
    }

    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    editor.show_panel(PanelId::Tools);
    let window = editor.workspace.float_panel(
        PanelId::Tools,
        Some(WindowGeometry {
            outer_position: [100., 100.],
            inner_size: [320., 240.],
        }),
    );
    assert!(
        editor
            .workspace
            .set_tools_stage_anchor(window, Some(ToolsStageAnchor::BottomRight))
    );
    let saved = serde_json::to_string(&editor.workspace).unwrap();
    let restored: crate::workspace::WorkspacePrefs = serde_json::from_str(&saved).unwrap();
    assert_eq!(
        restored.tools_stage_anchor(PanelId::Tools),
        Some(ToolsStageAnchor::BottomRight)
    );
    assert!(restored.tools_popover_prefers_left());

    let render_tools = |editor: &mut Editor| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(400., 720.),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| editor.tools_contents(ui));
            },
        )
    };
    editor
        .workspace
        .set_tools_stage_anchor(window, Some(ToolsStageAnchor::MiddleLeft));
    let _ = render_tools(&mut editor);
    assert_eq!(
        editor.detached.tools_desired_inner_height, None,
        "side-centre anchors must use the one-column vertical rail"
    );
    editor
        .workspace
        .set_tools_stage_anchor(window, Some(ToolsStageAnchor::TopLeft));
    let _ = render_tools(&mut editor);
    assert!(
        editor.detached.tools_desired_inner_height.is_some(),
        "corner anchors keep the responsive horizontal/multi-column layout"
    );
}

#[test]
fn native_borderless_header_exposes_a_working_close_control() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    editor.show_panel(PanelId::Align);
    editor.set_panel_placement(&ctx, PanelId::Align, PanelPlacement::Detached);
    let mut open = true;
    let mut render = |events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(360., 260.),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::central_panel(&ctx.style()).inner_margin(0.))
                    .show(ctx, |ui| {
                        open = editor.native_floating_panel(
                            ui,
                            PanelId::Align,
                            &[PanelId::Align],
                            [248., 190.],
                            1.0,
                        );
                    });
            },
        )
    };
    let first = render(vec![]);
    let nodes = &first
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes;
    let title_nodes: Vec<_> = nodes
        .iter()
        .filter_map(|(_, node)| {
            (node.label() == Some("Align"))
                .then(|| node.bounds())
                .flatten()
        })
        .collect();
    assert_eq!(
        title_nodes.len(),
        1,
        "a one-group native window must have one title/tab header, not nested duplicate chrome"
    );
    assert!(
        title_nodes[0].y1 <= f64::from(neoflash_ui::metrics::panel_tab() + 1.),
        "the only title stays inside the locked floating header"
    );
    let close = first
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .find_map(|(_, node)| {
            (node.label() == Some("Close panel"))
                .then(|| node.bounds())
                .flatten()
                .map(|rect| {
                    egui::pos2(
                        ((rect.x0 + rect.x1) * 0.5) as f32,
                        ((rect.y0 + rect.y1) * 0.5) as f32,
                    )
                })
        })
        .expect("locked NF2 floating header must expose Close panel");
    render(vec![
        egui::Event::PointerMoved(close),
        crate::tests::primary_button(close, true),
    ]);
    render(vec![crate::tests::primary_button(close, false)]);
    drop(render);
    assert!(!open);
}

#[test]
fn collapsed_native_header_retires_the_expanded_body_dock_targets() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    editor.show_panel(PanelId::Align);
    editor.set_panel_placement(&ctx, PanelId::Align, PanelPlacement::Detached);
    let render = |editor: &mut Editor, events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(360., 260.),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::central_panel(&ctx.style()).inner_margin(0.))
                    .show(ctx, |ui| {
                        let _ = editor.native_floating_panel(
                            ui,
                            PanelId::Align,
                            &[PanelId::Align],
                            [248., 190.],
                            1.0,
                        );
                    });
            },
        )
    };
    let expanded = render(&mut editor, Vec::new());
    let surface = DockSurface::Floating(PanelId::Align);
    let old_body = editor
        .detached
        .dock_viewports
        .iter()
        .find(|target| target.surface == surface)
        .expect("expanded floating viewport target")
        .content
        .center();
    assert!(
        editor
            .detached
            .dock_groups
            .iter()
            .any(|target| target.surface == surface)
    );
    let collapse = expanded
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .find_map(|(_, node)| {
            (node.label() == Some("Collapse panel"))
                .then(|| node.bounds())
                .flatten()
                .map(|rect| {
                    egui::pos2(
                        ((rect.x0 + rect.x1) * 0.5) as f32,
                        ((rect.y0 + rect.y1) * 0.5) as f32,
                    )
                })
        })
        .expect("Collapse panel button");
    let _ = render(
        &mut editor,
        vec![
            egui::Event::PointerMoved(collapse),
            crate::tests::primary_button(collapse, true),
            crate::tests::primary_button(collapse, false),
        ],
    );
    let _ = render(&mut editor, Vec::new());
    assert!(
        editor
            .detached
            .dock_viewports
            .iter()
            .all(|target| target.surface != surface)
    );
    assert!(
        editor
            .detached
            .dock_groups
            .iter()
            .all(|target| target.surface != surface)
    );
    assert_ne!(
        editor
            .dock_surface_at(old_body)
            .map(|target| target.surface),
        Some(surface),
        "the former body must not remain an invisible docking surface"
    );
}

#[test]
fn floating_tools_header_fits_its_title_and_window_actions() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    editor.show_panel(PanelId::Tools);
    editor.set_panel_placement(&ctx, PanelId::Tools, PanelPlacement::Detached);
    let width = crate::detached::floating_tools_width();
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 260.),
            )),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default()
                .frame(egui::Frame::central_panel(&ctx.style()).inner_margin(0.))
                .show(ctx, |ui| {
                    let _ = editor.native_floating_panel(
                        ui,
                        PanelId::Tools,
                        &[PanelId::Tools],
                        [width, 160.],
                        1.0,
                    );
                });
        },
    );
    let nodes = &output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("floating Tools accessibility tree")
        .nodes;
    let bounds = |label: &str| {
        nodes.iter().find_map(|(_, node)| {
            (node.label() == Some(label))
                .then(|| node.bounds())
                .flatten()
        })
    };
    let tools = bounds("Tools").expect("Tools tab");
    let menu = bounds("Panel menu").expect("floating menu");
    for label in ["Tools", "Panel menu", "Collapse panel", "Close panel"] {
        let rect = bounds(label).unwrap_or_else(|| panic!("missing {label}"));
        assert!(rect.x0 >= -0.5 && rect.x1 <= f64::from(width + 0.5));
        assert!(rect.y1 <= f64::from(neoflash_ui::metrics::panel_tab() + 0.5));
    }
    assert!(
        tools.x1 <= menu.x0,
        "Tools title must not overlap window actions"
    );
}

#[test]
fn single_group_native_header_keeps_overflow_tabs_panel_commands_and_tab_targets() {
    fn render(
        editor: &mut Editor,
        ctx: &egui::Context,
        window: PanelId,
        panels: &[PanelId],
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(520., 360.),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::central_panel(&ctx.style()).inner_margin(0.))
                    .show(ctx, |ui| {
                        let _ = editor.native_floating_panel(ui, window, panels, [320., 220.], 1.0);
                    });
            },
        )
    }
    fn center(output: &egui::FullOutput, label: &str) -> Option<egui::Pos2> {
        output
            .platform_output
            .accesskit_update
            .as_ref()?
            .nodes
            .iter()
            .find_map(|(_, node)| {
                (node.label() == Some(label))
                    .then(|| node.bounds())
                    .flatten()
                    .map(|rect| {
                        egui::pos2(
                            ((rect.x0 + rect.x1) * 0.5) as f32,
                            ((rect.y0 + rect.y1) * 0.5) as f32,
                        )
                    })
            })
    }

    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    let panels = [
        PanelId::Align,
        PanelId::Transform,
        PanelId::Color,
        PanelId::Info,
        PanelId::Filters,
    ];
    for panel in panels {
        editor.show_panel(panel);
    }
    let window = editor.workspace.float_group(
        &panels,
        PanelId::Align,
        WindowGeometry {
            outer_position: [120., 100.],
            inner_size: [520., 360.],
        },
    );
    let first = render(&mut editor, &ctx, window, &panels, Vec::new());
    let target = editor
        .detached
        .dock_groups
        .iter()
        .find(|target| target.surface == DockSurface::Floating(window))
        .expect("single floating group target");
    assert_eq!(
        target.tabs.len(),
        3,
        "visible native tabs remain individual targets"
    );
    let overflow = panels
        .into_iter()
        .find(|panel| !target.tabs.iter().any(|(tab, _)| tab == panel))
        .expect("fourth/fifth tab is available through overflow");

    let menu = center(&first, "Panel menu").expect("Panel menu button");
    let _ = render(
        &mut editor,
        &ctx,
        window,
        &panels,
        vec![
            egui::Event::PointerMoved(menu),
            crate::tests::primary_button(menu, true),
            crate::tests::primary_button(menu, false),
        ],
    );
    let mut opened = render(&mut editor, &ctx, window, &panels, Vec::new());
    if center(&opened, overflow.title()).is_none() {
        opened = render(&mut editor, &ctx, window, &panels, Vec::new());
    }
    assert!(center(&opened, "Dock Left").is_some());
    assert!(center(&opened, "Hide").is_some());
    let overflow_row = center(&opened, overflow.title()).expect("overflow panel menu row");
    let _ = render(
        &mut editor,
        &ctx,
        window,
        &panels,
        vec![
            egui::Event::PointerMoved(overflow_row),
            crate::tests::primary_button(overflow_row, true),
            crate::tests::primary_button(overflow_row, false),
        ],
    );
    let active = editor
        .workspace
        .floating
        .iter()
        .find(|floating| floating.id == window)
        .and_then(|floating| match floating.root.as_ref()? {
            crate::workspace::FloatingNode::Group(group) => Some(group.active),
            crate::workspace::FloatingNode::Split { .. } => None,
        });
    assert_eq!(active, Some(overflow));
}

#[test]
fn single_group_floating_header_limits_tabs_to_its_real_minimum_width() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    let panels = [
        PanelId::Align,
        PanelId::Transform,
        PanelId::Color,
        PanelId::Info,
        PanelId::Filters,
    ];
    for panel in panels {
        editor.show_panel(panel);
    }
    let minimum = crate::workspace::minimum_window_size(PanelId::Filters)[0];
    let window = editor.workspace.float_group(
        &panels,
        PanelId::Align,
        WindowGeometry {
            outer_position: [120., 100.],
            inner_size: [minimum, 300.],
        },
    );
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(minimum, 300.),
            )),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default()
                .frame(egui::Frame::central_panel(&ctx.style()).inner_margin(0.))
                .show(ctx, |ui| {
                    let _ = editor.native_floating_panel(ui, window, &panels, [minimum, 260.], 1.0);
                });
        },
    );
    let target = editor
        .detached
        .dock_groups
        .iter()
        .find(|target| target.surface == DockSurface::Floating(window))
        .expect("minimum-width single group target");
    assert!(
        (1..3).contains(&target.tabs.len()),
        "minimum width shows only the tabs that fit; remaining tabs stay in overflow"
    );
    assert!(
        target
            .tabs
            .iter()
            .any(|(panel, _)| *panel == PanelId::Align)
    );
    let nodes = &output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("minimum-width header accessibility tree")
        .nodes;
    let bounds = |label: &str| {
        nodes.iter().find_map(|(_, node)| {
            (node.label() == Some(label))
                .then(|| node.bounds())
                .flatten()
        })
    };
    let menu = bounds("Panel menu").expect("Panel menu");
    for label in ["Panel menu", "Collapse panel", "Close panel"] {
        let rect = bounds(label).unwrap();
        assert!(rect.x0 >= 0.0 && rect.x1 <= f64::from(minimum));
    }
    assert!(
        target.tabs.last().unwrap().1.right() <= menu.x0 as f32,
        "visible tabs must stop before the fixed window actions"
    );
}

#[test]
fn focusing_a_revealed_panel_targets_its_docked_or_detached_surface() {
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    editor.show_panel(PanelId::Color);
    let docked = ctx.run(egui::RawInput::default(), |ctx| {
        editor.focus_dock_panel(ctx, PanelId::Color);
    });
    assert!(
        docked
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .is_some_and(|output| output.commands.contains(&egui::ViewportCommand::Focus))
    );

    editor.set_panel_placement(&ctx, PanelId::Color, PanelPlacement::Detached);
    let detached_id = egui::ViewportId::from_hash_of(PanelId::Color.viewport_key().unwrap());
    let mut input = egui::RawInput::default();
    input.viewports.insert(
        detached_id,
        egui::ViewportInfo {
            parent: Some(egui::ViewportId::ROOT),
            ..Default::default()
        },
    );
    let _ = ctx.run(input, |ctx| {
        editor.focus_dock_panel(ctx, PanelId::Color);
    });
    assert_eq!(
        editor.workspace.panel_surface(PanelId::Color),
        DockSurface::Floating(PanelId::Color)
    );
    assert_eq!(editor.detached.focus, Some(PanelId::Color));
}
