//! Rulers, guides, grid and snapping through the real stage event loop, menus and shortcuts
//! (docs/NF2-RULERS-GUIDES-GRID.md "Tests").
use crate::{
    app::{Editor, Tool},
    stage_view::{SnapAccuracy, ViewPrefs},
    tests::{primary_button, rectangle_stage, stage_input_frame},
};
use eframe::egui::{self, Event, Pos2, Rect, Vec2};
use neoflash_core::*;

/// Press at `from`, drag through `to`, release there — four frames so egui sees a drag.
fn drag(editor: &mut Editor, ctx: &egui::Context, t: f64, from: Pos2, to: Pos2) {
    let mid = from.lerp(to, 0.5);
    stage_input_frame(
        editor,
        ctx,
        t,
        vec![Event::PointerMoved(from), primary_button(from, true)],
    );
    stage_input_frame(editor, ctx, t + 0.02, vec![Event::PointerMoved(mid)]);
    stage_input_frame(editor, ctx, t + 0.04, vec![Event::PointerMoved(to)]);
    stage_input_frame(editor, ctx, t + 0.06, vec![primary_button(to, false)]);
    stage_input_frame(editor, ctx, t + 0.08, vec![]);
}

fn select_stage() -> (Editor, egui::Context, Rect) {
    let (mut editor, ctx, page) = rectangle_stage();
    editor.tool = Tool::Select;
    stage_input_frame(&mut editor, &ctx, 0.01, vec![]);
    (editor, ctx, page)
}

#[test]
fn ruler_drag_out_move_and_drop_back_are_single_undo_steps() {
    let (mut editor, ctx, page) = select_stage();
    assert!(editor.stage_view.prefs.rulers, "NF2 default shows rulers");
    let canvas = editor.stage_view.canvas;
    // Out of the top ruler: a horizontal guide 100 px below the stage top.
    let ruler = Pos2::new(page.center().x, canvas.top() + 10.);
    drag(
        &mut editor,
        &ctx,
        0.1,
        ruler,
        Pos2::new(page.center().x, page.top() + 100.),
    );
    assert_eq!(
        editor.guides(),
        [Guide {
            axis: GuideAxis::Horizontal,
            position: 100.
        }]
    );
    assert_eq!(editor.project.format_version, Guide::FORMAT_VERSION);
    editor.project.validate().unwrap();
    let added = editor.project.clone();

    // Out of the left ruler: a vertical guide.
    let left = Pos2::new(canvas.left() + 10., page.center().y);
    drag(
        &mut editor,
        &ctx,
        0.3,
        left,
        Pos2::new(page.left() + 250.4, page.center().y),
    );
    assert_eq!(editor.guides()[1].axis, GuideAxis::Vertical);
    assert_eq!(editor.guides()[1].position, 250., "whole pixels");
    editor.undo();
    assert_eq!(editor.project, added);

    // Move: drag the guide line itself; one Undo restores it.
    let on_guide = Pos2::new(page.center().x + 37., page.top() + 100.);
    drag(
        &mut editor,
        &ctx,
        0.5,
        on_guide,
        on_guide + Vec2::new(0., 50.),
    );
    assert_eq!(editor.guides()[0].position, 150.);
    assert!(
        editor.selection.is_empty(),
        "a guide press is not a stage selection/marquee press"
    );
    editor.undo();
    assert_eq!(editor.project, added);

    // Drop back onto the ruler: deleted, one Undo brings it back.
    drag(
        &mut editor,
        &ctx,
        0.7,
        on_guide,
        Pos2::new(on_guide.x, canvas.top() + 8.),
    );
    assert!(editor.guides().is_empty());
    editor.undo();
    assert_eq!(editor.project, added);

    // Releasing a ruler drag on the ruler itself creates nothing.
    drag(&mut editor, &ctx, 0.9, ruler, ruler + Vec2::new(40., 4.));
    assert_eq!(editor.project, added);
}

#[test]
fn locked_guides_stay_put_and_guides_are_per_timeline() {
    let (mut editor, ctx, page) = select_stage();
    editor.add_guide(GuideAxis::Vertical, 300.);
    editor.stage_view.prefs.guides_lock = true;
    stage_input_frame(&mut editor, &ctx, 0.1, vec![]);
    let on_guide = Pos2::new(page.left() + 300., page.center().y);
    drag(
        &mut editor,
        &ctx,
        0.2,
        on_guide,
        on_guide + Vec2::new(80., 0.),
    );
    assert_eq!(
        editor.guides()[0].position,
        300.,
        "locked guides cannot be dragged"
    );
    assert!(editor.stage_view.selected_guide.is_none());

    // Hidden guides are not interactive either.
    editor.stage_view.prefs.guides_lock = false;
    editor.stage_view.prefs.guides_show = false;
    drag(
        &mut editor,
        &ctx,
        0.4,
        on_guide,
        on_guide + Vec2::new(80., 0.),
    );
    assert_eq!(editor.guides()[0].position, 300.);

    // A symbol timeline has its own (empty) guide list; the scene keeps its guide.
    let root = editor.project.root.clone();
    let mut symbol = editor.project.symbol(&root).unwrap().clone();
    symbol.id = "guide-symbol".into();
    symbol.name = "Guide symbol".into();
    symbol.timeline.guides.clear();
    for layer in &mut symbol.timeline.layers {
        layer.id = new_id("layer");
        for key in &mut layer.keyframes {
            key.elements.clear();
        }
    }
    editor.project.symbols.push(symbol);
    editor.enter_symbol("guide-symbol".into());
    assert!(editor.guides().is_empty());
    editor.add_guide(GuideAxis::Horizontal, -20.);
    assert_eq!(editor.guides().len(), 1);
    editor.leave_to_level(0);
    assert_eq!(editor.guides()[0].position, 300.);
    // Guides survive save/open.
    let reopened = Project::from_json(&editor.project.to_json().unwrap()).unwrap();
    assert_eq!(reopened, editor.project);
}

fn shortcut_frame(editor: &mut Editor, ctx: &egui::Context, t: f64, events: Vec<Event>) {
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1280., 900.))),
            time: Some(t),
            events,
            ..Default::default()
        },
        |ctx| {
            editor.shortcuts(ctx);
            editor.stage(ctx);
        },
    );
}

fn key(key: egui::Key, modifiers: egui::Modifiers) -> Event {
    Event::Key {
        key,
        physical_key: Some(key),
        pressed: true,
        repeat: false,
        modifiers,
    }
}

#[test]
fn shortcuts_toggle_view_settings_and_delete_the_selected_guide() {
    use egui::{Key, Modifiers as M};
    let (mut editor, ctx, page) = select_stage();
    let defaults = ViewPrefs::default();
    let cmd = M::COMMAND;
    for (event, check) in [
        (
            key(Key::R, cmd | M::ALT | M::SHIFT),
            (|v: &ViewPrefs| v.rulers) as fn(&ViewPrefs) -> bool,
        ),
        (key(Key::Semicolon, cmd), |v| v.guides_show),
        (key(Key::Semicolon, cmd | M::SHIFT), |v| v.guides_snap),
        (key(Key::Semicolon, cmd | M::ALT), |v| v.guides_lock),
        (key(Key::Quote, cmd), |v| v.grid_show),
        (key(Key::Quote, cmd | M::SHIFT), |v| v.grid_snap),
    ] {
        let before = check(&editor.stage_view.prefs);
        shortcut_frame(&mut editor, &ctx, 0.1, vec![event.clone()]);
        assert_ne!(check(&editor.stage_view.prefs), before, "{event:?}");
        shortcut_frame(&mut editor, &ctx, 0.2, vec![event.clone()]);
        assert_eq!(check(&editor.stage_view.prefs), before, "{event:?}");
    }
    assert_eq!(editor.stage_view.prefs, defaults);
    shortcut_frame(&mut editor, &ctx, 0.3, vec![key(Key::G, cmd | M::ALT)]);
    assert!(editor.stage_view.settings_open, "⌥⌘G opens Edit grid");
    editor.stage_view.settings_open = false;

    // Select a guide by clicking it, then Delete removes the guide, not stage content.
    editor.add_guide(GuideAxis::Vertical, 200.);
    editor.stage_view.selected_guide = None;
    let at = Pos2::new(page.left() + 200., page.center().y);
    shortcut_frame(
        &mut editor,
        &ctx,
        0.4,
        vec![Event::PointerMoved(at), primary_button(at, true)],
    );
    shortcut_frame(&mut editor, &ctx, 0.45, vec![primary_button(at, false)]);
    assert!(editor.stage_view.selected_guide.is_some());
    shortcut_frame(&mut editor, &ctx, 0.5, vec![key(Key::Delete, M::NONE)]);
    assert!(editor.guides().is_empty());
    assert!(editor.history.can_undo());
}

#[test]
fn view_settings_clear_guides_is_one_undo_and_save_as_default_persists() {
    let (mut editor, _ctx, _page) = select_stage();
    editor.add_guide(GuideAxis::Vertical, 10.);
    editor.add_guide(GuideAxis::Horizontal, 20.);
    let with_guides = editor.project.clone();
    editor.clear_guides();
    assert!(editor.guides().is_empty());
    editor.undo();
    assert_eq!(editor.project, with_guides);

    // Popover state round trip: theme-default colours stay `None`, edits come back.
    let p = neoflash_ui::palette(&_ctx);
    let mut prefs = ViewPrefs::default();
    let mut state = prefs.to_state(p);
    assert_eq!(state.grid_w, 20.);
    assert_eq!(state.units, "Units: Pixels");
    state.grid_snap = true;
    state.grid_w = 8.;
    state.units = "Units: Inches";
    state.guide_snap = "Snap: Can be distant";
    state.guide_color = egui::Color32::from_rgb(255, 0, 0);
    prefs.apply_state(&state, p);
    assert!(prefs.grid_snap);
    assert_eq!(prefs.grid_spacing[0], 8.);
    assert_eq!(prefs.units, crate::stage_view::RulerUnits::Inches);
    assert_eq!(prefs.guide_accuracy, SnapAccuracy::CanBeDistant);
    assert_eq!(prefs.guide_color, Some([255, 0, 0, 255]));
    assert_eq!(prefs.grid_color, None);

    // Save as default → workspace preferences JSON → next session.
    editor.workspace.view_defaults = Some(prefs.clone());
    let json = serde_json::to_string(&editor.workspace).unwrap();
    let loaded = crate::workspace::WorkspacePrefs::from_json(&json);
    assert_eq!(loaded.view_defaults, Some(prefs));
    // Older preferences without the key load the NF2 defaults.
    let old = crate::workspace::WorkspacePrefs::from_json("{\"version\":1}");
    assert_eq!(old.view_defaults, None);
}

#[test]
fn drawing_and_moving_snap_to_grid_and_guides() {
    let (mut editor, ctx, page) = rectangle_stage();
    editor.stage_view.prefs.grid_snap = true;
    editor.stage_view.prefs.grid_accuracy = SnapAccuracy::Normal;
    stage_input_frame(&mut editor, &ctx, 0.01, vec![]);
    // Rectangle corners land on 20 px grid intersections.
    let a = page.min + Vec2::new(103., 57.);
    let b = page.min + Vec2::new(178., 141.);
    stage_input_frame(
        &mut editor,
        &ctx,
        0.1,
        vec![Event::PointerMoved(a), primary_button(a, true)],
    );
    stage_input_frame(&mut editor, &ctx, 0.12, vec![Event::PointerMoved(b)]);
    assert!(
        editor
            .stage_view
            .snap_ring
            .is_some_and(|(_, snapped)| snapped),
        "snap ring shows the snapped corner while drawing"
    );
    stage_input_frame(&mut editor, &ctx, 0.14, vec![primary_button(b, false)]);
    let element = editor.symbol().timeline.layers[0].keyframes[0].elements[0].clone();
    assert_eq!((element.transform.x, element.transform.y), (100., 60.));
    let ElementKind::Rectangle { width, height, .. } = element.kind else {
        panic!("rectangle expected")
    };
    assert_eq!((width, height), (80., 80.));

    // Moving: with grid snapping off, the right edge (180) snaps to a guide at 205 when it comes
    // within 8 screen points, and the move is still one Undo step.
    editor.stage_view.prefs.grid_snap = false;
    editor.add_guide(GuideAxis::Vertical, 205.);
    editor.tool = Tool::Select;
    editor.set_selection(vec![]);
    let before = editor.project.clone();
    let grab = page.min + Vec2::new(140., 100.);
    drag(&mut editor, &ctx, 0.3, grab, grab + Vec2::new(21., 0.));
    let moved = &editor.symbol().timeline.layers[0].keyframes[0].elements[0];
    assert_eq!(moved.transform.x, 125., "right edge 205 = guide");
    assert_eq!(moved.transform.y, 60.);
    editor.undo();
    assert_eq!(editor.project, before);
    // Far from any line nothing snaps.
    drag(&mut editor, &ctx, 0.5, grab, grab + Vec2::new(-50., 0.));
    let moved = &editor.symbol().timeline.layers[0].keyframes[0].elements[0];
    assert_eq!(moved.transform.x, 50.);
}

#[test]
fn view_menu_and_zoom_control_share_the_toggles() {
    use crate::tests::primary_button as press;
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    let size = egui::vec2(1920., 1080.);
    let frame = |editor: &mut Editor, t: f64, events: Vec<Event>| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
                time: Some(t),
                events,
                ..Default::default()
            },
            |ctx| {
                editor.shortcuts(ctx);
                editor.menu(ctx);
                editor.bitmap_textures.begin_frame(&editor.project);
                editor.prepare_docked_panel_focus(ctx);
                editor.dock_grid(ctx);
                editor.stage(ctx);
            },
        )
    };
    let bounds = |out: &egui::FullOutput, label: &str| {
        out.platform_output
            .accesskit_update
            .as_ref()?
            .nodes
            .iter()
            .find_map(|(_, node)| {
                (node.label() == Some(label))
                    .then(|| node.bounds())
                    .flatten()
                    .map(|r| {
                        Rect::from_min_max(
                            Pos2::new(r.x0 as f32, r.y0 as f32),
                            Pos2::new(r.x1 as f32, r.y1 as f32),
                        )
                    })
            })
    };
    let click = |editor: &mut Editor, t: f64, at: Pos2| {
        frame(editor, t, vec![Event::PointerMoved(at), press(at, true)]);
        frame(editor, t + 0.01, vec![press(at, false)]);
        frame(editor, t + 0.02, vec![])
    };
    let mut out = frame(&mut editor, 0.0, vec![]);
    for i in 1..4 {
        out = frame(&mut editor, i as f64 * 0.02, vec![]);
    }
    // Zoom control toggles are visible (Some) and change the shared state.
    for (label, check) in [
        (
            "Rulers",
            (|v: &ViewPrefs| v.rulers) as fn(&ViewPrefs) -> bool,
        ),
        ("Guides", |v| v.guides_show),
        ("Grid", |v| v.grid_show),
        ("Snap to guides", |v| v.guides_snap),
    ] {
        let button = bounds(&out, label).unwrap_or_else(|| panic!("zoom control shows {label}"));
        let before = check(&editor.stage_view.prefs);
        click(&mut editor, 1.0, button.center());
        assert_ne!(check(&editor.stage_view.prefs), before, "{label}");
        out = click(&mut editor, 1.5, button.center());
        assert_eq!(check(&editor.stage_view.prefs), before, "{label}");
    }
    // ⚙ opens the real View settings popover; its Close button closes it.
    let gear = bounds(&out, "Rulers, guides and grid…").expect("⚙ in the zoom control");
    out = click(&mut editor, 2.0, gear.center());
    assert!(editor.stage_view.settings_open);
    let close = bounds(&out, "Close").expect("popover close button");
    click(&mut editor, 2.5, close.center());
    assert!(!editor.stage_view.settings_open);

    // View ▸ Rulers toggles the same setting the zoom control shows.
    let out = frame(&mut editor, 3.0, vec![]);
    let view = bounds(&out, "View").expect("View menu");
    let out = click(&mut editor, 3.1, view.center());
    let rulers = bounds(&out, "Rulers").expect("View ▸ Rulers is enabled");
    click(&mut editor, 3.2, rulers.center());
    assert!(!editor.stage_view.prefs.rulers);
}

#[test]
fn object_and_pixel_snapping_respect_priority_visibility_and_command_suspend() {
    let (mut editor, ctx, _page) = select_stage();
    // Unselected reference rectangle on layer 0: x in [200, 300] (center 250), y in [100, 160] (center 130).
    editor.add_element(
        ElementKind::Rectangle {
            width: 100.,
            height: 60.,
            fill: [40, 120, 200, 255],
            stroke: [0; 4],
            stroke_width: 0.,
        },
        200.,
        100.,
    );
    // Add a locked layer with an element at x = 400 (must NOT produce object snap candidates).
    let mut locked_layer = editor.symbol().timeline.layers[0].clone();
    locked_layer.id = new_id("layer");
    locked_layer.name = "Locked".into();
    locked_layer.locked = true;
    locked_layer.keyframes[0].elements = vec![Element {
        accessibility: None,
        id: new_id("locked-rect"),
        name: "LockedRect".into(),
        transform: Transform {
            x: 400.,
            y: 100.,
            ..Default::default()
        },
        kind: ElementKind::Rectangle {
            width: 50.,
            height: 50.,
            fill: [200, 50, 50, 255],
            stroke: [0; 4],
            stroke_width: 0.,
        },
        scripts: vec![],
    }];
    editor.symbol_mut().timeline.layers.push(locked_layer);
    editor.clear_selection();

    editor.stage_view.prefs.guides_snap = false;
    editor.stage_view.prefs.grid_snap = false;
    editor.stage_view.prefs.objects_snap = false;
    editor.stage_view.prefs.pixels_snap = false;
    assert!(editor.snapper().is_none());

    // Object snapping snaps to unselected center x=250, y=130 and ignores locked x=400.
    editor.toggle_object_snap();
    assert!(editor.stage_view.prefs.objects_snap);
    let snapped_obj = editor.snapper().unwrap().point([252.5, 128.0]);
    assert_eq!(snapped_obj.point, [250.0, 130.0]);
    let near_locked = editor.snapper().unwrap().point([401.0, 102.0]);
    assert_eq!(
        near_locked.point[0], 401.0,
        "locked layer object must not snap"
    );

    // Selecting the reference object excludes it from snapping against itself.
    editor.select_all();
    assert!(
        editor.snapper().is_none(),
        "only selected/locked objects exist so no object snap lines remain"
    );
    editor.clear_selection();

    // Priority: Guides > Objects > Grid > Pixels around x ≈ 251.3.
    editor.stage_view.prefs.guides_show = true;
    editor.stage_view.prefs.guides_snap = true;
    editor.stage_view.prefs.grid_snap = true;
    editor.stage_view.prefs.grid_spacing = [126.0, 126.0]; // grid line at 252.0
    editor.toggle_pixel_snap();
    assert!(editor.stage_view.prefs.pixels_snap);
    editor.symbol_mut().timeline.guides.push(Guide {
        axis: GuideAxis::Vertical,
        position: 253.0,
    });
    let probe = [251.3, 18.4];
    assert_eq!(
        editor.snapper().unwrap().point(probe).point[0],
        253.0,
        "1. Guides win"
    );

    editor.stage_view.prefs.guides_snap = false;
    assert_eq!(
        editor.snapper().unwrap().point(probe).point[0],
        250.0,
        "2. Objects win over Grid and Pixels"
    );

    editor.toggle_object_snap();
    assert_eq!(
        editor.snapper().unwrap().point(probe).point[0],
        252.0,
        "3. Grid wins over Pixels"
    );

    editor.stage_view.prefs.grid_snap = false;
    assert_eq!(
        editor.snapper().unwrap().point(probe).point,
        [251.0, 18.0],
        "4. Pixels snap to nearest integer"
    );

    // Holding Command (⌘) suspends all snapping.
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280., 900.))),
            time: Some(0.5),
            modifiers: egui::Modifiers::COMMAND,
            ..Default::default()
        },
        |ctx| editor.stage(ctx),
    );
    assert!(editor.stage_view.command_suspend);
    assert!(editor.snapper().is_none());
}
