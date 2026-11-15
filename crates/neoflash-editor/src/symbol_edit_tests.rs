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
