use neoflash_core::{Project, Script, ScriptLanguage, SourceUnit};
use std::process::Command;

fn fte3d_project(sources: &[(&str, &str)]) -> Project {
    let mut project = Project::blank();
    project.name = "AS3 Area 8 FTE Stage3D Shaders Suite".into();
    project.width = 800;
    project.height = 600;
    project.fps = 30;
    project.format_version = 3;
    project.sources = sources
        .iter()
        .enumerate()
        .map(|(i, (path, source))| SourceUnit {
            id: format!("fte3d-src-{i}"),
            path: (*path).into(),
            language: ScriptLanguage::ActionScript3,
            source: (*source).into(),
        })
        .collect();
    project
}

#[test]
fn as3_fte_ime_stage3d_shaders_and_3d_display_compile_and_execute_end_to_end() {
    let proj = fte3d_project(&[
        (
            "app/ImeEditorClient.as",
            r#"
            package app {
                import flash.geom.Rectangle;
                import flash.text.ime.CompositionAttributeRange;
                import flash.text.ime.IIMEClient;

                public class ImeEditorClient implements IIMEClient {
                    private var _buf:String = "NeoFlash ";
                    private var _compStart:int = 9;
                    private var _compEnd:int = 9;
                    private var _selAnchor:int = 9;
                    private var _selActive:int = 9;
                    public var lastAttrCount:int = 0;

                    public function ImeEditorClient() {}

                    public function get compositionStartIndex():int {
                        return this._compStart;
                    }

                    public function get compositionEndIndex():int {
                        return this._compEnd;
                    }

                    public function get selectionActiveIndex():int {
                        return this._selActive;
                    }

                    public function get selectionAnchorIndex():int {
                        return this._selAnchor;
                    }

                    public function get verticalTextLayout():Boolean {
                        return false;
                    }

                    public function updateComposition(
                        text:String,
                        attributes:Vector.<CompositionAttributeRange>,
                        compositionStartIndex:int,
                        compositionEndIndex:int
                    ):void {
                        this._compStart = compositionStartIndex;
                        this._compEnd = compositionEndIndex;
                        this.lastAttrCount = attributes ? attributes.length : 0;
                        this._buf = "NeoFlash " + text;
                    }

                    public function confirmComposition(text:String = null, preserveSelection:Boolean = false):void {
                        if (text != null) {
                            this._buf = "NeoFlash " + text;
                        }
                        this._compStart = this._buf.length;
                        this._compEnd = this._buf.length;
                        if (!preserveSelection) {
                            this._selAnchor = this._buf.length;
                            this._selActive = this._buf.length;
                        }
                    }

                    public function getTextBounds(startIndex:int, endIndex:int):Rectangle {
                        return new Rectangle(startIndex * 8, 0, (endIndex - startIndex) * 8, 18);
                    }

                    public function getTextInRange(startIndex:int, endIndex:int):String {
                        return this._buf.substring(startIndex, endIndex);
                    }

                    public function selectRange(anchorIndex:int, activeIndex:int):void {
                        this._selAnchor = anchorIndex;
                        this._selActive = activeIndex;
                    }
                }
            }
            "#,
        ),
        (
            "app/Area8Verifier.as",
            r#"
            package app {
                import flash.display.*;
                import flash.display3D.*;
                import flash.display3D.textures.*;
                import flash.events.Event;
                import flash.events.EventDispatcher;
                import flash.filters.ShaderFilter;
                import flash.geom.*;
                import flash.text.engine.*;
                import flash.text.ime.*;
                import flash.utils.ByteArray;

                public class Area8Verifier {
                    public static function verifyAll(rootClip:MovieClip):String {
                        // 1. FTE & IME
                        var fd:FontDescription = new FontDescription("Helvetica", FontWeight.BOLD, FontPosture.ITALIC, FontLookup.DEVICE, RenderingMode.CFF, CFFHinting.HORIZONTAL_STEM);
                        var ef:ElementFormat = new ElementFormat(fd, 16, 0x224466, 1.0, TextRotation.AUTO, TextBaseline.ROMAN, TextBaseline.USE_DOMINANT_BASELINE, 0.0, Kerning.ON);
                        var fm:FontMetrics = ef.getFontMetrics();
                        var mirror:EventDispatcher = new EventDispatcher();
                        var te1:TextElement = new TextElement("FTE ", ef, mirror);
                        var te2:TextElement = new TextElement("Engine\t99.5", ef);
                        var elems:Vector.<ContentElement> = new Vector.<ContentElement>();
                        elems.push(te1);
                        elems.push(te2);
                        var grp:GroupElement = new GroupElement(elems, ef);
                        var tabs:Vector.<TabStop> = new Vector.<TabStop>();
                        tabs.push(new TabStop(TabAlignment.DECIMAL, 120, "."));
                        var just:TextJustifier = TextJustifier.getJustifierForLocale("en_US");
                        var tb:TextBlock = new TextBlock(grp, tabs, just, TextRotation.ROTATE_0, TextBaseline.ROMAN, 0, true, fd, 16);
                        var line:TextLine = tb.createTextLine(null, 300, 0.0, true);
                        rootClip.addChild(line);

                        var ime:IIMEClient = new ImeEditorClient();
                        var ranges:Vector.<CompositionAttributeRange> = new Vector.<CompositionAttributeRange>();
                        ranges.push(new CompositionAttributeRange(0, 3, true, false));
                        ime.updateComposition("IME", ranges, 9, 12);
                        ime.confirmComposition("IME!", false);

                        // 2. Stage3D & AGAL
                        var s3d:Stage3D = rootClip.stage.stage3Ds[0];
                        s3d.requestContext3D(Context3DRenderMode.AUTO, Context3DProfile.STANDARD);
                        var ctx:Context3D = s3d.context3D;
                        ctx.configureBackBuffer(32, 32, 0, true);
                        var vb:VertexBuffer3D = ctx.createVertexBuffer(3, 6, Context3DBufferUsage.STATIC_DRAW);
                        var verts:Vector.<Number> = new Vector.<Number>();
                        verts.push(
                            -1, -1, 0, 0, 1, 0,
                             1, -1, 0, 0, 1, 0,
                             0,  1, 0, 0, 1, 0
                        );
                        vb.uploadFromVector(verts, 0, 3);
                        var ib:IndexBuffer3D = ctx.createIndexBuffer(3, Context3DBufferUsage.STATIC_DRAW);
                        var indices:Vector.<uint> = new Vector.<uint>();
                        indices.push(0, 1, 2);
                        ib.uploadFromVector(indices, 0, 3);

                        var vs:ByteArray = new ByteArray();
                        var vsRaw:Array = [
                            0xa0, 0x01, 0x00, 0x00, 0x00, 0xa1, 0x00,
                            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0f, 0x03, 0x00, 0x00, 0x00, 0xe4, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0f, 0x04, 0x01, 0x00, 0x00, 0xe4, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00
                        ];
                        for (var i:int = 0; i < vsRaw.length; i++) vs.writeByte(vsRaw[i]);
                        var fs:ByteArray = new ByteArray();
                        var fsRaw:Array = [
                            0xa0, 0x01, 0x00, 0x00, 0x00, 0xa1, 0x01,
                            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0f, 0x03, 0x00, 0x00, 0x00, 0xe4, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00
                        ];
                        for (var j:int = 0; j < fsRaw.length; j++) fs.writeByte(fsRaw[j]);

                        var prog:Program3D = ctx.createProgram();
                        prog.upload(vs, fs);
                        ctx.clear(0, 0, 0, 1);
                        ctx.setProgram(prog);
                        ctx.setVertexBufferAt(0, vb, 0, Context3DVertexBufferFormat.FLOAT_3);
                        ctx.setVertexBufferAt(1, vb, 3, Context3DVertexBufferFormat.FLOAT_3);
                        ctx.drawTriangles(ib, 0, 1);
                        var outBmp:BitmapData = new BitmapData(32, 32, true, 0);
                        ctx.drawToBitmapData(outBmp);
                        var centerPx:uint = outBmp.getPixel32(16, 16);
                        ctx.present();
                        ctx.dispose(false);

                        // 3. Pixel Bender Shaders + 3D DisplayObject / Transform + LoaderInfo
                        var pbBytes:ByteArray = new ByteArray();
                        pbBytes.writeUTFBytes('{"name":"SolidBlue","op":"solid","parameters":[{"name":"color","type":"float4","defaultValue":[0,0,1,1]}]}');
                        var sh:Shader = new Shader(pbBytes);
                        var shBmp:BitmapData = new BitmapData(4, 4, true, 0);
                        var job:ShaderJob = new ShaderJob(sh, shBmp, 4, 4);
                        job.start(true);

                        var sp:Sprite = new Sprite();
                        sp.graphics.beginShaderFill(sh);
                        sp.graphics.drawRect(0, 0, 10, 10);
                        sp.graphics.endFill();
                        sp.filters = [new ShaderFilter(sh)];
                        rootClip.addChild(sp);
                        sp.z = 50;
                        sp.rotationX = 20;
                        var pt2d:Point = sp.local3DToGlobal(new Vector3D(5, 5, 0));
                        var v3d:Vector3D = sp.globalToLocal3D(pt2d);

                        var li:LoaderInfo = LoaderInfo.getLoaderInfoByDefinition(sp);

                        return [
                            FontDescription.isFontCompatible("_sans", FontWeight.BOLD, FontPosture.ITALIC),
                            Math.round(fm.emBox.height),
                            line.atomCount,
                            line.hasTabs,
                            ime.getTextInRange(0, 13),
                            centerPx.toString(16),
                            shBmp.getPixel32(0, 0).toString(16),
                            sp.transform.matrix3D != null,
                            Math.round(v3d.x),
                            li.actionScriptVersion,
                            ActionScriptVersion.ACTIONSCRIPT3,
                            SWFVersion.FLASH10,
                            ColorCorrection.DEFAULT
                        ].join("|");
                    }
                }
            }
            "#,
        ),
    ]);

    let program = neoflash_script::compile_project(&proj)
        .expect("AS3 Area 8 FTE/Stage3D/Shaders must compile");
    let frame_script = Script {
        language: ScriptLanguage::ActionScript3,
        source: "import app.Area8Verifier;\ntrace(Area8Verifier.verifyAll(this));".into(),
    };
    let compiled_frame = neoflash_script::compile_with_program(&frame_script, &program)
        .expect("AS3 Area 8 frame script must compile");
    assert!(!compiled_frame.is_empty());

    let project_json = serde_json::to_string(&proj).unwrap();
    let bundle_json = serde_json::json!({
        "project": proj,
        "program": {
            "code": program.code,
            "class_names": program.class_names,
        }
    })
    .to_string();
    let script_json = serde_json::to_string(&frame_script).unwrap();
    let code_json = serde_json::to_string(&compiled_frame).unwrap();

    let node_runner = format!(
        r#"
        import assert from 'node:assert/strict';
        import {{ resolve }} from 'node:path';
        import {{ runtimeWasm }} from './tests/rust-runtime-host.mjs';
        import {{ createRuntimeHost }} from './tests/host-integration.mjs';

        const root = resolve('../..');
        const projectJson = {project_json};
        const bundle = {bundle_json};
        const script = {script_json};
        const code = {code_json};

        const player = new runtimeWasm.WebPlayer(JSON.stringify(projectJson));
        const {{ host }} = createRuntimeHost(
            root,
            player,
            [{{ script, code, location: 'Scene 1 / frame 1' }}],
            {{ bundle, project: projectJson }}
        );

        const rootClip = host.proxyFor([]);
        const Verifier = host.program().type('app.Area8Verifier');
        const out = Verifier.verifyAll(rootClip);
        assert.equal(
            out,
            'true|16|15|true|NeoFlash IME!|ff00ff00|ff0000ff|true|5|3|3|10|default'
        );

        host.disposeFamily();
        player.free();
        "#,
    );

    let output = Command::new("node")
        .args(["--input-type=module", "-e", &node_runner])
        .output()
        .expect("node must be available to execute compiled AS3 Area 8 program");
    assert!(
        output.status.success(),
        "Node AS3 Area 8 execution failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn as3_fte_ime_and_stage3d_diagnostics_reject_incomplete_interfaces_and_final_subclasses() {
    let bad_ime = fte3d_project(&[(
        "bad/BrokenIme.as",
        r#"
        package bad {
            import flash.text.ime.IIMEClient;
            public class BrokenIme implements IIMEClient {
                public function get compositionStartIndex():int { return 0; }
            }
        }
        "#,
    )]);
    let err = neoflash_script::compile_project(&bad_ime)
        .err()
        .expect("Incomplete IIMEClient implementation must fail compilation");
    assert!(
        err.iter()
            .any(|d| d.message.contains("IIMEClient") || d.message.contains("confirmComposition")),
        "Expected actionable IIMEClient diagnostic, got: {err:?}"
    );

    for (cls_name, base_qname) in [
        ("BadBlock", "flash.text.engine.TextBlock"),
        ("BadLine", "flash.text.engine.TextLine"),
        ("BadFmt", "flash.text.engine.ElementFormat"),
        ("BadCtx", "flash.display3D.Context3D"),
        ("BadTex", "flash.display3D.textures.Texture"),
        ("BadShader", "flash.display.Shader"),
        ("BadStage3D", "flash.display.Stage3D"),
        ("BadShaderFilter", "flash.filters.ShaderFilter"),
    ] {
        let p = fte3d_project(&[(
            "bad/Sub.as",
            &format!("package bad {{ public class {cls_name} extends {base_qname} {{}} }}"),
        )]);
        assert!(
            neoflash_script::compile_project(&p).is_err(),
            "Extending {base_qname} should be rejected with an actionable diagnostic"
        );
    }
}
