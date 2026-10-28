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
fn immediate_panels_share_state_close_reopen_and_dock_without_duplicate_contents() {
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(false);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let close = Rc::new(RefCell::new(None));
    let outputs = Rc::new(RefCell::new(Vec::new()));
    let (seen_child, close_child, outputs_child) = (seen.clone(), close.clone(), outputs.clone());
    egui::Context::set_immediate_viewport_renderer(move |ctx, mut viewport| {
        let id = viewport.ids.this;
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
    editor.restore_workspace(&ctx);
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
