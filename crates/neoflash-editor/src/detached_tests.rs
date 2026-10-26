//! Exercise real egui immediate-viewport callbacks with simulated window input.
//! OS move/resize and native clipboard acceptance remain live native checks.
use crate::{
    app::{BottomTab, Editor},
    workspace::{PanelId, PanelPlacement},
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
                editor.timeline(ctx);
                editor.tools(ctx);
                editor.prepare_docked_panel_focus(ctx);
                editor.bottom_panel(ctx);
                editor.stage(ctx);
                editor.detached_panels(ctx);
            },
        );
    };
    render(&mut editor, vec![]);
    assert_eq!(seen.borrow().len(), 3);
    assert_eq!(
        seen.borrow()
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        3
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
    assert_eq!(seen.borrow().len(), 2);
    assert_eq!(editor.bottom, BottomTab::Actions);
    editor.restore_workspace(&ctx);
    seen.borrow_mut().clear();
    render(&mut editor, vec![]);
    assert!(seen.borrow().is_empty());
    assert_eq!(editor.project, original);
    assert_eq!(editor.selection, selection);
    assert_eq!(editor.frame, 7);
    assert!(!editor.dirty());
    assert!(!editor.history.can_undo());
}
