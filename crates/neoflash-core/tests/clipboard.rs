use neoflash_core::*;
use serde_json::{Value, json};
use std::collections::HashSet;

fn shape(id: &str, x: f64) -> Element {
    Element {
        id: id.into(),
        name: format!("Shape {id}"),
        transform: Transform {
            x,
            y: 7.,
            scale_x: -1.5,
            rotation: 23.,
            ..Default::default()
        },
        kind: ElementKind::Rectangle {
            width: 25.,
            height: 15.,
            fill: [12, 34, 56, 255],
            stroke: [0, 0, 0, 255],
            stroke_width: 2.,
        },
        scripts: vec![ObjectScript {
            event: ObjectEvent::Click,
            script: Script {
                language: ScriptLanguage::ActionScript3,
                source: "trace(\"copied\");".into(),
            },
        }],
        accessibility: Some(Accessibility {
            name: "Copied action".into(),
            description: "Original authored description".into(),
            role: AccessibleRole::Button,
            tab_order: 4,
            disabled: false,
        }),
        ..Element::default()
    }
}
fn instance(id: &str, symbol: &str) -> Element {
    Element {
        kind: ElementKind::Instance {
            symbol_id: symbol.into(),
            playback: InstancePlayback::Loop,
        },
        ..shape(id, 13.)
    }
}
fn key(frame: u32, elements: Vec<Element>, tween: Option<Easing>, label: &str) -> Keyframe {
    Keyframe {
        frame,
        elements,
        tween,
        label: label.into(),
        ..Default::default()
    }
}
fn symbol(id: &str, kind: SymbolKind, keys: Vec<Keyframe>) -> Symbol {
    Symbol {
        folder: String::new(),
        class_name: None,
        linkage_id: None,
        id: id.into(),
        name: format!("Library {id}"),
        kind,
        timeline: Timeline {
            guides: vec![],
            frame_count: 24,
            layers: vec![Layer {
                properties: Default::default(),
                id: format!("{id}-layer"),
                name: "Artwork".into(),
                visible: true,
                locked: false,
                keyframes: keys,
            }],
            scripts: vec![FrameScript {
                frame: 3,
                script: Script {
                    language: ScriptLanguage::TypeScript,
                    source: "let count: number = 1; trace(count);".into(),
                },
            }],
        },
    }
}
fn source() -> Project {
    let leaf = shape("leaf-art", 2.);
    let mut later = leaf.clone();
    later.transform.x = 22.;
    let child = instance("shared-placement", "leaf");
    let mut next_child = child.clone();
    next_child.transform.rotation = 95.;
    let mut project = Project::blank();
    project.root = "root".into();
    project.symbols = vec![
        symbol(
            "root",
            SymbolKind::MovieClip,
            vec![key(
                0,
                vec![instance("left", "parent"), instance("right", "parent")],
                None,
                "Scene",
            )],
        ),
        symbol(
            "leaf",
            SymbolKind::Graphic,
            vec![
                key(0, vec![leaf], Some(Easing::EaseIn), "First"),
                key(8, vec![later], None, "Last"),
            ],
        ),
        symbol(
            "parent",
            SymbolKind::MovieClip,
            vec![
                key(0, vec![child], Some(Easing::Linear), "Start"),
                key(12, vec![next_child], None, "Middle"),
            ],
        ),
        symbol(
            "unused",
            SymbolKind::Button,
            vec![key(0, vec![shape("unused-art", 0.)], None, "")],
        ),
    ];
    project.validate().unwrap();
    project
}
fn selected(project: &Project) -> Vec<Element> {
    project.symbol("root").unwrap().timeline.layers[0].keyframes[0]
        .elements
        .clone()
}
fn payload(project: &Project) -> ClipboardPayload {
    ClipboardPayload::capture(project, selected(project), "document-A", [42., -7.]).unwrap()
}
fn refs(element: &Element) -> &str {
    let ElementKind::Instance { symbol_id, .. } = &element.kind else {
        panic!("instance expected")
    };
    symbol_id
}
fn pasted(result: &PasteResult) -> Vec<&Element> {
    let layer = &result.project.symbol("root").unwrap().timeline.layers[0];
    result
        .element_ids
        .iter()
        .map(|id| {
            layer.keyframes[0]
                .elements
                .iter()
                .find(|e| &e.id == id)
                .unwrap()
        })
        .collect()
}
fn as_value(payload: &ClipboardPayload) -> Value {
    serde_json::from_str(
        payload
            .to_text()
            .unwrap()
            .strip_prefix(CLIPBOARD_PREFIX)
            .unwrap(),
    )
    .unwrap()
}
fn text(value: &Value) -> String {
    format!(
        "{CLIPBOARD_PREFIX}{}",
        serde_json::to_string(value).unwrap()
    )
}
fn ids(project: &Project) -> HashSet<String> {
    let mut ids = HashSet::new();
    for symbol in &project.symbols {
        ids.insert(symbol.id.clone());
        for layer in &symbol.timeline.layers {
            ids.insert(layer.id.clone());
            for key in &layer.keyframes {
                for e in &key.elements {
                    ids.insert(e.id.clone());
                }
            }
        }
    }
    ids
}

#[test]
fn text_roundtrip_has_only_reachable_library_in_original_order_and_preserves_data() {
    let mut project = source();
    project.add_library_folder("Characters/Empty").unwrap();
    project
        .move_library_symbol("leaf", "Characters/Artwork")
        .unwrap();
    project.move_library_symbol("parent", "Characters").unwrap();
    project
        .relocate_library_folder("Characters", "Cast")
        .unwrap();
    assert_eq!(project.symbol("leaf").unwrap().folder, "Cast/Artwork");
    assert!(
        project
            .library_folder_paths()
            .contains(&"Cast/Empty".into())
    );
    assert!(
        project
            .relocate_library_folder("Cast", "Cast/Inside")
            .is_err()
    );
    assert!(project.add_library_folder("../outside").is_err());
    let mut project = Project::from_json(&project.to_json().unwrap()).unwrap();
    assert_eq!(project.format_version, 4);
    project.format_version = 5;
    project.symbol_mut("parent").unwrap().timeline.layers[0].keyframes[0].tween =
        Some(Easing::Flash { acceleration: 40. });
    let before = project.clone();
    let mut selection = selected(&project);
    selection.reverse();
    let p =
        ClipboardPayload::capture(&project, selection.clone(), "document-A", [-10., 30.]).unwrap();
    let value = as_value(&p);
    assert_eq!(
        value["symbols"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["leaf", "parent"]
    );
    assert_eq!(value["elements"], serde_json::to_value(selection).unwrap());
    let decoded = ClipboardPayload::from_text(&p.to_text().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(decoded.anchor(), [-10., 30.]);
    assert_eq!(as_value(&decoded), value);
    assert_eq!(project, before);

    // Flat authoring groups and affine/color transforms must survive capture,
    // text roundtrip and insertion into an older blank document together.
    for version in [4, 5] {
        let mut source = Project::blank();
        source.format_version = version;
        let mut artwork = vec![
            shape("group-a", 0.),
            shape("group-b", 30.),
            shape("group-c", 60.),
            shape("group-d", 90.),
        ];
        if version == 5 {
            artwork[0].transform.skew = 17.;
            artwork[0].transform.color_transform.offsets[0] = 12.;
        }
        let root = source.symbol_mut(&source.root.clone()).unwrap();
        root.timeline.frame_count = 2;
        let mut first = root.timeline.layers[0].clone();
        first.keyframes = vec![
            key(0, artwork[..2].to_vec(), None, ""),
            key(1, vec![shape("later-group-member", 0.)], None, ""),
        ];
        first.properties.groups = vec![AuthoringGroup {
            id: "layer-local-group".into(),
            name: "First".into(),
            members: vec![
                "group-a".into(),
                "group-b".into(),
                "later-group-member".into(),
            ],
        }];
        let mut second = first.clone();
        second.id = "second-group-layer".into();
        second.keyframes = vec![key(0, artwork[2..].to_vec(), None, "")];
        second.properties.groups[0].name = "Second".into();
        second.properties.groups[0].members = vec!["group-c".into(), "group-d".into()];
        root.timeline.layers = vec![first, second];
        source.validate().unwrap();
        let capture =
            ClipboardPayload::capture(&source, artwork.clone(), "group-source", [0., 0.]).unwrap();
        let capture = ClipboardPayload::from_text(&capture.to_text().unwrap())
            .unwrap()
            .unwrap();
        let destination = Project::blank();
        let root_id = destination.root.clone();
        let layer_id = destination.symbol(&root_id).unwrap().timeline.layers[0]
            .id
            .clone();
        let pasted = capture
            .paste_into(
                &destination,
                "group-destination",
                &root_id,
                &layer_id,
                0,
                [0., 0.],
            )
            .unwrap();
        assert_eq!(pasted.project.format_version, version);
        pasted.project.validate().unwrap();
        let layer = &pasted.project.symbol(&root_id).unwrap().timeline.layers[0];
        assert_eq!(layer.properties.groups.len(), 2);
        assert_ne!(layer.properties.groups[0].id, layer.properties.groups[1].id);
        assert!(
            layer
                .properties
                .groups
                .iter()
                .all(|group| group.members.len() == 2)
        );
        for (original, copied) in artwork.iter().zip(&layer.keyframes[0].elements) {
            assert_eq!(original.transform, copied.transform);
            assert_eq!(original.kind, copied.kind);
            assert_eq!(original.scripts, copied.scripts);
            assert_ne!(original.id, copied.id);
        }
    }
}

#[test]
fn cross_session_remaps_shared_closure_once_and_repeated_frame_placements_consistently() {
    let mut source = source();
    source
        .move_library_symbol("leaf", "Sprites/Details")
        .unwrap();
    source.move_library_symbol("parent", "Sprites").unwrap();
    let p = payload(&source);
    let mut destination = source.clone();
    destination.library_folders.clear();
    for symbol in &mut destination.symbols {
        symbol.folder.clear();
    }
    destination.fps = 60;
    destination.name = "Different document".into();
    let original = destination.clone();
    let old_ids = ids(&destination);
    let result = p
        .paste_into(
            &destination,
            "document-B",
            "root",
            "root-layer",
            0,
            [100., -20.],
        )
        .unwrap();
    assert_eq!(destination, original);
    assert_eq!(result.project.symbols.len(), destination.symbols.len() + 2);
    assert_eq!(
        &result.project.symbols[1..destination.symbols.len()],
        &destination.symbols[1..]
    );
    assert_eq!(
        &result.project.symbols[0].timeline.layers[0].keyframes[0].elements[..2],
        &destination.symbols[0].timeline.layers[0].keyframes[0].elements
    );
    let placements = pasted(&result);
    assert_eq!(
        placements
            .iter()
            .map(|e| e.name.as_str())
            .collect::<Vec<_>>(),
        selected(&source)
            .iter()
            .map(|e| e.name.as_str())
            .collect::<Vec<_>>()
    );
    assert_eq!(refs(placements[0]), refs(placements[1]));
    let imported_parent = result.project.symbol(refs(placements[0])).unwrap();
    let keys = &imported_parent.timeline.layers[0].keyframes;
    assert_eq!(keys[0].elements[0].id, keys[1].elements[0].id);
    assert_ne!(keys[0].elements[0].id, "shared-placement");
    assert_eq!(refs(&keys[0].elements[0]), refs(&keys[1].elements[0]));
    let imported_leaf = result.project.symbol(refs(&keys[0].elements[0])).unwrap();
    assert_eq!(imported_leaf.folder, "Sprites/Details");
    assert_eq!(imported_parent.folder, "Sprites");
    assert_eq!(
        result.project.library_folder_paths(),
        vec!["Sprites", "Sprites/Details"]
    );
    assert_eq!(imported_leaf.name, "Library leaf");
    assert_eq!(imported_leaf.kind, SymbolKind::Graphic);
    assert_eq!(imported_parent.name, "Library parent");
    assert_eq!(
        keys[1].frame, 12,
        "authored frame indices are not rescaled to destination FPS"
    );
    assert_eq!(result.project.fps, 60);
    assert_eq!(
        imported_parent.timeline.scripts,
        source.symbol("parent").unwrap().timeline.scripts
    );
    for (i, e) in placements.iter().enumerate() {
        let original = &selected(&source)[i];
        assert_eq!(e.transform.x, original.transform.x + 100.);
        assert_eq!(e.transform.y, original.transform.y - 20.);
        assert_eq!(e.transform.scale_x, original.transform.scale_x);
        assert_eq!(e.scripts, original.scripts);
        assert_eq!(e.accessibility, original.accessibility);
        assert!(!old_ids.contains(&e.id));
    }
    for symbol in &result.project.symbols[destination.symbols.len()..] {
        assert!(!old_ids.contains(&symbol.id));
        for layer in &symbol.timeline.layers {
            assert!(!old_ids.contains(&layer.id));
            for key in &layer.keyframes {
                for e in &key.elements {
                    assert!(!old_ids.contains(&e.id));
                }
            }
        }
    }
    assert_eq!(
        result.project.symbols[destination.symbols.len()].id,
        imported_leaf.id
    );
    result.project.validate().unwrap();
    let second = p
        .paste_into(
            &result.project,
            "document-B",
            "root",
            "root-layer",
            0,
            [0., 0.],
        )
        .unwrap();
    assert!(
        second
            .element_ids
            .iter()
            .all(|id| !result.element_ids.contains(id))
    );
    assert_eq!(
        second.project.symbols.len(),
        result.project.symbols.len() + 2
    );
}

#[test]
fn format_10_object_motion_tracks_survive_cross_session_symbol_clipboard() {
    let mut start = key(
        0,
        vec![shape("track-a", 0.0), shape("track-b", 0.0)],
        Some(Easing::Linear),
        "Motion start",
    );
    start.tween_kind = TweenKind::Motion;
    let mut a_track = MotionTrackOverride::new("track-a");
    a_track.easing = Some(Easing::EaseIn);
    a_track.sync_symbols = Some(true);
    let mut b_track = MotionTrackOverride::new("track-b");
    b_track.easing = Some(Easing::EaseOut);
    b_track.sync_symbols = Some(false);
    start.motion_tracks = vec![a_track, b_track];

    let mut end = key(
        10,
        vec![shape("track-a", 100.0), shape("track-b", 200.0)],
        None,
        "Object keys",
    );
    end.keyed_properties = Some(vec![TweenProperty::X]);
    end.motion_keys = Some(vec![
        MotionPropertyKey {
            element_id: "track-a".into(),
            properties: vec![TweenProperty::X],
            roving: false,
        },
        MotionPropertyKey {
            element_id: "track-b".into(),
            properties: vec![TweenProperty::X],
            roving: false,
        },
    ]);
    let motion_symbol = symbol("object-motion", SymbolKind::MovieClip, vec![start, end]);

    let mut source = Project::blank();
    source.symbols.push(motion_symbol);
    let placement = instance("motion-placement", "object-motion");
    source.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .push(placement.clone());
    source.upgrade_format_for_features();
    assert_eq!(source.format_version, 10);
    source.validate().unwrap();
    let source_pose: Vec<_> = sample_layer(
        &source.symbol("object-motion").unwrap().timeline.layers[0],
        5,
    )
    .into_iter()
    .map(|element| (element.name, element.transform.x))
    .collect();

    let payload =
        ClipboardPayload::capture(&source, vec![placement], "format-10-source", [0.0, 0.0])
            .unwrap();
    let payload = ClipboardPayload::from_text(&payload.to_text().unwrap())
        .unwrap()
        .unwrap();
    let destination = Project::blank();
    let root = destination.root.clone();
    let layer_id = destination.symbols[0].timeline.layers[0].id.clone();
    let result = payload
        .paste_into(
            &destination,
            "format-10-destination",
            &root,
            &layer_id,
            0,
            [0.0, 0.0],
        )
        .unwrap();
    assert_eq!(result.project.format_version, 10);
    result.project.validate().unwrap();

    let imported = result
        .project
        .symbols
        .iter()
        .find(|candidate| candidate.name == "Library object-motion")
        .unwrap();
    let keys = &imported.timeline.layers[0].keyframes;
    let start_ids: HashSet<_> = keys[0]
        .elements
        .iter()
        .map(|element| element.id.as_str())
        .collect();
    assert!(
        keys[0]
            .motion_tracks
            .iter()
            .all(|track| start_ids.contains(track.element_id.as_str()))
    );
    assert!(
        keys[1]
            .motion_keys
            .as_ref()
            .unwrap()
            .iter()
            .all(|object_key| start_ids.contains(object_key.element_id.as_str()))
    );
    assert!(!start_ids.contains("track-a"));
    assert!(!start_ids.contains("track-b"));
    let imported_pose: Vec<_> = sample_layer(&imported.timeline.layers[0], 5)
        .into_iter()
        .map(|element| (element.name, element.transform.x))
        .collect();
    assert_eq!(imported_pose, source_pose);
}

#[test]
fn same_session_uses_current_library_graph_and_does_not_resurrect_old_dependencies() {
    let original = source();
    let p = payload(&original);
    let mut live = original.clone();
    let mut replacement = live.symbol("leaf").unwrap().clone();
    replacement.id = "replacement".into();
    replacement.name = "Edited live leaf".into();
    replacement.timeline.layers[0].id = "replacement-layer".into();
    for key in &mut replacement.timeline.layers[0].keyframes {
        key.elements[0].id = "replacement-art".into();
        key.elements[0].transform.x = 999.;
    }
    live.symbols.retain(|s| s.id != "leaf");
    live.symbols.push(replacement);
    for key in &mut live.symbol_mut("parent").unwrap().timeline.layers[0].keyframes {
        if let ElementKind::Instance { symbol_id, .. } = &mut key.elements[0].kind {
            *symbol_id = "replacement".into();
        }
    }
    live.validate().unwrap();
    let result = p
        .paste_into(&live, "document-A", "root", "root-layer", 0, [0., 0.])
        .unwrap();
    assert_eq!(result.project.symbols.len(), live.symbols.len());
    assert_eq!(refs(pasted(&result)[0]), "parent");
    assert_eq!(result.project.symbol("parent"), live.symbol("parent"));
    assert_eq!(
        result.project.symbol("replacement"),
        live.symbol("replacement")
    );
    assert!(result.project.symbol("leaf").is_none());
}

#[test]
fn missing_same_session_definition_recovers_identity_and_reuses_live_children() {
    let original = source();
    let p = payload(&original);
    let mut destination = original.clone();
    destination.symbol_mut("root").unwrap().timeline.layers[0].keyframes[0]
        .elements
        .clear();
    destination.symbols.retain(|s| s.id != "parent");
    destination.symbol_mut("leaf").unwrap().name = "Live child edited after copy".into();
    let result = p
        .paste_into(
            &destination,
            "document-A",
            "root",
            "root-layer",
            0,
            [0., 0.],
        )
        .unwrap();
    assert_eq!(refs(pasted(&result)[0]), "parent");
    assert_eq!(result.project.symbol("parent"), original.symbol("parent"));
    assert_eq!(result.project.symbol("leaf"), destination.symbol("leaf"));
    let again = p
        .paste_into(
            &result.project,
            "document-A",
            "root",
            "root-layer",
            0,
            [0., 0.],
        )
        .unwrap();
    assert_eq!(again.project.symbols.len(), result.project.symbols.len());
    assert_ne!(again.element_ids, result.element_ids);
}

#[test]
fn recovery_remaps_ids_that_have_been_reused_by_another_entity_type() {
    let original = source();
    let p = payload(&original);
    let mut destination = original.clone();
    destination.symbols.retain(|s| s.id != "parent");
    let layer = &mut destination.symbol_mut("root").unwrap().timeline.layers[0];
    layer.id = "parent".into();
    layer.keyframes[0].elements = vec![shape("parent-layer", 0.)];
    destination.validate().unwrap();
    let result = p
        .paste_into(&destination, "document-A", "root", "parent", 0, [0., 0.])
        .unwrap();
    let parent = result.project.symbol(refs(pasted(&result)[0])).unwrap();
    assert_ne!(parent.id, "parent");
    assert_ne!(parent.timeline.layers[0].id, "parent-layer");
    assert_eq!(
        parent.timeline.layers[0].keyframes[0].elements[0].id,
        parent.timeline.layers[0].keyframes[1].elements[0].id
    );
    result.project.validate().unwrap();
}

#[test]
fn candidate_only_insertion_materializes_sampled_pose_without_copying_labels_or_frame_scripts() {
    let mut source = Project::blank();
    let copied = shape("copied", -30.);
    source.symbols[0].timeline.layers[0].keyframes[0].elements = vec![copied.clone()];
    let p = ClipboardPayload::capture(&source, vec![copied.clone()], "A", [0., 0.]).unwrap();
    let mut destination = Project::blank();
    let id = destination.root.clone();
    let layer_id = destination.symbols[0].timeline.layers[0].id.clone();
    let start = shape("existing", 10.);
    let mut end = start.clone();
    end.transform.x = 110.;
    destination.symbols[0].timeline.layers[0].keyframes = vec![
        key(0, vec![start], Some(Easing::EaseIn), "Start label"),
        key(10, vec![end], None, "End label"),
    ];
    destination.symbols[0].timeline.scripts = vec![FrameScript {
        frame: 5,
        script: Script {
            language: ScriptLanguage::JavaScript,
            source: "this.stop();".into(),
        },
    }];
    let before = destination.clone();
    let result = p
        .paste_into(&destination, "B", &id, &layer_id, 5, [40., -2.])
        .unwrap();
    assert_eq!(destination, before);
    let timeline = &result.project.symbols[0].timeline;
    let keys = &timeline.layers[0].keyframes;
    assert_eq!(keys.len(), 3);
    assert_eq!(keys[0], before.symbols[0].timeline.layers[0].keyframes[0]);
    assert_eq!(keys[2], before.symbols[0].timeline.layers[0].keyframes[1]);
    assert_eq!(keys[1].elements[0].transform.x, 35.);
    assert_eq!(keys[1].elements[0].scripts, keys[0].elements[0].scripts);
    assert_eq!(keys[1].tween, Some(Easing::EaseIn));
    assert!(keys[1].label.is_empty());
    assert_eq!(timeline.scripts, before.symbols[0].timeline.scripts);
    assert_eq!(keys[1].elements[1].transform.x, 10.);
    assert_eq!(keys[1].elements[1].transform.y, 5.);
    assert_eq!(keys[1].elements[1].scripts, copied.scripts);
    assert_eq!(keys[1].elements[1].accessibility, copied.accessibility);
    let existing = p
        .paste_into(&destination, "B", &id, &layer_id, 0, [0., 0.])
        .unwrap();
    assert_eq!(
        existing.project.symbols[0].timeline.layers[0].keyframes[0].label,
        "Start label"
    );
    assert_eq!(
        existing.project.symbols[0].timeline.layers[0].keyframes[0].tween,
        Some(Easing::EaseIn)
    );
}

#[test]
fn insertion_before_first_authored_key_has_empty_sample_and_no_inherited_label() {
    let source = source();
    let p = payload(&source);
    let mut destination = Project::blank();
    let id = destination.root.clone();
    let layer_id = destination.symbols[0].timeline.layers[0].id.clone();
    destination.symbols[0].timeline.layers[0].keyframes = vec![key(
        10,
        vec![shape("future", 50.)],
        Some(Easing::EaseOut),
        "Future",
    )];
    let result = p
        .paste_into(&destination, "other", &id, &layer_id, 5, [0., 0.])
        .unwrap();
    let keys = &result.project.symbols[0].timeline.layers[0].keyframes;
    assert_eq!(keys[0].elements.len(), 2);
    assert!(keys[0].label.is_empty());
    assert_eq!(keys[0].tween, None);
    assert_eq!(
        keys[1],
        destination.symbols[0].timeline.layers[0].keyframes[0]
    );
}

#[test]
fn self_cycles_ancestor_cycles_invalid_targets_and_overflow_reject_atomically() {
    let destination = source();
    let before = destination.clone();
    let p = payload(&destination);
    for (symbol, layer, frame, offset) in [
        ("parent", "parent-layer", 0, [0., 0.]),
        ("leaf", "leaf-layer", 0, [0., 0.]),
        ("root", "root-layer", 24, [0., 0.]),
        ("missing", "root-layer", 0, [0., 0.]),
        ("root", "leaf-layer", 0, [0., 0.]),
        ("root", "root-layer", 0, [f64::NAN, 0.]),
        ("root", "root-layer", 0, [1e12, 0.]),
        ("root", "root-layer", 0, [f64::MAX, 0.]),
    ] {
        assert!(
            p.paste_into(&destination, "document-A", symbol, layer, frame, offset)
                .is_err()
        );
        assert_eq!(destination, before);
    }
    for locked in [true, false] {
        let mut destination = before.clone();
        destination.symbols[0].timeline.layers[0].locked = locked;
        destination.symbols[0].timeline.layers[0].visible = locked;
        let before = destination.clone();
        assert!(
            p.paste_into(
                &destination,
                "document-A",
                "root",
                "root-layer",
                0,
                [0., 0.]
            )
            .is_err()
        );
        assert_eq!(destination, before);
    }
    let cross = p.paste_into(
        &destination,
        "other-session",
        "parent",
        "parent-layer",
        0,
        [0., 0.],
    );
    assert!(
        cross.is_ok(),
        "fresh cross-session identities do not introduce a self-cycle"
    );
    let mut broken = before;
    broken.symbols.retain(|s| s.id != "leaf");
    assert!(
        p.paste_into(&broken, "document-A", "root", "root-layer", 0, [0., 0.])
            .is_err()
    );
}

#[test]
fn parser_ignores_foreign_text_and_rejects_versions_unknown_fields_and_bad_closures() {
    assert!(
        ClipboardPayload::from_text("ordinary text or /path/to/file")
            .unwrap()
            .is_none()
    );
    assert!(
        ClipboardPayload::from_text(&"x".repeat(MAX_CLIPBOARD_BYTES + 1))
            .unwrap()
            .is_none()
    );
    assert!(ClipboardPayload::from_text(CLIPBOARD_PREFIX).is_err());
    let project = source();
    let p = payload(&project);
    let valid = as_value(&p);
    let mut value = valid.clone();
    value["format_version"] = json!(99);
    assert!(matches!(
        ClipboardPayload::from_text(&text(&value)),
        Err(ClipboardError::UnsupportedVersion(99))
    ));
    let mut value = valid.clone();
    value["future_feature"] = json!(true);
    assert!(ClipboardPayload::from_text(&text(&value)).is_err());
    let mut value = valid.clone();
    value["elements"][0]["kind"]["future_feature"] = json!(true);
    assert!(ClipboardPayload::from_text(&text(&value)).is_err());
    let mut value = valid.clone();
    value["symbols"][0]["timeline"]["scripts"][0]["script"]["run_now"] = json!(true);
    assert!(ClipboardPayload::from_text(&text(&value)).is_err());
    let mut value = valid.clone();
    value["symbols"].as_array_mut().unwrap().remove(0);
    assert!(ClipboardPayload::from_text(&text(&value)).is_err());
    let mut value = valid.clone();
    value["symbols"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::to_value(project.symbol("unused").unwrap()).unwrap());
    assert!(ClipboardPayload::from_text(&text(&value)).is_err());
    let mut value = valid.clone();
    value["symbols"][0]["timeline"]["layers"][0]["keyframes"][0]["elements"][0]["kind"] =
        json!({"type":"instance","symbol_id":"parent","playback":"loop"});
    assert!(ClipboardPayload::from_text(&text(&value)).is_err());
    let mut value = valid.clone();
    value["anchor"] = json!([1]);
    assert!(ClipboardPayload::from_text(&text(&value)).is_err());
    let mut value = valid.clone();
    value["source_session"] = json!("");
    assert!(ClipboardPayload::from_text(&text(&value)).is_err());
    let bad_number = text(&valid).replace("\"anchor\":[42.0,-7.0]", "\"anchor\":[1e999,0]");
    assert!(ClipboardPayload::from_text(&bad_number).is_err());
    assert!(
        ClipboardPayload::capture(&project, selected(&project), "session", [f64::INFINITY, 0.])
            .is_err()
    );
    assert!(ClipboardPayload::capture(&project, vec![], "session", [0., 0.]).is_err());
}

#[test]
fn nullable_metadata_and_omitted_defaults_are_accepted_but_duplicate_fields_are_not() {
    let project = source();
    let p = payload(&project);
    let mut value = as_value(&p);
    value["elements"][0]["accessibility"] = Value::Null;
    value["elements"][1]
        .as_object_mut()
        .unwrap()
        .remove("scripts");
    value["symbols"][0]["timeline"]
        .as_object_mut()
        .unwrap()
        .remove("scripts");
    assert!(
        ClipboardPayload::from_text(&text(&value))
            .unwrap()
            .is_some()
    );
    let duplicate = p.to_text().unwrap().replacen(
        "\"format_version\":1",
        "\"format_version\":1,\"format_version\":1",
        1,
    );
    assert!(ClipboardPayload::from_text(&duplicate).is_err());
}

#[test]
fn text_and_object_budgets_reject_before_pasting() {
    let mut portable = Project::blank();
    portable.format_version = 3;
    portable.resources.push(
        ResourceUnit::from_bytes(
            "large-resource",
            "data/large.bin",
            "application/octet-stream",
            &vec![0; MAX_CLIPBOARD_BYTES],
        )
        .unwrap(),
    );
    assert!(matches!(
        ClipboardPayload::capture(&portable, vec![shape("portable", 0.)], "session", [0., 0.]),
        Err(ClipboardError::TooLarge)
    ));
    let oversized = format!("{CLIPBOARD_PREFIX}{}", " ".repeat(MAX_CLIPBOARD_BYTES));
    assert!(matches!(
        ClipboardPayload::from_text(&oversized),
        Err(ClipboardError::TooLarge)
    ));
    let mut project = Project::blank();
    let mut element = shape("huge-source", 0.);
    element.scripts[0].script.source = "x".repeat(MAX_CLIPBOARD_BYTES);
    project.symbols[0].timeline.layers[0].keyframes[0].elements = vec![element.clone()];
    assert!(matches!(
        ClipboardPayload::capture(&project, vec![element], "session", [0., 0.]),
        Err(ClipboardError::TooLarge)
    ));
    let element = shape("many", 0.);
    assert!(
        ClipboardPayload::capture(&Project::blank(), vec![element; 10001], "session", [0., 0.])
            .is_err()
    );
    let mut project = source();
    project.symbol_mut("leaf").unwrap().timeline.frame_count = 20_000;
    project.symbol_mut("leaf").unwrap().timeline.layers[0].keyframes = (0..16_385)
        .map(|frame| key(frame, vec![], None, ""))
        .collect();
    assert!(
        ClipboardPayload::capture(&project, selected(&project), "session", [0., 0.])
            .unwrap_err()
            .to_string()
            .contains("budget")
    );
}

#[test]
fn pasted_nested_movieclips_run_independent_clocks_dispatch_and_overrides_after_reopen() {
    fn script(source: &str) -> Script {
        Script {
            language: ScriptLanguage::ActionScript3,
            source: source.into(),
        }
    }
    fn object_scripts(load: &str, click: &str) -> Vec<ObjectScript> {
        vec![
            ObjectScript {
                event: ObjectEvent::Load,
                script: script(load),
            },
            ObjectScript {
                event: ObjectEvent::Click,
                script: script(click),
            },
        ]
    }
    let mut source = source();
    for symbol in &mut source.symbols {
        if symbol.id == "leaf" || symbol.id == "parent" {
            symbol.kind = SymbolKind::MovieClip;
            symbol.timeline.scripts = vec![
                FrameScript {
                    frame: 0,
                    script: script(&format!("{}-frame0", symbol.id)),
                },
                FrameScript {
                    frame: 1,
                    script: script(&format!("{}-frame1", symbol.id)),
                },
            ];
        }
        for element in symbol
            .timeline
            .layers
            .iter_mut()
            .flat_map(|layer| &mut layer.keyframes)
            .flat_map(|key| &mut key.elements)
        {
            if let ElementKind::Instance { playback, .. } = &mut element.kind {
                *playback = InstancePlayback::Independent;
            }
            element.scripts = match element.id.as_str() {
                "left" | "right" => object_scripts("outer-load", "outer-click"),
                "shared-placement" => object_scripts("inner-load", "inner-click"),
                "leaf-art" => vec![
                    ObjectScript {
                        event: ObjectEvent::Click,
                        script: script("leaf-click"),
                    },
                    ObjectScript {
                        event: ObjectEvent::EnterFrame,
                        script: script("leaf-enter"),
                    },
                ],
                _ => vec![],
            };
        }
    }
    let payload = payload(&source);
    let destination = Project::blank();
    let layer = destination.symbols[0].timeline.layers[0].id.clone();
    let pasted = payload
        .paste_into(
            &destination,
            "recipient-session",
            &destination.root,
            &layer,
            0,
            [0., 0.],
        )
        .unwrap();
    let project = pasted.project;
    assert_eq!(
        project.symbols.len(),
        3,
        "both placements share one imported parent and one imported leaf"
    );
    let top = &project.symbols[0].timeline.layers[0].keyframes[0].elements;
    assert_eq!(refs(&top[0]), refs(&top[1]));
    let parent = project.symbol(refs(&top[0])).unwrap();
    let child = &parent.timeline.layers[0].keyframes[0].elements[0];
    let leaf = project.symbol(refs(child)).unwrap();
    let artwork = &leaf.timeline.layers[0].keyframes[0].elements[0];
    let a = vec![pasted.element_ids[0].clone()];
    let b = vec![pasted.element_ids[1].clone()];
    let inner_a = vec![a[0].clone(), child.id.clone()];
    let inner_b = vec![b[0].clone(), child.id.clone()];
    let art_a = vec![a[0].clone(), child.id.clone(), artwork.id.clone()];
    let art_b = vec![b[0].clone(), child.id.clone(), artwork.id.clone()];
    let summarize = |dispatches: Vec<ScriptDispatch>| {
        dispatches
            .into_iter()
            .map(|event| (event.target_path, event.event, event.script))
            .collect::<Vec<_>>()
    };
    let exercise = |project: &Project| {
        let before = project.clone();
        let mut player = Player::new(project);
        assert_ne!(player.lifetime_id(&a), player.lifetime_id(&b));
        assert_ne!(player.lifetime_id(&inner_a), player.lifetime_id(&inner_b));
        let first_lifetime = player.lifetime_id(&a).unwrap();
        let initial_events = player.drain_scripts();
        for event in &initial_events {
            assert_eq!(
                Some(event.target_lifetime),
                player.lifetime_id(&event.target_path)
            );
            assert!(player.is_dispatch_current(project, event));
        }
        let initial = summarize(initial_events);
        for (path, event, source) in [
            (&a, ScriptEvent::Load, "outer-load"),
            (&b, ScriptEvent::Load, "outer-load"),
            (&inner_a, ScriptEvent::Load, "inner-load"),
            (&inner_b, ScriptEvent::Load, "inner-load"),
            (&a, ScriptEvent::Frame(0), "parent-frame0"),
            (&b, ScriptEvent::Frame(0), "parent-frame0"),
            (&inner_a, ScriptEvent::Frame(0), "leaf-frame0"),
            (&inner_b, ScriptEvent::Frame(0), "leaf-frame0"),
        ] {
            assert!(initial.contains(&(path.clone(), event, script(source))));
        }
        assert_eq!(initial.len(), 8);
        assert!(player.control(project, &[], PlaybackCommand::Stop));
        assert!(player.control(project, &a, PlaybackCommand::Stop));
        assert!(player.control(project, &inner_a, PlaybackCommand::GotoAndStop(4)));
        player.drain_scripts();
        player.tick(project);
        let tick_one = summarize(player.drain_scripts());
        assert!(tick_one.contains(&(b.clone(), ScriptEvent::Frame(1), script("parent-frame1"))));
        assert!(tick_one.contains(&(
            inner_b.clone(),
            ScriptEvent::Frame(1),
            script("leaf-frame1")
        )));
        assert!(
            !tick_one
                .iter()
                .any(|(path, event, _)| (path == &a || path == &inner_a)
                    && *event == ScriptEvent::Frame(1))
        );
        player.tick(project);
        player.drain_scripts();
        assert_eq!(player.frame(), 0);
        assert_eq!(player.clip_frame(&a), Some(0));
        assert_eq!(player.clip_frame(&b), Some(2));
        assert_eq!(player.clip_frame(&inner_a), Some(4));
        assert_eq!(player.clip_frame(&inner_b), Some(2));
        let sibling_before = player
            .evaluate(project)
            .into_iter()
            .find(|item| item.instance_path == art_b)
            .unwrap();
        assert!(player.set_property(&art_a, "x", 123.));
        assert_eq!(player.property(project, &art_a, "x"), Some(123.));
        assert_eq!(player.property(project, &art_b, "x"), Some(3.25));
        assert_eq!(
            player
                .evaluate(project)
                .into_iter()
                .find(|item| item.instance_path == art_b)
                .unwrap(),
            sibling_before
        );
        assert_ne!(
            player
                .evaluate(project)
                .into_iter()
                .find(|item| item.instance_path == art_a)
                .unwrap()
                .transform,
            sibling_before.transform
        );
        assert_eq!(
            project, &before,
            "runtime controls and overrides cannot edit the pasted document"
        );
        player.dispatch_click(project, &art_a);
        let clicks = summarize(player.drain_scripts());
        assert_eq!(
            clicks,
            vec![
                (art_a.clone(), ScriptEvent::Click, script("leaf-click")),
                (inner_a.clone(), ScriptEvent::Click, script("inner-click")),
                (a.clone(), ScriptEvent::Click, script("outer-click"))
            ]
        );
        player.reset(project);
        assert_ne!(player.lifetime_id(&a), Some(first_lifetime));
        assert_eq!(player.clip_frame(&inner_a), Some(0));
        assert_eq!(player.property(project, &art_a, "x"), Some(2.));
        assert_eq!(summarize(player.drain_scripts()), initial);
        (initial, tick_one, clicks, player.evaluate(project))
    };
    let first = exercise(&project);
    let reopened = Project::from_json(&project.to_json().unwrap()).unwrap();
    assert_eq!(exercise(&reopened), first);
}

#[test]
fn format_7_display_features_and_cross_session_motion_guide_and_sound_paste_round_trip() {
    // 1. Format-7 display features on top-level elements (filters, blend_mode, cache_as_bitmap, visible: false)
    let mut filtered_el = shape("filtered-shape", 10.0);
    filtered_el.visible = false;
    filtered_el.cache_as_bitmap = true;
    filtered_el.blend_mode = BlendMode::Multiply;
    filtered_el.filters = vec![FilterSpec::Blur {
        blur_x: 4.0,
        blur_y: 4.0,
        quality: 1,
    }];
    let mut src_doc = Project::blank();
    src_doc.format_version = 7;
    src_doc.symbols[0].timeline.layers[0].keyframes[0].elements = vec![filtered_el.clone()];
    src_doc.validate().unwrap();

    let captured_display =
        ClipboardPayload::capture(&src_doc, vec![filtered_el], "session-fmt7", [0.0, 0.0])
            .expect("capture format-7 display element");
    let dst_blank = Project::blank();
    let dst_layer = dst_blank.symbols[0].timeline.layers[0].id.clone();
    let pasted_display = captured_display
        .paste_into(
            &dst_blank,
            "other-session",
            &dst_blank.root,
            &dst_layer,
            0,
            [0.0, 0.0],
        )
        .expect("paste format-7 display element");
    assert_eq!(pasted_display.project.format_version, 7);
    let pasted_el = &pasted_display.project.symbols[0].timeline.layers[0].keyframes[0].elements[0];
    assert!(!pasted_el.visible);
    assert!(pasted_el.cache_as_bitmap);
    assert_eq!(pasted_el.blend_mode, BlendMode::Multiply);
    assert_eq!(pasted_el.filters.len(), 1);

    // 2. Cross-session paste of a symbol with motion_guide and FrameSound (with in_point/out_point)
    let mut guided_doc = Project::blank();
    guided_doc.format_version = 7;
    let wav_bytes = [
        b'R', b'I', b'F', b'F', 36, 0, 0, 0, b'W', b'A', b'V', b'E', b'f', b'm', b't', b' ', 16, 0,
        0, 0, 1, 0, 1, 0, 0x44, 0xAC, 0, 0, 0x88, 0x58, 1, 0, 2, 0, 16, 0, b'd', b'a', b't', b'a',
        0, 0, 0, 0,
    ];
    guided_doc.resources.push(
        ResourceUnit::from_bytes("snd-res-1", "audio/chime.wav", "audio/wav", &wav_bytes).unwrap(),
    );
    let guide_layer = Layer {
        id: "guide-layer-1".into(),
        name: "Guide".into(),
        visible: true,
        locked: false,
        properties: LayerProperties {
            role: LayerRole::Guide,
            ..Default::default()
        },
        keyframes: vec![Keyframe {
            frame: 0,
            elements: vec![shape("guide-path", 0.0)],
            ..Default::default()
        }],
    };
    let mut sound = FrameSound::new("snd-res-1");
    sound.in_point = Some(0.1);
    sound.out_point = Some(0.5);
    let normal_layer = Layer {
        id: "guided-layer-1".into(),
        name: "Guided".into(),
        visible: true,
        locked: false,
        properties: LayerProperties {
            motion_guide: Some("guide-layer-1".into()),
            ..Default::default()
        },
        keyframes: vec![Keyframe {
            frame: 0,
            elements: vec![shape("guided-art", 0.0)],
            sound: Some(sound),
            ..Default::default()
        }],
    };
    guided_doc.symbols.push(Symbol {
        id: "sym-guided".into(),
        name: "GuidedClip".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        class_name: None,
        linkage_id: Some("GuidedClipExport".into()),
        timeline: Timeline {
            frame_count: 5,
            layers: vec![normal_layer, guide_layer],
            scripts: vec![],
            guides: vec![],
        },
    });
    let inst = instance("guided-inst", "sym-guided");
    guided_doc.symbols[0].timeline.layers[0].keyframes[0].elements = vec![inst.clone()];
    guided_doc.validate().unwrap();

    let captured_guided =
        ClipboardPayload::capture(&guided_doc, vec![inst], "session-guided", [0.0, 0.0])
            .expect("capture symbol with motion_guide and FrameSound trim points");
    let pasted_guided = captured_guided
        .paste_into(
            &dst_blank,
            "session-destination",
            &dst_blank.root,
            &dst_layer,
            0,
            [0.0, 0.0],
        )
        .expect("cross-session paste of symbol with motion_guide and FrameSound");
    pasted_guided.project.validate().unwrap();
    assert_eq!(pasted_guided.project.format_version, 7);
    let imported_sym = pasted_guided
        .project
        .symbols
        .iter()
        .find(|s| s.name == "GuidedClip")
        .unwrap();
    let imported_guide_id = imported_sym.timeline.layers[1].id.clone();
    assert_ne!(imported_guide_id, "guide-layer-1");
    assert_eq!(
        imported_sym.timeline.layers[0]
            .properties
            .motion_guide
            .as_deref(),
        Some(imported_guide_id.as_str())
    );
    let imported_res_id = pasted_guided.project.resources[0].id.clone();
    assert_ne!(imported_res_id, "snd-res-1");
    assert_eq!(
        imported_sym.timeline.layers[0].keyframes[0]
            .sound
            .as_ref()
            .unwrap()
            .resource,
        imported_res_id
    );
}

#[test]
fn paste_into_intermediate_frame_preserves_motion_shape_and_guided_tween_invariants() {
    let mut src = Project::blank();
    let badge = shape("badge", 15.0);
    src.symbols[0].timeline.layers[0].keyframes[0].elements = vec![badge.clone()];
    let clip = ClipboardPayload::capture(&src, vec![badge], "clip-src", [0.0, 0.0]).unwrap();

    // 1. Intermediate paste inside a Motion tween span with ◆ property keyframes must not leave
    // orphaned ◆ keys or invalid tween_kind on the newly split keyframe.
    let mut motion_doc = Project::blank();
    motion_doc.format_version = 7;
    let root_id = motion_doc.root.clone();
    let layer_id = motion_doc.symbol(&root_id).unwrap().timeline.layers[0]
        .id
        .clone();
    {
        let timeline = &mut motion_doc.symbol_mut(&root_id).unwrap().timeline;
        timeline.frame_count = 11;
        let mut k0 = key(0, vec![shape("mover", 0.0)], Some(Easing::Linear), "");
        k0.tween_kind = TweenKind::Motion;
        let mut k6 = key(6, vec![shape("mover", 60.0)], Some(Easing::Linear), "");
        k6.tween_kind = TweenKind::Motion;
        k6.keyed_properties = Some(vec![TweenProperty::X]);
        timeline.layers[0].keyframes = vec![k0, k6];
    }
    motion_doc.validate().unwrap();
    let pasted_motion = clip
        .paste_into(&motion_doc, "clip-src", &root_id, &layer_id, 3, [0.0, 0.0])
        .expect("paste at intermediate frame of motion tween with ◆ property key must validate");
    pasted_motion.project.validate().unwrap();
    let split_key = pasted_motion
        .project
        .symbol(&root_id)
        .unwrap()
        .timeline
        .layers[0]
        .keyframes
        .iter()
        .find(|k| k.frame == 3)
        .unwrap();
    assert_eq!(split_key.tween, None);
    assert_eq!(split_key.tween_kind, TweenKind::Classic);

    // 2. Intermediate paste inside a Shape tween span preserves TweenKind::Shape on the split keyframe.
    let mut shape_doc = Project::blank();
    shape_doc.format_version = 6;
    let shape_root = shape_doc.root.clone();
    let shape_layer = shape_doc.symbol(&shape_root).unwrap().timeline.layers[0]
        .id
        .clone();
    {
        let timeline = &mut shape_doc.symbol_mut(&shape_root).unwrap().timeline;
        timeline.frame_count = 11;
        let mut s0 = key(0, vec![shape("morph", 0.0)], Some(Easing::Linear), "");
        s0.tween_kind = TweenKind::Shape;
        let s10 = key(10, vec![shape("morph", 100.0)], None, "");
        timeline.layers[0].keyframes = vec![s0, s10];
    }
    shape_doc.validate().unwrap();
    let pasted_shape = clip
        .paste_into(
            &shape_doc,
            "clip-src",
            &shape_root,
            &shape_layer,
            5,
            [0.0, 0.0],
        )
        .expect("paste at intermediate frame of shape tween must validate");
    pasted_shape.project.validate().unwrap();
    let shape_split = pasted_shape
        .project
        .symbol(&shape_root)
        .unwrap()
        .timeline
        .layers[0]
        .keyframes
        .iter()
        .find(|k| k.frame == 5)
        .unwrap();
    assert_eq!(shape_split.tween, Some(Easing::Linear));
    assert_eq!(shape_split.tween_kind, TweenKind::Shape);

    // 3. Intermediate paste inside a guided Classic tween samples along the motion guide and
    // preserves orient_to_path.
    let mut guided_doc = Project::blank();
    guided_doc.format_version = 6;
    let guided_root = guided_doc.root.clone();
    let guided_layer = guided_doc.symbol(&guided_root).unwrap().timeline.layers[0]
        .id
        .clone();
    {
        let timeline = &mut guided_doc.symbol_mut(&guided_root).unwrap().timeline;
        timeline.frame_count = 11;
        let mut c0 = key(0, vec![shape("runner", 0.0)], Some(Easing::Linear), "");
        c0.elements[0].transform.y = 0.0;
        c0.orient_to_path = true;
        let mut c10 = key(10, vec![shape("runner", 100.0)], None, "");
        c10.elements[0].transform.y = 100.0;
        timeline.layers[0].properties.motion_guide = Some("guide-row".into());
        timeline.layers[0].keyframes = vec![c0, c10];
        timeline.layers.push(Layer {
            id: "guide-row".into(),
            name: "Guide".into(),
            visible: true,
            locked: false,
            properties: LayerProperties {
                role: LayerRole::Guide,
                ..Default::default()
            },
            keyframes: vec![Keyframe {
                frame: 0,
                elements: vec![Element {
                    id: "guide-line".into(),
                    name: "guide-line".into(),
                    transform: Transform::default(),
                    kind: ElementKind::Path {
                        commands: vec![
                            PathCommand::MoveTo { x: 0.0, y: 0.0 },
                            PathCommand::LineTo { x: 100.0, y: 0.0 },
                            PathCommand::LineTo { x: 100.0, y: 100.0 },
                        ],
                        fill_rule: Default::default(),
                        fill: [0, 0, 0, 0],
                        stroke: [0, 0, 0, 255],
                        stroke_width: 1.0,
                    },
                    ..Default::default()
                }],
                ..Default::default()
            }],
        });
    }
    guided_doc.validate().unwrap();
    let pasted_guided = clip
        .paste_into(
            &guided_doc,
            "clip-src",
            &guided_root,
            &guided_layer,
            5,
            [0.0, 0.0],
        )
        .expect("paste at intermediate frame of guided classic tween must validate");
    pasted_guided.project.validate().unwrap();
    let guided_split = pasted_guided
        .project
        .symbol(&guided_root)
        .unwrap()
        .timeline
        .layers[0]
        .keyframes
        .iter()
        .find(|k| k.frame == 5)
        .unwrap();
    assert_eq!(guided_split.tween, Some(Easing::Linear));
    assert_eq!(guided_split.tween_kind, TweenKind::Classic);
    assert!(guided_split.orient_to_path);
    let runner = guided_split
        .elements
        .iter()
        .find(|e| e.id == "runner")
        .unwrap();
    assert!(
        (runner.transform.x - 100.0).abs() < 1e-6 && runner.transform.y.abs() < 1e-6,
        "guided runner at frame 5 must be sampled at guide corner (100, 0), got ({}, {})",
        runner.transform.x,
        runner.transform.y
    );
}
