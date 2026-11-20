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

#[test]
fn t318_nested_edit_in_place_library_mutations_scene_delete_and_undo_never_dangle() {
    use neoflash_core::{AuthoringGroup, IkArmature, IkBone};

    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    add_empty_symbol(&mut editor, "outer-sym");
    add_empty_symbol(&mut editor, "inner-sym");
    add_empty_symbol(&mut editor, "alt-sym");

    // Populate `inner-sym` with a shape, an AuthoringGroup, and an IkArmature so symbol duplication
    // also exercises group + IK armature ID freshening.
    {
        let inner = editor.project.symbol_mut("inner-sym").unwrap();
        inner.timeline.layers[0].properties.groups = vec![AuthoringGroup {
            id: "grp-inner".into(),
            name: "InnerGrp".into(),
            members: vec!["inner-rect".into()],
        }];
        inner.timeline.layers[0].keyframes[0].elements =
            vec![rectangle("inner-rect", [220, 80, 40, 255])];
        inner.timeline.layers[0].keyframes[0].ik_armatures = vec![IkArmature {
            id: "ik-inner".into(),
            name: "InnerRig".into(),
            bones: vec![IkBone {
                id: "bone-inner".into(),
                element_id: "inner-rect".into(),
                parent_bone_id: None,
                length: 15.0,
                min_angle: None,
                max_angle: None,
                stiffness: 1.0,
            }],
            effector_bone_id: "bone-inner".into(),
            target_pos: [15.0, 0.0],
            pole_pos: None,
        }];
    }
    editor.project.upgrade_format_for_features();
    editor.project.validate().unwrap();

    // 1. Duplicate `inner-sym`: must freshen internal element, group, and IK armature IDs cleanly.
    let dup_id = editor
        .duplicate_symbol("inner-sym")
        .expect("duplicate_symbol must succeed");
    editor
        .project
        .validate()
        .expect("project must validate after duplicating symbol with groups and IK");
    let dup_sym = editor.project.symbol(&dup_id).unwrap();
    assert_ne!(
        dup_sym.timeline.layers[0].properties.groups[0].id,
        "grp-inner"
    );
    assert_ne!(
        dup_sym.timeline.layers[0].keyframes[0].ik_armatures[0].id,
        "ik-inner"
    );

    // 2. Place `outer-sym` on root, enter `Edit in Place` (level 1), place `inner-sym` inside `outer-sym`,
    // and enter 2-level nested `Edit in Place` (`Scene 1 -> outer-sym -> inner-sym`, depth 3).
    let outer_tf = Transform {
        x: 60.0,
        y: 80.0,
        scale_x: 1.5,
        scale_y: 0.75,
        rotation: 18.0,
        ..Default::default()
    };
    let _outer_target = place(&mut editor, "outer-sym", outer_tf);
    assert!(editor.enter_selected_symbol_in_place());
    assert_eq!(editor.edit_depth(), 2);
    assert_eq!(editor.symbol_id(), "outer-sym");

    let inner_tf = Transform {
        x: 20.0,
        y: -15.0,
        rotation: -10.0,
        ..Default::default()
    };
    let _inner_target = place(&mut editor, "inner-sym", inner_tf);
    assert!(editor.enter_selected_symbol_in_place());
    assert_eq!(editor.edit_depth(), 3);
    assert_eq!(editor.symbol_id(), "inner-sym");
    assert_matrix_close(
        editor.edit_local_to_context_matrix().unwrap(),
        affine_multiply(transform_matrix(outer_tf), transform_matrix(inner_tf)),
    );

    // 3. Delete `inner-sym` in Library while editing it in place -> safely unwinds to `outer-sym`
    // while preserving `outer-sym`'s exact affine placement!
    assert!(editor.delete_symbol("inner-sym"));
    editor.project.validate().unwrap();
    assert_eq!(editor.edit_depth(), 2);
    assert_eq!(editor.symbol_id(), "outer-sym");
    assert!(editor.editing_in_place());
    assert_matrix_close(
        editor.edit_local_to_context_matrix().unwrap(),
        transform_matrix(outer_tf),
    );

    // 4. Undo deletion -> `inner-sym` returns; re-enter `inner-sym`, then delete ancestor `outer-sym`!
    editor.undo();
    editor.project.validate().unwrap();
    assert!(editor.project.symbol("inner-sym").is_some());
    assert!(editor.enter_symbol_in_place_by_id("inner-sym"));
    assert_eq!(editor.edit_depth(), 3);

    assert!(editor.delete_symbol("outer-sym"));
    editor.project.validate().unwrap();
    assert_eq!(editor.edit_depth(), 1);
    assert!(editor.editing_scene());
    assert!(
        editor.selection.is_empty(),
        "deleting ancestor symbol must clear stale selection targets"
    );

    // 5. Undo `outer-sym` deletion, swap instance symbol, Break Apart, and Convert to Symbol.
    editor.undo();
    editor.project.validate().unwrap();
    let root_id = editor.project.root.clone();
    let restored_outer_elem =
        editor.project.symbol(&root_id).unwrap().timeline.layers[0].keyframes[0].elements[0]
            .id
            .clone();
    let root_layer_id = editor.project.symbol(&root_id).unwrap().timeline.layers[0]
        .id
        .clone();
    editor.set_selection(vec![crate::selection::SelectionTarget {
        layer: root_layer_id,
        element: restored_outer_elem,
    }]);
    assert!(editor.swap_selected_instance_to_symbol("alt-sym"));
    editor.project.validate().unwrap();

    // Populate `alt-sym` with a rectangle so Break Apart has concrete child geometry, then Break Apart
    // and Convert to Symbol (`F8`).
    editor.transact_named("Add rect to alt-sym", |ed| {
        ed.project.symbol_mut("alt-sym").unwrap().timeline.layers[0].keyframes[0].elements =
            vec![rectangle("alt-rect", [40, 120, 220, 255])];
    });
    editor.break_apart_selection();
    editor.project.validate().unwrap();
    assert!(!editor.selection.is_empty());

    editor.begin_symbol_conversion();
    let mut draft = editor.symbol_conversion_draft().unwrap().clone();
    draft.name = "RewrappedSymbol".into();
    editor
        .commit_symbol_conversion(
            &draft,
            egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(20.0, 20.0)),
        )
        .expect("Convert to Symbol must succeed");
    editor.project.validate().unwrap();

    // 6. Add Scene 2, place `alt-sym` inside Scene 2, enter Edit in Place inside Scene 2, then delete Scene 2!
    editor.new_scene();
    assert_eq!(editor.scene_list().len(), 2);
    let scene2_id = editor.current_scene().to_owned();
    let _scene2_inst = place(&mut editor, "alt-sym", Transform::default());
    assert!(editor.enter_selected_symbol_in_place());
    assert_eq!(editor.edit_depth(), 2);
    assert_eq!(editor.symbol_id(), "alt-sym");

    editor.delete_scene();
    editor.project.validate().unwrap();
    assert!(editor.project.symbol(&scene2_id).is_none());
    assert_eq!(editor.edit_depth(), 1);
    assert_eq!(editor.symbol_id(), root_id);
    assert!(editor.editing_scene());

    // Undo + Redo across scene deletion remain 100% valid with zero dangling SymbolIds.
    editor.undo();
    editor.project.validate().unwrap();
    editor.redo();
    editor.project.validate().unwrap();

    // 7. Composed cumulative matrix underflow across 2+ nested `InPlace` levels (`1e-4 * 1e-4` -> `det = 1e-16 < 1e-12`,
    // while each level individually has `det = 1e-8 >= 1e-12`): `normalize_symbol_edit_session` via `commit_history`
    // must unwind cleanly to `outer-sym` so `current_matrix` never fails with `SingularPlacement`.
    add_empty_symbol(&mut editor, "tiny-outer");
    add_empty_symbol(&mut editor, "tiny-inner");
    let _tiny_outer_on_root = place(
        &mut editor,
        "tiny-outer",
        Transform {
            scale_x: 1.0,
            scale_y: 1.0,
            ..Default::default()
        },
    );
    assert!(editor.enter_selected_symbol_in_place());
    assert_eq!(editor.symbol_id(), "tiny-outer");
    let _tiny_inner_on_outer = place(
        &mut editor,
        "tiny-inner",
        Transform {
            scale_x: 1e-4,
            scale_y: 1e-4,
            ..Default::default()
        },
    );
    assert!(editor.enter_selected_symbol_in_place());
    assert_eq!(editor.edit_depth(), 3);
    assert_eq!(editor.symbol_id(), "tiny-inner");

    // Shrink `tiny-outer` on root to `1e-4` in one transaction: `tiny-outer` alone has `det = 1e-8` (valid),
    // `tiny-inner` alone has `det = 1e-8` (valid), but composed `cumulative` has `det = 1e-16` (singular!).
    editor.transact_named("Shrink outer placement", |ed| {
        let root_sym = ed.project.symbol_mut(&root_id).unwrap();
        let outer_el = root_sym.timeline.layers[0].keyframes[0]
            .elements
            .iter_mut()
            .find(|el| {
                matches!(
                    &el.kind,
                    neoflash_core::ElementKind::Instance { symbol_id, .. } if symbol_id == "tiny-outer"
                )
            })
            .unwrap();
        outer_el.transform.scale_x = 1e-4;
        outer_el.transform.scale_y = 1e-4;
    });
    assert_eq!(
        editor.edit_depth(),
        2,
        "composed singular InPlace depth must unwind cleanly to nearest invertible ancestor"
    );
    assert_eq!(editor.symbol_id(), "tiny-outer");
    assert!(
        editor.edit_session.current_matrix(&editor.project).is_ok(),
        "current_matrix must remain invertible after normalization"
    );
}

#[test]
fn t359_t360_convert_to_symbol_all_registrations_empty_clip_stage_marker_selection_and_edit_flow() {
    use crate::app::Tool;
    use crate::stage_mapping::StageMapping;
    use crate::tests::{primary_button, rectangle_stage, stage_input_frame};
    use neoflash_core::{
        FilterSpec, ObjectEvent, ObjectScript, Script, ScriptLanguage, SymbolKind, evaluate,
    };

    // Part A: All 9 registration grid points (0..9) on a rotated, scaled, filtered, scripted single element.
    let ctx = egui::Context::default();
    let painter = ctx.layer_painter(egui::LayerId::background());
    let mut editor = Editor::from_project(&ctx, Project::blank());
    let root_id = editor.project.root.clone();
    editor.add_element_transformed(
        ElementKind::Rectangle {
            width: 100.0,
            height: 80.0,
            fill: [40, 120, 220, 255],
            stroke: [10, 20, 40, 255],
            stroke_width: 2.0,
        },
        Transform {
            x: 200.0,
            y: 150.0,
            scale_x: 1.5,
            scale_y: 0.8,
            rotation: 35.0,
            alpha: 0.7,
            ..Transform::default()
        },
    );
    editor.transact_named("Attach filter & script", |ed| {
        let el = ed.selected_element_mut().unwrap();
        el.name = "custom_shape".into();
        el.filters.push(FilterSpec::Blur {
            blur_x: 4.0,
            blur_y: 4.0,
            quality: 1,
        });
        el.scripts.push(ObjectScript {
            event: ObjectEvent::Load,
            script: Script {
                language: ScriptLanguage::ActionScript2,
                source: "trace('loaded');".into(),
            },
        });
    });

    for reg in 0..9usize {
        let snap_before = editor.project.clone();
        let before_draws = evaluate(&editor.project, &root_id, 0);
        editor.convert_to_symbol();
        let mut draft = editor.symbol_conversion.take().unwrap();
        draft.registration = reg;
        draft.set_name(format!("ClipReg{reg}"));
        draft.export_enabled = true;
        let bounds = crate::symbol_dialog::conversion_bounds(&editor, &draft, &painter).unwrap();
        let expected_anchor = egui::pos2(
            bounds.left() + bounds.width() * (reg % 3) as f32 / 2.0,
            bounds.top() + bounds.height() * (reg / 3) as f32 / 2.0,
        );
        editor.commit_symbol_conversion(&draft, bounds).unwrap();
        editor.project.validate().unwrap();

        let inst = editor.selected_element().unwrap();
        assert_eq!(
            inst.name, "",
            "Convert to Symbol never copies Symbol.name into Instance.name"
        );
        assert!((inst.transform.x - f64::from(expected_anchor.x)).abs() < 1e-6);
        assert!((inst.transform.y - f64::from(expected_anchor.y)).abs() < 1e-6);
        assert_eq!(inst.transform.rotation, 0.0);
        assert_eq!(inst.transform.scale_x, 1.0);
        assert_eq!(inst.transform.scale_y, 1.0);
        assert!((inst.transform.alpha - 0.7).abs() < 1e-9);
        assert_eq!(inst.filters.len(), 1);
        assert_eq!(inst.scripts.len(), 1);

        let ElementKind::Instance { symbol_id, .. } = &inst.kind else {
            panic!("expected Instance");
        };
        let created_sym = editor.project.symbol(symbol_id).unwrap();
        assert_eq!(
            created_sym.linkage_id.as_deref(),
            Some(format!("ClipReg{reg}").as_str())
        );

        let after_draws = evaluate(&editor.project, &root_id, 0);
        assert_eq!(before_draws.len(), after_draws.len());
        for (b, a) in before_draws.iter().zip(&after_draws) {
            assert_eq!(b.kind, a.kind);
            assert!((b.alpha - a.alpha).abs() < 1e-9);
            assert_matrix_close(a.transform, b.transform);
        }

        editor.undo();
        assert_eq!(editor.project, snap_before);
    }

    // Part B: Exact Stage flow — MovieClip at (200, 150) -> delete contents -> empty ○+ marker,
    // click/marquee/Lasso/drag/double-click Edit in Place -> Library isolated 50x50 square at (20, 30) -> (220, 180)!
    let (mut editor, ctx, page) = rectangle_stage();
    editor.set_tool(Tool::Select);
    let root_id = editor.project.root.clone();
    editor.add_element(
        ElementKind::Rectangle {
            width: 100.0,
            height: 80.0,
            fill: [220, 80, 40, 255],
            stroke: [0; 4],
            stroke_width: 0.0,
        },
        200.0,
        150.0,
    );
    editor.convert_to_symbol();
    let mut draft = editor.symbol_conversion.take().unwrap();
    draft.registration = 0; // Top-left registration -> exact (200, 150)
    draft.kind = SymbolKind::MovieClip;
    draft.set_name("ContainerClip");
    let painter = ctx.layer_painter(egui::LayerId::background());
    let bounds = crate::symbol_dialog::conversion_bounds(&editor, &draft, &painter).unwrap();
    editor.commit_symbol_conversion(&draft, bounds).unwrap();

    let mc_inst_id = editor.selected_element().unwrap().id.clone();
    let mc_sym_id = match &editor.selected_element().unwrap().kind {
        ElementKind::Instance { symbol_id, .. } => symbol_id.clone(),
        _ => panic!("expected Instance"),
    };
    assert_eq!(editor.selected_element().unwrap().name, "");
    assert!((editor.selected_element().unwrap().transform.x - 200.0).abs() < 1e-6);
    assert!((editor.selected_element().unwrap().transform.y - 150.0).abs() < 1e-6);

    // Selected non-empty MovieClip paints its + registration crosshair at (200, 150).
    let scene_non_empty = evaluate(&editor.project, &root_id, 0);
    let (empty_m, reg_m) =
        editor.stage_instance_markers(&scene_non_empty, page.min, StageMapping::identity());
    assert!(empty_m.is_empty());
    assert_eq!(reg_m.len(), 1);
    assert!((reg_m[0].1 - (page.min + egui::vec2(200.0, 150.0))).length() < 1e-3);

    // Enter & delete contents -> exit to Stage.
    assert!(editor.enter_selected_symbol_in_place());
    editor.select_all();
    editor.delete_selected();
    editor.leave_symbol_to_level(0);
    editor.clear_selection();

    let scene_empty = evaluate(&editor.project, &root_id, 0);
    assert!(scene_empty.is_empty(), "empty MovieClip emits 0 DrawItems");
    let (empty_m, _) =
        editor.stage_instance_markers(&scene_empty, page.min, StageMapping::identity());
    assert_eq!(empty_m.len(), 1);
    assert_eq!(empty_m[0].0, mc_inst_id);
    assert!(!empty_m[0].2, "unselected empty marker");
    let marker_screen = page.min + egui::vec2(200.0, 150.0);
    assert!((empty_m[0].1 - marker_screen).length() < 1e-3);

    // Authoring marker is suppressed while playing.
    editor.playing = true;
    assert!(
        editor
            .stage_instance_markers(&scene_empty, page.min, StageMapping::identity())
            .0
            .is_empty()
    );
    editor.playing = false;

    // 1. Click hit-test within 9 pt of ○+ marker selects the empty MovieClip.
    let click_pt = marker_screen + egui::vec2(5.0, -3.0);
    let _ = stage_input_frame(
        &mut editor,
        &ctx,
        1.0,
        vec![
            egui::Event::PointerMoved(click_pt),
            primary_button(click_pt, true),
        ],
    );
    let _ = stage_input_frame(
        &mut editor,
        &ctx,
        1.05,
        vec![primary_button(click_pt, false)],
    );
    assert_eq!(
        editor.selection.single().map(|t| t.element.as_str()),
        Some(mc_inst_id.as_str()),
        "click within 9 pt selects empty MovieClip"
    );

    // 2. Marquee containing marker center selects the empty MovieClip.
    editor.clear_selection();
    let m0 = marker_screen - egui::vec2(15.0, 15.0);
    let m1 = marker_screen + egui::vec2(15.0, 15.0);
    let _ = stage_input_frame(&mut editor, &ctx, 2.0, vec![primary_button(m0, true)]);
    let _ = stage_input_frame(&mut editor, &ctx, 2.05, vec![egui::Event::PointerMoved(m1)]);
    let _ = stage_input_frame(&mut editor, &ctx, 2.10, vec![primary_button(m1, false)]);
    assert_eq!(
        editor.selection.single().map(|t| t.element.as_str()),
        Some(mc_inst_id.as_str()),
        "marquee selects empty MovieClip"
    );

    // 3. Lasso containing marker center selects the empty MovieClip.
    editor.clear_selection();
    editor.set_tool(Tool::Lasso);
    let l0 = marker_screen + egui::vec2(-16.0, -16.0);
    let l1 = marker_screen + egui::vec2(16.0, -16.0);
    let l2 = marker_screen + egui::vec2(16.0, 16.0);
    let l3 = marker_screen + egui::vec2(-16.0, 16.0);
    let _ = stage_input_frame(&mut editor, &ctx, 3.0, vec![primary_button(l0, true)]);
    let _ = stage_input_frame(&mut editor, &ctx, 3.03, vec![egui::Event::PointerMoved(l1)]);
    let _ = stage_input_frame(&mut editor, &ctx, 3.06, vec![egui::Event::PointerMoved(l2)]);
    let _ = stage_input_frame(&mut editor, &ctx, 3.09, vec![egui::Event::PointerMoved(l3)]);
    let _ = stage_input_frame(&mut editor, &ctx, 3.12, vec![primary_button(l3, false)]);
    assert_eq!(
        editor.selection.single().map(|t| t.element.as_str()),
        Some(mc_inst_id.as_str()),
        "Lasso selects empty MovieClip"
    );

    // 4. Drag-move empty MovieClip on Stage (+30, +20), then Undo back to (200, 150).
    editor.set_tool(Tool::Select);
    editor.stage_view.prefs.grid_snap = false;
    editor.stage_view.prefs.guides_snap = false;
    let drag_to = marker_screen + egui::vec2(30.0, 20.0);
    let _ = stage_input_frame(
        &mut editor,
        &ctx,
        4.0,
        vec![primary_button(marker_screen, true)],
    );
    let _ = stage_input_frame(
        &mut editor,
        &ctx,
        4.05,
        vec![egui::Event::PointerMoved(drag_to)],
    );
    let _ = stage_input_frame(
        &mut editor,
        &ctx,
        4.10,
        vec![primary_button(drag_to, false)],
    );
    assert!((editor.selected_element().unwrap().transform.x - 230.0).abs() < 1e-4);
    assert!((editor.selected_element().unwrap().transform.y - 170.0).abs() < 1e-4);
    editor.undo();
    assert!((editor.selected_element().unwrap().transform.x - 200.0).abs() < 1e-4);
    assert!((editor.selected_element().unwrap().transform.y - 150.0).abs() < 1e-4);

    // 5. Double-clicking empty MovieClip marker on Stage enters Edit in Place!
    let _ = stage_input_frame(
        &mut editor,
        &ctx,
        5.0,
        vec![
            egui::Event::PointerMoved(marker_screen),
            primary_button(marker_screen, true),
        ],
    );
    let _ = stage_input_frame(
        &mut editor,
        &ctx,
        5.05,
        vec![primary_button(marker_screen, false)],
    );
    let _ = stage_input_frame(
        &mut editor,
        &ctx,
        5.12,
        vec![primary_button(marker_screen, true)],
    );
    let _ = stage_input_frame(
        &mut editor,
        &ctx,
        5.16,
        vec![primary_button(marker_screen, false)],
    );
    assert_eq!(
        editor.edit_depth(),
        2,
        "double-click enters Edit in Place on empty MovieClip"
    );
    assert_eq!(editor.symbol_id(), mc_sym_id);
    editor.leave_symbol_to_level(0);

    // 6. Enter via Library & draw a 50x50 square at (20, 30) -> back on Stage, X=200, Y=150, square at (220, 180)!
    editor.enter_symbol_isolated(mc_sym_id.clone());
    editor.add_element(
        ElementKind::Rectangle {
            width: 50.0,
            height: 50.0,
            fill: [30, 180, 90, 255],
            stroke: [0; 4],
            stroke_width: 0.0,
        },
        20.0,
        30.0,
    );
    editor.leave_symbol_to_level(0);
    editor.select_single_id(mc_inst_id.clone());

    let inst_after = editor.selected_element().unwrap();
    assert!((inst_after.transform.x - 200.0).abs() < 1e-6);
    assert!((inst_after.transform.y - 150.0).abs() < 1e-6);
    let draws = evaluate(&editor.project, &root_id, 0);
    assert_eq!(draws.len(), 1);
    assert!((draws[0].transform[4] - 220.0).abs() < 1e-6);
    assert!((draws[0].transform[5] - 180.0).abs() < 1e-6);
    let (_, reg_after) = editor.stage_instance_markers(&draws, page.min, StageMapping::identity());
    assert_eq!(reg_after.len(), 1);
    assert!((reg_after[0].1 - marker_screen).length() < 1e-3);

    // Part C: Placing from Library onto Stage leaves element.name == "".
    editor
        .place_library_symbol(&ctx, &mc_sym_id, Some([320.0, 240.0]))
        .unwrap();
    assert_eq!(editor.selected_element().unwrap().name, "");
    editor.project.validate().unwrap();
}

#[test]
fn empty_movieclip_free_transform_drag_double_click_and_subselect() {
    use crate::app::Tool;
    use crate::tests::{primary_button, rectangle_stage, stage_input_frame};
    use neoflash_core::SymbolKind;
    let (mut editor, ctx, page) = rectangle_stage();
    editor.set_tool(Tool::Select);
    editor.stage_view.prefs.grid_snap = false;
    editor.stage_view.prefs.guides_snap = false;
    editor.add_element(
        ElementKind::Rectangle {
            width: 60.0,
            height: 40.0,
            fill: [180, 90, 40, 255],
            stroke: [0; 4],
            stroke_width: 0.0,
        },
        200.0,
        150.0,
    );
    editor.convert_to_symbol();
    let mut draft = editor.symbol_conversion.take().unwrap();
    draft.registration = 0;
    draft.kind = SymbolKind::MovieClip;
    draft.set_name("EmptyTransformClip");
    let painter = ctx.layer_painter(egui::LayerId::background());
    let bounds = crate::symbol_dialog::conversion_bounds(&editor, &draft, &painter).unwrap();
    editor.commit_symbol_conversion(&draft, bounds).unwrap();

    let mc_inst_id = editor.selected_element().unwrap().id.clone();
    let mc_sym_id = match &editor.selected_element().unwrap().kind {
        ElementKind::Instance { symbol_id, .. } => symbol_id.clone(),
        _ => panic!("expected Instance"),
    };
    assert!(editor.enter_selected_symbol_in_place());
    editor.select_all();
    editor.delete_selected();
    editor.leave_symbol_to_level(0);
    editor.clear_selection();

    let marker_screen = page.min + egui::vec2(200.0, 150.0);

    // 1. Subselect tool click within 9 pt of ○+ marker selects the empty MovieClip.
    editor.set_tool(Tool::Subselect);
    let _ = stage_input_frame(&mut editor, &ctx, 0.5, vec![]);
    let sub_click = marker_screen + egui::vec2(4.0, -3.0);
    let _ = stage_input_frame(
        &mut editor,
        &ctx,
        1.0,
        vec![
            egui::Event::PointerMoved(sub_click),
            primary_button(sub_click, true),
        ],
    );
    let _ = stage_input_frame(
        &mut editor,
        &ctx,
        1.05,
        vec![primary_button(sub_click, false)],
    );
    assert_eq!(
        editor.selection.single().map(|t| t.element.as_str()),
        Some(mc_inst_id.as_str()),
        "Tool::Subselect click within 9 pt selects empty MovieClip"
    );

    // 2. FreeTransform (Tool::Transform) click-selects and drag-moves (transform.x, transform.y)
    // without Zone::Pivot or Zone::Rotate hijacking the gesture!
    editor.clear_selection();
    editor.set_tool(Tool::Transform);
    let ft_click = marker_screen + egui::vec2(3.0, 2.0);
    let _ = stage_input_frame(
        &mut editor,
        &ctx,
        2.0,
        vec![
            egui::Event::PointerMoved(ft_click),
            primary_button(ft_click, true),
        ],
    );
    let _ = stage_input_frame(
        &mut editor,
        &ctx,
        2.05,
        vec![primary_button(ft_click, false)],
    );
    assert_eq!(
        editor.selection.single().map(|t| t.element.as_str()),
        Some(mc_inst_id.as_str()),
        "Tool::Transform click selects empty MovieClip"
    );

    let drag_to = marker_screen + egui::vec2(40.0, -25.0);
    let _ = stage_input_frame(
        &mut editor,
        &ctx,
        3.0,
        vec![primary_button(marker_screen, true)],
    );
    let _ = stage_input_frame(
        &mut editor,
        &ctx,
        3.05,
        vec![egui::Event::PointerMoved(drag_to)],
    );
    let _ = stage_input_frame(
        &mut editor,
        &ctx,
        3.10,
        vec![primary_button(drag_to, false)],
    );
    let moved_t = editor.selected_element().unwrap().transform;
    assert!((moved_t.x - 240.0).abs() < 1e-4, "moved_t={moved_t:?}");
    assert!((moved_t.y - 125.0).abs() < 1e-4, "moved_t={moved_t:?}");
    assert_eq!(
        moved_t.pivot, None,
        "Zone::Pivot must not steal drag on empty symbol"
    );
    assert!(
        moved_t.rotation.abs() < 1e-6,
        "Zone::Rotate must not steal drag on empty symbol"
    );
    editor.undo();
    assert!((editor.selected_element().unwrap().transform.x - 200.0).abs() < 1e-4);
    assert!((editor.selected_element().unwrap().transform.y - 150.0).abs() < 1e-4);

    // 3. Double-clicking ○+ with Tool::Transform active enters Edit in Place (not swallowed by Pivot reset)!
    let _ = stage_input_frame(
        &mut editor,
        &ctx,
        4.0,
        vec![
            egui::Event::PointerMoved(marker_screen),
            primary_button(marker_screen, true),
        ],
    );
    let _ = stage_input_frame(
        &mut editor,
        &ctx,
        4.04,
        vec![primary_button(marker_screen, false)],
    );
    let _ = stage_input_frame(
        &mut editor,
        &ctx,
        4.10,
        vec![primary_button(marker_screen, true)],
    );
    let _ = stage_input_frame(
        &mut editor,
        &ctx,
        4.14,
        vec![primary_button(marker_screen, false)],
    );
    assert_eq!(
        editor.edit_depth(),
        2,
        "Tool::Transform double-click on empty MovieClip enters Edit in Place"
    );
    assert_eq!(editor.symbol_id(), mc_sym_id);
    editor.project.validate().unwrap();
}

#[test]
fn t380_empty_movie_clip_library_drop_and_edit_in_place_populate_clear_roundtrip() {
    use crate::app::Tool;
    use crate::tests::{primary_button, stage_input_frame};

    let ctx = egui::Context::default();
    let mut project = Project::blank();
    project.width = 400;
    project.height = 600;
    let mut editor = Editor::from_project(&ctx, project);
    add_empty_symbol(&mut editor, "empty_container");

    // Simulate Timeline focus on Frame 1 before dragging `empty_container` from Library onto Stage at (0, 600).
    let root_layer = editor.symbol().timeline.layers[0].id.clone();
    editor.select_frame_range(FrameRange {
        layers: vec![root_layer],
        start: 0,
        end: 0,
    });
    editor.timeline_edit.focused = true;
    editor.set_tool(Tool::Select);
    let _ = stage_input_frame(&mut editor, &ctx, 0.05, vec![]);

    let drop_screen = editor.stage_view.origin + egui::vec2(0.0, 600.0 * editor.zoom);
    editor.library_panel.drag = Some(crate::library::LibraryDrag::Symbol(
        "empty_container".into(),
    ));
    let _ = stage_input_frame(
        &mut editor,
        &ctx,
        0.10,
        vec![egui::Event::PointerMoved(drop_screen)],
    );
    assert!(
        !editor.selection.is_empty(),
        "dropping an empty MovieClip from Library onto Stage selects the new placement"
    );
    assert!(
        !editor.timeline_edit.focused,
        "dropping onto Stage transfers focus from Timeline to Stage"
    );
    assert_eq!(
        editor.selected_frame_range(),
        None,
        "dropping onto Stage clears the stale single-frame Timeline selection"
    );
    let placed_target = editor.selection.single().unwrap().clone();
    let placed_t = editor.selected_sampled().unwrap().transform;
    assert!((placed_t.x - 0.0).abs() < 1.0 && (placed_t.y - 600.0).abs() < 1.0);

    // Double-click ○+ at (0, 600) on Stage via real pointer events to enter Edit in Place,
    // draw graphics offset from (0, 0), and exit: registration crosshair stays at symbol (0, 0)
    // and Stage (x, y) stays preserved!
    let marker_click = drop_screen + egui::vec2(3.0, -3.0);
    let _ = stage_input_frame(
        &mut editor,
        &ctx,
        0.20,
        vec![
            egui::Event::PointerMoved(marker_click),
            primary_button(marker_click, true),
        ],
    );
    let _ = stage_input_frame(
        &mut editor,
        &ctx,
        0.24,
        vec![primary_button(marker_click, false)],
    );
    let _ = stage_input_frame(
        &mut editor,
        &ctx,
        0.30,
        vec![primary_button(marker_click, true)],
    );
    let _ = stage_input_frame(
        &mut editor,
        &ctx,
        0.34,
        vec![primary_button(marker_click, false)],
    );
    assert_eq!(editor.edit_depth(), 2);
    assert_eq!(editor.symbol_id(), "empty_container");
    editor.add_element(
        ElementKind::Rectangle {
            width: 80.0,
            height: 50.0,
            fill: [40, 140, 220, 255],
            stroke: [0; 4],
            stroke_width: 0.0,
        },
        25.0,
        -15.0,
    );
    assert!(editor.leave_symbol_to_level(0));
    assert_eq!(editor.edit_depth(), 1);
    assert_eq!(editor.selection.targets, vec![placed_target.clone()]);
    let after_draw_t = editor.selected_sampled().unwrap().transform;
    assert!((after_draw_t.x - placed_t.x).abs() < 1e-6);
    assert!((after_draw_t.y - placed_t.y).abs() < 1e-6);
    let populated_bounds = editor.selection_info_bounds(&ctx).unwrap().0;
    assert!((populated_bounds.width() - 80.0).abs() < 1e-3);
    assert!((populated_bounds.height() - 50.0).abs() < 1e-3);

    // Re-enter Edit in Place, delete all graphics inside, and exit:
    // empty ○+ remains selectable, alignable to Stage center (200, 300), and valid!
    assert!(editor.enter_selected_symbol_in_place());
    editor.select_all();
    editor.delete_selected();
    assert!(editor.leave_symbol_to_level(0));
    assert_eq!(editor.selection.targets, vec![placed_target.clone()]);
    let emptied_bounds = editor.selection_info_bounds(&ctx).unwrap().0;
    assert_eq!(emptied_bounds.size(), egui::Vec2::ZERO);

    editor.authoring_modify.to_stage = true;
    editor.align_selection(&ctx, 0, 1);
    editor.align_selection(&ctx, 1, 1);
    let centered_t = editor.selected_sampled().unwrap().transform;
    assert!((centered_t.x - 200.0).abs() < 1e-5);
    assert!((centered_t.y - 300.0).abs() < 1e-5);
    editor.project.validate().unwrap();
}

#[test]
fn t410_reflected_and_skewed_edit_in_place_roundtrip_dimming_and_stage_gizmos() {
    use crate::app::Tool;
    use crate::stage_mapping::StageMapping;
    use crate::tests::{primary_button, stage_input_frame};
    use neoflash_core::{IkArmature, IkBone, PathCommand};

    // 1. Direct oblique 2-axis skew (`skew_x = 25 deg`, `skew_y = -15 deg`) + negative scale
    //    (`scale_x = -1.0`, `scale_y = -1.5` and single-axis reflection `det < 0`) round-trips & NaN guards.
    let skew_x = 25.0_f64.to_radians();
    let skew_y = (-15.0_f64).to_radians();
    let oblique_reflected = [
        -skew_y.cos(),
        -skew_y.sin(),
        -1.5 * (-skew_x.sin()),
        -1.5 * skew_x.cos(),
        320.0,
        240.0,
    ];
    let single_flip_oblique = [
        -skew_y.cos(),
        -skew_y.sin(),
        1.5 * (-skew_x.sin()),
        1.5 * skew_x.cos(),
        320.0,
        240.0,
    ];
    assert!(
        StageMapping::try_new(single_flip_oblique)
            .unwrap()
            .determinant()
            < 0.0
    );
    for matrix in [oblique_reflected, single_flip_oblique] {
        let mapping = StageMapping::try_new(matrix).unwrap();
        for pt in [[0.0, 0.0], [42.5, -18.25], [-120.0, 87.5]] {
            let stage = mapping.local_to_stage(pt);
            let back = mapping.stage_to_local(stage);
            assert!((back[0] - pt[0]).abs() < 1e-8);
            assert!((back[1] - pt[1]).abs() < 1e-8);

            let origin = egui::pos2(140.0, 95.0);
            let screen =
                mapping.edit_to_screen(origin, 1.35, egui::pos2(pt[0] as f32, pt[1] as f32));
            let local_pos = mapping.screen_to_edit(origin, 1.35, screen);
            assert!((f64::from(local_pos.x) - pt[0]).abs() < 1e-3);
            assert!((f64::from(local_pos.y) - pt[1]).abs() < 1e-3);
        }
    }
    // Singular and NaN/Infinity matrices are rejected cleanly without NaN propagation.
    assert!(StageMapping::try_new([0.0, 0.0, 1.0, 2.0, 10.0, 20.0]).is_err());
    assert!(StageMapping::try_new([f64::NAN, 0.0, 0.0, 1.0, 0.0, 0.0]).is_err());
    assert_eq!(
        crate::stage::inverse([f64::NAN, 0.0, 0.0, 1.0, 0.0, 0.0], egui::pos2(5.0, 5.0)),
        None
    );
    assert_eq!(
        crate::stage::inverse([1.0, 2.0, 2.0, 4.0, 0.0, 0.0], egui::pos2(5.0, 5.0)),
        None
    );

    // 2. Build nested reflected + skewed Edit in Place project (`scale_x = -1.0`, `scale_y = -1.5`,
    //    `skew_x = 25 deg`, `skew_y = -15 deg`, AND a reflected `det < 0` child level) with parent siblings.
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    add_empty_symbol(&mut editor, "outer-skew");
    add_empty_symbol(&mut editor, "inner-flip");

    // Parent sibling context on root Stage: vibrant red rectangle + text + video placeholder.
    let parent_rect_color = [212, 54, 48, 255];
    editor.symbol_mut().timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: "parent-sibling-rect".into(),
            name: "parent-sibling-rect".into(),
            transform: Transform {
                x: 40.0,
                y: 40.0,
                ..Default::default()
            },
            kind: ElementKind::Rectangle {
                width: 60.0,
                height: 40.0,
                fill: parent_rect_color,
                stroke: [0; 4],
                stroke_width: 0.0,
            },
            ..Element::default()
        });
    editor.symbol_mut().timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: "parent-sibling-text".into(),
            name: "parent-sibling-text".into(),
            transform: Transform {
                x: 40.0,
                y: 100.0,
                scale_x: -1.0,
                scale_y: 1.2,
                skew: 20.0,
                ..Default::default()
            },
            kind: ElementKind::Text {
                text: "ReflectedLabel".into(),
                font_family: "Inter".into(),
                font_size: 16.0,
                color: [30, 60, 90, 255],
            },
            ..Element::default()
        });
    editor.symbol_mut().timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: "parent-sibling-oval".into(),
            name: "parent-sibling-oval".into(),
            transform: Transform {
                x: 180.0,
                y: 40.0,
                scale_x: -1.0,
                scale_y: 1.0,
                ..Default::default()
            },
            kind: ElementKind::Ellipse {
                width: 80.0,
                height: 50.0,
                fill: [70, 90, 180, 255],
                stroke: [20, 30, 60, 255],
                stroke_width: 1.0,
            },
            ..Element::default()
        });

    let outer_target = place(
        &mut editor,
        "outer-skew",
        Transform {
            x: 340.0,
            y: 260.0,
            scale_x: -1.0,
            scale_y: -1.5,
            rotation: -15.0,
            skew: 25.0,
            ..Default::default()
        },
    );
    editor.set_selection(vec![outer_target]);
    assert!(editor.enter_selected_symbol_in_place());

    // Place `inner-flip` with `scale_x = -1.0, scale_y = 1.0, skew = -15.0` so cumulative determinant < 0!
    let inner_target = place(
        &mut editor,
        "inner-flip",
        Transform {
            x: 20.0,
            y: -10.0,
            scale_x: -1.0,
            scale_y: 1.0,
            skew: -15.0,
            ..Default::default()
        },
    );
    let child_fill = [36, 184, 96, 255];
    editor.transact(|ed| {
        let inner_sym = ed.project.symbol_mut("inner-flip").unwrap();
        inner_sym.timeline.layers[0].keyframes[0].elements = vec![
            Element {
                id: "child-box".into(),
                name: "child-box".into(),
                transform: Transform {
                    x: 10.0,
                    y: 15.0,
                    ..Default::default()
                },
                kind: ElementKind::Rectangle {
                    width: 50.0,
                    height: 36.0,
                    fill: child_fill,
                    stroke: [0; 4],
                    stroke_width: 0.0,
                },
                ..Element::default()
            },
            Element {
                id: "child-bone-tip".into(),
                name: "child-bone-tip".into(),
                transform: Transform {
                    x: 90.0,
                    y: 15.0,
                    ..Default::default()
                },
                kind: ElementKind::Path {
                    commands: vec![
                        PathCommand::MoveTo { x: 0.0, y: 0.0 },
                        PathCommand::LineTo { x: 24.0, y: 0.0 },
                        PathCommand::LineTo { x: 24.0, y: 18.0 },
                        PathCommand::Close,
                    ],
                    fill_rule: neoflash_core::FillRule::NonZero,
                    fill: [60, 120, 230, 255],
                    stroke: [20, 40, 80, 255],
                    stroke_width: 2.0,
                },
                ..Element::default()
            },
        ];
    });
    editor.set_selection(vec![inner_target]);
    assert!(editor.enter_selected_symbol_in_place());
    assert_eq!(editor.symbol_id(), "inner-flip");

    let live_mapping = editor.edit_stage_mapping().unwrap();
    assert!(
        live_mapping.determinant() < 0.0,
        "cumulative Edit in Place transform has negative determinant (reflected + skewed): {}",
        live_mapping.determinant()
    );
    let round_local = editor
        .edit_stage_to_local(editor.edit_local_to_stage([35.0, 33.0]).unwrap())
        .unwrap();
    assert!((round_local[0] - 35.0).abs() < 1e-7);
    assert!((round_local[1] - 33.0).abs() < 1e-7);

    // Render Stage and verify dimmed parent context (`alpha == 191`), opaque child (`alpha == 255`),
    // finite non-inverted polygon/mesh winding, and accurate child hit-testing!
    editor.set_tool(Tool::Select);
    let out = stage_input_frame(&mut editor, &ctx, 0.05, vec![]);
    let mut saw_dimmed_parent = false;
    let mut saw_opaque_child = false;
    let expected_dimmed_parent = egui::Color32::from_rgba_unmultiplied(
        parent_rect_color[0],
        parent_rect_color[1],
        parent_rect_color[2],
        (255.0_f64 * 0.75) as u8,
    );
    let expected_opaque_child =
        egui::Color32::from_rgba_unmultiplied(child_fill[0], child_fill[1], child_fill[2], 255);
    fn color_matches(actual: egui::Color32, expected: egui::Color32) -> bool {
        (i16::from(actual.r()) - i16::from(expected.r())).abs() <= 2
            && (i16::from(actual.g()) - i16::from(expected.g())).abs() <= 2
            && (i16::from(actual.b()) - i16::from(expected.b())).abs() <= 2
            && (i16::from(actual.a()) - i16::from(expected.a())).abs() <= 2
    }
    fn inspect_shape(
        shape: &egui::Shape,
        dimmed_parent: egui::Color32,
        opaque_child: egui::Color32,
        saw_dimmed_parent: &mut bool,
        saw_opaque_child: &mut bool,
    ) {
        match shape {
            egui::Shape::Vec(nested) => {
                for item in nested {
                    inspect_shape(
                        item,
                        dimmed_parent,
                        opaque_child,
                        saw_dimmed_parent,
                        saw_opaque_child,
                    );
                }
            }
            egui::Shape::Rect(rect_shape) => {
                assert!(rect_shape.rect.is_finite());
                if color_matches(rect_shape.fill, dimmed_parent) {
                    *saw_dimmed_parent = true;
                }
                if color_matches(rect_shape.fill, opaque_child) {
                    *saw_opaque_child = true;
                }
            }
            egui::Shape::Mesh(mesh) => {
                for v in &mesh.vertices {
                    assert!(v.pos.is_finite(), "non-finite mesh vertex in Edit in Place");
                    if color_matches(v.color, dimmed_parent) {
                        *saw_dimmed_parent = true;
                    }
                    if color_matches(v.color, opaque_child) {
                        *saw_opaque_child = true;
                    }
                }
            }
            egui::Shape::Path(path) => {
                for pt in &path.points {
                    assert!(pt.is_finite(), "non-finite polygon vertex in Edit in Place");
                }
                if path.closed && path.fill != egui::Color32::TRANSPARENT {
                    if color_matches(path.fill, dimmed_parent) {
                        *saw_dimmed_parent = true;
                    }
                    if color_matches(path.fill, opaque_child) {
                        *saw_opaque_child = true;
                    }
                    if path.points.len() >= 3 {
                        let a = path.points[0];
                        let b = path.points[1];
                        let c = path.points[2];
                        let signed_area = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
                        assert!(
                            signed_area >= -1e-3,
                            "reflected closed filled PathShape must keep non-inverted winding, got {signed_area}"
                        );
                    }
                }
            }
            _ => {}
        }
    }
    for clipped in &out.shapes {
        inspect_shape(
            &clipped.shape,
            expected_dimmed_parent,
            expected_opaque_child,
            &mut saw_dimmed_parent,
            &mut saw_opaque_child,
        );
    }
    assert!(
        saw_dimmed_parent,
        "parent sibling rendered at 75% dimmed opacity"
    );
    assert!(saw_opaque_child, "active child rendered at 100% opacity");

    // Click mapped center of `child-box` (`local = (35, 33)`) -> selects `child-box`!
    let child_screen = live_mapping.edit_to_screen(
        editor.stage_view.origin,
        editor.zoom,
        egui::pos2(35.0, 33.0),
    );
    let _ = stage_input_frame(
        &mut editor,
        &ctx,
        0.10,
        vec![
            egui::Event::PointerMoved(child_screen),
            primary_button(child_screen, true),
        ],
    );
    let _ = stage_input_frame(
        &mut editor,
        &ctx,
        0.14,
        vec![primary_button(child_screen, false)],
    );
    assert_eq!(
        editor.selection.single().map(|t| t.element.as_str()),
        Some("child-box")
    );

    // 3A. 3D Translation & Rotation gizmos inside reflected + skewed Edit in Place map pointer deltas cleanly!
    let pre_3d_project = editor.project.clone();
    editor.set_tool(Tool::Translate3D);
    let _ = stage_input_frame(&mut editor, &ctx, 0.20, vec![]);
    let (_, _, gizmo_geom) = editor
        .selected_3d_gizmo_screen_geometry(editor.stage_view.origin, live_mapping)
        .expect("3D gizmo geometry available for selected child");
    let x_drag_dir = (gizmo_geom.x_tip - gizmo_geom.center).normalized();
    let x_end = gizmo_geom.x_tip + x_drag_dir * 36.0;
    let _ = stage_input_frame(
        &mut editor,
        &ctx,
        0.24,
        vec![
            egui::Event::PointerMoved(gizmo_geom.x_tip),
            primary_button(gizmo_geom.x_tip, true),
        ],
    );
    let _ = stage_input_frame(
        &mut editor,
        &ctx,
        0.28,
        vec![egui::Event::PointerMoved(x_end)],
    );
    let _ = stage_input_frame(&mut editor, &ctx, 0.32, vec![primary_button(x_end, false)]);
    let state_after_3d = editor.element_3d_state("child-box");
    assert!(
        state_after_3d.translate_3d[0] > 5.0,
        "dragging along mapped +X 3D handle must increase local translate_3d[0], got {:?}",
        state_after_3d.translate_3d
    );
    assert!(
        state_after_3d.translate_3d[1].abs() < 1.0,
        "dragging purely along mapped +X 3D handle must keep translate_3d[1] ~ 0, got {:?}",
        state_after_3d.translate_3d
    );

    editor.set_tool(Tool::Rotate3D);
    let _ = stage_input_frame(&mut editor, &ctx, 0.34, vec![]);
    let (_, _, rot_geom) = editor
        .selected_3d_gizmo_screen_geometry(editor.stage_view.origin, live_mapping)
        .unwrap();
    let y_drag_dir = (rot_geom.y_tip - rot_geom.center).normalized();
    let rot_end = rot_geom.x_tip + y_drag_dir * 30.0;
    let _ = stage_input_frame(
        &mut editor,
        &ctx,
        0.35,
        vec![
            egui::Event::PointerMoved(rot_geom.x_tip),
            primary_button(rot_geom.x_tip, true),
        ],
    );
    let _ = stage_input_frame(
        &mut editor,
        &ctx,
        0.37,
        vec![egui::Event::PointerMoved(rot_end)],
    );
    let _ = stage_input_frame(
        &mut editor,
        &ctx,
        0.39,
        vec![primary_button(rot_end, false)],
    );
    assert!(
        editor.element_3d_state("child-box").rotate_3d[0].abs() > 2.0,
        "Rotate3D handle drag mapped cleanly through reflected/skewed Edit in Place"
    );
    if editor.project != pre_3d_project {
        editor.undo();
    }
    editor.project.validate().unwrap();

    // 3B. Bone IK effector gizmo drag inside reflected + skewed Edit in Place!
    assert_eq!(editor.symbol_id(), "inner-flip");
    editor.transact(|ed| {
        let inner_sym = ed.project.symbol_mut("inner-flip").unwrap();
        inner_sym.timeline.layers[0].keyframes[0].ik_armatures = vec![IkArmature {
            id: "arm-eip".into(),
            name: "EipRig".into(),
            bones: vec![
                IkBone {
                    id: "bone-root".into(),
                    element_id: "child-box".into(),
                    parent_bone_id: None,
                    length: 80.0,
                    min_angle: None,
                    max_angle: None,
                    stiffness: 0.2,
                },
                IkBone {
                    id: "bone-leaf".into(),
                    element_id: "child-bone-tip".into(),
                    parent_bone_id: Some("bone-root".into()),
                    length: 24.0,
                    min_angle: None,
                    max_angle: None,
                    stiffness: 0.2,
                },
            ],
            effector_bone_id: "bone-leaf".into(),
            target_pos: [114.0, 15.0],
            pole_pos: None,
        }];
    });
    editor.set_tool(Tool::Bone);
    let _ = stage_input_frame(&mut editor, &ctx, 0.40, vec![]);
    let eff_start_screen = live_mapping.edit_to_screen(
        editor.stage_view.origin,
        editor.zoom,
        egui::pos2(114.0, 15.0),
    );
    let desired_local_target = egui::pos2(85.0, 62.0);
    let eff_end_screen =
        live_mapping.edit_to_screen(editor.stage_view.origin, editor.zoom, desired_local_target);
    let _ = stage_input_frame(
        &mut editor,
        &ctx,
        0.44,
        vec![
            egui::Event::PointerMoved(eff_start_screen),
            primary_button(eff_start_screen, true),
        ],
    );
    let _ = stage_input_frame(
        &mut editor,
        &ctx,
        0.48,
        vec![egui::Event::PointerMoved(eff_end_screen)],
    );
    let _ = stage_input_frame(
        &mut editor,
        &ctx,
        0.52,
        vec![primary_button(eff_end_screen, false)],
    );
    let solved_target = editor.symbol().timeline.layers[0].keyframes[0].ik_armatures[0].target_pos;
    assert!(
        (solved_target[0] - f64::from(desired_local_target.x)).abs() < 1.0
            && (solved_target[1] - f64::from(desired_local_target.y)).abs() < 1.0,
        "IK effector target mapped cleanly to local symbol coordinates: {solved_target:?}"
    );
    editor.undo();

    // 3C. 9-slice guides inside oblique/reflected Edit in Place map pointer drag & oblique dashes!
    editor.set_tool(Tool::Select);
    editor
        .set_symbol_scale9_grid("inner-flip", Some([20.0, 15.0, 60.0, 40.0]))
        .unwrap();
    let guide_x0_screen = live_mapping.edit_to_screen(
        editor.stage_view.origin,
        editor.zoom,
        egui::pos2(20.0, 35.0),
    );
    let guide_x0_dragged = live_mapping.edit_to_screen(
        editor.stage_view.origin,
        editor.zoom,
        egui::pos2(32.0, 35.0),
    );
    let _ = stage_input_frame(
        &mut editor,
        &ctx,
        0.60,
        vec![
            egui::Event::PointerMoved(guide_x0_screen),
            primary_button(guide_x0_screen, true),
        ],
    );
    let _ = stage_input_frame(
        &mut editor,
        &ctx,
        0.64,
        vec![egui::Event::PointerMoved(guide_x0_dragged)],
    );
    let _ = stage_input_frame(
        &mut editor,
        &ctx,
        0.68,
        vec![primary_button(guide_x0_dragged, false)],
    );
    let updated_grid = editor
        .symbol()
        .scale9_grid()
        .expect("scale9_grid stays active");
    assert!(
        (updated_grid[0] - 32.0).abs() < 0.25,
        "dragging oblique 9-slice left guide mapped to local x=32.0, got {updated_grid:?}"
    );
    editor.project.validate().unwrap();
}
