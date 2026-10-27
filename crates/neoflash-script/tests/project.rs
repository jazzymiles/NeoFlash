use neoflash_core::{Project, ScriptLanguage, SourceUnit};
fn project(sources: &[(&str, &str)]) -> Project {
    let mut project = Project::blank();
    project.format_version = 2;
    project.sources = sources
        .iter()
        .enumerate()
        .map(|(i, (path, source))| SourceUnit {
            id: format!("source-{i}"),
            path: (*path).into(),
            language: ScriptLanguage::ActionScript3,
            source: (*source).into(),
        })
        .collect();
    project
}
#[test]
fn module_resolution_fails_with_original_source_path() {
    for sources in [
        vec![(
            "Main.as",
            "package { import missing.Model; public class Main {} }",
        )],
        vec![
            ("A.as", "package { public class A extends B {} }"),
            ("B.as", "package { public class B extends A {} }"),
        ],
        vec![
            ("A.as", "package { public class A {} }"),
            ("duplicate.as", "package { public class A {} }"),
        ],
    ] {
        let errors = neoflash_script::compile_project(&project(&sources))
            .err()
            .expect("must reject");
        assert!(
            errors.iter().all(|e| e.message.contains(".as:")),
            "{errors:?}"
        );
    }
}
#[test]
fn class_syntax_outside_profile_is_never_erased() {
    for source in [
        "package { public class A {private var secret:int;} }",
        "package { public class A { public function get value():int{return 1;} } }",
        "package { public class A { public function A(value:int=0){} } }",
        "package { public class A implements Missing {} }",
        "package { import flash.display.MovieClip; public class A extends MovieClip {public var x:Number;} }",
        "package { import flash.display.Shape; public class A extends Shape {public var graphics:Object;} }",
        "package { import flash.display.Sprite; public class A extends Sprite {public function addChild(value:Object):void {}} }",
        "package { import flash.utils.ByteArray; public class A extends ByteArray {} }",
        "package { import flash.utils.Endian; public class A extends Endian {} }",
        "package { import flash.display.Graphics; public class A extends Graphics {} }",
        "package { import flash.display.Graphics; public class A {public function A(){new Graphics();}} }",
        "package { import flash.utils.Endian; public class A {public function A(){new Endian();}} }",
        "package { import flash.utils.ByteArray; public class A {public function A(){new ByteArray(8);}} }",
    ] {
        assert!(
            neoflash_script::compile_project(&project(&[("A.as", source)])).is_err(),
            "{source}"
        );
    }
    let mut legacy = project(&[("A.as", "class A { public var items:Array=[]; }")]);
    legacy.sources[0].language = ScriptLanguage::ActionScript2;
    assert!(
        neoflash_script::compile_project(&legacy).is_err(),
        "AS2 object initializers need constructor initialization"
    );
    legacy.sources[0].source = "package { public class A {} }".into();
    assert!(
        neoflash_script::compile_project(&legacy).is_err(),
        "AS3 package syntax must not be accepted as AS2"
    );
    legacy.sources[0].source = "import flash.utils.ByteArray; class A {}".into();
    assert!(
        neoflash_script::compile_project(&legacy).is_err(),
        "ByteArray is an AS3 family, not an AS2 import"
    );
    let mut mixed = project(&[
        ("A.as", "class A extends B {}"),
        ("B.as", "package {public class B {}}"),
    ]);
    mixed.sources[0].language = ScriptLanguage::ActionScript2;
    assert!(
        neoflash_script::compile_project(&mixed).is_err(),
        "Cross-dialect inheritance requires a deliberate bridge"
    );
}
#[test]
fn linked_classes_are_resolved_before_publication() {
    let mut p = project(&[(
        "Model.as",
        "package model {public class Model { public var value:int; }}",
    )]);
    p.document_class = Some("missing.Main".into());
    assert!(neoflash_script::compile_project(&p).is_err());
    p.document_class = Some("model.Model".into());
    assert!(neoflash_script::compile_project(&p).is_ok());
}
