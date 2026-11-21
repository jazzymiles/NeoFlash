use image::{ExtendedColorType, ImageEncoder};
use neoflash_core::{
    Color, DisplayMutation, DynamicKind, Element, ElementKind, FillRule, FrameScript, FrameSound,
    GradientKind, GradientSpec, GraphicsOp, InstancePlayback, Keyframe, Layer, LayerProperties,
    LineCaps, LineJoints, LineScaleMode, ObjectEvent, ObjectScript, PathCommand, Player, Project,
    ResourceUnit, Script, ScriptLanguage, SoundCommand, SoundSync, SourceUnit, Symbol, SymbolKind,
    Timeline, Transform,
};
use neoflash_export::{
    AudioExportPolicy, ExportError, FontExportPolicy, HtmlPublishConfig, PublishOptions,
    PublishProfile, SizeLevel, export_desktop_app, export_project_with_config,
    export_project_with_options, export_project_with_profile, export_shared_runtime,
};
use neoflash_import::{SwcConflictPolicy, import_swc_bytes, link_swc_into_project};
use neoflash_script::{Diagnostic, check_project, check_project_warnings};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use zip::write::SimpleFileOptions;

const EXAMPLE_SLUGS: &[&str] = &[
    "00-child-module",
    "01-math-geometry",
    "02-oop-xml-collections",
    "03-movieclips-nested-timelines",
    "04-events-stage-align-drag",
    "05-drawing-api-filters-bitmapdata",
    "06-dynamic-input-html-text",
    "07-sound-video-loadpublish",
    "08-interactive-arcade-miniapp",
];

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let dir = std::env::temp_dir().join(format!(
            "neoflash-pub-gate-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).expect("create temp dir");
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates dir")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn runtime_dir() -> PathBuf {
    let dir = repo_root().join("target").join("web-runtime");
    assert!(
        dir.join("neoflash_web.js").is_file() && dir.join("neoflash_web_bg.wasm").is_file(),
        "real WASM web runtime is required at {} (run ./scripts/build-web.sh first)",
        dir.display()
    );
    dir
}

fn tf(x: f64, y: f64) -> Transform {
    Transform {
        x,
        y,
        ..Transform::default()
    }
}

fn rect_el(id: &str, name: &str, x: f64, y: f64, w: f64, h: f64, fill: Color) -> Element {
    Element {
        id: id.into(),
        name: name.into(),
        transform: tf(x, y),
        kind: ElementKind::Path {
            commands: vec![
                PathCommand::MoveTo { x: 0.0, y: 0.0 },
                PathCommand::LineTo { x: w, y: 0.0 },
                PathCommand::LineTo { x: w, y: h },
                PathCommand::LineTo { x: 0.0, y: h },
                PathCommand::Close,
            ],
            fill_rule: FillRule::NonZero,
            fill,
            stroke: [248, 250, 252, 255],
            stroke_width: 2.0,
        },
        ..Element::default()
    }
}

fn text_el(id: &str, name: &str, x: f64, y: f64, text: &str, color: Color) -> Element {
    Element {
        id: id.into(),
        name: name.into(),
        transform: tf(x, y),
        kind: ElementKind::Text {
            text: text.into(),
            font_family: "Arial".into(),
            font_size: 16.0,
            color,
        },
        ..Element::default()
    }
}

fn instance_el(id: &str, name: &str, symbol_id: &str, x: f64, y: f64) -> Element {
    Element {
        id: id.into(),
        name: name.into(),
        transform: tf(x, y),
        kind: ElementKind::Instance {
            symbol_id: symbol_id.into(),
            playback: InstancePlayback::Independent,
        },
        ..Element::default()
    }
}

fn single_key_layer(id: &str, name: &str, elements: Vec<Element>) -> Layer {
    Layer {
        id: id.into(),
        name: name.into(),
        visible: true,
        locked: false,
        keyframes: vec![Keyframe {
            frame: 0,
            elements,
            ..Keyframe::default()
        }],
        properties: LayerProperties::default(),
    }
}

fn mc_symbol(
    id: &str,
    name: &str,
    frame_count: u32,
    layers: Vec<Layer>,
    scripts: Vec<FrameScript>,
) -> Symbol {
    Symbol {
        id: id.into(),
        name: name.into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count,
            layers,
            scripts,
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    }
}

fn button_symbol(id: &str, name: &str, fill: Color) -> Symbol {
    Symbol {
        id: id.into(),
        name: name.into(),
        folder: String::new(),
        kind: SymbolKind::Button,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![single_key_layer(
                &format!("{id}_l"),
                "Up",
                vec![rect_el(&format!("{id}_r"), "", 0.0, 0.0, 140.0, 56.0, fill)],
            )],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    }
}

fn project_from_root(name: &str, root: Symbol, extra_symbols: Vec<Symbol>) -> Project {
    let mut symbols = extra_symbols;
    let root_id = root.id.clone();
    symbols.push(root);
    let mut project = Project {
        format_version: 6,
        name: name.into(),
        width: 960,
        height: 540,
        fps: 24,
        background: [15, 23, 42, 255],
        root: root_id,
        symbols,
        library_folders: Vec::new(),
        sources: Vec::new(),
        resources: Vec::new(),
        document_class: None,
        scenes: Vec::new(),
        script_language: None,
        also_compile_compatible_scripts: true,
    };
    project.upgrade_format_for_features();
    project
}

fn frame_script(frame: u32, language: ScriptLanguage, source: &str) -> FrameScript {
    FrameScript {
        layer: None,
        frame,
        script: Script {
            language,
            source: source.into(),
        },
    }
}

fn obj_script(event: ObjectEvent, language: ScriptLanguage, source: &str) -> ObjectScript {
    ObjectScript {
        event,
        script: Script {
            language,
            source: source.into(),
        },
    }
}

fn bundle_json_from_file(bundle_path: &Path) -> Value {
    let text = fs::read_to_string(bundle_path).expect("read bundle.js");
    let json_text = text
        .trim()
        .strip_prefix("globalThis.NeoFlashBundle = ")
        .expect("bundle prefix")
        .strip_suffix(';')
        .expect("bundle suffix");
    serde_json::from_str(json_text).expect("parse bundle json")
}

fn snapshot_tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        let mut entries: Vec<_> = fs::read_dir(dir)
            .expect("read_dir")
            .map(|r| r.expect("dir entry"))
            .collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            let rel = path
                .strip_prefix(base)
                .expect("strip prefix")
                .to_string_lossy()
                .replace('\\', "/");
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                out.insert(rel, fs::read(&path).expect("read snapshot file"));
            }
        }
    }
    let mut out = BTreeMap::new();
    if root.exists() {
        walk(root, root, &mut out);
    }
    out
}

// --- Minimal SWC builder for AS3 SWC linkage & asset gate ---

struct BitWriter {
    buf: u128,
    len: u32,
}

impl BitWriter {
    fn new() -> Self {
        Self { buf: 0, len: 0 }
    }
    fn push(&mut self, val: u64, n: u32) {
        let mask = if n == 64 { u64::MAX } else { (1u64 << n) - 1 };
        self.buf = (self.buf << n) | u128::from(val & mask);
        self.len += n;
    }
    fn finish(mut self) -> Vec<u8> {
        let pad = (8 - (self.len % 8)) % 8;
        if pad > 0 {
            self.push(0, pad);
        }
        let total = (self.len / 8) as usize;
        let mut out = Vec::with_capacity(total);
        for i in (0..total).rev() {
            out.push(((self.buf >> (i * 8)) & 0xff) as u8);
        }
        out
    }
}

fn swf_tag(out: &mut Vec<u8>, code: u16, payload: &[u8]) {
    if payload.len() < 0x3f {
        out.extend_from_slice(&((code << 6) | (payload.len() as u16)).to_le_bytes());
    } else {
        out.extend_from_slice(&((code << 6) | 0x3f).to_le_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    }
    out.extend_from_slice(payload);
}

fn build_gate_swc_bytes() -> Vec<u8> {
    let mut shape = Vec::new();
    shape.extend_from_slice(&1_u16.to_le_bytes());
    shape.push(0x00);
    shape.push(1);
    shape.push(0x00);
    shape.extend_from_slice(&[56, 189, 248]);
    shape.push(0);
    shape.push(0x10);
    let mut sb = BitWriter::new();
    sb.push(0b0_00101, 6);
    sb.push(0, 5);
    sb.push(1, 1);
    for (vert, val) in [
        (0u64, 400u64),
        (1, 400),
        (0, (-400i32) as i64 as u64),
        (1, (-400i32) as i64 as u64),
    ] {
        sb.push(0b11, 2);
        sb.push(8, 4);
        sb.push(0, 1);
        sb.push(vert, 1);
        sb.push(val, 10);
    }
    sb.push(0, 6);
    shape.extend_from_slice(&sb.finish());

    let mut sprite = Vec::new();
    sprite.extend_from_slice(&2_u16.to_le_bytes());
    sprite.extend_from_slice(&1_u16.to_le_bytes());
    let mut mb = BitWriter::new();
    mb.push(0, 1);
    mb.push(0, 1);
    mb.push(5, 5);
    mb.push(0, 5);
    mb.push(0, 5);
    let mut po2 = vec![0x06];
    po2.extend_from_slice(&1_u16.to_le_bytes());
    po2.extend_from_slice(&1_u16.to_le_bytes());
    po2.extend_from_slice(&mb.finish());
    swf_tag(&mut sprite, 26, &po2);
    swf_tag(&mut sprite, 1, &[]);
    swf_tag(&mut sprite, 0, &[]);

    let mut tags = Vec::new();
    swf_tag(&mut tags, 69, &[0x08, 0x00, 0x00, 0x00]);
    swf_tag(&mut tags, 2, &shape);
    swf_tag(&mut tags, 39, &sprite);
    let mut sym_cls = Vec::new();
    sym_cls.extend_from_slice(&1_u16.to_le_bytes());
    sym_cls.extend_from_slice(&2_u16.to_le_bytes());
    sym_cls.extend_from_slice(b"com.kit.SwcPanel\0");
    swf_tag(&mut tags, 76, &sym_cls);
    swf_tag(&mut tags, 1, &[]);

    let mut body = Vec::new();
    let mut hdr = BitWriter::new();
    hdr.push(16, 5);
    hdr.push(0, 16);
    hdr.push(4000, 16);
    hdr.push(0, 16);
    hdr.push(4000, 16);
    body.extend_from_slice(&hdr.finish());
    body.extend_from_slice(&0x1800_u16.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&tags);
    swf_tag(&mut body, 0, &[]);
    let total_len = (8 + body.len()) as u32;
    let mut swf = vec![b'F', b'W', b'S', 10];
    swf.extend_from_slice(&total_len.to_le_bytes());
    swf.extend_from_slice(&body);

    let mut rgba = Vec::with_capacity(16 * 4);
    for _ in 0..16 {
        rgba.extend_from_slice(&[34, 211, 238, 255]);
    }
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(&rgba, 4, 4, ExtendedColorType::Rgba8)
        .unwrap();

    let catalog = r#"<?xml version="1.0" encoding="utf-8"?>
<swc xmlns="http://www.adobe.com/flash/swccatalog/9">
  <versions><swc version="1.2"/></versions>
  <features><feature-script-deps/><feature-components/><feature-files/></features>
  <components>
    <component className="com.kit:SwcPanel" name="SwcPanel" icon="assets/swc-icon.png"/>
  </components>
  <libraries>
    <library path="library.swf">
      <script name="com/kit/SwcPanel" mod="100">
        <def id="com.kit:SwcPanel"/>
        <dep id="flash.display:MovieClip" type="i"/>
      </script>
      <script name="com/kit/SwcHelper" mod="100">
        <def id="com.kit:SwcHelper"/>
      </script>
    </library>
  </libraries>
  <files><file path="assets/swc-icon.png" mod="100"/></files>
</swc>"#;

    let mut cur = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut cur);
        let opts = SimpleFileOptions::default();
        z.start_file("catalog.xml", opts).unwrap();
        z.write_all(catalog.as_bytes()).unwrap();
        z.start_file("library.swf", opts).unwrap();
        z.write_all(&swf).unwrap();
        z.start_file("assets/swc-icon.png", opts).unwrap();
        z.write_all(&png).unwrap();
        z.finish().unwrap();
    }
    cur.into_inner()
}

// --- Focused project builders (shared between unit tests & browser gate exporter) ---

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct InteractionSpec {
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    x: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    y: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    to_x: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    to_y: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    key: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expect_trace: Option<&'static str>,
}

struct CaseSpec {
    id: &'static str,
    group: &'static str,
    language: &'static str,
    project: Project,
    child_project: Option<Project>,
    clean_chrome: bool,
    allow_file_protocol: bool,
    expected_pass: &'static str,
    expected_error_substrings: Vec<&'static str>,
    interactions: Vec<InteractionSpec>,
    verify_restart: bool,
}

fn build_focus_as2_frame_scripts() -> CaseSpec {
    let root = mc_symbol(
        "scene",
        "Main",
        4,
        vec![
            Layer {
                id: "l_ui".into(),
                name: "UI".into(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![text_el(
                        "st",
                        "statusText",
                        40.0,
                        40.0,
                        "init",
                        [226, 232, 240, 255],
                    )],
                    ..Keyframe::default()
                }],
                properties: LayerProperties::default(),
            },
            Layer {
                id: "l0".into(),
                name: "Labels".into(),
                visible: true,
                locked: false,
                keyframes: vec![
                    Keyframe {
                        frame: 0,
                        label: "start".into(),
                        ..Keyframe::default()
                    },
                    Keyframe {
                        frame: 3,
                        label: "gate_end".into(),
                        ..Keyframe::default()
                    },
                ],
                properties: LayerProperties::default(),
            },
        ],
        vec![
            frame_script(
                0,
                ScriptLanguage::ActionScript2,
                "var logSeq = ['f1'];\ngotoAndStop('gate_end');",
            ),
            frame_script(
                3,
                ScriptLanguage::ActionScript2,
                "stop();\nlogSeq.push('f4');\nstatusText.text = logSeq.join('>');\ntrace('[PASS] focus-as2-frame-scripts seq=' + logSeq.join('>'));",
            ),
        ],
    );
    CaseSpec {
        id: "focus-as2-frame-scripts",
        group: "focused",
        language: "as2",
        project: project_from_root("Focus AS2 Frame Scripts", root, vec![]),
        child_project: None,
        clean_chrome: false,
        allow_file_protocol: true,
        expected_pass: "[PASS] focus-as2-frame-scripts seq=f1>f4",
        expected_error_substrings: vec![],
        interactions: vec![],
        verify_restart: true,
    }
}

fn build_focus_as3_frame_scripts() -> CaseSpec {
    let root = mc_symbol(
        "scene",
        "Main",
        4,
        vec![
            Layer {
                id: "l_ui".into(),
                name: "UI".into(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![text_el(
                        "st",
                        "statusText",
                        40.0,
                        40.0,
                        "init",
                        [226, 232, 240, 255],
                    )],
                    ..Keyframe::default()
                }],
                properties: LayerProperties::default(),
            },
            Layer {
                id: "l0".into(),
                name: "Labels".into(),
                visible: true,
                locked: false,
                keyframes: vec![
                    Keyframe {
                        frame: 0,
                        label: "start".into(),
                        ..Keyframe::default()
                    },
                    Keyframe {
                        frame: 3,
                        label: "gate_end".into(),
                        ..Keyframe::default()
                    },
                ],
                properties: LayerProperties::default(),
            },
        ],
        vec![
            frame_script(
                0,
                ScriptLanguage::ActionScript3,
                "var logSeq:Array = ['f1'];\ngotoAndStop('gate_end');",
            ),
            frame_script(
                3,
                ScriptLanguage::ActionScript3,
                "stop();\nlogSeq.push('f4');\nstatusText.text = logSeq.join('>');\ntrace('[PASS] focus-as3-frame-scripts seq=' + logSeq.join('>'));",
            ),
        ],
    );
    CaseSpec {
        id: "focus-as3-frame-scripts",
        group: "focused",
        language: "as3",
        project: project_from_root("Focus AS3 Frame Scripts", root, vec![]),
        child_project: None,
        clean_chrome: false,
        allow_file_protocol: true,
        expected_pass: "[PASS] focus-as3-frame-scripts seq=f1>f4",
        expected_error_substrings: vec![],
        interactions: vec![],
        verify_restart: true,
    }
}

fn build_focus_as2_object_handlers() -> CaseSpec {
    let clip_sym = mc_symbol(
        "clip_sym",
        "ClickClip",
        1,
        vec![single_key_layer(
            "cl",
            "Art",
            vec![rect_el(
                "cr",
                "",
                0.0,
                0.0,
                140.0,
                56.0,
                [56, 189, 248, 255],
            )],
        )],
        vec![],
    );
    let btn_sym = button_symbol("btn_sym", "GateButton", [244, 114, 182, 255]);
    let gem_sym = mc_symbol(
        "gem_sym",
        "GemSym",
        1,
        vec![single_key_layer(
            "gl",
            "Art",
            vec![rect_el(
                "gr",
                "",
                0.0,
                0.0,
                100.0,
                40.0,
                [250, 204, 21, 255],
            )],
        )],
        vec![],
    );
    let outer_sym = mc_symbol(
        "outer_sym",
        "OuterBox",
        1,
        vec![single_key_layer(
            "ol",
            "Outer",
            vec![
                rect_el("obg", "", 0.0, 0.0, 180.0, 90.0, [30, 41, 59, 255]),
                instance_el("gem_i", "innerGem", "gem_sym", 30.0, 20.0),
            ],
        )],
        vec![],
    );

    let mut btn_clip = instance_el("clip_i", "clipBtn", "clip_sym", 80.0, 120.0);
    btn_clip.scripts = vec![
        obj_script(
            ObjectEvent::Load,
            ScriptLanguage::ActionScript2,
            "_root.objLoaded = (_root.objLoaded || 0) + 1;",
        ),
        obj_script(
            ObjectEvent::EnterFrame,
            ScriptLanguage::ActionScript2,
            "if (!_root.efSeen) { _root.efSeen = true; _root.maybePass(); }",
        ),
        obj_script(
            ObjectEvent::Click,
            ScriptLanguage::ActionScript2,
            "trace('[EVENT] as2-clip-click target=' + this._name);",
        ),
    ];
    let mut push_btn = instance_el("btn_i", "pushBtn", "btn_sym", 260.0, 120.0);
    push_btn.scripts = vec![obj_script(
        ObjectEvent::Click,
        ScriptLanguage::ActionScript2,
        "trace('[EVENT] as2-button-click target=' + this._name);",
    )];
    let outer_inst = instance_el("outer_i", "outerBox", "outer_sym", 460.0, 120.0);

    let root = mc_symbol(
        "scene",
        "Main",
        1,
        vec![single_key_layer(
            "main_l",
            "Stage",
            vec![
                text_el(
                    "st",
                    "statusText",
                    40.0,
                    40.0,
                    "waiting",
                    [226, 232, 240, 255],
                ),
                btn_clip,
                push_btn,
                outer_inst,
            ],
        )],
        vec![frame_script(
            0,
            ScriptLanguage::ActionScript2,
            r#"stop();
var owner:MovieClip = this;
_root.maybePass = function():Void {
  if (!_root.passEmitted && _root.objLoaded == 1 && _root.efSeen) {
    _root.passEmitted = true;
    trace("[PASS] focus-as2-object-handlers loaded=" + _root.objLoaded);
  }
};
this.outerBox.innerGem.onRelease = function():Void {
  trace("[EVENT] as2-nested-target=" + this._name + " parent=" + this._parent._name);
};
_root.maybePass();
"#,
        )],
    );
    CaseSpec {
        id: "focus-as2-object-handlers",
        group: "focused",
        language: "as2",
        project: project_from_root(
            "Focus AS2 Object Handlers",
            root,
            vec![clip_sym, btn_sym, gem_sym, outer_sym],
        ),
        child_project: None,
        clean_chrome: false,
        allow_file_protocol: true,
        expected_pass: "[PASS] focus-as2-object-handlers loaded=1",
        expected_error_substrings: vec![],
        interactions: vec![
            InteractionSpec {
                kind: "click",
                x: Some(150.0),
                y: Some(148.0),
                to_x: None,
                to_y: None,
                key: None,
                text: None,
                expect_trace: Some("[EVENT] as2-clip-click target=clipBtn"),
            },
            InteractionSpec {
                kind: "click",
                x: Some(330.0),
                y: Some(148.0),
                to_x: None,
                to_y: None,
                key: None,
                text: None,
                expect_trace: Some("[EVENT] as2-button-click target=pushBtn"),
            },
            InteractionSpec {
                kind: "click",
                x: Some(540.0),
                y: Some(160.0),
                to_x: None,
                to_y: None,
                key: None,
                text: None,
                expect_trace: Some("[EVENT] as2-nested-target=innerGem parent=outerBox"),
            },
        ],
        verify_restart: true,
    }
}

fn build_focus_as3_object_handlers() -> CaseSpec {
    let clip_sym = mc_symbol(
        "clip_sym",
        "ClickClip",
        1,
        vec![single_key_layer(
            "cl",
            "Art",
            vec![rect_el(
                "cr",
                "",
                0.0,
                0.0,
                140.0,
                56.0,
                [129, 140, 248, 255],
            )],
        )],
        vec![],
    );
    let btn_sym = button_symbol("btn_sym", "SimpleGateButton", [244, 114, 182, 255]);
    let gem_sym = mc_symbol(
        "gem_sym",
        "GemSym",
        1,
        vec![single_key_layer(
            "gl",
            "Art",
            vec![rect_el(
                "gr",
                "",
                0.0,
                0.0,
                100.0,
                40.0,
                [52, 211, 153, 255],
            )],
        )],
        vec![],
    );
    let parent_sym = mc_symbol(
        "parent_sym",
        "ParentBox",
        1,
        vec![single_key_layer(
            "pl",
            "Outer",
            vec![
                rect_el("pbg", "", 0.0, 0.0, 180.0, 90.0, [30, 41, 59, 255]),
                instance_el("gem_i", "innerGem", "gem_sym", 30.0, 20.0),
            ],
        )],
        vec![],
    );

    let mut as3_clip = instance_el("clip_i", "as3ClipBtn", "clip_sym", 80.0, 120.0);
    as3_clip.scripts = vec![
        obj_script(
            ObjectEvent::Load,
            ScriptLanguage::ActionScript3,
            "if (root != null) { root.as3ObjLoaded = (root.as3ObjLoaded || 0) + 1; }",
        ),
        obj_script(
            ObjectEvent::Click,
            ScriptLanguage::ActionScript3,
            "trace('[EVENT] as3-obj-click name=' + this.name);",
        ),
    ];
    let mut as3_btn = instance_el("btn_i", "as3SimpleBtn", "btn_sym", 260.0, 120.0);
    as3_btn.scripts = vec![obj_script(
        ObjectEvent::Click,
        ScriptLanguage::ActionScript3,
        "trace('[EVENT] as3-button-click name=' + this.name);",
    )];
    let parent_inst = instance_el("parent_i", "parentBox", "parent_sym", 460.0, 120.0);

    let root = mc_symbol(
        "scene",
        "Main",
        1,
        vec![single_key_layer(
            "main_l",
            "Stage",
            vec![
                text_el(
                    "st",
                    "statusText",
                    40.0,
                    40.0,
                    "waiting",
                    [226, 232, 240, 255],
                ),
                as3_clip,
                as3_btn,
                parent_inst,
            ],
        )],
        vec![frame_script(
            0,
            ScriptLanguage::ActionScript3,
            r#"import flash.events.MouseEvent;
stop();
var parentRef:MovieClip = this.parentBox as MovieClip;
parentRef.addEventListener(MouseEvent.CLICK, function(e:MouseEvent):void {
  trace("[EVENT] as3-target=" + e.target.name + " currentTarget=" + e.currentTarget.name);
});
trace("[PASS] focus-as3-object-handlers loaded=" + this.as3ObjLoaded);
"#,
        )],
    );
    CaseSpec {
        id: "focus-as3-object-handlers",
        group: "focused",
        language: "as3",
        project: project_from_root(
            "Focus AS3 Object Handlers",
            root,
            vec![clip_sym, btn_sym, gem_sym, parent_sym],
        ),
        child_project: None,
        clean_chrome: false,
        allow_file_protocol: true,
        expected_pass: "[PASS] focus-as3-object-handlers loaded=1",
        expected_error_substrings: vec![],
        interactions: vec![
            InteractionSpec {
                kind: "click",
                x: Some(150.0),
                y: Some(148.0),
                to_x: None,
                to_y: None,
                key: None,
                text: None,
                expect_trace: Some("[EVENT] as3-obj-click name=as3ClipBtn"),
            },
            InteractionSpec {
                kind: "click",
                x: Some(330.0),
                y: Some(148.0),
                to_x: None,
                to_y: None,
                key: None,
                text: None,
                expect_trace: Some("[EVENT] as3-button-click name=as3SimpleBtn"),
            },
            InteractionSpec {
                kind: "click",
                x: Some(540.0),
                y: Some(160.0),
                to_x: None,
                to_y: None,
                key: None,
                text: None,
                expect_trace: Some("[EVENT] as3-target=innerGem currentTarget=parentBox"),
            },
        ],
        verify_restart: true,
    }
}

fn build_focus_as2_linkage_class() -> CaseSpec {
    let mut badge_sym = mc_symbol(
        "badge_sym",
        "GateBadge",
        1,
        vec![single_key_layer(
            "bl",
            "Art",
            vec![rect_el("br", "", 0.0, 0.0, 90.0, 48.0, [52, 211, 153, 255])],
        )],
        vec![],
    );
    badge_sym.linkage_id = Some("GateBadge".into());
    badge_sym.class_name = Some("widgets.BadgeWidget".into());

    let root = mc_symbol(
        "scene",
        "Main",
        1,
        vec![single_key_layer(
            "ml",
            "Stage",
            vec![
                text_el("st", "statusText", 40.0, 40.0, "init", [226, 232, 240, 255]),
                instance_el("placed_badge", "placedBadge", "badge_sym", 120.0, 140.0),
            ],
        )],
        vec![frame_script(
            0,
            ScriptLanguage::ActionScript2,
            r#"import widgets.BadgeWidget;
stop();
var attached = this.attachMovie("GateBadge", "dynBadge", 10, { _x: 260, _y: 140 });
var placedOk = (this.placedBadge instanceof BadgeWidget) && this.placedBadge.labelCode() == "WIDGET-OK";
var dynOk = (attached instanceof BadgeWidget) && attached.labelCode() == "WIDGET-OK";
if (placedOk && dynOk) {
  statusText.text = "PASS linkage=" + attached.labelCode();
  trace("[PASS] focus-as2-linkage-class placed=" + placedOk + " dyn=" + dynOk);
} else {
  throw new Error("AS2 linkage class check failed: placed=" + placedOk + " dyn=" + dynOk);
}
"#,
        )],
    );
    let mut project = project_from_root("Focus AS2 Linkage Class", root, vec![badge_sym]);
    project.sources = vec![SourceUnit {
        id: "src_badge".into(),
        path: "widgets/BadgeWidget.as".into(),
        language: ScriptLanguage::ActionScript2,
        source: r#"class widgets.BadgeWidget extends MovieClip {
  public function labelCode():String {
    return "WIDGET-OK";
  }
}
"#
        .into(),
    }];
    CaseSpec {
        id: "focus-as2-linkage-class",
        group: "focused",
        language: "as2",
        project,
        child_project: None,
        clean_chrome: false,
        allow_file_protocol: true,
        expected_pass: "[PASS] focus-as2-linkage-class placed=true dyn=true",
        expected_error_substrings: vec![],
        interactions: vec![],
        verify_restart: true,
    }
}

fn build_focus_as3_doc_and_symbol_class() -> CaseSpec {
    let mut badge_sym = mc_symbol(
        "badge_sym",
        "GateBadge",
        1,
        vec![single_key_layer(
            "bl",
            "Art",
            vec![rect_el(
                "br",
                "",
                0.0,
                0.0,
                90.0,
                48.0,
                [129, 140, 248, 255],
            )],
        )],
        vec![],
    );
    badge_sym.linkage_id = Some("GateBadge".into());
    badge_sym.class_name = Some("widgets.GateBadge".into());

    let root = mc_symbol(
        "scene",
        "Main",
        1,
        vec![single_key_layer(
            "ml",
            "Stage",
            vec![
                text_el("st", "statusText", 40.0, 40.0, "init", [226, 232, 240, 255]),
                instance_el("placed_badge", "placedBadge", "badge_sym", 120.0, 140.0),
            ],
        )],
        vec![],
    );
    let mut project = project_from_root("Focus AS3 Doc & Symbol Class", root, vec![badge_sym]);
    project.document_class = Some("app.GateMain".into());
    project.sources = vec![
        SourceUnit {
            id: "src_main".into(),
            path: "app/GateMain.as".into(),
            language: ScriptLanguage::ActionScript3,
            source: r#"package app {
  import flash.display.MovieClip;
  import flash.utils.getDefinitionByName;
  import widgets.GateBadge;
  public dynamic class GateMain extends MovieClip {
    public static var bootCount:int = 0;
    public function GateMain() {
      super();
      stop();
      bootCount += 1;
      var cls:Class = getDefinitionByName("widgets.GateBadge") as Class;
      var dyn:GateBadge = new cls() as GateBadge;
      dyn.x = 260;
      dyn.y = 140;
      addChild(dyn);
      var stageOk:Boolean = (this.stage != null);
      var placedOk:Boolean = (this.placedBadge is GateBadge) && (this.placedBadge as GateBadge).tag() == "BADGE-3";
      var dynOk:Boolean = (dyn != null && dyn.tag() == "BADGE-3");
      if (stageOk && placedOk && dynOk) {
        trace("[PASS] focus-as3-doc-and-symbol-class boots=" + bootCount + " stage=" + stageOk);
      } else {
        throw new Error("Doc/Symbol class check failed");
      }
    }
  }
}
"#
            .into(),
        },
        SourceUnit {
            id: "src_badge".into(),
            path: "widgets/GateBadge.as".into(),
            language: ScriptLanguage::ActionScript3,
            source: r#"package widgets {
  import flash.display.MovieClip;
  public dynamic class GateBadge extends MovieClip {
    public function GateBadge() {
      super();
    }
    public function tag():String {
      return "BADGE-3";
    }
  }
}
"#
            .into(),
        },
    ];
    CaseSpec {
        id: "focus-as3-doc-and-symbol-class",
        group: "focused",
        language: "as3",
        project,
        child_project: None,
        clean_chrome: false,
        allow_file_protocol: true,
        expected_pass: "[PASS] focus-as3-doc-and-symbol-class boots=1 stage=true",
        expected_error_substrings: vec![],
        interactions: vec![],
        verify_restart: true,
    }
}

fn build_focus_as2_include_initclip() -> CaseSpec {
    let init_sym = mc_symbol(
        "init_comp",
        "GateInitComp",
        1,
        vec![single_key_layer("il", "Art", vec![])],
        vec![frame_script(
            0,
            ScriptLanguage::ActionScript2,
            r#"#initclip 5
#include "includes/macros.as"
_global.gateInitValue = computeGateMagic(21);
#endinitclip
"#,
        )],
    );
    let root = mc_symbol(
        "scene",
        "Main",
        1,
        vec![single_key_layer(
            "ml",
            "Stage",
            vec![
                text_el("st", "statusText", 40.0, 40.0, "init", [226, 232, 240, 255]),
                instance_el("ic", "initInst", "init_comp", 0.0, 0.0),
            ],
        )],
        vec![frame_script(
            0,
            ScriptLanguage::ActionScript2,
            r#"#include "includes/macros.as"
stop();
var frameVal:Number = computeGateMagic(10);
if (_global.gateInitValue == 42 && frameVal == 20) {
  statusText.text = "PASS initclip=" + _global.gateInitValue;
  trace("[PASS] focus-as2-include-initclip init=" + _global.gateInitValue + " frame=" + frameVal);
} else {
  throw new Error("include/initclip mismatch");
}
"#,
        )],
    );
    let mut project = project_from_root("Focus AS2 Include Initclip", root, vec![init_sym]);
    project.resources = vec![
        ResourceUnit::from_bytes(
            "res_inc_macros",
            "includes/macros.as",
            "text/plain",
            b"function computeGateMagic(v:Number):Number { return v * 2; }\n",
        )
        .unwrap(),
    ];
    CaseSpec {
        id: "focus-as2-include-initclip",
        group: "focused",
        language: "as2",
        project,
        child_project: None,
        clean_chrome: false,
        allow_file_protocol: true,
        expected_pass: "[PASS] focus-as2-include-initclip init=42 frame=20",
        expected_error_substrings: vec![],
        interactions: vec![],
        verify_restart: true,
    }
}

fn build_focus_as3_include_swc() -> CaseSpec {
    let swc_bytes = build_gate_swc_bytes();
    let swc_lib = import_swc_bytes("gate_kit.swc", &swc_bytes).expect("import gate SWC");

    let root = mc_symbol(
        "scene",
        "Main",
        1,
        vec![single_key_layer(
            "ml",
            "Stage",
            vec![text_el(
                "st",
                "statusText",
                40.0,
                40.0,
                "init",
                [226, 232, 240, 255],
            )],
        )],
        vec![frame_script(
            0,
            ScriptLanguage::ActionScript3,
            r#"import flash.display.Loader;
import flash.events.Event;
import flash.net.URLRequest;
import flash.utils.getDefinitionByName;
include "includes/as3_helpers.as";
stop();
var panelCls:Class = getDefinitionByName("com.kit.SwcPanel") as Class;
var panel:* = new panelCls();
panel.x = 120;
panel.y = 140;
addChild(panel);
var helperCls:Class = getDefinitionByName("com.kit.SwcHelper") as Class;
var helperObj:* = new helperCls();
var mulVal:int = doubleGateArg(21);
var iconLoader:Loader = new Loader();
iconLoader.contentLoaderInfo.addEventListener(Event.COMPLETE, function(e:Event):void {
  if (panel != null && helperObj != null && mulVal == 42 && iconLoader.contentLoaderInfo.width == 4) {
    trace("[PASS] focus-as3-include-swc mul=" + mulVal + " iconW=" + iconLoader.contentLoaderInfo.width);
  }
});
addChild(iconLoader);
iconLoader.load(new URLRequest("swc/assets/swc-icon.png"));
"#,
        )],
    );
    let mut project = project_from_root("Focus AS3 Include & SWC", root, vec![]);
    project.resources.push(
        ResourceUnit::from_bytes(
            "res_inc_as3",
            "includes/as3_helpers.as",
            "text/plain",
            b"function doubleGateArg(n:int):int { return n * 2; }\n",
        )
        .unwrap(),
    );
    let link_rep = link_swc_into_project(&mut project, &swc_lib, SwcConflictPolicy::PreferNewer)
        .expect("link gate SWC");
    assert!(
        link_rep.linked_symbols >= 1 && link_rep.linked_resources >= 1,
        "expected SWC symbol and PNG asset linked: {link_rep:?}"
    );
    CaseSpec {
        id: "focus-as3-include-swc",
        group: "focused",
        language: "as3",
        project,
        child_project: None,
        clean_chrome: false,
        allow_file_protocol: true,
        expected_pass: "[PASS] focus-as3-include-swc mul=42 iconW=4",
        expected_error_substrings: vec![],
        interactions: vec![],
        verify_restart: true,
    }
}

fn build_focus_as2_scenes() -> CaseSpec {
    let scene1 = mc_symbol(
        "scene_1",
        "Intro",
        2,
        vec![single_key_layer(
            "s1_l",
            "Intro",
            vec![text_el(
                "s1_st",
                "statusText",
                40.0,
                40.0,
                "intro",
                [226, 232, 240, 255],
            )],
        )],
        vec![frame_script(
            0,
            ScriptLanguage::ActionScript2,
            "var trail = ['S1'];",
        )],
    );
    let scene2 = mc_symbol(
        "scene_2",
        "Finale",
        2,
        vec![single_key_layer(
            "s2_l",
            "Finale",
            vec![text_el(
                "s2_st",
                "finaleText",
                40.0,
                80.0,
                "finale",
                [52, 211, 153, 255],
            )],
        )],
        vec![frame_script(
            0,
            ScriptLanguage::ActionScript2,
            "stop();\ntrail.push('S2');\ntrace('[PASS] focus-as2-scenes path=' + trail.join('->'));",
        )],
    );
    let mut project = project_from_root("Focus AS2 Scenes", scene1, vec![scene2]);
    project.scenes = vec!["scene_1".into(), "scene_2".into()];
    CaseSpec {
        id: "focus-as2-scenes",
        group: "focused",
        language: "as2",
        project,
        child_project: None,
        clean_chrome: false,
        allow_file_protocol: true,
        expected_pass: "[PASS] focus-as2-scenes path=S1->S2",
        expected_error_substrings: vec![],
        interactions: vec![],
        verify_restart: true,
    }
}

fn build_focus_as3_scenes() -> CaseSpec {
    let scene1 = mc_symbol(
        "scene_1",
        "Intro",
        2,
        vec![single_key_layer(
            "s1_l",
            "Intro",
            vec![text_el(
                "s1_st",
                "statusText",
                40.0,
                40.0,
                "intro",
                [226, 232, 240, 255],
            )],
        )],
        vec![frame_script(
            0,
            ScriptLanguage::ActionScript3,
            "var trail:Array = [this.currentScene.name];\ngotoAndStop(1, 'Finale');",
        )],
    );
    let scene2 = mc_symbol(
        "scene_2",
        "Finale",
        2,
        vec![single_key_layer(
            "s2_l",
            "Finale",
            vec![text_el(
                "s2_st",
                "finaleText",
                40.0,
                80.0,
                "finale",
                [129, 140, 248, 255],
            )],
        )],
        vec![frame_script(
            0,
            ScriptLanguage::ActionScript3,
            "stop();\ntrail.push(this.currentScene.name);\ntrace('[PASS] focus-as3-scenes path=' + trail.join('->'));",
        )],
    );
    let mut project = project_from_root("Focus AS3 Scenes", scene1, vec![scene2]);
    project.scenes = vec!["scene_1".into(), "scene_2".into()];
    CaseSpec {
        id: "focus-as3-scenes",
        group: "focused",
        language: "as3",
        project,
        child_project: None,
        clean_chrome: false,
        allow_file_protocol: true,
        expected_pass: "[PASS] focus-as3-scenes path=Intro->Finale",
        expected_error_substrings: vec![],
        interactions: vec![],
        verify_restart: true,
    }
}

fn build_focus_loadpublish_html(lang: ScriptLanguage) -> CaseSpec {
    let (id, slug_lang) = match lang {
        ScriptLanguage::ActionScript2 => ("focus-as2-loadpublish-html", "as2"),
        ScriptLanguage::ActionScript3 => ("focus-as3-loadpublish-html", "as3"),
        _ => unreachable!(),
    };
    let child_root = mc_symbol(
        "child_root",
        "ChildMain",
        6,
        vec![single_key_layer(
            "cl",
            "Child",
            vec![rect_el(
                "cr",
                "",
                0.0,
                0.0,
                120.0,
                80.0,
                [56, 189, 248, 255],
            )],
        )],
        vec![frame_script(
            0,
            lang,
            "stop();\ntrace('[CHILD-READY] ' + this._totalframes || 'as3');",
        )],
    );
    let mut child_project = project_from_root("Gate HTML Child", child_root, vec![]);
    child_project.width = 240;
    child_project.height = 160;
    if lang == ScriptLanguage::ActionScript3 {
        child_project
            .symbol_mut("child_root")
            .unwrap()
            .timeline
            .scripts = vec![frame_script(
            0,
            ScriptLanguage::ActionScript3,
            "stop();\ntrace('[CHILD-READY] as3 frames=' + this.totalFrames);",
        )];
    }

    let holder_sym = mc_symbol(
        "holder_sym",
        "HolderSym",
        1,
        vec![single_key_layer("hl", "H", vec![])],
        vec![],
    );
    let script = match lang {
        ScriptLanguage::ActionScript2 => {
            r#"stop();
var h:MovieClip = this.holder;
h.loadMovie("child/index.html");
var done:Boolean = false;
this.onEnterFrame = function():Void {
  if (!done && h._totalframes == 6) {
    done = true;
    trace("[PASS] focus-as2-loadpublish-html frames=" + h._totalframes);
  }
};
"#
        }
        ScriptLanguage::ActionScript3 => {
            r#"import flash.display.Loader;
import flash.display.MovieClip;
import flash.events.Event;
import flash.net.URLRequest;
stop();
var h:MovieClip = this.holder as MovieClip;
var loader:Loader = new Loader();
h.addChild(loader);
loader.contentLoaderInfo.addEventListener(Event.COMPLETE, function(e:Event):void {
  trace("[PASS] focus-as3-loadpublish-html w=" + int(loader.contentLoaderInfo.width) + " h=" + int(loader.contentLoaderInfo.height));
});
loader.load(new URLRequest("child/index.html"));
"#
        }
        _ => unreachable!(),
    };
    let root = mc_symbol(
        "scene",
        "Main",
        1,
        vec![single_key_layer(
            "ml",
            "Stage",
            vec![
                text_el(
                    "st",
                    "statusText",
                    40.0,
                    40.0,
                    "mounting",
                    [226, 232, 240, 255],
                ),
                instance_el("holder_i", "holder", "holder_sym", 120.0, 120.0),
            ],
        )],
        vec![frame_script(0, lang, script)],
    );
    CaseSpec {
        id,
        group: "focused",
        language: slug_lang,
        project: project_from_root(id, root, vec![holder_sym]),
        child_project: Some(child_project),
        clean_chrome: false,
        // External unbundled `child/index.html` fetches from HTTP(S); browsers forbid arbitrary sibling `file://` fetch by default.
        allow_file_protocol: false,
        expected_pass: if lang == ScriptLanguage::ActionScript2 {
            "[PASS] focus-as2-loadpublish-html frames=6"
        } else {
            "[PASS] focus-as3-loadpublish-html w=240 h=160"
        },
        expected_error_substrings: vec![],
        interactions: vec![],
        verify_restart: true,
    }
}

fn build_focus_restart_reset(lang: ScriptLanguage) -> CaseSpec {
    match lang {
        ScriptLanguage::ActionScript2 => {
            let root = mc_symbol(
                "scene",
                "Main",
                1,
                vec![single_key_layer(
                    "ml",
                    "Stage",
                    vec![text_el(
                        "st",
                        "statusText",
                        40.0,
                        40.0,
                        "init",
                        [226, 232, 240, 255],
                    )],
                )],
                vec![frame_script(
                    0,
                    ScriptLanguage::ActionScript2,
                    r#"stop();
_global.gateRunCount = (_global.gateRunCount || 0) + 1;
var dyn:MovieClip = this.createEmptyMovieClip("dynClip", 5);
dyn.beginFill(0x38BDF8, 100);
dyn.moveTo(0, 0); dyn.lineTo(40, 0); dyn.lineTo(40, 40); dyn.lineTo(0, 40); dyn.endFill();
var ticks:Number = 0;
setInterval(function():Void { ticks = ticks + 1; }, 40);
trace("[PASS] focus-as2-restart-reset run=" + _global.gateRunCount + " dyn=" + (dyn != null));
"#,
                )],
            );
            CaseSpec {
                id: "focus-as2-restart-reset",
                group: "focused",
                language: "as2",
                project: project_from_root("Focus AS2 Restart Reset", root, vec![]),
                child_project: None,
                clean_chrome: false,
                allow_file_protocol: true,
                expected_pass: "[PASS] focus-as2-restart-reset run=1 dyn=true",
                expected_error_substrings: vec![],
                interactions: vec![],
                verify_restart: true,
            }
        }
        ScriptLanguage::ActionScript3 => {
            let root = mc_symbol(
                "scene",
                "Main",
                1,
                vec![single_key_layer("ml", "Stage", vec![])],
                vec![],
            );
            let mut project = project_from_root("Focus AS3 Restart Reset", root, vec![]);
            project.document_class = Some("app.RestartMain".into());
            project.sources = vec![SourceUnit {
                id: "src_restart_as3".into(),
                path: "app/RestartMain.as".into(),
                language: ScriptLanguage::ActionScript3,
                source: r#"package app {
  import flash.display.MovieClip;
  import flash.display.Sprite;
  import flash.events.TimerEvent;
  import flash.utils.Timer;
  public dynamic class RestartMain extends MovieClip {
    public static var runCounter:int = 0;
    public function RestartMain() {
      super();
      stop();
      runCounter += 1;
      var box:Sprite = new Sprite();
      box.graphics.beginFill(0x818CF8, 1.0);
      box.graphics.drawRect(80, 120, 48, 48);
      box.graphics.endFill();
      addChild(box);
      var timer:Timer = new Timer(40, 0);
      timer.addEventListener(TimerEvent.TIMER, function(e:TimerEvent):void {});
      timer.start();
      trace("[PASS] focus-as3-restart-reset run=" + runCounter + " children=" + this.numChildren);
    }
  }
}
"#
                .into(),
            }];
            CaseSpec {
                id: "focus-as3-restart-reset",
                group: "focused",
                language: "as3",
                project,
                child_project: None,
                clean_chrome: false,
                allow_file_protocol: true,
                expected_pass: "[PASS] focus-as3-restart-reset run=1 children=1",
                expected_error_substrings: vec![],
                interactions: vec![],
                verify_restart: true,
            }
        }
        _ => unreachable!(),
    }
}

fn build_focus_as3_clean_chrome() -> CaseSpec {
    let root = mc_symbol(
        "scene",
        "Main",
        1,
        vec![single_key_layer(
            "ml",
            "Stage",
            vec![rect_el(
                "box",
                "",
                100.0,
                100.0,
                120.0,
                120.0,
                [52, 211, 153, 255],
            )],
        )],
        vec![frame_script(
            0,
            ScriptLanguage::ActionScript3,
            "stop();\ntrace('[PASS] focus-as3-clean-chrome rendered=true');",
        )],
    );
    CaseSpec {
        id: "focus-as3-clean-chrome",
        group: "focused",
        language: "as3",
        project: project_from_root("Focus AS3 Clean Chrome", root, vec![]),
        child_project: None,
        clean_chrome: true,
        allow_file_protocol: true,
        expected_pass: "[PASS] focus-as3-clean-chrome rendered=true",
        expected_error_substrings: vec![],
        interactions: vec![],
        verify_restart: false,
    }
}

fn minimal_wav_bytes() -> Vec<u8> {
    let samples: u32 = 64;
    let data_len: u32 = samples * 2;
    let mut b = Vec::with_capacity(44 + data_len as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&8000u32.to_le_bytes());
    b.extend_from_slice(&16000u32.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    b.resize(44 + data_len as usize, 0);
    b
}

fn fill_rect_el(id: &str, name: &str, x: f64, y: f64, w: f64, h: f64, fill: Color) -> Element {
    Element {
        id: id.into(),
        name: name.into(),
        transform: tf(x, y),
        kind: ElementKind::Path {
            commands: vec![
                PathCommand::MoveTo { x: 0.0, y: 0.0 },
                PathCommand::LineTo { x: w, y: 0.0 },
                PathCommand::LineTo { x: w, y: h },
                PathCommand::LineTo { x: 0.0, y: h },
                PathCommand::Close,
            ],
            fill_rule: FillRule::NonZero,
            fill,
            stroke: [0, 0, 0, 0],
            stroke_width: 0.0,
        },
        ..Element::default()
    }
}

fn build_focus_t304_display_graphics() -> CaseSpec {
    let btn_sym = Symbol {
        id: "btn_t304".into(),
        name: "T304Button".into(),
        folder: String::new(),
        kind: SymbolKind::Button,
        timeline: Timeline {
            frame_count: 4,
            layers: vec![Layer {
                id: "btn_l".into(),
                name: "States".into(),
                visible: true,
                locked: false,
                keyframes: vec![
                    Keyframe {
                        frame: 0,
                        elements: vec![fill_rect_el(
                            "btn_up",
                            "",
                            0.0,
                            0.0,
                            140.0,
                            56.0,
                            [59, 130, 246, 255],
                        )],
                        ..Keyframe::default()
                    },
                    Keyframe {
                        frame: 1,
                        elements: vec![fill_rect_el(
                            "btn_over",
                            "",
                            0.0,
                            0.0,
                            140.0,
                            56.0,
                            [245, 158, 11, 255],
                        )],
                        sound: Some(FrameSound {
                            sync: SoundSync::Event,
                            ..FrameSound::new("snd_btn")
                        }),
                        ..Keyframe::default()
                    },
                    Keyframe {
                        frame: 2,
                        elements: vec![fill_rect_el(
                            "btn_down",
                            "",
                            0.0,
                            0.0,
                            140.0,
                            56.0,
                            [239, 68, 68, 255],
                        )],
                        sound: Some(FrameSound {
                            sync: SoundSync::Event,
                            ..FrameSound::new("snd_btn")
                        }),
                        ..Keyframe::default()
                    },
                    Keyframe {
                        frame: 3,
                        elements: vec![fill_rect_el(
                            "btn_hit",
                            "",
                            0.0,
                            0.0,
                            140.0,
                            56.0,
                            [255, 255, 255, 255],
                        )],
                        ..Keyframe::default()
                    },
                ],
                properties: LayerProperties::default(),
            }],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    };

    let as2_source = r#"import flash.geom.Matrix;
stop();
var rotMc:MovieClip = createEmptyMovieClip("as2RotMc", 10);
rotMc.beginFill(0x3366cc, 100);
rotMc.moveTo(0, 0); rotMc.lineTo(100, 0); rotMc.lineTo(100, 40); rotMc.lineTo(0, 40); rotMc.endFill();
rotMc._rotation = 90;
var mcReadOk:Boolean = Math.abs(rotMc._width - 40) < 0.01 && Math.abs(rotMc._height - 100) < 0.01;
rotMc._width = 80; rotMc._height = 150;
var mcSetOk:Boolean = Math.abs(rotMc._rotation - 90) < 0.01 && Math.abs(rotMc._width - 80) < 0.01 && Math.abs(rotMc._height - 150) < 0.01;
var skewMc:MovieClip = createEmptyMovieClip("as2SkewMc", 11);
skewMc.beginFill(0xcc6633, 100);
skewMc.moveTo(0, 0); skewMc.lineTo(100, 0); skewMc.lineTo(100, 40); skewMc.lineTo(0, 40); skewMc.endFill();
skewMc.transform.matrix = new Matrix(1, 0, 0.5, 1, 0, 0);
var skewReadOk:Boolean = Math.abs(skewMc._width - 120) < 0.01 && Math.abs(skewMc._height - 40) < 0.01;
skewMc._width = 240;
var skewSetOk:Boolean = Math.abs(skewMc._width - 240) < 0.01 && Math.abs(skewMc.transform.matrix.c - 1.0) < 0.01;
rotMc.removeMovieClip();
skewMc.removeMovieClip();
var btnRef:Button = Button(this["authoredBtn"]);
var bw0:Number = btnRef._width; var bh0:Number = btnRef._height;
btnRef._rotation = 90;
var btnRotOk:Boolean = Math.abs(btnRef._width - bh0) < 0.01 && Math.abs(btnRef._height - bw0) < 0.01;
btnRef._width = bh0 * 2;
btnRotOk = btnRotOk && Math.abs(btnRef._rotation - 90) < 0.01 && Math.abs(btnRef._width - bh0 * 2) < 0.01;
btnRef._rotation = 0; btnRef._xscale = 100; btnRef._yscale = 100;
var tfRef:TextField = createTextField("as2Tf", 12, 0, 0, 120, 30);
tfRef._rotation = 90;
var tfRotOk:Boolean = Math.abs(tfRef._width - 30) < 0.01 && Math.abs(tfRef._height - 120) < 0.01;
tfRef._width = 60;
tfRotOk = tfRotOk && Math.abs(tfRef._rotation - 90) < 0.01 && Math.abs(tfRef._width - 60) < 0.01;
tfRef.removeTextField();
var shMc:MovieClip = createEmptyMovieClip("as2StrokeHolder", 20);
shMc._x = 40; shMc._y = 180; shMc._xscale = 600; shMc._yscale = 600;
var hLine:MovieClip = shMc.createEmptyMovieClip("line", 1);
hLine.lineStyle(0, 0x22d3ee, 100, true, "none", "square", "miter", 6);
hLine.moveTo(0, 4); hLine.lineTo(20, 4);
var lb:Object = hLine.getBounds(hLine); var lr:Object = hLine.getRect(hLine);
var strokeBoundsOk:Boolean = lr.yMin == 4 && lr.yMax == 4 && lb.yMin < 4 && lb.yMax > 4;
var gradMc:MovieClip = createEmptyMovieClip("as2Grad", 21);
gradMc._x = 40; gradMc._y = 280;
gradMc.beginGradientFill("linear", [0xff0000, 0x00ff00], [100, 100], [0, 255], {matrixType: "box", x: 0, y: 0, w: 60, h: 40, r: 0}, "reflect", "linearRGB", 0);
gradMc.moveTo(0, 0); gradMc.lineTo(180, 0); gradMc.lineTo(180, 40); gradMc.lineTo(0, 40); gradMc.lineTo(0, 0); gradMc.endFill();
var blendMc:MovieClip = createEmptyMovieClip("as2Blend", 22);
blendMc._x = 260; blendMc._y = 280;
var bBg:MovieClip = blendMc.createEmptyMovieClip("bg", 1);
bBg.beginFill(0xc86432, 100); bBg.moveTo(0, 0); bBg.lineTo(120, 0); bBg.lineTo(120, 60); bBg.lineTo(0, 60); bBg.endFill();
var bSub:MovieClip = blendMc.createEmptyMovieClip("sub", 2);
bSub.beginFill(0x32141e, 100); bSub.moveTo(0, 0); bSub.lineTo(60, 0); bSub.lineTo(60, 60); bSub.lineTo(0, 60); bSub.endFill();
bSub.blendMode = 9;
var bInv:MovieClip = blendMc.createEmptyMovieClip("inv", 3);
bInv.beginFill(0xffffff, 100); bInv.moveTo(60, 0); bInv.lineTo(120, 0); bInv.lineTo(120, 60); bInv.lineTo(60, 60); bInv.endFill();
bInv.blendMode = 10;
var blendNameOk:Boolean = bSub.blendMode == "subtract" && bInv.blendMode == "invert";
this["as2Ok"] = mcReadOk && mcSetOk && skewReadOk && skewSetOk && btnRotOk && tfRotOk && strokeBoundsOk && blendNameOk;
"#;

    let as3_source = r#"import flash.display.BlendMode;
import flash.display.CapsStyle;
import flash.display.InterpolationMethod;
import flash.display.JointStyle;
import flash.display.LineScaleMode;
import flash.display.Shape;
import flash.display.SimpleButton;
import flash.display.SpreadMethod;
import flash.display.Sprite;
import flash.events.MouseEvent;
import flash.geom.Matrix;
import flash.geom.Rectangle;
import flash.media.SoundTransform;
import flash.text.TextField;
stop();
var authored:SimpleButton = this.getChildByName("authoredBtn") as SimpleButton;
var authSlotsOk:Boolean = authored != null && authored.upState != null && authored.overState != null && authored.downState != null && authored.hitTestState != null;
var st:SoundTransform = new SoundTransform(0.5, -0.5);
authored.soundTransform = st;
st.volume = 1.0;
var stOk:Boolean = Math.abs(authored.soundTransform.volume - 0.5) < 0.001 && Math.abs(authored.soundTransform.pan + 0.5) < 0.001;
var upS:Shape = new Shape(); upS.graphics.beginFill(0x22c55e, 1); upS.graphics.drawRect(0, 0, 120, 60); upS.graphics.endFill();
var overS:Shape = new Shape(); overS.graphics.beginFill(0xf59e0b, 1); overS.graphics.drawRect(0, 0, 120, 60); overS.graphics.endFill();
var downS:Shape = new Shape(); downS.graphics.beginFill(0xef4444, 1); downS.graphics.drawRect(0, 0, 120, 60); downS.graphics.endFill();
var hitS:Shape = new Shape(); hitS.graphics.beginFill(0xffffff, 1); hitS.graphics.drawRect(10, 5, 100, 50); hitS.graphics.endFill();
var dynBtn:SimpleButton = new SimpleButton(upS, overS, downS, hitS);
dynBtn.x = 240; dynBtn.y = 40;
dynBtn.trackAsMenu = true;
dynBtn.soundTransform = new SoundTransform(0.4, 0.25);
addChild(dynBtn);
var clicks:int = 0;
dynBtn.addEventListener(MouseEvent.CLICK, function(e:MouseEvent):void {
  clicks += 1;
  var nextOver:Shape = new Shape();
  nextOver.graphics.beginFill(0x38bdf8, 1);
  nextOver.graphics.drawRect(0, 0, 120, 60);
  nextOver.graphics.endFill();
  dynBtn.overState = nextOver;
  trace("[T304-CLICK] dynBtn clicks=" + clicks);
});
var btnHitMissOk:Boolean = !dynBtn.hitTestPoint(242, 42, true) && dynBtn.hitTestPoint(290, 65, true) && dynBtn.trackAsMenu === true;
var zeroSp:Sprite = new Sprite(); zeroSp.rotation = 30; zeroSp.width = 50;
var zeroOk:Boolean = zeroSp.scaleX == 0 && zeroSp.scaleY == 0;
var rotSp:Sprite = new Sprite(); rotSp.graphics.beginFill(0x4488cc, 1); rotSp.graphics.drawRect(0, 0, 100, 40); rotSp.graphics.endFill();
rotSp.rotation = 90;
var spRotOk:Boolean = Math.abs(rotSp.width - 40) < 0.01 && Math.abs(rotSp.height - 100) < 0.01;
rotSp.width = 80; rotSp.height = 150;
spRotOk = spRotOk && Math.abs(rotSp.rotation - 90) < 0.01 && Math.abs(rotSp.width - 80) < 0.01 && Math.abs(rotSp.height - 150) < 0.01;
var skewSp:Sprite = new Sprite(); skewSp.graphics.beginFill(0xcc8844, 1); skewSp.graphics.drawRect(0, 0, 100, 40); skewSp.graphics.endFill();
skewSp.transform.matrix = new Matrix(1, 0, 0.5, 1, 0, 0);
var spSkewOk:Boolean = Math.abs(skewSp.width - 120) < 0.01 && Math.abs(skewSp.height - 40) < 0.01;
skewSp.width = 240;
spSkewOk = spSkewOk && Math.abs(skewSp.width - 240) < 0.01 && Math.abs(skewSp.transform.matrix.c - 1.0) < 0.01;
var tf3:TextField = new TextField(); tf3.width = 120; tf3.height = 30; tf3.rotation = 90;
var tf3Ok:Boolean = Math.abs(tf3.width - 30) < 0.01 && Math.abs(tf3.height - 120) < 0.01;
tf3.width = 60;
tf3Ok = tf3Ok && Math.abs(tf3.rotation - 90) < 0.01 && Math.abs(tf3.width - 60) < 0.01;
var strokeHolder:Sprite = new Sprite();
strokeHolder.x = 220; strokeHolder.y = 180; strokeHolder.scaleX = 6; strokeHolder.scaleY = 6;
addChild(strokeHolder);
var hair:Shape = new Shape();
hair.graphics.lineStyle(0, 0xff3366, 1, true, LineScaleMode.NONE, CapsStyle.SQUARE, JointStyle.MITER, 6);
hair.graphics.moveTo(0, 4); hair.graphics.lineTo(20, 4);
strokeHolder.addChild(hair);
var normStroke:Shape = new Shape();
normStroke.graphics.lineStyle(2, 0xff3366, 1, false, LineScaleMode.NORMAL, CapsStyle.ROUND, JointStyle.ROUND, 3);
normStroke.graphics.moveTo(25, 4); normStroke.graphics.lineTo(45, 4);
strokeHolder.addChild(normStroke);
var hb:Rectangle = hair.getBounds(hair); var hr:Rectangle = hair.getRect(hair);
var hairBoundsOk:Boolean = hr.height == 0 && hb.height > 0;
var repGrad:Shape = new Shape(); repGrad.x = 40; repGrad.y = 360;
var gm:Matrix = new Matrix(); gm.createGradientBox(60, 40, 0, 0, 0);
repGrad.graphics.beginGradientFill("linear", [0x0000ff, 0xffff00], [1, 1], [0, 255], gm, SpreadMethod.REPEAT, InterpolationMethod.RGB, 0);
repGrad.graphics.drawRect(0, 0, 180, 40); repGrad.graphics.endFill();
addChild(repGrad);
var layerBack:Shape = new Shape(); layerBack.x = 420; layerBack.y = 280;
layerBack.graphics.beginFill(0xc86432, 1); layerBack.graphics.drawRect(0, 0, 180, 60); layerBack.graphics.endFill();
addChild(layerBack);
var layerGrp:Sprite = new Sprite(); layerGrp.x = 420; layerGrp.y = 280; layerGrp.blendMode = BlendMode.LAYER;
var lFill:Shape = new Shape(); lFill.graphics.beginFill(0x00dc64, 1); lFill.graphics.drawRect(0, 0, 180, 60); lFill.graphics.endFill();
layerGrp.addChild(lFill);
var lErase:Shape = new Shape(); lErase.graphics.beginFill(0x000000, 1); lErase.graphics.drawRect(0, 0, 60, 60); lErase.graphics.endFill();
lErase.blendMode = BlendMode.ERASE;
layerGrp.addChild(lErase);
var lAlpha:Shape = new Shape(); lAlpha.graphics.beginFill(0xffffff, 1); lAlpha.graphics.drawRect(0, 0, 120, 60); lAlpha.graphics.endFill();
lAlpha.blendMode = BlendMode.ALPHA;
layerGrp.addChild(lAlpha);
addChild(layerGrp);
var as2Passed:Boolean = Boolean(this["as2Ok"]);
var as3Passed:Boolean = authSlotsOk && stOk && btnHitMissOk && zeroOk && spRotOk && spSkewOk && tf3Ok && hairBoundsOk && layerGrp.blendMode == BlendMode.LAYER;
if (!as2Passed || !as3Passed) {
  throw new Error("T-304 gate failed: as2=" + as2Passed + " as3=" + as3Passed);
}
trace("[PASS] focus-t304-display-graphics as2=" + as2Passed + " as3=" + as3Passed);
"#;

    let root = mc_symbol(
        "scene",
        "Main",
        1,
        vec![single_key_layer(
            "ml",
            "Stage",
            vec![instance_el("abtn", "authoredBtn", "btn_t304", 40.0, 40.0)],
        )],
        vec![
            frame_script(0, ScriptLanguage::ActionScript2, as2_source),
            frame_script(0, ScriptLanguage::ActionScript3, as3_source),
        ],
    );
    let mut project = project_from_root(
        "Focus T-304 AS2+AS3 Display & Graphics",
        root,
        vec![btn_sym],
    );
    project.resources.push(
        ResourceUnit::from_bytes(
            "snd_btn",
            "audio/btn_click.wav",
            "audio/wav",
            &minimal_wav_bytes(),
        )
        .expect("valid button wav resource"),
    );
    project.upgrade_format_for_features();

    CaseSpec {
        id: "focus-t304-display-graphics",
        group: "focused",
        language: "as3",
        project,
        child_project: None,
        clean_chrome: false,
        allow_file_protocol: true,
        expected_pass: "[PASS] focus-t304-display-graphics as2=true as3=true",
        expected_error_substrings: vec![],
        interactions: vec![InteractionSpec {
            kind: "click",
            x: Some(290.0),
            y: Some(65.0),
            to_x: None,
            to_y: None,
            key: None,
            text: None,
            expect_trace: Some("[T304-CLICK] dynBtn clicks=1"),
        }],
        verify_restart: true,
    }
}

fn build_diagnostic_cases() -> Vec<CaseSpec> {
    // 1. AS2 mapped frame runtime error
    let as2_frame = CaseSpec {
        id: "diag-as2-frame-error",
        group: "diagnostic",
        language: "as2",
        project: project_from_root(
            "Diag AS2 Frame Error",
            mc_symbol(
                "scene",
                "Main",
                1,
                vec![single_key_layer("ml", "Stage", vec![])],
                vec![frame_script(
                    0,
                    ScriptLanguage::ActionScript2,
                    "var ok:Number = 1;\nthrow new Error('boom-as2-frame');",
                )],
            ),
            vec![],
        ),
        child_project: None,
        clean_chrome: false,
        allow_file_protocol: true,
        expected_pass: "",
        expected_error_substrings: vec!["boom-as2-frame", "Main / frame 1:2:"],
        interactions: vec![],
        verify_restart: false,
    };

    // 2. AS2 mapped #include runtime error
    let mut as2_inc_proj = project_from_root(
        "Diag AS2 Include Error",
        mc_symbol(
            "scene",
            "Main",
            1,
            vec![single_key_layer("ml", "Stage", vec![])],
            vec![frame_script(
                0,
                ScriptLanguage::ActionScript2,
                "#include \"includes/fault_as2.as\"\nthrowFromIncluded();",
            )],
        ),
        vec![],
    );
    as2_inc_proj.resources.push(
        ResourceUnit::from_bytes(
            "inc_fault",
            "includes/fault_as2.as",
            "text/plain",
            b"function throwFromIncluded():Void {\n  throw new Error('boom-as2-include');\n}\n",
        )
        .unwrap(),
    );
    let as2_inc = CaseSpec {
        id: "diag-as2-include-error",
        group: "diagnostic",
        language: "as2",
        project: as2_inc_proj,
        child_project: None,
        clean_chrome: false,
        allow_file_protocol: true,
        expected_pass: "",
        expected_error_substrings: vec!["boom-as2-include", "includes/fault_as2.as:2:"],
        interactions: vec![],
        verify_restart: false,
    };

    // 3. AS3 mapped SourceUnit class runtime error
    let mut as3_cls_proj = project_from_root(
        "Diag AS3 Class Error",
        mc_symbol(
            "scene",
            "Main",
            1,
            vec![single_key_layer("ml", "Stage", vec![])],
            vec![],
        ),
        vec![],
    );
    as3_cls_proj.document_class = Some("app.FaultyDoc".into());
    as3_cls_proj.sources.push(SourceUnit {
        id: "src_fault_as3".into(),
        path: "app/FaultyDoc.as".into(),
        language: ScriptLanguage::ActionScript3,
        source: r#"package app {
  import flash.display.MovieClip;
  public class FaultyDoc extends MovieClip {
    public function FaultyDoc() {
      super();
      throw new Error("boom-as3-class");
    }
  }
}
"#
        .into(),
    });
    let as3_cls = CaseSpec {
        id: "diag-as3-class-error",
        group: "diagnostic",
        language: "as3",
        project: as3_cls_proj,
        child_project: None,
        clean_chrome: false,
        allow_file_protocol: true,
        expected_pass: "",
        expected_error_substrings: vec!["boom-as3-class", "app/FaultyDoc.as:6:"],
        interactions: vec![],
        verify_restart: false,
    };

    // 4. AS3 mapped include runtime error
    let mut as3_inc_proj = project_from_root(
        "Diag AS3 Include Error",
        mc_symbol(
            "scene",
            "Main",
            1,
            vec![single_key_layer("ml", "Stage", vec![])],
            vec![frame_script(
                0,
                ScriptLanguage::ActionScript3,
                "include \"includes/fault_as3.as\";\nblowUpNow();",
            )],
        ),
        vec![],
    );
    as3_inc_proj.resources.push(
        ResourceUnit::from_bytes(
            "res_inc_fault",
            "includes/fault_as3.as",
            "text/plain",
            b"function blowUpNow():void {\n  throw new Error('boom-as3-include');\n}\n",
        )
        .unwrap(),
    );
    let as3_inc = CaseSpec {
        id: "diag-as3-include-error",
        group: "diagnostic",
        language: "as3",
        project: as3_inc_proj,
        child_project: None,
        clean_chrome: false,
        allow_file_protocol: true,
        expected_pass: "",
        expected_error_substrings: vec!["boom-as3-include", "includes/fault_as3.as:2:"],
        interactions: vec![],
        verify_restart: false,
    };

    vec![as2_frame, as2_inc, as3_cls, as3_inc]
}

fn load_authored_examples() -> Vec<CaseSpec> {
    let root = repo_root().join("examples");
    let mut out = Vec::new();
    for lang in ["as2", "as3"] {
        for slug in EXAMPLE_SLUGS {
            let file = root.join(lang).join(format!("{slug}.nflash"));
            let json = fs::read_to_string(&file).unwrap_or_else(|e| {
                panic!("read example {}: {e}", file.display());
            });
            let project: Project = serde_json::from_str(&json).unwrap_or_else(|e| {
                panic!("parse example {}: {e}", file.display());
            });
            let allow_file_protocol = *slug != "02-oop-xml-collections";
            let interactions = match *slug {
                "04-events-stage-align-drag" => vec![InteractionSpec {
                    kind: "drag",
                    x: Some(560.0),
                    y: Some(360.0),
                    to_x: Some(700.0),
                    to_y: Some(360.0),
                    key: None,
                    text: None,
                    expect_trace: None,
                }],
                "06-dynamic-input-html-text" => vec![InteractionSpec {
                    kind: "text_input",
                    x: Some(640.0),
                    y: Some(275.0),
                    to_x: None,
                    to_y: None,
                    key: None,
                    text: Some("OK"),
                    expect_trace: None,
                }],
                "08-interactive-arcade-miniapp" => vec![
                    InteractionSpec {
                        kind: "key",
                        x: None,
                        y: None,
                        to_x: None,
                        to_y: None,
                        key: Some("ArrowLeft"),
                        text: None,
                        expect_trace: None,
                    },
                    InteractionSpec {
                        kind: "key",
                        x: None,
                        y: None,
                        to_x: None,
                        to_y: None,
                        key: Some("KeyR"),
                        text: None,
                        expect_trace: None,
                    },
                ],
                _ => vec![],
            };
            let id = Box::leak(format!("{lang}-{slug}").into_boxed_str());
            let expected_pass = Box::leak(format!("[PASS] {lang}/{slug}").into_boxed_str());
            out.push(CaseSpec {
                id,
                group: "example",
                language: lang,
                project,
                child_project: None,
                clean_chrome: false,
                allow_file_protocol,
                expected_pass,
                expected_error_substrings: vec![],
                interactions,
                verify_restart: true,
            });
        }
    }
    out
}

fn build_all_focused_and_diag_cases() -> Vec<CaseSpec> {
    let mut cases = vec![
        build_focus_as2_frame_scripts(),
        build_focus_as3_frame_scripts(),
        build_focus_as2_object_handlers(),
        build_focus_as3_object_handlers(),
        build_focus_as2_linkage_class(),
        build_focus_as3_doc_and_symbol_class(),
        build_focus_as2_include_initclip(),
        build_focus_as3_include_swc(),
        build_focus_as2_scenes(),
        build_focus_as3_scenes(),
        build_focus_loadpublish_html(ScriptLanguage::ActionScript2),
        build_focus_loadpublish_html(ScriptLanguage::ActionScript3),
        build_focus_restart_reset(ScriptLanguage::ActionScript2),
        build_focus_restart_reset(ScriptLanguage::ActionScript3),
        build_focus_as3_clean_chrome(),
        build_focus_t304_display_graphics(),
    ];
    cases.extend(build_diagnostic_cases());
    cases
}

fn full_options(level: SizeLevel) -> PublishOptions {
    PublishOptions {
        fonts: FontExportPolicy::Embed,
        size: level,
        audio: AudioExportPolicy::Original,
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GateManifestCase {
    id: &'static str,
    group: &'static str,
    language: &'static str,
    stage_width: u32,
    stage_height: u32,
    levels: Vec<String>,
    clean_chrome: bool,
    allow_file_protocol: bool,
    expected_pass: &'static str,
    expected_error_substrings: Vec<&'static str>,
    interactions: Vec<InteractionSpec>,
    verify_restart: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GateManifest {
    tier: &'static str,
    shared_runtime_dir: String,
    cases: Vec<GateManifestCase>,
}

fn export_case_to_dir(case: &CaseSpec, base_dir: &Path, rt: &Path, levels: &[&str]) -> Vec<String> {
    let mut emitted_levels = Vec::new();
    let html_cfg = if case.clean_chrome {
        HtmlPublishConfig::clean_publish_default()
    } else {
        HtmlPublishConfig::default()
    };
    for &lvl in levels {
        let size = match lvl {
            "full" => SizeLevel::Full,
            "balanced" => SizeLevel::Balanced,
            "compact" => SizeLevel::Compact,
            "shared" => SizeLevel::Shared("../../../_shared_runtime/".into()),
            _ => unreachable!(),
        };
        let out_dir = base_dir.join(case.group).join(case.id).join(lvl);
        let opts = full_options(size);
        export_project_with_config(&case.project, &out_dir, rt, &opts, &html_cfg)
            .unwrap_or_else(|e| panic!("export {} [{lvl}] failed: {e}", case.id));
        assert!(
            out_dir.join("index.html").is_file(),
            "{} [{lvl}] index",
            case.id
        );
        assert!(
            out_dir.join("bundle.js").is_file(),
            "{} [{lvl}] bundle",
            case.id
        );
        if lvl != "shared" {
            assert!(
                out_dir.join("runtime.js").is_file(),
                "{} [{lvl}] runtime",
                case.id
            );
        }
        if let Some(child) = &case.child_project {
            let child_dir = out_dir.join("child");
            export_project_with_config(
                child,
                &child_dir,
                rt,
                &full_options(SizeLevel::Full),
                &HtmlPublishConfig::default(),
            )
            .unwrap_or_else(|e| panic!("export child for {} [{lvl}] failed: {e}", case.id));
        }
        emitted_levels.push(lvl.to_owned());
    }
    emitted_levels
}

#[test]
fn publish_gate_atomic_output_safety() {
    let temp = TempDir::new("atomic");
    let rt = runtime_dir();

    // Seed a valid baseline NeoFlash export directory to verify untouched byte-for-byte on failure.
    let valid_proj = build_focus_as2_frame_scripts().project;
    let existing_html = temp.0.join("existing-html");
    export_project_with_options(
        &valid_proj,
        &existing_html,
        &rt,
        &full_options(SizeLevel::Full),
    )
    .expect("seed existing export");
    let before_html = snapshot_tree(&existing_html);

    let unrelated = temp.0.join("unrelated-user-dir");
    fs::create_dir_all(&unrelated).unwrap();
    fs::write(unrelated.join("notes.txt"), b"keep me intact").unwrap();
    let before_unrelated = snapshot_tree(&unrelated);

    let desktop_root = temp.0.join("desktop-out");
    let existing_app = export_desktop_app(
        &valid_proj,
        &desktop_root,
        &rt,
        &full_options(SizeLevel::Full),
        &HtmlPublishConfig::clean_publish_default(),
    )
    .expect("seed existing desktop app");
    let before_desktop = snapshot_tree(&desktop_root);
    assert!(existing_app.is_dir());

    // Build all failure fixtures that must fail preflight/compile/options before touching disk.
    let mut bad_as2_syntax = valid_proj.clone();
    bad_as2_syntax.symbol_mut("scene").unwrap().timeline.scripts = vec![frame_script(
        0,
        ScriptLanguage::ActionScript2,
        "var x:Number = ;",
    )];

    let mut bad_as3_type = valid_proj.clone();
    bad_as3_type.symbol_mut("scene").unwrap().timeline.scripts = vec![frame_script(
        0,
        ScriptLanguage::ActionScript3,
        "var badVal:int = unknownGateSymbol();",
    )];

    let mut bad_missing_inc = valid_proj.clone();
    bad_missing_inc
        .symbol_mut("scene")
        .unwrap()
        .timeline
        .scripts = vec![frame_script(
        0,
        ScriptLanguage::ActionScript2,
        "#include \"missing/does_not_exist.as\"",
    )];

    let mut bad_doc_class = valid_proj.clone();
    bad_doc_class.document_class = Some("missing.UnboundMain".into());

    let mut bad_invalid_project = valid_proj.clone();
    bad_invalid_project.root = "missing_root_id".into();

    let bad_failures: Vec<(&str, &Project, PublishOptions)> = vec![
        ("as2-syntax", &bad_as2_syntax, full_options(SizeLevel::Full)),
        (
            "as3-compile",
            &bad_as3_type,
            full_options(SizeLevel::Balanced),
        ),
        (
            "missing-include",
            &bad_missing_inc,
            full_options(SizeLevel::Compact),
        ),
        (
            "missing-doc-class",
            &bad_doc_class,
            full_options(SizeLevel::Full),
        ),
        (
            "invalid-project",
            &bad_invalid_project,
            full_options(SizeLevel::Full),
        ),
        (
            "invalid-shared-url",
            &valid_proj,
            full_options(SizeLevel::Shared("ftp://bad-scheme/runtime".into())),
        ),
    ];

    for (label, proj, opts) in &bad_failures {
        let fresh = temp.0.join(format!("fresh-{label}"));
        let err = export_project_with_options(proj, &fresh, &rt, opts)
            .expect_err("failed options export must error");
        assert!(
            !fresh.exists(),
            "{label}: fresh output dir must not be created: {err}"
        );

        let fresh_cfg = temp.0.join(format!("fresh-cfg-{label}"));
        export_project_with_config(proj, &fresh_cfg, &rt, opts, &HtmlPublishConfig::default())
            .expect_err("failed config export must error");
        assert!(
            !fresh_cfg.exists(),
            "{label}: fresh cfg dir must not be created"
        );

        export_project_with_options(proj, &existing_html, &rt, opts)
            .expect_err("failed overwrite must error");
        assert_eq!(
            snapshot_tree(&existing_html),
            before_html,
            "{label}: existing NeoFlash export was mutated after failure"
        );

        export_project_with_options(proj, &unrelated, &rt, opts)
            .expect_err("failed unrelated export must error");
        assert_eq!(
            snapshot_tree(&unrelated),
            before_unrelated,
            "{label}: unrelated directory was mutated after failure"
        );

        let fresh_desktop = temp.0.join(format!("fresh-desktop-{label}"));
        export_desktop_app(
            proj,
            &fresh_desktop,
            &rt,
            opts,
            &HtmlPublishConfig::clean_publish_default(),
        )
        .expect_err("failed fresh desktop export must error");
        assert!(
            !fresh_desktop.exists(),
            "{label}: fresh desktop dir must not be created before compilation"
        );

        export_desktop_app(
            proj,
            &desktop_root,
            &rt,
            opts,
            &HtmlPublishConfig::clean_publish_default(),
        )
        .expect_err("failed desktop overwrite must error");
        assert_eq!(
            snapshot_tree(&desktop_root),
            before_desktop,
            "{label}: existing desktop app was mutated after failure"
        );
    }

    // Also verify invalid PublishProfile / broken custom HTML template never creates or mutates output.
    let mut bad_profile = PublishProfile::new("GateProfile", full_options(SizeLevel::Full));
    bad_profile.html_template = Some("<html><body>missing placeholders</body></html>".into());
    let fresh_prof = temp.0.join("fresh-profile");
    export_project_with_profile(&valid_proj, &fresh_prof, &rt, &bad_profile)
        .expect_err("bad custom HTML wrapper must fail");
    assert!(!fresh_prof.exists());
    export_project_with_profile(&valid_proj, &existing_html, &rt, &bad_profile)
        .expect_err("bad custom HTML wrapper must not mutate existing export");
    assert_eq!(snapshot_tree(&existing_html), before_html);

    // Non-empty unrelated folder must be rejected cleanly by export_project_with_options.
    let unrelated_err =
        export_project_with_options(&valid_proj, &unrelated, &rt, &full_options(SizeLevel::Full))
            .expect_err("must reject unrelated non-empty directory");
    assert!(matches!(unrelated_err, ExportError::Destination(_)));
    assert_eq!(snapshot_tree(&unrelated), before_unrelated);
}

#[test]
fn publish_gate_mapped_diagnostics_and_source_maps() {
    let temp = TempDir::new("diag");
    let rt = runtime_dir();

    // 1. Compile-time diagnostic mapping assertions (AS2/AS3 frame, #include, ObjectScript, SourceUnit)
    let mut compile_checks: Vec<(&str, Project, &str, usize)> = Vec::new();

    compile_checks.push((
        "as2-frame-compile",
        project_from_root(
            "Compile AS2 Frame",
            mc_symbol(
                "scene",
                "Main",
                1,
                vec![single_key_layer("ml", "Stage", vec![])],
                vec![frame_script(
                    0,
                    ScriptLanguage::ActionScript2,
                    "var ok = 1;\nvar bad = ;",
                )],
            ),
            vec![],
        ),
        "Main / frame 1",
        2,
    ));

    let mut as2_inc_bad = project_from_root(
        "Compile AS2 Include",
        mc_symbol(
            "scene",
            "Main",
            1,
            vec![single_key_layer("ml", "Stage", vec![])],
            vec![frame_script(
                0,
                ScriptLanguage::ActionScript2,
                "#include \"includes/bad_inc.as\"",
            )],
        ),
        vec![],
    );
    as2_inc_bad.resources.push(
        ResourceUnit::from_bytes(
            "res_bad_inc",
            "includes/bad_inc.as",
            "text/plain",
            b"var a = 1;\nvar broken = );\n",
        )
        .unwrap(),
    );
    compile_checks.push(("as2-include-compile", as2_inc_bad, "includes/bad_inc.as", 2));

    let mut btn_el = rect_el("b0", "btnClip", 20.0, 20.0, 80.0, 40.0, [255, 0, 0, 255]);
    btn_el.scripts = vec![obj_script(
        ObjectEvent::Click,
        ScriptLanguage::ActionScript2,
        "var x = 1;\nvar y = ;",
    )];
    compile_checks.push((
        "as2-obj-compile",
        project_from_root(
            "Compile AS2 Object",
            mc_symbol(
                "scene",
                "Main",
                1,
                vec![single_key_layer("ml", "Interactions", vec![btn_el])],
                vec![],
            ),
            vec![],
        ),
        "Main / Interactions / frame 1 / btnClip / Click",
        2,
    ));

    let mut as3_cls_bad = project_from_root(
        "Compile AS3 Class",
        mc_symbol(
            "scene",
            "Main",
            1,
            vec![single_key_layer("ml", "Stage", vec![])],
            vec![],
        ),
        vec![],
    );
    as3_cls_bad.document_class = Some("app.BrokenMain".into());
    as3_cls_bad.sources.push(SourceUnit {
        id: "s1".into(),
        path: "app/BrokenMain.as".into(),
        language: ScriptLanguage::ActionScript3,
        source: "package app {\n  import flash.display.MovieClip;\n  public class BrokenMain extends MovieClip {\n    public function BrokenMain() {\n      definitelyMissingFn();\n    }\n  }\n}\n".into(),
    });
    compile_checks.push(("as3-class-compile", as3_cls_bad, "app/BrokenMain.as", 5));

    for (label, proj, expected_path, expected_line) in compile_checks {
        let diags: Vec<Diagnostic> = check_project(&proj).expect_err("expected compile diagnostic");
        assert!(
            diags.iter().any(|d| {
                d.details.path.as_deref() == Some(expected_path) && d.line == expected_line
            }),
            "{label}: expected path='{expected_path}' line={expected_line}, got: {diags:#?}"
        );
    }

    // 2. Runtime diagnostic exports preserve mapped source_locations / source_map metadata in bundle.js.
    for case in build_diagnostic_cases() {
        check_project(&case.project).expect("runtime diagnostic project must compile");
        let out_dir = temp.0.join(case.id);
        export_project_with_options(&case.project, &out_dir, &rt, &full_options(SizeLevel::Full))
            .expect("export diagnostic project");
        let bundle = bundle_json_from_file(&out_dir.join("bundle.js"));
        let mut all_sources = Vec::<String>::new();
        if let Some(arr) = bundle["program"]["source_locations"].as_array() {
            for loc in arr {
                if let Some(s) = loc["source"].as_str() {
                    all_sources.push(s.to_owned());
                }
            }
        }
        if let Some(scripts) = bundle["scripts"].as_array() {
            for sc in scripts {
                if let Some(arr) = sc["source_locations"].as_array() {
                    for loc in arr {
                        if let Some(s) = loc["source"].as_str() {
                            all_sources.push(s.to_owned());
                        }
                    }
                }
            }
        }
        let expected_file = case.expected_error_substrings[1]
            .split(':')
            .next()
            .expect("file prefix");
        assert!(
            all_sources.iter().any(|s| s == expected_file),
            "{}: expected mapped source '{expected_file}' in bundle source_locations, got: {all_sources:?}",
            case.id
        );
    }
}

#[test]
fn publish_gate_exports_and_runtime_contracts() {
    let rt = runtime_dir();
    let export_dir_env = std::env::var_os("NEOFLASH_PUBLISH_GATE_DIR").map(|raw| {
        let p = PathBuf::from(raw);
        if p.is_relative() {
            repo_root().join(p)
        } else {
            p
        }
    });
    let temp = TempDir::new("matrix");
    let out_root = export_dir_env.as_deref().unwrap_or(temp.0.as_path());
    let tier = match std::env::var("NEOFLASH_CHECK_TIER").as_deref() {
        Ok("release") => "release",
        Ok("standard") if export_dir_env.is_some() => "standard",
        Ok("standard") | Err(_) => "workspace",
        Ok(other) => panic!("unknown check tier {other:?}"),
    };

    let shared_dir = out_root.join("_shared_runtime");
    export_shared_runtime(&shared_dir, &rt).expect("export shared runtime");

    let examples = load_authored_examples();
    assert_eq!(examples.len(), 18, "expected all 18 AS2 + AS3 examples");

    let focused_and_diag = build_all_focused_and_diag_cases();
    let mut manifest_cases = Vec::new();

    let mut all_cases = examples;
    all_cases.extend(focused_and_diag);

    // Run every behaviour in the standard browser gate. Rotate representative
    // projects through the four levels and reserve the cross-product for release.
    // Safari's smoke cases, if available, use Full.
    let case_levels: Vec<&[&str]> = all_cases
        .iter()
        .map(|case| -> &[&str] {
            if tier == "release" {
                return &["full", "balanced", "compact", "shared"];
            }
            match case.id {
                "focus-as2-frame-scripts" | "focus-as3-frame-scripts" => &["balanced"],
                "focus-as2-include-initclip" | "focus-as3-include-swc" => &["compact"],
                "focus-as2-restart-reset" | "focus-as3-restart-reset" => &["shared"],
                "focus-as2-scenes"
                | "focus-as3-scenes"
                | "focus-as3-clean-chrome"
                | "as2-07-sound-video-loadpublish"
                | "as3-07-sound-video-loadpublish" => &["full"],
                _ if tier == "standard" => &["full"],
                _ => &[],
            }
        })
        .collect();

    // Validate & typecheck every case first, then fan out independent (case, level) exports.
    for case in &all_cases {
        case.project
            .validate()
            .unwrap_or_else(|e| panic!("{} validate failed: {e}", case.id));
        let program = check_project(&case.project)
            .unwrap_or_else(|e| panic!("{} check_project failed: {e:?}", case.id));
        if case.group == "example" {
            assert!(
                !program.code.is_empty() || !program.class_names.is_empty(),
                "{} produced an empty script program",
                case.id
            );
            let warnings = check_project_warnings(&case.project);
            let unsupported: Vec<_> = warnings
                .iter()
                .filter(|w| w.code == "unsupported-feature")
                .collect();
            assert!(unsupported.is_empty(), "{}: {unsupported:?}", case.id);
        }
    }

    let mut export_jobs: Vec<(&CaseSpec, &str)> = Vec::new();
    for (case, levels) in all_cases.iter().zip(case_levels.iter()) {
        if levels.is_empty() {
            continue;
        }
        for &lvl in *levels {
            export_jobs.push((case, lvl));
        }
    }
    let next_job = AtomicU64::new(0);
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, 6)
        .min(export_jobs.len().max(1));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let idx = next_job.fetch_add(1, Ordering::Relaxed) as usize;
                    if idx >= export_jobs.len() {
                        break;
                    }
                    let (case, lvl) = export_jobs[idx];
                    export_case_to_dir(case, out_root, &rt, &[lvl]);
                }
            });
        }
    });

    for (case, levels) in all_cases.iter().zip(case_levels.iter()) {
        if case.group == "example" && case.id.ends_with("07-sound-video-loadpublish") {
            let b = bundle_json_from_file(
                &out_root
                    .join(case.group)
                    .join(case.id)
                    .join("full/bundle.js"),
            );
            let resources = b["project"]["resources"].as_array().expect("resources");
            if resources.iter().any(|r| r["path"] == "media/clip.mp4") {
                assert_eq!(
                    b["resource_aliases"]["media/clip.avi"].as_str(),
                    Some("media/clip.mp4"),
                    "{}: transcoded AVI must preserve media/clip.avi alias in bundle",
                    case.id
                );
            }
        }
        if case.id == "focus-as2-scenes" || case.id == "focus-as3-scenes" {
            let b = bundle_json_from_file(
                &out_root
                    .join(case.group)
                    .join(case.id)
                    .join("full/bundle.js"),
            );
            let scenes = b["scenes"].as_array().expect("scenes array in bundle");
            assert_eq!(scenes.len(), 2, "{}: expected 2 scenes in bundle", case.id);
        }
        if case.id == "focus-as3-clean-chrome" {
            let html = fs::read_to_string(
                out_root
                    .join(case.group)
                    .join(case.id)
                    .join("full/index.html"),
            )
            .expect("read clean chrome html");
            assert!(
                html.contains("<footer hidden>") && html.contains(r#"id="status" hidden"#),
                "clean chrome must hide status/restart bar"
            );
        }
        manifest_cases.push(GateManifestCase {
            id: case.id,
            group: case.group,
            language: case.language,
            stage_width: case.project.width,
            stage_height: case.project.height,
            levels: levels.iter().map(|s| (*s).to_owned()).collect(),
            clean_chrome: case.clean_chrome,
            allow_file_protocol: case.allow_file_protocol,
            expected_pass: case.expected_pass,
            expected_error_substrings: case.expected_error_substrings.clone(),
            interactions: case.interactions.clone(),
            verify_restart: case.verify_restart,
        });
    }

    let manifest = GateManifest {
        tier,
        shared_runtime_dir: "_shared_runtime".into(),
        cases: manifest_cases,
    };
    fs::create_dir_all(out_root).expect("create gate out_root");
    fs::write(
        out_root.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).expect("serialize manifest"),
    )
    .expect("write manifest.json");
}

#[test]
fn t304_representative_project_native_render_and_hit_test() {
    let case = build_focus_t304_display_graphics();
    let project = case.project;
    project.validate().expect("T-304 project validates");
    check_project(&project).expect("T-304 project scripts compile cleanly");

    let mut player = Player::new(&project);
    let snap0 = player.runtime_snapshot(&project);
    let root = snap0
        .objects
        .iter()
        .find(|o| o.parent_ref.is_none())
        .expect("root object")
        .object_ref
        .clone();
    let authored_btn = snap0
        .objects
        .iter()
        .find(|o| o.display.name == "authoredBtn")
        .expect("authoredBtn on stage")
        .object_ref
        .clone();

    // 1. Authored button hitTestState, slot materialization, and soundTransform pan-law gain scaling.
    assert_eq!(
        player.hit_test_snapshot_point(&project, 80.0, 60.0),
        Some(authored_btn.clone())
    );
    player
        .object_set_button_sound_transform(&project, &authored_btn, 0.5, -1.0)
        .expect("set button soundTransform");
    let _ = player.drain_sound_commands();
    assert!(player.object_button_state(&project, &authored_btn, 1));
    let cmds = player.drain_sound_commands();
    assert_eq!(cmds.len(), 1, "entering Over state emits button sound");
    match &cmds[0] {
        SoundCommand::Play { envelope, .. } => {
            assert!((envelope[0][1] - 0.5).abs() < 1e-6);
            assert!(envelope[0][2].abs() < 1e-6);
        }
        other => panic!("unexpected sound command: {other:?}"),
    }
    let up_slot = player
        .object_button_slot(&project, &authored_btn, 0)
        .expect("read upState")
        .expect("materialized upState");
    assert!(player.object_alive(&up_slot));

    // Dynamic SimpleButton with narrower hitTestState than visual upState + live slot override.
    let dyn_btn = player.create_display(DynamicKind::Button).unwrap();
    assert!(player.object_set_property(&dyn_btn, "x", 240.0));
    assert!(player.object_set_property(&dyn_btn, "y", 40.0));
    player
        .display_mutate(
            &project,
            DisplayMutation::Add {
                parent: root.clone(),
                child: dyn_btn.clone(),
                index: None,
            },
        )
        .unwrap();
    let make_rect = |player: &mut Player, color: u32, x: f64, y: f64, w: f64, h: f64| {
        let sh = player.create_display(DynamicKind::Shape).unwrap();
        player
            .graphics_apply(
                &sh,
                &[
                    GraphicsOp::BeginFill { color, alpha: 1.0 },
                    GraphicsOp::DrawRect {
                        x,
                        y,
                        width: w,
                        height: h,
                    },
                    GraphicsOp::EndFill,
                ],
            )
            .unwrap();
        sh
    };
    let up_s = make_rect(&mut player, 0x22c55e, 0.0, 0.0, 120.0, 60.0);
    let over_s = make_rect(&mut player, 0x38bdf8, 0.0, 0.0, 120.0, 60.0);
    let hit_s = make_rect(&mut player, 0xffffff, 10.0, 5.0, 100.0, 50.0);
    player
        .object_set_button_slot(&project, &dyn_btn, 0, Some(up_s))
        .unwrap();
    player
        .object_set_button_slot(&project, &dyn_btn, 1, Some(over_s))
        .unwrap();
    player
        .object_set_button_slot(&project, &dyn_btn, 3, Some(hit_s))
        .unwrap();
    assert_eq!(player.hit_test_snapshot_point(&project, 242.0, 42.0), None);
    assert_eq!(
        player.hit_test_snapshot_point(&project, 290.0, 65.0),
        Some(dyn_btn.clone())
    );
    assert!(player.object_button_state(&project, &dyn_btn, 1));

    // 2. Hairline stroke under 6x container scale stays 1px crisp.
    let stroke_holder = player.create_display(DynamicKind::Sprite).unwrap();
    assert!(player.object_set_property(&stroke_holder, "x", 220.0));
    assert!(player.object_set_property(&stroke_holder, "y", 180.0));
    assert!(player.object_set_property(&stroke_holder, "scale_x", 6.0));
    assert!(player.object_set_property(&stroke_holder, "scale_y", 6.0));
    player
        .display_mutate(
            &project,
            DisplayMutation::Add {
                parent: root.clone(),
                child: stroke_holder.clone(),
                index: None,
            },
        )
        .unwrap();
    let hair = player.create_display(DynamicKind::Shape).unwrap();
    player
        .graphics_apply(
            &hair,
            &[
                GraphicsOp::LineStyle {
                    width: Some(0.0),
                    color: 0xff3366,
                    alpha: 1.0,
                    pixel_hinting: true,
                    scale_mode: LineScaleMode::None,
                    caps: LineCaps::Square,
                    joints: LineJoints::Miter,
                    miter_limit: 6.0,
                },
                GraphicsOp::MoveTo { x: 0.0, y: 4.0 },
                GraphicsOp::LineTo { x: 20.0, y: 4.0 },
            ],
        )
        .unwrap();
    player
        .display_mutate(
            &project,
            DisplayMutation::Add {
                parent: stroke_holder,
                child: hair,
                index: None,
            },
        )
        .unwrap();

    // 3. Reflect + linearRGB gradient and Repeat gradient.
    let grad_sh = player.create_display(DynamicKind::Shape).unwrap();
    assert!(player.object_set_property(&grad_sh, "x", 40.0));
    assert!(player.object_set_property(&grad_sh, "y", 280.0));
    player
        .graphics_apply(
            &grad_sh,
            &[
                GraphicsOp::BeginGradientFill {
                    gradient: GradientSpec {
                        kind: GradientKind::Linear,
                        colors: vec![0xff0000, 0x00ff00],
                        alphas: vec![1.0, 1.0],
                        ratios: vec![0.0, 255.0],
                        matrix: [60.0 / 1638.4, 0.0, 0.0, 40.0 / 1638.4, 30.0, 20.0],
                        spread_method: "reflect".into(),
                        interpolation_method: "linearRGB".into(),
                        focal_point_ratio: 0.0,
                    },
                },
                GraphicsOp::DrawRect {
                    x: 0.0,
                    y: 0.0,
                    width: 180.0,
                    height: 40.0,
                },
                GraphicsOp::EndFill,
            ],
        )
        .unwrap();
    player
        .display_mutate(
            &project,
            DisplayMutation::Add {
                parent: root.clone(),
                child: grad_sh,
                index: None,
            },
        )
        .unwrap();

    // 4. Subtract, Invert, and Layer + Erase + Alpha blend modes in native compositor.
    let make_mc_rect = |player: &mut Player, color: u32, x: f64, y: f64, w: f64, h: f64| {
        let mc = player.create_display(DynamicKind::MovieClip).unwrap();
        player
            .graphics_apply(
                &mc,
                &[
                    GraphicsOp::BeginFill { color, alpha: 1.0 },
                    GraphicsOp::DrawRect {
                        x,
                        y,
                        width: w,
                        height: h,
                    },
                    GraphicsOp::EndFill,
                ],
            )
            .unwrap();
        mc
    };
    let blend_bg = make_mc_rect(&mut player, 0xc86432, 260.0, 280.0, 120.0, 60.0);
    let sub_sh = make_mc_rect(&mut player, 0x32141e, 260.0, 280.0, 60.0, 60.0);
    let inv_sh = make_mc_rect(&mut player, 0xffffff, 320.0, 280.0, 60.0, 60.0);
    for ch in [&blend_bg, &sub_sh, &inv_sh] {
        player
            .display_mutate(
                &project,
                DisplayMutation::Add {
                    parent: root.clone(),
                    child: ch.clone(),
                    index: None,
                },
            )
            .unwrap();
    }
    player
        .as2_render_command(
            &project,
            &sub_sh,
            serde_json::json!({"op":"set","name":"blendMode","value":9}),
        )
        .unwrap();
    player
        .as2_render_command(
            &project,
            &inv_sh,
            serde_json::json!({"op":"set","name":"blendMode","value":10}),
        )
        .unwrap();

    let layer_bg = make_rect(&mut player, 0xc86432, 420.0, 280.0, 180.0, 60.0);
    player
        .display_mutate(
            &project,
            DisplayMutation::Add {
                parent: root.clone(),
                child: layer_bg,
                index: None,
            },
        )
        .unwrap();
    let layer_grp = player.create_display(DynamicKind::Sprite).unwrap();
    assert!(player.object_set_property(&layer_grp, "x", 420.0));
    assert!(player.object_set_property(&layer_grp, "y", 280.0));
    player
        .display_mutate(
            &project,
            DisplayMutation::Add {
                parent: root.clone(),
                child: layer_grp.clone(),
                index: None,
            },
        )
        .unwrap();
    player
        .as2_render_command(
            &project,
            &layer_grp,
            serde_json::json!({"op":"set","as3":true,"name":"blendMode","value":"layer"}),
        )
        .unwrap();
    let l_fill = make_rect(&mut player, 0x00dc64, 0.0, 0.0, 180.0, 60.0);
    let l_erase = make_rect(&mut player, 0x000000, 0.0, 0.0, 60.0, 60.0);
    let l_alpha = make_rect(&mut player, 0xffffff, 0.0, 0.0, 120.0, 60.0);
    for ch in [&l_fill, &l_erase, &l_alpha] {
        player
            .display_mutate(
                &project,
                DisplayMutation::Add {
                    parent: layer_grp.clone(),
                    child: ch.clone(),
                    index: None,
                },
            )
            .unwrap();
    }
    player
        .as2_render_command(
            &project,
            &l_erase,
            serde_json::json!({"op":"set","as3":true,"name":"blendMode","value":"erase"}),
        )
        .unwrap();
    player
        .as2_render_command(
            &project,
            &l_alpha,
            serde_json::json!({"op":"set","as3":true,"name":"blendMode","value":"alpha"}),
        )
        .unwrap();

    let rgba = player
        .render_snapshot_rgba(&project, project.width, project.height)
        .expect("native render_snapshot_rgba");
    let px = |x: u32, y: u32| -> [u8; 4] {
        let i = ((y * project.width + x) * 4) as usize;
        [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
    };
    // Hovered dynamic SimpleButton shows sky-blue overState [56, 189, 248]
    assert_eq!(px(290, 65), [56, 189, 248, 255]);
    // Crisp 1px hairline at y = 180 + 6*4 = 204; y = 202 and y = 206 stay transparent
    assert_eq!(px(260, 204), [255, 51, 102, 255]);
    assert_eq!(px(260, 202)[3], 0);
    assert_eq!(px(260, 206)[3], 0);
    // Reflect + linearRGB gradient mirrors (40+30=70) and (40+89=129) with bright linearRGB yellow
    assert_eq!(px(70, 300), px(129, 300));
    assert!(px(70, 300)[0] > 170 && px(70, 300)[1] > 170);
    // Subtract [150, 80, 20] and Invert [55, 155, 205]
    assert_eq!(px(290, 310), [150, 80, 20, 255]);
    assert_eq!(px(350, 310), [55, 155, 205, 255]);
    // Layer + Erase + Alpha: erased left [200, 100, 50], kept middle [0, 220, 100], masked right [200, 100, 50]
    assert_eq!(px(450, 310), [200, 100, 50, 255]);
    assert_eq!(px(510, 310), [0, 220, 100, 255]);
    assert_eq!(px(570, 310), [200, 100, 50, 255]);
}
