use neoflash_core::{ElementKind, FillRule, InstancePlayback, ScriptLanguage, evaluate};
use neoflash_import::{ImportFormat, import_project, inspect};
use std::{fs, io::Read, path::Path};

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
