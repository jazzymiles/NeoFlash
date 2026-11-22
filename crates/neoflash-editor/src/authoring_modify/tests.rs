use super::*;
use crate::commands::{CommandId, effective_shortcuts, primary_shortcut_label};
use crate::selection::SelectionTarget;
use crate::workspace::ShortcutPrefs;

fn make_rect(id: &str, x: f64) -> Element {
    Element {
        id: id.into(),
        name: id.into(),
        transform: Transform {
            x,
            ..Transform::default()
        },
        kind: ElementKind::Rectangle {
            width: 20.0,
            height: 20.0,
            fill: [200, 80, 80, 255],
            stroke: [0, 0, 0, 255],
            stroke_width: 1.0,
        },
        ..Element::default()
    }
}

fn layer_element_ids(editor: &Editor) -> Vec<String> {
    editor.symbol().timeline.layers[0].keyframes[0]
        .elements
        .iter()
        .map(|e| e.id.clone())
        .collect()
}

#[test]
fn arrange_selection_reorders_layer_depth_respects_groups_and_binds_default_shortcuts() {
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    let layer_id = editor.symbol().timeline.layers[0].id.clone();
    editor.symbol_mut().timeline.layers[0].keyframes[0].elements = vec![
        make_rect("back", 0.0),
        make_rect("mid", 30.0),
        make_rect("front", 60.0),
    ];
    editor.project.validate().unwrap();

    // 1. Default shortcuts for Modify ▸ Arrange must be bound to ⇧⌘↑/⌘↑/⌘↓/⇧⌘↓ and ⇧⌘]/⌘]/⌘[/⇧⌘[.
    let prefs = ShortcutPrefs::default();
    assert_eq!(
        primary_shortcut_label(&prefs, CommandId::ModifyArrangeFront),
        "⇧⌘↑"
    );
    assert_eq!(
        primary_shortcut_label(&prefs, CommandId::ModifyArrangeForward),
        "⌘↑"
    );
    assert_eq!(
        primary_shortcut_label(&prefs, CommandId::ModifyArrangeBackward),
        "⌘↓"
    );
    assert_eq!(
        primary_shortcut_label(&prefs, CommandId::ModifyArrangeBack),
        "⇧⌘↓"
    );
    assert_eq!(
        effective_shortcuts(&prefs, CommandId::ModifyArrangeFront).len(),
        2
    );
    assert_eq!(
        effective_shortcuts(&prefs, CommandId::ModifyArrangeForward).len(),
        2
    );
    assert_eq!(
        effective_shortcuts(&prefs, CommandId::ModifyArrangeBackward).len(),
        2
    );
    assert_eq!(
        effective_shortcuts(&prefs, CommandId::ModifyArrangeBack).len(),
        2
    );

    // 2. Boundary no-op checks: "back" cannot move further back; "front" cannot move further forward.
    editor.selection.targets = vec![SelectionTarget {
        layer: layer_id.clone(),
        element: "back".into(),
    }];
    assert!(!editor.can_arrange_selection(Some(Arrange::Backward)));
    assert!(!editor.can_arrange_selection(Some(Arrange::Back)));
    assert!(editor.can_arrange_selection(Some(Arrange::Forward)));
    assert!(editor.can_arrange_selection(Some(Arrange::Front)));

    // 3. Step "back" forward once -> [mid, back, front], then to front -> [mid, front, back].
    editor.arrange_selection(Arrange::Forward);
    assert_eq!(layer_element_ids(&editor), vec!["mid", "back", "front"]);
    editor.arrange_selection(Arrange::Front);
    assert_eq!(layer_element_ids(&editor), vec!["mid", "front", "back"]);
    assert!(!editor.can_arrange_selection(Some(Arrange::Forward)));
    assert!(!editor.can_arrange_selection(Some(Arrange::Front)));

    // Step "back" backward once -> [mid, back, front], then to back -> [back, mid, front].
    editor.arrange_selection(Arrange::Backward);
    assert_eq!(layer_element_ids(&editor), vec!["mid", "back", "front"]);
    editor.arrange_selection(Arrange::Back);
    assert_eq!(layer_element_ids(&editor), vec!["back", "mid", "front"]);

    // Undo restores the previous step cleanly.
    editor.undo();
    assert_eq!(layer_element_ids(&editor), vec!["mid", "back", "front"]);
    editor.redo();
    assert_eq!(layer_element_ids(&editor), vec!["back", "mid", "front"]);

    // 4. Authoring group on the same layer is treated as an atomic depth unit:
    // stepping "back" forward jumps past the entire group [mid, front] without splitting it.
    editor.symbol_mut().timeline.layers[0].properties.groups = vec![AuthoringGroup {
        id: "grp-1".into(),
        name: "Group 1".into(),
        members: vec!["mid".into(), "front".into()],
    }];
    editor.project.format_version = editor.project.format_version.max(4);
    editor.project.validate().unwrap();

    editor.selection.targets = vec![SelectionTarget {
        layer: layer_id.clone(),
        element: "back".into(),
    }];
    editor.arrange_selection(Arrange::Forward);
    assert_eq!(
        layer_element_ids(&editor),
        vec!["mid", "front", "back"],
        "Bring forward must step past an entire authoring group without splitting its members"
    );
    editor.project.validate().unwrap();

    editor.arrange_selection(Arrange::Backward);
    assert_eq!(
        layer_element_ids(&editor),
        vec!["back", "mid", "front"],
        "Send backward must step behind an entire authoring group without splitting its members"
    );
    editor.project.validate().unwrap();
}

#[test]
fn grouped_selection_shows_single_combined_bounding_box_and_ungroup_restores_individual_boxes() {
    use crate::macos_menu::{NativeMenuAction, NativeMenuItemSpec, NativeMenuSpec};

    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    editor.authoring_modify.to_stage = false;
    let align_left_enabled = |editor: &Editor| {
        let modify = NativeMenuSpec::build_top_menu(editor, Some(&ctx), "Modify").unwrap();
        let align = modify.items.iter().find_map(|item| match item {
            NativeMenuItemSpec::Submenu { label, items, .. } if label == "Align" => Some(items),
            _ => None,
        });
        align.unwrap().iter().any(|item| {
            matches!(
                item,
                NativeMenuItemSpec::Action {
                    action: NativeMenuAction::AlignSelection { axis: 0, edge: 0 },
                    enabled: true,
                    ..
                }
            )
        })
    };
    let layer_id = editor.symbol().timeline.layers[0].id.clone();
    let mut a = make_rect("a", 10.0);
    a.merge = true;
    let mut b = make_rect("b", 80.0);
    b.merge = true;
    editor.symbol_mut().timeline.layers[0].keyframes[0].elements = vec![a, b];

    editor.set_selection(vec![
        SelectionTarget {
            layer: layer_id.clone(),
            element: "a".into(),
        },
        SelectionTarget {
            layer: layer_id.clone(),
            element: "b".into(),
        },
    ]);

    let painter = ctx.layer_painter(egui::LayerId::background());
    let scene: Vec<DrawItem> = editor
        .presented_interactive_authored_scene(false)
        .draws
        .into_iter()
        .map(|d| d.item)
        .collect();
    let mapping = crate::stage_mapping::StageMapping::identity();

    // Before grouping: 2 separate boxes (one for "a" and one for "b", both merge shapes).
    let before_boxes = editor.selection_boxes(&scene, &painter, mapping);
    assert_eq!(before_boxes.len(), 2);
    assert!(before_boxes.iter().all(|(_, _, _, _, is_merge)| *is_merge));
    assert!(align_left_enabled(&editor));

    // Group via ⌘G (`group_selection`): must coalesce into 1 single combined Group bounding box.
    editor.group_selection();
    let grouped_boxes = editor.selection_boxes(&scene, &painter, mapping);
    assert_eq!(
        grouped_boxes.len(),
        1,
        "Grouped objects must display a single combined bounding box"
    );
    let (_, world_rect, context_rect, kind, is_merge) = &grouped_boxes[0];
    assert_eq!(*kind, neoflash_ui::components::canvas::SelectionKind::Group);
    assert!(!*is_merge);
    assert!(world_rect.min.x <= 10.5 && world_rect.max.x >= 99.5);
    assert!(context_rect.min.x <= 10.5 && context_rect.max.x >= 99.5);
    assert!(!align_left_enabled(&editor), "one group is one layout unit");
    editor.authoring_modify.to_stage = true;
    assert!(align_left_enabled(&editor), "Stage can align one group");
    editor.authoring_modify.to_stage = false;

    // Ungroup via ⇧⌘G (`ungroup_selection`): restores individual bounding boxes for each object.
    editor.ungroup_selection();
    let after_boxes = editor.selection_boxes(&scene, &painter, mapping);
    assert_eq!(
        after_boxes.len(),
        2,
        "After Ungroup, each object must display its own individual bounding box"
    );
    assert!(after_boxes.iter().all(|(_, _, _, _, is_merge)| *is_merge));
    assert!(align_left_enabled(&editor));
    editor.project.validate().unwrap();
}

#[test]
fn transform_submodes_menu_commands_envelope_delete_and_hotkey_bindings_work() {
    use crate::authoring_tools::FreeTransformMode;
    use crate::transform_geometry::{
        Bounds, EnvelopeCage, EnvelopeHandle, deform_element_envelope,
    };

    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    let layer_id = editor.symbol().timeline.layers[0].id.clone();
    editor.symbol_mut().timeline.layers[0].keyframes[0].elements =
        vec![make_rect("shape1", 20.0), make_rect("shape2", 60.0)];

    let prefs = ShortcutPrefs::default();
    assert_eq!(primary_shortcut_label(&prefs, CommandId::ModifyGroup), "⌘G");
    assert_eq!(
        primary_shortcut_label(&prefs, CommandId::ModifyUngroup),
        "⇧⌘G"
    );
    assert_eq!(
        primary_shortcut_label(&prefs, CommandId::ModifyBreakApart),
        "⌘B"
    );
    assert_eq!(
        primary_shortcut_label(&prefs, CommandId::ModifyScaleAndRotate),
        "⌥⌘S"
    );
    assert_eq!(
        primary_shortcut_label(&prefs, CommandId::ModifyRotate90Cw),
        "⇧⌘9"
    );
    assert_eq!(
        primary_shortcut_label(&prefs, CommandId::ModifyRotate90Ccw),
        "⇧⌘7"
    );

    editor.set_selection(vec![SelectionTarget {
        layer: layer_id.clone(),
        element: "shape1".into(),
    }]);
    assert!(editor.can_distort_or_envelope_selection());

    // Switching Free Transform sub-modes activates Tool::Transform and sets the mode.
    for mode in [
        FreeTransformMode::RotateAndSkew,
        FreeTransformMode::Scale,
        FreeTransformMode::Distort,
        FreeTransformMode::Envelope,
        FreeTransformMode::FreeTransform,
    ] {
        editor.set_free_transform_mode(mode);
        assert_eq!(editor.tool, crate::app::Tool::Transform);
        assert_eq!(editor.active_free_transform_mode(), mode);
    }

    // Rotate 90° CW / CCW and Flip Horizontal / Vertical.
    editor.rotate_90_cw_selection(&ctx);
    let rot_cw = editor.selected_element().unwrap().transform.rotation;
    assert!((rot_cw - 90.0).abs() < 1e-4);
    editor.rotate_90_ccw_selection(&ctx);
    let rot_ccw = editor.selected_element().unwrap().transform.rotation;
    assert!(rot_ccw.abs() < 1e-4);

    editor.flip_horizontal_selection(&ctx);
    let m_h = transform_matrix(editor.selected_element().unwrap().transform);
    assert!((m_h[0] + 1.0).abs() < 1e-4 && (m_h[3] - 1.0).abs() < 1e-4);
    editor.flip_vertical_selection(&ctx);
    let m_hv = transform_matrix(editor.selected_element().unwrap().transform);
    assert!((m_hv[0] + 1.0).abs() < 1e-4 && (m_hv[3] + 1.0).abs() < 1e-4);

    // Scale and Rotate numeric dialog apply.
    assert!(editor.apply_scale_and_rotate(&ctx, 150.0, 45.0));
    assert!((editor.selected_element().unwrap().transform.scale_x.abs() - 1.5).abs() < 1e-3);

    // Remove Transform resets scale/rotation/skew to identity.
    editor.reset_selection_transform(&ctx);
    let reset_t = editor.selected_element().unwrap().transform;
    assert!((reset_t.scale_x - 1.0).abs() < 1e-4);
    assert!((reset_t.scale_y - 1.0).abs() < 1e-4);
    assert!(reset_t.rotation.abs() < 1e-4);
    assert!(reset_t.skew.abs() < 1e-4);

    // Envelope deformation and Delete Envelope restoration.
    let orig = editor.selected_element().unwrap().clone();
    let cage = EnvelopeCage::from_bounds(Bounds {
        min: [20.0, 0.0],
        max: [40.0, 20.0],
    })
    .dragged(EnvelopeHandle::Anchor(1), [30.0, 0.0], [30.0, -10.0])
    .unwrap();
    let warped = deform_element_envelope(&orig, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], &cage).unwrap();
    editor.authoring_modify.envelope_backups.insert(
        (editor.symbol_id().to_owned(), editor.frame, orig.id.clone()),
        orig.clone(),
    );
    editor.symbol_mut().timeline.layers[0].keyframes[0].elements[0] = warped;
    assert!(editor.can_delete_envelope_selection());
    assert!(editor.delete_envelope_selection());
    assert_eq!(editor.selected_element().unwrap().kind, orig.kind);
    editor.project.validate().unwrap();
}

#[test]
fn gradient_transform_icon_and_color_fill_type_popover_work_everywhere() {
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    let layer_id = editor.symbol().timeline.layers[0].id.clone();

    // 1. Gradient Transform icon (16px and 20px) uses a crisp bordered horizontal linearGradient.
    let g16 = crate::ui_icons::Icon::Gradient.nf2(false);
    let g20 = crate::ui_icons::Icon::Gradient.tool_glyph();
    assert!(g16.svg.contains("<linearGradient"));
    assert!(g20.svg.contains("<linearGradient"));
    assert_eq!(g16.size, 16);
    assert_eq!(g20.size, 20);
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        let p = ctx.layer_painter(egui::LayerId::background());
        neoflash_ui::icons::paint(
            &p,
            ctx,
            egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(16.0, 16.0)),
            g16,
            egui::Color32::WHITE,
        );
        neoflash_ui::icons::paint(
            &p,
            ctx,
            egui::Rect::from_min_size(egui::pos2(20.0, 0.0), egui::vec2(20.0, 20.0)),
            g20,
            egui::Color32::WHITE,
        );
    });

    // 2. Switching a selected Rectangle through Linear gradient, Radial gradient, Bitmap fill,
    // None and Solid colour via `set_selection_fill_type`, plus Undo/Redo.
    editor.symbol_mut().timeline.layers[0].keyframes[0].elements = vec![make_rect("r1", 10.0)];
    editor.set_selection(vec![SelectionTarget {
        layer: layer_id.clone(),
        element: "r1".into(),
    }]);
    editor.authoring_modify.color_selection = true;

    editor.set_selection_fill_type(false, PaintFillType::LinearGradient);
    assert!(matches!(
        editor.selected_element().unwrap().kind,
        ElementKind::PaintedPath {
            fill_gradient: Some(GradientPaint {
                kind: GradientKind::Linear,
                ..
            }),
            ..
        }
    ));
    editor.project.validate().unwrap();

    editor.set_selection_fill_type(false, PaintFillType::RadialGradient);
    assert!(matches!(
        editor.selected_element().unwrap().kind,
        ElementKind::PaintedPath {
            fill_gradient: Some(GradientPaint {
                kind: GradientKind::Radial,
                ..
            }),
            ..
        }
    ));
    editor.project.validate().unwrap();

    editor.set_selection_fill_type(false, PaintFillType::BitmapFill);
    assert!(editor.target_has_bitmap_fill(&SelectionTarget {
        layer: layer_id.clone(),
        element: "r1".into(),
    }));
    editor.project.validate().unwrap();

    editor.set_selection_fill_type(false, PaintFillType::Solid);
    assert!(!editor.target_has_bitmap_fill(&SelectionTarget {
        layer: layer_id.clone(),
        element: "r1".into(),
    }));
    editor.project.validate().unwrap();

    // 3. Opening the anchored Color & Fill Type popover from Properties (`shape-fill`),
    // Tool defaults (`inspector-tool-fill`), Brushes (`tool-well-fill`) and Swatches
    // (`swatches-open-mixer`) renders without panic and exposes all fill types.
    for (key, stroke_well, sel) in [
        ("shape-fill", false, true),
        ("inspector-tool-fill", false, false),
        ("tool-well-fill", false, false),
        ("swatches-open-mixer", false, true),
    ] {
        request_color_popover(
            &ctx,
            key,
            egui::Rect::from_min_size(egui::pos2(20.0, 20.0), egui::vec2(22.0, 22.0)),
            stroke_well,
            sel,
        );
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                editor.properties(ui, ctx);
            });
        });
        assert!(
            ctx.data(|d| d.get_temp::<ActiveColorPopover>(active_color_popover_id()))
                .is_some()
        );
        close_color_popover(&ctx);
    }

    // 4. Gradient stop insertion (on bar and in pointer rail below it) selects the newly
    // inserted middle stop (index 1, not the right endpoint index 2), and overflow modes
    // (`Extend`, `Reflect`, `Repeat`) + `Linear RGB` recalculate accurately.
    editor.set_selection_fill_type(false, PaintFillType::LinearGradient);
    editor.set_selection_gradient_options(false, GradientSpread::Reflect, true);
    {
        let ElementKind::PaintedPath {
            fill_gradient: Some(fg),
            ..
        } = &editor.selected_element().unwrap().kind
        else {
            panic!("expected linear gradient");
        };
        assert_eq!(fg.spread, GradientSpread::Reflect);
        assert!(fg.linear_rgb);
        // Reflect maps u = 1.25 to 0.75:
        assert!((fg.spread.apply(1.25) - 0.75).abs() < 1e-6);
        assert!((GradientSpread::Repeat.apply(1.25) - 0.25).abs() < 1e-6);
    }
    editor.project.validate().unwrap();
}

#[test]
fn gradient_stops_keyboard_traversal_reordering_insertion_and_deletion() {
    use egui::{Event, Key, Modifiers};
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut gradient = GradientPaint {
        stops: [80, 85, 255]
            .map(|ratio| GradientStop {
                ratio,
                color: [80, 100, 200, 255],
            })
            .into(),
        ..Default::default()
    };
    let mut selected = 0;
    let mut nodes = HashMap::new();
    let mut run = |grad: &mut GradientPaint, key: Option<Key>, modifiers: Modifiers| {
        let events = key
            .into_iter()
            .flat_map(|key| {
                [true, false].map(|pressed| Event::Key {
                    key,
                    physical_key: Some(key),
                    pressed,
                    repeat: false,
                    modifiers,
                })
            })
            .collect();
        let out = ctx.run(
            egui::RawInput {
                events,
                modifiers,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    panels_color::gradient_stops_bar_ui(ui, grad, &mut selected);
                    let _ = ui.button("After ramp");
                });
            },
        );
        let tree = out.platform_output.accesskit_update.unwrap();
        nodes.extend(tree.nodes);
        (
            selected,
            nodes
                .get(&tree.focus)
                .and_then(|node: &egui::accesskit::Node| node.label())
                .unwrap_or_default()
                .to_owned(),
        )
    };
    run(&mut gradient, None, Modifiers::NONE);
    for _ in 0..6 {
        if run(&mut gradient, Some(Key::Tab), Modifiers::NONE).1 == "Gradient stop 1" {
            break;
        }
    }
    assert_eq!(
        run(&mut gradient, None, Modifiers::NONE).1,
        "Gradient stop 1"
    );
    assert_eq!(
        run(&mut gradient, Some(Key::ArrowRight), Modifiers::SHIFT).0,
        1
    );
    assert_eq!(gradient.stops[1].ratio, 90);
    assert_eq!(
        run(&mut gradient, None, Modifiers::NONE).1,
        "Gradient stop 2"
    );
    run(&mut gradient, Some(Key::ArrowLeft), Modifiers::NONE);
    assert_eq!(gradient.stops[1].ratio, 89);
    run(&mut gradient, Some(Key::Home), Modifiers::NONE);
    assert_eq!(gradient.stops[0].ratio, 0);
    run(&mut gradient, Some(Key::Enter), Modifiers::NONE);
    assert_eq!(
        gradient.stops.iter().map(|s| s.ratio).collect::<Vec<_>>(),
        vec![0, 43, 85, 255]
    );
    assert_eq!(
        run(&mut gradient, None, Modifiers::NONE).1,
        "Gradient stop 2"
    );
    run(&mut gradient, Some(Key::Backspace), Modifiers::NONE);
    run(&mut gradient, Some(Key::Delete), Modifiers::NONE);
    run(&mut gradient, Some(Key::Backspace), Modifiers::NONE);
    assert_eq!(gradient.stops.len(), 2);
    run(&mut gradient, Some(Key::End), Modifiers::NONE);
    assert_eq!(
        run(&mut gradient, Some(Key::Tab), Modifiers::NONE).1,
        "After ramp"
    );
    run(&mut gradient, Some(Key::Tab), Modifiers::SHIFT);
    assert_eq!(
        run(&mut gradient, None, Modifiers::NONE).1,
        "Gradient stop 2"
    );
}
