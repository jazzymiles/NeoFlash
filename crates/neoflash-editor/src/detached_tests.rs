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
            PanelId::Actions => format!(
                "{} · {} · Frame 1",
                editor.symbol().name,
                editor.symbol().timeline.layers[editor.layer].name
            ),
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
                crate::workspace::maximum_window_size(PanelId::Align),
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
    let halo_pointer = egui::pos2(500., 30.);
    let premature_stage_pointer = egui::pos2(500., 70.);
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
        !visible.contains(halo_pointer),
        "the visible edge target stays compact (~28..34 pt)"
    );
    assert!(
        acceptance.contains(halo_pointer),
        "the invisible return-to-dock halo remains slightly easier to hit (~32..44 pt)"
    );
    assert!(
        !acceptance.contains(premature_stage_pointer),
        "tightened acceptance halo must not capture pointers far inside the Stage"
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

    // Narrow 1-column (42 pt) and 2-column (88 pt) widths use the compact TOOLS_GRIP + More (⋯) header.
    for narrow_width in [
        crate::detached::floating_tools_width(),
        neoflash_ui::metrics::tools_two_columns(),
    ] {
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(narrow_width, 680.),
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
                            [crate::detached::floating_tools_width(), 160.],
                            1.0,
                        );
                    });
            },
        );
        let nodes = &output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("narrow floating Tools accessibility tree")
            .nodes;
        let bounds = |label: &str| {
            nodes.iter().find_map(|(_, node)| {
                (node.label() == Some(label))
                    .then(|| node.bounds())
                    .flatten()
            })
        };
        let tools = bounds("Tools").expect("Tools grip handle");
        let menu = bounds("Panel menu").expect("More menu button");
        for label in ["Tools", "Panel menu"] {
            let rect = bounds(label).unwrap_or_else(|| panic!("missing {label}"));
            assert!(
                rect.x0 >= -0.5 && rect.x1 <= f64::from(narrow_width + 0.5),
                "{label} bounds {rect:?} must fit within narrow width {narrow_width}"
            );
            assert!(rect.y1 <= f64::from(neoflash_ui::metrics::panel_tab() + 0.5));
        }
        assert!(
            tools.x1 <= menu.x0 + 0.5,
            "Tools grip must not overlap More menu button at width {narrow_width}"
        );
    }

    // Wide 5-column width fits the full titled FloatingPanelHeader and all 3 action buttons.
    let wide_width = crate::detached::floating_full_tools_header_width();
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(wide_width, 260.),
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
                        [crate::detached::floating_tools_width(), 160.],
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
        assert!(rect.x0 >= -0.5 && rect.x1 <= f64::from(wide_width + 0.5));
        assert!(rect.y1 <= f64::from(neoflash_ui::metrics::panel_tab() + 0.5));
    }
    assert!(
        tools.x1 <= menu.x0,
        "Tools title must not overlap window actions"
    );
}

#[test]
fn detached_tools_supports_one_two_and_five_column_widths_full_painting_compaction_and_roundtrip() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    editor.show_panel(PanelId::Tools);
    editor.set_panel_placement(&ctx, PanelId::Tools, PanelPlacement::Detached);

    let min_w = crate::workspace::minimum_window_size(PanelId::Tools)[0];
    assert!(
        (40.0..=44.0).contains(&min_w),
        "Tools minimum width {min_w} must be 1 tool button + NF2 padding/border (~40-44 pt)"
    );
    assert_eq!(min_w, crate::detached::floating_tools_width());

    // Helper that renders `Tools` at `width` (and optional wider `inner_width` to simulate live native resize)
    // and returns (Select bounds, Subselect bounds, Line bounds, desired_height, max painted bg right edge).
    let mut inspect_layout = |width: f32, inner_width: f32| {
        let tools_id = egui::ViewportId::from_hash_of(PanelId::Tools.viewport_key().unwrap());
        let mut input = egui::RawInput {
            viewport_id: tools_id,
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 720.),
            )),
            ..Default::default()
        };
        input.viewports.insert(
            tools_id,
            egui::ViewportInfo {
                parent: Some(egui::ViewportId::ROOT),
                outer_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(120., 90.),
                    egui::vec2(inner_width, 720.),
                )),
                inner_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(120., 90.),
                    egui::vec2(inner_width, 720.),
                )),
                ..Default::default()
            },
        );
        let bg_color = neoflash_ui::palette(&ctx).bg_panel;
        let output = ctx.run(input, |ctx| {
            let viewport_rect = ctx.viewport_rect();
            let full_rect = egui::Rect::from_min_size(
                viewport_rect.min,
                egui::vec2(viewport_rect.width().max(inner_width).max(min_w), 720.),
            );
            ctx.layer_painter(egui::LayerId::background())
                .rect_filled(full_rect, 0.0, bg_color);
            egui::CentralPanel::default()
                .frame(neoflash_ui::components::Surface::PanelFlush.frame(ctx))
                .show(ctx, |ui| {
                    ui.set_min_size(viewport_rect.size());
                    let _ = editor.native_floating_panel(
                        ui,
                        PanelId::Tools,
                        &[PanelId::Tools],
                        [min_w, 160.],
                        1.0,
                    );
                });
        });
        let max_bg_right = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(r) if r.fill == bg_color && r.rect.left() <= 0.5 => {
                    Some(r.rect.right())
                }
                _ => None,
            })
            .fold(0.0_f32, f32::max);
        let nodes = &output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("Tools accessibility nodes")
            .nodes;
        let tool_bounds = |prefix: &str| {
            nodes.iter().find_map(|(_, node)| {
                node.label()
                    .filter(|l| l.starts_with(prefix))
                    .and_then(|_| node.bounds())
            })
        };
        (
            tool_bounds("Select").unwrap_or_else(|| panic!("missing Select at width {width}")),
            tool_bounds("Subselect"),
            tool_bounds("Free Transform")
                .unwrap_or_else(|| panic!("missing Free Transform at width {width}")),
            tool_bounds("Lasso").unwrap_or_else(|| panic!("missing Lasso at width {width}")),
            editor
                .detached
                .tools_desired_inner_height
                .expect("desired inner height recorded"),
            max_bg_right,
        )
    };

    // 1) 1-column layout at minimum width (42 pt), also simulating a wider native inner_rect (188 pt)
    //    to prove the background paint covers 100% of the viewport with no black vertical strip.
    let (sel_1, sub_1, tr_1, lasso_1, h_1col, bg_right_1) = inspect_layout(min_w, 188.0);
    assert!(
        sub_1.is_none(),
        "1-column tool_rail groups Selection + Subselection into one flyout button"
    );
    assert!(
        (sel_1.x0 - tr_1.x0).abs() < 1.0
            && (sel_1.x0 - lasso_1.x0).abs() < 1.0
            && tr_1.y0 > sel_1.y0
            && lasso_1.y0 > tr_1.y0,
        "1-column width ({min_w} pt) must stack tool groups in a single vertical column"
    );
    assert!(
        bg_right_1 >= 188.0 - 0.5,
        "background fill ({bg_right_1}) must cover the full native inner width without any black strip"
    );

    // 2) 2-column layout (88 pt).
    let two_col_w = neoflash_ui::metrics::tools_two_columns();
    let (sel_2, sub_2, tr_2, _lasso_2, h_2col, bg_right_2) = inspect_layout(two_col_w, two_col_w);
    let sub_2 = sub_2.expect("2-column layout shows Subselection directly");
    assert!(
        (sel_2.y0 - sub_2.y0).abs() < 1.0 && sub_2.x0 > sel_2.x0,
        "2-column width ({two_col_w} pt) must place Select and Subselect side-by-side"
    );
    assert!(
        tr_2.y0 > sel_2.y0,
        "2-column width wraps subsequent tools onto lower rows"
    );
    assert!(bg_right_2 >= two_col_w - 0.5);

    // 3) 3-column (132 pt), 4-column (176 pt), and 5-column (220 pt) layouts.
    let (_, _, _, _, h_3col, _) = inspect_layout(132.0, 132.0);
    let (_, _, _, _, h_4col, _) = inspect_layout(176.0, 176.0);
    let (sel_5, sub_5, _tr_5, lasso_5, h_5col, bg_right_5) = inspect_layout(220.0, 220.0);
    let sub_5 = sub_5.expect("5-column layout shows Subselection directly");
    assert!(
        (sel_5.y0 - sub_5.y0).abs() < 1.0
            && (sel_5.y0 - lasso_5.y0).abs() < 1.0
            && lasso_5.x0 > sub_5.x0,
        "5-column width (220 pt) places Select, Subselect, Line, Lasso, Pen across the first row"
    );
    assert!(bg_right_5 >= 220.0 - 0.5);
    assert!(
        h_1col > h_2col && h_2col > h_3col && h_3col >= h_4col && h_4col >= h_5col,
        "desired height must monotonically compact as column count increases: 1col={h_1col}, 2col={h_2col}, 3col={h_3col}, 4col={h_4col}, 5col={h_5col}"
    );

    // 4) Bottom-up height compaction keeps outer_position[1] (top edge) fixed across 1, 2, 3, 4, and 5 columns.
    let top_y = 96.0_f32;
    let tools_id = egui::ViewportId::from_hash_of(PanelId::Tools.viewport_key().unwrap());
    editor
        .workspace
        .set_tools_stage_anchor(PanelId::Tools, None);
    for (col_w, desired_h) in [
        (min_w, h_1col),
        (two_col_w, h_2col),
        (132.0, h_3col),
        (176.0, h_4col),
        (220.0, h_5col),
        (min_w, h_1col),
    ] {
        editor.workspace.remember_floating_geometry(
            PanelId::Tools,
            WindowGeometry {
                outer_position: [140.0, top_y],
                inner_size: [col_w, 720.0],
            },
        );
        let mut input = egui::RawInput {
            viewport_id: tools_id,
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(col_w, 720.0),
            )),
            ..Default::default()
        };
        input.viewports.insert(
            tools_id,
            egui::ViewportInfo {
                parent: Some(egui::ViewportId::ROOT),
                outer_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(140.0, top_y),
                    egui::vec2(col_w, 720.0),
                )),
                inner_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(140.0, top_y),
                    egui::vec2(col_w, 720.0),
                )),
                ..Default::default()
            },
        );
        let _ = ctx.run(input, |ctx| {
            editor.compact_floating_tools_window_bottom_up(
                ctx,
                PanelId::Tools,
                desired_h,
                min_w,
                160.0,
                1.0,
            );
        });
        let compacted = editor
            .workspace
            .detached_geometry(PanelId::Tools)
            .expect("compacted Tools geometry");
        assert_eq!(
            compacted.outer_position,
            [140.0, top_y],
            "bottom-up compaction at width {col_w} must keep outer_position[1] (top edge) fixed"
        );
        assert!(
            (compacted.inner_size[0] - col_w).abs() < 0.5
                && (compacted.inner_size[1] - desired_h).abs() < 0.5,
            "compacted inner_size {:?} must match [{col_w}, {desired_h}]",
            compacted.inner_size
        );
    }

    // 5) Persisted geometry round-trip across serialization, UI scale (zoom), and monitor recovery.
    let monitors = [crate::workspace::ScreenRect {
        origin: [0.0, 0.0],
        size: [2560.0, 1440.0],
    }];
    let fallback = monitors[0];
    for zoom in [1.0_f32, 1.25, 1.5, 2.0] {
        for logical_w in [min_w, two_col_w, 188.0] {
            let saved = WindowGeometry {
                outer_position: [24.0 * zoom, 80.0 * zoom],
                inner_size: [logical_w * zoom, h_2col * zoom],
            };
            editor
                .workspace
                .remember_floating_geometry(PanelId::Tools, saved);
            let json = serde_json::to_string(&editor.workspace).unwrap();
            let restored: crate::workspace::WorkspacePrefs = serde_json::from_str(&json).unwrap();
            let roundtrip = restored.detached_geometry(PanelId::Tools).unwrap();
            let recovered = crate::workspace::recover_geometry_with_bounds(
                PanelId::Tools,
                Some(roundtrip),
                crate::detached::scaled_size([min_w, 160.0], zoom),
                crate::detached::scaled_size(
                    crate::workspace::maximum_window_size(PanelId::Tools),
                    zoom,
                ),
                &monitors,
                fallback,
            );
            assert_eq!(
                recovered, saved,
                "1-col/2-col/5-col Tools geometry must round-trip cleanly at zoom {zoom}"
            );
        }
    }
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

#[test]
fn tools_undocks_from_dots_keeps_two_column_title_and_redocks_by_drag() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    ctx.set_embed_viewports(false);
    let mut editor = Editor::from_project(&ctx, Project::blank());
    editor.show_panel(PanelId::Tools);
    assert!(editor.workspace.is_panel_docked(PanelId::Tools));

    let root_origin = egui::pos2(80., 60.);
    let root_size = egui::vec2(1440., 900.);
    let render_root = |editor: &mut Editor, events| {
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, root_size)),
            events,
            ..Default::default()
        };
        input
            .viewports
            .get_mut(&egui::ViewportId::ROOT)
            .unwrap()
            .inner_rect = Some(egui::Rect::from_min_size(root_origin, root_size));
        ctx.run(input, |ctx| {
            editor.dock_grid(ctx);
            editor.stage(ctx);
            editor.finalize_panel_drag(ctx);
        })
    };

    // 1) Docked Tools: clicking `⋯` opens the panel menu, while dragging `⋯` tears Tools off cleanly.
    let first = render_root(&mut editor, vec![]);
    let tools_group = editor
        .detached
        .dock_groups
        .iter()
        .find(|g| g.tabs.iter().any(|(p, _)| *p == PanelId::Tools))
        .expect("docked Tools group")
        .clone();
    let local_header = tools_group.header.translate(-root_origin.to_vec2());
    let dots_pos = egui::pos2(local_header.right() - 20., local_header.center().y);
    assert!(
        first
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .any(|(_, n)| n.label() == Some("Panel commands and tabs")),
        "docked Tools header must expose the More (⋯) control"
    );
    // Click without drag keeps Tools docked and opens the popup menu.
    let _ = render_root(
        &mut editor,
        vec![
            egui::Event::PointerMoved(dots_pos),
            crate::tests::primary_button(dots_pos, true),
            crate::tests::primary_button(dots_pos, false),
        ],
    );
    let clicked = render_root(&mut editor, vec![]);
    assert!(editor.workspace.is_panel_docked(PanelId::Tools));
    assert!(
        clicked
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .any(|(_, n)| n.label() == Some("Detach")),
        "clicking `⋯` without dragging must still open the panel menu"
    );
    // Close popup by pressing Escape, then drag `⋯` out onto the center of Stage (460, 320).
    let _ = render_root(
        &mut editor,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    let _ = render_root(
        &mut editor,
        vec![
            egui::Event::PointerMoved(dots_pos),
            crate::tests::primary_button(dots_pos, true),
        ],
    );
    let _ = render_root(
        &mut editor,
        vec![egui::Event::PointerMoved(egui::pos2(460., 320.))],
    );
    let _ = render_root(
        &mut editor,
        vec![crate::tests::primary_button(egui::pos2(460., 320.), false)],
    );
    assert!(
        !editor.workspace.is_panel_docked(PanelId::Tools),
        "dragging `⋯` on docked Tools must detach Tools"
    );

    // 2) Detached 2-column (88 pt) Tools keeps the visible `"Tools"` text instead of replacing it
    //    with horizontal grip dots, whereas 1-column (42 pt) uses GRIP_H without text overflow.
    let tools_id = egui::ViewportId::from_hash_of(PanelId::Tools.viewport_key().unwrap());
    let render_floating_tools = |editor: &mut Editor, width: f32, origin: egui::Pos2, events| {
        let mut input = egui::RawInput {
            viewport_id: tools_id,
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 460.),
            )),
            events,
            ..Default::default()
        };
        input.viewports.insert(
            tools_id,
            egui::ViewportInfo {
                parent: Some(egui::ViewportId::ROOT),
                outer_rect: Some(egui::Rect::from_min_size(origin, egui::vec2(width, 460.))),
                inner_rect: Some(egui::Rect::from_min_size(origin, egui::vec2(width, 460.))),
                ..Default::default()
            },
        );
        ctx.run(input, |ctx| {
            egui::CentralPanel::default()
                .frame(egui::Frame::central_panel(&ctx.style()).inner_margin(0.))
                .show(ctx, |ui| {
                    let _ = editor.native_floating_panel(
                        ui,
                        PanelId::Tools,
                        &[PanelId::Tools],
                        [crate::detached::floating_tools_width(), 160.],
                        1.0,
                    );
                });
        })
    };
    let two_col = render_floating_tools(
        &mut editor,
        neoflash_ui::metrics::tools_two_columns(),
        egui::pos2(480., 300.),
        vec![],
    );
    assert!(
        two_col.shapes.iter().any(|s| matches!(
            &s.shape,
            egui::Shape::Text(t) if t.galley.job.text == "Tools"
        )),
        "2-column detached Tools must retain the visible 'Tools' title text"
    );
    let one_col = render_floating_tools(
        &mut editor,
        crate::detached::floating_tools_width(),
        egui::pos2(480., 300.),
        vec![],
    );
    assert!(
        !one_col.shapes.iter().any(|s| matches!(
            &s.shape,
            egui::Shape::Text(t) if t.galley.job.text == "Tools"
        )),
        "1-column (42 pt) detached Tools uses the compact grip icon instead of clipped text"
    );

    // 3) Dragging detached Tools (even from `⋯`, and even into the top-left corner of Main)
    //    re-docks Tools cleanly into the Left dock rail instead of Stage-anchoring it.
    let _ = render_root(&mut editor, vec![]);
    let float_origin = egui::pos2(480., 300.);
    let two_col_w = neoflash_ui::metrics::tools_two_columns();
    let float_dots = egui::pos2(two_col_w - 18., 18.);
    let _ = render_floating_tools(
        &mut editor,
        two_col_w,
        float_origin,
        vec![
            egui::Event::PointerMoved(float_dots),
            crate::tests::primary_button(float_dots, true),
            egui::Event::PointerGone,
        ],
    );
    // Drop near the top-left corner of Main (x = 28, y = 18 inside the 32 pt top bar above Left rail).
    let _ = render_root(
        &mut editor,
        vec![
            egui::Event::PointerMoved(egui::pos2(28., 18.)),
            crate::tests::primary_button(egui::pos2(28., 18.), false),
        ],
    );
    assert!(
        editor.workspace.is_panel_docked(PanelId::Tools),
        "dragging detached Tools back to the main window rail (including top-left corner) must dock it"
    );
}

#[test]
fn t319_ctx_data_modal_disables_dock_grid_and_detached_viewports_and_records_child_focus() {
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(false);
    let child_prev_focus_recorded = Rc::new(RefCell::new(false));
    let focus_child = child_prev_focus_recorded.clone();

    egui::Context::set_immediate_viewport_renderer(move |ctx, mut viewport| {
        let id = viewport.ids.this;
        let mut input = egui::RawInput {
            viewport_id: id,
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(420., 520.),
            )),
            ..Default::default()
        };
        input.viewports.insert(
            id,
            egui::ViewportInfo {
                parent: Some(egui::ViewportId::ROOT),
                outer_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(120., 120.),
                    egui::vec2(420., 520.),
                )),
                inner_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(120., 120.),
                    egui::vec2(420., 520.),
                )),
                ..Default::default()
            },
        );
        let _ = ctx.run(input, |ctx| {
            // Focus a synthetic widget inside the child viewport to confirm `record_end_of_frame_focus` snapshots it.
            let focus_id = egui::Id::new("t319-child-focused-input");
            egui::Area::new(egui::Id::new("t319-child-focus-probe"))
                .fixed_pos(egui::pos2(4.0, 4.0))
                .show(ctx, |ui| {
                    let mut txt = String::from("abc");
                    let r = ui.add(egui::TextEdit::singleline(&mut txt).id(focus_id));
                    r.request_focus();
                });
            (viewport.viewport_ui_cb)(ctx);
            *focus_child.borrow_mut() =
                crate::panels::dialog_helpers::prev_wants_keyboard_input(ctx);
        });
    });

    let mut editor = Editor::from_project(&ctx, Project::demo());
    editor.set_panel_placement(&ctx, PanelId::Properties, PanelPlacement::Detached);

    let render = |editor: &mut Editor, events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1440., 900.),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                editor.shortcuts(ctx);
                editor.prepare_docked_panel_focus(ctx);
                editor.dock_grid(ctx);
                editor.stage(ctx);
                editor.draw_dialogs(ctx);
                editor.detached_panels(ctx);
                crate::panels::dialog_helpers::record_end_of_frame_focus(ctx);
            },
        )
    };

    render(&mut editor, vec![]);
    assert!(
        *child_prev_focus_recorded.borrow(),
        "detached viewport must record end-of-frame focus snapshot for Escape isolation"
    );

    // Locate an inactive bottom-dock tab (e.g. Output while Timeline is active).
    let output_tab_center = editor
        .detached
        .dock_groups
        .iter()
        .find_map(|g| {
            g.tabs
                .iter()
                .find(|(id, _)| *id == PanelId::Output)
                .map(|(_, r)| r.center())
        })
        .expect("Output tab exists in Classic bottom dock");
    assert!(editor.workspace.is_panel_active(PanelId::Timeline));

    // Open a `ctx.data`-backed modal (`Layer properties`): `has_modal_dialog()` is false, `has_modal_dialog_in(&ctx)` is true!
    editor.open_layer_properties(&ctx, 0);
    assert!(!editor.has_modal_dialog());
    assert!(editor.has_modal_dialog_in(&ctx));
    render(&mut editor, vec![]);

    // Clicking the inactive Output dock tab while Layer properties is open MUST be blocked by `has_modal_dialog_in`.
    render(
        &mut editor,
        vec![
            egui::Event::PointerMoved(output_tab_center),
            crate::tests::primary_button(output_tab_center, true),
            crate::tests::primary_button(output_tab_center, false),
        ],
    );
    render(&mut editor, vec![]);
    assert!(
        editor.workspace.is_panel_active(PanelId::Timeline),
        "dock_grid tab clicks must be blocked when a ctx.data modal (Layer properties) is open"
    );

    editor.cancel_layer_properties(&ctx);
    assert!(!editor.has_modal_dialog_in(&ctx));
}

#[test]
fn t372_floating_actions_and_studio_panels_stay_above_timeline_clicks_and_yield_to_help_modals_and_menus()
 {
    use crate::workspace::{PanelId, PanelPlacement};
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;

    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    ctx.set_embed_viewports(false);
    let observed_levels: Rc<RefCell<HashMap<egui::ViewportId, Option<egui::WindowLevel>>>> =
        Rc::new(RefCell::new(HashMap::new()));
    let observed_cmds: Rc<RefCell<HashMap<egui::ViewportId, Vec<egui::ViewportCommand>>>> =
        Rc::new(RefCell::new(HashMap::new()));
    let levels_cb = observed_levels.clone();
    let cmds_cb = observed_cmds.clone();
    egui::Context::set_immediate_viewport_renderer(move |ctx, mut viewport| {
        let id = viewport.ids.this;
        levels_cb
            .borrow_mut()
            .insert(id, viewport.builder.window_level);
        let mut input = egui::RawInput {
            viewport_id: id,
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(640., 520.),
            )),
            ..Default::default()
        };
        input.viewports.insert(
            id,
            egui::ViewportInfo {
                parent: Some(egui::ViewportId::ROOT),
                outer_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(120., 80.),
                    egui::vec2(640., 520.),
                )),
                inner_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(120., 80.),
                    egui::vec2(640., 520.),
                )),
                ..Default::default()
            },
        );
        let out = ctx.run(input, |ctx| {
            (viewport.viewport_ui_cb)(ctx);
        });
        if let Some(vo) = out.viewport_output.get(&id) {
            cmds_cb.borrow_mut().insert(id, vo.commands.clone());
        }
    });

    let mut project = Project::demo();
    project.symbols[0].timeline.frame_count = 12;
    let mut editor = Editor::from_project(&ctx, project);
    for panel in [
        PanelId::Actions,
        PanelId::Properties,
        PanelId::Library,
        PanelId::Output,
        PanelId::Tools,
    ] {
        editor.set_panel_visible(panel, true);
        editor.set_panel_placement(&ctx, panel, PanelPlacement::Detached);
    }

    let actions_vp = egui::ViewportId::from_hash_of(PanelId::Actions.viewport_key().unwrap());
    let props_vp = egui::ViewportId::from_hash_of(PanelId::Properties.viewport_key().unwrap());
    let library_vp = egui::ViewportId::from_hash_of(PanelId::Library.viewport_key().unwrap());
    let output_vp = egui::ViewportId::from_hash_of(PanelId::Output.viewport_key().unwrap());

    let step = |ed: &mut Editor, events: Vec<egui::Event>| -> egui::FullOutput {
        observed_levels.borrow_mut().clear();
        observed_cmds.borrow_mut().clear();
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1440., 900.),
                )),
                focused: true,
                events,
                ..Default::default()
            },
            |ctx| {
                ed.sync_closed_detached_panels(ctx);
                ed.menu(ctx);
                ed.dock_grid(ctx);
                ed.stage(ctx);
                ed.draw_dialogs(ctx);
                ed.detached_panels(ctx);
                crate::panels::dialog_helpers::record_end_of_frame_focus(ctx);
            },
        )
    };

    crate::native_windows::set_native_menu_tracking_for_test(false);
    let _ = step(&mut editor, vec![]);
    for (vp, title) in [
        (actions_vp, "Actions — NeoFlash"),
        (props_vp, "Properties — NeoFlash"),
        (library_vp, "Library — NeoFlash"),
        (output_vp, "Output — NeoFlash"),
    ] {
        assert_eq!(
            observed_levels.borrow().get(&vp).copied().flatten(),
            Some(egui::WindowLevel::AlwaysOnTop),
            "{title} must float above the root Stage/Timeline window"
        );
        assert!(crate::native_windows::window_should_float_for_title(Some(
            title
        )));
    }
    assert!(!crate::native_windows::should_order_below_root_for_test());

    // Clicking on the Timeline dock or Stage must NEVER hide or lower floating `Actions`.
    let timeline_rect = editor
        .detached
        .dock_groups
        .iter()
        .find(|g| g.tabs.iter().any(|(id, _)| *id == PanelId::Timeline))
        .map(|g| g.rect)
        .expect("Timeline group rendered in bottom dock");
    let timeline_click = egui::pos2(timeline_rect.left() + 340.0, timeline_rect.center().y);
    let _ = step(
        &mut editor,
        vec![
            egui::Event::PointerMoved(timeline_click),
            crate::tests::primary_button(timeline_click, true),
            crate::tests::primary_button(timeline_click, false),
        ],
    );
    let _ = step(&mut editor, vec![]);
    assert!(editor.workspace.is_panel_visible(PanelId::Actions));
    assert_eq!(
        observed_levels.borrow().get(&actions_vp).copied().flatten(),
        Some(egui::WindowLevel::AlwaysOnTop)
    );
    assert!(crate::native_windows::window_should_float_for_title(Some(
        "Actions — NeoFlash"
    )));

    // 1) Opening Help (`HelpManual` and `Scripting Reference`) drops Actions & all panels to Normal + below root.
    crate::panels::help_manual::open_help_manual(&ctx);
    let _ = step(&mut editor, vec![]);
    assert_eq!(
        observed_levels.borrow().get(&actions_vp).copied().flatten(),
        Some(egui::WindowLevel::Normal)
    );
    assert!(!crate::native_windows::window_should_float_for_title(Some(
        "Actions — NeoFlash"
    )));
    assert!(crate::native_windows::should_order_below_root_for_test());
    crate::panels::help_manual::close_help_manual(&ctx);
    let _ = step(&mut editor, vec![]);
    assert_eq!(
        observed_levels.borrow().get(&actions_vp).copied().flatten(),
        Some(egui::WindowLevel::AlwaysOnTop)
    );
    assert!(!crate::native_windows::should_order_below_root_for_test());

    // 2) Modal dialogs (Preferences, Document Settings), in-window menus, and native NSMenu tracking
    //    drop Actions & all floating panels to Normal and restore cleanly when closed.
    editor.open_preferences(crate::panels::preferences::PAGE_GENERAL);
    let _ = step(&mut editor, vec![]);
    assert_eq!(
        observed_levels.borrow().get(&actions_vp).copied().flatten(),
        Some(egui::WindowLevel::Normal)
    );
    assert!(crate::native_windows::should_order_below_root_for_test());
    editor.preferences = None;
    let _ = step(&mut editor, vec![]);
    assert_eq!(
        observed_levels.borrow().get(&actions_vp).copied().flatten(),
        Some(egui::WindowLevel::AlwaysOnTop)
    );

    crate::native_windows::set_native_menu_tracking_for_test(true);
    let _ = step(&mut editor, vec![]);
    assert_eq!(
        observed_levels.borrow().get(&actions_vp).copied().flatten(),
        Some(egui::WindowLevel::Normal)
    );
    assert!(!crate::native_windows::window_should_float_for_title(Some(
        "Actions — NeoFlash"
    )));
    crate::native_windows::set_native_menu_tracking_for_test(false);
    let _ = step(&mut editor, vec![]);
    assert_eq!(
        observed_levels.borrow().get(&actions_vp).copied().flatten(),
        Some(egui::WindowLevel::AlwaysOnTop)
    );
}

#[test]
fn t374_collapsed_floating_windows_persist_across_restarts_preserve_expanded_height_and_reset_on_template()
 {
    use crate::workspace::{
        PanelId, PanelPlacement, WindowGeometry, WorkspacePrefs, WorkspaceTemplate,
    };
    use std::cell::RefCell;
    use std::collections::{BTreeMap, HashMap};
    use std::rc::Rc;

    #[derive(Default)]
    struct MemStorage {
        map: BTreeMap<String, String>,
        flushes: usize,
    }
    impl eframe::Storage for MemStorage {
        fn get_string(&self, key: &str) -> Option<String> {
            self.map.get(key).cloned()
        }
        fn set_string(&mut self, key: &str, value: String) {
            self.map.insert(key.to_owned(), value);
        }
        fn flush(&mut self) {
            self.flushes += 1;
        }
    }

    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    ctx.set_embed_viewports(false);
    let observed_initial_sizes: Rc<RefCell<HashMap<egui::ViewportId, Option<egui::Vec2>>>> =
        Rc::new(RefCell::new(HashMap::new()));
    let live_sizes: Rc<RefCell<HashMap<egui::ViewportId, egui::Vec2>>> =
        Rc::new(RefCell::new(HashMap::new()));
    let child_outputs: Rc<RefCell<HashMap<egui::ViewportId, egui::FullOutput>>> =
        Rc::new(RefCell::new(HashMap::new()));
    let pending_child_events: Rc<RefCell<HashMap<egui::ViewportId, Vec<egui::Event>>>> =
        Rc::new(RefCell::new(HashMap::new()));
    let sizes_cb = observed_initial_sizes.clone();
    let live_cb = live_sizes.clone();
    let outs_cb = child_outputs.clone();
    let events_cb = pending_child_events.clone();
    egui::Context::set_immediate_viewport_renderer(move |ctx, mut viewport| {
        let id = viewport.ids.this;
        sizes_cb
            .borrow_mut()
            .insert(id, viewport.builder.inner_size);
        if let Some(sz) = viewport.builder.inner_size {
            live_cb.borrow_mut().insert(id, sz);
        }
        let events = events_cb.borrow_mut().remove(&id).unwrap_or_default();
        let inner_sz = live_cb
            .borrow()
            .get(&id)
            .copied()
            .unwrap_or(egui::vec2(760.0, 520.0));
        let mut input = egui::RawInput {
            viewport_id: id,
            focused: !events.is_empty(),
            events,
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, inner_sz)),
            ..Default::default()
        };
        input.viewports.insert(
            id,
            egui::ViewportInfo {
                parent: Some(egui::ViewportId::ROOT),
                outer_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(160.0, 110.0),
                    inner_sz,
                )),
                inner_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(160.0, 110.0),
                    inner_sz,
                )),
                ..Default::default()
            },
        );
        let out = ctx.run(input, |ctx| {
            (viewport.viewport_ui_cb)(ctx);
        });
        for vo in out.viewport_output.values() {
            for cmd in &vo.commands {
                if let egui::ViewportCommand::InnerSize(sz) = cmd {
                    live_cb.borrow_mut().insert(id, *sz);
                }
            }
        }
        outs_cb.borrow_mut().insert(id, out);
    });

    let mut editor = Editor::from_project(&ctx, Project::blank());
    editor.set_panel_visible(PanelId::Actions, true);
    editor.set_panel_placement(&ctx, PanelId::Actions, PanelPlacement::Detached);
    editor.workspace.remember_floating_geometry(
        PanelId::Actions,
        WindowGeometry {
            outer_position: [160.0, 110.0],
            inner_size: [780.0, 540.0],
        },
    );
    editor.persist_workspace_requested = false;

    let actions_vp = egui::ViewportId::from_hash_of(PanelId::Actions.viewport_key().unwrap());
    let apply_root_cmds = |root_out: &egui::FullOutput| {
        for (vp_id, vo) in &root_out.viewport_output {
            for cmd in &vo.commands {
                if let egui::ViewportCommand::InnerSize(sz) = cmd {
                    live_sizes.borrow_mut().insert(*vp_id, *sz);
                }
            }
        }
    };
    let run_detached = |ed: &mut Editor| {
        let root_out = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1440.0, 900.0),
                )),
                ..Default::default()
            },
            |ctx| ed.detached_panels(ctx),
        );
        apply_root_cmds(&root_out);
    };

    run_detached(&mut editor);
    let collapse_center = child_outputs
        .borrow()
        .get(&actions_vp)
        .and_then(|out| {
            out.platform_output
                .accesskit_update
                .as_ref()?
                .nodes
                .iter()
                .find_map(|(_, node)| {
                    (node.label() == Some("Collapse panel"))
                        .then(|| node.bounds())
                        .flatten()
                        .map(|b| {
                            egui::pos2(((b.x0 + b.x1) * 0.5) as f32, ((b.y0 + b.y1) * 0.5) as f32)
                        })
                })
        })
        .expect("Actions floating header exposes Collapse panel button");

    pending_child_events.borrow_mut().insert(
        actions_vp,
        vec![
            egui::Event::PointerMoved(collapse_center),
            crate::tests::primary_button(collapse_center, true),
        ],
    );
    run_detached(&mut editor);
    pending_child_events.borrow_mut().insert(
        actions_vp,
        vec![crate::tests::primary_button(collapse_center, false)],
    );
    run_detached(&mut editor);

    assert!(
        editor
            .workspace
            .is_floating_window_collapsed(PanelId::Actions)
    );
    assert!(
        editor.persist_workspace_requested,
        "collapsing a floating window must request workspace persistence immediately"
    );
    assert_eq!(
        editor
            .workspace
            .detached_geometry(PanelId::Actions)
            .map(|g| g.inner_size[1]),
        Some(540.0),
        "collapsing must preserve uncollapsed expanded height in WorkspacePrefs"
    );

    let mut storage = MemStorage::default();
    editor.flush_workspace_to_storage(&mut storage);
    assert_eq!(storage.flushes, 1);

    // Simulate a fresh process launch with fresh egui Context (zero temp memory):
    let restored_prefs = WorkspacePrefs::load_for_viewport(Some(&storage), [800.0, 600.0]);
    assert!(
        restored_prefs.is_floating_window_collapsed(PanelId::Actions),
        "collapsed floating state must survive restart and load_for_viewport"
    );
    let ctx2 = egui::Context::default();
    ctx2.enable_accesskit();
    ctx2.set_embed_viewports(false);
    let mut restored_editor = Editor::from_project(&ctx2, Project::blank());
    restored_editor.workspace = restored_prefs;
    observed_initial_sizes.borrow_mut().clear();
    let _ = ctx2.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1440.0, 900.0),
            )),
            ..Default::default()
        },
        |ctx| restored_editor.detached_panels(ctx),
    );
    assert_eq!(
        observed_initial_sizes
            .borrow()
            .get(&actions_vp)
            .copied()
            .flatten(),
        Some(egui::vec2(780.0, neoflash_ui::metrics::panel_tab())),
        "restarted collapsed floating window must open directly at header height while preserving saved expanded height"
    );
    assert_eq!(
        restored_editor
            .workspace
            .detached_geometry(PanelId::Actions)
            .map(|g| g.inner_size[1]),
        Some(540.0)
    );

    // Expanding the restored collapsed window restores its saved 540 pt expanded height.
    let expand_center = child_outputs
        .borrow()
        .get(&actions_vp)
        .and_then(|out| {
            out.platform_output
                .accesskit_update
                .as_ref()?
                .nodes
                .iter()
                .find_map(|(_, node)| {
                    (node.label() == Some("Expand panel"))
                        .then(|| node.bounds())
                        .flatten()
                        .map(|b| {
                            egui::pos2(((b.x0 + b.x1) * 0.5) as f32, ((b.y0 + b.y1) * 0.5) as f32)
                        })
                })
        })
        .expect("collapsed floating header exposes Expand panel button");
    pending_child_events.borrow_mut().insert(
        actions_vp,
        vec![
            egui::Event::PointerMoved(expand_center),
            crate::tests::primary_button(expand_center, true),
        ],
    );
    let root_out = ctx2.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1440.0, 900.0),
            )),
            ..Default::default()
        },
        |ctx| restored_editor.detached_panels(ctx),
    );
    apply_root_cmds(&root_out);
    pending_child_events.borrow_mut().insert(
        actions_vp,
        vec![crate::tests::primary_button(expand_center, false)],
    );
    let root_out = ctx2.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1440.0, 900.0),
            )),
            ..Default::default()
        },
        |ctx| restored_editor.detached_panels(ctx),
    );
    apply_root_cmds(&root_out);
    assert!(
        !restored_editor
            .workspace
            .is_floating_window_collapsed(PanelId::Actions)
    );
    assert_eq!(
        live_sizes.borrow().get(&actions_vp).copied(),
        Some(egui::vec2(780.0, 540.0)),
        "expanding after restart must restore the saved 540 pt height"
    );

    // Re-collapse and verify switching workspace templates clears collapsed_floating cleanly.
    assert!(
        restored_editor
            .workspace
            .set_floating_window_collapsed(PanelId::Actions, true)
    );
    restored_editor.apply_workspace_template(&ctx2, WorkspaceTemplate::Developer);
    assert!(restored_editor.workspace.collapsed_floating.is_empty());
}

#[test]
fn t384_universal_floating_snap_covers_all_draggable_panels_edges_stage_and_monitors() {
    use crate::detached::{
        FloatingSnapTargets, PANEL_SNAP_DISTANCE, TOOLS_ANCHOR_DISTANCE, TOOLS_ANCHOR_MARGIN,
        snap_floating_window_geometry, snap_tools_stage_anchor,
    };
    use crate::workspace::ScreenRect;

    assert_eq!(PANEL_SNAP_DISTANCE, 8.0);
    assert_eq!(TOOLS_ANCHOR_DISTANCE, 28.0);

    // 1) Pure math across UI zoom factors: flush abutment, co-aligned palettes, Stage inset,
    //    root_inner content edges, and single/dual monitor work-areas.
    for zoom in [1.0_f32, 1.25, 1.5, 2.0] {
        let props_rect =
            egui::Rect::from_min_size(egui::pos2(400., 200.) * zoom, egui::vec2(320., 420.) * zoom);
        let stage_rect =
            egui::Rect::from_min_size(egui::pos2(60., 90.) * zoom, egui::vec2(1400., 760.) * zoom);
        let root_content =
            egui::Rect::from_min_size(egui::pos2(0., 40.) * zoom, egui::vec2(1920., 1016.) * zoom);
        let root_inner =
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1920., 1080.) * zoom);
        let monitors = [
            ScreenRect {
                origin: [0.0, 24.0 * zoom],
                size: [1920.0 * zoom, 1056.0 * zoom],
            },
            ScreenRect {
                origin: [1920.0 * zoom, 0.0],
                size: [1920.0 * zoom, 1080.0 * zoom],
            },
        ];
        let targets = FloatingSnapTargets {
            other_windows: &[props_rect],
            stage_rect: Some(stage_rect),
            root_content: Some(root_content),
            root_inner: Some(root_inner),
            monitors: &monitors,
        };

        // Flush right abutment (`my.left == other.right`) + co-aligned top (`my.top == other.top`)
        let near_side_by_side = WindowGeometry {
            outer_position: [725.0 * zoom, 206.0 * zoom],
            inner_size: [260.0 * zoom, 240.0 * zoom],
        };
        let snapped = snap_floating_window_geometry(near_side_by_side, zoom, false, &targets);
        assert!(
            (snapped.outer_position[0] - 720.0 * zoom).abs() < 0.01
                && (snapped.outer_position[1] - 200.0 * zoom).abs() < 0.01,
            "zoom={zoom}: expected side-by-side flush+top snap [720, 200]*zoom, got {:?}",
            snapped.outer_position
        );

        // Outside 8 pt snap threshold (`9.5 * zoom` away): must NOT snap prematurely!
        let outside_threshold = WindowGeometry {
            outer_position: [729.5 * zoom, 209.5 * zoom],
            inner_size: [260.0 * zoom, 240.0 * zoom],
        };
        assert_eq!(
            snap_floating_window_geometry(outside_threshold, zoom, false, &targets),
            outside_threshold,
            "zoom={zoom}: 9.5 pt must stay completely free of 8 pt snap zone"
        );

        // Suppress flag (`⌘` / `Alt` held) bypasses snap even within 2 pt.
        let suppressed = snap_floating_window_geometry(near_side_by_side, zoom, true, &targets);
        assert_eq!(suppressed, near_side_by_side);

        // Stacked below (`my.top == other.bottom`) + co-aligned right (`my.right == other.right`),
        // plus co-aligned left with a 200 pt vertical gap (<= 360 * zoom).
        let near_below = WindowGeometry {
            outer_position: [455.0 * zoom, 626.0 * zoom],
            inner_size: [260.0 * zoom, 200.0 * zoom],
        };
        let snapped_below = snap_floating_window_geometry(near_below, zoom, false, &targets);
        assert!(
            (snapped_below.outer_position[0] - 460.0 * zoom).abs() < 0.01
                && (snapped_below.outer_position[1] - 620.0 * zoom).abs() < 0.01,
            "zoom={zoom}: stacked right-coaligned + top-flush snap failed: {:?}",
            snapped_below.outer_position
        );

        let distant_coaligned = WindowGeometry {
            outer_position: [404.0 * zoom, 820.0 * zoom], // 200*zoom vertical gap <= 360*zoom
            inner_size: [260.0 * zoom, 180.0 * zoom],
        };
        let snapped_coaligned =
            snap_floating_window_geometry(distant_coaligned, zoom, false, &targets);
        assert!((snapped_coaligned.outer_position[0] - 400.0 * zoom).abs() < 0.01);

        let too_far_perpendicular = WindowGeometry {
            outer_position: [404.0 * zoom, 1020.0 * zoom], // 400*zoom vertical gap > 360*zoom
            inner_size: [260.0 * zoom, 180.0 * zoom],
        };
        assert!(
            (snap_floating_window_geometry(too_far_perpendicular, zoom, false, &targets)
                .outer_position[0]
                - 404.0 * zoom)
                .abs()
                < 0.01,
            "zoom={zoom}: perpendicular gap > 360*zoom must not snag co-alignment"
        );

        // Stage inset (`stage_rect` shrunk by `TOOLS_ANCHOR_MARGIN * zoom` = 16 * zoom -> [76, 106])
        let near_stage_inset = WindowGeometry {
            outer_position: [81.0 * zoom, 101.0 * zoom],
            inner_size: [300.0 * zoom, 220.0 * zoom],
        };
        let snapped_stage = snap_floating_window_geometry(near_stage_inset, zoom, false, &targets);
        assert!(
            (snapped_stage.outer_position[0] - (60.0 + TOOLS_ANCHOR_MARGIN) * zoom).abs() < 0.01
                && (snapped_stage.outer_position[1] - (90.0 + TOOLS_ANCHOR_MARGIN) * zoom).abs()
                    < 0.01
        );

        // Second monitor far right edge (`3840 * zoom`) and top edge (`0.0`)
        let near_monitor2_edge = WindowGeometry {
            outer_position: [(3840.0 - 320.0 - 6.0) * zoom, 5.0 * zoom],
            inner_size: [320.0 * zoom, 440.0 * zoom],
        };
        let snapped_mon2 = snap_floating_window_geometry(near_monitor2_edge, zoom, false, &targets);
        assert!(
            (snapped_mon2.outer_position[0] - (3840.0 - 320.0) * zoom).abs() < 0.01
                && snapped_mon2.outer_position[1].abs() < 0.01
        );

        // Tools Stage anchor tightened to 28 pt: snaps at 20 pt, stays free at 40 pt!
        let tools_w = crate::detached::floating_tools_width() * zoom;
        let middle_left_y = stage_rect.center().y - 120.0 * zoom;
        let tools_at_20 = WindowGeometry {
            outer_position: [
                stage_rect.left() + (TOOLS_ANCHOR_MARGIN + 20.0) * zoom,
                middle_left_y,
            ],
            inner_size: [tools_w, 240.0 * zoom],
        };
        assert!(snap_tools_stage_anchor(stage_rect, tools_at_20, zoom).is_some());
        let tools_at_40 = WindowGeometry {
            outer_position: [
                stage_rect.left() + (TOOLS_ANCHOR_MARGIN + 40.0) * zoom,
                middle_left_y,
            ],
            inner_size: [tools_w, 240.0 * zoom],
        };
        assert!(
            snap_tools_stage_anchor(stage_rect, tools_at_40, zoom).is_none(),
            "tightened 28 pt Tools Stage anchor must not grab Tools from 40 pt away"
        );
    }

    // 2) Verify Editor::snap_floating_geometry_for_window across ALL 19+ user-named panels,
    //    plus hidden-window exclusion and collapsed-window header height preservation.
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    editor.set_panel_visible(PanelId::Properties, true);
    editor.set_panel_placement(&ctx, PanelId::Properties, PanelPlacement::Detached);
    editor.workspace.remember_floating_geometry(
        PanelId::Properties,
        WindowGeometry {
            outer_position: [500.0, 180.0],
            inner_size: [320.0, 440.0],
        },
    );
    // Hidden detached Library must NOT act as a ghost snap target.
    editor.set_panel_placement(&ctx, PanelId::Library, PanelPlacement::Detached);
    editor.workspace.remember_floating_geometry(
        PanelId::Library,
        WindowGeometry {
            outer_position: [150.0, 180.0],
            inner_size: [300.0, 500.0],
        },
    );
    editor.set_panel_visible(PanelId::Library, false);

    let all_draggable = [
        PanelId::Actions,
        PanelId::Library,
        PanelId::Tools,
        PanelId::Color,
        PanelId::Swatches,
        PanelId::Align,
        PanelId::Transform,
        PanelId::Info,
        PanelId::Brushes,
        PanelId::Output,
        PanelId::CompilerErrors,
        PanelId::Debugger,
        PanelId::Timeline,
        PanelId::Scene,
        PanelId::Ease,
        PanelId::MotionEditor,
        PanelId::History,
        PanelId::MovieExplorer,
    ];
    for panel in all_draggable {
        editor.set_panel_visible(panel, panel != PanelId::Library);
        editor.set_panel_placement(&ctx, panel, PanelPlacement::Detached);
        let candidate = WindowGeometry {
            outer_position: [826.0, 184.0], // 6 pt from Properties.right (820), 4 pt from top (180)
            inner_size: [280.0, 260.0],
        };
        let snapped =
            editor.snap_floating_geometry_for_window(Some(panel), false, candidate, 1.0, false);
        assert_eq!(
            snapped.outer_position,
            [820.0, 180.0],
            "{panel:?} must snap flush to Properties.right and co-align with Properties.top"
        );
        // Near hidden Library's right edge (450.0) must NOT snap while Library is hidden.
        let near_hidden = WindowGeometry {
            outer_position: [454.0, 184.0],
            inner_size: [280.0, 260.0],
        };
        let unsnapped_x =
            editor.snap_floating_geometry_for_window(Some(panel), false, near_hidden, 1.0, false);
        assert_eq!(
            unsnapped_x.outer_position[0], 454.0,
            "{panel:?} must ignore hidden floating Library"
        );
        if panel != PanelId::Properties {
            editor.set_panel_visible(panel, false);
        }
    }

    // Collapsed Properties (`inner_size[1] = 440`, visible header = `panel_tab() = 36`):
    // Snapping Swatches below collapsed Properties snaps to `180 + panel_tab()`, NOT `180 + 440`!
    editor
        .workspace
        .set_floating_window_collapsed(PanelId::Properties, true);
    editor.set_panel_visible(PanelId::Swatches, true);
    editor.set_panel_placement(&ctx, PanelId::Swatches, PanelPlacement::Detached);
    let tab_h = neoflash_ui::metrics::panel_tab();
    let below_collapsed = WindowGeometry {
        outer_position: [504.0, 180.0 + tab_h + 5.0],
        inner_size: [300.0, 360.0],
    };
    let snapped_below_collapsed = editor.snap_floating_geometry_for_window(
        Some(PanelId::Swatches),
        false,
        below_collapsed,
        1.0,
        false,
    );
    assert_eq!(
        snapped_below_collapsed.outer_position,
        [500.0, 180.0 + tab_h]
    );
}

#[test]
fn t384_live_drag_and_drop_parity_and_modifier_suppression_across_floating_and_torn_off_panels() {
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(false);
    let mut editor = Editor::from_project(&ctx, Project::blank());

    // Set up anchored stationary floating Properties at [400, 200] size [320, 420] (right = 720).
    editor.set_panel_visible(PanelId::Properties, true);
    editor.set_panel_placement(&ctx, PanelId::Properties, PanelPlacement::Detached);
    editor.workspace.remember_floating_geometry(
        PanelId::Properties,
        WindowGeometry {
            outer_position: [400.0, 200.0],
            inner_size: [320.0, 420.0],
        },
    );

    // Test live drag + release parity and ⌘/Alt bypass for Actions, Swatches, Align, and Tools.
    for (panel, size) in [
        (PanelId::Actions, egui::vec2(680.0, 440.0)),
        (PanelId::Swatches, egui::vec2(324.0, 490.0)),
        (PanelId::Align, egui::vec2(320.0, 440.0)),
        (PanelId::Tools, egui::vec2(88.0, 360.0)),
    ] {
        editor.set_panel_visible(panel, true);
        editor.set_panel_placement(&ctx, panel, PanelPlacement::Detached);
        let start_origin = egui::pos2(900.0, 240.0);
        editor.workspace.remember_floating_geometry(
            panel,
            WindowGeometry {
                outer_position: [start_origin.x, start_origin.y],
                inner_size: [size.x, size.y],
            },
        );
        let vp_id = egui::ViewportId::from_hash_of(panel.viewport_key().unwrap());

        let run_panel_frame = |ed: &mut Editor,
                               origin: egui::Pos2,
                               modifiers: egui::Modifiers,
                               events: Vec<egui::Event>|
         -> Vec<egui::ViewportCommand> {
            let mut input = egui::RawInput {
                viewport_id: vp_id,
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                modifiers,
                events,
                ..Default::default()
            };
            let info = egui::ViewportInfo {
                parent: Some(egui::ViewportId::ROOT),
                outer_rect: Some(egui::Rect::from_min_size(origin, size)),
                inner_rect: Some(egui::Rect::from_min_size(origin, size)),
                ..Default::default()
            };
            input.viewports.insert(vp_id, info);
            let out = ctx.run(input, |ctx| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::central_panel(&ctx.style()).inner_margin(0.))
                    .show(ctx, |ui| {
                        let _ = ed.native_floating_panel(ui, panel, &[panel], [40.0, 120.0], 1.0);
                    });
                ed.finalize_panel_drag(ctx);
            });
            out.viewport_output
                .get(&vp_id)
                .map(|vo| vo.commands.clone())
                .unwrap_or_default()
        };

        // Initial layout frame
        let _ = run_panel_frame(&mut editor, start_origin, egui::Modifiers::NONE, vec![]);
        let grab_local = egui::pos2(10.0, 14.0);
        // Frame 1: Press header grip at local (10, 14) -> global (910, 254).
        let _ = run_panel_frame(
            &mut editor,
            start_origin,
            egui::Modifiers::NONE,
            vec![
                egui::Event::PointerMoved(grab_local),
                crate::tests::primary_button(grab_local, true),
            ],
        );

        // Frame 2: Live drag (still held!) so raw outer_position is [725, 204]
        // (`5 pt` from Properties.right=720, `4 pt` from Properties.top=200).
        // Local pos relative to start_origin (900, 240) is (725 + 10 - 900, 204 + 14 - 240) = (-165, -22).
        let live_local = egui::pos2(-165.0, -22.0);
        let cmds_snapped = run_panel_frame(
            &mut editor,
            start_origin,
            egui::Modifiers::NONE,
            vec![egui::Event::PointerMoved(live_local)],
        );
        assert!(
            cmds_snapped.contains(&egui::ViewportCommand::OuterPosition(egui::pos2(
                720.0, 200.0
            ))),
            "{panel:?}: live drag must emit OuterPosition(720, 200), got {cmds_snapped:?}"
        );
        let live_rect = editor
            .live_dragged_floating_window_rect(panel, 1.0)
            .expect("live dragged rect");
        assert_eq!(live_rect.min, egui::pos2(720.0, 200.0));

        // Frame 3: Hold `Alt` while still at the same pointer — live drag must unsnap to [725, 204]!
        let alt_mods = egui::Modifiers {
            alt: true,
            ..Default::default()
        };
        let cmds_alt = run_panel_frame(
            &mut editor,
            start_origin,
            alt_mods,
            vec![egui::Event::PointerMoved(live_local)],
        );
        assert!(
            cmds_alt.contains(&egui::ViewportCommand::OuterPosition(egui::pos2(
                725.0, 204.0
            ))),
            "{panel:?}: holding Alt during live drag must suspend snapping to (725, 204)"
        );

        // Frame 4: Release `Alt` and release mouse button at `live_local` — drop must match live snap [720, 200]!
        let _ = run_panel_frame(
            &mut editor,
            start_origin,
            egui::Modifiers::NONE,
            vec![crate::tests::primary_button(live_local, false)],
        );
        let dropped = editor
            .workspace
            .detached_geometry(panel)
            .expect("dropped geometry");
        assert_eq!(
            dropped.outer_position,
            [720.0, 200.0],
            "{panel:?}: drop position must match live snapped position [720, 200] 1:1"
        );

        editor.set_panel_visible(panel, false);
    }

    // Verify `⌘` (`command: true`) suppresses BOTH docking and snapping even directly over a Main window dock edge!
    editor.set_panel_visible(PanelId::Align, true);
    editor.set_panel_placement(&ctx, PanelId::Align, PanelPlacement::Detached);
    editor.workspace.remember_floating_geometry(
        PanelId::Align,
        WindowGeometry {
            outer_position: [500.0, 300.0],
            inner_size: [260.0, 240.0],
        },
    );
    let align_vp = egui::ViewportId::from_hash_of(PanelId::Align.viewport_key().unwrap());
    let root_size = egui::vec2(1440.0, 900.0);
    // Register Main dock viewport first
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, root_size)),
            ..Default::default()
        },
        |ctx| {
            editor.dock_grid(ctx);
            editor.stage(ctx);
        },
    );
    assert!(
        editor.dock_hit(egui::pos2(24.0, 400.0), 1.0).is_some(),
        "without ⌘, x=24 is inside Left dock acceptance zone"
    );

    // Start dragging floating Align from its grip (10, 14)
    let mut child_in = egui::RawInput {
        viewport_id: align_vp,
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(260.0, 240.0),
        )),
        events: vec![
            egui::Event::PointerMoved(egui::pos2(10.0, 14.0)),
            crate::tests::primary_button(egui::pos2(10.0, 14.0), true),
            egui::Event::PointerGone,
        ],
        ..Default::default()
    };
    child_in.viewports.insert(
        align_vp,
        egui::ViewportInfo {
            parent: Some(egui::ViewportId::ROOT),
            outer_rect: Some(egui::Rect::from_min_size(
                egui::pos2(500.0, 300.0),
                egui::vec2(260.0, 240.0),
            )),
            inner_rect: Some(egui::Rect::from_min_size(
                egui::pos2(500.0, 300.0),
                egui::vec2(260.0, 240.0),
            )),
            ..Default::default()
        },
    );
    let _ = ctx.run(child_in, |ctx| {
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(&ctx.style()).inner_margin(0.))
            .show(ctx, |ui| {
                let _ = editor.native_floating_panel(
                    ui,
                    PanelId::Align,
                    &[PanelId::Align],
                    [248.0, 190.0],
                    1.0,
                );
            });
    });

    // Drop at x=24, y=400 right on the Left dock edge WHILE holding `⌘` (`command: true`):
    // Must NOT dock into Left rail AND must NOT snap to x=0 (`24 - 10 = 14`, or even at `x=15` -> `outer_x=5` within 8 pt of 0)!
    let cmd_mods = egui::Modifiers {
        command: true,
        mac_cmd: true,
        ..Default::default()
    };
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, root_size)),
            modifiers: cmd_mods,
            events: vec![
                egui::Event::PointerMoved(egui::pos2(15.0, 400.0)),
                crate::tests::primary_button(egui::pos2(15.0, 400.0), false),
            ],
            ..Default::default()
        },
        |ctx| {
            editor.dock_grid(ctx);
            assert!(
                editor.dock_hit(egui::pos2(15.0, 400.0), 1.0).is_none(),
                "holding ⌘ must suppress dock_hit even right on a dock header/edge"
            );
            editor.finalize_panel_drag(ctx);
        },
    );
    assert!(
        !editor.workspace.is_panel_docked(PanelId::Align),
        "holding ⌘ on release must keep Align floating instead of docking"
    );
    assert_eq!(
        editor
            .workspace
            .detached_geometry(PanelId::Align)
            .unwrap()
            .outer_position,
        [5.0, 386.0],
        "holding ⌘ must also bypass the 8 pt snap to x=0"
    );

    // Also verify `⌘` suppresses `dock_hit` right over a docked panel's tab header (`Library`)
    // AND verify multi-tab floating dock (`[Color, Swatches, Info]`) snaps cleanly on live drag + drop!
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, root_size)),
            modifiers: egui::Modifiers::NONE,
            ..Default::default()
        },
        |ctx| {
            editor.menu(ctx);
            editor.dock_grid(ctx);
        },
    );
    let library_header_center = editor
        .detached
        .dock_groups
        .iter()
        .find(|g| g.tabs.iter().any(|(p, _)| *p == PanelId::Library))
        .expect("docked Library group")
        .header
        .center();
    assert!(
        editor.dock_hit(library_header_center, 1.0).is_some(),
        "without ⌘, hovering docked Library header hits 'Place tab'"
    );
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, root_size)),
            modifiers: cmd_mods,
            ..Default::default()
        },
        |ctx| {
            editor.menu(ctx);
            editor.dock_grid(ctx);
        },
    );
    assert!(
        editor.dock_hit(library_header_center, 1.0).is_none(),
        "holding ⌘ over docked Library header must suppress docking"
    );
    // Reset command modifier for subsequent frames
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, root_size)),
            modifiers: egui::Modifiers::NONE,
            ..Default::default()
        },
        |ctx| {
            editor.menu(ctx);
            editor.dock_grid(ctx);
        },
    );

    editor.set_panel_visible(PanelId::Align, false);
    let group_panels = [PanelId::Color, PanelId::Swatches, PanelId::Info];
    for p in group_panels {
        editor.show_panel(p);
    }
    let group_win = editor.workspace.float_group(
        &group_panels,
        PanelId::Color,
        WindowGeometry {
            outer_position: [900.0, 240.0],
            inner_size: [340.0, 380.0],
        },
    );
    let group_vp = egui::ViewportId::from_hash_of(group_win.viewport_key().unwrap());
    let run_group_frame = |ed: &mut Editor,
                           events: Vec<egui::Event>|
     -> Vec<egui::ViewportCommand> {
        let size = egui::vec2(340.0, 380.0);
        let origin = egui::pos2(900.0, 240.0);
        let mut input = egui::RawInput {
            viewport_id: group_vp,
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        };
        input.viewports.insert(
            group_vp,
            egui::ViewportInfo {
                parent: Some(egui::ViewportId::ROOT),
                outer_rect: Some(egui::Rect::from_min_size(origin, size)),
                inner_rect: Some(egui::Rect::from_min_size(origin, size)),
                ..Default::default()
            },
        );
        let out = ctx.run(input, |ctx| {
            egui::CentralPanel::default()
                .frame(egui::Frame::central_panel(&ctx.style()).inner_margin(0.))
                .show(ctx, |ui| {
                    let _ =
                        ed.native_floating_panel(ui, group_win, &group_panels, [248.0, 190.0], 1.0);
                });
            ed.finalize_panel_drag(ctx);
        });
        out.viewport_output
            .get(&group_vp)
            .map(|vo| vo.commands.clone())
            .unwrap_or_default()
    };
    let _ = run_group_frame(&mut editor, vec![]);
    let _ = run_group_frame(
        &mut editor,
        vec![
            egui::Event::PointerMoved(egui::pos2(10.0, 14.0)),
            crate::tests::primary_button(egui::pos2(10.0, 14.0), true),
        ],
    );
    // Raw outer_position [726, 205] -> snaps flush to Properties.right (720) & top (200)
    let group_live_local = egui::pos2(-164.0, -21.0);
    let group_cmds = run_group_frame(
        &mut editor,
        vec![egui::Event::PointerMoved(group_live_local)],
    );
    assert!(
        group_cmds.contains(&egui::ViewportCommand::OuterPosition(egui::pos2(
            720.0, 200.0
        )))
    );
    let _ = run_group_frame(
        &mut editor,
        vec![crate::tests::primary_button(group_live_local, false)],
    );
    assert_eq!(
        editor
            .workspace
            .detached_geometry(group_win)
            .unwrap()
            .outer_position,
        [720.0, 200.0]
    );
}

#[test]
fn t384_tightened_dock_zones_and_clean_stage_overlay_when_moving_floating_windows() {
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(false);
    let mut editor = Editor::from_project(&ctx, Project::blank());

    // 1) Verify edge_targets & edge_acceptance_targets stay strictly in 28..=44 * zoom across window sizes.
    for zoom in [1.0_f32, 1.5, 2.0] {
        let win = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1920.0, 1080.0) * zoom);
        for (edge, rect) in crate::dock_grid::edge_targets(win, zoom) {
            let band = if edge.vertical() {
                rect.width()
            } else {
                rect.height()
            };
            assert!(
                (28.0 * zoom - 0.1..=44.0 * zoom + 0.1).contains(&band),
                "visible {edge:?} band {band} must be within 28..44 * {zoom}"
            );
        }
        for (edge, rect) in crate::dock_grid::edge_acceptance_targets(win, zoom) {
            let band = if edge.vertical() {
                rect.width()
            } else {
                rect.height()
            };
            assert!(
                (28.0 * zoom - 0.1..=44.0 * zoom + 0.1).contains(&band),
                "acceptance {edge:?} band {band} must be within 28..44 * {zoom}"
            );
        }
    }

    // 2) Screenshot 4 regression: dragging floating `Properties` over the middle of the Stage
    //    must paint ZERO unhovered "Top"/"Bottom"/"Left"/"Right" edge overlays!
    editor.set_panel_visible(PanelId::Properties, true);
    editor.set_panel_placement(&ctx, PanelId::Properties, PanelPlacement::Detached);
    editor.workspace.remember_floating_geometry(
        PanelId::Properties,
        WindowGeometry {
            outer_position: [760.0, 340.0],
            inner_size: [320.0, 320.0],
        },
    );
    let props_vp = egui::ViewportId::from_hash_of(PanelId::Properties.viewport_key().unwrap());
    let pending_props_events: Rc<RefCell<Vec<egui::Event>>> = Rc::new(RefCell::new(Vec::new()));
    let events_cb = pending_props_events.clone();
    egui::Context::set_immediate_viewport_renderer(move |ctx, mut viewport| {
        let id = viewport.ids.this;
        let events = if id == props_vp {
            std::mem::take(&mut *events_cb.borrow_mut())
        } else {
            Vec::new()
        };
        let mut input = egui::RawInput {
            viewport_id: id,
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(320.0, 320.0),
            )),
            events,
            ..Default::default()
        };
        input.viewports.insert(
            id,
            egui::ViewportInfo {
                parent: Some(egui::ViewportId::ROOT),
                outer_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(760.0, 340.0),
                    egui::vec2(320.0, 320.0),
                )),
                inner_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(760.0, 340.0),
                    egui::vec2(320.0, 320.0),
                )),
                ..Default::default()
            },
        );
        let _ = ctx.run(input, |ctx| (viewport.viewport_ui_cb)(ctx));
    });

    let render_root = |ed: &mut Editor, root_events: Vec<egui::Event>| -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1920.0, 1080.0),
                )),
                events: root_events,
                ..Default::default()
            },
            |ctx| {
                ed.dock_grid(ctx);
                ed.stage(ctx);
                ed.detached_panels(ctx);
            },
        )
    };
    let has_text = |out: &egui::FullOutput, needle: &str| -> bool {
        out.shapes.iter().any(|s| match &s.shape {
            egui::Shape::Text(t) => t.galley.job.text == needle,
            _ => false,
        })
    };

    let _ = render_root(&mut editor, vec![]);
    // Press floating Properties grip at local (10, 14) -> global (770, 354), move to (820, 380) in middle of Stage!
    *pending_props_events.borrow_mut() = vec![
        egui::Event::PointerMoved(egui::pos2(10.0, 14.0)),
        crate::tests::primary_button(egui::pos2(10.0, 14.0), true),
        egui::Event::PointerMoved(egui::pos2(60.0, 40.0)),
    ];
    let stage_out = render_root(&mut editor, vec![]);
    assert!(editor.panel_drag_moved());
    assert_eq!(
        editor.dragged_whole_floating_window(),
        Some(PanelId::Properties)
    );
    for premature in ["Top", "Bottom", "Left", "Right"] {
        assert!(
            !has_text(&stage_out, premature),
            "dragging floating Properties over the Stage must NOT light up '{premature}' dock zone"
        );
    }

    // Move within 60 pt of Top edge (`y = 95`, `content.top() = 40` -> distance = `55 <= 72`, `> 42` acceptance):
    // ONLY `"Top"` hint appears; `"Bottom"`, `"Left"`, `"Right"` stay hidden and `dock_hit` is still None!
    let near_top_out = render_root(
        &mut editor,
        vec![egui::Event::PointerMoved(egui::pos2(960.0, 95.0))],
    );
    assert!(editor.dock_hit(egui::pos2(960.0, 95.0), 1.0).is_none());
    assert!(
        has_text(&near_top_out, "Top"),
        "moving within 72 pt of Top edge shows subtle Top edge hint"
    );
    for other in ["Bottom", "Left", "Right"] {
        assert!(
            !has_text(&near_top_out, other),
            "distant '{other}' edge hint must remain hidden"
        );
    }

    // Move into Top acceptance band (`y = 55`, distance `15 <= 42`): `"Dock Top"` active target lights up!
    let hit_top_out = render_root(
        &mut editor,
        vec![egui::Event::PointerMoved(egui::pos2(960.0, 55.0))],
    );
    assert!(has_text(&hit_top_out, "Dock Top"));
}
