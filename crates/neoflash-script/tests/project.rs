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
    // Project-resource includes share one meaningful class/module resolution family.
    let mut included_project = project(&[]);
    included_project.resources = vec![
        neoflash_core::ResourceUnit::from_bytes(
            "include-main",
            "scripts/main.as",
            "text/plain",
            b"#include '../shared.as'\ntrace(answer);",
        )
        .unwrap(),
        neoflash_core::ResourceUnit::from_bytes(
            "include-shared",
            "shared.as",
            "text/plain",
            b"var answer:Number=42;",
        )
        .unwrap(),
        neoflash_core::ResourceUnit::from_bytes(
            "include-cycle",
            "cycle.as",
            "text/plain",
            b"#include 'cycle.as'",
        )
        .unwrap(),
    ];
    let program = neoflash_script::compile_project(&included_project).unwrap();
    let script = |source: &str| neoflash_core::Script {
        language: ScriptLanguage::ActionScript2,
        source: source.into(),
    };
    assert!(
        neoflash_script::compile_with_program(&script("#include \"scripts/main.as\""), &program)
            .is_ok()
    );
    for source in [
        "#include \"missing.as\"",
        "#include \"cycle.as\"",
        "#include \"../shared.as\"",
    ] {
        assert!(neoflash_script::compile_with_program(&script(source), &program).is_err());
    }
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
        "package { public class A {public private var secret:int;} }",
        "package { import flash.external.ExternalInterface; public class A extends ExternalInterface {} }",
        "package { import flash.external.ExternalInterface; public class A {public function A(){new ExternalInterface();}} }",
        "package {import flash.media.Video;public class A extends Video {}}",
        "package {import flash.net.NetConnection;public class A extends NetConnection {}}",
        "package {import flash.net.NetStream;public class A extends NetStream {}}",
        "package {import flash.events.NetStatusEvent;public class A extends NetStatusEvent {}}",
        "package {import flash.net.NetStream;public class A {public function A(){new NetStream();}}}",
        "package {import flash.net.NetConnection;public class A {public function A(){new NetConnection(1);}}}",
        "package {import flash.media.Video;public class A {public function A(){new Video(1,2,3);}}}",
        "package { public class A extends Date {} }",
        "package { public class A extends XML {} }",
        "package { public class A extends XMLList {} }",
        "package { public class A extends Namespace {} }",
        "package { public class A extends QName {} }",
        "package { public class A extends Vector {} }",
        "package { import flash.utils.Dictionary; public class A extends Dictionary {} }",
        "package { import flash.geom.Transform; public class A extends Transform {} }",
        "package { import flash.geom.ColorTransform; public class A extends ColorTransform {} }",
        "package { import flash.geom.Transform; public class A {public function A(){new Transform();}} }",
        "package { import flash.display.MovieClip; public class A extends MovieClip {public var transform:Object;} }",
        "package { public class A extends RegExp {} }",
        "package { public class A extends JSON {} }",
        "package { public class A {public function A(){new JSON();}} }",
        "package { public class A {public var json:JSON;} }",
        "package { public class A {public function A(){JSON(3);}} }",
        "package { public class A {public function check(Date:Object):Boolean{return null is Date;}} }",
        "package { public class A { public function get value(arg:int):int{return arg;} } }",
        "package { public class A { public function set value(arg:int):int{return arg;} } }",
        "package { public class A { public function get value():int{return 1;} public function set value(arg:Number):void{} } }",
        "package { public class A { private function get value():int{return 1;} public function set value(next:int):void{} } }",
        "package { public interface A { public function value():int; } }",
        "package { public interface A { function value():int{return 1;} } }",
        "package { public interface A { function A(); } }",
        "package { public interface A { var value:int; } }",
        "package { public class A { public function A(value:int=0,required:int){} } }",
        "package { public class A { public function A(value:int=Number(1)){} } }",
        "package { public class A { public function get value(...items):int{return 1;} } }",
        "package { public class A { public function set value(next:int=1):void{} } }",
        "package { public class A { public function set value(next:int,...items):void{} } }",
        "package { public class A { public function run(value:int,...value):void{} } }",
        "package { public class A implements Missing {} }",
        "package { import flash.display.Bitmap; public class A extends Bitmap {} }",
        "package { import flash.display.Loader; public class A extends Loader {} }",
        "package { import flash.display.LoaderInfo; public class A extends LoaderInfo {} }",
        "package { import flash.display.LoaderInfo; public class A { public function A(){new LoaderInfo();} } }",
        "package { import flash.display.Loader; public class A { public function A(){new Loader(4);} } }",
        "package { public class A { public function A(){var legacy=MovieClipLoader;} } }",
        "package { import flash.text.TextField; public class A extends TextField {} }",
        "package { import flash.media.Sound; public class A extends Sound {} }",
        "package { import flash.media.SoundChannel; public class A extends SoundChannel {} }",
        "package { import flash.media.SoundTransform; public class A extends SoundTransform {} }",
        "package { import flash.media.SoundChannel; public class A { public function A(){new SoundChannel();} } }",
        "package { import flash.text.TextFormat; public class A extends TextFormat {} }",
        "package { import flash.text.TextFormatAlign; public class A extends TextFormatAlign {} }",
        "package { import flash.text.TextFieldType; public class A extends TextFieldType {} }",
        "package { import flash.text.TextFieldType; public class A {public function A(){new TextFieldType();}} }",
        "package { import flash.events.FocusEvent; public class A extends FocusEvent {} }",
        "package { import flash.events.TextEvent; public class A extends TextEvent {} }",
        "package { public class A {public function A(){var legacy=Selection;}} }",
        "package { import flash.text.TextFormatAlign; public class A { public function A(){new TextFormatAlign();} } }",
        "package { import flash.text.TextField; public class A { public function A(){new TextField(4);} } }",
        "package { import flash.display.BitmapData; public class A extends BitmapData {} }",
        "package { import flash.display.PixelSnapping; public class A { public function A(){new PixelSnapping();} } }",
        "package { import flash.display.PNGEncoderOptions; public class A extends PNGEncoderOptions {} }",
        "package { import flash.display.PNGEncoderOptions; public class A {public function A(){new PNGEncoderOptions(false,true);}} }",
        "package { import flash.net.URLLoader; public class A extends URLLoader {} }",
        "package { import flash.net.SharedObject; public class A extends SharedObject {} }",
        "package { import flash.net.ObjectEncoding; public class A extends ObjectEncoding {} }",
        "package { import flash.net.SharedObjectFlushStatus; public class A extends SharedObjectFlushStatus {} }",
        "package { import flash.net.SharedObject; public class A { public function A(){new SharedObject();} } }",
        "package { import flash.net.ObjectEncoding; public class A { public function A(){new ObjectEncoding();} } }",
        "package { import flash.net.SharedObjectFlushStatus; public class A { public function A(){new SharedObjectFlushStatus();} } }",
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
        "package { import flash.display.GradientType; public class A extends GradientType {} }",
        "package { import flash.display.GraphicsPathCommand; public class A {public function A(){new GraphicsPathCommand();}} }",
        "package { import flash.display.MovieClip; public class A extends MovieClip {public function beginGradientFill():void{}} }",
        "package { import flash.display.Sprite; public class A extends Sprite {public var mask:Object;} }",
        "package { import flash.display.MovieClip; public class A extends MovieClip {public var loaderInfo:Object;} }",
        "package { import flash.display.MovieClip; public class A extends MovieClip {public var filters:Array;} }",
        "package { import flash.display.Sprite; public class A extends Sprite {public var mouseEnabled:Boolean;} }",
        "package { import flash.display.Sprite; public class A extends Sprite {public function startDrag():void{}} }",
        "package { import flash.filters.BitmapFilter; public class A extends BitmapFilter {} }",
        "package { import flash.filters.BitmapFilterQuality; public class A {public function A(){new BitmapFilterQuality();}} }",
        "package { import flash.filters.BlurFilter; public class A extends BlurFilter {} }",
        "package { import flash.filters.GlowFilter; public class A extends GlowFilter {} }",
        "package { import flash.filters.DropShadowFilter; public class A extends DropShadowFilter {} }",
        "package { import flash.filters.ColorMatrixFilter; public class A extends ColorMatrixFilter {} }",
        "package { import flash.filters.BitmapFilter; public class A {public function A(){new BitmapFilter();}} }",
        "package { import flash.display.Shape; public class A extends Shape {public function get scrollRect():Object{return null;}} }",
        "package { import flash.display.MovieClip; public class A extends MovieClip {public function setMask(value:Object):void{}} }",
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
        "class A {public function A(value:Number=0){}}",
        "import flash.external.ExternalInterface;class A extends ExternalInterface {}",
        "import flash.external.ExternalInterface;class A {public function A(){new ExternalInterface();}}",
        "import flash.net.navigateToURL;class A {}",
        "class A {public function run(...values):Void{}}",
        "interface A {function value():Number{return 1;}}",
        "class A {public function A(){new Video();}}",
        "class A {public function A(){new NetConnection(1);}}",
        "class A {public function A(){new NetStream();}}",
        "import flash.net.NetStream;class A {}",
        "class A extends Date {}",
        "import flash.geom.Transform; class A extends Transform {}",
        "import flash.geom.ColorTransform; class A extends ColorTransform {}",
        "class A {public function A(){new RegExp(\"x\");}}",
        "class A {public function A(){JSON.parse(\"{}\");}}",
        "class A {public function get value(arg:Number):Number{return arg;}}",
        "class A extends TextField {}",
        "class A extends TextFormat {}",
        "class A {public function A(){new TextField();}}",
        "class A {public function A(){new Sound(this);}}",
        "class A {public function A(){new MovieClipLoader(this);}}",
        "class A extends MovieClipLoader {}",
        "class A {public function A(){new SharedObject();}}",
        "class A extends SharedObject {}",
        "class A {public function A(){new Selection();}}",
        "class A {public function A(){Selection();}}",
        "class A extends Selection {}",
        "import flash.filters.BitmapFilter; class A extends BitmapFilter {}",
        "import flash.filters.BitmapFilterQuality; class A {}",
        "import flash.filters.BlurFilter; class A extends BlurFilter {}",
        "import flash.filters.BitmapFilter; class A {public function A(){new BitmapFilter();}}",
        "class A extends MovieClip {public var filters:Array;}",
        "import flash.text.TextFieldType; class A {}",
        "import flash.events.FocusEvent; class A {}",
        "import flash.events.TextEvent; class A {}",
        "import flash.net.SharedObject; class A {}",
        "import flash.net.ObjectEncoding; class A {}",
        "import flash.display.Loader; class A {}",
        "import flash.display.LoaderInfo; class A {}",
        "import flash.media.Sound; class A {}",
        "import flash.display.GradientType; class A {}",
        "import flash.display.PNGEncoderOptions; class A {}",
    ] {
        legacy.sources[0].source = source.into();
        assert!(
            neoflash_script::compile_project(&legacy).is_err(),
            "{source}"
        );
    }
    legacy.sources[0].source = "class A {public var images:MovieClipLoader; public var saved:SharedObject; public function A(){images=new MovieClipLoader();saved=SharedObject.getLocal(\"save\");} public function persist(value:Object):Boolean{saved.data.value=value;var size:Number=saved.getSize();return saved.flush();} public function erase():Void{saved.clear();} public function focus(target:TextField):Boolean {Selection.addListener(this);target.type=\"input\";target.maxChars=12;target.restrict=\"A-Z\";Selection.setSelection(1,3);var start:Number=Selection.getBeginIndex();var end:Number=Selection.getEndIndex();var caret:Number=Selection.getCaretIndex();var old:String=Selection.getFocus();target.setSelection(0,1);target.replaceSelectedText(\"A\");Selection.removeListener(this);return Selection.setFocus(target);} public function onSetFocus(old:Object,next:Object):Void{this.saved.data.focus=next!=null;} public function shadow(Selection:Object):String{return Selection.getFocus();} public function load(target:MovieClip):Boolean {images.addListener(this);return images.loadClip(\"image.png\",target);} public function progress(target:MovieClip):Object {return images.getProgress(target);} public function unload(target:MovieClip):Boolean {images.removeListener(this);return images.unloadClip(target);}}".into();
    assert!(neoflash_script::compile_project(&legacy).is_ok());
    legacy.sources[0].source = "dynamic class A {private var stored:Number=1; private static var count:Number=0; public function get value():Number{return stored;} public function set value(next:Number):Void{stored=next;count++;} public static function get changes():Number{return count;} public function exercise():Number{value+=4;return value+changes;}}".into();
    assert!(
        neoflash_script::compile_project(&legacy).is_ok(),
        "AS2 concrete source accessors reuse class descriptors"
    );
    let custom_selection = project(&[
        (
            "Selection.as",
            "package {public class Selection {public function Selection(){} public static function getFocus():String{return \"user\";}}}",
        ),
        (
            "User.as",
            "package {public class User {public function readFocus():String{var value:Selection=new Selection();return Selection.getFocus();}}}",
        ),
    ]);
    let custom_result = neoflash_script::compile_project(&custom_selection);
    assert!(custom_result.is_ok(), "{:?}", custom_result.err());
    let mut custom_selection = custom_selection;
    for source in &mut custom_selection.sources {
        source.language = ScriptLanguage::ActionScript2;
        source.source = source.source.replacen("package {", "", 1);
        source.source.pop();
    }
    let custom_result = neoflash_script::compile_project(&custom_selection);
    assert!(custom_result.is_ok(), "{:?}", custom_result.err());
    legacy.sources[0].source = r#"import flash.filters.BitmapFilter; import flash.filters.BlurFilter; import flash.filters.GlowFilter; import flash.filters.DropShadowFilter; import flash.filters.ColorMatrixFilter;
        class A {public function apply(target:MovieClip):BitmapFilter {
            var blur:BlurFilter=new BlurFilter(4,6,1);var glow:GlowFilter=new GlowFilter(0x336699,.5,4,4,2);
            var shadow:DropShadowFilter=new DropShadowFilter(3,45,0,.5);var matrix:ColorMatrixFilter=new ColorMatrixFilter();
            var base:BitmapFilter=blur.clone();target.filters=[base,glow,shadow,matrix];var copy:Array=target.filters;
            copy[0].blurX=8;target.filters=copy;target.filters=null;return base;
        }}"#.into();
    assert!(neoflash_script::compile_project(&legacy).is_ok());
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
    legacy.sources[0].source = "import flash.display.Bitmap; class A {}".into();
    assert!(
        neoflash_script::compile_project(&legacy).is_err(),
        "AS2 uses MovieClip.attachBitmap, not the AS3 Bitmap constructor"
    );
    legacy.sources[0].source =
        "import flash.display.BitmapData; class A extends BitmapData {}".into();
    assert!(
        neoflash_script::compile_project(&legacy).is_err(),
        "BitmapData native inheritance is not implemented in either dialect"
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
                "package {public interface I {function run(value:int=1):int;}}",
            ),
            (
                "A.as",
                "package {public class A implements I {public function run(value:int):int{return value;}}}",
            ),
        ],
        vec![
            (
                "B.as",
                "package {public class B {public function run(value:int=1):int{return value;}}}",
            ),
            (
                "A.as",
                "package {public class A extends B {public override function run(value:int):int{return value;}}}",
            ),
        ],
        vec![
            (
                "I.as",
                "package {public interface I {function run(...values):int;}}",
            ),
            (
                "A.as",
                "package {public class A implements I {public function run():int{return 0;}}}",
            ),
        ],
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
    // One package/component contract: namespace-separated private fields,
    // protected dispatch, package-private collaborators and deliberate imports.
    let packages = project(&[
        (
            "model/Base.as",
            "package model {public class Base {private var value:int=10; protected var factor:int=2; internal var revision:int=0; public function Base(){} protected function delta():int{return factor;} public final function advance():int{value+=delta();revision++;return value;} public function get total():int{return value;}}}",
        ),
        (
            "model/Derived.as",
            "package model {public final class Derived extends Base {private var value:int=100; public function Derived(){super();} protected override function delta():int{return super.delta()*2;} public function get local():int{return value;} public function ledger():int{return Ledger.read(this);}}}",
        ),
        (
            "model/Ledger.as",
            "package model {class Ledger {internal static function read(value:Base):int{return value.revision;}}}",
        ),
        (
            "data/Record.as",
            "package data {public dynamic class Record {public var title:String; private var secret:int=9; public function get summary():String{return title;}}}",
        ),
        (
            "data/Fixed.as",
            "package data {public class Fixed extends Record {public function Fixed(){super();}}}",
        ),
        (
            "Main.as",
            "package {import model.*;import data.*;public class Main {private var counter:Derived;public function Main(){counter=new Derived();}public function run():int{var record:Record=new Record();record.extra=1;return counter.advance()+counter.local()+counter.ledger();}}}",
        ),
    ]);
    let result = neoflash_script::compile_project(&packages);
    assert!(result.is_ok(), "{:?}", result.err());
    for sources in [
        vec![
            ("Base.as", "package {public final class Base {}}"),
            (
                "Derived.as",
                "package {public class Derived extends Base {}}",
            ),
        ],
        vec![
            (
                "Base.as",
                "package {public class Base {public final function run():int{return 1;}}}",
            ),
            (
                "Derived.as",
                "package {public class Derived extends Base {public override function run():int{return 2;}}}",
            ),
        ],
        vec![
            (
                "Base.as",
                "package {public class Base {protected function run():int{return 1;}}}",
            ),
            (
                "Derived.as",
                "package {public class Derived extends Base {public override function run():int{return 2;}}}",
            ),
        ],
        vec![
            ("model/Hidden.as", "package model {class Hidden {}}"),
            (
                "Main.as",
                "package {import model.Hidden;public class Main {}}",
            ),
        ],
        vec![
            (
                "model/Hidden.as",
                "package model {internal class Hidden {}}",
            ),
            (
                "Main.as",
                "package {public class Main {public var hidden:model.Hidden;}}",
            ),
        ],
        vec![
            (
                "model/deep/Hidden.as",
                "package model.deep {public class Hidden {}}",
            ),
            (
                "Main.as",
                "package {import model.*;public class Main {public var value:Hidden;}}",
            ),
        ],
        vec![
            ("a/Entry.as", "package a {public class Entry {}}"),
            ("b/Entry.as", "package b {public class Entry {}}"),
            (
                "Main.as",
                "package {import a.*;import b.*;public class Main {}}",
            ),
        ],
    ] {
        let result = neoflash_script::compile_project(&project(&sources));
        assert!(
            result.is_err(),
            "unexpected class/package acceptance: {sources:?}"
        );
        assert!(
            result
                .err()
                .unwrap()
                .iter()
                .all(|error| error.message.contains(".as:"))
        );
    }
    let exact_wins = project(&[
        ("a/Entry.as", "package a {public class Entry {}}"),
        ("b/Entry.as", "package b {public class Entry {}}"),
        (
            "Main.as",
            "package {import a.*;import b.*;import a.Entry;public class Main {public var value:Entry;}}",
        ),
    ]);
    assert!(neoflash_script::compile_project(&exact_wins).is_ok());
    let mut legacy_private = project(&[
        (
            "legacy/Base.as",
            "class legacy.Base {private var value:Number=3;private function add():Number{return value+1;}public function Base(){}}",
        ),
        (
            "legacy/Child.as",
            "class legacy.Child extends legacy.Base {public function read():Number{return value+add();}}",
        ),
    ]);
    for source in &mut legacy_private.sources {
        source.language = ScriptLanguage::ActionScript2;
    }
    let legacy_program = neoflash_script::compile_project(&legacy_private)
        .expect("AS2 subclasses inherit private access");
    let mut outside = neoflash_core::Script {
        language: ScriptLanguage::ActionScript2,
        source: "import legacy.Child;var value:Child=new Child();result=value.value;".into(),
    };
    assert!(neoflash_script::compile_with_program(&outside, &legacy_program).is_err());
    outside.source =
        "import legacy.Child;var value:Child=new Child();result=value[\"value\"];".into();
    assert!(
        neoflash_script::compile_with_program(&outside, &legacy_program).is_ok(),
        "AS2 indexed access remains a runtime public escape"
    );
    let video_project = project(&[(
        "Player.as",
        r#"package {
        import flash.media.Video;import flash.media.SoundTransform;import flash.net.NetConnection;import flash.net.NetStream;import flash.events.NetStatusEvent;
        public class Player {
            public var stream:NetStream;public var video:Video;
            public function Player(){var connection:NetConnection=new NetConnection();connection.connect(null);stream=new NetStream(connection);stream.soundTransform=new SoundTransform(0);stream.client=this;stream.addEventListener(NetStatusEvent.NET_STATUS,onStatus);video=new Video(320,180);video.attachNetStream(stream);}
            public function onStatus(event:NetStatusEvent):void{var code:String=event.info.code;}
            public function onMetaData(info:Object):void{}
            public function run():Boolean{stream.play("media/motion.mp4");stream.pause();stream.seek(2);stream.resume();stream.togglePause();stream.close();video.clear();video.attachNetStream(null);return (stream is NetStream)&&((video as Video)!=null);}
        }}"#,
    )]);
    let video_program = neoflash_script::compile_project(&video_project);
    assert!(video_program.is_ok(), "{:?}", video_program.err());
    let legacy_video=neoflash_core::Script{language:ScriptLanguage::ActionScript2,source:"var connection:NetConnection=new NetConnection();connection.connect(null);var stream:NetStream=new NetStream(connection);placedVideo.attachVideo(stream);stream.onStatus=function(info:Object):Void{result=info.code;};stream.play(\"media/motion.mp4\");stream.pause(true);stream.seek(2);stream.pause(false);stream.close();placedVideo.clear();".into()};
    assert!(neoflash_script::compile(&legacy_video).is_ok());
    let legacy_drag=neoflash_core::Script{language:ScriptLanguage::ActionScript2,source:"card.onPress=function():Void{this.startDrag(false,20,20,250,145);localX=this._xmouse;};card.onReleaseOutside=function():Void{this.stopDrag();hit=this.hitTest(zone);pointHit=this.hitTest(300,200,true);drop=this._droptarget;};card.onRollOver=function():Void{rolls++;};card.onRollOut=function():Void{rolls--;};".into()};
    let drag_code = neoflash_script::compile(&legacy_drag).expect("legacy drag callbacks compile");
    assert!(drag_code.contains("setLegacyCallback"));
    let web_project = project(&[(
        "Web.as",
        r#"package {
        import flash.external.ExternalInterface;import flash.net.navigateToURL;import flash.net.sendToURL;import flash.net.URLRequest;
        public class Web {public function Web(){ExternalInterface.addCallback("echo",echo);}public function echo(value:Object):Object{return value;}
            public function send(request:URLRequest):Object{var local:Function=navigateToURL;local(request,"preview");sendToURL(request);return ExternalInterface.call("Host.reply",{value:7});}
        }}"#,
    )]);
    let web_result = neoflash_script::compile_project(&web_project);
    assert!(web_result.is_ok(), "{:?}", web_result.err());
    let legacy_web=neoflash_core::Script{language:ScriptLanguage::ActionScript2,source:"import flash.external.ExternalInterface;function echo(value:Object):Object{return value;}var ok:Boolean=ExternalInterface.addCallback(\"legacy\",this,echo);var reply:Object=ExternalInterface.call(\"Host.reply\",7);getURL(\"/receipt\",\"preview\");this.getURL(\"/receipt\",\"preview\",\"GET\");var form:LoadVars=new LoadVars();form.send(\"/receipt\",\"preview\");".into()};
    let web_code = neoflash_script::compile_with_program(&legacy_web, &web_result.unwrap())
        .expect("AS2 web interface compiles");
    assert!(web_code.contains("__nfLegacy.ExternalInterface"));
    assert!(web_code.contains("__nfLegacy.getURL.call(__nfFrame"));
    let shadow_web = neoflash_core::Script {
        language: ScriptLanguage::ActionScript2,
        source: "function getURL(value:String):String{return value;}result=getURL(\"custom\");"
            .into(),
    };
    assert!(
        !neoflash_script::compile(&shadow_web)
            .unwrap()
            .contains("__nfLegacy.getURL")
    );
    let mut shadow_video = project(&[
        (
            "NetConnection.as",
            "class NetConnection {public function NetConnection(value:Number){} public function connect(value:Object):Void{}}",
        ),
        (
            "Use.as",
            "class Use {public function Use(){var connection:NetConnection=new NetConnection(7);connection.connect(null);}}",
        ),
    ]);
    for source in &mut shadow_video.sources {
        source.language = ScriptLanguage::ActionScript2;
    }
    let shadow_video_result = neoflash_script::compile_project(&shadow_video);
    assert!(
        shadow_video_result.is_ok(),
        "{:?}",
        shadow_video_result.err()
    );
    let mut legacy_interfaces = project(&[
        (
            "contracts/Named.as",
            "interface contracts.Named {public function label():String;}",
        ),
        (
            "contracts/Counter.as",
            "interface contracts.Counter extends contracts.Named {function value():Number;}",
        ),
        (
            "contracts/Resettable.as",
            "interface contracts.Resettable {function reset():Void;}",
        ),
        (
            "model/Base.as",
            "class model.Base {public function label():String{return \"counter\";}}",
        ),
        (
            "model/Counter.as",
            "class model.Counter extends model.Base implements contracts.Counter, contracts.Resettable {public function value():Number{return 3;} public function reset():Void{}}",
        ),
    ]);
    for unit in &mut legacy_interfaces.sources {
        unit.language = ScriptLanguage::ActionScript2;
    }
    let linked = neoflash_script::compile_project(&legacy_interfaces)
        .expect("AS2 inherited interfaces reuse the owned linker");
    let legacy_interface_script = neoflash_core::Script {language:ScriptLanguage::ActionScript2,source:"import model.Counter;import contracts.Named;var value:Named=new Counter();result=value.label();".into()};
    assert!(neoflash_script::compile_with_program(&legacy_interface_script, &linked).is_ok());
    for bad in [
        "interface contracts.Named {function get label():String;}",
        "interface contracts.Named {public static function label():String;}",
        "interface contracts.Named {public function label():Number;}",
        "interface contracts.Named {function missing():Void;}",
        "interface contracts.Named {var label:String;}",
    ] {
        legacy_interfaces.sources[0].source = bad.into();
        assert!(
            neoflash_script::compile_project(&legacy_interfaces).is_err(),
            "{bad}"
        );
    }
    let optional_contracts = project(&[
        (
            "I.as",
            "package {public interface I {function run(value:int=1,...values):int;}}",
        ),
        (
            "B.as",
            "package {public class B implements I {public function B(seed:int=0){} public function run(value:int=2,...values):int{return value+values.length;}}}",
        ),
        (
            "A.as",
            "package {public class A extends B {public function A(){super();} public override function run(value:int=3,...values):int{return super.run(value)+values.length;}}}",
        ),
    ]);
    let result = neoflash_script::compile_project(&optional_contracts);
    assert!(
        result.is_ok(),
        "optional contracts permit different default values: {:?}",
        result.err()
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
        r#"package model {
            import flash.display.Graphics;
            import flash.display.GradientType;
            import flash.display.SpreadMethod;
            import flash.display.InterpolationMethod;
            import flash.display.GraphicsPathCommand;
            import flash.display.GraphicsPathWinding;
            import flash.geom.Matrix;
            import flash.geom.Rectangle;
            import flash.display.DisplayObject;
            import flash.display.Loader;
            import flash.display.LoaderInfo;
            import flash.display.BitmapData;
            import flash.display.PNGEncoderOptions;
            import flash.net.URLRequest;
            import flash.net.SharedObject;
            import flash.net.SharedObjectFlushStatus;
            import flash.net.ObjectEncoding;
            import flash.utils.ByteArray;
            import flash.text.TextField;
            import flash.text.TextFieldType;
            import flash.events.TextEvent;
            import flash.events.FocusEvent;
            import flash.filters.BitmapFilter;
            import flash.filters.BitmapFilterQuality;
            import flash.filters.BlurFilter;
            import flash.filters.GlowFilter;
            import flash.filters.DropShadowFilter;
            import flash.filters.ColorMatrixFilter;
            public class Model extends flash.display.MovieClip {
                public function imagePipeline(source:BitmapData):ByteArray {
                    var copy:BitmapData=source.clone();
                    copy.colorTransform(copy.rect,new flash.geom.ColorTransform());
                    copy.applyFilter(source,source.rect,new flash.geom.Point(),new BlurFilter());
                    var affected:Rectangle=copy.generateFilterRect(copy.rect,new BlurFilter());
                    copy.draw(source,new Matrix(),null,"normal",affected,false);
                    var pixels:ByteArray=copy.getPixels(copy.rect);pixels.position=0;
                    copy.setPixels(copy.rect,pixels);pixels.clear();
                    copy.copyPixelsToByteArray(copy.rect,pixels);
                    var options:PNGEncoderOptions=new PNGEncoderOptions(true);
                    return copy.encode(copy.rect,options);
                }
                public var savedFilter:BitmapFilter;
                public function applyFilters():BitmapFilter {
                    var blur:BlurFilter=new BlurFilter(4,6,BitmapFilterQuality.LOW);var glow:GlowFilter=new GlowFilter(0x336699,.5,4,4,2);
                    var shadow:DropShadowFilter=new DropShadowFilter(3,45,0,.5);var matrix:ColorMatrixFilter=new ColorMatrixFilter();
                    var values:Array=matrix.matrix;values[4]=16;matrix.matrix=values;
                    savedFilter=BitmapFilter(blur.clone());filters=[savedFilter,glow,shadow,matrix];
                    var copy:Array=filters;copy[0].blurX=8;filters=copy;filters=null;
                    if (savedFilter is BlurFilter){return savedFilter as BlurFilter;}
                    return null;
                }
                public var input:TextField;
                public var lastInput:TextEvent;
                public var lastFocus:FocusEvent;
                public function setupInput():void {
                    input=new TextField();input.type=TextFieldType.INPUT;
                    input.selectable=true;input.displayAsPassword=false;
                    input.maxChars=12;input.restrict="A-Za-z";
                    input.setSelection(0,0);input.replaceSelectedText("Ada");
                    input.addEventListener(TextEvent.TEXT_INPUT,onInput);
                    input.addEventListener(FocusEvent.FOCUS_IN,onFocus);
                    stage.focus=input;
                }
                public function onInput(event:TextEvent):TextEvent {
                    lastInput=TextEvent(event);if (event.text=="!"){event.preventDefault();}
                    var start:int=input.selectionBeginIndex;var end:int=input.selectionEndIndex;
                    var caret:int=input.caretIndex;
                    if (lastInput is TextEvent){return lastInput as TextEvent;}
                    return new TextEvent(TextEvent.TEXT_INPUT,true,true,"test");
                }
                public function onFocus(event:FocusEvent):FocusEvent {
                    lastFocus=FocusEvent(event);
                    if (lastFocus is FocusEvent){return lastFocus as FocusEvent;}
                    return new FocusEvent(FocusEvent.FOCUS_OUT,true,false,input);
                }
                public var saved:SharedObject;
                public function persist(value:Object):Boolean {
                    SharedObject.defaultObjectEncoding=ObjectEncoding.AMF3;
                    saved=SharedObject.getLocal("compiler-save");
                    var typed:SharedObject=SharedObject(saved);
                    if (!(typed is SharedObject)) {return false;}
                    var bytes:ByteArray=new ByteArray();
                    ByteArray.defaultObjectEncoding=ObjectEncoding.AMF0;
                    bytes.objectEncoding=ObjectEncoding.AMF3;
                    bytes.writeObject(value);bytes.position=0;
                    typed.data.value=bytes.readObject();
                    var result:String=typed.flush();
                    return result==SharedObjectFlushStatus.FLUSHED && (typed as SharedObject)!=null;
                }
                public var imageLoader:Loader;
                public var imageInfo:LoaderInfo;
                public function loadImage(request:URLRequest,bytes:ByteArray):LoaderInfo {
                    imageLoader=new Loader();imageLoader.load(request);imageLoader.close();
                    imageLoader.loadBytes(bytes);
                    imageInfo=LoaderInfo(imageLoader.contentLoaderInfo);
                    imageLoader.unload();
                    if (imageInfo is LoaderInfo) {return imageInfo as LoaderInfo;}
                    return null;
                }
                public function clip(source:DisplayObject,view:Rectangle):Rectangle {
                    mask=source; scrollRect=view;
                    var copy:Rectangle=scrollRect;copy.y+=1;scrollRect=copy;
                    mask=null;scrollRect=null;return copy;
                }
                public function drag(view:Rectangle,target:DisplayObject):Boolean {
                    mouseEnabled=true;mouseChildren=false;buttonMode=true;useHandCursor=true;
                    startDrag(false,view);stopDrag();
                    var local:Number=mouseX+mouseY;
                    return hitTestObject(target)&&hitTestPoint(local,0,true);
                }
                public function paint(source:Graphics):void {
                    var drawing:Graphics=graphics;
                    var box:Matrix=new Matrix(); box.createGradientBox(80,40);
                    drawing.beginGradientFill(GradientType.LINEAR,[0xff0000,0x0000ff],[1,1],[0,255],box,SpreadMethod.PAD,InterpolationMethod.RGB,0);
                    drawing.lineStyle(2); drawing.lineGradientStyle(GradientType.RADIAL,[0,0xffffff],[1,1],[0,255],box);
                    drawing.drawRoundRect(0,0,80,40,8);
                    drawing.drawRoundRectComplex(90,0,80,40,2,4,6,8);
                    var commands:Vector.<int>=Vector.<int>([GraphicsPathCommand.MOVE_TO,GraphicsPathCommand.CUBIC_CURVE_TO]);
                    var values:Vector.<Number>=Vector.<Number>([0,0,10,10,20,10,30,0]);
                    drawing.drawPath(commands,values,GraphicsPathWinding.NON_ZERO);
                    drawing.cubicCurveTo(35,0,40,10,45,0);drawing.endFill();
                    drawing.copyFrom(source);
                }
            }
        }"#.into();
    assert!(neoflash_script::compile_project(&p).is_ok());
}
