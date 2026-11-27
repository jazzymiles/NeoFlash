use super::*;
use crate::{
    check_project, check_project_warnings, check_warnings_with_path, compile_project,
    compile_with_program,
};
use neoflash_core::{
    Element, ElementKind, FrameScript, InstancePlayback, Keyframe, Layer, LayerProperties,
    ObjectEvent, ObjectScript, Project, PublishSettings, Script, ScriptLanguage, SourceUnit,
    Symbol, SymbolKind, Timeline, Transform,
};
use std::path::PathBuf;
use std::process::Command;

fn node_binary() -> &'static str {
    ["/opt/homebrew/bin/node", "/usr/local/bin/node"]
        .into_iter()
        .find(|path| std::path::Path::new(path).exists())
        .unwrap_or("node")
}

fn load_example(name: &str) -> Project {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = manifest_dir.join("../../examples").join(name);
    let json = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read example {}: {e}", path.display()));
    Project::from_json(&json)
        .unwrap_or_else(|e| panic!("failed to parse example {}: {e}", path.display()))
}

fn execute_js_tetris_in_node(js_project: &Project) {
    let program = check_project(js_project)
        .unwrap_or_else(|errs| panic!("JS Tetris check_project failed: {errs:?}"));
    let root_sym = js_project
        .symbols
        .iter()
        .find(|s| s.id == js_project.root)
        .expect("root symbol");
    let frame0 = root_sym
        .timeline
        .scripts
        .iter()
        .find(|fs| fs.frame == 0)
        .expect("frame 0 script");

    assert!(
        !frame0.script.source.contains("__nfFamilyPropertyValue"),
        "emitted JS must never contain internal AS VM intrinsics"
    );

    let compiled_handler = compile_with_program(&frame0.script, &program)
        .unwrap_or_else(|errs| panic!("compile_with_program failed: {errs:?}"));

    let harness = format!(
        r#"
globalThis.trace = () => {{}};
globalThis.performance = globalThis.performance || {{ now: () => 0 }};

function makeDisplayMock(name) {{
    return {{
        name,
        x: 0,
        y: 0,
        scaleX: 1,
        scaleY: 1,
        alpha: 1,
        rotation: 0,
        visible: true,
        text: "",
        children: [],
        listeners: {{}},
        graphics: {{
            ops: 0,
            clear() {{ this.ops++; }},
            lineStyle() {{ this.ops++; }},
            beginFill() {{ this.ops++; }},
            moveTo() {{ this.ops++; }},
            lineTo() {{ this.ops++; }},
            curveTo() {{ this.ops++; }},
            drawRect() {{ this.ops++; }},
            drawCircle() {{ this.ops++; }},
            endFill() {{ this.ops++; }}
        }},
        addChild(c) {{ this.children.push(c); return c; }},
        removeChild(c) {{ return c; }},
        addEventListener(type, fn) {{ (this.listeners[type] = this.listeners[type] || []).push(fn); }},
        removeEventListener() {{}},
        stop() {{ this.stopped = true; }},
        play() {{ this.stopped = false; }},
        gotoAndPlay() {{}},
        gotoAndStop() {{}}
    }};
}}

const clip = makeDisplayMock("root");
clip.stage = {{
    stageWidth: 640,
    stageHeight: 480,
    focus: null,
    listeners: {{}},
    addEventListener(type, fn) {{ (this.listeners[type] = this.listeners[type] || []).push(fn); }},
    removeEventListener() {{}}
}};
for (const tf of ["scoreText", "linesText", "levelText", "statusText"]) {{
    clip[tf] = makeDisplayMock(tf);
}}
for (let i = 0; i < 4; i++) {{
    clip["act" + i] = makeDisplayMock("act" + i);
    clip["next" + i] = makeDisplayMock("next" + i);
}}

const handlerFn = new Function({handler_json});
handlerFn.call(clip);

if (typeof clip.resetGame !== "function") throw new Error("missing clip.resetGame");
if (typeof clip.stepGravity !== "function") throw new Error("missing clip.stepGravity");
if (typeof clip.moveHorizontal !== "function") throw new Error("missing clip.moveHorizontal");
if (typeof clip.rotatePiece !== "function") throw new Error("missing clip.rotatePiece");
if (typeof clip.renderBoard !== "function") throw new Error("missing clip.renderBoard");

clip.resetGame();
const movedLeft = clip.moveHorizontal(-1);
const movedRight = clip.moveHorizontal(1);
clip.rotatePiece();
clip.stepGravity();
clip.renderBoard();

if (!movedLeft || !movedRight) {{
    throw new Error("expected horizontal moves to succeed after resetGame()");
}}
if (!String(clip.scoreText.text).startsWith("SCORE:")) {{
    throw new Error("unexpected HUD score text: " + clip.scoreText.text);
}}
console.log("NODE_TETRIS_OK:" + clip.scoreText.text + ":" + clip.statusText.text);
"#,
        handler_json = serde_json::to_string(&compiled_handler).unwrap()
    );

    let output = Command::new(node_binary())
        .arg("-e")
        .arg(&harness)
        .output()
        .expect("failed to launch node");
    assert!(
        output.status.success(),
        "Node execution of converted Tetris JS failed!\nstdout: {}\nstderr: {}\nJS source:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
        frame0.script.source
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("NODE_TETRIS_OK:SCORE: 0:PLAYING"),
        "unexpected Node stdout: {stdout}"
    );
}

#[test]
fn convert_tetris_as2_to_as3_and_js_compiles_and_executes() {
    let tetris_as2 = load_example("games/tetris-as2.nflash");

    // 1. AS2 -> AS3
    let (as3_project, as3_report) =
        convert_project_scripts(&tetris_as2, ConversionTarget::ActionScript3)
            .expect("tetris-as2 -> AS3 conversion should succeed");
    assert_eq!(
        as3_project.script_language,
        Some(ScriptLanguage::ActionScript3)
    );
    assert_eq!(as3_report.source_language, ScriptLanguage::ActionScript2);
    assert_eq!(as3_report.target_language, ScriptLanguage::ActionScript3);
    assert!(as3_report.converted_frame_scripts >= 1);
    compile_project(&as3_project).unwrap_or_else(|errs| {
        panic!("converted tetris-as2 -> AS3 compile_project failed: {errs:?}")
    });
    check_project(&as3_project).unwrap_or_else(|errs| {
        panic!("converted tetris-as2 -> AS3 check_project failed: {errs:?}")
    });

    // 2. AS2 -> JS + live Node execution
    let (js_project, js_report) =
        convert_project_scripts(&tetris_as2, ConversionTarget::JavaScript)
            .expect("tetris-as2 -> JS conversion should succeed");
    assert_eq!(js_project.script_language, Some(ScriptLanguage::JavaScript));
    assert_eq!(js_report.target_language, ScriptLanguage::JavaScript);
    assert!(js_report.converted_frame_scripts >= 1);
    compile_project(&js_project).unwrap_or_else(|errs| {
        panic!("converted tetris-as2 -> JS compile_project failed: {errs:?}")
    });
    execute_js_tetris_in_node(&js_project);
}

#[test]
fn convert_tetris_as3_to_as2_and_js_compiles_and_executes() {
    let tetris_as3 = load_example("games/tetris-as3.nflash");

    // 1. AS3 -> AS2
    let (as2_project, as2_report) =
        convert_project_scripts(&tetris_as3, ConversionTarget::ActionScript2)
            .expect("tetris-as3 -> AS2 conversion should succeed");
    assert_eq!(
        as2_project.script_language,
        Some(ScriptLanguage::ActionScript2)
    );
    assert_eq!(as2_report.source_language, ScriptLanguage::ActionScript3);
    assert_eq!(as2_report.target_language, ScriptLanguage::ActionScript2);
    assert!(as2_report.converted_frame_scripts >= 1);
    compile_project(&as2_project).unwrap_or_else(|errs| {
        panic!("converted tetris-as3 -> AS2 compile_project failed: {errs:?}")
    });
    check_project(&as2_project).unwrap_or_else(|errs| {
        panic!("converted tetris-as3 -> AS2 check_project failed: {errs:?}")
    });

    // 2. AS3 -> JS + live Node execution
    let (js_project, js_report) =
        convert_project_scripts(&tetris_as3, ConversionTarget::JavaScript)
            .expect("tetris-as3 -> JS conversion should succeed");
    assert_eq!(js_project.script_language, Some(ScriptLanguage::JavaScript));
    assert_eq!(js_report.target_language, ScriptLanguage::JavaScript);
    assert!(js_report.converted_frame_scripts >= 1);
    compile_project(&js_project).unwrap_or_else(|errs| {
        panic!("converted tetris-as3 -> JS compile_project failed: {errs:?}")
    });
    execute_js_tetris_in_node(&js_project);
}

#[test]
fn convert_arkanoid_as2_key_is_down_shim_to_as3_and_js() {
    let arkanoid = load_example("games/arkanoid-as2.nflash");

    let (as3_proj, as3_rep) = convert_project_scripts(&arkanoid, ConversionTarget::ActionScript3)
        .expect("arkanoid -> AS3");
    assert!(
        as3_rep
            .issues
            .iter()
            .any(|i| i.kind == ConversionIssueKind::ShimInjected),
        "expected Key.isDown ShimInjected issue in report"
    );
    check_project(&as3_proj)
        .unwrap_or_else(|errs| panic!("arkanoid -> AS3 check_project failed: {errs:?}"));

    let (js_proj, js_rep) =
        convert_project_scripts(&arkanoid, ConversionTarget::JavaScript).expect("arkanoid -> JS");
    assert!(
        js_rep
            .issues
            .iter()
            .any(|i| i.kind == ConversionIssueKind::ShimInjected)
    );
    check_project(&js_proj)
        .unwrap_or_else(|errs| panic!("arkanoid -> JS check_project failed: {errs:?}"));
}

#[test]
fn rejects_js_or_ts_to_actionscript_cleanly() {
    let mut proj = Project::blank();
    proj.script_language = Some(ScriptLanguage::JavaScript);
    let layer_id = proj.symbols[0].timeline.layers[0].id.clone();
    proj.symbols[0].timeline.scripts.push(FrameScript {
        layer: Some(layer_id),
        frame: 0,
        script: Script {
            language: ScriptLanguage::JavaScript,
            source: "this.stop();".into(),
        },
    });
    let err_as3 = convert_project_scripts(&proj, ConversionTarget::ActionScript3)
        .expect_err("JS -> AS3 must be rejected");
    assert!(err_as3.contains("not supported"));
    let err_as2 = convert_project_scripts(&proj, ConversionTarget::ActionScript2)
        .expect_err("JS -> AS2 must be rejected");
    assert!(err_as2.contains("not supported"));
}

#[test]
fn scale_alpha_drawing_attach_movie_and_object_script_lifting() {
    let mut proj = Project::blank();
    proj.script_language = Some(ScriptLanguage::ActionScript2);

    let btn_sym_id = "sym-btn-play".to_owned();
    proj.symbols.push(Symbol {
        id: btn_sym_id.clone(),
        name: "PlayBtn".into(),
        folder: String::new(),
        kind: SymbolKind::Button,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![Layer {
                id: "layer-btn".into(),
                name: "Layer 1".into(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    ..Default::default()
                }],
                properties: LayerProperties::default(),
            }],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    });

    let enemy_sym_id = "sym-enemy".to_owned();
    proj.symbols.push(Symbol {
        id: enemy_sym_id.clone(),
        name: "EnemySymbol".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![Layer {
                id: "layer-enemy".into(),
                name: "Layer 1".into(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    ..Default::default()
                }],
                properties: LayerProperties::default(),
            }],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: Some("enemy_link".into()),
    });

    let root_layer_id = proj.symbols[0].timeline.layers[0].id.clone();
    proj.symbols[0].timeline.frame_count = 10;
    proj.symbols[0].timeline.layers[0].keyframes = vec![
        Keyframe {
            frame: 0,
            elements: vec![
                Element {
                    id: "el-unnamed-btn".into(),
                    name: String::new(),
                    kind: ElementKind::Instance {
                        symbol_id: btn_sym_id.clone(),
                        playback: InstancePlayback::Loop,
                    },
                    transform: Transform::default(),
                    scripts: vec![ObjectScript {
                        event: ObjectEvent::Click,
                        script: Script {
                            language: ScriptLanguage::ActionScript2,
                            source: "on (release) {\n    this._alpha = 50;\n    gotoAndStop(2);\n}"
                                .into(),
                        },
                    }],
                    ..Element::default()
                },
                Element {
                    id: "el-unnamed-clip".into(),
                    name: String::new(),
                    kind: ElementKind::Instance {
                        symbol_id: enemy_sym_id,
                        playback: InstancePlayback::Loop,
                    },
                    transform: Transform::default(),
                    scripts: vec![ObjectScript {
                        event: ObjectEvent::EnterFrame,
                        script: Script {
                            language: ScriptLanguage::ActionScript2,
                            source: "onClipEvent (enterFrame) {\n    _x += 2;\n    _alpha -= 5;\n}"
                                .into(),
                        },
                    }],
                    ..Element::default()
                },
            ],
            ..Default::default()
        },
        Keyframe {
            frame: 5,
            elements: vec![Element {
                id: "el-unnamed-btn".into(),
                name: String::new(),
                kind: ElementKind::Instance {
                    symbol_id: btn_sym_id,
                    playback: InstancePlayback::Loop,
                },
                transform: Transform::default(),
                ..Element::default()
            }],
            ..Default::default()
        },
    ];

    proj.symbols[0].timeline.scripts.push(FrameScript {
        layer: Some(root_layer_id),
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript2,
            source: r#"
var fadeStep:Number = 10;
var foe = this.attachMovie("enemy_link", "foe1", 9999, {_x: 40, _alpha: 25, _xscale: 50});
foe._alpha += fadeStep;
var curA:Number = foe._alpha;
var canvas:MovieClip = this.createEmptyMovieClip("canvas", 5000);
canvas.lineStyle(2, 0x00FF00, 50);
canvas.beginFill(0xFF0000, 100);
canvas.moveTo(0, 0);
canvas.lineTo(20, 20);
canvas.endFill();
"#
            .into(),
        },
    });

    let (as3_proj, rep) =
        convert_project_scripts(&proj, ConversionTarget::ActionScript3).expect("AS2 -> AS3");
    assert_eq!(rep.auto_named_instances, 2);
    assert_eq!(rep.converted_object_scripts, 2);

    let kf0_btn_name = &as3_proj.symbols[0].timeline.layers[0].keyframes[0].elements[0].name;
    let kf5_btn_name = &as3_proj.symbols[0].timeline.layers[0].keyframes[1].elements[0].name;
    let kf0_clip_name = &as3_proj.symbols[0].timeline.layers[0].keyframes[0].elements[1].name;
    assert_eq!(kf0_btn_name, "btn_1");
    assert_eq!(kf5_btn_name, "btn_1");
    assert_eq!(kf0_clip_name, "clip_1");

    let f0_src = &as3_proj.symbols[0].timeline.scripts[0].script.source;
    assert!(
        f0_src.contains("foe.alpha = 0.25;"),
        "expected folded 25 -> 0.25 in:\n{f0_src}"
    );
    assert!(
        f0_src.contains("foe.scaleX = 0.5;"),
        "expected folded 50 -> 0.5 in:\n{f0_src}"
    );
    assert!(
        f0_src.contains("foe.alpha += (fadeStep) / 100;"),
        "expected compound percentage assignment division in:\n{f0_src}"
    );
    assert!(
        f0_src.contains("(foe.alpha * 100)"),
        "expected percentage read multiplication in:\n{f0_src}"
    );
    assert!(
        f0_src.contains("canvas.graphics.lineStyle(2, 0x00FF00, 0.5);"),
        "expected graphics.lineStyle alpha scaling in:\n{f0_src}"
    );
    assert!(
        f0_src.contains("this.alpha = 0.5;"),
        "Button handler must keep parent `this`:\n{f0_src}"
    );
    assert!(
        f0_src.contains("this.clip_1.x += 2;"),
        "Clip handler must rewrite to `this.clip_1`:\n{f0_src}"
    );

    check_project(&as3_proj).unwrap_or_else(|errs| {
        panic!("converted AS3 project failed check_project: {errs:?}\n{f0_src}")
    });

    let (js_proj, js_rep) =
        convert_project_scripts(&proj, ConversionTarget::JavaScript).expect("AS2 -> JS");
    assert_eq!(js_rep.auto_named_instances, 2);
    let js_f0 = &js_proj.symbols[0].timeline.scripts[0].script.source;
    assert!(
        js_f0.contains("foe.alpha += (fadeStep) / 100;"),
        "expected (fadeStep) / 100 in JS:\n{js_f0}"
    );
    check_project(&js_proj)
        .unwrap_or_else(|errs| panic!("converted JS project failed check_project: {errs:?}"));
}

#[test]
fn layered_conversion_binds_each_display_object_once_and_keeps_authored_names() {
    let mut project = Project::blank();
    project.script_language = Some(ScriptLanguage::ActionScript2);
    let timeline = &mut project.symbols[0].timeline;
    timeline.frame_count = 2;
    let mut second = timeline.layers[0].clone();
    second.id = "actions-2".into();
    timeline.layers.push(second);
    let mut child = Element {
        id: "play-button".into(),
        scripts: vec![ObjectScript {
            event: ObjectEvent::Click,
            script: Script {
                language: ScriptLanguage::ActionScript2,
                source: "on(release) { trace(123); } onClipEvent(mouseMove) { trace(456); } onClipEvent(unload) { trace(678); } onClipEvent(data) { trace(789); }".into(),
            },
        }],
        ..Element::default()
    };
    timeline.layers[0].keyframes[0].elements.push(child.clone());
    child.name = "Play - 2".into();
    child.scripts.clear();
    timeline.layers[0].keyframes.push(Keyframe {
        frame: 1,
        elements: vec![child],
        ..Default::default()
    });
    for layer in &timeline.layers {
        timeline.scripts.push(FrameScript {
            layer: Some(layer.id.clone()),
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript2,
                source: "trace(42);".into(),
            },
        });
    }
    project.upgrade_format_for_features();
    project.validate().unwrap();
    for target in [
        ConversionTarget::ActionScript3,
        ConversionTarget::JavaScript,
    ] {
        let (converted, report) = convert_project_scripts(&project, target).unwrap();
        assert_eq!(report.auto_named_instances, 1);
        assert_eq!(report.converted_object_scripts, 1);
        let timeline = &converted.symbols[0].timeline;
        assert!(
            timeline.layers[0]
                .keyframes
                .iter()
                .all(|key| key.elements[0].name == "Play - 2")
        );
        let scripts = timeline
            .scripts
            .iter()
            .map(|frame| frame.script.source.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(scripts.matches("123").count(), 1, "{scripts}");
        assert_eq!(scripts.matches("42").count(), 2, "{scripts}");
        assert!(scripts.contains(r#"this["Play - 2"]"#), "{scripts}");
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.kind == ConversionIssueKind::ManualAttention
                    && issue.message.contains("data")
                    && issue.location.contains("frame")),
            "{report:?}"
        );
        assert!(
            scripts.contains("789"),
            "manual data handler must remain editable"
        );
        let program =
            check_project(&converted).unwrap_or_else(|e| panic!("{target:?}: {e:?}\n{scripts}"));
        let handlers: Vec<_> = timeline
            .scripts
            .iter()
            .map(|f| compile_with_program(&f.script, &program).unwrap())
            .collect();
        if target == ConversionTarget::JavaScript {
            let runner = r#"
                const assert = require('node:assert/strict');
                const listeners = {};
                const traces = [];
                globalThis.trace = value => traces.push(value);
                const target = name => ({ addEventListener(event, callback) {
                    if (listeners[name + event]) throw Error('duplicate handler ' + event);
                    listeners[name + event] = callback;
                } });
                const clip = { "Play - 2": target("child"), stage: target("stage"), trace };
                for (const source of JSON.parse(process.argv[1])) new Function(source).call(clip);
                assert.deepEqual(traces,[42,42]);
                listeners.childclick({});
                listeners.stagemouseMove({});
                listeners.childremovedFromStage({});
                assert.deepEqual(traces,[42,42,123,456,678]);
            "#;
            let result = Command::new(node_binary())
                .args(["-e", runner, &serde_json::to_string(&handlers).unwrap()])
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}\n{scripts}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
    }
}

#[test]
fn mixed_actionscript_and_web_conversion_handles_object_and_frame_languages() {
    let mut project = Project::blank();
    project.script_language = Some(ScriptLanguage::ActionScript2);
    let timeline = &mut project.symbols[0].timeline;
    let typed = ObjectScript {
        event: ObjectEvent::Load,
        script: Script {
            language: ScriptLanguage::TypeScript,
            source: "const x: number = 4; this.answer = x;".into(),
        },
    };
    let converted = ObjectScript {
        event: ObjectEvent::Click,
        script: Script {
            language: ScriptLanguage::ActionScript2,
            source: "trace(246);".into(),
        },
    };
    timeline.layers[0].keyframes[0].elements.push(Element {
        id: "mixed-button".into(),
        name: "button".into(),
        scripts: vec![typed, converted],
        ..Element::default()
    });
    let mut copy = project.clone();
    copy.symbols[0].timeline.scripts.clear();
    for target in [
        ConversionTarget::ActionScript3,
        ConversionTarget::ActionScript2,
    ] {
        let error =
            convert_project_scripts(&copy, target).expect_err("unsupported web object language");
        assert!(
            error.contains("TypeScript") && error.contains("not supported"),
            "{error}"
        );
    }
    project.symbols[0].timeline.scripts.push(FrameScript {
        frame: 0,
        layer: None,
        script: Script {
            language: ScriptLanguage::JavaScript,
            source: "this.existing = 'web';".into(),
        },
    });
    let (js, report) = convert_project_scripts(&project, ConversionTarget::JavaScript).unwrap();
    let timeline = &js.symbols[0].timeline;
    assert_eq!(report.converted_object_scripts, 2);
    let frame = &timeline.scripts[0].script.source;
    assert!(
        frame.contains("this.existing = 'web';") && frame.contains("246"),
        "{frame}"
    );
    let objects = &timeline.layers[0].keyframes[0].elements[0].scripts;
    assert_eq!(objects.len(), 1);
    assert_eq!(objects[0].script.language, ScriptLanguage::JavaScript);
    assert!(
        objects[0].script.source.contains("this.answer")
            && !objects[0].script.source.contains(": number")
    );
    check_project(&js).unwrap_or_else(|errors| panic!("{errors:?}\n{frame}"));
}

#[test]
fn expands_include_units_cleanly_across_as3_as2_and_js() {
    let mut proj = Project::blank();
    proj.script_language = Some(ScriptLanguage::ActionScript2);
    proj.sources.push(SourceUnit {
        id: "inc-constants".into(),
        path: "shared/constants.as".into(),
        language: ScriptLanguage::ActionScript2,
        source: "var GRAVITY:Number = 9.8;\nthis._alpha = 50;\n".into(),
    });
    proj.sources.push(SourceUnit {
        id: "unused-source".into(),
        path: "shared/draft.as".into(),
        language: ScriptLanguage::ActionScript2,
        source: "// Authored for a future frame.\nvar note = 'draft';".into(),
    });
    let lid = proj.symbols[0].timeline.layers[0].id.clone();
    proj.symbols[0].timeline.scripts.push(FrameScript {
        layer: Some(lid),
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript2,
            source: "#include \"shared/constants.as\"\ntrace(GRAVITY);\n".into(),
        },
    });

    let (as3_proj, _) =
        convert_project_scripts(&proj, ConversionTarget::ActionScript3).expect("include -> AS3");
    assert_eq!(
        as3_proj.sources, proj.sources,
        "include and unused source remain editable"
    );
    assert!(
        as3_proj.symbols[0].timeline.scripts[0]
            .script
            .source
            .contains("GRAVITY")
    );
    assert!(
        as3_proj.symbols[0].timeline.scripts[0]
            .script
            .source
            .contains("this.alpha = 0.5;")
    );
    check_project(&as3_proj).expect("expanded AS3 check_project");

    let (js_proj, _) =
        convert_project_scripts(&proj, ConversionTarget::JavaScript).expect("include -> JS");
    assert_eq!(js_proj.sources, proj.sources);
    assert!(
        js_proj.symbols[0].timeline.scripts[0]
            .script
            .source
            .contains("this.alpha = 0.5;")
    );
    check_project(&js_proj).expect("expanded JS check_project");
}

#[test]
fn as3_to_as2_lowers_draw_rect_circle_vector_document_class_and_flags_e4x() {
    let mut proj = Project::blank();
    proj.script_language = Some(ScriptLanguage::ActionScript3);
    proj.document_class = Some("app.MainDoc".into());
    proj.sources.push(SourceUnit {
        id: "src-doc".into(),
        path: "app/MainDoc.as".into(),
        language: ScriptLanguage::ActionScript3,
        source: r#"package app {
    import flash.display.MovieClip;
    public class MainDoc extends MovieClip {
        public var scores:Vector.<int> = new Vector.<int>();
        public function MainDoc() {
            super();
        }
    }
}
"#
        .into(),
    });
    let layer_id = proj.symbols[0].timeline.layers[0].id.clone();
    proj.symbols[0].timeline.scripts.push(FrameScript {
        layer: Some(layer_id),
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript3,
            source: r#"import flash.display.Sprite;
var box:Sprite = new Sprite();
addChild(box);
box.alpha = 0.5;
box.alpha += 0.1;
box.graphics.beginFill(0x336699, 0.25);
box.graphics.drawRect(10, 20, 100, 50);
box.graphics.drawCircle(50, 50, 20);
box.graphics.endFill();
var raw:int = int(7.9);
var mask:uint = uint(15);
"#
            .into(),
        },
    });

    let (as2_proj, rep) =
        convert_project_scripts(&proj, ConversionTarget::ActionScript2).expect("AS3 -> AS2");
    assert_eq!(as2_proj.document_class.as_deref(), Some("app.MainDoc"));
    assert!(as2_proj.symbols[0].class_name.is_none());
    assert!(
        rep.issues
            .iter()
            .any(|i| i.kind == ConversionIssueKind::Approximated && i.message.contains("Vector")),
        "expected Approximated issue for Vector.<int>"
    );

    let f0 = &as2_proj.symbols[0].timeline.scripts[0].script.source;
    assert!(
        !f0.contains("_documentApp"),
        "unused _documentApp alias must not be injected on Frame 0:\n{f0}"
    );
    assert!(
        f0.contains("createEmptyMovieClip"),
        "expected Sprite -> createEmptyMovieClip:\n{f0}"
    );
    assert!(f0.contains("box._alpha = 50;"), "expected 0.5 -> 50:\n{f0}");
    assert!(
        f0.contains("box.beginFill(0x336699, 25);"),
        "expected 0.25 -> 25:\n{f0}"
    );
    assert!(
        f0.contains("curveTo"),
        "expected drawCircle -> 8-segment curveTo:\n{f0}"
    );
    assert!(f0.contains("| 0"), "expected int(...) -> | 0:\n{f0}");
    assert!(f0.contains(">>> 0"), "expected uint(...) -> >>> 0:\n{f0}");

    check_project(&as2_proj)
        .unwrap_or_else(|errs| panic!("AS3 -> AS2 check_project failed: {errs:?}\nFrame 0:\n{f0}"));

    let mut e4x_proj = Project::blank();
    e4x_proj.script_language = Some(ScriptLanguage::ActionScript3);
    let lid = e4x_proj.symbols[0].timeline.layers[0].id.clone();
    e4x_proj.symbols[0].timeline.scripts.push(FrameScript {
        layer: Some(lid),
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript3,
            source:
                "var buf:ByteArray = new ByteArray();\nvar items = xmlDoc..item;\nvar gpu = stage.stage3Ds;\n"
                    .into(),
        },
    });
    let (e4x_as2, e4x_rep) = convert_project_scripts(&e4x_proj, ConversionTarget::ActionScript2)
        .expect("E4X AS3 -> AS2");
    let e4x_src = &e4x_as2.symbols[0].timeline.scripts[0].script.source;
    assert!(
        e4x_src.contains("FIXME [NeoFlash Convert]"),
        "expected FIXME comment in:\n{e4x_src}"
    );
    assert!(
        e4x_rep
            .issues
            .iter()
            .filter(|i| i.kind == ConversionIssueKind::ManualAttention && i.line >= 1)
            .count()
            >= 3,
        "expected ManualAttention issues with line/column for ByteArray, E4X, and Stage3D: {:?}",
        e4x_rep.issues
    );
}

#[test]
fn as3_classes_to_js_modules_bind_methods_and_map_dictionary() {
    let mut proj = Project::blank();
    proj.script_language = Some(ScriptLanguage::ActionScript3);
    proj.sources.push(SourceUnit {
        id: "src-helper".into(),
        path: "game/ScoreModel.as".into(),
        language: ScriptLanguage::ActionScript3,
        source: r#"package game {
    import flash.utils.Dictionary;
    public class ScoreModel {
        public var cache:Dictionary = new Dictionary(true);
        public var lookup:Dictionary = new Dictionary();
        public function record(k:String, v:int):int {
            lookup[k] = v;
            return int(lookup[k]) | 0;
        }
    }
}
"#
        .into(),
    });
    proj.sources.push(SourceUnit {
        id: "src-controller".into(),
        path: "game/HudController.as".into(),
        language: ScriptLanguage::ActionScript3,
        source: r#"package game {
    import flash.display.MovieClip;
    import flash.events.Event;
    public class HudController extends MovieClip {
        public var model:ScoreModel = new ScoreModel();
        public function HudController() {
            super();
            addEventListener(Event.ENTER_FRAME, onFrame);
        }
        public function onFrame(e:Event):void {
            model.record("pts", 10);
        }
    }
}
"#
        .into(),
    });

    let (js_proj, rep) =
        convert_project_scripts(&proj, ConversionTarget::JavaScript).expect("AS3 classes -> JS");
    assert_eq!(rep.converted_source_units, 2);
    let score_js = &js_proj.sources[0].source;
    let hud_js = &js_proj.sources[1].source;
    assert_eq!(js_proj.sources[0].path, "game/ScoreModel.js");
    assert_eq!(js_proj.sources[1].path, "game/HudController.js");
    assert!(
        score_js.contains("new WeakMap()"),
        "expected Dictionary(true) -> WeakMap:\n{score_js}"
    );
    assert!(
        score_js.contains("new Map()"),
        "expected Dictionary() -> Map:\n{score_js}"
    );
    assert!(
        hud_js.contains("import { ScoreModel } from \"./ScoreModel.js\";"),
        "expected relative ES import:\n{hud_js}"
    );
    assert!(
        hud_js.contains("this.onFrame.bind(this)"),
        "expected .bind(this) on class method listener:\n{hud_js}"
    );

    check_project(&js_proj)
        .unwrap_or_else(|errs| panic!("AS3 classes -> JS check_project failed: {errs:?}"));
}

#[test]
fn swift_codegen_extracts_swift_host_promotes_numerics_escapes_keywords_and_uses_weak_self() {
    let mut scene = Project::new_xcode_swift_scene(800, 600, 60, ScriptLanguage::ActionScript3);
    check_project(&scene).expect("new_xcode_swift_scene AS3 must pass check_project");
    let preset_warnings = check_project_warnings(&scene);
    assert!(
        !preset_warnings
            .iter()
            .any(|w| w.code == "swift-host-contract" || w.code == "xcode-swift-unsupported"),
        "fresh Xcode Swift preset must have zero Swift contract/target warnings: {preset_warnings:?}"
    );

    scene.symbols[0].timeline.scripts[0].script.source.push_str(
        r#"
var guard:int = 7;
var repeat:int = 2;
var ratio:Number = guard / repeat;
var truncated:int = int(guard / repeat);
var mixed:Number = ratio + guard;
var masked:int = (guard << 2) & 15;
SwiftHost.hapticImpact("medium");
"#,
    );

    let compiled = compile_project_scripts_to_swift(&scene);
    assert_eq!(compiled.host_methods.len(), 2);
    assert_eq!(compiled.host_methods[0].name, "notifySceneReady");
    assert_eq!(
        compiled.host_methods[0].params,
        vec![
            SwiftProxyParam {
                name: "width".into(),
                swift_type: "Double".into(),
                as_type: "Number".into(),
            },
            SwiftProxyParam {
                name: "height".into(),
                swift_type: "Double".into(),
                as_type: "Number".into(),
            },
        ]
    );
    assert_eq!(compiled.host_methods[0].return_swift_type, None);
    assert_eq!(compiled.host_methods[1].name, "hapticImpact");
    assert_eq!(compiled.host_methods[1].params[0].swift_type, "String");
    assert!(
        !compiled.source_classes_swift.contains("class SwiftHost"),
        "SwiftHost mock class must be omitted from source_classes_swift"
    );

    let root_swift = &compiled.symbols[0];
    assert_eq!(root_swift.swift_class_name, "MainTimeline");
    assert!(root_swift.properties_swift.contains("`guard`: Int"));
    assert!(root_swift.properties_swift.contains("`repeat`: Int"));
    let f0_swift = root_swift
        .frame_methods_swift
        .get(&0)
        .expect("frame_0 swift");
    assert!(
        f0_swift.contains("self.stage?.host?.notifySceneReady("),
        "expected SwiftHost call lowered to self.stage?.host?.notifySceneReady in:\n{f0_swift}"
    );
    assert!(
        f0_swift.contains("self.stage?.host?.hapticImpact(style: \"medium\")"),
        "expected SwiftHost.hapticImpact lowered in:\n{f0_swift}"
    );
    assert!(
        f0_swift.contains("[weak self]"),
        "expected ARC-safe [weak self] on resize listener closure in:\n{f0_swift}"
    );
    assert!(
        f0_swift.contains("Double(self.`guard`) / Double(self.`repeat`)"),
        "expected float division promotion in:\n{f0_swift}"
    );
    assert!(
        f0_swift.contains("Int(Double(self.`guard`) / Double(self.`repeat`))"),
        "expected Int(Double(a) / Double(b)) truncation in:\n{f0_swift}"
    );

    let scene_as2 = Project::new_xcode_swift_scene(800, 600, 60, ScriptLanguage::ActionScript2);
    check_project(&scene_as2).expect("new_xcode_swift_scene AS2 must pass check_project");
    let compiled_as2 = compile_project_scripts_to_swift(&scene_as2);
    assert_eq!(compiled_as2.host_methods.len(), 2);
}

#[test]
fn warnings_guardrails_validate_swift_host_contract_and_xcode_target_constructs() {
    let bad_host = Script {
        language: ScriptLanguage::ActionScript3,
        source: r#"package {
    import flash.display.MovieClip;
    public class SwiftHost {
        public function instanceBad():void {}
        public static function untypedBad(x, clip:MovieClip):* {
            return x;
        }
    }
}
"#
        .into(),
    };
    let direct_warnings = check_warnings_with_path(&bad_host, Some("SwiftHost.as"));
    let contract_ws: Vec<_> = direct_warnings
        .iter()
        .filter(|w| w.code == "swift-host-contract")
        .collect();
    assert!(
        contract_ws.len() >= 4,
        "expected warnings for non-static method, untyped param, MovieClip param, and '*' return; got: {contract_ws:?}"
    );
    assert!(
        contract_ws
            .iter()
            .all(|w| w.line >= 4 && w.span.1 > w.span.0)
    );

    let mut proj = Project::new_xcode_swift_scene(640, 480, 30, ScriptLanguage::ActionScript3);
    proj.set_publish_settings(&PublishSettings {
        xcode_swift_target: true,
        ..Default::default()
    });
    proj.symbols[0].timeline.scripts[0]
        .script
        .source
        .push_str(
            "\nvar rawBytes:ByteArray = new ByteArray();\nvar nodes = doc..item;\nvar s3d = stage.stage3Ds;\neval(\"foo\");\n",
        );

    let proj_warnings = check_project_warnings(&proj);
    let unsupported_ws: Vec<_> = proj_warnings
        .iter()
        .filter(|w| w.code == "xcode-swift-unsupported")
        .collect();
    assert!(
        unsupported_ws
            .iter()
            .any(|w| w.message.contains("ByteArray")),
        "missing ByteArray Xcode warning: {unsupported_ws:?}"
    );
    assert!(
        unsupported_ws.iter().any(|w| w.message.contains("E4X")),
        "missing E4X Xcode warning: {unsupported_ws:?}"
    );
    assert!(
        unsupported_ws
            .iter()
            .any(|w| w.message.contains("stage3Ds")),
        "missing Stage3D Xcode warning: {unsupported_ws:?}"
    );
    assert!(
        unsupported_ws.iter().any(|w| w.message.contains("eval()")),
        "missing eval() Xcode warning: {unsupported_ws:?}"
    );
}
