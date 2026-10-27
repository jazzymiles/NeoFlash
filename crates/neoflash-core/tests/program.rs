use neoflash_core::*;

fn unit() -> SourceUnit {
    SourceUnit {
        id: "source-component".into(),
        path: "ui/Component.as".into(),
        language: ScriptLanguage::ActionScript3,
        source: "package ui { public class Component { public var count:int = 0; } }".into(),
    }
}

#[test]
fn program_document_round_trip_and_invalid_source_boundaries() {
    let legacy = Project::demo();
    assert!(!legacy.to_json().unwrap().contains("document_class"));
    let mut project = legacy;
    project.sources.push(unit());
    project.document_class = Some("ui.Component".into());
    assert!(project.validate().is_err());
    project.enable_program();
    assert_eq!(
        Project::from_json(&project.to_json().unwrap()).unwrap(),
        project
    );
    for path in ["../outside.as", "/absolute.as", "ui\\Component.as", ""] {
        let mut invalid = project.clone();
        invalid.sources[0].path = path.into();
        assert!(invalid.validate().is_err(), "{path}");
    }
    let mut collision = unit();
    collision.id = "second-source".into();
    collision.path = "UI/component.as".into();
    project.sources.push(collision);
    assert!(project.validate().is_err());
}

#[test]
fn linked_symbol_clipboard_transfers_program_snapshot_and_rejects_conflicts_atomically() {
    let mut source = Project::demo();
    source.enable_program();
    source.sources.push(unit());
    let element = source
        .symbol(&source.root)
        .unwrap()
        .timeline
        .layers
        .iter()
        .flat_map(|layer| &layer.keyframes)
        .flat_map(|key| &key.elements)
        .find(|element| matches!(element.kind, ElementKind::Instance { .. }))
        .unwrap()
        .clone();
    if let ElementKind::Instance { symbol_id, .. } = &element.kind {
        source.symbol_mut(symbol_id).unwrap().class_name = Some("ui.Component".into());
    }
    let payload =
        ClipboardPayload::capture(&source, vec![element], "source-session", [0., 0.]).unwrap();
    let parsed = ClipboardPayload::from_text(&payload.to_text().unwrap())
        .unwrap()
        .unwrap();
    let target = Project::blank();
    let root = target.root.clone();
    let layer = target.symbol(&root).unwrap().timeline.layers[0].id.clone();
    let pasted = parsed
        .paste_into(&target, "target-session", &root, &layer, 0, [0., 0.])
        .unwrap();
    assert_eq!(pasted.project.format_version, 2);
    assert_eq!(pasted.project.sources.len(), 1);
    assert_eq!(pasted.project.sources[0].source, source.sources[0].source);
    assert_ne!(pasted.project.sources[0].id, source.sources[0].id);
    assert!(
        pasted
            .project
            .symbols
            .iter()
            .any(|symbol| symbol.class_name.as_deref() == Some("ui.Component"))
    );
    let repeated = parsed
        .paste_into(
            &pasted.project,
            "target-session",
            &root,
            &layer,
            0,
            [10., 0.],
        )
        .unwrap();
    assert_eq!(repeated.project.sources.len(), 1);
    let mut conflict = pasted.project.clone();
    conflict.sources[0].source.push_str("// different");
    let before = conflict.clone();
    assert!(
        parsed
            .paste_into(&conflict, "target-session", &root, &layer, 0, [0., 0.])
            .is_err()
    );
    assert_eq!(conflict, before);
}
