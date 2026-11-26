use neoflash_core::{
    JsflAuthoringState, JsflSandbox, Keyframe, Layer, LayerProperties, LayerRole, Project,
};

#[test]
fn fo09_jsfl_remove_frames_trims_all_unlocked_non_folder_layers_and_recalculates_frame_count() {
    let mut project = Project::blank();
    assert_eq!(
        project.symbol(&project.root).unwrap().timeline.frame_count,
        120,
        "Project::blank() starts with 120 frames"
    );

    let sandbox = JsflSandbox::new();
    let state = JsflAuthoringState::for_project(&project);
    let script = r#"
        var tl = fl.getDocumentDOM().getTimeline();
        tl.addNewLayer("Vault Background", "normal");
        tl.addNewLayer("UI Folder", "folder");
        tl.addNewLayer("CRT Terminal", "normal");
        tl.addNewLayer("Actions", "normal");
        tl.removeFrames(1, tl.frameCount);
        fl.trace("frameCount=" + tl.frameCount + " layers=" + tl.layerCount);
    "#;
    let res = sandbox
        .execute(&project, &state, "setup_vault_ui.jsfl", script)
        .expect("JSFL removeFrames across multi-layer scene must succeed");
    res.project.validate().expect("project must stay valid");

    let root = res.project.symbol(&res.project.root).unwrap();
    assert_eq!(
        root.timeline.frame_count, 1,
        "removeFrames(1, tl.frameCount) without explicit single-layer selection must trim all unlocked non-folder layers to 1 frame"
    );
    assert_eq!(root.timeline.layers.len(), 5);
    assert_eq!(res.output_lines, vec!["frameCount=1 layers=5"]);

    // Verify locked layers are skipped when no explicit layer selection is active,
    // while explicit setSelectedLayers(idx, true) restricts removeFrames to that layer.
    let root_id = project.root.clone();
    {
        let root_mut = project.symbol_mut(&root_id).unwrap();
        root_mut.timeline.frame_count = 10;
        root_mut.timeline.layers.push(Layer {
            id: "locked-bg".into(),
            name: "Locked BG".into(),
            visible: true,
            locked: true,
            keyframes: vec![Keyframe {
                frame: 0,
                label: "bg0".into(),
                ..Keyframe::default()
            }],
            properties: LayerProperties::default(),
        });
        root_mut.timeline.layers.push(Layer {
            id: "folder-row".into(),
            name: "Group".into(),
            visible: true,
            locked: false,
            keyframes: Vec::new(),
            properties: LayerProperties {
                role: LayerRole::Folder,
                ..LayerProperties::default()
            },
        });
        root_mut.timeline.layers.push(Layer {
            id: "fx-row".into(),
            name: "FX".into(),
            visible: true,
            locked: false,
            keyframes: vec![
                Keyframe {
                    frame: 0,
                    label: "fx0".into(),
                    ..Keyframe::default()
                },
                Keyframe {
                    frame: 5,
                    label: "fx5".into(),
                    ..Keyframe::default()
                },
            ],
            properties: LayerProperties::default(),
        });
    }
    project.format_version = 4;
    project.validate().unwrap();

    let state2 = JsflAuthoringState::for_project(&project);
    let partial = sandbox
        .execute(
            &project,
            &state2,
            "skip_locked.jsfl",
            "fl.getDocumentDOM().getTimeline().removeFrames(1, 5);",
        )
        .unwrap();
    partial.project.validate().unwrap();
    let p_tl = &partial.project.symbol(&root_id).unwrap().timeline;
    // Unlocked layers 0 and 3 lost 4 frames (1..5), shifting fx5 -> frame 1 (exposure 6),
    // while locked layer 1 stayed untouched at exposure 10.
    assert_eq!(p_tl.frame_count, 10);
    assert_eq!(
        p_tl.layers[0]
            .properties
            .end_frame
            .unwrap_or(p_tl.frame_count),
        6
    );
    assert_eq!(
        p_tl.layers[1]
            .properties
            .end_frame
            .unwrap_or(p_tl.frame_count),
        10
    );
    assert_eq!(p_tl.layers[3].keyframes[1].frame, 1);
    assert_eq!(p_tl.layers[3].keyframes[1].label, "fx5");

    // Explicit single-layer selection via setSelectedLayers restricts removeFrames to layer 3 only.
    let single = sandbox
        .execute(
            &project,
            &state2,
            "single_selected.jsfl",
            "var tl = fl.getDocumentDOM().getTimeline(); tl.setSelectedLayers(3, true); tl.removeFrames(1, 5);",
        )
        .unwrap();
    single.project.validate().unwrap();
    let s_tl = &single.project.symbol(&root_id).unwrap().timeline;
    assert_eq!(
        s_tl.layers[0]
            .properties
            .end_frame
            .unwrap_or(s_tl.frame_count),
        10
    );
    assert_eq!(
        s_tl.layers[3]
            .properties
            .end_frame
            .unwrap_or(s_tl.frame_count),
        6
    );
    assert_eq!(s_tl.layers[3].keyframes[1].frame, 1);
}
