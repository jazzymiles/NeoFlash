use neoflash_core::{Project, ScriptLanguage, SourceUnit};
use neoflash_script::{
    DiagnosticSeverity,
    language_service::{HintContext, ThisKind},
};

fn as3_context() -> HintContext<'static> {
    HintContext::new(
        &Project::demo(),
        ScriptLanguage::ActionScript3,
        ThisKind::MovieClip,
    )
    .unwrap()
}

fn as2_context() -> HintContext<'static> {
    HintContext::new(
        &Project::demo(),
        ScriptLanguage::ActionScript2,
        ThisKind::MovieClip,
    )
    .unwrap()
}

#[test]
fn as3_owner_diagnostics_flag_invalid_members_and_legacy_callbacks_with_exact_spans() {
    let ctx = as3_context();

    let cases: &[(&str, &str, &str)] = &[
        (
            "import flash.text.TextField;\nvar tf:TextField = new TextField();\ntf.nonExistentProp = 1;",
            "nonExistentProp",
            "TextField",
        ),
        (
            "import flash.text.TextField;\nvar tf = new TextField();\ntf.innerHTML = '<b>hi</b>';",
            "innerHTML",
            "htmlText",
        ),
        (
            "import flash.text.TextField;\nvar tf:TextField = new TextField();\ntf.html = true;",
            "html",
            "htmlText",
        ),
        (
            "import flash.text.TextField;\nvar tf:TextField = new TextField();\ntf.onChanged = function():void {};",
            "onChanged",
            "addEventListener",
        ),
        (
            "import flash.display.BitmapData;\nvar bmp:BitmapData = new BitmapData(16, 16);\nbmp.unknownMethod();",
            "unknownMethod",
            "BitmapData",
        ),
        (
            "import flash.display.BitmapData;\nvar bmp = new BitmapData(16, 16);\nbmp.putPixel(1, 2, 0xff0000);",
            "putPixel",
            "setPixel",
        ),
        (
            "import flash.display.BitmapData;\nvar bmp:BitmapData = new BitmapData(16, 16);\nbmp.lock();",
            "lock",
            "not supported",
        ),
        (
            "import flash.display.Sprite;\nvar sp:Sprite = new Sprite();\nsp.graphics.fillRect(0, 0, 20, 20);",
            "fillRect",
            "drawRect",
        ),
        (
            "import flash.display.Graphics;\nfunction draw(g:Graphics):void {\n    g.arc(10, 10, 5);\n}",
            "arc",
            "drawCircle",
        ),
        (
            "import flash.media.Sound;\nvar snd:Sound = new Sound();\nsnd.start();",
            "start",
            "play",
        ),
        (
            "import flash.media.Sound;\nvar snd:Sound = new Sound();\nsnd.onSoundComplete = function():void {};",
            "onSoundComplete",
            "addEventListener",
        ),
        (
            "import flash.display.SimpleButton;\nvar btn:SimpleButton = new SimpleButton();\nbtn.onRelease = function():void {};",
            "onRelease",
            "addEventListener",
        ),
        (
            "import flash.display.Stage;\nStage.scaleMode = 'noScale';",
            "scaleMode",
            "stage.scaleMode",
        ),
        (
            "this.stage.onResize = function():void {};",
            "onResize",
            "addEventListener",
        ),
        (
            "import flash.net.NetStream;\nvar ns:NetStream;\nns.onMetaData = function(info:Object):void {};",
            "onMetaData",
            "client",
        ),
        (
            "import flash.net.SharedObject;\nvar so = SharedObject.getLocal('save');\nso.setItem('score', 10);",
            "setItem",
            "flush()",
        ),
        (
            "import flash.ui.ContextMenu;\nvar cm:ContextMenu = new ContextMenu();\ncm.onSelect = function():void {};",
            "onSelect",
            "addEventListener",
        ),
        (
            "var doc:XML = <items><item>1</item></items>;\ndoc.load('data.xml');",
            "load",
            "URLLoader",
        ),
        (
            "var doc:XML = <items/>;\ndoc.onLoad = function():void {};",
            "onLoad",
            "URLLoader",
        ),
    ];

    for &(source, expected_span_text, expected_hint) in cases {
        let analysis = ctx.semantic_analysis(source);
        let diags = analysis.diagnostics();
        assert!(
            !diags.is_empty(),
            "expected warning on `{expected_span_text}` in:\n{source}"
        );
        let matched = diags
            .iter()
            .find(|d| &source[d.span.0..d.span.1] == expected_span_text)
            .unwrap_or_else(|| {
                panic!(
                    "expected diagnostic span `{expected_span_text}` in:\n{source}\ngot: {diags:?}"
                )
            });
        assert_eq!(matched.severity, DiagnosticSeverity::Warning);
        assert!(
            matched.message.contains(expected_hint),
            "expected `{expected_hint}` in message `{}` for:\n{source}",
            matched.message
        );
    }
}

#[test]
fn as2_owner_diagnostics_flag_invalid_callbacks_html_string_and_unknown_members() {
    let ctx = as2_context();

    let cases: &[(&str, &str, &str)] = &[
        (
            "var tf:TextField;\ntf.html = '<b>bold</b>';",
            "html",
            "htmlText",
        ),
        (
            "this.createTextField('caption', 1, 0, 0, 120, 24);\ncaption.innerHTML = '<b>hi</b>';",
            "innerHTML",
            "htmlText",
        ),
        (
            "var tf:TextField;\ntf.onSomething = function() {};",
            "onSomething",
            "onChanged",
        ),
        (
            "var btn:Button;\nbtn.onSomething = function() {};",
            "onSomething",
            "Button",
        ),
        (
            "var btn:Button;\nbtn.onClick = function() {};",
            "onClick",
            "onRelease",
        ),
        ("var snd = new Sound();\nsnd.play();", "play", "start"),
        (
            "var snd:Sound = new Sound();\nsnd.onComplete = function() {};",
            "onComplete",
            "onSoundComplete",
        ),
        (
            "import flash.display.BitmapData;\nvar bmp:BitmapData = new BitmapData(10, 10);\nbmp.unknownMethod();",
            "unknownMethod",
            "BitmapData",
        ),
        (
            "var so = SharedObject.getLocal('prefs');\nso.save();",
            "save",
            "flush()",
        ),
        (
            "var xml = new XML();\nxml.toXMLString();",
            "toXMLString",
            "toString()",
        ),
    ];

    for &(source, expected_span_text, expected_hint) in cases {
        let analysis = ctx.semantic_analysis(source);
        let diags = analysis.diagnostics();
        assert!(
            !diags.is_empty(),
            "expected AS2 warning on `{expected_span_text}` in:\n{source}"
        );
        let matched = diags
            .iter()
            .find(|d| &source[d.span.0..d.span.1] == expected_span_text)
            .unwrap_or_else(|| {
                panic!("expected AS2 span `{expected_span_text}` in:\n{source}\ngot: {diags:?}")
            });
        assert_eq!(matched.severity, DiagnosticSeverity::Warning);
        assert!(
            matched.message.contains(expected_hint),
            "expected `{expected_hint}` in AS2 message `{}` for:\n{source}",
            matched.message
        );
    }
}

#[test]
fn class_fields_and_this_member_receivers_are_checked_without_false_positives() {
    let mut project = Project::blank();
    let class_src = "\
package demo {
    import flash.display.Sprite;
    import flash.text.TextField;
    public class Hud extends Sprite {
        private var label:TextField;
        private var inferred = new TextField();
        public function refresh():void {
            label.nonExistentProp = 1;
            this.inferred.innerHTML = '<b>score</b>';
            this.graphics.fillRect(0, 0, 50, 10);
        }
    }
}";
    project.sources.push(SourceUnit {
        id: "hud".into(),
        path: "demo/Hud.as".into(),
        language: ScriptLanguage::ActionScript3,
        source: class_src.into(),
    });
    let ctx = HintContext::new(&project, ScriptLanguage::ActionScript3, ThisKind::MovieClip)
        .unwrap()
        .with_this_class("demo.Hud".into());
    let analysis = ctx.semantic_analysis(class_src);
    let spans: Vec<&str> = analysis
        .diagnostics()
        .iter()
        .map(|d| &class_src[d.span.0..d.span.1])
        .collect();
    assert_eq!(spans, vec!["nonExistentProp", "innerHTML", "fillRect"]);
}

#[test]
fn dynamic_movieclip_object_prototype_e4x_and_valid_apis_emit_zero_warnings() {
    let as3 = as3_context();
    for valid_as3 in [
        // Dynamic MovieClip (`this` and `mc:MovieClip`) & Object
        "this.customScore = 42; this.customHelper = function():void {}; this.customHelper();",
        "import flash.display.MovieClip; var mc:MovieClip = new MovieClip(); mc.anyProp = 1; mc.anyMethod(); mc.onCustom = function():void {};",
        "var obj:Object = {}; obj.anything = 123; obj.run = function():void {}; obj.run();",
        // Untyped & wildcard variables
        "var untyped; untyped.nonExistentProp = 1; untyped.unknownMethod();",
        "function handle(raw, wild:*):void { raw.foo = 1; raw.bar(); wild.baz = 2; }",
        // Prototype extensions
        "import flash.text.TextField; TextField.prototype.customHelper = function():void {};",
        // Valid AS3 APIs across checked owners
        "import flash.text.TextField; import flash.events.Event; var tf:TextField = new TextField(); tf.text = 'ok'; tf.htmlText = '<b>ok</b>'; tf.addEventListener(Event.CHANGE, function(e:Event):void {});",
        "import flash.display.Sprite; var sp:Sprite = new Sprite(); sp.graphics.beginFill(0xff0000); sp.graphics.drawRect(0, 0, 10, 10); sp.graphics.endFill();",
        "import flash.display.BitmapData; import flash.geom.Rectangle; var bmp:BitmapData = new BitmapData(8, 8); bmp.fillRect(new Rectangle(0, 0, 8, 8), 0xffffffff); bmp.setPixel32(1, 1, 0xff00ff00);",
        "import flash.media.Sound; var snd:Sound = new Sound(); var ch = snd.play();",
        "import flash.net.SharedObject; var so:SharedObject = SharedObject.getLocal('save'); so.data.highScore = 99; so.flush();",
        "import flash.net.NetStream; var ns:NetStream; ns.client = { onMetaData: function(info:Object):void {} };",
        // E4X arbitrary child property reads & writes on XML / XMLList
        "var doc:XML = <catalog><book>Flash</book></catalog>; var list = doc.book; doc.author = 'NeoFlash'; var out:String = doc.toXMLString();",
    ] {
        let diags = as3.semantic_analysis(valid_as3).diagnostics().to_vec();
        assert!(
            diags.is_empty(),
            "unexpected AS3 warning for valid code:\n{valid_as3}\ngot: {diags:?}"
        );
    }

    let as2 = as2_context();
    for valid_as2 in [
        "this.anything = 1; this.onCustomHook = function() {}; var mc:MovieClip = _root; mc.score = 10; mc.customFn();",
        "var obj:Object = { a: 1 }; obj.b = 2; obj.c();",
        "var untyped = getThing(); untyped.foo = 1; untyped.bar();",
        "TextField.prototype.helper = function() {}; Sound.prototype.tag = 'bgm';",
        "var ss = new TextField.StyleSheet();",
        "var tf:TextField; tf.html = true; tf.htmlText = '<b>ok</b>'; tf._x = 12; tf.onChanged = function() {}; tf.onSetFocus = function() {};",
        "var btn:Button; btn._visible = true; btn.enabled = false; btn.onRelease = function() {}; btn.onRollOver = function() {};",
        "var snd:Sound = new Sound(); snd.attachSound('click'); snd.setVolume(80); snd.start(0, 1); snd.onSoundComplete = function() {};",
        "Stage.scaleMode = 'noScale'; Stage.align = 'TL'; Stage.addListener({ onResize: function() {} });",
        "var ns:NetStream; ns.onMetaData = function(info) {}; ns.onStatus = function(info) {}; ns.play('clip.flv');",
        "var so:SharedObject = SharedObject.getLocal('save'); so.data.level = 3; so.flush(); so.onSync = function() {};",
        "var xml:XML = new XML(); xml.ignoreWhite = true; xml.onLoad = function(ok:Boolean) {}; xml.load('feed.xml');",
    ] {
        let diags = as2.semantic_analysis(valid_as2).diagnostics().to_vec();
        assert!(
            diags.is_empty(),
            "unexpected AS2 warning for valid code:\n{valid_as2}\ngot: {diags:?}"
        );
    }
}
