use neoflash_import::{Severity, import_project, inspect};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static ID: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "neoflash-metadata-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(path.join("LIBRARY")).unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/simple");
        for name in [
            "DOMDocument.xml",
            "PublishSettings.xml",
            "LIBRARY/Badge.xml",
        ] {
            fs::copy(source.join(name), path.join(name)).unwrap();
        }
        Self(path)
    }
    fn change(&self, from: &str, to: &str) {
        let p = self.0.join("DOMDocument.xml");
        let text = fs::read_to_string(&p).unwrap();
        assert!(text.contains(from));
        fs::write(p, text.replace(from, to)).unwrap();
    }
    fn publish(&self, language: &str, settings: &str) {
        fs::write(self.0.join("PublishSettings.xml"),format!("<flash_profiles><flash_profile><PublishFlashProperties>{language}{settings}</PublishFlashProperties></flash_profile></flash_profiles>")).unwrap();
    }
    fn blocked(&self, code: &str) {
        let report = inspect(&self.0).unwrap();
        assert!(!report.can_import);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.severity == Severity::Error && d.code == code),
            "{:?}",
            report.diagnostics
        );
        assert!(import_project(&self.0).is_err());
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn inactive_as3_defaults_are_only_ignored_for_unambiguous_as2() {
    let temp = Temp::new();
    let settings = "<AS3PackagePaths>.</AS3PackagePaths><AS3ConfigConst>CONFIG::FLASH_AUTHORING=&quot;true&quot;;</AS3ConfigConst>";
    temp.publish("<ActionScriptVersion>2</ActionScriptVersion>", settings);
    let report = inspect(&temp.0).unwrap();
    assert!(report.can_import, "{:?}", report.diagnostics);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == "inactive_script_configuration")
    );
    for language in [
        "<ActionScriptVersion>3</ActionScriptVersion>",
        "",
        "<ActionScriptVersion>2</ActionScriptVersion><ActionScriptVersion>3</ActionScriptVersion>",
    ] {
        temp.publish(language, settings);
        temp.blocked("external_script_configuration");
    }
    for active in [
        "<PackagePaths>source</PackagePaths>",
        "<DocumentClass>Main</DocumentClass>",
    ] {
        temp.publish("<ActionScriptVersion>2</ActionScriptVersion>", active);
        temp.blocked("external_script_configuration");
    }
}

#[test]
fn known_editor_metadata_warns_but_unknown_children_still_block() {
    let temp = Temp::new();
    temp.change(
        "<DOMDocument xmlns=",
        "<DOMDocument platform=\"Windows\" xmlns=",
    );
    temp.change(
        "<Include href=",
        "<Include itemID=\"source-library-id\" href=",
    );
    temp.change("</DOMDocument>","<PrinterSettings/><publishHistory><PublishItem publishSize=\"430\" publishTime=\"1580411027\"/><PublishItem publishSize=\"417\" publishTime=\"1580410915\"/></publishHistory></DOMDocument>");
    let report = inspect(&temp.0).unwrap();
    assert!(report.can_import, "{:?}", report.diagnostics);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == "editor_metadata")
    );
    temp.change(
        "<PrinterSettings/>",
        "<PrinterSettings><Actionscript/></PrinterSettings>",
    );
    temp.blocked("unsupported_element");
    temp.change(
        "<PrinterSettings><Actionscript/></PrinterSettings>",
        "<PrinterSettings/>",
    );
    temp.change("<publishHistory>", "<publishHistory><UnknownFeature/>");
    temp.blocked("unsupported_element");
}

#[test]
fn pivot_metadata_cannot_change_placement_or_hide_actual_3d() {
    let temp = Temp::new();
    temp.change(
        "<DOMSymbolInstance name=",
        "<DOMSymbolInstance centerPoint3DX=\"900\" centerPoint3DY=\"-12\" name=",
    );
    let report = inspect(&temp.0).unwrap();
    assert!(report.can_import, "{:?}", report.diagnostics);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == "transform_pivot")
    );
    let project = import_project(&temp.0).unwrap();
    let items = neoflash_core::evaluate(&project, &project.root, 0);
    assert_eq!((items[1].transform[4], items[1].transform[5]), (40., 60.));
    temp.change("centerPoint3DX=\"900\"", "centerPoint3DX=\"NaN\"");
    temp.blocked("invalid_number");
    temp.change(
        "centerPoint3DX=\"NaN\"",
        "centerPoint3DX=\"900\" centerPoint3DZ=\"1\"",
    );
    temp.blocked("unsupported_attribute");
}

#[test]
fn nested_and_split_publish_values_cannot_hide_script_configuration() {
    let temp = Temp::new();
    temp.publish(
        "<ActionScriptVersion>2</ActionScriptVersion>",
        "<PackagePaths><path>source</path></PackagePaths>",
    );
    temp.blocked("invalid_script_configuration");
    temp.publish(
        "<ActionScriptVersion>2<!-- split -->3</ActionScriptVersion>",
        "<AS3PackagePaths>.</AS3PackagePaths>",
    );
    temp.blocked("script_language");
    temp.blocked("external_script_configuration");
    temp.publish(
        "<ActionScriptVersion>2</ActionScriptVersion>",
        "<AS3PackagePaths xmlns=\"urn:unknown\">.</AS3PackagePaths>",
    );
    temp.blocked("invalid_script_configuration");
    temp.publish(
        "<ActionScriptVersion>2</ActionScriptVersion><ActionScriptVersion/>",
        "<AS3PackagePaths>.</AS3PackagePaths>",
    );
    temp.blocked("external_script_configuration");
}
