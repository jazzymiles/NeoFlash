use neoflash_core::{ElementKind, FillRule, InstancePlayback, ScriptLanguage, evaluate};
use neoflash_import::{ImportFormat, import_project, inspect};
use std::{fs, io::Read, path::Path};

#[test]
fn native_mx_button_states_and_static_text_match_the_public_sources() {
    use neoflash_core::{
        FlattenOptions, Player, Project, SymbolKind, TextAlign, hit_test_path, path_bounds,
    };
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/fla-corpus");
    // Values independently checked against the pinned JPEXS reader's XFL
    // export of these unchanged source files. This is a semantic/model check,
    // not an Adobe rendering or browser-script execution oracle.
    for (relative, is_button) in [
        ("upstream/ruffle/mx-button/DefineButton-MX.fla", true),
        ("upstream/ruffle/mx-static-text/DefineText2-MX.fla", false),
    ] {
        let path = corpus.join(relative);
        let original = fs::read(&path).unwrap();
        let report = inspect(&path).unwrap();
        assert!(report.can_import, "{:?}", report.diagnostics);
        assert_eq!(report.format, ImportFormat::LegacyBinaryFla);
        assert_eq!(report.statistics.scripts, 0);
        assert!(
            report
                .diagnostics
                .iter()
                .all(|diagnostic| !diagnostic.location.is_empty())
        );
        let project = import_project(&path).unwrap();
        project.validate().unwrap();
        assert_eq!((project.width, project.height, project.fps), (550, 400, 12));
        let root = project.symbol(&project.root).unwrap();
        assert_eq!(root.timeline.frame_count, 1);
        assert_eq!(root.timeline.layers.len(), 1);
        let placement = &root.timeline.layers[0].keyframes[0].elements[0];
        if is_button {
            let ElementKind::Instance {
                symbol_id,
                playback,
            } = &placement.kind
            else {
                panic!()
            };
            assert_eq!(*playback, InstancePlayback::Independent);
            assert_eq!((placement.transform.x, placement.transform.y), (37.5, 23.));
            let button = project.symbol(symbol_id).unwrap();
            assert_eq!(button.name, "TestButton");
            assert_eq!(button.kind, SymbolKind::Button);
            assert_eq!(button.timeline.frame_count, 4);
            assert_eq!(button.timeline.layers.len(), 2);
            let mut player = Player::new(&project);
            let reference = player
                .object_ref(std::slice::from_ref(&placement.id))
                .unwrap();
            for (state, expected_color, dimensions) in [
                (0, [51, 102, 153, 255], (65., 36.)),
                (1, [51, 102, 153, 255], (65., 36.)),
                (2, [0, 0, 0, 255], (130., 72.)),
            ] {
                assert!(player.object_button_state(&project, &reference, state));
                let snapshot = player.runtime_snapshot(&project);
                assert_eq!(snapshot.draws.len(), 1, "Hit artwork must not be painted");
                let neoflash_core::RuntimeDrawKind::Element(ElementKind::PaintedPath {
                    commands,
                    fill,
                    ..
                }) = &snapshot.draws[0].item.kind
                else {
                    panic!()
                };
                assert_eq!(*fill, expected_color);
                let bounds = path_bounds(commands).unwrap().unwrap();
                assert_eq!(
                    (bounds.x_max - bounds.x_min, bounds.y_max - bounds.y_min),
                    dimensions
                );
                let object = snapshot
                    .objects
                    .iter()
                    .find(|object| object.object_ref == reference)
                    .unwrap();
                assert_eq!(object.button_hits.len(), 1);
                let ElementKind::PaintedPath {
                    commands,
                    fill_rule,
                    ..
                } = &object.button_hits[0].kind
                else {
                    panic!()
                };
                assert!(
                    hit_test_path(
                        commands,
                        *fill_rule,
                        [120., 60.],
                        true,
                        0.,
                        FlattenOptions::default()
                    )
                    .unwrap()
                );
                assert!(
                    !hit_test_path(
                        commands,
                        *fill_rule,
                        [140., 60.],
                        true,
                        0.,
                        FlattenOptions::default()
                    )
                    .unwrap()
                );
            }
            assert!(!player.object_button_state(&project, &reference, 3));
        } else {
            assert_eq!((placement.transform.x, placement.transform.y), (13., 10.75));
            assert_eq!(placement.transform.alpha, 128. / 255.);
            let ElementKind::TextBox {
                text,
                width,
                height,
                format,
                word_wrap,
                multiline,
            } = &placement.kind
            else {
                panic!()
            };
            assert_eq!(text, "-_-");
            assert_eq!((*width, *height), (27., 19.45));
            assert_eq!(format.font, "Verdana");
            assert_eq!(format.size, 16.);
            assert!(format.bold && format.italic);
            assert_eq!(format.align, TextAlign::Left);
            assert_eq!(format.color, 0xf0f0f0);
            assert_eq!(format.leading, 2.);
            assert!(!word_wrap && !multiline);
        }
        let resource = project
            .resources
            .iter()
            .find(|resource| resource.media_type == "application/x-neoflash-fla-source")
            .unwrap();
        assert_eq!(resource.decoded_bytes().unwrap(), original);
        assert_eq!(
            Project::from_json(&project.to_json().unwrap()).unwrap(),
            project
        );
        assert_eq!(fs::read(path).unwrap(), original);
    }
}

#[test]
fn imports_the_unchanged_public_adobe_fla_with_all_artwork_and_scripts() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/fla-corpus/upstream/ruffle/avm1-frame-labels/test.fla");
    let before = fs::read(&source).unwrap();
    assert_eq!(before.len(), 6025);
    let report = inspect(&source).unwrap();
    assert_eq!(report.format, ImportFormat::XflArchive);
    assert!(report.can_import, "{:?}", report.diagnostics);
    assert_eq!(
        (
            report.statistics.symbols,
            report.statistics.layers,
            report.statistics.keyframes,
            report.statistics.elements,
            report.statistics.scripts
        ),
        (2, 3, 11, 3, 9)
    );
    let p = import_project(&source).unwrap();
    p.validate().unwrap();
    // Independently confirmed by the original companion SWF header and styles.
    assert_eq!((p.width, p.height, p.fps), (550, 400, 24));
    let root = p.symbol(&p.root).unwrap();
    assert_eq!(root.timeline.frame_count, 7);
    let labels: Vec<_> = root
        .timeline
        .layers
        .iter()
        .flat_map(|l| &l.keyframes)
        .filter(|k| !k.label.is_empty())
        .map(|k| (k.frame, k.label.as_str()))
        .collect();
    assert_eq!(
        labels,
        [(2, "SomeFrame"), (4, "end"), (5, "SomeFrame"), (6, "BADà")]
    );
    let clip = root
        .timeline
        .layers
        .iter()
        .flat_map(|l| &l.keyframes)
        .flat_map(|k| &k.elements)
        .find(|e| e.name == "clip")
        .unwrap();
    assert_eq!((clip.transform.x, clip.transform.y), (21.25, 21.25));
    let ElementKind::Instance {
        symbol_id,
        playback,
    } = &clip.kind
    else {
        panic!("lost clip")
    };
    assert_eq!(*playback, InstancePlayback::Independent);
    let child = p.symbol(symbol_id).unwrap();
    assert_eq!(child.timeline.frame_count, 2);
    for key in &child.timeline.layers[0].keyframes {
        assert_eq!(key.elements.len(), 1);
        let ElementKind::Path {
            commands,
            fill_rule,
            fill,
            stroke,
            stroke_width,
        } = &key.elements[0].kind
        else {
            panic!("ordinary Flash shape did not become an editable path")
        };
        assert_eq!(*fill_rule, FillRule::NonZero);
        assert_eq!(*fill, [255, 0, 0, 255]);
        assert_eq!(*stroke, [0, 0, 0, 255]);
        assert_eq!(*stroke_width, 1.0);
        let bounds = neoflash_core::path_bounds(commands).unwrap().unwrap();
        assert_eq!(
            (bounds.x_min, bounds.y_min, bounds.x_max, bounds.y_max),
            (-21.25, -21.25, 21.25, 21.25)
        );
    }
    assert_eq!(evaluate(&p, &p.root, 0).len(), 1);
    assert!(evaluate(&p, &p.root, 5).is_empty()); // Original placement ends at frame 5.
    // Compare every source script with the original ZIP XML, without normalizing text.
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&before)).unwrap();
    for (name, symbol) in [("DOMDocument.xml", root), ("LIBRARY/Symbol 1.xml", child)] {
        let mut xml = String::new();
        zip.by_name(name).unwrap().read_to_string(&mut xml).unwrap();
        let document = roxmltree::Document::parse(&xml).unwrap();
        let source_scripts: Vec<_> = document
            .descendants()
            .filter(|n| n.has_tag_name("script"))
            .map(|n| n.text().unwrap())
            .collect();
        let imported_scripts: Vec<_> = symbol
            .timeline
            .scripts
            .iter()
            .map(|s| {
                assert_eq!(s.script.language, ScriptLanguage::ActionScript2);
                s.script.source.as_str()
            })
            .collect();
        assert_eq!(imported_scripts, source_scripts);
    }
    assert_eq!(fs::read(source).unwrap(), before);
}
