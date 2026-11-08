//! Panel integration checks use real egui layout, input, clipping and AX output.
use crate::{
    app::Editor,
    tests::primary_button,
    theme::{DesignUi as _, ThemeMode},
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
            editor.bitmap_textures.begin_frame(&editor.project);
            editor.prepare_docked_panel_focus(ctx);
            if !editor.panels_hidden {
                editor.dock_grid(ctx);
            }
            editor.document_bar(ctx);
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
/// Like [`named_bounds`], but the node must lie inside `area` (labels such as "New folder"
/// exist in several panels).
fn named_bounds_in(output: &egui::FullOutput, label: &str, area: Rect) -> Option<Rect> {
    output
        .platform_output
        .accesskit_update
        .as_ref()?
        .nodes
        .iter()
        .filter(|(_, node)| node.label() == Some(label))
        .filter_map(|(_, node)| {
            node.bounds().map(|r| {
                Rect::from_min_max(
                    Pos2::new(r.x0 as f32, r.y0 as f32),
                    Pos2::new(r.x1 as f32, r.y1 as f32),
                )
            })
        })
        .find(|rect| area.contains_rect(*rect))
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

/// Center of a painted `NF2/TL Marker` Kind=Keyframe (filled, Tone=Default) that is not clipped.
fn key_marker(shape: &egui::epaint::ClippedShape, ctx: &egui::Context) -> Option<Pos2> {
    match &shape.shape {
        egui::Shape::Circle(circle)
            if (1.5..=3.5).contains(&circle.radius)
                && circle.fill == neoflash_ui::palette(ctx).timeline_marker
                && shape.clip_rect.contains(circle.center) =>
        {
            Some(circle.center)
        }
        _ => None,
    }
}

#[test]
fn narrow_tools_rail_shows_fitting_groups_then_more_and_flyouts_choose_tools() {
    fn rail(
        editor: &mut Editor,
        ctx: &egui::Context,
        time: f64,
        events: Vec<Event>,
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400., 400.))),
                time: Some(time),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let rect = Rect::from_min_size(Pos2::new(8., 8.), Vec2::new(52., 340.));
                    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                        editor.tools_contents(ui)
                    });
                });
            },
        )
    }
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    let before = editor.project.clone();
    rail(&mut editor, &ctx, 0.1, vec![]);
    let out = rail(&mut editor, &ctx, 0.2, vec![]);
    // One column: grouped tools, pinned colours/options, and "…" for the rest.
    assert!(named_bounds(&out, "Selection (V)").is_some());
    assert!(
        named_bounds(&out, "Subselection (A)").is_none(),
        "grouped behind Selection"
    );
    for pinned in [
        "Fill colour",
        "Stroke colour",
        "Swap colors",
        "Tool options",
    ] {
        assert!(named_bounds(&out, pinned).is_some(), "missing {pinned}");
    }
    let more = named_bounds(&out, "More tools").expect("short rail ends with More tools");
    assert!(more.bottom() <= 348., "{more:?}");
    assert!(
        named_bounds(&out, "Fit stage").is_none(),
        "Fit moved into More tools"
    );

    // Regression: an outside press used to query egui's layer stack while `Context::input`
    // still held its write lock. Opening Options and then right-clicking a grouped tool therefore
    // deadlocked the whole editor. The same frame must now close Options and open the flyout.
    let options = named_bounds(&out, "Tool options").expect("rail pins Tool options");
    rail(
        &mut editor,
        &ctx,
        0.21,
        vec![
            Event::PointerMoved(options.center()),
            primary_button(options.center(), true),
            primary_button(options.center(), false),
        ],
    );
    let out = rail(&mut editor, &ctx, 0.22, vec![]);
    assert!(
        named_bounds(&out, "Tool properties…").is_some(),
        "Options popover is open before the outside click"
    );
    let selection = named_bounds(&out, "Selection (V)").unwrap().center();
    rail(
        &mut editor,
        &ctx,
        0.23,
        vec![
            Event::PointerMoved(selection),
            Event::PointerButton {
                pos: selection,
                button: egui::PointerButton::Secondary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            Event::PointerButton {
                pos: selection,
                button: egui::PointerButton::Secondary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    let out = rail(&mut editor, &ctx, 0.24, vec![]);
    assert!(
        named_bounds(&out, "Subselection tool").is_some(),
        "outside secondary click replaces Options with the grouped-tool flyout"
    );
    rail(
        &mut editor,
        &ctx,
        0.25,
        vec![Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    rail(&mut editor, &ctx, 0.26, vec![]);

    rail(
        &mut editor,
        &ctx,
        0.3,
        vec![
            Event::PointerMoved(more.center()),
            primary_button(more.center(), true),
            primary_button(more.center(), false),
        ],
    );
    let out = rail(&mut editor, &ctx, 0.4, vec![]);
    let fit = named_bounds(&out, "Fit stage").expect("More tools lists Fit stage");
    editor.zoom = 1.5;
    rail(
        &mut editor,
        &ctx,
        0.5,
        vec![
            Event::PointerMoved(fit.center()),
            primary_button(fit.center(), true),
            primary_button(fit.center(), false),
        ],
    );
    assert_eq!(editor.zoom, 0., "Fit from the overflow popover");
    // Right-click opens the Selection group; choosing Subselection makes it the visible tool.
    let selection = named_bounds(&out, "Selection (V)").unwrap().center();
    rail(
        &mut editor,
        &ctx,
        0.7,
        vec![
            Event::PointerMoved(selection),
            Event::PointerButton {
                pos: selection,
                button: egui::PointerButton::Secondary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            Event::PointerButton {
                pos: selection,
                button: egui::PointerButton::Secondary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    let out = rail(&mut editor, &ctx, 0.8, vec![]);
    let item = named_bounds(&out, "Subselection tool").expect("flyout lists the group");
    rail(
        &mut editor,
        &ctx,
        0.9,
        vec![
            Event::PointerMoved(item.center()),
            primary_button(item.center(), true),
            primary_button(item.center(), false),
        ],
    );
    assert!(editor.tool == crate::app::Tool::Subselect);
    let out = rail(&mut editor, &ctx, 1.0, vec![]);
    assert!(named_bounds(&out, "Subselection (A)").is_some());
    assert!(named_bounds(&out, "Selection (V)").is_none());

    // v2.2's painted corner mark opens the remembered group without activating its rail tool.
    editor.set_tool(crate::app::Tool::Brush);
    let out = rail(&mut editor, &ctx, 1.1, vec![]);
    let subselection = named_bounds(&out, "Subselection (A)").unwrap();
    let mark = subselection.right_bottom() - Vec2::splat(5.);
    rail(
        &mut editor,
        &ctx,
        1.2,
        vec![
            Event::PointerMoved(mark),
            primary_button(mark, true),
            primary_button(mark, false),
        ],
    );
    let out = rail(&mut editor, &ctx, 1.3, vec![]);
    assert!(named_bounds(&out, "Subselection tool").is_some());
    assert!(editor.tool == crate::app::Tool::Brush);
    rail(
        &mut editor,
        &ctx,
        1.4,
        vec![Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    // The Escape frame still paints the popup before closing it. Let egui retire that
    // foreground hit layer, then obtain the live rail bounds for the next interaction.
    let out = rail(
        &mut editor,
        &ctx,
        1.45,
        vec![Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(named_bounds(&out, "Subselection tool").is_none());
    let subselection = named_bounds(&out, "Subselection (A)").unwrap();
    // The main button body still selects the group's remembered tool, without a flyout.
    rail(
        &mut editor,
        &ctx,
        1.5,
        vec![
            Event::PointerMoved(subselection.center()),
            primary_button(subselection.center(), true),
        ],
    );
    rail(
        &mut editor,
        &ctx,
        1.55,
        vec![primary_button(subselection.center(), false)],
    );
    let out = rail(&mut editor, &ctx, 1.6, vec![]);
    assert!(editor.tool == crate::app::Tool::Subselect);
    assert!(named_bounds(&out, "Subselection tool").is_none());

    // Holding the same body retains the existing long-press entry path.
    rail(
        &mut editor,
        &ctx,
        1.7,
        vec![
            Event::PointerMoved(subselection.center()),
            primary_button(subselection.center(), true),
        ],
    );
    let out = rail(&mut editor, &ctx, 1.9, vec![]);
    assert!(named_bounds(&out, "Subselection tool").is_none());
    rail(&mut editor, &ctx, 2.2, vec![]);
    let out = rail(&mut editor, &ctx, 2.3, vec![]);
    assert!(named_bounds(&out, "Subselection tool").is_some());
    rail(
        &mut editor,
        &ctx,
        2.4,
        vec![primary_button(subselection.center(), false)],
    );
    assert_eq!(editor.project, before);
}

#[test]
fn tools_scroll_to_fit_and_library_scroll_to_last_symbol_at_minimum_window() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut project = Project::blank();
    for i in 0..100 {
        let mut symbol = project.symbols[0].clone();
        symbol.id = format!("fixture-symbol-{i}");
        symbol.name = format!("Symbol {i:03}");
        symbol.timeline.layers[0].id = format!("fixture-layer-{i}");
        project.symbols.push(symbol);
    }
    let mut editor = Editor::from_project(&ctx, project);
    editor.set_theme(&ctx, ThemeMode::Light);
    let before = editor.project.clone();
    let size = Vec2::new(1024., 720.);
    let first = frame(&mut editor, &ctx, size, 0.1, vec![]);
    // Scroll actual panels, not their state objects: a user must be able to
    // reach both the final view tool and a long library's last entry. The
    // default dock is the two-column Tools panel; its whole body scrolls.
    let tools = editor
        .detached
        .dock_groups
        .iter()
        .find(|group| group.tabs.iter().any(|(panel, _)| *panel == PanelId::Tools))
        .expect("Tools has a dock group")
        .rect;
    for tool in ["Selection (V)", "Subselection (A)", "Hand (H / hold Space)"] {
        assert!(
            named_bounds(&first, tool).is_some(),
            "two columns show {tool}"
        );
    }
    let selection = named_bounds(&first, "Selection (V)").expect("Tools shows Selection");
    frame(
        &mut editor,
        &ctx,
        size,
        0.2,
        wheel(selection.center(), Vec2::new(0., -2000.)),
    );
    // Wheel input changes the next layout; inspect the following settled paints.
    let mut out = frame(&mut editor, &ctx, size, 0.21, vec![]);
    for step in 1..5 {
        out = frame(&mut editor, &ctx, size, 0.21 + step as f64 / 60., vec![]);
    }
    let fit = named_bounds(&out, "Fit stage").expect("Fit must have an accessible button");
    assert!(tools.contains_rect(fit), "{fit:?} in {tools:?}");
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
    // At 1024 × 720 the inspector column is too short for Properties over a
    // usable Library; the Library alone must still scroll to every symbol.
    editor.set_panel_visible(PanelId::Properties, false);
    frame(&mut editor, &ctx, size, 0.41, vec![]);
    let before_library_scroll = frame(&mut editor, &ctx, size, 0.42, vec![]);
    let library = editor
        .detached
        .dock_groups
        .iter()
        .find(|group| {
            group
                .tabs
                .iter()
                .any(|(panel, _)| *panel == PanelId::Library)
        })
        .expect("Library has a dock group")
        .rect;
    let first = named_bounds(&before_library_scroll, "Symbol 000")
        .expect("first symbol remains accessible");
    assert!(
        library.contains_rect(first),
        "first symbol must be visible in Library: {first:?} in {library:?}"
    );
    frame(
        &mut editor,
        &ctx,
        size,
        0.5,
        wheel(first.center(), Vec2::new(0., -10000.)),
    );
    let mut out = frame(&mut editor, &ctx, size, 0.51, vec![]);
    for step in 1..10 {
        out = frame(&mut editor, &ctx, size, 0.51 + step as f64 / 60., vec![]);
    }
    let last = named_bounds(&out, "Symbol 099").expect("last symbol remains accessible");
    assert!(last.bottom() <= 720., "{last:?}");
    assert!(
        library.contains_rect(last),
        "last symbol must be visible in Library: {last:?} in {library:?}"
    );
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
    assert_eq!(
        editor.library_selected.as_deref(),
        Some("fixture-symbol-99")
    );
    assert_eq!(editor.project, before);
    assert!(!editor.dirty());
    let settled = frame(&mut editor, &ctx, size, 1., vec![]);
    let source = named_bounds(&settled, "Symbol 099").unwrap().center();
    // Between the Tools and Library columns, above the bottom dock: the stage.
    let destination = Pos2::new(
        (tools.right() + library.left()) / 2.,
        (tools.top() + library.bottom()) / 2.,
    );
    frame(
        &mut editor,
        &ctx,
        size,
        1.1,
        vec![
            Event::PointerMoved(source),
            primary_button(source, true),
            Event::PointerMoved(destination),
            primary_button(destination, false),
        ],
    );
    assert!(
        matches!(editor.selected_element().map(|element| &element.kind),
        Some(ElementKind::Instance { symbol_id, .. }) if symbol_id == "fixture-symbol-99"),
        "source={source:?} target={destination:?} status={} drag={} selected={:?}",
        editor.status,
        editor.library_panel.drag.is_some(),
        editor.selected_element()
    );
    assert!(
        editor.library_panel.drag.is_none(),
        "Stage consumes the source payload once"
    );
    editor.undo();
    assert_eq!(
        editor.project, before,
        "one Undo removes the single batched drag placement"
    );
    // Preview overflow preserves three list rows and fixed actions in a short
    // split; a taller group restores the same first-frame preview inline.
    let size = Vec2::new(1024., 1000.);
    editor.library_selected = Some("fixture-symbol-99".into());
    for (index, theme) in [ThemeMode::Light, ThemeMode::Dark].into_iter().enumerate() {
        editor.set_panel_visible(PanelId::Properties, true);
        // NF2 starts Library at 100 pt (header + search); the user drags the splitter to give the
        // list room before using the preview overflow.
        for group in &mut editor.workspace.dock.right.groups {
            group.weight = 0.5;
        }
        editor.set_theme(&ctx, theme);
        let time = 2. + index as f64 * 2.;
        // The split changes this frame; hit-testing uses the previous layout,
        // so inspect and click after one settled paint.
        frame(&mut editor, &ctx, size, time, vec![]);
        let out = frame(&mut editor, &ctx, size, time + 0.05, vec![]);
        let library = editor
            .detached
            .dock_groups
            .iter()
            .find(|g| g.tabs.iter().any(|(panel, _)| *panel == PanelId::Library))
            .unwrap()
            .rect;
        for label in [
            "Open symbol preview",
            "Library options",
            "New symbol",
            "New folder",
            "Item properties",
            "Import…",
        ] {
            let rect = named_bounds_in(&out, label, library)
                .unwrap_or_else(|| panic!("missing Library control: {label}"));
            assert!(
                library.contains_rect(rect),
                "{label}: {rect:?} must stay in {library:?}"
            );
        }
        let trigger = named_bounds(&out, "Open symbol preview").unwrap().center();
        frame(
            &mut editor,
            &ctx,
            size,
            time + 0.1,
            vec![
                Event::PointerMoved(trigger),
                primary_button(trigger, true),
                primary_button(trigger, false),
            ],
        );
        let popup = frame(&mut editor, &ctx, size, time + 0.3, vec![]);
        assert!(popup.shapes.iter().any(|shape| matches!(&shape.shape,egui::Shape::Text(text) if text.galley.job.text=="Empty first frame")),"overflow opens the shared first-frame renderer ({theme:?})");
        frame(
            &mut editor,
            &ctx,
            size,
            time + 0.4,
            vec![Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        editor.set_panel_visible(PanelId::Properties, false);
        frame(&mut editor, &ctx, size, time + 0.6, vec![]);
        let out = frame(&mut editor, &ctx, size, time + 0.9, vec![]);
        let library = editor
            .detached
            .dock_groups
            .iter()
            .find(|g| g.tabs.iter().any(|(panel, _)| *panel == PanelId::Library))
            .unwrap()
            .rect;
        assert!(out.shapes.iter().any(|shape| matches!(&shape.shape,egui::Shape::Text(text) if text.galley.job.text=="Empty first frame")),"taller Library restores inline preview");
        for label in [
            "Library options",
            "New symbol",
            "New folder",
            "Item properties",
            "Import…",
        ] {
            let rect = named_bounds_in(&out, label, library)
                .unwrap_or_else(|| panic!("{label} must stay in {library:?}"));
            assert!(
                library.contains_rect(rect),
                "{label}: {rect:?} must stay in {library:?}"
            );
        }
        assert_eq!(editor.project, before);
    }
}

#[test]
fn layer_labels_and_keys_share_exact_rows_after_vertical_scroll() {
    for theme in [ThemeMode::Light, ThemeMode::Dark] {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
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
        let initial = frame(&mut editor, &ctx, Vec2::new(1024., 720.), 0.1, vec![]);
        // Native font metrics and dock headers determine where the rows begin.
        // Scroll over a painted key, rather than a fixed window coordinate that
        // can land on the ruler or header after a legitimate layout change.
        let rows_point = initial
            .shapes
            .iter()
            .filter_map(|shape| key_marker(shape, &ctx))
            .min_by(|a, b| a.y.total_cmp(&b.y))
            .expect("the timeline must paint a visible key before scrolling");
        frame(
            &mut editor,
            &ctx,
            Vec2::new(1024., 720.),
            0.2,
            wheel(rows_point, Vec2::new(0., -260.)),
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
        // NF2 v2.2: keys sit on the glyph line (9 pt above the row bottom); compare row centres.
        let glyph_below_centre = crate::timeline_paint::ROW_HEIGHT / 2.
            - neoflash_ui::components::timeline::GLYPH_LINE_FROM_BOTTOM;
        let keys: Vec<_> = out
            .shapes
            .iter()
            .filter_map(|s| key_marker(s, &ctx).map(|c| c.y - glyph_below_centre))
            .collect();
        let row_top = keys.iter().copied().fold(f32::INFINITY, f32::min)
            - crate::timeline_paint::ROW_HEIGHT / 2.;
        let row_bottom = keys.iter().copied().fold(f32::NEG_INFINITY, f32::max)
            + crate::timeline_paint::ROW_HEIGHT / 2.;
        let mut visible = 0;
        let mut first_row = usize::MAX;
        let mut first_label = None;
        for shape in &out.shapes {
            if let egui::Shape::Text(text) = &shape.shape
                && text.galley.job.text.starts_with("Row ")
                && shape.clip_rect.height() <= crate::timeline_paint::ROW_HEIGHT + 1.
            {
                let y = text.pos.y + text.galley.size().y / 2.;
                if shape.clip_rect.contains(Pos2::new(text.pos.x + 2., y))
                    && text.pos.x < neoflash_ui::metrics::LAYER_COLUMN_MODE_SOLO
                    && y >= row_top
                    && y <= row_bottom
                {
                    visible += 1;
                    let row = text.galley.job.text[4..].parse::<usize>().unwrap();
                    if first_label.is_none() || row > first_row {
                        first_row = row;
                        first_label = Some(Pos2::new(text.pos.x + 2., y));
                    }
                    // NF2/TL Layer: 8 pt inset, 16 pt chevron column, 16 pt type icon, 8 pt gap.
                    assert!(
                        text.pos.x < 72.,
                        "layer names must align beside the type icon"
                    );
                    assert!(
                        keys.iter().any(|key| (*key - y).abs() < 1.),
                        "unaligned label at {y}, keys {keys:?}"
                    );
                }
            }
        }
        assert!(visible >= 2);
        assert!(
            first_row <= 24,
            "wheel must actually scroll shared rows, first={first_row}"
        );
        let from = out
            .shapes
            .iter()
            .filter_map(|shape| key_marker(shape, &ctx))
            .filter(|center| {
                (center.y - glyph_below_centre - row_top - crate::timeline_paint::ROW_HEIGHT / 2.)
                    .abs()
                    < 1.
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

        // Rename through the same visible row used above; committing is a single
        // document edit, while the in-progress field itself changes no project.
        let point = first_label.unwrap();
        for time in [0.8, 0.9] {
            frame(
                &mut editor,
                &ctx,
                Vec2::new(1024., 720.),
                time,
                vec![
                    Event::PointerMoved(point),
                    primary_button(point, true),
                    primary_button(point, false),
                ],
            );
        }
        frame(&mut editor, &ctx, Vec2::new(1024., 720.), 1., vec![]);
        frame(
            &mut editor,
            &ctx,
            Vec2::new(1024., 720.),
            1.1,
            vec![Event::Text("Renamed layer".into())],
        );
        assert_eq!(editor.project, before, "typing a name is still a draft");
        frame(
            &mut editor,
            &ctx,
            Vec2::new(1024., 720.),
            1.2,
            vec![Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert_eq!(
            editor.symbol().timeline.layers[first_row].name,
            "Renamed layer"
        );
        editor.undo();
        assert_eq!(editor.project, before);
        assert!(!editor.history.can_undo());

        // Column sizing is a workspace preference, while layer moves are one
        // document edit; both use actual rendered control geometry.
        let layout = frame(&mut editor, &ctx, Vec2::new(1024., 720.), 1.3, vec![]);
        let divider = named_bounds(&layout, "Resize layer column")
            .expect("layer divider is accessible")
            .center();
        let width = editor.workspace.sizes.layer_column_width;
        frame(
            &mut editor,
            &ctx,
            Vec2::new(1024., 720.),
            1.4,
            vec![
                Event::PointerMoved(divider),
                primary_button(divider, true),
                Event::PointerMoved(divider - Vec2::new(60., 0.)),
                primary_button(divider - Vec2::new(60., 0.), false),
            ],
        );
        // The NF2 column with Mode + Solo starts at 372 (near its 420 maximum), so narrow it.
        assert_eq!(width, neoflash_ui::metrics::LAYER_COLUMN_MODE_SOLO);
        assert!((editor.workspace.sizes.layer_column_width - width + 60.).abs() < 1.);
        assert_eq!(editor.project, before);
        assert!(!editor.history.can_undo());
        let layout = frame(&mut editor, &ctx, Vec2::new(1024., 720.), 1.5, vec![]);
        let row = named_bounds(&layout, &format!("Row {first_row:02} (Layer)"))
            .expect("scrolled layer remains visible")
            .center();
        let moved_id = editor.symbol().timeline.layers[first_row].id.clone();
        let destination = row + Vec2::new(0., crate::timeline_paint::ROW_HEIGHT * 1.1);
        frame(
            &mut editor,
            &ctx,
            Vec2::new(1024., 720.),
            1.6,
            vec![
                Event::PointerMoved(row),
                primary_button(row, true),
                Event::PointerMoved(destination),
                primary_button(destination, false),
            ],
        );
        assert_eq!(editor.symbol().timeline.layers[first_row - 1].id, moved_id);
        editor.undo();
        assert_eq!(editor.project, before);
        assert!(!editor.history.can_undo());

        // A moved folder carries its mask and masked child without rewriting
        // their relationships or losing authored frames.
        editor.project.format_version = 4;
        let ids: Vec<_> = editor
            .symbol()
            .timeline
            .layers
            .iter()
            .take(5)
            .map(|layer| layer.id.clone())
            .collect();
        let layers = &mut editor.symbol_mut().timeline.layers;
        layers[0].properties.role = LayerRole::Folder;
        layers[0].keyframes.clear();
        layers[1].properties.parent = Some(ids[0].clone());
        layers[1].properties.role = LayerRole::Mask;
        for element in layers[1]
            .keyframes
            .iter_mut()
            .flat_map(|key| &mut key.elements)
        {
            element.kind = ElementKind::Rectangle {
                width: 10.,
                height: 10.,
                fill: [0, 0, 0, 255],
                stroke: [0; 4],
                stroke_width: 0.,
            };
        }
        layers[2].properties.parent = Some(ids[0].clone());
        layers[2].properties.mask = Some(ids[1].clone());
        let grouped = editor.project.clone();
        editor.reorder_timeline_layer(&ids[0], Some(&ids[4]));
        assert_eq!(
            editor.symbol().timeline.layers[1..4]
                .iter()
                .map(|layer| layer.id.clone())
                .collect::<Vec<_>>(),
            ids[..3]
        );
        editor.project.validate().unwrap();
        editor.undo();
        assert_eq!(editor.project, grouped);
        assert!(!editor.history.can_undo());

        let rows = crate::layer_controls::visible_layer_indices(&editor.symbol().timeline);
        let folder_row = rows.iter().position(|&i| i == 0).unwrap();
        assert_eq!(&rows[folder_row..folder_row + 3], &[0, 1, 2]);
        let top = editor.symbol().timeline.layers[rows[0]].id.clone();
        editor.reorder_timeline_layer_visual(&ids[0], Some(&top));
        assert_eq!(editor.symbol().timeline.layers.last().unwrap().id, ids[2]);
        assert_eq!(
            crate::layer_controls::visible_layer_indices(&editor.symbol().timeline)[0],
            editor.symbol().timeline.layers.len() - 3
        );
        editor.undo();
        assert_eq!(editor.project, grouped);
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
    editor.reset_active_workspace(&ctx);
    let out = frame(&mut editor, &ctx, Vec2::new(1024., 720.), 0.2, vec![]);
    assert!(named_bounds(&out, "Selection (V)").is_some());
    assert!(
        out.platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .any(|(_, node)| node.role() == egui::accesskit::Role::Tab
                && node.is_selected() == Some(true))
    );
    let tab = editor
        .detached
        .dock_groups
        .iter()
        .flat_map(|group| &group.tabs)
        .find(|(panel, _)| *panel == PanelId::Timeline)
        .unwrap()
        .1;
    assert!(
        out.shapes
            .iter()
            .any(|shape| matches!(&shape.shape, egui::Shape::Rect(rect)
        if rect.fill == ThemeMode::Dark.palette().accent && rect.rect.height() == 2.
            && tab.contains_rect(rect.rect) && shape.clip_rect.contains_rect(rect.rect))),
        "active tab marker must actually be inside the painted clip"
    );
    let defaults = crate::workspace::WorkspacePrefs::default();
    for panel in PanelId::ALL {
        assert_eq!(
            editor.workspace.is_panel_visible(panel),
            defaults.is_panel_visible(panel)
        );
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
                    response = Some(
                        ui.add_enabled_semantic(
                            enabled,
                            neoflash_ui::components::IconButton::new(
                                neoflash_ui::icons::i16::PEN,
                                "Pen (P)",
                            )
                            .selected(true),
                        ),
                    );
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

/// Narrow detached Timeline: More ▸ Hidden controls ▸ Onion skin options… opens the onion popover
/// anchored to the More button (R-052). Returns the frame after the popover opened.
fn open_onion_options_via_more(
    editor: &mut Editor,
    ctx: &egui::Context,
    size: Vec2,
    start: f64,
    output: &egui::FullOutput,
) -> egui::FullOutput {
    let more = named_bounds(output, "Hidden controls and frame view options")
        .expect("narrow bar lists hidden controls in More")
        .center();
    detached_timeline_frame(
        editor,
        ctx,
        size,
        start,
        vec![Event::PointerMoved(more), primary_button(more, true)],
    );
    let mut out = detached_timeline_frame(
        editor,
        ctx,
        size,
        start + 0.05,
        vec![primary_button(more, false)],
    );
    // The menu may be shifted over its own button in a short window: park the pointer outside it.
    let park = Pos2::new(4., size.y - 4.);
    for pass in 1..4 {
        out = detached_timeline_frame(
            editor,
            ctx,
            size,
            start + 0.05 + pass as f64 / 60.,
            vec![Event::PointerMoved(park)],
        );
    }
    if named_bounds(&out, "Onion skin options…").is_none() {
        // Many hidden controls: they are grouped; open the Onion skin submenu.
        // The bar's 32 pt "Onion skin" toggle shares the label; the submenu row is the wide one.
        let group = out
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("accesskit")
            .nodes
            .iter()
            .filter(|(_, node)| node.label() == Some("Onion skin"))
            .filter_map(|(_, node)| node.bounds())
            .map(|r| {
                Rect::from_min_max(
                    Pos2::new(r.x0 as f32, r.y0 as f32),
                    Pos2::new(r.x1 as f32, r.y1 as f32),
                )
            })
            .find(|r| r.width() > 60.)
            .expect("More groups hidden onion controls")
            .center();
        for pass in 0..10 {
            out = detached_timeline_frame(
                editor,
                ctx,
                size,
                start + 0.1 + pass as f64 * 0.05,
                vec![Event::PointerMoved(group)],
            );
        }
    }
    let item = named_bounds(&out, "Onion skin options…")
        .unwrap_or_else(|| {
            let labels: Vec<String> = out
                .platform_output
                .accesskit_update
                .as_ref()
                .map(|u| {
                    u.nodes
                        .iter()
                        .filter_map(|(_, n)| n.label().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            panic!("More lists the hidden onion options: {labels:?}")
        })
        .center();
    detached_timeline_frame(
        editor,
        ctx,
        size,
        start + 0.2,
        vec![Event::PointerMoved(item), primary_button(item, true)],
    );
    out = detached_timeline_frame(
        editor,
        ctx,
        size,
        start + 0.25,
        vec![primary_button(item, false)],
    );
    for pass in 1..4 {
        out = detached_timeline_frame(editor, ctx, size, start + 0.25 + pass as f64 / 60., vec![]);
    }
    out
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
fn detached_timeline_at_750_exposes_compact_onion_options_inside_viewport() {
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
            "Onion skin",
            "Hidden controls and frame view options",
        ] {
            let bounds = named_bounds(&output, label)
                .unwrap_or_else(|| panic!("missing AX control {label}"));
            assert!(
                viewport.contains_rect(bounds),
                "{mode:?} {label}: {bounds:?}"
            );
        }
        output = open_onion_options_via_more(&mut editor, &ctx, size, 0.2, &output);
        for label in ["Outlines", "Anchor", "Onion opacity"] {
            let bounds = named_bounds(&output, label)
                .unwrap_or_else(|| panic!("missing popup control {label}"));
            assert!(
                viewport.contains_rect(bounds),
                "{mode:?} {label}: {bounds:?}"
            );
        }
        assert!(fully_visible_text(&output, "Opacity", viewport));
        assert_eq!(editor.project, before);
        assert!(!editor.history.can_undo());
    }
}

#[test]
fn detached_timeline_minimum_width_opens_working_opacity_control() {
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
        "onion details live in the options popover"
    );
    let output = open_onion_options_via_more(&mut editor, &ctx, size, 0.2, &initial);
    let bounds = named_bounds(&output, "Onion opacity")
        .expect("Opacity is available through the onion options popover");
    assert!(viewport.contains_rect(bounds), "{bounds:?}");
    let opacity = editor.workspace.timeline_prefs.onion_falloff;
    // Click the NF2/Slider rail itself (140 pt wide, 7 pt knob inset).
    let point = Pos2::new(bounds.left() + 72., bounds.center().y);
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
        editor.workspace.timeline_prefs.onion_falloff, opacity,
        "popup slider must accept pointer input"
    );
    assert_eq!(editor.project, before);
    assert!(!editor.dirty());
    assert!(!editor.history.can_undo());
}

#[test]
fn bounded_timeline_footer_preserves_stage_at_minimum_root_window() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut editor = Editor::from_project(&ctx, Project::demo());
    editor.set_theme(&ctx, ThemeMode::Light);
    let size = Vec2::new(1024., 720.);
    let viewport = Rect::from_min_size(Pos2::ZERO, size);
    let render = |editor: &mut Editor, time: f64, events: Vec<Event>| {
        let mut stage = Rect::NOTHING;
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(viewport),
                time: Some(time),
                events,
                ..Default::default()
            },
            |ctx| {
                editor.menu(ctx);
                editor.dock_grid(ctx);
                stage = ctx.available_rect();
                editor.stage(ctx);
            },
        );
        (output, stage)
    };
    let initial_height = editor.workspace.dock.bottom.size;
    for pass in 0..20 {
        let (output, stage) = render(&mut editor, pass as f64 / 60., vec![]);
        assert!(stage.width() >= 600. && stage.height() >= 200., "{stage:?}");
        // The NF2 controls bar keeps transport and the pinned onion group in the window.
        for label in ["Play preview", "Onion skin", "Onion skin options"] {
            let bounds = named_bounds(&output, label).unwrap_or_else(|| panic!("missing {label}"));
            assert!(viewport.contains_rect(bounds), "{label}: {bounds:?}");
        }
        assert_eq!(
            editor.workspace.dock.bottom.size, initial_height,
            "timeline must not grow on repaint"
        );
    }
    // Native input can batch press, movement and release between paints. The
    // saved height must survive that gesture and leave a usable Stage.
    let (output, _) = render(&mut editor, 1., vec![]);
    let grip = named_bounds(&output, "Resize bottom dock").expect("Timeline height grip");
    assert!(grip.height() >= 6.);
    let start = grip.center();
    let end = start + Vec2::new(0., 60.);
    render(
        &mut editor,
        1.1,
        vec![
            Event::PointerMoved(start),
            primary_button(start, true),
            Event::PointerMoved(end),
            primary_button(end, false),
        ],
    );
    assert!((editor.workspace.dock.bottom.size - initial_height + 60.).abs() < 1.);
    let saved_height = editor.workspace.dock.bottom.size;
    let (_, stage) = render(&mut editor, 1.4, vec![]);
    assert_eq!(editor.workspace.dock.bottom.size, saved_height);
    assert!(stage.height() >= 160., "{stage:?}");
    assert!(!editor.dirty());
}

/// Design §8: all main menus open, their live toggles change the editor both ways, and view/workspace
/// changes leave authored content and Undo untouched. Preview/unsupported rows stay distinguishable.
#[test]
fn menu_bar_toggles_live_state_without_changing_the_document() {
    let mut d = Driver::new();
    d.settle();
    let project = d.editor.project.clone();
    let close_menu = |d: &mut Driver| {
        d.step(vec![Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        d.settle();
    };
    // Grid, Guides and Loop playback also exist in the canvas/timeline controls. Resolve the
    // upper menu row explicitly; AccessKit's node order is not the visual stacking order.
    let click_row = |d: &mut Driver, label: &str| {
        let at = d
            .out
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .filter(|(_, node)| {
                node.label() == Some(label) && node.role() != egui::accesskit::Role::Tab
            })
            .filter_map(|(_, node)| node.bounds())
            .min_by(|a, b| a.y0.total_cmp(&b.y0))
            .unwrap_or_else(|| {
                panic!("menu row {label:?} is not shown; visible: {:?}", d.labels())
            });
        d.press(
            Pos2::new(((at.x0 + at.x1) / 2.) as f32, ((at.y0 + at.y1) / 2.) as f32),
            egui::PointerButton::Primary,
        );
    };
    let menus: [(&str, &[&str]); 9] = [
        (
            "File",
            &["Publish settings… (⇧F12)", "Publish (⇧⌥F12)", "Print… (⌘P)"],
        ),
        (
            "Edit",
            &[
                "Find and replace (⌘F)",
                "Preferences… (⌘U)",
                "Edit symbols (⌘E)",
            ],
        ),
        (
            "View",
            &["Rulers, guides & grid…", "Show shape hints (⌥⌘H)"],
        ),
        (
            "Insert",
            &["New symbol… (⌘F8)", "Layer folder", "Motion tween"],
        ),
        ("Modify", &["Break apart (⌘B)", "Ungroup (⇧⌘G)"]),
        (
            "Control",
            &["Go to end (⌥⇧.)", "Test scene (⌥⌘↵)", "Mute sounds (⌥⌘M)"],
        ),
        ("Document", &["Document settings… (⌘J)"]),
        ("Window", &["Panels", "Workspaces", "Appearance"]),
        (
            "Help",
            &["Search functions… (F1)", "Open example", "About NeoFlash"],
        ),
    ];
    for (title, rows) in menus {
        d.click(title);
        for row in rows {
            assert!(d.find(row).is_some(), "{title} ▸ {row}");
        }
        let unavailable: &[&str] = match title {
            "File" => &["Print… (⌘P)"],
            "View" => &["Preview mode", "Snapping", "Pasteboard"],
            "Modify" => &["Symbol"],
            "Window" => &["Duplicate window (⌥⌘K)"],
            _ => &[],
        };
        for label in unavailable {
            let node = d
                .out
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some(*label))
                .unwrap_or_else(|| panic!("missing {title} ▸ {label}"));
            assert!(
                node.1.is_disabled(),
                "{title} ▸ {label} must not promise an available command"
            );
        }
        if title == "Edit" {
            d.click("Find and replace (⌘F)");
            assert!(
                d.editor.find_replace.is_some(),
                "opens the real Find dialog"
            );
            d.click("Close");
            assert!(d.editor.find_replace.is_none());
        } else {
            close_menu(&mut d);
        }
    }

    type Toggle = fn(&Editor) -> bool;
    let toggles: &[(&str, Option<&str>, &str, Toggle)] = &[
        ("View", None, "Rulers (⌥⇧⌘R)", |e| {
            e.stage_view.prefs.rulers
        }),
        ("View", Some("Grid"), "Show grid (⌘')", |e| {
            e.stage_view.prefs.grid_show
        }),
        ("View", Some("Grid"), "Snap to grid (⇧⌘')", |e| {
            e.stage_view.prefs.grid_snap
        }),
        ("View", Some("Grid"), "Show over objects", |e| {
            e.stage_view.prefs.grid_over_objects
        }),
        ("View", Some("Guides"), "Show guides (⌘;)", |e| {
            e.stage_view.prefs.guides_show
        }),
        ("View", Some("Guides"), "Lock guides", |e| {
            e.stage_view.prefs.guides_lock
        }),
        ("View", Some("Guides"), "Snap to guides (⇧⌘;)", |e| {
            e.stage_view.prefs.guides_snap
        }),
        ("View", None, "Show shape hints (⌥⌘H)", |e| {
            e.timeline_shape_hints.show
        }),
        ("Control", None, "Loop playback", |e| {
            e.workspace.timeline.loop_playback
        }),
        ("Control", None, "Play all scenes", |e| {
            e.timeline_edit.play_all_scenes
        }),
        ("Control", None, "Mute sounds (⌥⌘M)", |e| {
            e.timeline_audio.muted
        }),
        ("Control", None, "Enable simple buttons (⌥⌘B)", |e| {
            e.enable_simple_buttons
        }),
        ("Window", None, "Hide panels (F4)", |e| e.panels_hidden),
    ];
    for &(menu, submenu, label, state) in toggles {
        let initial = state(&d.editor);
        for expected in [!initial, initial] {
            d.click(menu);
            if let Some(submenu) = submenu {
                click_row(&mut d, submenu);
            }
            click_row(&mut d, label);
            assert_eq!(state(&d.editor), expected, "{menu} ▸ {label}");
            close_menu(&mut d);
            if label == "Hide panels (F4)" {
                assert_eq!(d.tab(PanelId::Timeline).is_none(), expected);
                assert_eq!(d.find("Selection (V)").is_none(), expected);
            }
        }
    }

    // A representative panel actually disappears and returns, rather than only changing a checkmark.
    assert!(d.tab(PanelId::Timeline).is_some());
    for visible in [false, true] {
        d.click("Window");
        d.click("Panels");
        d.click("Timeline (⌥⌘T)");
        assert_eq!(
            d.editor.workspace.is_panel_visible(PanelId::Timeline),
            visible
        );
        assert_eq!(d.tab(PanelId::Timeline).is_some(), visible);
    }
    let initial_theme = d.editor.workspace.theme;
    for theme in [ThemeMode::Light, ThemeMode::Dark, initial_theme] {
        d.click("Window");
        d.click("Appearance");
        d.click(if theme == ThemeMode::Light {
            "Light"
        } else {
            "Dark"
        });
        assert_eq!(d.editor.workspace.theme, theme);
        assert_eq!(
            neoflash_ui::palette(&d.ctx).bg_app,
            theme.nf2().palette().bg_app
        );
    }
    assert_eq!(d.editor.project, project);
    assert!(!d.editor.dirty());
    assert!(!d.editor.history.can_undo());
}

/// Design §8 stage-object menu opens on right-click over the Stage with its designed rows.
#[test]
fn stage_context_menu_follows_design() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut editor = Editor::from_project(&ctx, Project::demo());
    editor.set_tool(crate::app::Tool::Bucket);
    let size = Vec2::new(1920., 1080.);
    frame(&mut editor, &ctx, size, 0.1, vec![]);
    // The centre of the demo's main Orbit instance, mapped through the live Stage view.
    let at = editor.stage_view.origin + egui::vec2(748. * editor.zoom, 288. * editor.zoom);
    let secondary = |pressed| Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Secondary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    frame(&mut editor, &ctx, size, 0.2, vec![Event::PointerMoved(at)]);
    frame(&mut editor, &ctx, size, 0.3, vec![secondary(true)]);
    frame(&mut editor, &ctx, size, 0.4, vec![secondary(false)]);
    assert!(
        !editor.selection.is_empty(),
        "right-click selects the object even while a paint tool is active"
    );
    assert!(
        editor.selected_instance_symbol().is_some(),
        "the fixture point targets a symbol instance"
    );
    let out = frame(&mut editor, &ctx, size, 0.5, vec![]);
    for row in [
        "Paste in place (⇧⌘V)",
        "Convert to symbol… (F8)",
        "Edit in place",
        "Edit in new window",
        "Swap symbol…",
        "Duplicate symbol…",
        "Create motion tween",
        "Distribute to layers (⇧⌘D)",
        "Show in library",
    ] {
        assert!(named_bounds(&out, row).is_some(), "Stage menu ▸ {row}");
    }
}

/// DL-01 Preferences: Edit ▸ Preferences… opens the dialog; the theme previews live and Cancel restores
/// it; Save applies and persists the General and Timeline settings; Reset to defaults resets the
/// current page; Esc cancels; Keyboard shortcuts… opens the Shortcuts page; the project never changes.
#[test]
fn preferences_dialog_previews_saves_cancels_and_resets() {
    use crate::panels::preferences::{PAGE_SHORTCUTS, PAGE_TIMELINE};
    let mut d = Driver::new();
    let project = d.editor.project.clone();
    d.editor.set_theme(&d.ctx, ThemeMode::Light);
    d.settle();
    let open = |d: &mut Driver| {
        d.click("Edit");
        d.click("Preferences… (⌘U)");
        assert!(d.editor.preferences.is_some(), "Edit ▸ Preferences… opens");
    };

    open(&mut d);
    d.click("Dark");
    assert_eq!(d.ctx.theme(), egui::Theme::Dark, "the theme previews live");
    assert_eq!(d.editor.workspace.theme, ThemeMode::Light, "not saved yet");
    d.click("Cancel");
    assert!(d.editor.preferences.is_none());
    assert_eq!(
        d.ctx.theme(),
        egui::Theme::Light,
        "Cancel restores the theme"
    );

    // Light → Dark with Save, plus Shift-select.
    open(&mut d);
    d.click("Dark");
    d.click("Add with ⇧ (otherwise click adds)");
    d.click("Save");
    assert!(d.editor.preferences.is_none());
    assert_eq!(d.editor.workspace.theme, ThemeMode::Dark);
    assert_eq!(d.ctx.theme(), egui::Theme::Dark);
    assert!(!d.editor.workspace.shift_select);

    // Timeline page: switches, Reset to defaults for the page, then Save.
    open(&mut d);
    d.editor.preferences.as_mut().unwrap().page = PAGE_TIMELINE;
    d.settle();
    d.click("Snap playhead to keyframes");
    d.click("Show seconds");
    let draft = d.editor.preferences.clone().unwrap();
    assert!(draft.timeline.snap_playhead && !draft.timeline.show_seconds);
    d.click("Reset to defaults");
    let draft = d.editor.preferences.clone().unwrap();
    assert!(!draft.timeline.snap_playhead && draft.timeline.show_seconds);
    assert!(!draft.shift_select, "Reset touches only the current page");
    d.click("Snap playhead to keyframes");
    d.click("Save");
    assert!(d.editor.workspace.timeline.snap_playhead);
    assert!(d.editor.workspace.timeline.show_seconds);

    // Esc cancels; Keyboard shortcuts… opens the Shortcuts page.
    open(&mut d);
    d.step(vec![Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }]);
    d.settle();
    assert!(d.editor.preferences.is_none(), "Esc cancels");
    d.click("Edit");
    d.click("Keyboard shortcuts… (⌥⇧⌘K)");
    assert_eq!(
        d.editor.preferences.as_ref().map(|p| p.page),
        Some(PAGE_SHORTCUTS)
    );
    d.click("Cancel");

    // The main Help row, F1 and Timeline ▸ Frame view ▸ Help share the searchable command
    // palette. Help focuses search immediately, shows full menu paths and executes the selected
    // command instead of opening the shortcut editor.
    d.click("Help");
    d.click("Search functions… (F1)");
    assert!(d.editor.preferences.is_none());
    assert!(
        crate::panels::function_search::is_function_search_open(&d.ctx),
        "Help opens the immediate command palette"
    );
    assert!(d.find("Search menu items and functions").is_some());
    d.step(vec![Event::Text("keyframe".into())]);
    d.settle();
    assert!(d.find("Modify > Timeline > Convert to keyframes").is_some());
    d.step(vec![Event::Key {
        key: egui::Key::Escape,
        physical_key: Some(egui::Key::Escape),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }]);
    d.settle();
    assert!(!crate::panels::function_search::is_function_search_open(
        &d.ctx
    ));

    d.step(vec![Event::Key {
        key: egui::Key::F1,
        physical_key: Some(egui::Key::F1),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }]);
    d.settle();
    assert!(crate::panels::function_search::is_function_search_open(
        &d.ctx
    ));
    d.step(vec![Event::Text("select all".into())]);
    d.settle();
    d.click("Edit > Select all");
    assert!(!d.editor.selection.is_empty(), "a search result runs");
    assert!(!crate::panels::function_search::is_function_search_open(
        &d.ctx
    ));

    d.step(vec![Event::Key {
        key: egui::Key::F1,
        physical_key: Some(egui::Key::F1),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }]);
    d.settle();
    d.step(vec![Event::Text("deselect".into())]);
    d.settle();
    d.step(vec![Event::Key {
        key: egui::Key::Enter,
        physical_key: Some(egui::Key::Enter),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }]);
    d.settle();
    assert!(
        d.editor.selection.is_empty(),
        "Enter runs the active result"
    );

    d.click("Frame view options");
    let timeline_help = d
        .out
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .filter_map(|(_, node)| {
            (node.label() == Some("Help"))
                .then(|| node.bounds())
                .flatten()
        })
        .max_by(|a, b| a.y0.total_cmp(&b.y0))
        .expect("Timeline Frame view menu exposes Help");
    d.press(
        Pos2::new(
            ((timeline_help.x0 + timeline_help.x1) / 2.) as f32,
            ((timeline_help.y0 + timeline_help.y1) / 2.) as f32,
        ),
        egui::PointerButton::Primary,
    );
    assert!(d.editor.preferences.is_none());
    assert!(
        crate::panels::function_search::is_function_search_open(&d.ctx),
        "Timeline Help opens the same palette"
    );
    d.step(vec![Event::Key {
        key: egui::Key::Escape,
        physical_key: Some(egui::Key::Escape),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }]);
    d.settle();

    // Undo levels, onion range and UI scale apply on Save (set on the draft directly).
    d.editor.open_preferences(0);
    let mut draft = d.editor.preferences.take().unwrap();
    draft.undo_levels = 20.;
    draft.onion = [5., 2.];
    draft.ui_scale = 1.25;
    d.editor.save_preferences(&d.ctx, &draft);
    assert_eq!(d.editor.history.capacity(), 20);
    assert_eq!((d.editor.onion_before, d.editor.onion_after), (5, 2));
    assert_eq!(d.editor.workspace.ui_scale, 1.25);
    d.settle();
    assert_eq!(
        d.ctx.zoom_factor(),
        1.25,
        "UI scale applies from the next frame"
    );
    let json = serde_json::to_string(&d.editor.workspace).unwrap();
    let restored = crate::workspace::WorkspacePrefs::from_json(&json);
    assert_eq!(restored.undo_levels, 20);
    assert_eq!(restored.onion_range, [5, 2]);
    assert!(!restored.shift_select);
    assert_eq!(d.editor.project, project);
}

/// Whole editor frame in `eframe::App::update` order (without playback/audio/native window glue).
fn full_frame(
    editor: &mut Editor,
    ctx: &egui::Context,
    time: f64,
    events: Vec<Event>,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1920., 1080.))),
            time: Some(time),
            events,
            ..Default::default()
        },
        |ctx| {
            editor.shortcuts(ctx);
            editor.normalize_selection();
            editor.menu(ctx);
            editor.status_bar(ctx);
            editor.bitmap_textures.begin_frame(&editor.project);
            editor.prepare_docked_panel_focus(ctx);
            if editor.has_document() {
                if !editor.panels_hidden {
                    editor.dock_grid(ctx);
                }
                editor.document_bar(ctx);
                editor.stage(ctx);
            } else {
                editor.no_document_view(ctx);
            }
            editor.draw_dialogs(ctx);
            editor.workspace_dialog(ctx);
            editor.customize_tools_dialog(ctx);
            editor.trace_bitmap_dialog(ctx);
            editor.symbol_creation_dialog(ctx);
            if editor.has_document() {
                editor.detached_panels(ctx);
            }
            editor.planned_panels(ctx);
        },
    )
}

/// Drives clicks through [`full_frame`] and finds controls by their accessible name.
struct Driver {
    ctx: egui::Context,
    editor: Editor,
    time: f64,
    out: egui::FullOutput,
}
impl Driver {
    fn new() -> Self {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut editor = Editor::from_project(&ctx, Project::demo());
        let out = full_frame(&mut editor, &ctx, 0.1, vec![]);
        Self {
            ctx,
            editor,
            time: 0.1,
            out,
        }
    }
    fn step(&mut self, events: Vec<Event>) {
        self.time += 0.1;
        self.out = full_frame(&mut self.editor, &self.ctx, self.time, events);
    }
    fn settle(&mut self) {
        self.step(vec![]);
        self.step(vec![]);
    }
    /// Bounds of the control named `label`; dock tabs are skipped (menu rows share panel names),
    /// and non-tab controls (such as open menu items) take precedence over background document tabs.
    fn find(&self, label: &str) -> Option<Rect> {
        let nodes = &self.out.platform_output.accesskit_update.as_ref()?.nodes;
        let to_rect = |r: egui::accesskit::Rect| {
            Rect::from_min_max(
                Pos2::new(r.x0 as f32, r.y0 as f32),
                Pos2::new(r.x1 as f32, r.y1 as f32),
            )
        };
        nodes
            .iter()
            .find_map(|(_, node)| {
                (node.label() == Some(label)
                    && node.role() != egui::accesskit::Role::Tab
                    && node.toggled().is_none())
                .then(|| node.bounds())
                .flatten()
                .map(to_rect)
            })
            .or_else(|| {
                nodes.iter().find_map(|(_, node)| {
                    (node.label() == Some(label) && node.role() != egui::accesskit::Role::Tab)
                        .then(|| node.bounds())
                        .flatten()
                        .map(to_rect)
                })
            })
    }
    fn labels(&self) -> Vec<String> {
        let mut labels: Vec<String> = self
            .out
            .platform_output
            .accesskit_update
            .as_ref()
            .map(|u| {
                u.nodes
                    .iter()
                    .filter_map(|(_, n)| n.label().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        labels.sort();
        labels.dedup();
        labels
    }
    fn texts(&self) -> Vec<String> {
        let mut texts: Vec<String> = self
            .out
            .platform_output
            .accesskit_update
            .as_ref()
            .map(|u| {
                u.nodes
                    .iter()
                    .filter_map(|(_, n)| n.label().or(n.value()).map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        texts.sort();
        texts.dedup();
        texts
    }
    fn press(&mut self, at: Pos2, button: egui::PointerButton) {
        let event = |pressed| Event::PointerButton {
            pos: at,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.step(vec![Event::PointerMoved(at), event(true)]);
        self.step(vec![event(false)]);
        self.settle();
    }
    fn click(&mut self, label: &str) {
        let at = self
            .find(label)
            .unwrap_or_else(|| panic!("{label:?} is not shown; visible: {:?}", self.labels()))
            .center();
        self.press(at, egui::PointerButton::Primary);
    }
    /// Accessible bounds of the dock tab of `panel` (role Tab, so menu rows with the same name do not match).
    fn tab(&self, panel: PanelId) -> Option<Rect> {
        self.out
            .platform_output
            .accesskit_update
            .as_ref()?
            .nodes
            .iter()
            .find_map(|(_, node)| {
                (node.role() == egui::accesskit::Role::Tab && node.label() == Some(panel.title()))
                    .then(|| node.bounds())
                    .flatten()
                    .map(|r| {
                        Rect::from_min_max(
                            Pos2::new(r.x0 as f32, r.y0 as f32),
                            Pos2::new(r.x1 as f32, r.y1 as f32),
                        )
                    })
            })
    }
}

/// Every panel, preview and dialog that can be opened can also be closed again from the UI: Window menu rows
/// toggle real panels both ways, each panel tab's menu hides it, preview chrome closes previews (popovers by
/// clicking outside), and modal dialogs close with their buttons.
#[test]
fn every_panel_preview_and_dialog_opens_and_closes() {
    use crate::panels::planned::{self, Chrome, PlannedPanel};
    let mut d = Driver::new();
    let project = d.editor.project.clone();
    let window_row = |panel: PanelId| match panel {
        PanelId::Timeline => "Timeline (⌥⌘T)",
        PanelId::Tools => "Tools (⌘F2)",
        PanelId::Brushes => "Brushes",
        PanelId::Properties => "Properties (⌘F3)",
        PanelId::Library => "Library (⌘L)",
        PanelId::Actions => "Actions (F9)",
        PanelId::Align => "Align (⌘K)",
        PanelId::Color => "Color (⇧⌘F9)",
        PanelId::Swatches => "Swatches (⌘F9)",
        PanelId::Transform => "Transform (⌘T)",
        PanelId::Info => "Info (⌘I)",
        PanelId::Output => "Output (F2)",
        PanelId::Accessibility => "Accessibility (⇧F11)",
        PanelId::Ease => "Ease",
        PanelId::History => "History (⌘F10)",
        PanelId::Scene => "Scene (⇧F2)",
        PanelId::MotionEditor => "Motion editor",
        PanelId::Project => "Project",
        PanelId::Filters => "Filters",
        PanelId::CompilerErrors => "Compiler errors (⌥F2)",
        PanelId::CodeSnippets => "Code snippets",
        PanelId::MovieExplorer => "Movie Explorer",
    };

    let click_panel_menu = |d: &mut Driver, label: &str| {
        // A panel body can expose a button with the same name (e.g. Filters).
        // Window's panel rows are checkboxes; target that real menu control.
        let node = d
            .out
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .find(|(_, node)| {
                node.role() == egui::accesskit::Role::CheckBox && node.label() == Some(label)
            })
            .unwrap_or_else(|| panic!("Window row {label}"));
        let rect = node.1.bounds().unwrap();
        d.press(
            egui::pos2(
                (rect.x0 + rect.x1) as f32 / 2.,
                (rect.y0 + rect.y1) as f32 / 2.,
            ),
            egui::PointerButton::Primary,
        );
    };
    // Real panels: Window menu opens and closes; the tab menu's Hide closes too.
    for panel in PanelId::ALL {
        d.editor.set_panel_visible(panel, false);
        d.settle();
        d.click("Window");
        d.click("Panels");
        click_panel_menu(&mut d, window_row(panel));
        assert!(
            d.editor.workspace.is_panel_visible(panel),
            "Window ▸ {panel:?} opens"
        );
        assert!(d.tab(panel).is_some(), "{panel:?} tab is drawn");
        d.click("Window");
        d.click("Panels");
        click_panel_menu(&mut d, window_row(panel));
        assert!(
            !d.editor.workspace.is_panel_visible(panel),
            "Window ▸ {panel:?} closes"
        );
        assert!(d.tab(panel).is_none(), "{panel:?} tab disappears");

        d.click("Window");
        d.click("Panels");
        click_panel_menu(&mut d, window_row(panel));
        assert!(d.editor.workspace.is_panel_visible(panel));
        let tab = d.tab(panel).unwrap();
        d.press(tab.center(), egui::PointerButton::Secondary);
        d.click("Hide");
        assert!(
            !d.editor.workspace.is_panel_visible(panel),
            "{panel:?} tab menu ▸ Hide closes"
        );
    }
    d.editor.reset_active_workspace(&d.ctx);
    d.settle();

    // Inactive background tabs and Hide panels: Window menu must open in 1 click, not 2.
    assert!(!d.editor.workspace.is_panel_active(PanelId::Actions));
    d.click("Window");
    d.click("Panels");
    d.click(window_row(PanelId::Actions));
    assert!(
        d.editor.workspace.is_panel_active(PanelId::Actions),
        "inactive background tab opens on first click in Window menu"
    );
    d.editor.set_panels_hidden(true);
    d.settle();
    d.click("Window");
    d.click("Panels");
    d.click(window_row(PanelId::Properties));
    assert!(!d.editor.panels_hidden);
    assert!(
        d.editor.workspace.is_panel_active(PanelId::Properties),
        "panel hidden by Hide panels opens on first click in Window menu"
    );
    d.editor.reset_active_workspace(&d.ctx);
    d.settle();

    // Planned previews: each closes from its own chrome. Failures are collected in one pass.
    let mut stuck = Vec::new();
    for panel in PlannedPanel::ALL {
        planned::set_open(&d.ctx, panel, true);
        d.settle();
        assert!(planned::is_open(&d.ctx, panel));
        let close = match panel.chrome() {
            Chrome::Panel(_) => d.find("Close panel"),
            Chrome::Dialog => d.find("Close"),
            Chrome::Window => d.find("Close preview"),
        };
        match close {
            Some(rect) => d.press(rect.center(), egui::PointerButton::Primary),
            None => stuck.push(format!("{panel:?}: no close control")),
        }
        if planned::is_open(&d.ctx, panel) {
            stuck.push(format!("{panel:?}: still open"));
            planned::set_open(&d.ctx, panel, false);
            d.settle();
        }
    }
    assert!(stuck.is_empty(), "previews that do not close: {stuck:#?}");

    // Modal dialogs.
    d.click("Help");
    d.click("About NeoFlash");
    assert!(d.editor.show_about);
    d.click("Close");
    assert!(!d.editor.show_about, "About closes");

    d.click("Insert");
    d.click("New symbol… (⌘F8)");
    assert!(d.editor.symbol_conversion.is_some(), "New symbol opens");
    d.click("Cancel");
    assert!(
        d.editor.symbol_conversion.is_none(),
        "New symbol closes with Cancel"
    );

    d.click("Document");
    d.click("Document settings… (⌘J)");
    assert!(
        d.editor.document_settings.is_some(),
        "Document settings opens"
    );
    d.click("Cancel");
    assert!(
        d.editor.document_settings.is_none(),
        "Document settings closes with Cancel"
    );

    // Detached panels are native macOS windows here; their system close is handled in
    // `detached_panels` (`close_requested`) and is not reachable from a headless egui test.
    assert_eq!(
        d.editor.project, project,
        "opening and closing changes no document data"
    );

    // Unsaved-changes confirmation closes with Cancel and keeps the work.
    d.editor.transact(|e| e.project.name = "Unsaved".into());
    let edited = d.editor.project.clone();
    d.click("Help");
    d.click("Open example");
    assert!(
        d.editor.confirm.is_none(),
        "Open example inserts a document tab"
    );
    d.editor.switch_session_tab(&d.ctx, 0);
    d.editor.request(crate::app::Pending::CloseDocument, &d.ctx);
    d.settle();
    d.click("Cancel");
    assert!(
        d.editor.confirm.is_none(),
        "confirmation closes with Cancel"
    );
    assert_eq!(d.editor.project, edited);
}

#[test]
fn history_panel_lists_named_steps_jumps_back_and_forward_clears_and_updates_edit_menu() {
    let mut d = Driver::new();
    d.editor = Editor::from_project(&d.ctx, Project::blank());
    d.editor.show_panel(PanelId::History);
    let initial = d.editor.project.clone();

    d.editor.add_element(
        ElementKind::Rectangle {
            width: 80.,
            height: 50.,
            fill: [91, 140, 255, 255],
            stroke: [0, 0, 0, 0],
            stroke_width: 0.,
        },
        20.,
        30.,
    );
    d.editor.transact_named("Move", |s| {
        s.symbol_mut().timeline.layers[0].keyframes[0].elements[0]
            .transform
            .x = 60.;
    });
    d.editor.transact_named("Recolour", |s| {
        if let ElementKind::Rectangle { fill, .. } =
            &mut s.symbol_mut().timeline.layers[0].keyframes[0].elements[0].kind
        {
            *fill = [240, 100, 40, 255];
        }
    });
    let after_three = d.editor.project.clone();
    assert_eq!(
        d.editor.history.undo_labels().collect::<Vec<_>>(),
        vec!["Rectangle", "Move", "Recolour"]
    );
    d.settle();

    // Edit menu shows the descriptive top step label.
    d.click("Edit");
    assert!(
        d.find("Undo Recolour (⌘Z)").is_some(),
        "visible: {:?}",
        d.labels()
    );
    // Close Edit menu by clicking History panel's "Move" step row to jump back to step 2.
    d.click("Edit");
    d.click("Move");
    assert_eq!(d.editor.history.undo_len(), 2);
    assert_eq!(d.editor.history.undo_label(), Some("Move"));
    assert_eq!(d.editor.history.redo_label(), Some("Recolour"));

    d.click("Edit");
    assert!(d.find("Undo Move (⌘Z)").is_some());
    assert!(d.find("Redo Recolour (⇧⌘Z)").is_some());
    d.click("Edit");

    // Jump all the way back to "Open" (step 0) without losing the 3 undone steps.
    d.click("Open");
    assert_eq!(d.editor.history.undo_len(), 0);
    assert_eq!(d.editor.project, initial);
    assert_eq!(d.editor.history.redo_len(), 3);

    // Click the dimmed "Recolour" row to jump forward across all 3 steps.
    d.click("Recolour");
    assert_eq!(d.editor.history.undo_len(), 3);
    assert_eq!(d.editor.project, after_three);
    d.editor.project.validate().unwrap();

    // Clear history preserves the document and empties both stacks.
    d.click("Clear history");
    assert!(!d.editor.history.can_undo());
    assert!(!d.editor.history.can_redo());
    assert_eq!(d.editor.project, after_three);
}

#[test]
fn swatches_panel_scans_document_paints_applies_solid_none_and_gradient_and_persists_swatches() {
    let mut d = Driver::new();
    let mut project = Project::blank();
    project.format_version = 5;
    let grad = GradientPaint {
        kind: GradientKind::Linear,
        stops: vec![
            GradientStop {
                ratio: 0,
                color: [255, 20, 120, 255],
            },
            GradientStop {
                ratio: 255,
                color: [20, 200, 255, 255],
            },
        ],
        matrix: [80.0, 0.0, 0.0, 80.0, 40.0, 40.0],
        focal_point_ratio: 0.0,
    };
    let mut editor = Editor::from_project(&d.ctx, project);
    editor.add_element(
        ElementKind::Rectangle {
            width: 90.,
            height: 60.,
            fill: [18, 52, 86, 255],
            stroke: [12, 34, 56, 255],
            stroke_width: 2.,
        },
        40.,
        40.,
    );
    let rect_selection = editor.selection.clone();
    editor.add_element(
        ElementKind::PaintedPath {
            commands: vec![
                PathCommand::MoveTo { x: 0., y: 0. },
                PathCommand::LineTo { x: 80., y: 0. },
                PathCommand::LineTo { x: 80., y: 80. },
                PathCommand::Close,
            ],
            fill_rule: FillRule::default(),
            fill: [255, 20, 120, 255],
            stroke: [0, 0, 0, 0],
            stroke_width: 0.,
            fill_gradient: Some(grad.clone()),
            stroke_gradient: None,
        },
        160.,
        40.,
    );
    let painted_selection = editor.selection.clone();
    editor.clear_history();
    editor.show_panel(PanelId::Swatches);
    d.editor = editor;
    d.settle();

    let doc_colors = d.editor.document_swatch_colors();
    assert!(doc_colors.contains(&[18, 52, 86, 255]));
    assert!(doc_colors.contains(&[12, 34, 56, 255]));
    assert!(doc_colors.contains(&[255, 20, 120, 255]));
    let palette = neoflash_ui::components::default_palette();
    let doc_grads = d.editor.document_gradient_swatches(&palette);
    assert!(
        doc_grads.len() > 6,
        "includes authored gradient + NF2 presets"
    );

    // Solid swatch application + Undo on the selected rectangle.
    d.editor.selection = rect_selection.clone();
    d.editor.apply_swatch_solid(&d.ctx, [240, 100, 40, 255]);
    assert_eq!(d.editor.history.undo_label(), Some("Fill colour"));
    d.editor.project.validate().unwrap();
    if let ElementKind::Rectangle { fill, .. } =
        &d.editor.symbol().timeline.layers[0].keyframes[0].elements[0].kind
    {
        assert_eq!(*fill, [240, 100, 40, 255]);
    } else {
        panic!("expected rectangle");
    }
    d.editor.undo();
    if let ElementKind::Rectangle { fill, .. } =
        &d.editor.symbol().timeline.layers[0].keyframes[0].elements[0].kind
    {
        assert_eq!(*fill, [18, 52, 86, 255]);
    }

    // No-colour fill & active-stroke handoff with Color panel.
    d.click("No colour");
    assert_eq!(d.editor.history.undo_label(), Some("No fill"));
    d.editor.undo();
    d.ctx.data_mut(|data| {
        data.insert_temp(egui::Id::new("color-panel-active-stroke"), true);
    });
    d.editor.apply_swatch_solid(&d.ctx, [200, 40, 60, 255]);
    assert_eq!(d.editor.history.undo_label(), Some("Stroke colour"));
    d.ctx.data_mut(|data| {
        data.insert_temp(egui::Id::new("color-panel-active-stroke"), false);
    });

    // Gradient swatch application on PaintedPath + Undo.
    d.editor.selection = painted_selection;
    let preset_grad = doc_grads.last().unwrap().1.clone();
    d.editor.apply_swatch_gradient(&d.ctx, &preset_grad);
    assert_eq!(d.editor.history.undo_label(), Some("Fill gradient"));
    d.editor.project.validate().unwrap();
    d.editor.undo();

    // Converted `Add swatch` menu trigger persists custom swatch in WorkspacePrefs.
    d.editor.clear_selection();
    d.editor.fill = [77, 88, 99, 255];
    Editor::open_planned(&d.ctx, "Color menu ▸ Add swatch");
    d.settle();
    assert!(
        d.editor
            .workspace
            .custom_swatches
            .contains(&[77, 88, 99, 255])
    );
}

#[test]
fn info_panel_edits_selection_bounds_and_samples_live_stage_cursor_and_rgba() {
    let mut d = Driver::new();
    let mut project = Project::blank();
    project.background = [20, 30, 40, 255];
    let mut editor = Editor::from_project(&d.ctx, project);
    editor.add_element(
        ElementKind::Rectangle {
            width: 80.,
            height: 60.,
            fill: [91, 140, 255, 255],
            stroke: [0, 0, 0, 0],
            stroke_width: 0.,
        },
        100.,
        150.,
    );
    let sel = editor.selection.clone();
    editor.clear_history();
    editor.show_panel(PanelId::Info);
    d.editor = editor;
    d.editor.selection = sel;
    d.settle();

    let (bounds, editable) = d
        .editor
        .selection_info_bounds(&d.ctx)
        .expect("selected rect");
    assert!(editable);
    assert!((bounds.min.x - 100.).abs() < 0.5 && (bounds.min.y - 150.).abs() < 0.5);
    assert!((bounds.width() - 80.).abs() < 0.5 && (bounds.height() - 60.).abs() < 0.5);

    d.editor.transact_named("Transform", |s| {
        s.apply_info_bounds_edit(bounds, 200.0, 250.0, 160.0, 120.0);
    });
    d.editor.project.validate().unwrap();
    let (moved, _) = d.editor.selection_info_bounds(&d.ctx).unwrap();
    assert!((moved.min.x - 200.).abs() < 0.5 && (moved.min.y - 250.).abs() < 0.5);
    assert!((moved.width() - 160.).abs() < 0.5 && (moved.height() - 120.).abs() < 0.5);
    d.editor.undo();
    let (restored, _) = d.editor.selection_info_bounds(&d.ctx).unwrap();
    assert!((restored.min.x - 100.).abs() < 0.5 && (restored.width() - 80.).abs() < 0.5);

    // Move pointer over painted rectangle on stage: live coordinates and RGBA readout update.
    let zoom = d.editor.zoom.max(0.01);
    let origin = d.editor.stage_view.origin;
    let over_rect = Pos2::new(origin.x + 140.0 * zoom, origin.y + 180.0 * zoom);
    d.step(vec![Event::PointerMoved(over_rect)]);
    d.step(vec![Event::PointerMoved(over_rect)]);
    assert_eq!(
        d.editor
            .sample_stage_color_at(&d.ctx, Pos2::new(140.0, 180.0)),
        [91, 140, 255, 255]
    );
    assert!(
        d.texts().iter().any(|l| l == "R 91  G 140  B 255  A 100%"),
        "visible: {:?}",
        d.texts()
    );
    assert_eq!(
        d.editor
            .sample_stage_color_at(&d.ctx, Pos2::new(10.0, 10.0)),
        [20, 30, 40, 255],
        "bare stage reports document background colour"
    );

    // Off-stage pointer shows "—".
    d.step(vec![Event::PointerMoved(Pos2::new(4.0, 4.0))]);
    d.step(vec![Event::PointerMoved(Pos2::new(4.0, 4.0))]);
    assert!(d.texts().iter().any(|l| l == "—"));
}

#[test]
fn project_panel_manages_folders_sources_actions_handoff_and_status() {
    let mut d = Driver::new();
    d.editor = Editor::from_project(&d.ctx, Project::blank());
    d.editor.set_panel_visible(PanelId::Timeline, false);
    d.editor.set_panel_visible(PanelId::Library, false);
    d.editor.set_panel_visible(PanelId::Actions, false);
    d.editor.show_panel(PanelId::Project);
    d.settle();

    d.click("New folder");
    assert!(d.editor.project_panel.folders.contains("folder"));
    let src_id = d
        .editor
        .project_new_source_file(ScriptLanguage::ActionScript3);
    d.editor.project.validate().unwrap();
    assert_eq!(d.editor.project.sources.len(), 1);
    assert_eq!(d.editor.project.sources[0].path, "folder/Class1.as");
    assert_eq!(d.editor.selected_source.as_deref(), Some(src_id.as_str()));
    assert!(d.editor.workspace.is_panel_visible(PanelId::Actions));
    assert_eq!(d.editor.history.undo_label(), Some("New source file"));

    // Link a symbol to `folder.Class1` and verify Project panel's dirty + linkage status caption.
    let sym_name = "OrbitClip".to_owned();
    d.editor.transact_named("Class linkage", |s| {
        let mut sym = s.project.symbols[0].clone();
        sym.id = new_id("symbol");
        sym.name = sym_name.clone();
        sym.timeline.layers[0].id = new_id("layer");
        sym.timeline.layers[0].keyframes[0].elements.clear();
        sym.class_name = Some("folder.Class1".into());
        s.project.enable_program();
        s.project.symbols.push(sym);
    });
    d.settle();
    let expected_caption =
        format!("● unsaved changes · Class1.as is linked to symbol \"{sym_name}\"");
    assert!(
        d.texts().iter().any(|l| l == &expected_caption),
        "expected {expected_caption:?} in {:?}",
        d.texts()
    );

    // Delete the selected source file and undo.
    assert!(d.editor.project_delete_selected());
    assert!(d.editor.project.sources.is_empty());
    d.editor.project.validate().unwrap();
    d.editor.undo();
    assert_eq!(d.editor.project.sources.len(), 1);
    assert_eq!(d.editor.project.sources[0].path, "folder/Class1.as");
}

#[test]
fn document_settings_and_library_symbol_commands_commit_atomically_and_preserve_validity() {
    use crate::app::Pending;
    use crate::panels::document_settings::DocumentSettingsDraft;
    use crate::stage_view::RulerUnits;

    let mut d = Driver::new();
    let mut project = Project::blank();
    project.width = 500;
    project.height = 400;
    project.fps = 24;
    let mut editor = Editor::from_project(&d.ctx, project);
    editor.add_element(
        ElementKind::Rectangle {
            width: 100.,
            height: 80.,
            fill: [91, 140, 255, 255],
            stroke: [0, 0, 0, 0],
            stroke_width: 0.,
        },
        50.,
        40.,
    );
    editor.clear_history();
    let before_doc = editor.project.clone();
    d.editor = editor;
    d.settle();

    // Open Document settings via Document ▸ Document settings…, click Match contents,
    // Make default, and OK.
    d.click("Document");
    d.click("Document settings… (⌘J)");
    assert!(d.editor.document_settings.is_some());
    d.click("Match contents");
    {
        let draft = d.editor.document_settings.as_mut().unwrap();
        assert!((draft.width - 100.0).abs() < 0.5);
        assert!((draft.height - 80.0).abs() < 0.5);
        draft.fps = 30.0;
        draft.units = RulerUnits::Inches;
    }
    d.click("Make default");
    assert_eq!(d.editor.workspace.document_defaults.unwrap().fps, 30);
    d.click("OK");
    assert!(d.editor.document_settings.is_none());
    assert_eq!(d.editor.project.width, 100);
    assert_eq!(d.editor.project.height, 80);
    assert_eq!(d.editor.project.fps, 30);
    assert_eq!(d.editor.stage_view.prefs.units, RulerUnits::Inches);
    assert_eq!(d.editor.history.undo_label(), Some("Document settings"));
    d.editor.project.validate().unwrap();
    let shifted = &d.editor.symbol().timeline.layers[0].keyframes[0].elements[0];
    assert!(shifted.transform.x.abs() < 1e-3 && shifted.transform.y.abs() < 1e-3);

    // One Undo restores original 500×400 stage and (50, 40) artwork position.
    d.editor.undo();
    assert_eq!(d.editor.project, before_doc);

    // Scale content with stage scales root elements proportionally in one Undo.
    d.editor.commit_document_settings(DocumentSettingsDraft {
        width: 1000.0,
        height: 800.0,
        constrain: true,
        aspect: 1.25,
        scale_content: true,
        anchor: neoflash_ui::components::RegistrationPoint::TopLeft,
        units: RulerUnits::Pixels,
        background: egui::Color32::from_rgb(16, 20, 28),
        fps: 60.0,
        match_shift: None,
    });
    d.editor.project.validate().unwrap();
    let scaled = &d.editor.symbol().timeline.layers[0].keyframes[0].elements[0];
    assert!((scaled.transform.x - 100.0).abs() < 1e-3 && (scaled.transform.y - 80.0).abs() < 1e-3);
    d.editor.undo();
    assert_eq!(d.editor.project, before_doc);

    // `Make default` applies to `File ▸ New`.
    d.editor.perform(Pending::New, &d.ctx);
    assert_eq!(d.editor.project.width, 100);
    assert_eq!(d.editor.project.height, 80);
    assert_eq!(d.editor.project.fps, 30);
    assert_eq!(d.editor.stage_view.prefs.units, RulerUnits::Inches);

    // Library symbol commands: Duplicate, Swap (with playback clamping & cycle rejection), Delete.
    fn push_test_symbol(project: &mut Project, name: &str, kind: SymbolKind, frames: u32) -> Id {
        let mut sym = project.symbols[0].clone();
        sym.id = new_id("symbol");
        sym.name = name.into();
        sym.kind = kind;
        sym.folder.clear();
        sym.timeline.frame_count = frames;
        sym.timeline.layers[0].id = new_id("layer");
        sym.timeline.layers[0].keyframes[0].elements.clear();
        let id = sym.id.clone();
        project.symbols.push(sym);
        id
    }
    let sym_a = push_test_symbol(&mut d.editor.project, "Hero", SymbolKind::MovieClip, 4);
    d.editor.project.symbol_mut(&sym_a).unwrap().class_name = Some("Hero".into());
    d.editor.project.symbol_mut(&sym_a).unwrap().linkage_id = Some("hero_mc".into());
    d.editor.project.format_version = 4;

    let sym_b = push_test_symbol(&mut d.editor.project, "Badge", SymbolKind::Graphic, 2);
    let sym_c = push_test_symbol(&mut d.editor.project, "Spark", SymbolKind::Graphic, 1);

    // Duplicate clears class/linkage and assigns fresh IDs.
    let dup_id = d.editor.duplicate_symbol(&sym_a).expect("duplicated Hero");
    d.editor.project.validate().unwrap();
    let dup = d.editor.project.symbol(&dup_id).unwrap();
    assert_eq!(dup.name, "Hero copy");
    assert!(dup.class_name.is_none() && dup.linkage_id.is_none());
    d.editor.undo();
    assert!(d.editor.project.symbol(&dup_id).is_none());

    // Place an instance of Hero on root with custom transform, instance name, script & accessibility,
    // and place Badge inside Hero (`Hero -> Badge`) and Spark inside Badge (`Badge -> Spark`).
    d.editor.add_element(
        ElementKind::Instance {
            symbol_id: sym_a.clone(),
            playback: InstancePlayback::Independent,
        },
        120.,
        140.,
    );
    let placed_id = {
        let el = &mut d.editor.symbol_mut().timeline.layers[0].keyframes[0].elements[0];
        el.name = "player1".into();
        el.transform.rotation = 35.0;
        el.scripts.push(ObjectScript {
            event: ObjectEvent::EnterFrame,
            script: Script {
                language: ScriptLanguage::ActionScript2,
                source: "trace(1);".into(),
            },
        });
        el.accessibility = Some(Accessibility {
            name: "Player".into(),
            description: String::new(),
            role: AccessibleRole::Button,
            tab_order: 0,
            disabled: false,
        });
        el.id.clone()
    };

    // Enter Hero and place Badge inside Hero (`Hero -> Badge`), then enter Badge and place Spark:
    // swapping inside Hero to Hero (direct cycle) or inside Badge to Hero (indirect cycle) is rejected.
    d.editor.enter_symbol(sym_a.clone());
    d.editor.add_element(
        ElementKind::Instance {
            symbol_id: sym_b.clone(),
            playback: InstancePlayback::Loop,
        },
        10.,
        10.,
    );
    assert!(!d.editor.can_swap_instance_symbol(&sym_a));
    assert!(!d.editor.swap_instance_symbol(&sym_a));
    d.editor.enter_symbol(sym_b.clone());
    d.editor.add_element(
        ElementKind::Instance {
            symbol_id: sym_c,
            playback: InstancePlayback::Loop,
        },
        5.,
        5.,
    );
    assert!(!d.editor.can_swap_instance_symbol(&sym_a));
    assert!(!d.editor.swap_instance_symbol(&sym_a));
    d.editor.leave_to_level(0);

    // On root, select the Hero instance and swap it to Badge (MovieClip -> Graphic).
    d.editor.select_single_id(placed_id.clone());
    assert!(d.editor.can_swap_instance_symbol(&sym_b));
    assert!(d.editor.swap_instance_symbol(&sym_b));
    assert_eq!(d.editor.history.undo_label(), Some("Swap symbol"));
    d.editor.project.validate().unwrap();
    let swapped = d.editor.symbol().timeline.layers[0].keyframes[0]
        .elements
        .iter()
        .find(|e| e.id == placed_id)
        .unwrap();
    assert_eq!(swapped.name, "player1");
    assert_eq!(swapped.transform.rotation, 35.0);
    assert_eq!(swapped.scripts.len(), 1);
    assert!(swapped.accessibility.is_some());
    assert!(matches!(
        &swapped.kind,
        ElementKind::Instance {
            symbol_id,
            playback: InstancePlayback::Loop
        } if symbol_id == &sym_b
    ));
    d.editor.undo();
    let unswapped = d.editor.symbol().timeline.layers[0].keyframes[0]
        .elements
        .iter()
        .find(|e| e.id == placed_id)
        .unwrap();
    assert!(matches!(
        &unswapped.kind,
        ElementKind::Instance { symbol_id, .. } if symbol_id == &sym_a
    ));

    // Delete symbol refuses root/scene symbols, exits active editing breadcrumb, removes instances, and undoes.
    let root_id = d.editor.project.root.clone();
    assert!(!d.editor.delete_symbol(&root_id));
    d.editor.enter_symbol_isolated(sym_a.clone());
    let before_delete = d.editor.project.clone();
    assert!(d.editor.delete_symbol(&sym_a));
    assert_eq!(d.editor.editing, vec![root_id]);
    assert!(d.editor.project.symbol(&sym_a).is_none());
    assert!(
        !d.editor.symbol().timeline.layers[0].keyframes[0]
            .elements
            .iter()
            .any(|e| e.id == placed_id)
    );
    d.editor.project.validate().unwrap();
    d.editor.undo();
    assert_eq!(d.editor.project, before_delete);
}

/// Preferences that change editing defaults: Timeline ▸ Default ease drives new classic tweens,
/// Text ▸ Default size drives the Text tool, Clipboard ▸ Default paste = In place keeps positions.
#[test]
fn preference_defaults_reach_tweens_text_and_paste() {
    let mut d = Driver::new();
    // Default ease.
    d.editor.workspace.default_ease = Easing::Linear;
    d.editor.layer = 0;
    let layer = d.editor.symbol().timeline.layers[0].id.clone();
    d.editor.frame = 0;
    d.editor.transact(|e| {
        let l = &mut e.symbol_mut().timeline.layers[0];
        let mut key = l.keyframes[0].clone();
        key.frame = 10;
        key.tween = None;
        l.keyframes.retain(|k| k.frame != 10);
        l.keyframes.push(key);
        l.keyframes.sort_by_key(|k| k.frame);
    });
    d.editor.frame = 0;
    d.editor.create_tween(TweenKind::Classic);
    let tween = d.editor.symbol().timeline.layers[0].keyframes[0].tween;
    assert_eq!(
        tween,
        Some(Easing::Linear),
        "new classic tween on {layer} uses the preferred ease"
    );

    // Default paste in place.
    let (_, element) = d
        .editor
        .sampled_targets()
        .into_iter()
        .next()
        .unwrap_or_else(|| {
            d.editor.select_all();
            d.editor.sampled_targets().into_iter().next().unwrap()
        });
    let text = ClipboardPayload::capture(
        &d.editor.project,
        vec![element.clone()],
        &d.editor.clipboard.session,
        [element.transform.x + 500., element.transform.y + 500.],
    )
    .and_then(|payload| payload.to_text())
    .unwrap();
    d.editor.workspace.paste_in_place = true;
    d.editor.paste_objects(&d.ctx, &text, false);
    let pasted = d.editor.selected_element().unwrap().transform;
    assert_eq!(
        (pasted.x, pasted.y),
        (element.transform.x, element.transform.y)
    );

    // Default text size.
    d.editor.workspace.text_size = 48.;
    d.editor.set_tool(crate::app::Tool::Text);
    d.editor.clear_selection();
    d.settle();
    d.press(Pos2::new(800., 400.), egui::PointerButton::Primary);
    let text_size = d.editor.selected_element().and_then(|e| match &e.kind {
        ElementKind::Text { font_size, .. } => Some(*font_size),
        _ => None,
    });
    assert_eq!(text_size, Some(48.));
}

#[test]
fn preferences_general_autosave_on_launch_open_recent_and_recovery_dialog_work_through_ui() {
    use crate::workspace::OnLaunch;

    let temp_root =
        std::env::temp_dir().join(format!("neoflash-panel-recovery-ui-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_root);
    let docs_dir = temp_root.join("docs");
    let rec_dir = temp_root.join("recovery");
    std::fs::create_dir_all(&docs_dir).unwrap();
    std::fs::create_dir_all(&rec_dir).unwrap();
    let docs_dir = std::fs::canonicalize(&docs_dir).unwrap();
    let rec_dir = std::fs::canonicalize(&rec_dir).unwrap();

    let mut d = Driver::new();
    d.editor = Editor::from_project(&d.ctx, Project::blank());
    d.editor.recovery.set_dir(Some(rec_dir.clone()));
    d.settle();

    // Save two projects so `recent_files` records `[Beta.nflash, Alpha.nflash]`.
    let alpha_path = docs_dir.join("Alpha.nflash");
    let beta_path = docs_dir.join("Beta.nflash");
    d.editor.project.name = "AlphaProject".into();
    assert!(d.editor.save_to_path(alpha_path.clone()));
    d.editor.project.name = "BetaProject".into();
    assert!(d.editor.save_to_path(beta_path.clone()));
    assert_eq!(d.editor.workspace.general.existing_recent_files().len(), 2);

    // Preferences ▸ General: wire On launch select + Autosave switch & minutes stepper.
    d.click("Edit");
    d.click("Preferences… (⌘U)");
    assert!(d.editor.preferences.is_some());
    assert!(d.find("Autosave minutes").is_some());
    d.click("Example");
    d.click("Last document");
    assert_eq!(
        d.editor.preferences.as_ref().unwrap().general.on_launch,
        OnLaunch::LastDocument
    );
    d.click("Every");
    assert!(!d.editor.preferences.as_ref().unwrap().general.autosave);
    d.editor
        .preferences
        .as_mut()
        .unwrap()
        .general
        .autosave_minutes = 12;
    d.settle();

    // `Reset to defaults` restores General defaults while preserving `recent_files`.
    d.click("Reset to defaults");
    {
        let draft = d.editor.preferences.as_ref().unwrap();
        assert_eq!(draft.general.on_launch, OnLaunch::Example);
        assert!(draft.general.autosave);
        assert_eq!(draft.general.autosave_minutes, 5);
        assert_eq!(draft.general.recent_files.len(), 2);
    }

    // Re-select `New document` and toggle autosave minutes to 2, then Save.
    d.click("Example");
    d.click("New document");
    d.editor
        .preferences
        .as_mut()
        .unwrap()
        .general
        .autosave_minutes = 2;
    d.click("Save");
    assert!(d.editor.preferences.is_none());
    assert_eq!(d.editor.workspace.general.on_launch, OnLaunch::NewDocument);
    assert_eq!(d.editor.workspace.general.autosave_minutes, 2);

    // File ▸ Open recent opens `Alpha.nflash` when clicked.
    d.click("File");
    let open_recent = d
        .find("Open recent")
        .expect("Open recent submenu row")
        .center();
    d.step(vec![Event::PointerMoved(open_recent)]);
    d.settle();
    d.click("Alpha.nflash");
    assert_eq!(d.editor.project.name, "AlphaProject");
    assert_eq!(d.editor.path.as_deref(), Some(alpha_path.as_path()));
    assert!(!d.editor.dirty());

    // Dirty `Alpha` + autosave snapshot -> File ▸ Open recent ▸ `Beta.nflash` -> Discard deletes snapshot.
    d.editor
        .transact(|e| e.project.name = "AlphaUnsaved".into());
    d.editor.tick_autosave(10.0, 1_700_020_000);
    d.editor.tick_autosave(130.0, 1_700_020_120);
    assert_eq!(std::fs::read_dir(&rec_dir).unwrap().count(), 2);
    d.click("File");
    let open_recent = d.find("Open recent").unwrap().center();
    d.step(vec![Event::PointerMoved(open_recent)]);
    d.settle();
    d.click("Beta.nflash");
    assert!(
        d.editor.confirm.is_none(),
        "Open recent preserves the dirty tab"
    );
    assert_eq!(d.editor.project.name, "BetaProject");
    assert_eq!(
        std::fs::read_dir(&rec_dir).unwrap().count(),
        2,
        "opening another document preserves the inactive recovery snapshot"
    );
    d.editor.request_close_documents(&d.ctx, true);
    d.settle();
    d.click("Discard");
    assert_eq!(std::fs::read_dir(&rec_dir).unwrap().count(), 0);

    // Seed two crash-recovery snapshots and test Restore and Discard buttons in `Recover unsaved work?`.
    let mut crashed_newer = Project::blank();
    crashed_newer.name = "RecoveredAlpha".into();
    std::fs::write(
        rec_dir.join("snap-a.nflash"),
        crashed_newer.to_json().unwrap(),
    )
    .unwrap();
    std::fs::write(
        rec_dir.join("snap-a.json"),
        serde_json::to_string(&crate::recovery::RecoverySidecar {
            original_path: Some(alpha_path.clone()),
            document_name: "Alpha.nflash".into(),
            timestamp_unix_secs: 1_700_030_500,
        })
        .unwrap(),
    )
    .unwrap();

    let mut crashed_older = Project::blank();
    crashed_older.name = "RecoveredUntitled".into();
    std::fs::write(
        rec_dir.join("snap-b.nflash"),
        crashed_older.to_json().unwrap(),
    )
    .unwrap();
    std::fs::write(
        rec_dir.join("snap-b.json"),
        serde_json::to_string(&crate::recovery::RecoverySidecar {
            original_path: None,
            document_name: "Untitled".into(),
            timestamp_unix_secs: 1_700_030_100,
        })
        .unwrap(),
    )
    .unwrap();

    d.editor.check_recovery_on_launch();
    d.settle();
    assert!(
        d.texts().iter().any(|t| t == "Recover unsaved work?"),
        "recovery dialog visible: {:?}",
        d.texts()
    );

    // Click Restore on the newer snapshot (`Alpha.nflash`).
    d.click("Restore");
    assert_eq!(d.editor.project.name, "RecoveredAlpha");
    assert!(d.editor.dirty());
    assert_eq!(d.editor.path.as_deref(), Some(alpha_path.as_path()));

    // The next copy is immediately accessible while the first recovered tab stays intact.
    assert!(d.texts().iter().any(|t| t == "Recover unsaved work?"));
    assert!(d.editor.recovery.has_prompt());
    assert!(rec_dir.join("snap-b.nflash").exists());
    // Saving the restored Alpha cleans up only snap-a and then exposes the next prompt.
    assert!(d.editor.save(false));
    assert!(!rec_dir.join("snap-a.nflash").exists());
    assert!(!rec_dir.join("snap-a.json").exists());
    d.settle();
    assert!(d.texts().iter().any(|t| t == "Recover unsaved work?"));
    d.click("Discard");
    assert!(!d.editor.recovery.has_prompt());
    assert!(!rec_dir.join("snap-b.nflash").exists());
    assert!(!rec_dir.join("snap-b.json").exists());
    assert_eq!(d.editor.project.name, "RecoveredAlpha");
    assert!(!d.editor.dirty());

    let _ = std::fs::remove_dir_all(&temp_root);
}

#[test]
fn code_editor_preferences_wire_ui_editing_hints_layout_and_format_on_save() {
    use crate::panels::preferences::PAGE_CODE_EDITOR;
    use crate::workspace::{CodeEditorPrefs, WorkspacePrefs};

    let mut d = Driver::new();
    d.editor = Editor::from_project(&d.ctx, Project::blank());
    let untouched_project = d.editor.project.clone();
    d.settle();

    // Open Edit ▸ Preferences… and switch to the Code editor page via the category tree.
    d.click("Edit");
    d.click("Preferences… (⌘U)");
    d.click("Code editor");
    assert_eq!(
        d.editor.preferences.as_ref().map(|p| p.page),
        Some(PAGE_CODE_EDITOR)
    );

    // Colour scheme controls remain disabled while Font and Editing controls are active.
    let node_disabled = |out: &egui::FullOutput, label: &str| {
        out.platform_output.accesskit_update.as_ref().and_then(|u| {
            u.nodes
                .iter()
                .find(|(_, n)| n.label() == Some(label))
                .map(|(_, n)| n.is_disabled())
        })
    };
    assert_eq!(node_disabled(&d.out, "Line numbers"), Some(false));
    assert_eq!(node_disabled(&d.out, "Font size"), Some(false));
    assert_eq!(node_disabled(&d.out, "Line height"), Some(false));
    assert_eq!(node_disabled(&d.out, "Code hint delay"), Some(false));
    assert_eq!(node_disabled(&d.out, "Duplicate"), Some(true));
    assert_eq!(node_disabled(&d.out, "Import…"), Some(true));

    // Toggle every switch and pick Tab: 2 spaces through the real dialog UI.
    d.click("Line numbers");
    d.click("Word wrap");
    d.click("Auto-close brackets");
    d.click("Code hints");
    d.click("Format on save");
    d.click("Highlight current line");
    d.click("Tab: 4 spaces");
    d.click("Tab: 2 spaces");
    {
        let ce = &d.editor.preferences.as_ref().unwrap().code_editor;
        assert!(!ce.line_numbers);
        assert!(ce.word_wrap);
        assert!(!ce.auto_close_brackets);
        assert!(!ce.code_hints);
        assert!(!ce.format_on_save);
        assert!(!ce.highlight_current_line);
        assert_eq!(ce.tab_size, 2);
    }

    // Reset to defaults restores CodeEditorPrefs::default() without closing the dialog.
    d.click("Reset to defaults");
    assert_eq!(
        d.editor.preferences.as_ref().unwrap().code_editor,
        CodeEditorPrefs::default()
    );

    // Configure custom settings and click Save; verify WorkspacePrefs persistence & clean project.
    d.click("Tab: 4 spaces");
    d.click("Tab: 2 spaces");
    d.click("Line numbers");
    d.click("Highlight current line");
    {
        let ce = &mut d.editor.preferences.as_mut().unwrap().code_editor;
        ce.font_size = 18.0;
        ce.line_height = 2.0;
        ce.hint_delay_ms = 300;
    }
    d.click("Save");
    assert!(d.editor.preferences.is_none());
    assert_eq!(d.editor.project, untouched_project);
    assert_eq!(d.editor.workspace.code_editor.tab_size, 2);
    assert!(!d.editor.workspace.code_editor.line_numbers);
    assert!(!d.editor.workspace.code_editor.highlight_current_line);
    assert_eq!(d.editor.workspace.code_editor.font_size, 18.0);
    assert_eq!(d.editor.workspace.code_editor.line_height, 2.0);
    let restored = WorkspacePrefs::from_json(&serde_json::to_string(&d.editor.workspace).unwrap());
    assert_eq!(restored.code_editor, d.editor.workspace.code_editor);

    // Layout checks: font_size/line_height scale row height, and word_wrap toggles wrapping.
    let long_line = "var orbitVelocity = 1234567890 + 1234567890 + 1234567890 + 1234567890;";
    let mut wrap_off = d.editor.workspace.code_editor.clone();
    wrap_off.word_wrap = false;
    let mut wrap_on = wrap_off.clone();
    wrap_on.word_wrap = true;
    let default_ce = CodeEditorPrefs::default();
    let mut rows_default = (0usize, 0.0f32);
    let mut rows_large_unwrapped = (0usize, 0.0f32);
    let mut rows_large_wrapped = (0usize, 0.0f32);
    let _ = d.ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let g0 = crate::sources::code_layout_with_prefs(ui, &long_line, 180.0, &default_ce);
            let g1 = crate::sources::code_layout_with_prefs(ui, &long_line, 180.0, &wrap_off);
            let g2 = crate::sources::code_layout_with_prefs(ui, &long_line, 180.0, &wrap_on);
            rows_default = (g0.rows.len(), g0.rows[0].rect().height());
            rows_large_unwrapped = (g1.rows.len(), g1.rows[0].rect().height());
            rows_large_wrapped = (g2.rows.len(), g2.rows[0].rect().height());
        });
    });
    assert!(
        rows_large_unwrapped.1 > rows_default.1 + 8.0,
        "18px * 2.0 row height ({}) exceeds 13px * 1.6 ({})",
        rows_large_unwrapped.1,
        rows_default.1
    );
    assert_eq!(rows_large_unwrapped.0, 1, "word_wrap=false never wraps");
    assert!(
        rows_large_wrapped.0 > 1,
        "word_wrap=true wraps at wrap_width"
    );

    // Open Actions on an AS3 frame script to verify highlight_current_line, Tab, auto-close, and hints.
    d.editor.project.symbols[0]
        .timeline
        .scripts
        .push(FrameScript {
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript3,
                source: String::new(),
            },
        });
    d.editor.show_panel(PanelId::Actions);
    d.settle();
    let editor_id = d.editor.actions_editor_id();
    let focus_and_caret = |d: &mut Driver, pos: usize| {
        let mut state = egui::text_edit::TextEditState::load(&d.ctx, editor_id).unwrap_or_default();
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::one(
                egui::text::CCursor::new(pos),
            )));
        state.store(&d.ctx, editor_id);
        d.ctx.memory_mut(|m| m.request_focus(editor_id));
        d.step(vec![]);
    };
    let has_current_line_shape = |out: &egui::FullOutput, ctx: &egui::Context| {
        let target = neoflash_ui::palette(ctx).syntax_current_line;
        out.shapes.iter().any(|cs| match &cs.shape {
            egui::Shape::Rect(r) => r.fill == target && r.rect.width() > 100.0,
            _ => false,
        })
    };
    focus_and_caret(&mut d, 0);
    d.step(vec![]);
    assert!(
        !has_current_line_shape(&d.out, &d.ctx),
        "highlight_current_line=false omits current-line band"
    );
    d.editor.workspace.code_editor.highlight_current_line = true;
    focus_and_caret(&mut d, 0);
    d.step(vec![]);
    assert!(
        has_current_line_shape(&d.out, &d.ctx),
        "highlight_current_line=true paints current-line band"
    );

    // Line numbers off uses the narrow 24 pt marker-only gutter; turning it back on widens to >= 48 pt.
    let bg_panel = neoflash_ui::palette(&d.ctx).bg_panel;
    let has_gutter_width = |out: &egui::FullOutput, expected_min: f32, expected_max: f32| {
        out.shapes.iter().any(|cs| match &cs.shape {
            egui::Shape::Rect(r) => {
                r.fill == bg_panel
                    && r.rect.height() >= 60.0
                    && (expected_min..=expected_max).contains(&r.rect.width())
            }
            _ => false,
        })
    };
    assert!(
        has_gutter_width(&d.out, 23.5, 24.5),
        "line_numbers=false paints 24 pt marker gutter"
    );
    d.editor.workspace.code_editor.line_numbers = true;
    d.step(vec![]);
    assert!(
        has_gutter_width(&d.out, 48.0, 80.0),
        "line_numbers=true paints full line-number gutter"
    );

    // Tab inserts 2 spaces (`tab_size = 2`) and Shift+Tab removes them.
    focus_and_caret(&mut d, 0);
    d.step(vec![Event::Key {
        key: egui::Key::Tab,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }]);
    assert_eq!(d.editor.symbol().timeline.scripts[0].script.source, "  ");
    d.step(vec![Event::Key {
        key: egui::Key::Tab,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::SHIFT,
    }]);
    assert_eq!(d.editor.symbol().timeline.scripts[0].script.source, "");

    // Auto-close brackets: `(` -> `()`, `)` skips over closing `)`, Backspace inside `()` deletes both.
    focus_and_caret(&mut d, 0);
    d.step(vec![Event::Text("(".into())]);
    assert_eq!(d.editor.symbol().timeline.scripts[0].script.source, "()");
    let caret_after_open = egui::text_edit::TextEditState::load(&d.ctx, editor_id)
        .unwrap()
        .cursor
        .char_range()
        .unwrap()
        .primary
        .index;
    assert_eq!(caret_after_open, 1);
    d.step(vec![Event::Text(")".into())]);
    assert_eq!(d.editor.symbol().timeline.scripts[0].script.source, "()");
    let caret_after_skip = egui::text_edit::TextEditState::load(&d.ctx, editor_id)
        .unwrap()
        .cursor
        .char_range()
        .unwrap()
        .primary
        .index;
    assert_eq!(caret_after_skip, 2);
    focus_and_caret(&mut d, 1);
    d.step(vec![Event::Key {
        key: egui::Key::Backspace,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }]);
    assert_eq!(d.editor.symbol().timeline.scripts[0].script.source, "");
    d.editor.workspace.code_editor.auto_close_brackets = false;
    focus_and_caret(&mut d, 0);
    d.step(vec![Event::Text("(".into())]);
    assert_eq!(d.editor.symbol().timeline.scripts[0].script.source, "(");
    d.editor.workspace.code_editor.auto_close_brackets = true;

    // Code hint delay (300 ms): typing `this.` waits for the delay before opening the hint popover.
    d.editor.project.symbols[0].timeline.scripts[0]
        .script
        .source = "this".into();
    d.settle();
    focus_and_caret(&mut d, 4);
    d.step(vec![Event::Text(".".into())]);
    assert_eq!(d.editor.symbol().timeline.scripts[0].script.source, "this.");
    assert!(
        !d.labels().iter().any(|l| l == "gotoAndPlay()"),
        "hint popover must wait for hint_delay_ms (300 ms)"
    );
    // Advance +0.1s (total 100 ms < 300 ms): still waiting.
    d.step(vec![]);
    assert!(!d.labels().iter().any(|l| l == "gotoAndPlay()"));
    // Advance past 300 ms total: popover opens automatically.
    d.time += 0.25;
    d.step(vec![]);
    assert!(
        d.labels().iter().any(|l| l == "gotoAndPlay()"),
        "hint popover opens after hint_delay_ms: {:?}",
        d.labels()
    );

    // When `code_hints = false`, automatic `.` hints do not open, but ⌃Space still opens immediately.
    d.editor.workspace.code_editor.code_hints = false;
    d.editor.project.symbols[0].timeline.scripts[0]
        .script
        .source = "this".into();
    d.settle();
    focus_and_caret(&mut d, 4);
    d.step(vec![Event::Text(".".into())]);
    d.time += 0.5;
    d.step(vec![]);
    assert!(
        !d.labels().iter().any(|l| l == "gotoAndPlay()"),
        "automatic hints suppressed when code_hints=false"
    );
    d.step(vec![Event::Key {
        key: egui::Key::Space,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::CTRL,
    }]);
    d.step(vec![]);
    assert!(
        d.labels().iter().any(|l| l == "gotoAndPlay()"),
        "⌃Space opens hints on demand even when code_hints=false"
    );

    // Format on save: formats frame scripts, object scripts and project source units with tab_size (2)
    // without creating extra Undo entries.
    d.editor.project.symbols[0].timeline.scripts[0]
        .script
        .source = "function f() {\nstop();\n}".into();
    d.editor.add_element(
        ElementKind::Rectangle {
            width: 40.,
            height: 40.,
            fill: [91, 140, 255, 255],
            stroke: [0, 0, 0, 0],
            stroke_width: 0.,
        },
        10.,
        10.,
    );
    d.editor.symbol_mut().timeline.layers[0].keyframes[0].elements[0]
        .scripts
        .push(ObjectScript {
            event: ObjectEvent::Click,
            script: Script {
                language: ScriptLanguage::ActionScript2,
                source: "if (ok) {\nplay();\n}".into(),
            },
        });
    d.editor.project.enable_program();
    d.editor.project.sources.push(SourceUnit {
        id: new_id("source"),
        path: "Main.as".into(),
        language: ScriptLanguage::ActionScript3,
        source: "package {\npublic class Main {\n}\n}".into(),
    });
    let undo_before_save = d.editor.history.undo_len();
    let temp_path = std::env::temp_dir().join(format!(
        "neoflash-code-prefs-test-{}.nflash",
        std::process::id()
    ));

    d.editor.workspace.code_editor.format_on_save = false;
    assert!(d.editor.save_to_path(temp_path.clone()));
    assert_eq!(
        d.editor.symbol().timeline.scripts[0].script.source,
        "function f() {\nstop();\n}"
    );

    let unformatted = d.editor.project.clone();
    d.editor.workspace.code_editor.format_on_save = true;
    d.editor.workspace.code_editor.tab_size = 2;
    assert!(d.editor.save_to_path(temp_path.clone()));
    let _ = std::fs::remove_file(&temp_path);
    assert_eq!(d.editor.history.undo_len(), undo_before_save + 1);
    assert!(!d.editor.dirty());
    assert_eq!(
        d.editor.symbol().timeline.scripts[0].script.source,
        "function f() {\n  stop();\n}"
    );
    assert_eq!(
        d.editor.symbol().timeline.layers[0].keyframes[0].elements[0].scripts[0]
            .script
            .source,
        "if (ok) {\n  play();\n}"
    );
    assert_eq!(
        d.editor.project.sources[0].source,
        "package {\n  public class Main {\n  }\n}"
    );
    let saved = d.editor.project.clone();
    d.editor.undo();
    assert_eq!(
        d.editor.project, unformatted,
        "successful save formatting is one Undo"
    );
    assert!(d.editor.dirty());
    d.editor.redo();
    assert_eq!(d.editor.project, saved);
    assert!(!d.editor.dirty());
}

#[test]
fn filters_panel_edits_display_properties_and_filters_with_undo_and_format_7() {
    use crate::panels::filters::AuthoringFilterKind;
    let mut d = Driver::new();
    d.editor.select_all();
    d.editor.selection.targets.truncate(1);
    assert_eq!(d.editor.selection.len(), 1);
    d.editor.show_panel(PanelId::Filters);
    d.settle();

    let baseline = d.editor.project.clone();

    assert!(d.editor.set_selected_visible(false));
    assert!(!d.editor.selected_element().unwrap().visible);
    assert_eq!(d.editor.project.format_version, 7);
    d.editor.project.validate().unwrap();

    assert!(d.editor.set_selected_cache_as_bitmap(true));
    assert!(d.editor.selected_element().unwrap().cache_as_bitmap);
    d.editor.project.validate().unwrap();

    assert!(d.editor.set_selected_blend_mode(BlendMode::Multiply));
    assert_eq!(
        d.editor.selected_element().unwrap().blend_mode,
        BlendMode::Multiply
    );
    d.editor.project.validate().unwrap();

    assert_eq!(
        d.editor
            .add_selected_filter(AuthoringFilterKind::DropShadow),
        Some(0)
    );
    assert_eq!(
        d.editor
            .add_selected_filter(AuthoringFilterKind::AdjustColor),
        Some(1)
    );
    assert!(d.editor.update_selected_filter(
        1,
        FilterSpec::ColorMatrix {
            matrix: adjust_color_matrix(20.0, 15.0, -40.0, 30.0),
        },
        None,
    ));
    assert_eq!(d.editor.duplicate_selected_filter(0), Some(1));
    assert_eq!(d.editor.selected_element().unwrap().filters.len(), 3);
    d.settle();
    d.editor.project.validate().unwrap();

    assert!(d.editor.remove_selected_filter(1));
    assert_eq!(d.editor.selected_element().unwrap().filters.len(), 2);
    d.settle();
    d.editor.project.validate().unwrap();

    // Undo all 8 steps restores the exact baseline project.
    for _ in 0..8 {
        d.editor.undo();
    }
    assert_eq!(d.editor.project, baseline);

    // Held-frame edit inserts a keyframe automatically and leaves frame 0 untouched.
    let target = d.editor.selection.single().unwrap().clone();
    let layer_idx = d
        .editor
        .symbol()
        .timeline
        .layers
        .iter()
        .position(|l| l.id == target.layer)
        .unwrap();
    d.editor.layer = layer_idx;
    d.editor.frame = 4;
    d.editor.insert_frame();
    d.editor.frame = 2;
    d.editor.selection.targets = vec![target.clone()];
    assert!(d.editor.set_selected_blend_mode(BlendMode::Screen));
    d.editor.project.validate().unwrap();
    let layer = &d.editor.symbol().timeline.layers[layer_idx];
    let kf0 = layer.keyframes.iter().find(|k| k.frame == 0).unwrap();
    let kf2 = layer.keyframes.iter().find(|k| k.frame == 2).unwrap();
    assert_eq!(
        kf0.elements
            .iter()
            .find(|e| e.id == target.element)
            .unwrap()
            .blend_mode,
        BlendMode::Normal
    );
    assert_eq!(
        kf2.elements
            .iter()
            .find(|e| e.id == target.element)
            .unwrap()
            .blend_mode,
        BlendMode::Screen
    );
}

#[test]
fn preferences_drawing_page_binds_controls_resets_and_saves() {
    use crate::panels::preferences::PAGE_DRAWING;
    use crate::workspace::{DrawingPrefs, Tolerance};

    let mut d = Driver::new();
    let project_before = d.editor.project.clone();
    d.editor.stage_view.prefs.pixels_snap = false;
    d.editor.open_preferences(PAGE_DRAWING);
    d.settle();

    let is_disabled = |d: &Driver, label: &str| {
        d.out
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .find(|(_, n)| n.label() == Some(label))
            .unwrap_or_else(|| panic!("missing node {label:?}"))
            .1
            .is_disabled()
    };

    // Unwired engine/design-blocked controls stay disabled; wired controls are interactive.
    assert!(is_disabled(&d, "Show solid points"));
    assert!(is_disabled(&d, "Snap align"));
    for enabled_label in [
        "Merge drawing",
        "Object drawing",
        "Show pen preview",
        "Precise cursors",
        "Snap tolerance",
        "Snap to pixels",
    ] {
        assert!(
            !is_disabled(&d, enabled_label),
            "{enabled_label} must be enabled"
        );
    }

    // Toggle switches through the real dialog UI.
    d.click("Merge drawing");
    d.click("Show pen preview");
    d.click("Precise cursors");
    d.click("Snap to pixels");
    {
        let draft = d.editor.preferences.as_ref().unwrap();
        assert!(!draft.drawing.object_drawing);
        assert!(!draft.drawing.pen_preview);
        assert!(draft.drawing.precise_cursors);
        assert!(draft.pixels_snap);
    }

    // Reset to defaults restores DrawingPrefs::default() and pixels_snap = false.
    d.click("Reset to defaults");
    {
        let draft = d.editor.preferences.as_ref().unwrap();
        assert_eq!(draft.drawing, DrawingPrefs::default());
        assert!(!draft.pixels_snap);
    }

    // Edit controls again (switches + recognition tolerances + snap tolerance) and Save.
    d.click("Merge drawing");
    d.click("Show pen preview");
    d.click("Precise cursors");
    d.click("Snap to pixels");
    {
        let draft = d.editor.preferences.as_mut().unwrap();
        draft.drawing.connect_lines = Tolerance::CanBeDistant;
        draft.drawing.smooth_curves = Tolerance::MustBeClose;
        draft.drawing.recognise_lines = Tolerance::MustBeClose;
        draft.drawing.recognise_shapes = Tolerance::Off;
        draft.drawing.click_accuracy = Tolerance::CanBeDistant;
        draft.drawing.snap_tolerance = 18.0;
    }
    d.settle();
    d.click("Save");

    assert!(d.editor.preferences.is_none());
    assert!(!d.editor.workspace.drawing.object_drawing);
    assert!(!d.editor.workspace.drawing.pen_preview);
    assert!(d.editor.workspace.drawing.precise_cursors);
    assert_eq!(
        d.editor.workspace.drawing.connect_lines,
        Tolerance::CanBeDistant
    );
    assert_eq!(
        d.editor.workspace.drawing.smooth_curves,
        Tolerance::MustBeClose
    );
    assert_eq!(
        d.editor.workspace.drawing.recognise_lines,
        Tolerance::MustBeClose
    );
    assert_eq!(d.editor.workspace.drawing.recognise_shapes, Tolerance::Off);
    assert_eq!(
        d.editor.workspace.drawing.click_accuracy,
        Tolerance::CanBeDistant
    );
    assert_eq!(d.editor.workspace.drawing.snap_tolerance, 18.0);
    assert!(d.editor.stage_view.prefs.pixels_snap);
    assert_eq!(
        d.editor
            .workspace
            .view_defaults
            .as_ref()
            .map(|v| v.pixels_snap),
        Some(true)
    );
    assert_eq!(
        d.editor.project, project_before,
        "preferences never mutate the document"
    );
}

#[test]
fn preferences_shortcuts_records_detects_conflicts_manages_sets_and_rebinds_dispatch() {
    let mut d = Driver::new();
    d.editor = Editor::from_project(&d.ctx, Project::blank());
    d.settle();

    // Open directly on the Shortcuts page via Edit ▸ Keyboard shortcuts… (⌥⇧⌘K).
    d.click("Edit");
    d.click("Keyboard shortcuts… (⌥⇧⌘K)");
    assert_eq!(
        d.editor.preferences.as_ref().map(|p| p.page),
        Some(crate::panels::preferences::PAGE_SHORTCUTS)
    );

    // Search for "keyframe" so Insert ▸ Keyframe ("Keyframe") and Convert to keyframes are visible together.
    d.click("Search commands");
    d.step(vec![Event::Text("keyframe".into())]);
    d.settle();
    assert!(d.find("Keyframe").is_some());
    assert!(d.find("Convert to keyframes").is_some());

    let painted_text = |d: &Driver, needle: &str| -> bool {
        fn has_text(shapes: &[egui::epaint::ClippedShape], needle: &str) -> bool {
            shapes.iter().any(|cs| match &cs.shape {
                egui::Shape::Text(t) => t.galley.job.text == needle,
                egui::Shape::Vec(v) => v
                    .iter()
                    .any(|s| matches!(s, egui::Shape::Text(t) if t.galley.job.text == needle)),
                _ => false,
            })
        }
        has_text(&d.out.shapes, needle)
    };

    // Clicking a row enters recording ("Press keys…"); Esc cancels recording without closing Preferences.
    d.click("Convert to keyframes");
    assert!(
        d.find("Recording shortcut for Convert to keyframes")
            .is_some()
    );
    assert!(painted_text(&d, "Press keys…"));
    d.step(vec![Event::Key {
        key: egui::Key::Escape,
        physical_key: Some(egui::Key::Escape),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }]);
    d.settle();
    assert!(
        d.editor.preferences.is_some(),
        "Esc cancels recording instead of closing Preferences"
    );
    assert!(
        d.find("Recording shortcut for Convert to keyframes")
            .is_none()
    );

    // Recording F6 on Convert to keyframes shows the conflict strip with "Go to Insert ▸ Keyframe"
    // (which jumps to Keyframe's recorder; Cancel exits it), and clicking Accept keeps both and flags
    // both rows with "Also on …" plus the "1 conflict" badge.
    d.click("Convert to keyframes");
    d.step(vec![Event::Key {
        key: egui::Key::F6,
        physical_key: Some(egui::Key::F6),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }]);
    d.settle();
    assert!(d.find("Go to Insert ▸ Keyframe").is_some());
    d.click("Go to Insert ▸ Keyframe");
    let rec_rect = d.find("Recording shortcut for Keyframe").unwrap();
    d.press(
        Pos2::new(rec_rect.right() - 28.0, rec_rect.center().y),
        egui::PointerButton::Primary,
    );
    assert!(d.find("Recording shortcut for Keyframe").is_none());

    d.click("Convert to keyframes");
    d.step(vec![Event::Key {
        key: egui::Key::F6,
        physical_key: Some(egui::Key::F6),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }]);
    d.settle();
    d.click("Accept");
    assert!(d.find("1 conflict").is_some());
    assert!(painted_text(&d, "Also on Insert ▸ Keyframe"));
    assert!(painted_text(&d, "Also on Convert to keyframes"));
    // Clicking the "1 conflict" badge filters to only the conflicting commands; clicking again restores all matches.
    d.click("1 conflict");
    assert!(d.find("Blank keyframe").is_none());
    assert!(d.find("Keyframe").is_some());
    assert!(d.find("Convert to keyframes").is_some());
    d.click("1 conflict");
    assert!(d.find("Blank keyframe").is_some());

    // Backspace while recording clears Insert ▸ Keyframe and resolves the conflict; Reset command restores Convert to keyframes.
    d.click("Keyframe");
    d.step(vec![Event::Key {
        key: egui::Key::Backspace,
        physical_key: Some(egui::Key::Backspace),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }]);
    d.settle();
    assert!(d.find("1 conflict").is_none());
    assert!(!painted_text(&d, "Also on Insert ▸ Keyframe"));

    d.click("Convert to keyframes");
    d.click("Reset command");
    assert!(
        d.find("Recording shortcut for Convert to keyframes")
            .is_none()
    );

    // Bind Insert ▸ Keyframe to ⌥F6.
    d.click("Keyframe");
    assert!(d.find("Recording shortcut for Keyframe").is_some());
    d.step(vec![Event::Key {
        key: egui::Key::F6,
        physical_key: Some(egui::Key::F6),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::ALT,
    }]);
    d.settle();
    d.click("Accept");
    assert_eq!(
        d.editor
            .preferences
            .as_ref()
            .unwrap()
            .shortcuts
            .overrides
            .get("insert.keyframe"),
        Some(&vec!["⌥F6".to_owned()])
    );

    // Duplicate creates a named user set, switching sets swaps overrides, and Delete removes the active copy.
    d.click("Duplicate");
    assert!(d.find("NeoFlash default copy").is_some());
    d.click("NeoFlash default copy");
    d.click("NeoFlash default");
    assert!(
        d.editor
            .preferences
            .as_ref()
            .unwrap()
            .shortcuts
            .is_default_set()
    );
    d.click("NeoFlash default");
    d.click("NeoFlash default copy");
    d.click("Duplicate");
    assert!(d.find("NeoFlash default copy copy").is_some());
    d.click("Delete");
    assert!(d.find("NeoFlash default").is_some());
    d.click("NeoFlash default");
    d.click("NeoFlash default copy");

    // Clear search via the NF2/Search clear ✕ button, toggle Only assigned, search "pencil", and rebind Tools ▸ Pencil from Y to ⇧Y.
    let search_rect = d.find("Search commands").unwrap();
    d.press(
        Pos2::new(search_rect.right() - 20.0, search_rect.center().y),
        egui::PointerButton::Primary,
    );
    d.click("Only assigned");
    assert!(d.find("Revert").is_none());
    d.click("Only assigned");

    d.click("Search commands");
    d.step(vec![Event::Text("pencil".into())]);
    d.settle();
    assert!(d.find("Frame").is_none());
    d.click("Pencil");
    d.step(vec![Event::Key {
        key: egui::Key::Y,
        physical_key: Some(egui::Key::Y),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::SHIFT,
    }]);
    d.settle();
    d.click("Accept");

    // Save Preferences and verify live shortcut dispatch + dynamic menu/tool labels.
    d.click("Save");
    assert!(d.editor.preferences.is_none());
    assert_eq!(d.editor.workspace.shortcuts.set, "NeoFlash default copy");

    // Extend timeline to 5 frames, step to frame 3: old F6 does nothing, new ⌥F6 inserts a keyframe.
    d.editor.symbol_mut().timeline.frame_count = 5;
    d.editor.frame = 3;
    d.step(vec![Event::Key {
        key: egui::Key::F6,
        physical_key: Some(egui::Key::F6),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }]);
    d.settle();
    assert_eq!(d.editor.symbol().timeline.layers[0].keyframes.len(), 1);

    d.step(vec![Event::Key {
        key: egui::Key::F6,
        physical_key: Some(egui::Key::F6),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::ALT,
    }]);
    d.settle();
    assert_eq!(d.editor.symbol().timeline.layers[0].keyframes.len(), 2);

    // Tool shortcut: old Y does nothing, new ⇧Y selects Pencil.
    assert_eq!(d.editor.tool, crate::app::Tool::Select);
    d.step(vec![Event::Key {
        key: egui::Key::Y,
        physical_key: Some(egui::Key::Y),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }]);
    d.settle();
    assert_eq!(d.editor.tool, crate::app::Tool::Select);

    d.step(vec![Event::Key {
        key: egui::Key::Y,
        physical_key: Some(egui::Key::Y),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::SHIFT,
    }]);
    d.settle();
    assert_eq!(d.editor.tool, crate::app::Tool::Pencil);

    // Dynamic shortcut strings update in the Tools rail and application menus.
    assert!(d.find("Pencil (⇧Y)").is_some());
    d.click("Insert");
    assert!(d.find("Keyframe (⌥F6)").is_some());
}

#[test]
fn preferences_text_clipboard_and_publish_ui_and_behaviour() {
    let mut d = Driver::new();
    let temp_root = std::env::temp_dir().join(format!(
        "neoflash-prefs-publish-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let runtime_dir = temp_root.join("runtime");
    std::fs::create_dir_all(&runtime_dir).unwrap();
    std::fs::write(runtime_dir.join("neoflash_web.js"), "var NeoFlashWasm;").unwrap();
    std::fs::write(runtime_dir.join("neoflash_web_bg.wasm"), b"\0asm\x01\0\0\0").unwrap();
    let publish_dir = temp_root.join("publish-out");

    // 1. Exercise real Preferences UI controls on Text, Clipboard, and Publish pages via Driver.
    d.click("Edit");
    d.click("Preferences… (⌘U)");
    assert!(d.editor.preferences.is_some());

    // Text ▸ Font substitution select + Reset to defaults.
    d.click("Text");
    d.click("Show warning");
    d.click("Substitute silently");
    assert!(
        !d.editor
            .preferences
            .as_ref()
            .unwrap()
            .text_prefs
            .substitution_warning
    );
    d.click("Reset to defaults");
    assert!(
        d.editor
            .preferences
            .as_ref()
            .unwrap()
            .text_prefs
            .substitution_warning
    );
    d.click("Show warning");
    d.click("Substitute silently");

    // Clipboard ▸ Import vector paste as editable paths switch + Reset to defaults.
    d.click("Clipboard");
    d.click("Import vector paste as editable paths");
    assert!(
        !d.editor
            .preferences
            .as_ref()
            .unwrap()
            .clipboard_prefs
            .editable_vector_paste
    );
    d.click("Reset to defaults");
    assert!(
        d.editor
            .preferences
            .as_ref()
            .unwrap()
            .clipboard_prefs
            .editable_vector_paste
    );
    d.click("Import vector paste as editable paths");

    // Publish ▸ Output folder text field, Browse… button presence, switches, Reset to defaults, Save.
    // ("Publish" also exists in the top bar and Project panel behind the modal, so match the sidebar column.)
    let sidebar_x = d.find("Clipboard").expect("Clipboard category").min.x;
    let publish_nav = d
        .out
        .platform_output
        .accesskit_update
        .as_ref()
        .and_then(|u| {
            u.nodes.iter().find_map(|(_, node)| {
                let r = node.bounds()?;
                (node.label() == Some("Publish") && (r.x0 as f32 - sidebar_x).abs() < 4.0).then(
                    || {
                        Rect::from_min_max(
                            Pos2::new(r.x0 as f32, r.y0 as f32),
                            Pos2::new(r.x1 as f32, r.y1 as f32),
                        )
                        .center()
                    },
                )
            })
        })
        .expect("Preferences Publish category");
    d.press(publish_nav, egui::PointerButton::Primary);
    assert_eq!(
        d.editor.preferences.as_ref().map(|p| p.page),
        Some(crate::panels::preferences::PAGE_PUBLISH)
    );
    assert!(d.find("Browse…").is_some());
    let find_text_input = |driver: &Driver, label: &str| -> Pos2 {
        driver
            .out
            .platform_output
            .accesskit_update
            .as_ref()
            .and_then(|u| {
                u.nodes.iter().find_map(|(_, node)| {
                    (node.label() == Some(label) && node.role() == egui::accesskit::Role::TextInput)
                        .then(|| node.bounds())
                        .flatten()
                        .map(|r| {
                            Rect::from_min_max(
                                Pos2::new(r.x0 as f32, r.y0 as f32),
                                Pos2::new(r.x1 as f32, r.y1 as f32),
                            )
                            .center()
                        })
                })
            })
            .expect("Output folder TextInput node")
    };
    let field_center = find_text_input(&d, "Output folder");
    d.press(field_center, egui::PointerButton::Primary);
    d.step(vec![Event::Text(
        publish_dir.to_string_lossy().into_owned(),
    )]);
    d.settle();
    d.click("Treat warnings as errors");
    d.click("Publish on save");
    {
        let draft = d.editor.preferences.as_ref().unwrap();
        assert_eq!(
            draft.publish.output_folder.as_deref(),
            Some(publish_dir.to_string_lossy().as_ref())
        );
        assert!(draft.publish.warnings_as_errors);
        assert!(draft.publish.publish_on_save);
    }

    // Reset to defaults on Publish resets Publish only (Text and Clipboard draft edits remain).
    d.click("Reset to defaults");
    {
        let draft = d.editor.preferences.as_ref().unwrap();
        assert_eq!(draft.publish.output_folder, None);
        assert!(!draft.publish.warnings_as_errors);
        assert!(!draft.publish.publish_on_save);
        assert!(!draft.text_prefs.substitution_warning);
        assert!(!draft.clipboard_prefs.editable_vector_paste);
    }

    // Re-enter Publish settings and Save.
    let field_center = find_text_input(&d, "Output folder");
    d.press(field_center, egui::PointerButton::Primary);
    d.step(vec![Event::Text(
        publish_dir.to_string_lossy().into_owned(),
    )]);
    d.settle();
    d.click("Treat warnings as errors");
    d.click("Publish on save");
    d.click("Save");
    assert!(d.editor.preferences.is_none());
    assert!(!d.editor.workspace.text_prefs.substitution_warning);
    assert!(!d.editor.workspace.clipboard_prefs.editable_vector_paste);
    assert_eq!(
        d.editor.configured_publish_folder().as_deref(),
        Some(publish_dir.as_path())
    );
    assert!(d.editor.workspace.publish.warnings_as_errors);
    assert!(d.editor.workspace.publish.publish_on_save);

    // 2. Font substitution warning suppression (`TextPrefs::substitution_warning`).
    d.editor.perform(crate::app::Pending::New, &d.ctx);
    d.editor.add_element(
        ElementKind::TextBox {
            text: "Hello".into(),
            width: 120.,
            height: 40.,
            multiline: false,
            word_wrap: false,
            format: TextFormat {
                font: "NeoFlashMissingFontXYZ_999".into(),
                ..Default::default()
            },
        },
        20.,
        20.,
    );
    d.editor.output.clear();
    d.editor.workspace.text_prefs.substitution_warning = false;
    d.editor.load_project_fonts(&d.ctx);
    assert!(
        d.editor.output.is_empty(),
        "Substitute silently suppresses missing-font status/output warnings"
    );
    d.editor.workspace.text_prefs.substitution_warning = true;
    d.editor.load_project_fonts(&d.ctx);
    assert!(
        d.editor
            .output
            .iter()
            .any(|msg| msg.contains("NeoFlashMissingFontXYZ_999")),
        "Show warning logs missing-font warning"
    );

    // 3. `warnings_as_errors` blocks HTML5 publish before writing any files when warnings exist.
    assert!(!neoflash_export::export_warnings(&d.editor.project).is_empty());
    assert!(!d.editor.publish_html5_to(&publish_dir, &runtime_dir));
    assert!(!publish_dir.exists(), "blocked publish writes no files");
    assert!(d.editor.status.starts_with("Publish failed:"));
    assert!(d.editor.status.contains("NeoFlashMissingFontXYZ_999"));

    // 4. Configured output folder reuse (two publishes into the same folder succeed via marker)
    // and non-empty unrelated folder refusal.
    d.editor.workspace.publish.warnings_as_errors = false;
    d.editor.perform(crate::app::Pending::New, &d.ctx);
    d.editor.project.name = "First Publish".into();
    assert!(d.editor.publish_html5_to(&publish_dir, &runtime_dir));
    assert!(publish_dir.join("neoflash-export.json").is_file());
    assert!(
        std::fs::read_to_string(publish_dir.join("index.html"))
            .unwrap()
            .contains("First Publish")
    );
    d.editor.project.name = "Second Publish".into();
    assert!(
        d.editor.publish_html5_to(&publish_dir, &runtime_dir),
        "second publish reuses marker-owned output_folder"
    );
    assert!(
        std::fs::read_to_string(publish_dir.join("index.html"))
            .unwrap()
            .contains("Second Publish")
    );

    let unrelated_dir = temp_root.join("unrelated-non-empty");
    std::fs::create_dir_all(&unrelated_dir).unwrap();
    std::fs::write(unrelated_dir.join("keep.txt"), "do not clobber").unwrap();
    assert!(!d.editor.publish_html5_to(&unrelated_dir, &runtime_dir));
    assert!(d.editor.status.starts_with("Export failed:"));
    assert_eq!(
        std::fs::read_to_string(unrelated_dir.join("keep.txt")).unwrap(),
        "do not clobber"
    );

    // 5. `publish_on_save`: saving `.nflash` publishes to configured output_folder (not for `.fla`),
    // and reminds when output_folder is unset.
    let on_save_dir = temp_root.join("on-save-out");
    d.editor.workspace.publish.output_folder = Some(on_save_dir.to_string_lossy().into_owned());
    d.editor.workspace.publish.publish_on_save = true;
    let fla_path = temp_root.join("archive.fla");
    assert!(d.editor.save_to_path_with_runtime(fla_path, &runtime_dir));
    assert!(
        !on_save_dir.exists(),
        ".fla export does not trigger .nflash publish_on_save"
    );
    let nflash_path = temp_root.join("movie.nflash");
    assert!(
        d.editor
            .save_to_path_with_runtime(nflash_path.clone(), &runtime_dir)
    );
    assert!(on_save_dir.join("index.html").is_file());

    d.editor.workspace.publish.output_folder = None;
    d.editor.output.clear();
    assert!(
        d.editor
            .save_to_path_with_runtime(nflash_path, &runtime_dir)
    );
    assert!(
        d.editor
            .output
            .iter()
            .any(|msg| msg.contains("Preferences ▸ Publish")),
        "missing output_folder logs reminder on save"
    );

    let _ = std::fs::remove_dir_all(temp_root);
}

#[test]
fn timeline_preferences_page_wires_controls_live_falloff_caption_and_persists() {
    use crate::panels::preferences::PAGE_TIMELINE;
    use crate::workspace::{TweenOnDrag, WorkspacePrefs};

    let mut d = Driver::new();
    let project = d.editor.project.clone();

    // Session onion popover and Preferences share the single persisted onion_falloff state.
    d.editor.workspace.timeline_prefs.onion_falloff = 45.0;
    d.editor.open_preferences(PAGE_TIMELINE);
    d.settle();
    assert!(
        d.texts().iter().any(|t| t == "45 → 15 %"),
        "caption reflects live onion_falloff: {:?}",
        d.texts()
    );

    // Toggle switches, move Opacity falloff slider, and pick Tween type on drag.
    d.click("Span-based selection");
    d.click("Create keyframe on transform");
    let slider = d
        .find("Opacity falloff")
        .expect("Opacity falloff slider is enabled on Timeline page");
    let target = Pos2::new(slider.right() - 12.0, slider.center().y);
    d.step(vec![
        Event::PointerMoved(target),
        primary_button(target, true),
    ]);
    d.step(vec![primary_button(target, false)]);
    d.settle();
    let updated_falloff = d
        .editor
        .preferences
        .as_ref()
        .unwrap()
        .timeline_prefs
        .onion_falloff;
    assert!(
        updated_falloff > 90.0,
        "slider updated draft falloff: {updated_falloff}"
    );
    let expected_caption = format!("{updated_falloff:.0} → 15 %");
    assert!(
        d.texts().iter().any(|t| t == &expected_caption),
        "live caption updated to {expected_caption}"
    );

    let default_ease_select = d
        .find("Ease In-Out")
        .expect("Default ease select is shown above Tween type on drag");
    let tween_select = d
        .out
        .platform_output
        .accesskit_update
        .as_ref()
        .and_then(|u| {
            u.nodes.iter().find_map(|(_, node)| {
                let r = node.bounds()?;
                (node.label() == Some("None")
                    && (r.x0 as f32 - default_ease_select.left()).abs() < 8.0
                    && r.y0 as f32 > default_ease_select.bottom())
                .then(|| {
                    Rect::from_min_max(
                        Pos2::new(r.x0 as f32, r.y0 as f32),
                        Pos2::new(r.x1 as f32, r.y1 as f32),
                    )
                })
            })
        })
        .expect("Tween type on drag select shows None");
    d.press(tween_select.center(), egui::PointerButton::Primary);
    d.click("Classic tween");
    d.click("Save");

    assert!(d.editor.preferences.is_none());
    assert!(d.editor.workspace.timeline_prefs.span_selection);
    assert!(!d.editor.workspace.timeline_prefs.auto_keyframe);
    assert_eq!(
        d.editor.workspace.timeline_prefs.tween_on_drag,
        TweenOnDrag::Classic
    );
    assert_eq!(
        d.editor.workspace.timeline_prefs.onion_falloff,
        updated_falloff
    );

    let json = serde_json::to_string(&d.editor.workspace).unwrap();
    let restored = WorkspacePrefs::from_json(&json);
    assert_eq!(restored.timeline_prefs, d.editor.workspace.timeline_prefs);

    // Reset to defaults on Timeline page restores defaults without touching document history.
    d.editor.open_preferences(PAGE_TIMELINE);
    d.settle();
    d.click("Reset to defaults");
    d.click("Save");
    assert!(!d.editor.workspace.timeline_prefs.span_selection);
    assert!(d.editor.workspace.timeline_prefs.auto_keyframe);
    assert_eq!(
        d.editor.workspace.timeline_prefs.tween_on_drag,
        TweenOnDrag::None
    );
    assert_eq!(d.editor.workspace.timeline_prefs.onion_falloff, 70.0);
    assert_eq!(d.editor.project, project);
}

#[test]
fn distribute_to_layers_names_layers_preserves_painter_order_and_undoes() {
    let ctx = egui::Context::default();
    let mut project = Project::blank();
    let btn_sym_id = new_id("symbol");
    project.symbols.push(Symbol {
        id: btn_sym_id.clone(),
        name: "PlayButton".into(),
        kind: SymbolKind::Button,
        folder: String::new(),
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 4,
            guides: vec![],
            scripts: vec![],
            layers: vec![Layer {
                id: new_id("layer"),
                name: "Layer 1".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![Element {
                        id: new_id("object"),
                        name: "rect_1".into(),
                        transform: Transform::default(),
                        kind: ElementKind::Rectangle {
                            width: 80.,
                            height: 30.,
                            fill: [200, 60, 60, 255],
                            stroke: [0, 0, 0, 0],
                            stroke_width: 0.,
                        },
                        ..Element::default()
                    }],
                    ..Default::default()
                }],
            }],
        },
    });
    let el_unnamed_inst = Element {
        id: new_id("object"),
        name: "clip_1".into(),
        transform: Transform {
            x: 10.,
            y: 20.,
            ..Default::default()
        },
        kind: ElementKind::Instance {
            symbol_id: btn_sym_id.clone(),
            playback: InstancePlayback::Independent,
        },
        ..Element::default()
    };
    let el_named_inst = Element {
        id: new_id("object"),
        name: "hero_mc".into(),
        transform: Transform {
            x: 120.,
            y: 20.,
            ..Default::default()
        },
        kind: ElementKind::Instance {
            symbol_id: btn_sym_id,
            playback: InstancePlayback::Independent,
        },
        ..Element::default()
    };
    let el_text = Element {
        id: new_id("object"),
        name: "text_3".into(),
        transform: Transform {
            x: 10.,
            y: 90.,
            ..Default::default()
        },
        kind: ElementKind::Text {
            text: "Welcome Banner".into(),
            font_family: "sans-serif".into(),
            font_size: 20.,
            color: [255, 255, 255, 255],
        },
        ..Element::default()
    };
    let grp_a_id = new_id("object");
    let grp_b_id = new_id("object");
    let el_grp_a = Element {
        id: grp_a_id.clone(),
        name: "rectangle_4".into(),
        transform: Transform {
            x: 10.,
            y: 150.,
            ..Default::default()
        },
        kind: ElementKind::Rectangle {
            width: 30.,
            height: 30.,
            fill: [40, 120, 220, 255],
            stroke: [0, 0, 0, 0],
            stroke_width: 0.,
        },
        ..Element::default()
    };
    let el_grp_b = Element {
        id: grp_b_id.clone(),
        name: "oval_5".into(),
        transform: Transform {
            x: 50.,
            y: 150.,
            ..Default::default()
        },
        kind: ElementKind::Ellipse {
            width: 30.,
            height: 30.,
            fill: [240, 180, 40, 255],
            stroke: [0, 0, 0, 0],
            stroke_width: 0.,
        },
        ..Element::default()
    };

    let root_id = project.root.clone();
    project.format_version = 4;
    {
        let root_sym = project.symbol_mut(&root_id).unwrap();
        root_sym.timeline.frame_count = 10;
        let layer = &mut root_sym.timeline.layers[0];
        layer.keyframes[0].elements =
            vec![el_unnamed_inst, el_named_inst, el_text, el_grp_a, el_grp_b];
        layer.properties.groups.push(AuthoringGroup {
            id: new_id("group"),
            name: "Badge Group".into(),
            members: vec![grp_a_id, grp_b_id],
        });
    }
    project.validate().unwrap();

    let mut editor = Editor::from_project(&ctx, project);
    let before = editor.project.clone();
    let draws_before: Vec<_> = evaluate(&editor.project, &root_id, 0)
        .into_iter()
        .map(|d| (d.transform, d.kind))
        .collect();

    editor.select_all();
    assert!(editor.can_distribute_to_layers());
    assert!(editor.distribute_to_layers());
    editor.project.validate().unwrap();
    assert_eq!(editor.history.undo_label(), Some("Distribute to layers"));

    let layer_names: Vec<_> = editor
        .symbol()
        .timeline
        .layers
        .iter()
        .map(|l| l.name.as_str())
        .collect();
    assert_eq!(
        layer_names,
        vec![
            "Layer 1",
            "PlayButton",
            "hero_mc",
            "Welcome Banner",
            "Badge Group"
        ]
    );
    let draws_after: Vec<_> = evaluate(&editor.project, &root_id, 0)
        .into_iter()
        .map(|d| (d.transform, d.kind))
        .collect();
    assert_eq!(
        draws_after, draws_before,
        "back-to-front painter order must stay identical"
    );

    editor.undo();
    assert_eq!(editor.project, before);

    // Also verify distributing on frame > 0 when objects remain on frame 0 keeps unique element IDs.
    editor.frame = 5;
    editor.select_all();
    assert!(editor.distribute_to_layers());
    editor.project.validate().unwrap();
}

#[test]
fn enable_simple_buttons_evaluates_up_over_down_and_hit_tests_frame_3() {
    let ctx = egui::Context::default();
    let mut project = Project::blank();
    let make_rect = |w: f64, h: f64, fill: Color| Element {
        id: new_id("object"),
        name: "rect_1".into(),
        transform: Transform::default(),
        kind: ElementKind::Rectangle {
            width: w,
            height: h,
            fill,
            stroke: [0, 0, 0, 0],
            stroke_width: 0.,
        },
        ..Element::default()
    };

    let visible_btn_id = new_id("symbol");
    project.symbols.push(Symbol {
        id: visible_btn_id.clone(),
        name: "VisibleBtn".into(),
        kind: SymbolKind::Button,
        folder: String::new(),
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 4,
            guides: vec![],
            scripts: vec![],
            layers: vec![Layer {
                id: new_id("layer"),
                name: "States".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![
                    Keyframe {
                        frame: 0,
                        elements: vec![make_rect(100., 40., [220, 40, 40, 255])],
                        ..Default::default()
                    },
                    Keyframe {
                        frame: 1,
                        elements: vec![make_rect(100., 40., [40, 200, 80, 255])],
                        ..Default::default()
                    },
                    Keyframe {
                        frame: 2,
                        elements: vec![make_rect(100., 40., [40, 100, 240, 255])],
                        ..Default::default()
                    },
                    Keyframe {
                        frame: 3,
                        elements: vec![make_rect(160., 90., [0, 0, 0, 255])],
                        ..Default::default()
                    },
                ],
            }],
        },
    });

    let hotspot_btn_id = new_id("symbol");
    project.symbols.push(Symbol {
        id: hotspot_btn_id.clone(),
        name: "HotspotBtn".into(),
        kind: SymbolKind::Button,
        folder: String::new(),
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 4,
            guides: vec![],
            scripts: vec![],
            layers: vec![Layer {
                id: new_id("layer"),
                name: "Hotspot".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![
                    Keyframe {
                        frame: 0,
                        elements: vec![],
                        ..Default::default()
                    },
                    Keyframe {
                        frame: 3,
                        elements: vec![make_rect(60., 60., [0, 0, 0, 255])],
                        ..Default::default()
                    },
                ],
            }],
        },
    });

    let inst_visible_id = new_id("object");
    let inst_hotspot_id = new_id("object");
    let root_id = project.root.clone();
    project.symbol_mut(&root_id).unwrap().timeline.layers[0].keyframes[0].elements = vec![
        Element {
            id: inst_visible_id.clone(),
            name: "cta_btn".into(),
            transform: Transform {
                x: 50.,
                y: 50.,
                ..Default::default()
            },
            kind: ElementKind::Instance {
                symbol_id: visible_btn_id,
                playback: InstancePlayback::Independent,
            },
            ..Element::default()
        },
        Element {
            id: inst_hotspot_id.clone(),
            name: "hot_btn".into(),
            transform: Transform {
                x: 300.,
                y: 50.,
                ..Default::default()
            },
            kind: ElementKind::Instance {
                symbol_id: hotspot_btn_id,
                playback: InstancePlayback::Independent,
            },
            ..Element::default()
        },
    ];
    project.validate().unwrap();

    let mut editor = Editor::from_project(&ctx, project);
    let _ = full_frame(&mut editor, &ctx, 0.1, vec![]);
    let painter = ctx.layer_painter(egui::LayerId::background());

    // Hit-test uses frame 3 (160x90 at (50,50)), so (180, 120) hits even outside Up's 100x40 box!
    assert_eq!(
        editor.hit_test_simple_button(&painter, Pos2::new(180., 120.)),
        Some(inst_visible_id.clone())
    );
    // Invisible hotspot button (empty Up frame, 60x60 Hit frame at (300,50)) hits via frame 3!
    assert_eq!(
        editor.hit_test_simple_button(&painter, Pos2::new(320., 70.)),
        Some(inst_hotspot_id)
    );
    assert_eq!(
        editor.hit_test_simple_button(&painter, Pos2::new(10., 10.)),
        None
    );

    let first_fill = |editor: &mut Editor| -> Color {
        let scene = editor.current_authored_scene(true);
        match &scene.draws[0].item.kind {
            ElementKind::Rectangle { fill, .. } => *fill,
            other => panic!("expected rectangle, got {other:?}"),
        }
    };

    editor.set_enable_simple_buttons(true);
    assert_eq!(first_fill(&mut editor), [220, 40, 40, 255]); // Up

    editor.hovered_simple_button = Some(inst_visible_id.clone());
    assert_eq!(first_fill(&mut editor), [40, 200, 80, 255]); // Over

    editor.pressed_simple_button = Some(inst_visible_id.clone());
    assert_eq!(first_fill(&mut editor), [40, 100, 240, 255]); // Down

    editor.hovered_simple_button = None;
    assert_eq!(first_fill(&mut editor), [40, 200, 80, 255]); // Dragged outside while held = Over

    editor.set_enable_simple_buttons(false);
    assert!(editor.hovered_simple_button.is_none());
    assert!(editor.pressed_simple_button.is_none());
    assert_eq!(first_fill(&mut editor), [220, 40, 40, 255]); // Back to Up
}

#[test]
fn file_close_revert_recent_files_and_window_hide_panels() {
    let ctx = egui::Context::default();
    let dir = std::env::temp_dir().join(format!("neoflash-group2-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let file_path = dir.join("saved-test.nflash");

    let mut project = Project::blank();
    project.name = "SavedOriginal".into();
    std::fs::write(&file_path, project.to_json().unwrap()).unwrap();

    let mut editor = Editor::from_project(&ctx, Project::blank());
    editor.open_path(file_path.clone(), &ctx);
    assert_eq!(editor.project.name, "SavedOriginal");
    assert_eq!(editor.workspace.general.existing_recent_files().len(), 1);
    assert!(!editor.can_revert());

    // Mutate and revert from disk.
    editor.transact(|e| e.project.name = "UnsavedEdit".into());
    assert!(editor.dirty());
    assert!(editor.can_revert());
    editor.request_revert();
    assert!(editor.confirm_revert);
    assert!(editor.revert_to_saved(&ctx));
    assert!(!editor.confirm_revert);
    assert!(!editor.dirty());
    assert_eq!(editor.project.name, "SavedOriginal");

    // Revert uses the saved format, including FLA archives, and failed reads keep the edit.
    let fla_path = dir.join("saved-test.fla");
    assert!(editor.save_to_path(fla_path.clone()));
    let saved_width = editor.project.width;
    editor.transact(|e| e.project.width += 17);
    assert!(editor.revert_to_saved(&ctx));
    assert_eq!(editor.project.width, saved_width);
    assert_eq!(editor.path.as_ref(), Some(&fla_path));
    assert!(!editor.dirty());
    editor.project.validate().unwrap();
    editor.transact(|e| e.project.width += 23);
    let unsaved = editor.project.clone();
    std::fs::write(&fla_path, b"broken archive").unwrap();
    assert!(!editor.revert_to_saved(&ctx));
    assert_eq!(editor.project, unsaved);
    assert!(editor.dirty());
    editor.undo();
    assert_eq!(editor.project.width, saved_width);

    // Recent files deduplication, cap at 10, and clear.
    for i in 0..12 {
        editor
            .workspace
            .record_recent_file(dir.join(format!("proj-{i}.nflash")));
    }
    assert_eq!(editor.workspace.general.recent_files.len(), 10);
    editor.workspace.clear_recent_files();
    assert!(editor.workspace.general.recent_files.is_empty());

    // File ▸ Close prompts when dirty and resets to blank document when performed.
    editor.transact(|e| e.project.name = "DirtyBeforeClose".into());
    editor.request(crate::app::Pending::CloseDocument, &ctx);
    assert_eq!(
        editor.confirm,
        Some(crate::app::Pending::CloseSession(editor.active_session))
    );
    editor.continue_confirmation(&ctx);
    assert!(editor.path.is_none());
    assert!(!editor.dirty());

    // Window ▸ Hide panels (F4) hides chrome transiently without mutating PanelVisibility.
    let saved_visibility = editor.workspace.panels;
    assert!(!editor.panels_hidden);
    editor.toggle_hide_panels();
    assert!(editor.panels_hidden);
    assert_eq!(editor.workspace.panels, saved_visibility);
    let out_hidden = full_frame(&mut editor, &ctx, 0.2, vec![]);
    let has_tabs_while_hidden = out_hidden
        .platform_output
        .accesskit_update
        .as_ref()
        .is_some_and(|u| {
            u.nodes
                .iter()
                .any(|(_, n)| n.role() == egui::accesskit::Role::Tab)
        });
    assert!(!has_tabs_while_hidden);

    // Explicitly showing any panel auto-unhides panels.
    editor.show_panel(PanelId::Library);
    assert!(!editor.panels_hidden);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn compiler_errors_panel_collects_all_diagnostics_and_jumps_to_source() {
    use crate::panels::actions_sidebar::ScriptLocation;
    let ctx = egui::Context::default();
    let mut project = Project::blank();
    project.format_version = 3;
    let src_id = new_id("source");
    project.sources.push(SourceUnit {
        id: src_id.clone(),
        path: "src/Broken.as".into(),
        language: ScriptLanguage::ActionScript3,
        source: "package {\n  function ( {\n}\n".into(),
    });
    let root_id = project.root.clone();
    let obj_id = new_id("object");
    {
        let root = project.symbol_mut(&root_id).unwrap();
        root.timeline.frame_count = 5;
        root.timeline.scripts.push(FrameScript {
            frame: 2,
            script: Script {
                language: ScriptLanguage::ActionScript3,
                source: "var x = ;".into(),
            },
        });
        root.timeline.layers[0].keyframes[0].elements.push(Element {
            id: obj_id.clone(),
            name: "bad_btn".into(),
            transform: Transform::default(),
            kind: ElementKind::Text {
                text: "Custom Font Warning".into(),
                font_family: "Papyrus Custom NonWeb".into(),
                font_size: 18.,
                color: [255, 255, 255, 255],
            },
            scripts: vec![ObjectScript {
                event: ObjectEvent::Click,
                script: Script {
                    language: ScriptLanguage::ActionScript2,
                    source: "if ( {".into(),
                },
            }],
            ..Element::default()
        });
    }
    project.validate().unwrap();

    let mut editor = Editor::from_project(&ctx, project);
    editor.show_panel(PanelId::CompilerErrors);
    let snapshot = editor.project.clone();
    let _ = full_frame(&mut editor, &ctx, 0.1, vec![]);
    assert_eq!(
        editor.project, snapshot,
        "Compiler errors panel must never mutate project data"
    );

    editor.refresh_compiler_diagnostics();
    let items = &editor.compiler_errors.items;
    assert!(
        items
            .iter()
            .any(|i| i.is_error && i.script == Some(ScriptLocation::Source(src_id.clone()))),
        "expected project source error in {items:#?}"
    );
    assert!(
        items.iter().any(|i| i.is_error
            && i.script
                == Some(ScriptLocation::Frame {
                    symbol: root_id.clone(),
                    frame: 2,
                })),
        "expected frame script error in {items:#?}"
    );
    assert!(
        items.iter().any(|i| i.is_error
            && i.script
                == Some(ScriptLocation::Object {
                    symbol: root_id.clone(),
                    frame: 0,
                    element: obj_id.clone(),
                    event: ObjectEvent::Click,
                })),
        "expected object script error in {items:#?}"
    );
    assert!(
        items.iter().any(|i| !i.is_error),
        "expected HTML5 publish font warning in {items:#?}"
    );

    let frame_err_idx = items
        .iter()
        .position(|i| {
            i.script
                == Some(ScriptLocation::Frame {
                    symbol: root_id.clone(),
                    frame: 2,
                })
        })
        .unwrap();
    assert!(editor.jump_to_compiler_diagnostic(&ctx, frame_err_idx));
    assert_eq!(editor.frame, 2);
    assert!(editor.workspace.is_panel_visible(PanelId::Actions));
}

#[test]
fn code_snippets_all_compile_across_every_language_and_insert_with_undo() {
    use crate::panels::code_snippets::CODE_SNIPPETS;
    let languages = [
        ScriptLanguage::ActionScript3,
        ScriptLanguage::ActionScript2,
        ScriptLanguage::ActionScript1,
        ScriptLanguage::JavaScript,
        ScriptLanguage::TypeScript,
    ];
    let blank_program = neoflash_script::compile_project(&Project::blank()).unwrap();
    for (idx, snippet) in CODE_SNIPPETS.iter().enumerate() {
        for language in languages {
            let source = snippet.merge_into("", language, "orbit_mc");
            if let Err(errors) = neoflash_script::compile_with_program(
                &Script {
                    language,
                    source: source.clone(),
                },
                &blank_program,
            ) {
                panic!(
                    "snippet #{idx} ({:?}) failed to compile in {language:?}: {errors:#?}\nsource:\n{source}",
                    snippet.title
                );
            }
        }
    }

    // Insertion assigns an instance name to an unnamed selected symbol instance, merges AS3 imports
    // without duplication, and undoes in one transaction.
    let ctx = egui::Context::default();
    let mut project = Project::blank();
    let clip_sym_id = new_id("symbol");
    project.symbols.push(Symbol {
        id: clip_sym_id.clone(),
        name: "Star Spinner".into(),
        kind: SymbolKind::MovieClip,
        folder: String::new(),
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 1,
            guides: vec![],
            scripts: vec![],
            layers: vec![Layer {
                id: new_id("layer"),
                name: "Layer 1".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![],
                    ..Default::default()
                }],
            }],
        },
    });
    let inst_id = new_id("object");
    let root_id = project.root.clone();
    project.symbol_mut(&root_id).unwrap().timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: inst_id.clone(),
            name: String::new(),
            transform: Transform::default(),
            kind: ElementKind::Instance {
                symbol_id: clip_sym_id,
                playback: InstancePlayback::Independent,
            },
            ..Element::default()
        });
    project.validate().unwrap();

    let mut editor = Editor::from_project(&ctx, project);
    let before = editor.project.clone();
    editor.select_single_id(inst_id.clone());
    assert!(editor.insert_code_snippet(0)); // Click to go to frame and stop
    editor.project.validate().unwrap();
    assert_eq!(editor.history.undo_label(), Some("Insert code snippet"));

    let assigned_name = editor.symbol().timeline.layers[0].keyframes[0]
        .elements
        .iter()
        .find(|e| e.id == inst_id)
        .unwrap()
        .name
        .clone();
    assert_eq!(assigned_name, "star_spinner_mc");
    let script_src = &editor.symbol().timeline.scripts[0].script.source;
    assert!(script_src.contains("import flash.events.MouseEvent;"));
    assert!(script_src.contains("star_spinner_mc.addEventListener"));
    let program = neoflash_script::compile_project(&editor.project).unwrap();
    neoflash_script::compile_with_program(&editor.symbol().timeline.scripts[0].script, &program)
        .unwrap();

    // Inserting another MouseEvent snippet merges cleanly without duplicating the import line.
    assert!(editor.insert_code_snippet(4)); // Click to go to frame and play
    let merged_src = &editor.symbol().timeline.scripts[0].script.source;
    assert_eq!(
        merged_src
            .matches("import flash.events.MouseEvent;")
            .count(),
        1
    );
    neoflash_script::compile_with_program(&editor.symbol().timeline.scripts[0].script, &program)
        .unwrap();

    editor.undo();
    editor.undo();
    assert_eq!(editor.project, before);
}

#[test]
fn movie_explorer_builds_tree_filters_searches_and_navigates() {
    use crate::panels::movie_explorer::{
        CAT_SCRIPTS, CAT_TEXT, MovieTarget, build_movie_explorer_rows,
    };
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::demo());
    editor.new_scene();
    let second_scene = editor.current_scene().to_owned();
    let text_id = new_id("object");
    editor.transact(|e| {
        let sym = e.symbol_mut();
        sym.timeline.frame_count = 6;
        sym.timeline.scripts.push(FrameScript {
            frame: 3,
            script: Script {
                language: ScriptLanguage::ActionScript3,
                source: "trace(\"explorer-hit\");".into(),
            },
        });
        sym.timeline.layers[0].keyframes.push(Keyframe {
            frame: 3,
            elements: vec![Element {
                id: text_id.clone(),
                name: "banner_txt".into(),
                transform: Transform::default(),
                kind: ElementKind::Text {
                    text: "Secret Treasure".into(),
                    font_family: "sans-serif".into(),
                    font_size: 16.,
                    color: [255, 255, 255, 255],
                },
                ..Element::default()
            }],
            ..Default::default()
        });
    });
    let root_scene = editor.project.root.clone();
    editor.switch_scene(&root_scene);

    let all_rows = build_movie_explorer_rows(&editor);
    assert!(all_rows.iter().any(|r| r.label.contains("Secret Treasure")));
    assert!(all_rows.iter().any(|r| r.label.contains("Actions")));

    // Filter to Text only: scripts disappear, text remains.
    editor.movie_explorer.show_categories = [false; 4];
    editor.movie_explorer.show_categories[CAT_TEXT] = true;
    let text_rows = build_movie_explorer_rows(&editor);
    assert!(
        text_rows
            .iter()
            .any(|r| r.label.contains("Secret Treasure"))
    );
    assert!(!text_rows.iter().any(|r| r.label.contains("Actions  ·")));

    // Filter to Scripts only + search query.
    editor.movie_explorer.show_categories = [false; 4];
    editor.movie_explorer.show_categories[CAT_SCRIPTS] = true;
    let script_rows = build_movie_explorer_rows(&editor);
    assert!(
        script_rows
            .iter()
            .any(|r| matches!(r.target, MovieTarget::Script(_)))
    );

    editor.movie_explorer.show_categories = [true; 4];
    editor.movie_explorer.query = "secret treasure".into();
    let searched = build_movie_explorer_rows(&editor);
    let target = searched
        .iter()
        .find(|r| r.label.contains("Secret Treasure"))
        .unwrap()
        .target
        .clone();

    let snapshot = editor.project.clone();
    editor.navigate_movie_explorer_target(&target, false);
    assert_eq!(editor.current_scene(), second_scene);
    assert_eq!(editor.frame, 3);
    assert_eq!(
        editor.selected_element().map(|e| e.id.as_str()),
        Some(text_id.as_str())
    );
    assert_eq!(editor.project, snapshot);

    // UI regression: "Customise items to show" popup is closed by default, opens on click,
    // closes on "Show all items", and closes on outside click.
    ctx.enable_accesskit();
    editor.show_panel(PanelId::MovieExplorer);
    let size = Vec2::new(1440., 900.);
    let out = frame(&mut editor, &ctx, size, 0.0, vec![]);
    assert!(!egui::Popup::is_any_open(&ctx));
    assert!(named_bounds(&out, "Show symbol definitions").is_none());

    let gear = named_bounds(&out, "Customise items to show").expect("customise button");
    let _ = frame(
        &mut editor,
        &ctx,
        size,
        0.1,
        vec![primary_button(gear.center(), true)],
    );
    let _ = frame(
        &mut editor,
        &ctx,
        size,
        0.2,
        vec![primary_button(gear.center(), false)],
    );
    let out = frame(&mut editor, &ctx, size, 0.3, vec![]);
    assert!(egui::Popup::is_any_open(&ctx));
    let sym_defs_item =
        named_bounds(&out, "Show symbol definitions").expect("popup menu item visible");

    // Clicking an item toggles it; clicking outside closes the popup.
    let _ = frame(
        &mut editor,
        &ctx,
        size,
        0.4,
        vec![primary_button(sym_defs_item.center(), true)],
    );
    let _ = frame(
        &mut editor,
        &ctx,
        size,
        0.5,
        vec![primary_button(sym_defs_item.center(), false)],
    );
    assert!(!editor.movie_explorer.show_symbol_definitions);

    let outside = Pos2::new(200., 200.);
    let _ = frame(
        &mut editor,
        &ctx,
        size,
        0.6,
        vec![primary_button(outside, true)],
    );
    let _ = frame(
        &mut editor,
        &ctx,
        size,
        0.7,
        vec![primary_button(outside, false)],
    );
    let out = frame(&mut editor, &ctx, size, 0.8, vec![]);
    assert!(!egui::Popup::is_any_open(&ctx));
    assert!(named_bounds(&out, "Show symbol definitions").is_none());
}

#[test]
fn find_and_replace_covers_text_regex_font_colour_symbol_sound_and_undoes() {
    use crate::panels::find_replace::FindKind;
    let ctx = egui::Context::default();
    let mut project = Project::blank();
    project.format_version = 6;

    // Two audio resources for Sound swap.
    let wav_bytes = {
        let mut v = Vec::new();
        v.extend_from_slice(b"RIFF");
        v.extend_from_slice(&36u32.to_le_bytes());
        v.extend_from_slice(b"WAVEfmt ");
        v.extend_from_slice(&16u32.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes());
        v.extend_from_slice(&8000u32.to_le_bytes());
        v.extend_from_slice(&8000u32.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes());
        v.extend_from_slice(&8u16.to_le_bytes());
        v.extend_from_slice(b"data");
        v.extend_from_slice(&0u32.to_le_bytes());
        v
    };
    let snd_a = ResourceUnit::from_bytes(new_id("res"), "audio/click.wav", "audio/wav", &wav_bytes)
        .unwrap();
    let snd_b = ResourceUnit::from_bytes(new_id("res"), "audio/chime.wav", "audio/wav", &wav_bytes)
        .unwrap();
    let snd_a_id = snd_a.id.clone();
    let snd_b_id = snd_b.id.clone();
    project.resources.push(snd_a);
    project.resources.push(snd_b);

    // Two library symbols for Symbol swap.
    let make_sym = |name: &str| Symbol {
        id: new_id("symbol"),
        name: name.into(),
        kind: SymbolKind::MovieClip,
        folder: String::new(),
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 1,
            guides: vec![],
            scripts: vec![],
            layers: vec![Layer {
                id: new_id("layer"),
                name: "Layer 1".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![],
                    ..Default::default()
                }],
            }],
        },
    };
    let sym_old = make_sym("EnemyOld");
    let sym_new = make_sym("EnemyNew");
    let sym_old_id = sym_old.id.clone();
    let sym_new_id = sym_new.id.clone();
    project.symbols.push(sym_old);
    project.symbols.push(sym_new);

    let root_id = project.root.clone();
    let text_el_id = new_id("object");
    let rect_el_id = new_id("object");
    let inst_el_id = new_id("object");
    {
        let root = project.symbol_mut(&root_id).unwrap();
        root.timeline.scripts.push(FrameScript {
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript3,
                source: "var score_1 = 10; var score_25 = score_1 + 5;".into(),
            },
        });
        let key = &mut root.timeline.layers[0].keyframes[0];
        key.label = "intro_score_9".into();
        key.sound = Some(FrameSound {
            resource: snd_a_id.clone(),
            sync: SoundSync::Event,
            loops: 1,
            loop_forever: false,
            effect: SoundEffect::None,
            envelope: None,
            in_point: None,
            out_point: None,
        });
        key.elements = vec![
            Element {
                id: text_el_id.clone(),
                name: "score_label".into(),
                transform: Transform::default(),
                kind: ElementKind::Text {
                    text: "High score_100".into(),
                    font_family: "Helvetica".into(),
                    font_size: 24.,
                    color: [255, 0, 0, 255],
                },
                ..Element::default()
            },
            Element {
                id: rect_el_id.clone(),
                name: "box_1".into(),
                transform: Transform::default(),
                kind: ElementKind::Rectangle {
                    width: 50.,
                    height: 50.,
                    fill: [255, 0, 0, 180],
                    stroke: [0, 0, 0, 0],
                    stroke_width: 0.,
                },
                ..Element::default()
            },
            Element {
                id: inst_el_id.clone(),
                name: "foe_mc".into(),
                transform: Transform::default(),
                kind: ElementKind::Instance {
                    symbol_id: sym_old_id,
                    playback: InstancePlayback::Independent,
                },
                ..Element::default()
            },
        ];
    }
    project.validate().unwrap();

    let mut editor = Editor::from_project(&ctx, project);
    let baseline = editor.project.clone();

    // 1. Regex Text Find & Replace All across text field, frame label, and multiple occurrences in one script.
    editor.open_find_replace();
    {
        let st = editor.find_replace.as_mut().unwrap();
        st.kind = FindKind::Text;
        st.find = "score_[0-9]+".into();
        st.replace = "points".into();
        st.regex = true;
    }
    assert_eq!(editor.find_all_matches(), 5);
    assert_eq!(editor.replace_all_matches(), 5);
    editor.project.validate().unwrap();
    assert_eq!(editor.history.undo_label(), Some("Find and replace"));
    assert_eq!(
        editor.symbol().timeline.scripts[0].script.source,
        "var points = 10; var points = points + 5;"
    );
    assert_eq!(
        editor.symbol().timeline.layers[0].keyframes[0].label,
        "intro_points"
    );
    editor.undo();
    assert_eq!(editor.project, baseline);

    // 2. Font replace.
    {
        let st = editor.find_replace.as_mut().unwrap();
        st.kind = FindKind::Font;
        st.regex = false;
        st.find = "Helvetica".into();
        st.replace = "Georgia".into();
    }
    assert_eq!(editor.replace_all_matches(), 1);
    editor.project.validate().unwrap();
    match &editor.symbol().timeline.layers[0].keyframes[0].elements[0].kind {
        ElementKind::Text { font_family, .. } => assert_eq!(font_family, "Georgia"),
        other => panic!("unexpected {other:?}"),
    }
    editor.undo();

    // 3. Colour replace (#RRGGBB preserves existing alpha 180 on rectangle fill!).
    {
        let st = editor.find_replace.as_mut().unwrap();
        st.kind = FindKind::Colour;
        st.find = "#ff0000".into();
        st.replace = "#00ff88".into();
    }
    assert_eq!(editor.replace_all_matches(), 2);
    editor.project.validate().unwrap();
    match &editor.symbol().timeline.layers[0].keyframes[0].elements[1].kind {
        ElementKind::Rectangle { fill, .. } => assert_eq!(*fill, [0, 255, 136, 180]),
        other => panic!("unexpected {other:?}"),
    }
    editor.undo();

    // 4. Symbol replace.
    {
        let st = editor.find_replace.as_mut().unwrap();
        st.kind = FindKind::Symbol;
        st.find = "EnemyOld".into();
        st.replace = "EnemyNew".into();
    }
    assert_eq!(editor.replace_all_matches(), 1);
    editor.project.validate().unwrap();
    match &editor.symbol().timeline.layers[0].keyframes[0].elements[2].kind {
        ElementKind::Instance { symbol_id, .. } => assert_eq!(symbol_id, &sym_new_id),
        other => panic!("unexpected {other:?}"),
    }
    editor.undo();

    // 5. Sound replace.
    {
        let st = editor.find_replace.as_mut().unwrap();
        st.kind = FindKind::Sound;
        st.find = "audio/click.wav".into();
        st.replace = "audio/chime.wav".into();
    }
    assert_eq!(editor.replace_all_matches(), 1);
    editor.project.validate().unwrap();
    assert_eq!(
        editor.symbol().timeline.layers[0].keyframes[0]
            .sound
            .as_ref()
            .unwrap()
            .resource,
        snd_b_id
    );
    editor.undo();
    assert_eq!(editor.project, baseline);
}

#[test]
fn document_strip_list_close_cancel_empty_state_and_shortcuts_work_in_both_themes() {
    for theme in [ThemeMode::Dark, ThemeMode::Light] {
        let mut d = Driver::new();
        d.editor.set_theme(&d.ctx, theme);
        d.editor.transact(|e| e.project.name = "Alpha".into());
        d.editor.request(crate::app::Pending::New, &d.ctx);
        d.editor.transact(|e| e.project.name = "Beta".into());
        d.settle();
        let alpha = named_bounds(&d.out, "Alpha — unsaved").expect("visible document tab");
        let beta = named_bounds(&d.out, "Beta — unsaved").expect("visible active document tab");
        assert!(alpha.right() <= beta.left());
        assert!(
            alpha.top() >= neoflash_ui::metrics::TOP_BAR
                && alpha.bottom()
                    <= neoflash_ui::metrics::TOP_BAR + neoflash_ui::metrics::PANEL_HEADER
        );
        assert!((96. ..=220.).contains(&alpha.width()));
        d.press(alpha.center(), egui::PointerButton::Primary);
        assert_eq!(d.editor.project.name, "Alpha");
        d.editor.undo();
        assert_ne!(d.editor.project.name, "Alpha");
        d.editor.redo();
        d.click("Open documents");
        d.click("Next document");
        assert_eq!(d.editor.project.name, "Beta");
        d.click("Open documents");
        d.click("Close all");
        assert_eq!(d.editor.project.name, "Alpha");
        d.click("Discard");
        assert_eq!(d.editor.project.name, "Beta");
        d.click("Cancel");
        assert_eq!(d.editor.session_count(), 2);
        assert_eq!(d.editor.project.name, "Beta");
        d.editor.request_close_documents(&d.ctx, false);
        d.settle();
        d.click("Discard");
        d.click("Discard");
        d.settle();
        assert_eq!(d.editor.session_count(), 0);
        assert!(d.texts().iter().any(|text| text == "No document open"));
        let before = d.editor.project.clone();
        d.step(vec![Event::Key {
            key: egui::Key::F6,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        assert_eq!(d.editor.project, before);
        d.click("New document");
        assert_eq!(d.editor.session_count(), 1);
        assert!(!d.texts().iter().any(|text| text == "No document open"));
        assert!(!d.editor.history.can_undo());
    }
}

#[test]
fn onion_skin_outline_mode_and_ruler_range_handles_work_without_off_by_one() {
    let mut d = Driver::new();
    d.editor.frame = 5;
    d.editor.onion_before = 3;
    d.editor.onion_after = 3;
    assert!(!d.editor.onion && !d.editor.onion_outlines);
    d.settle();

    // 1. Clicking "Onion skin outlines" directly from off turns on both onion skin and outlines.
    d.click("Onion skin outlines");
    assert!(
        d.editor.onion && d.editor.onion_outlines,
        "clicking Onion skin outlines from off must enable onion skin in outline mode"
    );
    assert_eq!(d.editor.onion_range(), (2, 8));

    // 2. Clicking "Onion skin" switches from outlines to solid onion skin without turning it off.
    d.click("Onion skin");
    assert!(
        d.editor.onion && !d.editor.onion_outlines,
        "clicking Onion skin while in outline mode must switch to solid mode"
    );

    // 3. Clicking "Onion skin outlines" switches back to outline mode; clicking it again turns onion skin off.
    d.click("Onion skin outlines");
    assert!(d.editor.onion && d.editor.onion_outlines);
    d.click("Onion skin outlines");
    assert!(
        !d.editor.onion && !d.editor.onion_outlines,
        "clicking active Onion skin outlines button must turn onion skin off"
    );

    // 4. Checking "Outlines" inside the Onion skin options popover also activates onion skin.
    d.click("Onion skin options");
    d.click("Outlines");
    assert!(
        d.editor.onion && d.editor.onion_outlines,
        "checking Outlines in the options popover must enable onion skin"
    );
    // Close the popover by clicking the options button again.
    d.click("Onion skin options");
    d.settle();

    // 5. Verify the painted onion range handles on the ruler and drag both handles without
    // off-by-one jumps or playhead seek hijacking.
    let onion_color = neoflash_ui::palette(&d.ctx).timeline_onion;
    let handle_rects = |out: &egui::FullOutput| -> Vec<Rect> {
        let mut rects: Vec<Rect> = out
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Rect(r)
                    if r.fill == onion_color && (r.rect.width() - 3.0).abs() < 0.2 =>
                {
                    Some(r.rect)
                }
                _ => None,
            })
            .collect();
        rects.sort_by(|a, b| a.left().total_cmp(&b.left()));
        rects.dedup_by(|a, b| (a.left() - b.left()).abs() < 0.5);
        rects
    };
    let bars = handle_rects(&d.out);
    assert_eq!(
        bars.len(),
        2,
        "must paint start and end onion range handles"
    );
    let cw = d.editor.timeline_cell;
    let span_width = bars[1].right() - bars[0].left();
    assert!(
        (span_width - 7.0 * cw).abs() < 0.5,
        "range 2..=8 (7 frames) must span 7 * cell_width, got {span_width} (cw={cw})"
    );

    // Drag right handle by +2 frames: onion_after must become 5 (end = 10), not 6, and frame stays 5.
    let right_from = bars[1].center();
    let right_to = right_from + Vec2::new(2.0 * cw, 0.0);
    d.step(vec![
        Event::PointerMoved(right_from),
        primary_button(right_from, true),
    ]);
    d.step(vec![Event::PointerMoved(right_to)]);
    d.step(vec![primary_button(right_to, false)]);
    d.settle();
    assert_eq!(
        d.editor.frame, 5,
        "dragging onion handle must not seek playhead"
    );
    assert_eq!(
        d.editor.onion_after, 5,
        "right handle drag must not have +1 frame off-by-one error"
    );
    assert_eq!(d.editor.onion_range(), (2, 10));

    // Drag left handle by -1 frame: onion_before must become 4 (start = 1), and frame stays 5.
    let bars = handle_rects(&d.out);
    let left_from = bars[0].center();
    let left_to = left_from - Vec2::new(1.0 * cw, 0.0);
    d.step(vec![
        Event::PointerMoved(left_from),
        primary_button(left_from, true),
    ]);
    d.step(vec![Event::PointerMoved(left_to)]);
    d.step(vec![primary_button(left_to, false)]);
    d.settle();
    assert_eq!(d.editor.frame, 5);
    assert_eq!(d.editor.onion_before, 4);
    assert_eq!(d.editor.onion_range(), (1, 10));
    assert_eq!(d.editor.workspace.onion_range, [4, 5]);

    // Same-tick move+release drag (as synthesized by macOS CUA) on the right handle updates onion_after.
    let bars = handle_rects(&d.out);
    let right_from = bars[1].center();
    let right_to = right_from - Vec2::new(2.0 * cw, 0.0);
    d.step(vec![
        Event::PointerMoved(right_from),
        primary_button(right_from, true),
    ]);
    d.step(vec![
        Event::PointerMoved(right_to),
        primary_button(right_to, false),
    ]);
    d.settle();
    assert_eq!(d.editor.frame, 5);
    assert_eq!(d.editor.onion_after, 3);
    assert_eq!(d.editor.workspace.onion_range, [4, 3]);

    // Reopening/installing a document in the same app retains onion_before/onion_after from workspace.onion_range.
    let reopened = d.editor.project.clone();
    d.editor.install_document(&d.ctx, reopened, None, true);
    assert_eq!(
        (d.editor.onion_before, d.editor.onion_after),
        (4, 3),
        "reopening a document in the same app must retain onion range"
    );
}

#[test]
fn preferences_header_stays_top_aligned_when_pages_have_different_heights() {
    use crate::panels::preferences::{PAGE_CODE_EDITOR, PAGE_GENERAL};

    let mut d = Driver::new();
    d.editor.open_preferences(PAGE_GENERAL);
    d.settle();
    let general_close = d.find("Close").expect("Preferences close button");

    d.editor.preferences.as_mut().unwrap().page = PAGE_CODE_EDITOR;
    d.settle();
    let code_close = d.find("Close").expect("Preferences close button");

    assert!(
        (general_close.min - code_close.min).length() < 0.1,
        "Preferences header must not jump when page height changes: {general_close:?} vs {code_close:?}"
    );
    assert!(
        code_close.top() < 100.0,
        "Preferences header must stay at the top of the window: {code_close:?}"
    );
}

#[test]
fn preferences_short_window_keeps_header_and_footer_visible() {
    use crate::panels::preferences::PAGE_CODE_EDITOR;

    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut editor = Editor::from_project(&ctx, Project::demo());
    editor.open_preferences(PAGE_CODE_EDITOR);
    let render = |editor: &mut Editor, time| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1024.0, 720.0))),
                time: Some(time),
                ..Default::default()
            },
            |ctx| editor.preferences_dialog(ctx),
        )
    };
    render(&mut editor, 0.1);
    let out = render(&mut editor, 0.2);
    let close = named_bounds(&out, "Close").expect("Preferences close button");
    let save = named_bounds(&out, "Save").expect("Preferences save button");

    assert!(close.top() < 100.0, "header must remain visible: {close:?}");
    assert!(
        save.bottom() <= 720.0,
        "footer must remain inside the short window: {save:?}"
    );
    assert!(
        named_bounds(&out, "Vertical scroll").is_some(),
        "only the tall page body should scroll in a short window"
    );
}

#[test]
fn timeline_horizontal_scrollbar_is_only_visible_when_frames_overflow() {
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::demo());
    let has_wide_horizontal_track = |out: &egui::FullOutput| {
        let track = crate::theme::UiPalette::current(&ctx).scrollbar_track;
        out.shapes.iter().any(|shape| {
            matches!(
                &shape.shape,
                egui::Shape::Rect(rect)
                    if rect.fill == track
                        && rect.rect.width() > 400.0
                        && rect.rect.height() <= 20.0
            )
        })
    };

    frame(&mut editor, &ctx, Vec2::new(1920.0, 1080.0), 0.1, vec![]);
    let wide = frame(&mut editor, &ctx, Vec2::new(1920.0, 1080.0), 0.2, vec![]);
    assert!(
        !has_wide_horizontal_track(&wide),
        "timeline must not reserve a scrollbar strip when all frames fit"
    );

    frame(&mut editor, &ctx, Vec2::new(1024.0, 720.0), 0.3, vec![]);
    let narrow = frame(&mut editor, &ctx, Vec2::new(1024.0, 720.0), 0.4, vec![]);
    assert!(
        has_wide_horizontal_track(&narrow),
        "timeline scrollbar must remain available when frames overflow"
    );
}

/// T-101 / R-051: a density profile resizes NF2 chrome (Play, fields, rows) without changing
/// behaviour; the Preferences draft previews it live and Cancel / Save behave like the theme.
#[test]
fn density_profiles_resize_chrome_and_follow_preferences_preview() {
    use crate::workspace::{DensityPref, WorkspacePrefs};
    use neoflash_ui::metrics;
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut editor = Editor::from_project(&ctx, Project::demo());
    let before = editor.project.clone();
    let size = Vec2::new(1440., 900.);
    let mut time = 0.;
    let mut run = |editor: &mut Editor| {
        let mut out = None;
        for _ in 0..3 {
            time += 0.05;
            editor.apply_density(&ctx);
            out = Some(frame(editor, &ctx, size, time, vec![]));
        }
        out.unwrap()
    };
    let play = |out: &egui::FullOutput| named_bounds(out, "Play preview").expect("Play").height();

    let out = run(&mut editor);
    assert_eq!(play(&out), 32.);
    assert_eq!(ctx.style().spacing.interact_size.y, 32.);

    editor.set_density(&ctx, DensityPref::ClassicFlash);
    let out = run(&mut editor);
    assert_eq!(play(&out), 24.);
    assert_eq!(ctx.style().spacing.interact_size.y, 24.);
    assert!(editor.workspace.timeline.compact_rows);
    assert_eq!(editor.workspace.timeline.row_height(), 16.);
    assert_eq!(editor.workspace.sizes.layer_column_width, 292.);

    // Preferences previews the draft's density; Cancel (dropping the draft) restores the saved one.
    editor.open_preferences(0);
    editor.preferences.as_mut().unwrap().density = DensityPref::Compact;
    editor.apply_density(&ctx);
    assert_eq!(metrics::control(), 28.);
    editor.preferences = None;
    editor.apply_density(&ctx);
    assert_eq!(metrics::control(), 24.);

    // Save applies the profile with its layer-row preset and column width.
    editor.open_preferences(0);
    editor.preferences.as_mut().unwrap().density = DensityPref::Comfortable;
    let draft = editor.preferences.take().unwrap();
    editor.save_preferences(&ctx, &draft);
    let out = run(&mut editor);
    assert_eq!(play(&out), 32.);
    assert_eq!(editor.workspace.density, DensityPref::Comfortable);
    assert!(!editor.workspace.timeline.short_rows && !editor.workspace.timeline.compact_rows);
    assert_eq!(
        editor.workspace.sizes.layer_column_width,
        metrics::LAYER_COLUMN_MODE_SOLO
    );

    // Saved preferences from before density existed load as Comfortable.
    let prefs: WorkspacePrefs = serde_json::from_str("{}").unwrap();
    assert_eq!(prefs.density, DensityPref::Comfortable);
    let json = serde_json::to_string(&editor.workspace).unwrap();
    assert!(json.contains("\"density\":\"comfortable\""));
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());
}

/// T-101 / R-053: the Stage toolbar is a collapsed pill, a floating bar or a Flash-style edit bar;
/// the density profile chooses the state, the edit bar shrinks the canvas instead of covering it.
#[test]
fn stage_toolbar_states_follow_density_and_never_cover_artwork_in_edit_bar() {
    use crate::workspace::{DensityPref, StageToolbarMode};
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut editor = Editor::from_project(&ctx, Project::demo());
    let before = editor.project.clone();
    let size = Vec2::new(1440., 900.);
    let mut time = 0.;
    let mut run = |editor: &mut Editor| {
        let mut out = None;
        for _ in 0..3 {
            time += 0.05;
            editor.apply_density(&ctx);
            out = Some(frame(editor, &ctx, size, time, vec![]));
        }
        out.unwrap()
    };
    // Default (Comfortable): the collapsed pill; its expand button and zoom percentage are present,
    // the full toolbar is not.
    assert_eq!(editor.workspace.stage_toolbar, StageToolbarMode::Collapsed);
    let out = run(&mut editor);
    assert!(named_bounds(&out, "Show stage toolbar").is_some());
    assert!(named_bounds(&out, "Zoom in").is_none());
    let collapsed_canvas = editor.stage_view.canvas;

    // Floating: the full toolbar over the canvas.
    editor.workspace.stage_toolbar = StageToolbarMode::Expanded;
    let out = run(&mut editor);
    let zoom_in = named_bounds(&out, "Zoom in").expect("floating toolbar");
    assert!(collapsed_canvas.contains_rect(zoom_in));

    // Classic Flash selects the edit bar: the toolbar sits above the canvas, which gets shorter.
    editor.set_density(&ctx, DensityPref::ClassicFlash);
    assert_eq!(editor.workspace.stage_toolbar, StageToolbarMode::EditBar);
    let out = run(&mut editor);
    let canvas = editor.stage_view.canvas;
    let zoom_in = named_bounds(&out, "Zoom in").expect("edit bar toolbar");
    assert!(
        zoom_in.bottom() <= canvas.top() + 0.5,
        "{zoom_in:?} above {canvas:?}"
    );
    assert!(named_bounds(&out, "Show stage toolbar").is_none());
    // Its zoom buttons still work.
    let zoom = editor.zoom;
    let p = zoom_in.center();
    frame(
        &mut editor,
        &ctx,
        size,
        9.0,
        vec![
            Event::PointerMoved(p),
            crate::tests::primary_button(p, true),
        ],
    );
    frame(
        &mut editor,
        &ctx,
        size,
        9.05,
        vec![crate::tests::primary_button(p, false)],
    );
    assert!(editor.zoom > zoom);

    // Compact returns to the collapsed pill.
    editor.set_density(&ctx, DensityPref::Compact);
    assert_eq!(editor.workspace.stage_toolbar, StageToolbarMode::Collapsed);
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());
}

/// T-101 / R-054: Inspector / Library priority give the stacked right-dock group the larger share;
/// ⌥⌘L / ⌥⌘F3 and a double-click on the splitter switch it. Layout only, never the document.
#[test]
fn right_dock_priority_switches_by_shortcut_and_splitter_double_click() {
    use crate::workspace::{DockLayout, PanelId};
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut editor = Editor::from_project(&ctx, Project::demo());
    let before = editor.project.clone();
    let size = Vec2::new(1440., 900.);
    let weight = |editor: &Editor, panel: PanelId| {
        editor
            .workspace
            .dock
            .right
            .groups
            .iter()
            .find(|g| g.tabs.contains(&panel))
            .unwrap()
            .weight
    };
    // Default: Inspector priority, and Library keeps a usable share.
    assert!(editor.workspace.dock.has_priority(PanelId::Properties));
    assert_eq!(
        weight(&editor, PanelId::Library),
        1. - DockLayout::PRIORITY_WEIGHT
    );
    frame(&mut editor, &ctx, size, 0.0, vec![]);
    let key = |key, t| {
        vec![egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::ALT | egui::Modifiers::COMMAND,
        }]
        .into_iter()
        .chain(std::iter::once(egui::Event::PointerMoved(Pos2::new(t, t))))
        .collect::<Vec<_>>()
    };
    frame(&mut editor, &ctx, size, 0.1, key(egui::Key::L, 5.));
    assert!(editor.workspace.dock.has_priority(PanelId::Library));
    assert_eq!(
        weight(&editor, PanelId::Library),
        DockLayout::PRIORITY_WEIGHT
    );
    frame(&mut editor, &ctx, size, 0.2, key(egui::Key::F3, 6.));
    assert!(editor.workspace.dock.has_priority(PanelId::Properties));
    // Double-click on the splitter between Properties and Library gives the smaller group priority.
    let mut out = frame(&mut editor, &ctx, size, 0.3, vec![]);
    for i in 0..3 {
        out = frame(&mut editor, &ctx, size, 0.35 + i as f64 * 0.02, vec![]);
    }
    // The splitter sits directly above the Library group's header (its "Library" title).
    let search = named_bounds(&out, "Search library").expect("Library search");
    let header = out
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .filter(|(_, n)| n.label() == Some("Library"))
        .filter_map(|(_, n)| n.bounds())
        .map(|b| {
            Rect::from_min_max(
                Pos2::new(b.x0 as f32, b.y0 as f32),
                Pos2::new(b.x1 as f32, b.y1 as f32),
            )
        })
        .filter(|r| r.bottom() <= search.top() + 1. && r.left() >= search.left() - 40.)
        .max_by(|a, b| a.top().total_cmp(&b.top()))
        .expect("Library header above its search");
    let split = Pos2::new(search.center().x, header.top() - 3.);
    for (t, pressed) in [(1.0, true), (1.02, false), (1.06, true), (1.08, false)] {
        frame(
            &mut editor,
            &ctx,
            size,
            t,
            vec![
                Event::PointerMoved(split),
                crate::tests::primary_button(split, pressed),
            ],
        );
    }
    assert!(
        editor.workspace.dock.has_priority(PanelId::Library),
        "double-click on the splitter at {split:?}"
    );
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());
}

/// T-101 / R-055: the Color panel is compact by default (hex · alpha, recent colours) and opens the
/// full picker on demand; the choice is a workspace preference, never document state.
#[test]
fn color_panel_is_compact_and_opens_the_picker_on_demand() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut editor = Editor::from_project(&ctx, Project::demo());
    let before = editor.project.clone();
    editor.show_panel(PanelId::Color);
    let size = Vec2::new(1440., 900.);
    let mut time = 0.;
    let mut run = |editor: &mut Editor, events: Vec<Event>| {
        time += 0.05;
        let out = frame(editor, &ctx, size, time, events);
        time += 0.05;
        frame(editor, &ctx, size, time, vec![]);
        out
    };
    assert!(!editor.workspace.color_picker_expanded);
    let out = run(&mut editor, vec![]);
    let show = named_bounds(&out, "Show color picker")
        .expect("compact Color row")
        .center();
    assert!(named_bounds(&out, "Hide color picker").is_none());
    run(
        &mut editor,
        vec![
            Event::PointerMoved(show),
            crate::tests::primary_button(show, true),
        ],
    );
    run(&mut editor, vec![crate::tests::primary_button(show, false)]);
    assert!(editor.workspace.color_picker_expanded);
    let out = run(&mut editor, vec![]);
    let hide = named_bounds(&out, "Hide color picker")
        .expect("expanded picker")
        .center();
    run(
        &mut editor,
        vec![
            Event::PointerMoved(hide),
            crate::tests::primary_button(hide, true),
        ],
    );
    run(&mut editor, vec![crate::tests::primary_button(hide, false)]);
    assert!(!editor.workspace.color_picker_expanded);
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());
}

#[test]
fn export_image_defaults_to_the_active_scene() {
    use crate::panels::export_image::export_image_draft;

    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::demo());
    editor.new_scene();
    let active_scene = editor.symbol_id().to_owned();
    editor.frame = 0;

    editor.open_export_image(&ctx);
    let draft = export_image_draft(&ctx).expect("export draft");
    assert_eq!(
        draft.options.symbol_id.as_deref(),
        Some(active_scene.as_str())
    );
    assert_eq!(
        draft
            .options
            .range
            .resolve_frames(&editor.project, draft.options.symbol_id.as_deref()),
        (0, 0)
    );
}

/// T-126 / T-127: Export image (PNG/SVG/PSD/AI/PDF), Import video, Stroke style, Font embedding,
/// Bitmap fill swatch tiling, Deco pattern, Spray brush burst, and Stage context-menu Swap/Duplicate
/// symbol all work end-to-end with single-step Undo/Redo and valid project documents.
#[test]
fn t126_planned_dialogs_panels_and_menus_work_with_undo() {
    use crate::panels::{
        export_image::{
            ExportImageDraft, export_image_draft, has_export_image_dialog,
            set_inline_export_image_draft, update_export_image_draft,
        },
        font_embedding::{
            FontEmbeddingDraft, font_embedding_draft, has_font_embedding_dialog,
            set_inline_font_embedding_draft, update_font_embedding_draft,
        },
        import_video::{
            ImportVideoDraft, has_import_video_dialog, import_video_draft,
            set_inline_import_video_draft, update_import_video_draft,
        },
        stroke_style::has_stroke_style_dialog,
    };
    use neoflash_export::ImageExportFormat;

    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::demo());
    editor.project.validate().expect("demo validates");

    // 1. Export image modal dialog across PNG, SVG, PSD, AI, and PDF.
    let tmp_dir = std::env::temp_dir().join(format!("neoflash_t126_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp_dir);
    std::fs::create_dir_all(&tmp_dir).unwrap();

    Editor::open_planned(&ctx, "File ▸ Export ▸ Export image…");
    editor.drain_converted_entry_triggers(&ctx);
    assert!(has_export_image_dialog(&ctx));
    assert!(editor.has_modal_dialog_in(&ctx));
    let failed_export_parent = tmp_dir.join("not-a-directory");
    std::fs::write(&failed_export_parent, b"file").unwrap();
    let export_before_failure = editor.project.clone();
    let export_error = editor
        .confirm_export_image_to_path(&ctx, &failed_export_parent.join("image.png"))
        .expect_err("export into a file path must fail");
    assert!(!export_error.is_empty());
    assert_eq!(editor.project, export_before_failure);
    assert!(
        has_export_image_dialog(&ctx),
        "failed export must keep the draft open"
    );
    for fmt in ImageExportFormat::ALL {
        editor.open_export_image(&ctx);
        update_export_image_draft(&ctx, |d| d.options.format = fmt);
        set_inline_export_image_draft(
            &ctx,
            ExportImageDraft {
                options: neoflash_export::ImageExportOptions::for_project(&editor.project, 0),
            },
        );
        let out_path = tmp_dir.join(format!("export.{}", fmt.extension()));
        let report = editor
            .confirm_export_image_to_path(&ctx, &out_path)
            .expect("export image succeeds");
        assert!(out_path.exists());
        assert!(report.total_bytes > 0);
        assert!(
            export_image_draft(&ctx).is_none(),
            "successful export must clear modal and inline drafts"
        );
    }
    assert!(!has_export_image_dialog(&ctx));
    set_inline_export_image_draft(
        &ctx,
        ExportImageDraft {
            options: neoflash_export::ImageExportOptions::for_project(&editor.project, 0),
        },
    );
    editor.cancel_export_image(&ctx);
    assert!(
        export_image_draft(&ctx).is_none(),
        "inline Cancel must clear the draft"
    );

    // 2. Import video modal dialog + Undo/Redo + validation.
    let before_video = editor.project.clone();
    Editor::open_planned(&ctx, "File ▸ Import ▸ Import video…");
    editor.drain_converted_entry_triggers(&ctx);
    assert!(has_import_video_dialog(&ctx));
    update_import_video_draft(&ctx, |draft| {
        draft.file_path = tmp_dir.join("missing.mp4").display().to_string();
    });
    assert!(editor.confirm_import_video(&ctx).is_err());
    assert_eq!(
        editor.project, before_video,
        "failed import mutated document"
    );
    assert!(has_import_video_dialog(&ctx), "failed import closed dialog");

    let oversized_video_path = tmp_dir.join("oversized.mp4");
    std::fs::File::create(&oversized_video_path)
        .unwrap()
        .set_len((neoflash_core::MAX_RESOURCE_BYTES + 1) as u64)
        .unwrap();
    update_import_video_draft(&ctx, |draft| {
        draft.file_path = oversized_video_path.display().to_string();
    });
    let oversized_error = editor
        .confirm_import_video(&ctx)
        .expect_err("oversized video must be rejected before import");
    assert!(oversized_error.contains("16 MiB resource limit"));
    assert_eq!(
        editor.project, before_video,
        "oversized import mutated document"
    );
    assert!(
        has_import_video_dialog(&ctx),
        "oversized import closed dialog"
    );

    let video_path = tmp_dir.join("clip.mp4");
    std::fs::write(
        &video_path,
        b"\x00\x00\x00\x18ftypisom\x00\x00\x02\x00isomiso2avc1mp41",
    )
    .unwrap();
    update_import_video_draft(&ctx, |draft| {
        draft.file_path = video_path.display().to_string();
    });
    set_inline_import_video_draft(&ctx, ImportVideoDraft::default());
    let vid_res = editor
        .confirm_import_video(&ctx)
        .expect("import video succeeds");
    assert!(!has_import_video_dialog(&ctx));
    assert!(
        import_video_draft(&ctx).is_none(),
        "successful modal import must clear a stale inline draft"
    );
    assert_eq!(editor.history.undo_label(), Some("Import video"));
    editor.project.validate().expect("valid after import video");
    let after_first_video = editor.project.clone();

    editor.open_import_video(&ctx);
    update_import_video_draft(&ctx, |draft| {
        draft.file_path = video_path.display().to_string();
    });
    let reused = editor
        .confirm_import_video(&ctx)
        .expect("repeated identical video import succeeds");
    assert_eq!(
        reused, vid_res,
        "identical video bytes must reuse the resource"
    );
    assert_eq!(
        editor.project.resources.len(),
        after_first_video.resources.len(),
        "repeated import created an orphan resource"
    );
    editor
        .project
        .validate()
        .expect("repeat import stays valid");
    editor.undo();
    assert_eq!(editor.project, after_first_video);

    std::fs::write(
        &video_path,
        b"\x00\x00\x00\x18ftypisom\x00\x00\x02\x00isomiso2avc1mp42",
    )
    .unwrap();
    editor.open_import_video(&ctx);
    update_import_video_draft(&ctx, |draft| {
        draft.file_path = video_path.display().to_string();
    });
    let distinct = editor
        .confirm_import_video(&ctx)
        .expect("same-name video with different bytes succeeds");
    assert_ne!(distinct, vid_res);
    let video_resources: Vec<_> = editor
        .project
        .resources
        .iter()
        .filter(|resource| resource.is_video())
        .collect();
    assert_eq!(video_resources.len(), 2);
    assert_ne!(
        video_resources[0].path.to_ascii_lowercase(),
        video_resources[1].path.to_ascii_lowercase(),
        "different video bytes need distinct logical paths"
    );
    editor
        .project
        .validate()
        .expect("same-name videos stay valid");
    editor.undo();
    assert_eq!(
        editor.project, after_first_video,
        "one Undo must remove the distinct resource and placement"
    );

    editor.undo();
    assert_eq!(editor.project, before_video);
    editor.redo();
    editor
        .project
        .validate()
        .expect("valid after redo import video");
    editor.undo();
    let _ = std::fs::remove_dir_all(&tmp_dir);

    // 3. Stroke style modal dialog on a selected shape + Undo/Redo + validation.
    let rect_id = new_id("rect");
    editor.transact_named("Add rect", |ed| {
        let l_idx = ed.layer;
        let f_idx = ed.frame;
        if let Some(k) = ed.symbol_mut().timeline.layers[l_idx].key_at_mut(f_idx) {
            k.elements.push(Element {
                id: rect_id.clone(),
                name: "box".to_owned(),
                kind: ElementKind::Rectangle {
                    width: 80.0,
                    height: 60.0,
                    fill: [200, 100, 50, 255],
                    stroke: [20, 30, 40, 255],
                    stroke_width: 2.0,
                },
                ..Element::default()
            });
        }
    });
    editor.select_single_id(rect_id);
    let before_stroke = editor.project.clone();
    Editor::open_planned(&ctx, "Tool options ▸ Stroke style…");
    editor.drain_converted_entry_triggers(&ctx);
    assert!(has_stroke_style_dialog(&ctx));
    assert!(editor.confirm_stroke_style(&ctx));
    assert!(!has_stroke_style_dialog(&ctx));
    assert_eq!(editor.history.undo_label(), Some("Stroke style"));
    editor.project.validate().expect("valid after stroke style");
    editor.undo();
    assert_eq!(editor.project, before_stroke);

    // 4. Font embedding modal dialog + Undo/Redo + validation.
    let before_fonts = editor.project.clone();
    Editor::open_planned(&ctx, "Library menu ▸ Font embedding…");
    editor.drain_converted_entry_triggers(&ctx);
    assert!(has_font_embedding_dialog(&ctx));
    update_font_embedding_draft(&ctx, |draft| {
        draft.entries[0].family = "NeoFlash definitely missing font".into();
    });
    assert!(editor.confirm_font_embedding(&ctx).is_err());
    assert_eq!(
        editor.project, before_fonts,
        "failed font embedding mutated document"
    );
    assert!(
        has_font_embedding_dialog(&ctx),
        "failed font embedding closed dialog"
    );
    update_font_embedding_draft(&ctx, |draft| {
        draft.entries[0].family = "Inter".into();
    });
    set_inline_font_embedding_draft(&ctx, FontEmbeddingDraft::default());
    let embedded = editor.confirm_font_embedding(&ctx).expect("embed fonts");
    assert_eq!(embedded, 3);
    assert!(!has_font_embedding_dialog(&ctx));
    assert!(
        font_embedding_draft(&ctx).is_none(),
        "successful modal embedding must clear a stale inline draft"
    );
    assert_eq!(editor.history.undo_label(), Some("Font embedding"));
    editor
        .project
        .validate()
        .expect("valid after font embedding");
    let embedded_fonts: Vec<_> = editor
        .project
        .resources
        .iter()
        .filter(|resource| neoflash_export::is_font_resource(resource))
        .collect();
    assert_eq!(embedded_fonts.len(), 3);
    for font in embedded_fonts {
        let bytes = font.decoded_bytes().expect("embedded font bytes");
        ab_glyph::FontRef::try_from_slice_and_index(&bytes, 0)
            .expect("font dialog must embed a real renderable face");
    }
    assert!(
        editor
            .project
            .resources
            .iter()
            .any(|resource| { resource.path.eq_ignore_ascii_case("fonts/embedding.json") })
    );
    editor.undo();
    assert_eq!(editor.project, before_fonts);

    // 5. Bitmap fill swatch tile, Deco pattern, Spray brush burst + Undo/Redo.
    let _bmp_id = editor
        .apply_bitmap_swatch_tile(0)
        .expect("bitmap fill swatch tile");
    editor.project.validate().expect("valid after bitmap fill");
    let deco_added = editor.apply_deco_pattern(0, 100.0, 16.0);
    assert!(deco_added > 0);
    assert_eq!(editor.history.undo_label(), Some("Deco pattern"));
    editor.project.validate().expect("valid after deco pattern");
    editor.undo();

    let spray_added = editor.apply_spray_brush_burst(true, 100.0, 100.0);
    assert_eq!(spray_added, 5);
    assert_eq!(editor.history.undo_label(), Some("Spray brush"));
    editor.project.validate().expect("valid after spray brush");
    editor.undo();

    // 6. Stage context-menu Duplicate symbol & Swap symbol on an instance.
    let child_sym_id = new_id("sym");
    let mut child_sym = editor.symbol().clone();
    child_sym.id = child_sym_id.clone();
    child_sym.name = "Star".to_owned();
    child_sym.kind = SymbolKind::Graphic;
    child_sym.class_name = None;
    child_sym.linkage_id = None;
    crate::panels::scene::refresh_timeline_ids(&mut child_sym.timeline);
    editor.transact_named("Add child symbol", |ed| {
        ed.project.symbols.push(child_sym);
    });
    let inst_id = new_id("inst");
    editor.transact_named("Place instance", |ed| {
        let l_idx = ed.layer;
        let f_idx = ed.frame;
        if let Some(k) = ed.symbol_mut().timeline.layers[l_idx].key_at_mut(f_idx) {
            k.elements.push(Element {
                id: inst_id.clone(),
                name: "star_inst".to_owned(),
                kind: ElementKind::Instance {
                    symbol_id: child_sym_id.clone(),
                    playback: InstancePlayback::Loop,
                },
                ..Element::default()
            });
        }
    });
    editor.select_single_id(inst_id);
    let before_dup = editor.project.clone();
    let dup_sym_id = editor
        .duplicate_selected_instance_symbol()
        .expect("duplicates instance symbol");
    assert_ne!(dup_sym_id, child_sym_id);
    assert_eq!(editor.history.undo_label(), Some("Duplicate symbol"));
    editor
        .project
        .validate()
        .expect("valid after duplicate symbol");
    editor.library_selected = Some(child_sym_id.clone());
    assert!(editor.swap_selected_instance_with_library_or_next());
    assert_eq!(editor.history.undo_label(), Some("Swap symbol"));
    editor.undo();
    editor.undo();
    assert_eq!(editor.project, before_dup);
}

#[test]
fn t128_planned_features_brush_modes_lasso_focal_library_autosize_swatches_work() {
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::demo());

    // 1. Brush PaintBehind mode inserts merge-drawing stroke behind existing shapes.
    editor.transact_named("Clear layer", |ed| {
        let l_idx = ed.layer;
        let f_idx = ed.frame;
        if let Some(k) = ed.symbol_mut().timeline.layers[l_idx].key_at_mut(f_idx) {
            k.elements.clear();
        }
    });
    editor.workspace.drawing.object_drawing = false;
    editor.authoring_tools.brush_paint_mode = neoflash_core::BrushPaintMode::PaintNormal;
    editor.add_element(
        ElementKind::Rectangle {
            width: 100.0,
            height: 100.0,
            fill: [200, 40, 40, 255],
            stroke: [0, 0, 0, 0],
            stroke_width: 0.0,
        },
        0.0,
        0.0,
    );
    let first_id = editor.symbol().timeline.layers[editor.layer]
        .key_at(editor.frame)
        .unwrap()
        .elements[0]
        .id
        .clone();
    editor.tool = crate::app::Tool::Brush;
    editor.authoring_tools.brush_paint_mode = neoflash_core::BrushPaintMode::PaintBehind;
    editor.add_element(
        ElementKind::Path {
            commands: vec![
                PathCommand::MoveTo { x: 20.0, y: 20.0 },
                PathCommand::LineTo { x: 140.0, y: 20.0 },
                PathCommand::LineTo { x: 140.0, y: 140.0 },
                PathCommand::LineTo { x: 20.0, y: 140.0 },
                PathCommand::Close,
            ],
            fill_rule: FillRule::NonZero,
            fill: [40, 40, 200, 255],
            stroke: [0, 0, 0, 0],
            stroke_width: 0.0,
        },
        0.0,
        0.0,
    );
    {
        let k = editor.symbol().timeline.layers[editor.layer]
            .key_at(editor.frame)
            .expect("key");
        assert!(k.elements.len() >= 2);
        // The original red rect should remain on top of the behind-painted blue shape.
        assert_eq!(k.elements.last().unwrap().id, first_id);
    }

    // 2. Lasso partial-fill selection slices a merge-mode fill into inside/outside pieces + Undo.
    editor.undo(); // Undo PaintBehind
    let before_lasso = editor.project.clone();
    let inside_ids = editor.lasso_select_partial_fill(&[
        [40.0, 40.0],
        [120.0, 40.0],
        [120.0, 120.0],
        [40.0, 120.0],
    ]);
    assert!(
        !inside_ids.is_empty(),
        "lasso should slice and select inside fill piece"
    );
    assert_eq!(editor.history.undo_label(), Some("Lasso split geometry"));
    editor
        .project
        .validate()
        .expect("project valid after lasso partial fill");
    editor.undo();
    assert_eq!(editor.project, before_lasso);

    // 3. Radial gradient focal point handle updates focal_point_ratio in a single Undo step.
    editor.select_single_id(first_id.clone());
    editor.transact_named("Set radial gradient", |ed| {
        let l_idx = ed.layer;
        let f_idx = ed.frame;
        if let Some(k) = ed.symbol_mut().timeline.layers[l_idx].key_at_mut(f_idx)
            && let Some(el) = k.elements.iter_mut().find(|e| e.id == first_id)
        {
            el.kind = ElementKind::PaintedPath {
                commands: vec![
                    PathCommand::MoveTo { x: 0.0, y: 0.0 },
                    PathCommand::LineTo { x: 100.0, y: 0.0 },
                    PathCommand::LineTo { x: 100.0, y: 100.0 },
                    PathCommand::LineTo { x: 0.0, y: 100.0 },
                    PathCommand::Close,
                ],
                fill_rule: FillRule::NonZero,
                fill: [255, 255, 255, 255],
                stroke: [0, 0, 0, 0],
                stroke_width: 0.0,
                fill_gradient: Some(neoflash_core::GradientPaint {
                    kind: neoflash_core::GradientKind::Radial,
                    matrix: [0.06, 0.0, 0.0, 0.06, 50.0, 50.0],
                    stops: vec![
                        neoflash_core::GradientStop {
                            ratio: 0,
                            color: [255, 255, 255, 255],
                        },
                        neoflash_core::GradientStop {
                            ratio: 255,
                            color: [0, 0, 0, 255],
                        },
                    ],
                    focal_point_ratio: 0.0,
                }),
                stroke_gradient: None,
            };
        }
    });
    let before_focal = editor.project.clone();
    assert!(editor.set_selected_gradient_focal_point(0.65));
    {
        let k = editor.symbol().timeline.layers[editor.layer]
            .key_at(editor.frame)
            .unwrap();
        let el = k.elements.iter().find(|e| e.id == first_id).unwrap();
        match &el.kind {
            ElementKind::PaintedPath {
                fill_gradient: Some(fill),
                ..
            } => assert!((fill.focal_point_ratio - 0.65).abs() < 1e-6),
            other => panic!("expected radial gradient PaintedPath, got {other:?}"),
        }
    }
    editor.undo();
    assert_eq!(editor.project, before_focal);

    // 4. Library drag-drop folder organization & Shared Library linkage + synchronization.
    let sym_id = new_id("sym_drag");
    let mut sym = editor.symbol().clone();
    sym.id = sym_id.clone();
    sym.name = "Badge".to_owned();
    sym.kind = SymbolKind::MovieClip;
    sym.class_name = None;
    sym.linkage_id = Some("shared.Badge".to_owned());
    crate::panels::scene::refresh_timeline_ids(&mut sym.timeline);
    editor.transact_named("Add symbol", |ed| {
        ed.project.symbols.push(sym);
        ed.project.library_folders.push("UI".to_owned());
        ed.project.library_folders.push("Archive".to_owned());
    });
    assert!(
        editor.drag_drop_symbol_to_folder(&sym_id, "UI"),
        "{}",
        editor.status
    );
    assert_eq!(editor.history.undo_label(), Some("Move symbol"));
    assert_eq!(
        editor
            .project
            .symbols
            .iter()
            .find(|s| s.id == sym_id)
            .unwrap()
            .folder,
        "UI"
    );
    assert!(editor.drag_drop_folder_to_folder("UI", "Archive"));
    assert_eq!(editor.history.undo_label(), Some("Move folder"));
    assert_eq!(
        editor
            .project
            .symbols
            .iter()
            .find(|s| s.id == sym_id)
            .unwrap()
            .folder,
        "Archive/UI"
    );

    let linkage = neoflash_core::SharedLibraryLinkage {
        symbol_id: sym_id.clone(),
        url: "shared_ui.nflash".to_owned(),
        export_for_runtime: false,
        import_for_runtime: true,
        always_update: true,
    };
    assert!(editor.set_symbol_shared_library(&sym_id, Some(linkage.clone())));
    assert_eq!(
        editor.project.shared_library(&sym_id),
        Some(linkage.clone())
    );
    let mut donor = Project::blank();
    let mut donor_sym = donor.symbols[0].clone();
    donor_sym.id = new_id("donor_sym");
    donor_sym.name = "DonorBadge".to_owned();
    donor_sym.kind = SymbolKind::Button;
    donor_sym.linkage_id = Some("shared.Badge".to_owned());
    donor.symbols.push(donor_sym.clone());
    donor.set_shared_library(
        &donor_sym.id,
        Some(neoflash_core::SharedLibraryLinkage {
            symbol_id: donor_sym.id.clone(),
            url: "shared_ui.nflash".to_owned(),
            export_for_runtime: true,
            import_for_runtime: false,
            always_update: true,
        }),
    );
    assert_eq!(editor.sync_shared_library(&donor), 1);
    assert_eq!(editor.history.undo_label(), Some("Sync shared library"));
    assert_eq!(
        editor
            .project
            .symbols
            .iter()
            .find(|s| s.id == sym_id)
            .unwrap()
            .kind,
        SymbolKind::Button
    );

    // 5. Authored TextBox auto-size resizes and anchors in a single Undo step.
    editor.add_element(
        ElementKind::TextBox {
            text: "Auto-sized headline".to_owned(),
            width: 240.0,
            height: 60.0,
            word_wrap: false,
            multiline: false,
            format: neoflash_core::TextFormat::default(),
        },
        0.0,
        0.0,
    );
    let text_id = editor.symbol().timeline.layers[editor.layer]
        .key_at(editor.frame)
        .unwrap()
        .elements
        .last()
        .unwrap()
        .id
        .clone();
    editor.select_single_id(text_id);
    let before_auto = editor.project.clone();
    let (new_w, new_h) = editor
        .set_selected_text_auto_size("center")
        .expect("auto-size center succeeds");
    assert!(new_w > 8.0 && new_w < 240.0);
    assert!(new_h > 8.0);
    assert_eq!(editor.history.undo_label(), Some("Text auto-size"));
    editor.undo();
    assert_eq!(editor.project, before_auto);

    // 6. Document swatches persistence & AuthoredPlayback::set_looping(false).
    editor.project.set_custom_swatches(&[[12, 34, 56, 255]]);
    assert!(
        editor
            .project
            .custom_swatches()
            .contains(&[12, 34, 56, 255])
    );

    let root_id = editor.project.root.clone();
    let mut playback = neoflash_core::AuthoredPlayback::new(&editor.project, &root_id, 0);
    assert!(playback.is_looping());
    playback.set_looping(false);
    assert!(!playback.is_looping());
}

#[test]
fn t129_editor_resource_and_library_mutations_are_atomic() {
    let ctx = egui::Context::default();
    let mut project = Project::demo();
    project.format_version = project.format_version.max(3);
    project.resources.push(
        ResourceUnit::from_bytes(
            "unrelated-image",
            "bitmaps/photo.png",
            "image/png",
            b"unrelated image bytes",
        )
        .unwrap(),
    );
    project.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: "bitmap-target".into(),
            name: "Bitmap target".into(),
            kind: ElementKind::Rectangle {
                width: 40.0,
                height: 30.0,
                fill: [10, 20, 30, 255],
                stroke: [0; 4],
                stroke_width: 0.0,
            },
            ..Element::default()
        });
    let mut editor = Editor::from_project(&ctx, project);
    editor.select_single_id("bitmap-target".into());
    let before_bitmap = editor.project.clone();
    let tile_id = editor
        .apply_bitmap_swatch_tile(0)
        .expect("apply procedural bitmap tile");
    assert_ne!(tile_id, "unrelated-image");
    assert!(editor.project.resources.iter().any(|resource| {
        resource.id == tile_id && resource.path.eq_ignore_ascii_case("bitmaps/tile_1.png")
    }));
    let key = editor.symbol().timeline.layers[0]
        .key_at(editor.frame)
        .unwrap();
    assert!(
        key.bitmap_fills
            .iter()
            .any(|fill| fill.element_id == "bitmap-target" && fill.resource_id == tile_id)
    );
    assert_eq!(editor.history.undo_label(), Some("Bitmap fill"));
    editor.undo();
    assert_eq!(
        editor.project, before_bitmap,
        "one Undo must remove both the procedural tile and its binding"
    );

    let mut symbol = editor.symbol().clone();
    symbol.id = "library-symbol".into();
    symbol.name = "Library symbol".into();
    symbol.kind = SymbolKind::Graphic;
    symbol.class_name = None;
    symbol.linkage_id = None;
    symbol.folder = "Source".into();
    crate::panels::scene::refresh_timeline_ids(&mut symbol.timeline);
    editor.transact_named("Library fixture", |editor| {
        editor.project.symbols.push(symbol);
        editor.project.library_folders.extend([
            "Source".into(),
            "Destination".into(),
            "Destination/Source".into(),
        ]);
        editor.project.format_version = editor.project.format_version.max(4);
    });
    let before_failed_moves = editor.project.clone();
    assert!(!editor.drag_drop_folder_to_folder("Source", "Destination"));
    let root_id = editor.project.root.clone();
    assert!(!editor.drag_drop_symbol_to_folder(&root_id, "Destination"));
    assert_eq!(editor.project, before_failed_moves);

    let font = ResourceUnit::from_bytes(
        "res_preserved_font",
        "fonts/Preserved.ttf",
        "font/ttf",
        b"\x00\x01\x00\x00original-font-bytes",
    )
    .unwrap();
    editor.transact_named("Font fixture", |editor| {
        editor.project.resources.push(font.clone());
        editor.project.format_version = editor.project.format_version.max(3);
    });
    editor.open_font_embedding(&ctx);
    assert_eq!(
        editor.confirm_font_embedding(&ctx),
        Ok(1),
        "{}",
        editor.status
    );
    assert_eq!(
        editor
            .project
            .resources
            .iter()
            .find(|resource| resource.id == font.id),
        Some(&font),
        "font dialog must preserve imported font bytes when the face is unchanged"
    );

    editor.open_font_embedding(&ctx);
    crate::panels::font_embedding::update_font_embedding_draft(&ctx, |draft| {
        draft.entries[0].family = "Inter".into();
        draft.entries[0].style = "Regular".into();
    });
    assert_eq!(editor.confirm_font_embedding(&ctx), Ok(1));
    let changed = editor
        .project
        .resources
        .iter()
        .find(|resource| resource.id == font.id)
        .expect("changed font keeps its resource identity");
    assert_ne!(
        changed.data_base64, font.data_base64,
        "changing the selected face must replace stale imported bytes"
    );
    let changed_bytes = changed.decoded_bytes().unwrap();
    ab_glyph::FontRef::try_from_slice_and_index(&changed_bytes, 0)
        .expect("changed face embeds a real font");
    editor.project.validate().unwrap();
}
