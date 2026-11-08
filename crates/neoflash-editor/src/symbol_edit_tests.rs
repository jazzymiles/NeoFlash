use crate::app::Editor;
use crate::symbol_edit::SymbolEditEntry;
use eframe::egui;
use neoflash_core::{
    Element, ElementKind, FrameRange, InstancePlayback, Keyframe, Layer, LayerRole, Project,
    Transform, affine_multiply, evaluate_scene, evaluate_scene_with_frozen_clips, transform_matrix,
};

fn add_empty_symbol(editor: &mut Editor, id: &str) {
    let mut symbol = editor.symbol().clone();
    symbol.id = id.to_owned();
    symbol.name = id.to_owned();
    symbol.class_name = None;
    symbol.linkage_id = None;
    symbol.timeline.frame_count = 8;
    symbol.timeline.scripts.clear();
    symbol.timeline.guides.clear();
    for (index, layer) in symbol.timeline.layers.iter_mut().enumerate() {
        layer.id = format!("{id}-layer-{index}");
        layer.properties = Default::default();
        layer.keyframes.truncate(1);
        layer.keyframes[0].frame = 0;
        layer.keyframes[0].elements.clear();
    }
    editor.project.symbols.push(symbol);
}

fn place(
    editor: &mut Editor,
    symbol: &str,
    transform: Transform,
) -> crate::selection::SelectionTarget {
    editor.add_element_transformed(
        ElementKind::Instance {
            symbol_id: symbol.to_owned(),
            playback: InstancePlayback::Independent,
        },
        transform,
    );
    editor.selection.single().unwrap().clone()
}

fn assert_matrix_close(actual: [f64; 6], expected: [f64; 6]) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!((actual - expected).abs() < 1e-8, "{actual} != {expected}");
    }
}

fn rectangle(id: &str, fill: [u8; 4]) -> Element {
    Element {
        id: id.into(),
        name: id.into(),
        kind: ElementKind::Rectangle {
            width: 20.,
            height: 20.,
            fill,
            stroke: [0; 4],
            stroke_width: 0.,
        },
        ..Element::default()
    }
}

#[test]
fn in_place_session_keeps_exact_instance_affine_and_restores_parent_view() {
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::demo());
    add_empty_symbol(&mut editor, "shared-child");
    let first = place(
        &mut editor,
        "shared-child",
        Transform {
            x: 30.,
            y: 40.,
            ..Default::default()
        },
    );
    let transform = Transform {
        x: 210.,
        y: 125.,
        scale_x: 1.7,
        scale_y: 0.65,
        rotation: 28.,
        skew: -17.,
        ..Default::default()
    };
    let second = place(&mut editor, "shared-child", transform);
    assert_ne!(first, second);

    editor.frame = 3;
    editor.set_selection(vec![second.clone()]);
    editor.zoom = 1.6;
    editor.pan = egui::vec2(37., -19.);
    editor.stage_view.zero = [11., -7.];
    let layer = editor.symbol().timeline.layers[0].id.clone();
    let range = FrameRange {
        layers: vec![layer],
        start: 2,
        end: 4,
    };
    editor.select_frame_range(range.clone());
    let parent_selection = editor.selection.clone();
    let before_navigation = editor.project.clone();

    assert!(editor.enter_selected_symbol_in_place());
    assert!(editor.editing_in_place());
    assert_eq!(editor.symbol_id(), "shared-child");
    assert_eq!(
        editor.frame, 3,
        "in-place editing opens the child frame displayed by the selected placement"
    );
    assert_eq!(editor.pan, egui::vec2(37., -19.));
    assert_eq!(editor.edit_depth(), 2);
    assert!(matches!(
        &editor.edit_session.levels[1].entry,
        SymbolEditEntry::InPlace {
            parent_frame: 3,
            target,
            ..
        } if target == &second
    ));
    assert_matrix_close(
        editor.edit_local_to_context_matrix().unwrap(),
        transform_matrix(transform),
    );
    assert_eq!(editor.project, before_navigation);

    assert!(editor.leave_symbol_to_level(0));
    assert!(editor.editing_scene());
    assert_eq!(editor.frame, 3);
    assert_eq!(editor.selection, parent_selection);
    assert_eq!(editor.zoom, 1.6);
    assert_eq!(editor.pan, egui::vec2(37., -19.));
    assert_eq!(editor.stage_view.zero, [11., -7.]);
    assert_eq!(editor.frame_range(), Some(range));
    assert_eq!(editor.project, before_navigation);

    assert!(editor.enter_symbol_isolated("shared-child".into()));
    assert!(editor.editing_isolated());
    assert_eq!(editor.pan, egui::Vec2::ZERO);
    assert_matrix_close(
        editor.edit_local_to_context_matrix().unwrap(),
        [1., 0., 0., 1., 0., 0.],
    );
    assert!(editor.leave_symbol_to_level(0));
    assert_eq!(editor.frame, 3);
    assert_eq!(editor.pan, egui::vec2(37., -19.));
}

#[test]
fn nested_in_place_composes_live_matrices_and_invalid_levels_unwind_safely() {
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::demo());
    add_empty_symbol(&mut editor, "outer-child");
    add_empty_symbol(&mut editor, "inner-child");
    let root_id = editor.project.root.clone();
    let outer_transform = Transform {
        x: 70.,
        y: 90.,
        scale_x: 1.25,
        scale_y: 0.8,
        rotation: 15.,
        skew: 6.,
        ..Default::default()
    };
    let outer_target = place(&mut editor, "outer-child", outer_transform);
    editor.zoom = 1.25;
    editor.pan = egui::vec2(-8., 16.);
    editor.stage_view.zero = [3., 4.];
    assert!(editor.enter_selected_symbol_in_place());

    let inner_transform = Transform {
        x: -12.,
        y: 34.,
        scale_x: 0.6,
        scale_y: 1.4,
        rotation: -22.,
        skew: 11.,
        ..Default::default()
    };
    let inner_target = place(&mut editor, "inner-child", inner_transform);
    assert!(editor.enter_selected_symbol_in_place());
    assert_matrix_close(
        editor.edit_local_to_context_matrix().unwrap(),
        affine_multiply(
            transform_matrix(outer_transform),
            transform_matrix(inner_transform),
        ),
    );

    // Invalidating the exact inner placement must not silently choose another
    // instance of the same symbol.
    editor
        .project
        .symbol_mut("outer-child")
        .unwrap()
        .timeline
        .layers
        .iter_mut()
        .for_each(|layer| {
            for key in &mut layer.keyframes {
                key.elements
                    .retain(|element| element.id != inner_target.element);
            }
        });
    editor.normalize_selection();
    assert_eq!(editor.symbol_id(), "outer-child");
    assert!(editor.editing_in_place());
    assert_eq!(editor.edit_depth(), 2);

    editor
        .project
        .symbol_mut(&root_id)
        .unwrap()
        .timeline
        .layers
        .iter_mut()
        .for_each(|layer| {
            for key in &mut layer.keyframes {
                key.elements
                    .retain(|element| element.id != outer_target.element);
            }
        });
    editor.normalize_selection();
    assert_eq!(editor.symbol_id(), root_id);
    assert!(editor.editing_scene());
    assert_eq!(editor.zoom, 1.25);
    assert_eq!(editor.pan, egui::vec2(-8., 16.));
    assert_eq!(editor.stage_view.zero, [3., 4.]);
}

#[test]
fn singular_in_place_refuses_and_document_tabs_keep_context() {
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::demo());
    add_empty_symbol(&mut editor, "tab-child");
    let target = place(
        &mut editor,
        "tab-child",
        Transform {
            x: 44.,
            y: 55.,
            rotation: 12.,
            ..Default::default()
        },
    );
    assert!(editor.enter_selected_symbol_in_place());
    editor.zoom = 2.0;
    editor.frame = 4;
    let first_revision = editor.edit_session.revision;
    let first_project = editor.project.clone();

    editor.open_new_session_tab(&ctx, Project::blank(), None);
    assert!(editor.editing_scene());
    assert_eq!(editor.session_count(), 2);
    assert!(editor.switch_session_tab(&ctx, 0));
    assert_eq!(editor.project, first_project);
    assert_eq!(editor.symbol_id(), "tab-child");
    assert!(editor.editing_in_place());
    assert_eq!(editor.zoom, 2.0);
    assert_eq!(editor.frame, 4);
    assert_eq!(editor.edit_session.revision, first_revision);
    assert!(matches!(
        &editor.edit_session.levels[1].entry,
        SymbolEditEntry::InPlace { target: saved, .. } if saved == &target
    ));

    assert!(editor.leave_symbol_to_level(0));
    editor.set_selection(vec![target]);
    editor.selected_element_mut().unwrap().transform.scale_x = 0.;
    let before_path: Vec<_> = editor.edit_path().map(str::to_owned).collect();
    let before_revision = editor.edit_session.revision;
    let before_view = (editor.frame, editor.layer, editor.zoom, editor.pan);
    assert!(!editor.enter_selected_symbol_in_place());
    assert_eq!(
        editor.edit_path().map(str::to_owned).collect::<Vec<_>>(),
        before_path
    );
    assert_eq!(editor.edit_session.revision, before_revision);
    assert_eq!(
        (editor.frame, editor.layer, editor.zoom, editor.pan),
        before_view
    );
    assert!(editor.status.contains("not invertible"));

    editor.selected_element_mut().unwrap().transform.scale_x = 2.;
    editor
        .project
        .symbol_mut("tab-child")
        .unwrap()
        .set_scale9_grid(Some([1., 1., 20., 20.]));
    assert!(!editor.enter_selected_symbol_in_place());
    assert!(editor.status.contains("nine-slice"));
    assert!(editor.editing_scene());
}

#[test]
fn paused_preview_frame_same_symbol_isolation_and_timeline_focus_restore() {
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    add_empty_symbol(&mut editor, "preview-child");
    editor.symbol_mut().timeline.frame_count = 1;
    let target = place(
        &mut editor,
        "preview-child",
        Transform {
            x: 80.,
            y: 45.,
            rotation: 20.,
            ..Default::default()
        },
    );
    let range = FrameRange {
        layers: vec![target.layer.clone()],
        start: 0,
        end: 0,
    };
    editor.select_frame_range(range.clone());
    editor.timeline_edit.focused = false;
    let parent_selection = editor.selection.clone();
    let before_navigation = editor.project.clone();

    editor.set_playing(true);
    for _ in 0..5 {
        editor.tick_authored_preview();
    }
    editor.set_playing(false);
    assert_eq!(editor.frame, 0, "the one-frame parent stays on frame zero");
    assert_eq!(
        editor.presented_instance_frame(&target.element),
        Some(5),
        "the paused independent child keeps its own preview clock"
    );

    assert!(editor.enter_selected_symbol_in_place());
    assert_eq!(
        editor.frame, 5,
        "in-place entry keeps the presented child pose"
    );
    assert!(editor.editing_in_place());
    editor.enter_explorer_timeline("preview-child");
    assert!(editor.editing_isolated());
    assert_eq!(
        editor.edit_depth(),
        2,
        "rerooting does not duplicate the breadcrumb"
    );
    assert_eq!(editor.frame, 0);
    assert_matrix_close(
        editor.edit_local_to_context_matrix().unwrap(),
        [1., 0., 0., 1., 0., 0.],
    );

    assert!(editor.leave_symbol_to_level(0));
    assert_eq!(editor.selection, parent_selection);
    assert_eq!(editor.frame_range(), Some(range));
    assert!(
        !editor.timeline_edit.focused,
        "an unfocused cached Timeline range must stay unfocused"
    );
    assert_eq!(editor.project, before_navigation);
}

#[test]
fn unique_in_place_lookup_ignores_hidden_duplicate() {
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    add_empty_symbol(&mut editor, "lookup-child");
    let visible = place(
        &mut editor,
        "lookup-child",
        Transform {
            x: 20.,
            ..Default::default()
        },
    );
    let hidden = place(
        &mut editor,
        "lookup-child",
        Transform {
            x: 200.,
            ..Default::default()
        },
    );
    editor.set_selection(vec![hidden]);
    editor.selected_element_mut().unwrap().visible = false;
    editor.project.upgrade_format_for_features();
    editor.clear_selection();
    editor.project.validate().unwrap();

    assert!(editor.enter_symbol_in_place_by_id("lookup-child"));
    assert!(matches!(
        &editor.edit_session.levels[1].entry,
        SymbolEditEntry::InPlace { target, .. } if target == &visible
    ));
    assert_matrix_close(
        editor.edit_local_to_context_matrix().unwrap(),
        [1., 0., 0., 1., 20., 0.],
    );
}

#[test]
fn in_place_freezes_the_whole_paused_clip_tree_but_reevaluates_live_artwork() {
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    add_empty_symbol(&mut editor, "clock-selected");
    add_empty_symbol(&mut editor, "clock-sibling");
    {
        let sibling = editor.project.symbol_mut("clock-sibling").unwrap();
        sibling.timeline.layers[0].keyframes = vec![
            Keyframe {
                frame: 0,
                elements: vec![rectangle("sibling-frame-0", [220, 20, 20, 255])],
                ..Default::default()
            },
            Keyframe {
                frame: 5,
                elements: vec![rectangle("sibling-frame-5", [20, 220, 20, 255])],
                ..Default::default()
            },
        ];
    }
    editor.symbol_mut().timeline.frame_count = 1;
    let selected = place(&mut editor, "clock-selected", Transform::default());
    let sibling = place(
        &mut editor,
        "clock-sibling",
        Transform {
            x: 50.,
            ..Default::default()
        },
    );
    editor.set_selection(vec![selected.clone()]);
    editor.project.validate().unwrap();

    editor.set_playing(true);
    for _ in 0..5 {
        editor.tick_authored_preview();
    }
    editor.set_playing(false);
    assert_eq!(editor.presented_instance_frame(&selected.element), Some(5));
    assert_eq!(editor.presented_instance_frame(&sibling.element), Some(5));
    assert!(editor.enter_selected_symbol_in_place());

    let (parent_symbol, parent_frame, frames) = match &editor.edit_session.levels[1].entry {
        SymbolEditEntry::InPlace {
            parent_symbol,
            parent_frame,
            parent_clip_frames,
            ..
        } => (
            parent_symbol.clone(),
            *parent_frame,
            parent_clip_frames.clone(),
        ),
        _ => panic!("expected in-place entry"),
    };
    assert_eq!(frames.get(&vec![selected.element.clone()]), Some(&5));
    assert_eq!(frames.get(&vec![sibling.element.clone()]), Some(&5));

    let evaluate_parent = |project: &Project| {
        evaluate_scene_with_frozen_clips(
            project,
            &parent_symbol,
            parent_frame,
            &frames,
            std::slice::from_ref(&selected.element),
            true,
            true,
        )
        .0
    };
    let scene = evaluate_parent(&editor.project);
    assert!(scene.draws.iter().any(|draw| {
        draw.item.instance_path == [sibling.element.clone(), "sibling-frame-5".into()]
    }));

    let live_fill = [30, 60, 240, 255];
    let live = editor
        .project
        .symbol_mut("clock-sibling")
        .unwrap()
        .timeline
        .layers[0]
        .keyframes
        .iter_mut()
        .find(|key| key.frame == 5)
        .unwrap()
        .elements
        .first_mut()
        .unwrap();
    if let ElementKind::Rectangle { fill, .. } = &mut live.kind {
        *fill = live_fill;
    }
    let scene = evaluate_parent(&editor.project);
    assert!(scene.draws.iter().any(|draw| {
        draw.item.instance_path == [sibling.element.clone(), "sibling-frame-5".into()]
            && matches!(draw.item.kind, ElementKind::Rectangle { fill, .. } if fill == live_fill)
    }));
}

#[test]
fn parent_context_uses_its_own_solo_state_while_a_child_is_active() {
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    add_empty_symbol(&mut editor, "solo-child");
    let selected = place(&mut editor, "solo-child", Transform::default());
    let root = editor.project.root.clone();
    let solo_layer = selected.layer.clone();
    let mut other = Layer {
        id: "parent-nonsolo-layer".into(),
        name: "Parent nonsolo".into(),
        visible: true,
        locked: false,
        keyframes: vec![Keyframe {
            frame: 0,
            elements: vec![rectangle("parent-nonsolo-shape", [200, 80, 20, 255])],
            ..Default::default()
        }],
        properties: Default::default(),
    };
    other.properties.role = LayerRole::Normal;
    editor.symbol_mut().timeline.layers.push(other);
    editor.toggle_layer_solo(0, true);
    editor.set_selection(vec![selected]);
    assert!(editor.enter_selected_symbol_in_place());

    let projected = editor.project_for_symbol_stage_eval(&root);
    let root_symbol = projected.symbol(&root).unwrap();
    assert!(
        root_symbol
            .timeline
            .layers
            .iter()
            .find(|layer| layer.id == solo_layer)
            .unwrap()
            .visible
    );
    assert!(
        !root_symbol
            .timeline
            .layers
            .iter()
            .find(|layer| layer.id == "parent-nonsolo-layer")
            .unwrap()
            .visible
    );
    let scene = evaluate_scene(&projected, &root, 0, true, true);
    assert!(
        scene
            .draws
            .iter()
            .all(|draw| draw.item.element_id != "parent-nonsolo-shape")
    );
}

#[test]
fn structural_parent_mask_survives_an_empty_current_child_branch() {
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    add_empty_symbol(&mut editor, "empty-masked-child");
    let target = place(&mut editor, "empty-masked-child", Transform::default());
    let mask_id = "empty-branch-mask".to_owned();
    editor.symbol_mut().timeline.layers[0].properties.mask = Some(mask_id.clone());
    let mut mask = Layer {
        id: mask_id,
        name: "Mask".into(),
        visible: true,
        locked: true,
        keyframes: vec![Keyframe {
            frame: 0,
            elements: vec![rectangle("empty-branch-mask-shape", [0, 0, 0, 255])],
            ..Default::default()
        }],
        properties: Default::default(),
    };
    mask.properties.role = LayerRole::Mask;
    editor.symbol_mut().timeline.layers.push(mask);
    editor.project.format_version = editor.project.format_version.max(4);
    editor.project.validate().unwrap();

    let root = editor.project.root.clone();
    let (scene, clips) = evaluate_scene_with_frozen_clips(
        &editor.project,
        &root,
        0,
        &Default::default(),
        std::slice::from_ref(&target.element),
        true,
        true,
    );
    assert!(
        scene
            .draws
            .iter()
            .all(|draw| draw.item.instance_path.first() != Some(&target.element)),
        "the fixture's current child frame must be empty"
    );
    assert_eq!(clips.len(), 1, "the target relationship is structural");
    assert!(scene.clips.iter().any(|clip| clip.id == clips[0]));
}
