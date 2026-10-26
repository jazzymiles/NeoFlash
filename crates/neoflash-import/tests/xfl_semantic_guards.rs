use neoflash_core::{ElementKind, ScriptLanguage, evaluate};
use neoflash_import::{Severity, import_project, inspect};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

struct Source(PathBuf);
impl Source {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "neoflash-xfl-semantic-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(path.join("LIBRARY")).unwrap();
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/simple");
        for name in [
            "DOMDocument.xml",
            "PublishSettings.xml",
            "LIBRARY/Badge.xml",
        ] {
            fs::copy(fixture.join(name), path.join(name)).unwrap();
        }
        Self(path)
    }

    fn replace(&self, before: &str, after: &str) {
        let path = self.0.join("DOMDocument.xml");
        let source = fs::read_to_string(&path).unwrap();
        assert!(source.contains(before), "missing replacement: {before}");
        fs::write(path, source.replace(before, after)).unwrap();
    }

    fn blocked(&self, code: &str) {
        let report = inspect(&self.0).unwrap();
        assert!(!report.can_import, "input unexpectedly accepted");
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.severity == Severity::Error && d.code == code),
            "expected {code}; got {:?}",
            report.diagnostics
        );
        assert!(
            import_project(&self.0).is_err(),
            "no partial project may escape"
        );
    }
}
impl Drop for Source {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn all_script_text_fragments_survive_xml_comments_cdata_and_processing_instructions() {
    let source = Source::new();
    source.replace(
        "<script><![CDATA[stop();]]></script>",
        "<script>trace(1);<!-- author note -->\n<![CDATA[trace(2);]]><?author note?>trace(3);</script>",
    );
    let project = import_project(&source.0).unwrap();
    let scripts = &project.symbol(&project.root).unwrap().timeline.scripts;
    assert_eq!(scripts.len(), 1);
    assert_eq!(scripts[0].script.language, ScriptLanguage::ActionScript2);
    assert_eq!(scripts[0].script.source, "trace(1);\ntrace(2);trace(3);");
}

#[test]
fn all_static_text_fragments_survive_xml_comments_and_cdata() {
    let source = Source::new();
    source.replace(
        "<characters>Hello NeoFlash</characters>",
        "<characters>Hello <!-- label -->Neo<![CDATA[Flash]]><?author note?> &amp; Rust</characters>",
    );
    let project = import_project(&source.0).unwrap();
    let items = evaluate(&project, &project.root, 0);
    let texts: Vec<_> = items
        .iter()
        .filter_map(|item| match &item.kind {
            ElementKind::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(texts, ["Hello NeoFlash & Rust"]);
}

#[test]
fn text_after_comments_still_receives_layout_validation() {
    let source = Source::new();
    source.replace(
        "Hello NeoFlash</characters>",
        "Hello<!-- split -->\nNeoFlash</characters>",
    );
    source.blocked("multiline_text");
}

#[test]
fn nested_semantic_elements_are_not_flattened_into_script_or_text() {
    let source = Source::new();
    source.replace(
        "<![CDATA[stop();]]>",
        "stop();<UnknownAction>play();</UnknownAction>trace(1);",
    );
    source.blocked("unsupported_element");
    let source = Source::new();
    source.replace(
        "Hello NeoFlash</characters>",
        "Hello<UnknownText>NeoFlash</UnknownText>!</characters>",
    );
    source.blocked("unsupported_element");
}

#[test]
fn namespaced_root_and_frame_attributes_never_alias_supported_attributes() {
    for namespace in ["urn:unrecognized", "http://ns.adobe.com/xfl/2008/"] {
        let source = Source::new();
        source.replace(
            "<DOMDocument xmlns=",
            &format!("<DOMDocument xmlns:e=\"{namespace}\" e:width=\"1\" xmlns="),
        );
        source.blocked("unsupported_attribute_namespace");
        let source = Source::new();
        source.replace(
            "<DOMDocument xmlns=",
            &format!("<DOMDocument xmlns:e=\"{namespace}\" xmlns="),
        );
        source.replace("<DOMFrame index=", "<DOMFrame e:index=\"500\" index=");
        source.blocked("unsupported_attribute_namespace");
    }
}

#[test]
fn namespaced_shape_paints_never_shadow_unqualified_color() {
    for color_attributes in [
        r##"e:color="#000000" color="#FF0000""##,
        r##"color="#FF0000" e:color="#000000""##,
    ] {
        let source = Source::new();
        source.replace(
            "<DOMDocument xmlns=",
            "<DOMDocument xmlns:e=\"urn:unrecognized\" xmlns=",
        );
        let shape = format!(
            r#"<DOMShape><fills><FillStyle index="1"><SolidColor {color_attributes}/></FillStyle></fills><edges><Edge fillStyle1="1" edges="!0 0|100 0|0 100|0 0"/></edges></DOMShape>"#
        );
        source.replace(
            "<elements><DOMSymbolInstance",
            &format!("<elements>{shape}<DOMSymbolInstance"),
        );
        source.blocked("unsupported_attribute_namespace");
    }
}

#[test]
fn empty_editor_containers_are_audited_and_warned() {
    let source = Source::new();
    source.replace(
        "</DOMDocument>",
        "<folders><!-- no entries --></folders><swatchLists/><swatches>  </swatches></DOMDocument>",
    );
    let report = inspect(&source.0).unwrap();
    assert!(report.can_import, "{:?}", report.diagnostics);
    for tag in ["folders", "swatchLists", "swatches"] {
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.severity == Severity::Warning
                    && d.code == "editor_metadata"
                    && d.location == format!("DOMDocument.xml/{tag}"))
        );
    }
    import_project(&source.0).unwrap();
}

#[test]
fn nonempty_editor_containers_cannot_hide_nested_semantics() {
    for tag in ["folders", "swatchLists", "swatches"] {
        for child in [
            "<DOMFolderItem name=\"Folder\"/>",
            "<Unknown><Actionscript><script>play();</script></Actionscript></Unknown>",
        ] {
            let source = Source::new();
            source.replace(
                "</DOMDocument>",
                &format!("<{tag}>{child}</{tag}></DOMDocument>"),
            );
            source.blocked("unsupported_element");
        }
        let source = Source::new();
        source.replace(
            "</DOMDocument>",
            &format!("<{tag}>hidden content</{tag}></DOMDocument>"),
        );
        source.blocked("unexpected_text");
        let source = Source::new();
        source.replace(
            "</DOMDocument>",
            &format!("<{tag} unknown=\"true\"/></DOMDocument>"),
        );
        source.blocked("unsupported_attribute");
        let source = Source::new();
        source.replace(
            "</DOMDocument>",
            &format!("<{tag} xmlns=\"urn:unrecognized\"/></DOMDocument>"),
        );
        source.blocked("unknown_namespace");
        let source = Source::new();
        source.replace(
            "</DOMDocument>",
            &format!("<{tag}/><{tag}><Unknown/></{tag}></DOMDocument>"),
        );
        source.blocked("duplicate_container");
    }
}
