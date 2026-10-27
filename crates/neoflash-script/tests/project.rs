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
        "package { public class A { function get value():int{return 1;} } }",
        "package { public interface A { public function value():int; } }",
        "package { public interface A { function value():int{return 1;} } }",
        "package { public interface A { function A(); } }",
        "package { public interface A { var value:int; } }",
        "package { public class A { public function A(value:int=0){} } }",
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
        "interface A {function value():Number;}",
        "class A extends Date {}",
        "import flash.geom.Transform; class A extends Transform {}",
        "import flash.geom.ColorTransform; class A extends ColorTransform {}",
        "class A {public function A(){new RegExp(\"x\");}}",
        "class A {public function A(){JSON.parse(\"{}\");}}",
        "class A {public function get value():Number{return 1;}}",
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
    ] {
        legacy.sources[0].source = source.into();
        assert!(
            neoflash_script::compile_project(&legacy).is_err(),
            "{source}"
        );
    }
    legacy.sources[0].source = "class A {public var images:MovieClipLoader; public var saved:SharedObject; public function A(){images=new MovieClipLoader();saved=SharedObject.getLocal(\"save\");} public function persist(value:Object):Boolean{saved.data.value=value;var size:Number=saved.getSize();return saved.flush();} public function erase():Void{saved.clear();} public function focus(target:TextField):Boolean {Selection.addListener(this);target.type=\"input\";target.maxChars=12;target.restrict=\"A-Z\";Selection.setSelection(1,3);var start:Number=Selection.getBeginIndex();var end:Number=Selection.getEndIndex();var caret:Number=Selection.getCaretIndex();var old:String=Selection.getFocus();target.setSelection(0,1);target.replaceSelectedText(\"A\");Selection.removeListener(this);return Selection.setFocus(target);} public function onSetFocus(old:Object,next:Object):Void{this.saved.data.focus=next!=null;} public function shadow(Selection:Object):String{return Selection.getFocus();} public function load(target:MovieClip):Boolean {images.addListener(this);return images.loadClip(\"image.png\",target);} public function progress(target:MovieClip):Object {return images.getProgress(target);} public function unload(target:MovieClip):Boolean {images.removeListener(this);return images.unloadClip(target);}}".into();
    assert!(neoflash_script::compile_project(&legacy).is_ok());
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
