use neoflash_core::{ElementKind, ScriptLanguage, evaluate};
use neoflash_import::{ImportFormat, Severity, import_project, inspect};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let path = std::env::temp_dir().join(format!(
            "neoflash-import-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/simple")
}
fn simple(element: &str) -> String {
    format!(
        r#"<DOMDocument width="640" height="480" frameRate="24"><timelines><DOMTimeline><layers><DOMLayer><frames><DOMFrame index="0" duration="3"><elements>{element}</elements></DOMFrame></frames></DOMLayer></layers></DOMTimeline></timelines></DOMDocument>"#
    )
}
fn write_source(temp: &Temp, xml: &str) {
    fs::write(temp.0.join("DOMDocument.xml"), xml).unwrap();
}

#[test]
fn imports_symbols_frames_text_and_actionscript_without_execution() {
    let report = inspect(&fixture()).unwrap();
    assert_eq!(report.format, ImportFormat::XflDirectory);
    assert!(report.can_import, "{:?}", report.diagnostics);
    let p = import_project(&fixture()).unwrap();
    p.validate().unwrap();
    assert_eq!(p.symbols.len(), 2);
    assert_eq!(p.fps, 24);
    assert_eq!(p.symbols[0].timeline.frame_count, 4);
    assert_eq!(
        p.symbols[0].timeline.scripts[0].script.language,
        ScriptLanguage::ActionScript2
    );
    assert_eq!(p.symbols[0].timeline.scripts[0].script.source, "stop();");
    let items = evaluate(&p, &p.root, 2);
    assert_eq!(items.len(), 2);
    assert!(matches!(items[0].kind, ElementKind::Text { .. }));
    assert_eq!(items[1].transform[4], 80.);
    assert_eq!(items[1].alpha, 0.5);
    let layers = &p.symbols[0].timeline.layers;
    assert_eq!(layers[0].name, "Caption");
    assert_eq!(layers[1].name, "Foreground");
}

#[test]
fn imports_the_same_subset_from_zip_without_extracting() {
    let temp = Temp::new();
    let path = temp.0.join("example.fla");
    let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
    for name in [
        "DOMDocument.xml",
        "PublishSettings.xml",
        "LIBRARY/Badge.xml",
    ] {
        zip.start_file(
            name,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )
        .unwrap();
        zip.write_all(&fs::read(fixture().join(name)).unwrap())
            .unwrap();
    }
    zip.finish().unwrap();
    let report = inspect(&path).unwrap();
    assert_eq!(report.format, ImportFormat::XflArchive);
    assert!(report.can_import, "{:?}", report.diagnostics);
    assert_eq!(import_project(&path).unwrap().symbols.len(), 2);
}

#[test]
fn unsupported_geometry_and_unknown_semantics_block_import() {
    let temp = Temp::new();
    write_source(
        &temp,
        &simple("<DOMShape><edges><Edge edges=\"!0 0|100 100\" /></edges></DOMShape>"),
    );
    let report = inspect(&temp.0).unwrap();
    assert!(!report.can_import);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error && d.code == "xfl_shape_fill")
    );
    assert!(import_project(&temp.0).is_err());
    write_source(
        &temp,
        &simple(
            "<DOMRectangleObject objectWidth=\"10\" objectHeight=\"10\" unknownEffect=\"yes\"/>",
        ),
    );
    assert!(!inspect(&temp.0).unwrap().can_import);
}

#[test]
fn missing_text_nodes_and_skew_are_not_silently_discarded() {
    let temp = Temp::new();
    write_source(&temp, &simple("<DOMStaticText/>"));
    assert!(!inspect(&temp.0).unwrap().can_import);
    write_source(
        &temp,
        &simple(
            "<DOMRectangleObject objectWidth=\"10\" objectHeight=\"10\"><matrix><Matrix c=\"0.5\"/></matrix></DOMRectangleObject>",
        ),
    );
    let skewed = import_project(&temp.0).unwrap();
    skewed.validate().unwrap();
    assert_eq!(skewed.format_version, 5);
    let items = evaluate(&skewed, &skewed.root, 0);
    assert!((items[0].transform[2] - 0.5).abs() < 1e-6);

    write_source(
        &temp,
        &simple(
            "<DOMRectangleObject objectWidth=\"10\" objectHeight=\"10\"><matrix><Matrix a=\"1\" b=\"2\" c=\"2\" d=\"4\"/></matrix></DOMRectangleObject>",
        ),
    );
    assert!(
        inspect(&temp.0)
            .unwrap()
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error && d.code == "matrix_singular")
    );
}

#[test]
fn rejects_archive_traversal_and_oversized_input() {
    let temp = Temp::new();
    let path = temp.0.join("bad.fla");
    let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
    zip.start_file(
        "../DOMDocument.xml",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    zip.write_all(simple("").as_bytes()).unwrap();
    zip.finish().unwrap();
    assert!(inspect(&path).is_err());
    let big = temp.0.join("large");
    fs::create_dir(&big).unwrap();
    let file = fs::File::create(big.join("DOMDocument.xml")).unwrap();
    file.set_len(8 * 1024 * 1024 + 1).unwrap();
    assert!(inspect(&big).is_err());
}

#[test]
fn rejects_dtd_and_malformed_xml() {
    let temp = Temp::new();
    write_source(
        &temp,
        "<!DOCTYPE a [<!ENTITY external SYSTEM 'file:///etc/passwd'>]><DOMDocument>&external;</DOMDocument>",
    );
    let report = inspect(&temp.0).unwrap();
    assert!(!report.can_import);
    assert!(report.diagnostics.iter().any(|d| d.code == "xml_parse"));
    write_source(&temp, "<DOMDocument>");
    assert!(!inspect(&temp.0).unwrap().can_import);
}

#[test]
fn probes_legacy_fla_and_swf_without_claiming_editable_import() {
    let temp = Temp::new();
    let fla = temp.0.join("legacy.fla");
    fs::write(&fla, [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]).unwrap();
    // A magic signature alone is a truncated container, not a recognized FLA.
    assert!(inspect(&fla).is_err());
    let swf = temp.0.join("movie.swf");
    fs::write(&swf, [b'F', b'W', b'S', 8, 8, 0, 0, 0]).unwrap();
    let report = inspect(&swf).unwrap();
    assert_eq!(report.format, ImportFormat::Swf);
    assert_eq!(report.version.as_deref(), Some("8 (FWS)"));
    assert!(!report.can_import);
    fs::write(&swf, [b'F', b'W', b'S', 8, 50, 0, 0, 0]).unwrap();
    assert!(inspect(&swf).is_err());
}

#[test]
fn short_layers_become_blank_after_their_span() {
    let temp = Temp::new();
    write_source(
        &temp,
        r#"<DOMDocument><timelines><DOMTimeline><layers><DOMLayer><frames><DOMFrame index="0" duration="2"><elements><DOMRectangleObject objectWidth="10" objectHeight="10"/></elements></DOMFrame></frames></DOMLayer><DOMLayer><frames><DOMFrame index="0" duration="5"><elements/></DOMFrame></frames></DOMLayer></layers></DOMTimeline></timelines></DOMDocument>"#,
    );
    let p = import_project(&temp.0).unwrap();
    assert_eq!(evaluate(&p, &p.root, 1).len(), 1);
    assert!(evaluate(&p, &p.root, 2).is_empty());
}
