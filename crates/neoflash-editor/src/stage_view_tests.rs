//! Rulers, guides, grid and snapping through the real stage event loop, menus and shortcuts
//! (docs/NF2-RULERS-GUIDES-GRID.md "Tests").
use crate::{
    app::{Editor, Tool},
    stage_view::{GridMode, SnapAccuracy, ViewPrefs},
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

/// Identify actual grid paint by its page extent, independently of the chosen colour. Rulers,
/// page borders and the short symbol-registration cross must not count as a visible grid.
fn stage_grid_lines(editor: &Editor, out: &egui::FullOutput) -> Vec<[Pos2; 2]> {
    let size = Vec2::new(editor.project.width as f32, editor.project.height as f32) * editor.zoom;
    let origin = editor.stage_view.origin;
    let page = if editor.editing.len() > 1 {
        Rect::from_center_size(origin, size)
    } else {
        Rect::from_min_size(origin, size)
    };
    let visible = page.intersect(editor.stage_view.canvas);
    out.shapes
        .iter()
        .filter_map(|shape| {
            let egui::Shape::LineSegment { points, stroke } = &shape.shape else {
                return None;
            };
            if stroke.color.a() == 0
                || shape.clip_rect.min.distance(visible.min) > 0.01
                || shape.clip_rect.max.distance(visible.max) > 0.01
            {
                return None;
            }
            let [a, b] = *points;
            let vertical = (a.x - b.x).abs() < 0.01
                && a.x > visible.left() + 2.
                && a.x < visible.right() - 2.
                && (a.y - visible.top()).abs() < 1.
                && (b.y - visible.bottom()).abs() < 1.;
            let horizontal = (a.y - b.y).abs() < 0.01
                && a.y > visible.top() + 2.
                && a.y < visible.bottom() - 2.
                && (a.x - visible.left()).abs() < 1.
                && (b.x - visible.right()).abs() < 1.;
            (vertical || horizontal).then_some(*points)
        })
        .collect()
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

    // Popover state round trip: Dynamic is the default; unrelated edits do not set a manual colour.
    let p = neoflash_ui::palette(&_ctx);
    let mut prefs = ViewPrefs::default();
    let mut state = prefs.to_state(p);
    assert_eq!(prefs.grid_mode, GridMode::Dynamic);
    assert_eq!(state.grid_mode, "Dynamic");
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

    // An actual Static colour edit stores RGBA, including opacity.
    state.grid_mode = "Static";
    state.grid_color = egui::Color32::from_rgba_unmultiplied(32, 96, 192, 128);
    prefs.apply_state(&state, p);
    assert_eq!(prefs.grid_mode, GridMode::Static);
    assert_eq!(
        prefs.grid_color,
        Some(state.grid_color.to_srgba_unmultiplied())
    );

    // Legacy saved colours need not round-trip losslessly through premultiplied Color32. Merely
    // opening the popover, changing theme or mode must preserve the exact saved bytes.
    let saved_rgba = [31, 97, 193, 128];
    prefs.grid_color = Some(saved_rgba);
    for theme in [
        crate::theme::ThemeMode::Light,
        crate::theme::ThemeMode::Dark,
    ] {
        theme.apply(&_ctx);
        let p = neoflash_ui::palette(&_ctx);
        for (label, mode) in [("Dynamic", GridMode::Dynamic), ("Static", GridMode::Static)] {
            let mut state = prefs.to_state(p);
            state.grid_mode = label;
            prefs.apply_state(&state, p);
            assert_eq!(prefs.grid_mode, mode);
            assert_eq!(prefs.grid_color, Some(saved_rgba));
            prefs.apply_state(&prefs.to_state(p), p);
            assert_eq!(
                prefs.grid_color,
                Some(saved_rgba),
                "idle popover is lossless"
            );
        }
    }

    // Save as default → workspace preferences JSON → next session.
    editor.workspace.view_defaults = Some(prefs.clone());
    let json = serde_json::to_string(&editor.workspace).unwrap();
    let loaded = crate::workspace::WorkspacePrefs::from_json(&json);
    assert_eq!(loaded.view_defaults, Some(prefs));
    // Older preferences without the key load the NF2 defaults.
    let old = crate::workspace::WorkspacePrefs::from_json("{\"version\":1}");
    assert_eq!(old.view_defaults, None);
    let legacy: ViewPrefs =
        serde_json::from_str(r#"{"grid_color":[31,97,193,128],"grid_spacing":[8,24]}"#).unwrap();
    assert_eq!(
        legacy.grid_mode,
        GridMode::Dynamic,
        "missing mode defaults to Dynamic"
    );
    assert_eq!(legacy.grid_color, Some(saved_rgba));
    assert_eq!(legacy.grid_spacing, [8., 24.]);
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

    // Moving: the right edge (180) snaps to a guide at 205 when it comes within 8 screen points.
    // Grid candidates on the other edges are closer, but must not override the guide's priority.
    // Grid visibility is independent of snapping, and the move is still one Undo step.
    editor.stage_view.prefs.grid_show = false;
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
    // Far from the guide with grid snapping disabled, nothing snaps.
    editor.stage_view.prefs.grid_snap = false;
    drag(&mut editor, &ctx, 0.5, grab, grab + Vec2::new(-50., 0.));
    let moved = &editor.symbol().timeline.layers[0].keyframes[0].elements[0];
    assert_eq!(moved.transform.x, 50.);

    // Showing the grid alone does not snap either shape tool; the disabled Snap checkbox must
    // preserve the actual drawn coordinates, even a few pixels from an intersection.
    editor.stage_view.prefs.grid_show = true;
    for (i, tool) in [Tool::Rectangle, Tool::Ellipse].into_iter().enumerate() {
        editor.tool = tool;
        let before = editor.project.clone();
        drag(
            &mut editor,
            &ctx,
            0.7 + i as f64 * 0.2,
            page.min + Vec2::new(303., 157.),
            page.min + Vec2::new(378., 241.),
        );
        let drawn = editor.selected_sampled().expect("new shape selected");
        assert_eq!((drawn.transform.x, drawn.transform.y), (303., 157.));
        let (ElementKind::Rectangle { width, height, .. }
        | ElementKind::Ellipse { width, height, .. }) = drawn.kind
        else {
            panic!("rectangle or oval expected")
        };
        assert_eq!((width, height), (75., 84.));
        editor.project.validate().unwrap();
        editor.undo();
        assert_eq!(editor.project, before, "drawing is one Undo step");
    }
}

#[test]
fn view_menu_and_zoom_control_share_the_toggles() {
    use crate::tests::primary_button as press;
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    // The toggles live in the expanded floating toolbar (the default is the collapsed pill, R-053).
    editor.workspace.stage_toolbar = crate::workspace::StageToolbarMode::Expanded;
    let project_before = editor.project.clone();
    let saved_rgba = [31, 97, 193, 128];
    editor.stage_view.prefs.grid_color = Some(saved_rgba);
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
        let released = frame(editor, t + 0.01, vec![press(at, false)]);
        assert_eq!(
            released.viewport_output[&egui::ViewportId::ROOT].repaint_delay,
            std::time::Duration::ZERO,
            "a changed control requests the next frame even when the pointer stops"
        );
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
        let toggled = click(&mut editor, 1.0, button.center());
        assert_ne!(check(&editor.stage_view.prefs), before, "{label}");
        if label == "Grid" {
            assert!(
                stage_grid_lines(&editor, &toggled).is_empty(),
                "hidden grid emits no paint"
            );
        }
        out = click(&mut editor, 1.5, button.center());
        assert_eq!(check(&editor.stage_view.prefs), before, "{label}");
        if label == "Grid" {
            assert!(
                !stage_grid_lines(&editor, &out).is_empty(),
                "show takes effect without another input"
            );
        }
    }
    // ⚙ opens the real View settings popover; its Close button closes it.
    let gear = bounds(&out, "Rulers, guides and grid…").expect("⚙ in the zoom control");
    out = click(&mut editor, 2.0, gear.center());
    assert!(editor.stage_view.settings_open);
    // The lower "Show" checkbox is the grid's (the upper one belongs to Guides). It applies
    // before the next idle frame, without needing a mouse move or another menu command.
    let show_grid = out
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .filter_map(|(_, node)| {
            (node.label() == Some("Show"))
                .then(|| node.bounds())
                .flatten()
        })
        .max_by(|a, b| a.y0.total_cmp(&b.y0))
        .expect("grid Show checkbox");
    let at = Pos2::new(
        ((show_grid.x0 + show_grid.x1) * 0.5) as f32,
        ((show_grid.y0 + show_grid.y1) * 0.5) as f32,
    );
    out = click(&mut editor, 2.1, at);
    assert!(!editor.stage_view.prefs.grid_show);
    assert!(stage_grid_lines(&editor, &out).is_empty());
    out = click(&mut editor, 2.2, at);
    assert!(editor.stage_view.prefs.grid_show);
    assert!(!stage_grid_lines(&editor, &out).is_empty());

    // Use the real Select, then Save as default: the colour control follows the mode and the
    // latent Static RGBA survives Dynamic, persistence and an idle frame without editing content.
    let color_disabled = |out: &egui::FullOutput| {
        out.platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("Grid colour"))
            .expect("grid colour control")
            .1
            .is_disabled()
    };
    assert!(color_disabled(&out));
    for (i, (from, to, mode)) in [
        ("Dynamic", "Static", GridMode::Static),
        ("Static", "Dynamic", GridMode::Dynamic),
    ]
    .into_iter()
    .enumerate()
    {
        let at = bounds(&out, from).expect("current grid mode").center();
        out = click(&mut editor, 2.3 + i as f64 * 0.2, at);
        let option = bounds(&out, to).expect("grid mode option");
        out = click(&mut editor, 2.4 + i as f64 * 0.2, option.center());
        assert_eq!(editor.stage_view.prefs.grid_mode, mode);
        assert_eq!(color_disabled(&out), mode == GridMode::Dynamic);
        assert_eq!(editor.stage_view.prefs.grid_color, Some(saved_rgba));
        assert!(!stage_grid_lines(&editor, &out).is_empty());
    }
    let save = bounds(&out, "Save as default").expect("save default button");
    out = click(&mut editor, 2.7, save.center());
    let loaded = crate::workspace::WorkspacePrefs::from_json(
        &serde_json::to_string(&editor.workspace).unwrap(),
    );
    assert_eq!(loaded.view_defaults, Some(editor.stage_view.prefs.clone()));
    assert_eq!(loaded.view_defaults.unwrap().grid_color, Some(saved_rgba));
    let close = bounds(&out, "Close").expect("popover close button");
    click(&mut editor, 2.8, close.center());
    assert!(!editor.stage_view.settings_open);

    // View ▸ Rulers toggles the same setting the zoom control shows.
    let out = frame(&mut editor, 3.0, vec![]);
    let view = bounds(&out, "View").expect("View menu");
    let out = click(&mut editor, 3.1, view.center());
    let rulers = bounds(&out, "Rulers").expect("View ▸ Rulers is enabled");
    click(&mut editor, 3.2, rulers.center());
    assert!(!editor.stage_view.prefs.rulers);
    assert_eq!(editor.project, project_before);
    assert!(!editor.history.can_undo());
}

#[test]
fn grid_modes_preserve_axes_origin_and_snap_spacing_across_zoom_and_pan() {
    let ctx = egui::Context::default();
    let mut project = Project::blank();
    let mut symbol = project.symbols[0].clone();
    symbol.id = "grid-origin-symbol".into();
    symbol.name = "Grid origin".into();
    for layer in &mut symbol.timeline.layers {
        layer.id = new_id("grid-layer");
    }
    project.symbols.push(symbol);
    let mut editor = Editor::from_project(&ctx, project);
    editor.stage_view.prefs.guides_show = false;
    editor.stage_view.prefs.guides_snap = false;
    editor.stage_view.prefs.objects_snap = false;
    editor.stage_view.prefs.pixels_snap = false;
    editor.stage_view.prefs.grid_snap = true;
    let mut time = 0.;
    for (theme, background) in [
        (crate::theme::ThemeMode::Light, [19, 24, 36, 255]),
        (crate::theme::ThemeMode::Dark, [255; 4]),
    ] {
        editor.set_theme(&ctx, theme);
        editor.project.background = background;
        let before = editor.project.clone();
        for nested in [false, true] {
            if nested {
                editor.enter_symbol("grid-origin-symbol".into());
            } else {
                editor.leave_to_level(0);
            }
            editor.stage_view.prefs.grid_mode = GridMode::Dynamic;
            for zoom in [0.08, 0.15, 1., 20.] {
                editor.zoom = zoom;
                for spacing in [[20., 20.], [1., 20.], [20., 1.]] {
                    editor.stage_view.prefs.grid_spacing = spacing;
                    for pan in [Vec2::ZERO, Vec2::new(37.25, -19.5)] {
                        editor.pan = pan;
                        time += 0.1;
                        let out = stage_input_frame(&mut editor, &ctx, time, vec![]);
                        let lines = stage_grid_lines(&editor, &out);
                        for axis in 0..2 {
                            assert!(
                                lines.iter().any(|[a, b]| a[axis] == b[axis]),
                                "grid axis {axis} survives zoom={zoom}, spacing={spacing:?}, nested={nested}, pan={pan:?}"
                            );
                            if nested {
                                assert!(
                                    lines.iter().any(|[a, b]| a[axis] == b[axis]
                                        && (a[axis] - editor.stage_view.origin[axis]).abs() < 1.1),
                                    "grid zero stays at symbol registration"
                                );
                            }
                        }
                        // Coarse displayed cells never change the configured snap intersections.
                        let snapper = editor.snapper().expect("grid snap stays enabled");
                        assert_eq!(snapper.grid.unwrap().0, spacing);
                        let target = [spacing[0] * 3., spacing[1] * 7.];
                        assert_eq!(
                            snapper.point([target[0] + 0.1, target[1] + 0.1]).point,
                            target
                        );
                    }
                }
            }
            // Static does not coarsen; a subpixel axis hides without suppressing the other axis.
            editor.stage_view.prefs.grid_mode = GridMode::Static;
            editor.zoom = 0.08;
            for spacing in [[1., 20.], [20., 1.]] {
                editor.stage_view.prefs.grid_spacing = spacing;
                time += 0.1;
                let out = stage_input_frame(&mut editor, &ctx, time, vec![]);
                let lines = stage_grid_lines(&editor, &out);
                for axis in 0..2 {
                    assert_eq!(
                        lines.iter().any(|[a, b]| a[axis] == b[axis]),
                        spacing[axis] == 20.
                    );
                }
                assert_eq!(editor.snapper().unwrap().grid.unwrap().0, spacing);
            }
        }
        assert_eq!(
            editor.project, before,
            "view changes never edit the document"
        );
    }
    assert!(!editor.history.can_undo());
    editor.project.validate().unwrap();
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
        ..Element::default()
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

#[test]
fn drawing_snap_tolerance_scales_object_snapping_with_zoom() {
    let (mut editor, _ctx, _page) = select_stage();
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
    editor.clear_selection();
    editor.stage_view.prefs.guides_snap = false;
    editor.stage_view.prefs.grid_snap = false;
    editor.stage_view.prefs.pixels_snap = false;
    editor.stage_view.prefs.objects_snap = true;
    editor.zoom = 1.0;

    // Reference edge at x = 200; probe 14 px away misses default 8 px tolerance.
    assert_eq!(
        editor.snapper().unwrap().point([186.0, 130.0]).point[0],
        186.0
    );

    // Raising Preferences ▸ Drawing ▸ Snap tolerance to 16 px captures 14 px at 100% zoom,
    // and halves in stage coordinates to 8 stage px at 200% zoom.
    editor.workspace.drawing.snap_tolerance = 16.0;
    assert_eq!(
        editor.snapper().unwrap().point([186.0, 130.0]).point[0],
        200.0
    );
    editor.zoom = 2.0;
    assert_eq!(
        editor.snapper().unwrap().point([186.0, 130.0]).point[0],
        186.0
    );
    assert_eq!(
        editor.snapper().unwrap().point([193.0, 130.0]).point[0],
        200.0
    );
}
