use super::*;

fn test_project() -> Project {
    Project {
        format_version: 6,
        name: "TestDoc".to_owned(),
        width: 800,
        height: 600,
        fps: 24,
        background: [255, 255, 255, 255],
        root: "root-sym".to_owned(),
        symbols: vec![Symbol {
            id: "root-sym".to_owned(),
            name: "Scene 1".to_owned(),
            folder: String::new(),
            kind: SymbolKind::MovieClip,
            timeline: crate::Timeline {
                frame_count: 1,
                layers: vec![Layer {
                    id: "layer-1".to_owned(),
                    name: "Layer 1".to_owned(),
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: Vec::new(),
                        ..Default::default()
                    }],
                    properties: Default::default(),
                }],
                scripts: Vec::new(),
                guides: Vec::new(),
            },
            class_name: None,
            linkage_id: None,
        }],
        library_folders: Vec::new(),
        sources: Vec::new(),
        resources: Vec::new(),
        document_class: None,
        scenes: Vec::new(),
        script_language: None,
        also_compile_compatible_scripts: true,
    }
}

#[test]
fn native_and_jsfl_smart_batch_rename_across_library_and_nested_instances() {
    let mut project = test_project();
    project.symbols.push(Symbol {
        id: "sym-enemy-a".to_owned(),
        name: "old_enemy_grunt".to_owned(),
        folder: "Enemies".to_owned(),
        kind: SymbolKind::MovieClip,
        timeline: crate::Timeline {
            frame_count: 1,
            layers: vec![Layer {
                id: "enemy-layer".to_owned(),
                name: "Body".to_owned(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: Vec::new(),
                    ..Default::default()
                }],
                properties: Default::default(),
            }],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    });
    project.symbols.push(Symbol {
        id: "sym-enemy-b".to_owned(),
        name: "old_enemy_boss".to_owned(),
        folder: "Enemies".to_owned(),
        kind: SymbolKind::MovieClip,
        timeline: crate::Timeline {
            frame_count: 1,
            layers: vec![Layer {
                id: "boss-layer".to_owned(),
                name: "Hold".to_owned(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![Element {
                        id: "nested-inst".to_owned(),
                        name: "old_enemy_grunt_minion".to_owned(),
                        transform: Transform::default(),
                        kind: ElementKind::Instance {
                            symbol_id: "sym-enemy-a".to_owned(),
                            playback: InstancePlayback::Independent,
                        },
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                properties: Default::default(),
            }],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    });
    project.library_folders.push("Enemies".to_owned());

    // Place instances across multiple keyframes on root timeline.
    project.symbols[0].timeline.frame_count = 5;
    project.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: "stage-inst-1".to_owned(),
            name: "old_enemy_grunt".to_owned(),
            transform: Transform::default(),
            kind: ElementKind::Instance {
                symbol_id: "sym-enemy-a".to_owned(),
                playback: InstancePlayback::Independent,
            },
            ..Default::default()
        });
    project.symbols[0].timeline.layers[0]
        .keyframes
        .push(Keyframe {
            frame: 3,
            elements: vec![Element {
                id: "stage-inst-2".to_owned(),
                name: String::new(),
                transform: Transform::default(),
                kind: ElementKind::Instance {
                    symbol_id: "sym-enemy-b".to_owned(),
                    playback: InstancePlayback::Independent,
                },
                ..Default::default()
            }],
            ..Default::default()
        });

    let opts = BatchRenameOptions {
        scope: BatchRenameScope::LibraryAndInstances,
        target_symbol_ids: None,
        name_filter: Some("old_enemy_*".to_owned()),
        find: Some("old_enemy_".to_owned()),
        replace: "foe_".to_owned(),
        prefix: "ui_".to_owned(),
        suffix: "_v2_".to_owned(),
        sequence_start: Some(1),
        sequence_step: 1,
        sequence_digits: 3,
        sync_instance_names_to_symbol: true,
    };
    let report = batch_rename_library_and_instances(&mut project, &opts).unwrap();
    assert_eq!(report.renamed_symbols.len(), 2);
    assert_eq!(
        project.symbol("sym-enemy-a").unwrap().name,
        "ui_foe_grunt_v2_001"
    );
    assert_eq!(
        project.symbol("sym-enemy-b").unwrap().name,
        "ui_foe_boss_v2_002"
    );
    assert_eq!(report.renamed_instances.len(), 3);
    assert_eq!(
        project.symbols[0].timeline.layers[0].keyframes[0].elements[0].name,
        "ui_foe_grunt_v2_001_01"
    );
    assert_eq!(
        project.symbols[0].timeline.layers[0].keyframes[1].elements[0].name,
        "ui_foe_boss_v2_002_02"
    );
    assert_eq!(
        project.symbol("sym-enemy-b").unwrap().timeline.layers[0].keyframes[0].elements[0].name,
        "ui_foe_grunt_v2_001_minion"
    );
    assert!(validate(&project).is_empty());

    // Also test invoking batchRename from JSFL DOM and inspecting items/elements via JSFL.
    let sandbox = JsflSandbox::new();
    let state = JsflAuthoringState::for_project(&project);
    let script = r##"
        var doc = fl.getDocumentDOM();
        var res = doc.library.batchRename({
            scope: "library_and_instances",
            filter: "ui_foe_*",
            find: "ui_foe_",
            replace: "actor_",
            sequenceStart: 1,
            sequenceDigits: 2
        });
        fl.trace("symbols=" + res.renamedSymbols + " instances=" + res.renamedInstances);
    "##;
    let jsfl_res = sandbox
        .execute(&project, &state, "batch_rename.jsfl", script)
        .unwrap();
    assert_eq!(jsfl_res.output_lines, vec!["symbols=2 instances=3"]);
    assert_eq!(
        jsfl_res.project.symbol("sym-enemy-a").unwrap().name,
        "actor_grunt_v2_00101"
    );
}

#[test]
fn jsfl_builds_transforms_groups_converts_artwork_and_constructs_tween_timeline() {
    let mut project = test_project();
    let mut state = JsflAuthoringState::for_project(&project);
    let sandbox = JsflSandbox::new();

    let script = r##"
        var doc = fl.getDocumentDOM();
        doc.width = 1280;
        doc.height = 720;
        doc.frameRate = 60;
        doc.backgroundColor = "#102030";

        // Build shapes and text
        doc.addNewRectangle({left: 10, top: 20, right: 110, bottom: 70}, 8);
        var sel = doc.selection;
        sel[0].setCustomFill({style: "solid", color: "#ff3366"});
        sel[0].setCustomStroke({style: "solid", color: "#112233", thickness: 3});

        doc.addNewOval({left: 120, top: 20, right: 180, bottom: 80});
        doc.selectAll();
        doc.group();
        doc.moveSelectionBy({x: 15, y: 25});
        doc.scaleSelection(2.0, 1.5);
        doc.rotateSelection(30);
        doc.unGroup();

        // Convert selection to MovieClip symbol in Library and organize folders
        var symItem = doc.convertToSymbol("movie clip", "HeroBadge", "center");
        symItem.linkageIdentifier = "hero_badge_id";
        doc.library.newFolder("UI/Badges");
        doc.library.moveToFolder("UI/Badges", "HeroBadge", true);
        doc.library.duplicateItem("UI/Badges/HeroBadge");

        // Add text element and update its string
        doc.addNewText({left: 40, top: 200, right: 240, bottom: 240}, "Initial");
        doc.selection[0].setTextString("Score: 100");
        doc.selection[0].name = "score_txt";

        // Timeline & Layer construction, tweening, labels, and frame scripts
        var tl = doc.getTimeline();
        var fxLayerIdx = tl.addNewLayer("FX Layer", "normal");
        tl.reorderLayer(fxLayerIdx, 0, true);
        tl.setSelectedLayers(1, true);
        tl.insertFrames(9, true, 0);
        tl.insertKeyframe(9);
        doc.selectAll();
        doc.moveSelectionBy({x: 200, y: 50});
        tl.createMotionTween(0, 9);

        var mainLayer = tl.layers[1];
        mainLayer.frames[0].name = "intro";
        mainLayer.frames[0].actionScript = "stop();";
        mainLayer.frames[0].tweenEasing = 50;

        an.outputPanel.clear();
        fl.trace(doc.width, doc.height, doc.frameRate, doc.backgroundColor);
        fl.trace(tl.layerCount, tl.frameCount, mainLayer.frames[0].tweenType);
        fl.trace(doc.library.itemExists("UI/Badges/HeroBadge"));
    "##;

    let res = sandbox
        .execute_in_place(&mut project, &mut state, "author_scene.jsfl", script)
        .unwrap();
    assert_eq!(res.undo_label, "Run JSFL: author_scene");
    assert!(state.dirty);
    assert_eq!(project.width, 1280);
    assert_eq!(project.height, 720);
    assert_eq!(project.fps, 60);
    assert_eq!(project.background, [0x10, 0x20, 0x30, 0xFF]);
    assert_eq!(
        res.output_lines,
        vec!["1280 720 60 #102030", "2 10 motion", "true"]
    );
    let root_sym = project.symbol("root-sym").unwrap();
    assert_eq!(root_sym.timeline.layers.len(), 2);
    assert_eq!(root_sym.timeline.layers[0].name, "FX Layer");
    assert_eq!(root_sym.timeline.frame_count, 10);
    assert_eq!(root_sym.timeline.layers[1].keyframes[0].label, "intro");
    assert_eq!(
        root_sym.timeline.layers[1].keyframes[0].tween,
        Some(Easing::Flash { acceleration: 50.0 })
    );
    assert_eq!(root_sym.timeline.scripts[0].script.source, "stop();");
    let badge = project
        .symbols
        .iter()
        .find(|s| s.name == "HeroBadge")
        .unwrap();
    assert_eq!(badge.folder, "UI/Badges");
    assert_eq!(badge.linkage_id.as_deref(), Some("hero_badge_id"));
    assert!(project.symbols.iter().any(|s| s.name == "HeroBadge copy"));
    assert!(validate(&project).is_empty());
}

#[test]
fn jsfl_insert_and_remove_frames_transform_keys_and_scripts_atomically() {
    let mut project = test_project();
    let timeline = &mut project.symbols[0].timeline;
    timeline.frame_count = 6;
    timeline.layers[0].visible = false;
    timeline.layers[0].locked = true;
    timeline.layers[0].keyframes = vec![
        Keyframe {
            frame: 0,
            label: "old-start".into(),
            ..Keyframe::default()
        },
        Keyframe {
            frame: 3,
            label: "boundary".into(),
            ..Keyframe::default()
        },
    ];
    timeline.scripts = [(1, "removed();"), (3, "atBoundary();"), (5, "after();")]
        .into_iter()
        .map(|(frame, source)| FrameScript {
            layer: None,
            frame,
            script: Script {
                language: ScriptLanguage::JavaScript,
                source: source.into(),
            },
        })
        .collect();
    project.validate().unwrap();

    let sandbox = JsflSandbox::new();
    let state = JsflAuthoringState::for_project(&project);
    let removed = sandbox
        .execute(
            &project,
            &state,
            "remove_frames.jsfl",
            "fl.getDocumentDOM().getTimeline().removeFrames(0, 3);",
        )
        .unwrap();
    let timeline = &removed.project.symbols[0].timeline;
    assert_eq!(timeline.frame_count, 3);
    assert_eq!(
        timeline.layers[0]
            .keyframes
            .iter()
            .map(|key| (key.frame, key.label.as_str()))
            .collect::<Vec<_>>(),
        vec![(0, "boundary")]
    );
    assert_eq!(
        timeline
            .scripts
            .iter()
            .map(|script| (script.frame, script.script.source.as_str()))
            .collect::<Vec<_>>(),
        vec![(0, "atBoundary();"), (2, "after();")]
    );
    assert!(!timeline.layers[0].visible);
    assert!(timeline.layers[0].locked);
    removed.project.validate().unwrap();

    let inserted = sandbox
        .execute(
            &removed.project,
            &removed.state,
            "insert_frames.jsfl",
            "fl.getDocumentDOM().getTimeline().insertFrames(2, true, 0);",
        )
        .unwrap();
    let timeline = &inserted.project.symbols[0].timeline;
    assert_eq!(timeline.frame_count, 5);
    assert_eq!(timeline.layers[0].keyframes[0].frame, 0);
    assert_eq!(
        timeline
            .scripts
            .iter()
            .map(|script| (script.frame, script.script.source.as_str()))
            .collect::<Vec<_>>(),
        vec![(0, "atBoundary();"), (4, "after();")]
    );
    assert!(!timeline.layers[0].visible);
    assert!(timeline.layers[0].locked);
    inserted.project.validate().unwrap();
}

#[test]
fn atomic_rollback_on_throw_timeout_stale_handle_and_invalid_project() {
    let original_project = test_project();
    let original_state = JsflAuthoringState::for_project(&original_project);

    // 1. Uncaught throw after mutating document and layers
    let mut p1 = original_project.clone();
    let mut s1 = original_state.clone();
    let sandbox = JsflSandbox::new();
    let err_throw = sandbox
        .execute_in_place(
            &mut p1,
            &mut s1,
            "fail_throw.jsfl",
            "var doc = fl.getDocumentDOM();\ndoc.width = 1024;\ndoc.getTimeline().addNewLayer('Temp');\nthrow 'boom';\n",
        )
        .unwrap_err();
    assert_eq!(err_throw.file, "fail_throw.jsfl");
    assert_eq!(err_throw.line, 4);
    assert!(err_throw.message.contains("boom"));
    assert_eq!(p1, original_project);
    assert_eq!(s1, original_state);

    // 2. Step/fuel timeout cannot be swallowed by try/catch and rolls back
    let mut p2 = original_project.clone();
    let mut s2 = original_state.clone();
    let tight_sandbox = JsflSandbox::new().with_limits(JsflLimits {
        max_steps: 60,
        ..Default::default()
    });
    let err_timeout = tight_sandbox
        .execute_in_place(
            &mut p2,
            &mut s2,
            "infinite.jsfl",
            "fl.getDocumentDOM().width = 999;\ntry { while (true) {} } catch (e) {}\n",
        )
        .unwrap_err();
    assert_eq!(err_timeout.file, "infinite.jsfl");
    assert_eq!(err_timeout.line, 2);
    assert!(err_timeout.message.contains("exceeded step limit"));
    assert_eq!(p2, original_project);
    assert_eq!(s2, original_state);

    // 3. Stale handle access after deleting layer or element rolls back
    let mut p3 = original_project.clone();
    let mut s3 = original_state.clone();
    let err_stale = sandbox
        .execute_in_place(
            &mut p3,
            &mut s3,
            "stale.jsfl",
            "var tl = fl.getDocumentDOM().getTimeline();\nvar idx = tl.addNewLayer('Temp');\nvar staleLayer = tl.layers[idx];\ntl.deleteLayer(idx);\nstaleLayer.name = 'Ghost';\n",
        )
        .unwrap_err();
    assert_eq!(err_stale.file, "stale.jsfl");
    assert_eq!(err_stale.line, 5);
    assert!(err_stale.message.contains("Stale LayerHandle"));
    assert_eq!(p3, original_project);
    assert_eq!(s3, original_state);

    // 4. Invalid project property rolls back
    let mut p4 = original_project.clone();
    let mut s4 = original_state.clone();
    let err_invalid = sandbox
        .execute_in_place(
            &mut p4,
            &mut s4,
            "invalid_doc.jsfl",
            "var doc = fl.getDocumentDOM();\ndoc.frameRate = 120;\ndoc.width = 999999;\n",
        )
        .unwrap_err();
    assert_eq!(err_invalid.file, "invalid_doc.jsfl");
    assert_eq!(err_invalid.line, 3);
    assert_eq!(p4, original_project);
    assert_eq!(s4, original_state);

    // 5. Post-execution project.validate() failure (duplicate linkageIdentifier) rolls back
    let mut p5 = original_project.clone();
    let mut s5 = original_state.clone();
    let err_val = sandbox
        .execute_in_place(
            &mut p5,
            &mut s5,
            "dup_linkage.jsfl",
            "var lib = fl.getDocumentDOM().library;\nlib.addNewItem('movie clip', 'SymA');\nlib.addNewItem('movie clip', 'SymB');\nlib.items[0].linkageIdentifier = 'dup_id';\nlib.items[1].linkageIdentifier = 'dup_id';\n",
        )
        .unwrap_err();
    assert_eq!(err_val.file, "dup_linkage.jsfl");
    assert!(err_val.message.contains("failed validation"));
    assert_eq!(p5, original_project);
    assert_eq!(s5, original_state);
}

#[test]
fn flfile_capabilities_traversal_symlink_escape_output_cap_and_run_script_guards() {
    let project = test_project();
    let state = JsflAuthoringState::for_project(&project);

    // 1. Default sandbox denies FLfile read/write/delete
    let default_sb = JsflSandbox::new();
    let deny_err = default_sb
        .execute(
            &project,
            &state,
            "deny.jsfl",
            "FLfile.exists('file:///tmp/test.txt');",
        )
        .unwrap_err();
    assert!(deny_err.message.contains("denied"));

    // 2. Scoped virtual directory with read-only first, then read+write+delete
    let tmp_base = PathBuf::from("/virtual/neoflash_jsfl_test");
    let allowed_dir = tmp_base.join("allowed");
    let outside_dir = tmp_base.join("outside");
    let secret_file = outside_dir.join("secret.txt");
    let mem_fs = std::sync::Arc::new(MemoryJsflFileSystem::new());
    mem_fs.insert_folder(&allowed_dir);
    mem_fs.insert_folder(&outside_dir);
    mem_fs.insert_file(&secret_file, "top-secret");
    mem_fs.insert_symlink(allowed_dir.join("escape_link.txt"), &secret_file);

    let ro_sb = JsflSandbox::new()
        .with_file_system(mem_fs.clone())
        .with_flfile_capabilities(FlFileCapabilities {
            allowed_root: Some(allowed_dir.clone()),
            allow_read: true,
            allow_write: false,
            allow_delete: false,
        });

    // Write and delete rejected in read-only capability mode
    assert!(
        ro_sb
            .execute(
                &project,
                &state,
                "ro_write.jsfl",
                "FLfile.write('file:///notes.txt', 'hello');",
            )
            .is_err()
    );
    assert!(
        ro_sb
            .execute(
                &project,
                &state,
                "ro_del.jsfl",
                "FLfile.remove('file:///notes.txt');",
            )
            .is_err()
    );

    // Path traversal rejected
    let trav_err = ro_sb
        .execute(
            &project,
            &state,
            "traversal.jsfl",
            "FLfile.read('file:///../outside/secret.txt');",
        )
        .unwrap_err();
    assert!(trav_err.message.contains("traversal"));

    // Symlink escape rejected
    let sym_err = ro_sb
        .execute(
            &project,
            &state,
            "symlink.jsfl",
            "FLfile.read('file:///escape_link.txt');",
        )
        .unwrap_err();
    assert!(sym_err.message.contains("escapes allowed_root"));

    // Full read/write/delete within allowed_root succeeds
    let rw_sb = JsflSandbox::new()
        .with_file_system(mem_fs)
        .with_flfile_capabilities(FlFileCapabilities {
            allowed_root: Some(allowed_dir.clone()),
            allow_read: true,
            allow_write: true,
            allow_delete: true,
        });
    let file_res = rw_sb
        .execute(
            &project,
            &state,
            "file_ops.jsfl",
            r#"
            FLfile.createFolder("file:///sub");
            FLfile.write("file:///sub/data.txt", "alpha");
            FLfile.write("file:///sub/data.txt", "-beta", "append");
            var existsBefore = FLfile.exists("file:///sub/data.txt");
            var content = FLfile.read("file:///sub/data.txt");
            var listing = FLfile.listFolder("file:///sub", true);
            FLfile.remove("file:///sub/data.txt");
            var existsAfter = FLfile.exists("file:///sub/data.txt");
            fl.trace(existsBefore, content, listing.join(","), existsAfter);
        "#,
        )
        .unwrap();
    assert_eq!(
        file_res.output_lines,
        vec!["true alpha-beta data.txt false"]
    );

    // 3. Output flooding cap
    let cap_sb = JsflSandbox::new().with_limits(JsflLimits {
        max_output_lines: 5,
        ..Default::default()
    });
    let flood_err = cap_sb
        .execute(
            &project,
            &state,
            "flood.jsfl",
            "for (var i = 0; i < 20; i++) { fl.trace(i); }",
        )
        .unwrap_err();
    assert!(flood_err.message.contains("maximum trace lines"));

    // 4. Nested fl.runScript recursion guard and unsupported XMLUI diagnostic
    let mut rec_sb = JsflSandbox::new().with_limits(JsflLimits {
        max_script_recursion: 3,
        ..Default::default()
    });
    rec_sb.register_virtual_script(
        "file:///Commands/self.jsfl",
        "fl.runScript('file:///Commands/self.jsfl');",
    );
    let rec_err = rec_sb
        .execute(
            &project,
            &state,
            "entry.jsfl",
            "fl.runScript('file:///Commands/self.jsfl');",
        )
        .unwrap_err();
    assert!(rec_err.message.contains("recursion limit"));

    let xmlui_err = default_sb
        .execute(&project, &state, "panel.jsfl", "var x = fl.xmlui;")
        .unwrap_err();
    assert!(
        xmlui_err
            .message
            .contains("Unsupported JSFL property 'fl.xmlui'")
    );
}

#[test]
fn jsfl_change_color_script_replaces_color_across_entire_project_dom() {
    let mut project = test_project();
    let target_red = [230, 57, 70, 255];
    let replacement_blue = [29, 53, 87, 255];
    let untouched_green = [42, 157, 143, 255];

    // 1. Root scene elements: solid rectangle, gradient painted path, and text
    project.symbols[0].timeline.layers[0].keyframes[0].elements = vec![
        Element {
            id: "rect-1".to_owned(),
            name: String::new(),
            transform: Transform::default(),
            kind: ElementKind::Rectangle {
                width: 120.0,
                height: 80.0,
                fill: target_red,
                stroke: untouched_green,
                stroke_width: 2.0,
            },
            ..Default::default()
        },
        Element {
            id: "grad-1".to_owned(),
            name: String::new(),
            transform: Transform::default(),
            kind: ElementKind::PaintedPath {
                commands: vec![
                    PathCommand::MoveTo { x: 0.0, y: 0.0 },
                    PathCommand::LineTo { x: 50.0, y: 0.0 },
                    PathCommand::LineTo { x: 50.0, y: 50.0 },
                    PathCommand::Close,
                ],
                fill_rule: FillRule::NonZero,
                fill: target_red,
                stroke: target_red,
                fill_gradient: Some(crate::GradientPaint {
                    kind: crate::GradientKind::Linear,
                    matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                    stops: vec![
                        crate::GradientStop {
                            ratio: 0,
                            color: target_red,
                        },
                        crate::GradientStop {
                            ratio: 255,
                            color: untouched_green,
                        },
                    ],
                    ..Default::default()
                }),
                stroke_gradient: None,
                stroke_width: 3.0,
            },
            ..Default::default()
        },
        Element {
            id: "text-1".to_owned(),
            name: String::new(),
            transform: Transform::default(),
            kind: ElementKind::Text {
                text: "Heading".to_owned(),
                font_family: "sans-serif".to_owned(),
                font_size: 24.0,
                color: target_red,
            },
            ..Default::default()
        },
    ];

    // 2. Nested Library symbol with a stroke and another keyframe
    project.symbols.push(Symbol {
        id: "sym-badge".to_owned(),
        name: "Badge".to_owned(),
        folder: "UI".to_owned(),
        kind: SymbolKind::MovieClip,
        timeline: crate::Timeline {
            frame_count: 4,
            layers: vec![Layer {
                id: "badge-layer".to_owned(),
                name: "Icon".to_owned(),
                visible: true,
                locked: false,
                keyframes: vec![
                    Keyframe {
                        frame: 0,
                        elements: vec![Element {
                            id: "badge-ellipse".to_owned(),
                            name: String::new(),
                            transform: Transform::default(),
                            kind: ElementKind::Ellipse {
                                width: 40.0,
                                height: 40.0,
                                fill: untouched_green,
                                stroke: target_red,
                                stroke_width: 4.0,
                            },
                            ..Default::default()
                        }],
                        ..Default::default()
                    },
                    Keyframe {
                        frame: 2,
                        elements: vec![Element {
                            id: "badge-rect-f2".to_owned(),
                            name: String::new(),
                            transform: Transform::default(),
                            kind: ElementKind::Rectangle {
                                width: 40.0,
                                height: 40.0,
                                fill: target_red,
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                            },
                            ..Default::default()
                        }],
                        ..Default::default()
                    },
                ],
                properties: Default::default(),
            }],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    });
    project.library_folders.push("UI".to_owned());

    let state = JsflAuthoringState::for_project(&project);
    let res = run_change_color_jsfl(&project, &state, target_red, replacement_blue).unwrap();

    // 6 occurrences replaced:
    // rect-1 fill (1), grad-1 stop 0 (1), grad-1 stroke (1), text-1 (1),
    // badge-ellipse stroke (1), badge-rect-f2 fill (1)
    assert_eq!(res.return_value, JsflValue::Number(6.0));
    assert_eq!(
        res.output_lines,
        vec!["Changed 6 colour occurrence(s) from #e63946 to #1d3557"]
    );

    let updated = &res.project;
    let root_els = &updated.symbols[0].timeline.layers[0].keyframes[0].elements;
    match &root_els[0].kind {
        ElementKind::Rectangle { fill, stroke, .. } => {
            assert_eq!(*fill, replacement_blue);
            assert_eq!(*stroke, untouched_green);
        }
        other => panic!("unexpected element kind: {other:?}"),
    }
    match &root_els[1].kind {
        ElementKind::PaintedPath {
            fill_gradient: Some(grad),
            stroke,
            ..
        } => {
            assert_eq!(grad.stops[0].color, replacement_blue);
            assert_eq!(grad.stops[1].color, untouched_green);
            assert_eq!(*stroke, replacement_blue);
        }
        other => panic!("unexpected element kind: {other:?}"),
    }
    match &root_els[2].kind {
        ElementKind::Text { color, .. } => assert_eq!(*color, replacement_blue),
        other => panic!("unexpected element kind: {other:?}"),
    }

    let badge = updated.symbol("sym-badge").unwrap();
    match &badge.timeline.layers[0].keyframes[0].elements[0].kind {
        ElementKind::Ellipse { fill, stroke, .. } => {
            assert_eq!(*fill, untouched_green);
            assert_eq!(*stroke, replacement_blue);
        }
        other => panic!("unexpected element kind: {other:?}"),
    }
    match &badge.timeline.layers[0].keyframes[1].elements[0].kind {
        ElementKind::Rectangle { fill, .. } => assert_eq!(*fill, replacement_blue),
        other => panic!("unexpected element kind: {other:?}"),
    }
}

#[test]
fn native_and_jsfl_doc_change_color_and_project_color_inventory() {
    let mut project = test_project();
    let c_primary = [244, 162, 97, 255];
    let c_accent = [231, 111, 81, 255];
    let c_new = [38, 70, 83, 255];

    project.set_custom_swatches(&[c_primary, c_accent]);
    project.symbols[0].timeline.layers[0].keyframes[0].elements = vec![
        Element {
            id: "shape-a".to_owned(),
            name: String::new(),
            transform: Transform::default(),
            kind: ElementKind::Rectangle {
                width: 100.0,
                height: 60.0,
                fill: c_primary,
                stroke: c_accent,
                stroke_width: 2.0,
            },
            ..Default::default()
        },
        Element {
            id: "text-a".to_owned(),
            name: String::new(),
            transform: Transform::default(),
            kind: ElementKind::Text {
                text: "Label".to_owned(),
                font_family: "sans-serif".to_owned(),
                font_size: 18.0,
                color: c_primary,
            },
            ..Default::default()
        },
    ];

    let inventory = collect_project_colors(&project);
    assert_eq!(inventory.len(), 2);
    assert_eq!(inventory[0].color, c_primary);
    assert_eq!(inventory[0].fill_count, 1);
    assert_eq!(inventory[0].text_count, 1);
    assert!(inventory[0].in_custom_swatches);
    assert_eq!(inventory[1].color, c_accent);
    assert_eq!(inventory[1].stroke_count, 1);

    // Execute via JSFL `doc.getProjectColors()` and `doc.changeColor(...)`
    let state = JsflAuthoringState::for_project(&project);
    let sb = JsflSandbox::new();
    let jsfl_res = sb
        .execute(
            &project,
            &state,
            "doc_change_color.jsfl",
            r##"
            var doc = fl.getDocumentDOM();
            var before = doc.getProjectColors();
            var rep = doc.changeColor("#f4a261", "#264653");
            var after = doc.getProjectColors();
            fl.trace("before=" + before.length, "replaced=" + rep.totalReplaced, "after=" + after[0].color);
            rep.totalReplaced;
        "##,
        )
        .unwrap();
    assert_eq!(
        jsfl_res.output_lines,
        vec!["before=2 replaced=3 after=#264653"]
    );
    assert_eq!(jsfl_res.return_value, JsflValue::Number(3.0));
    assert_eq!(jsfl_res.project.custom_swatches(), vec![c_new, c_accent]);
}

#[test]
fn jsfl_element_symbol_type_and_frame_label_type_do_not_mutate_library_symbol() {
    let mut project = Project::blank();
    let mut clip = Symbol {
        id: "hero-mc".into(),
        name: "Hero".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: project.symbols[0].timeline.clone(),
        class_name: None,
        linkage_id: None,
    };
    clip.timeline.frame_count = 4;
    clip.timeline.layers[0].id = "hero-layer".into();
    clip.timeline.layers[0].keyframes[0].label = "start".into();
    project.symbols.push(clip);
    project.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: "inst-1".into(),
            name: String::new(),
            transform: Transform::default(),
            kind: ElementKind::Instance {
                symbol_id: "hero-mc".into(),
                playback: InstancePlayback::Independent,
            },
            ..Default::default()
        });

    let state = JsflAuthoringState::for_project(&project);
    let sb = JsflSandbox::new();
    let res = sb
        .execute(
            &project,
            &state,
            "instance_behavior_and_label_type.jsfl",
            r##"
            var doc = fl.getDocumentDOM();
            var tl = doc.getTimeline();
            var frame = tl.layers[0].frames[0];
            var el = frame.elements[0];
            var libItem = doc.library.items[0];
            fl.trace("init=" + el.symbolType + "/" + libItem.itemType + "/" + frame.labelType);
            el.symbolType = "graphic";
            fl.trace("gfx=" + el.symbolType + "/" + libItem.itemType + "/" + el.loop);
            el.symbolType = "button";
            fl.trace("btn=" + el.symbolType + "/" + libItem.itemType);
            frame.name = "author_note";
            frame.labelType = "comment";
            fl.trace("lbl=" + frame.name + "/" + frame.labelType);
            frame.labelType = "anchor";
            fl.trace("anc=" + frame.name + "/" + frame.labelType);
            "##,
        )
        .unwrap();

    assert_eq!(
        res.output_lines,
        vec![
            "init=movie clip/movie clip/none",
            "gfx=graphic/movie clip/loop",
            "btn=button/movie clip",
            "lbl=author_note/comment",
            "anc=author_note/anchor",
        ]
    );
    assert_eq!(
        res.project.symbol("hero-mc").unwrap().kind,
        SymbolKind::MovieClip,
        "Library Symbol.kind must remain MovieClip"
    );
    let kf = &res.project.symbols[0].timeline.layers[0].keyframes[0];
    assert_eq!(kf.label, "author_note");
    assert_eq!(kf.label_type, crate::FrameLabelType::Anchor);
    assert_eq!(
        kf.elements[0].instance_behavior,
        Some(SymbolKind::Button),
        "Instance behavior override must be stored per-instance"
    );
    res.project.validate().unwrap();
}
