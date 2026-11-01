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
            editor.dock_grid(ctx);
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
    let item = named_bounds(&out, "Subselection").expect("flyout lists the group");
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
        let keys: Vec<_> = out
            .shapes
            .iter()
            .filter_map(|s| key_marker(s, &ctx).map(|c| c.y))
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
                    && text.pos.x < neoflash_ui::metrics::LAYER_COLUMN
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
                (center.y - row_top - crate::timeline_paint::ROW_HEIGHT / 2.).abs() < 1.
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
        // The NF2 column starts at 280 (near its 320 maximum), so narrow it.
        assert_eq!(width, neoflash_ui::metrics::LAYER_COLUMN);
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
    editor.restore_workspace(&ctx);
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
            "Onion skin options",
        ] {
            let bounds = named_bounds(&output, label)
                .unwrap_or_else(|| panic!("missing AX control {label}"));
            assert!(
                viewport.contains_rect(bounds),
                "{mode:?} {label}: {bounds:?}"
            );
        }
        let options = named_bounds(&output, "Onion skin options")
            .unwrap()
            .center();
        detached_timeline_frame(
            &mut editor,
            &ctx,
            size,
            0.2,
            vec![Event::PointerMoved(options), primary_button(options, true)],
        );
        output = detached_timeline_frame(
            &mut editor,
            &ctx,
            size,
            0.3,
            vec![primary_button(options, false)],
        );
        for pass in 1..4 {
            output =
                detached_timeline_frame(&mut editor, &ctx, size, 0.3 + pass as f64 / 60., vec![]);
        }
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
    let options = named_bounds(&initial, "Onion skin options")
        .expect("the pinned onion group exposes its options popover")
        .center();
    detached_timeline_frame(
        &mut editor,
        &ctx,
        size,
        0.2,
        vec![Event::PointerMoved(options), primary_button(options, true)],
    );
    let mut output = detached_timeline_frame(
        &mut editor,
        &ctx,
        size,
        0.3,
        vec![primary_button(options, false)],
    );
    for pass in 1..4 {
        output = detached_timeline_frame(&mut editor, &ctx, size, 0.3 + pass as f64 / 60., vec![]);
    }
    let bounds = named_bounds(&output, "Onion opacity")
        .expect("Opacity is available through the onion options popover");
    assert!(viewport.contains_rect(bounds), "{bounds:?}");
    let opacity = editor.onion_opacity;
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
        editor.onion_opacity, opacity,
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

/// Design §8: every menu-bar dropdown shows its designed rows, and a preview row opens its planned
/// panel (Edit ▸ Find and replace → Find and replace preview).
#[test]
fn menu_bar_follows_design_and_opens_planned_previews() {
    use crate::panels::planned::{self, PlannedPanel};
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut editor = Editor::from_project(&ctx, Project::demo());
    let size = Vec2::new(1920., 1080.);
    let mut time = 0.1;
    let mut step = |editor: &mut Editor, events: Vec<Event>| {
        time += 0.1;
        frame(editor, &ctx, size, time, events)
    };
    let mut out = step(&mut editor, vec![]);
    let escape = || Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    let menus: [(&str, &[&str]); 8] = [
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
        (
            "Modify",
            &["Document… (⌘J)", "Break apart (⌘B)", "Ungroup (⇧⌘G)"],
        ),
        (
            "Control",
            &["Go to end (⌥⇧.)", "Test scene (⌥⌘↵)", "Mute sounds (⌥⌘M)"],
        ),
        (
            "Window",
            &["Swatches (⌘F9)", "Compiler errors (⌥F2)", "Movie Explorer"],
        ),
        ("Help", &["Open example", "About NeoFlash"]),
    ];
    for (title, rows) in menus {
        let at = named_bounds(&out, title).unwrap().center();
        step(
            &mut editor,
            vec![Event::PointerMoved(at), primary_button(at, true)],
        );
        step(&mut editor, vec![primary_button(at, false)]);
        out = step(&mut editor, vec![]);
        for row in rows {
            assert!(named_bounds(&out, row).is_some(), "{title} ▸ {row}");
        }
        if title == "Edit" {
            let find = named_bounds(&out, "Find and replace (⌘F)")
                .unwrap()
                .center();
            step(
                &mut editor,
                vec![Event::PointerMoved(find), primary_button(find, true)],
            );
            step(&mut editor, vec![primary_button(find, false)]);
            assert!(planned::is_open(&ctx, PlannedPanel::FindReplace));
        } else {
            step(&mut editor, vec![escape()]);
        }
        out = step(&mut editor, vec![]);
    }
}

/// Design §8 stage-object menu opens on right-click over the Stage with its designed rows.
#[test]
fn stage_context_menu_follows_design() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut editor = Editor::from_project(&ctx, Project::demo());
    let size = Vec2::new(1920., 1080.);
    frame(&mut editor, &ctx, size, 0.1, vec![]);
    // A point on the canvas (between the tools rail and the inspector, above the timeline).
    let at = Pos2::new(760., 300.);
    let secondary = |pressed| Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Secondary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    frame(&mut editor, &ctx, size, 0.2, vec![Event::PointerMoved(at)]);
    frame(&mut editor, &ctx, size, 0.3, vec![secondary(true)]);
    frame(&mut editor, &ctx, size, 0.4, vec![secondary(false)]);
    let out = frame(&mut editor, &ctx, size, 0.5, vec![]);
    for row in [
        "Paste in place (⇧⌘V)",
        "Convert to symbol… (F8)",
        "Create motion tween",
        "Distribute to layers (⇧⌘D)",
        "Show in library",
    ] {
        assert!(named_bounds(&out, row).is_some(), "Stage menu ▸ {row}");
    }
}

/// DL-01 Preferences ▸ General ▸ Theme: choosing Light is a draft until Save; Cancel discards it, Reset to
/// defaults restores Dark, and the project never changes.
#[test]
fn preferences_theme_applies_on_save_and_cancel_discards() {
    use crate::panels::planned::{self, PlannedPanel};
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    editor.set_theme(&ctx, ThemeMode::Dark);
    let project = editor.project.clone();
    let mut time = 0.0;
    let mut step = |editor: &mut Editor, events: Vec<Event>| {
        time += 0.1;
        ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1920., 1080.))),
                time: Some(time),
                events,
                ..Default::default()
            },
            |ctx| editor.planned_panels(ctx),
        )
    };
    let mut click = |editor: &mut Editor, label: &str| {
        let out = step(editor, vec![]);
        let at = named_bounds(&out, label)
            .unwrap_or_else(|| panic!("{label} is not shown"))
            .center();
        step(
            editor,
            vec![Event::PointerMoved(at), primary_button(at, true)],
        );
        step(editor, vec![primary_button(at, false)]);
        step(editor, vec![]);
    };

    planned::set_open(&ctx, PlannedPanel::Preferences, true);
    click(&mut editor, "Light");
    assert_eq!(editor.workspace.theme, ThemeMode::Dark, "draft only");
    assert_eq!(ctx.theme(), egui::Theme::Dark);
    click(&mut editor, "Cancel");
    assert!(!planned::is_open(&ctx, PlannedPanel::Preferences));
    assert_eq!(editor.workspace.theme, ThemeMode::Dark);
    assert_eq!(planned::planned_state(&ctx).theme_draft, None);

    planned::set_open(&ctx, PlannedPanel::Preferences, true);
    click(&mut editor, "Light");
    click(&mut editor, "Save");
    assert!(!planned::is_open(&ctx, PlannedPanel::Preferences));
    assert_eq!(editor.workspace.theme, ThemeMode::Light);
    assert_eq!(ctx.theme(), egui::Theme::Light);

    planned::set_open(&ctx, PlannedPanel::Preferences, true);
    click(&mut editor, "Reset to defaults");
    assert_eq!(editor.workspace.theme, ThemeMode::Light, "draft only");
    click(&mut editor, "Save");
    assert_eq!(editor.workspace.theme, ThemeMode::Dark);
    assert_eq!(ctx.theme(), egui::Theme::Dark);
    assert_eq!(editor.project, project);
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
            editor.dock_grid(ctx);
            editor.stage(ctx);
            editor.draw_dialogs(ctx);
            editor.symbol_creation_dialog(ctx);
            editor.detached_panels(ctx);
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
    /// Bounds of the control named `label`; dock tabs are skipped (menu rows share panel names).
    fn find(&self, label: &str) -> Option<Rect> {
        self.out
            .platform_output
            .accesskit_update
            .as_ref()?
            .nodes
            .iter()
            .find_map(|(_, node)| {
                (node.label() == Some(label) && node.role() != egui::accesskit::Role::Tab)
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
        PanelId::Properties => "Properties (⌘F3)",
        PanelId::Library => "Library (⌘L)",
        PanelId::Actions => "Actions (F9)",
        PanelId::Align => "Align (⌘K)",
        PanelId::Color => "Color (⇧⌘F9)",
        PanelId::Transform => "Transform (⌘T)",
        PanelId::Output => "Output (F2)",
        PanelId::Accessibility => "Accessibility (⇧F11)",
        PanelId::Ease => "Ease",
        PanelId::Scene => "Scene (⇧F2)",
        PanelId::MotionEditor => "Motion editor",
    };

    // Real panels: Window menu opens and closes; the tab menu's Hide closes too.
    for panel in PanelId::ALL {
        d.editor.set_panel_visible(panel, false);
        d.settle();
        d.click("Window");
        d.click(window_row(panel));
        assert!(
            d.editor.workspace.is_panel_visible(panel),
            "Window ▸ {panel:?} opens"
        );
        assert!(d.tab(panel).is_some(), "{panel:?} tab is drawn");
        d.click("Window");
        d.click(window_row(panel));
        assert!(
            !d.editor.workspace.is_panel_visible(panel),
            "Window ▸ {panel:?} closes"
        );
        assert!(d.tab(panel).is_none(), "{panel:?} tab disappears");

        d.click("Window");
        d.click(window_row(panel));
        assert!(d.editor.workspace.is_panel_visible(panel));
        let tab = d.tab(panel).unwrap();
        d.press(tab.center(), egui::PointerButton::Secondary);
        d.click("Hide");
        assert!(
            !d.editor.workspace.is_panel_visible(panel),
            "{panel:?} tab menu ▸ Hide closes"
        );
    }
    d.editor.restore_workspace(&d.ctx);
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
            // Popovers have no header: a click outside dismisses them.
            Chrome::Popover => Some(Rect::from_center_size(Pos2::new(1200., 500.), Vec2::ZERO)),
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
    assert!(d.editor.confirm.is_some(), "Open example asks to save");
    d.click("Cancel");
    assert!(
        d.editor.confirm.is_none(),
        "confirmation closes with Cancel"
    );
    assert_eq!(d.editor.project, edited);
}
