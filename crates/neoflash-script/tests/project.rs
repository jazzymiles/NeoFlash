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
        "package { public class A { public function get value(arg:int):int{return arg;} } }",
        "package { public class A { public function set value(arg:int):int{return arg;} } }",
        "package { public class A { public function get value():int{return 1;} public function set value(arg:Number):void{} } }",
        "package { public class A { function get value():int{return 1;} } }",
        "package { public interface A { public function value():int; } }",
        "package { public interface A { function value():int{return 1;} } }",
        "package { public interface A { function A(); } }",
        "package { public interface A { var value:int; } }",
        "package { public class A { public function A(value:int=0){} } }",
        "package { public class A implements Missing {} }",
        "package { import flash.net.URLLoader; public class A extends URLLoader {} }",
        "package { import flash.net.URLVariables; public class A extends URLVariables {} }",
        "package { import flash.events.ProgressEvent; public class A extends ProgressEvent {} }",
        "package { import flash.net.URLRequestMethod; public class A { public function A(){new URLRequestMethod();} } }",
        "package { import flash.net.URLLoaderDataFormat; public class A { public function A(){new URLLoaderDataFormat();} } }",
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
    for source in [
        "interface A {function value():Number;}",
        "class A {public function get value():Number{return 1;}}",
    ] {
        legacy.sources[0].source = source.into();
        assert!(
            neoflash_script::compile_project(&legacy).is_err(),
            "{source}"
        );
    }
    legacy.sources[0].source = "import flash.utils.ByteArray; class A {}".into();
    assert!(
        neoflash_script::compile_project(&legacy).is_err(),
        "ByteArray is an AS3 family, not an AS2 import"
    );
    legacy.sources[0].source = "import flash.net.URLLoader; class A {}".into();
    assert!(
        neoflash_script::compile_project(&legacy).is_err(),
        "AS2 uses LoadVars, not the AS3 URLLoader import"
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
    for sources in [
        vec![
            (
                "I.as",
                "package { public interface I {function run(value:int):int;} }",
            ),
            (
                "A.as",
                "package {public class A implements I { public function run(value:Number):int{return 1;} }}",
            ),
        ],
        vec![
            (
                "I.as",
                "package { public interface I {function run():int;} }",
            ),
            (
                "A.as",
                "package {public class A implements I { function run():int{return 1;} }}",
            ),
        ],
        vec![
            (
                "I.as",
                "package { public interface I {function get value():int;} }",
            ),
            (
                "A.as",
                "package {public class A implements I { public var value:int; }}",
            ),
        ],
        vec![
            ("I.as", "package { public interface I {} }"),
            ("A.as", "package {public class A extends I {} }"),
        ],
        vec![
            ("I.as", "package { public interface I {} }"),
            (
                "A.as",
                "package {public class A {public function A(){new I();}} }",
            ),
        ],
        vec![
            (
                "I.as",
                "package { public interface I extends J {function get value():int;} }",
            ),
            ("J.as", "package {public interface J extends I {} }"),
        ],
        vec![
            (
                "I.as",
                "package { public interface I {function get value():int;} }",
            ),
            (
                "J.as",
                "package {public interface J extends I {function set value(next:Number):void;} }",
            ),
        ],
        vec![
            (
                "I.as",
                "package { public interface I {function run():int;} }",
            ),
            (
                "J.as",
                "package {public interface J {function run():String;} }",
            ),
            ("K.as", "package {public interface K extends I,J {} }"),
        ],
        vec![
            (
                "B.as",
                "package {public class B {public function get value():int{return 1;}}}",
            ),
            (
                "A.as",
                "package {public class A extends B {public function get value():int{return 2;}}}",
            ),
        ],
    ] {
        let result = neoflash_script::compile_project(&project(&sources));
        assert!(result.is_err(), "unexpected acceptance: {sources:?}");
        assert!(
            result
                .err()
                .unwrap()
                .iter()
                .all(|e| e.message.contains(".as:"))
        );
    }
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
    p.sources[0].source =
        "package model {public interface Model {function get value():int;}}".into();
    assert!(
        neoflash_script::compile_project(&p).is_err(),
        "An interface cannot be linked as a document class"
    );
    p.document_class = None;
    p.symbols[0].class_name = Some("model.Model".into());
    p.symbols[0].linkage_id = Some("exported-model".into());
    for base in ["flash.display.Shape", "flash.display.Sprite", "Object"] {
        p.sources[0].source = format!("package model {{ public class Model extends {base} {{}} }}");
        assert!(
            neoflash_script::compile_project(&p).is_err(),
            "Exported MovieClip cannot use native base {base}"
        );
    }
    p.sources[0].source =
        "package model { public class Model extends flash.display.MovieClip {} }".into();
    assert!(neoflash_script::compile_project(&p).is_ok());
}
