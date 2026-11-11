
use super::*;
use neoflash_core::{
    BitmapCompression, BitmapExportSettings, FrameScript, GradientStop, ObjectScript,
    RectangleOptions, ResourceUnit, Script, SymbolKind,
};
use std::process::Command;

#[test]
fn bezier_path_conversion_elevates_quadratics_and_preserves_cubics_and_rounded_rects() {
    let cmds = vec![
        PathCommand::MoveTo { x: 0.0, y: 0.0 },
        PathCommand::QuadraticTo {
            cx: 30.0,
            cy: -60.0,
            x: 60.0,
            y: 0.0,
        },
        PathCommand::CubicTo {
            cx1: 60.0,
            cy1: 40.0,
            cx2: 0.0,
            cy2: 40.0,
            x: 0.0,
            y: 0.0,
        },
        PathCommand::Close,
    ];
    let contours = path_commands_to_ae_contours(&cmds, [10.0, 20.0]);
    assert_eq!(contours.len(), 1);
    let c = &contours[0];
    assert!(c.closed);
    // Duplicate closing vertex merged onto vertex 0 while keeping incoming tangent!
    assert_eq!(c.vertices.len(), 2);
    assert_eq!(c.vertices[0], [10.0, 20.0]);
    assert_eq!(c.vertices[1], [70.0, 20.0]);
    // Outgoing tangent from v0 for quadratic `(0,0) -> (30,-60) -> (60,0)` is `2/3 * (30, -60) = (20, -40)`
    assert!((c.out_tangents[0][0] - 20.0).abs() < 1e-6);
    assert!((c.out_tangents[0][1] - (-40.0)).abs() < 1e-6);
    // Incoming tangent at v0 from cubic closing segment `(cx2=0, cy2=40)` -> `(0, 40)`
    assert!((c.in_tangents[0][0] - 0.0).abs() < 1e-6);
    assert!((c.in_tangents[0][1] - 40.0).abs() < 1e-6);

    let mut rect_el = Element {
        id: "r1".into(),
        name: "Card".into(),
        transform: Transform::default(),
        kind: ElementKind::Rectangle {
            width: 120.0,
            height: 80.0,
            fill: [40, 120, 240, 255],
            stroke: [255, 255, 255, 255],
            stroke_width: 2.0,
        },
        ..Element::default()
    };
    rect_el.set_rectangle_options(RectangleOptions {
        corner_radii: [12.0, 12.0, 12.0, 12.0],
        corners_locked: true,
    });
    let (rcmds, _, _, _, _, _, _) = element_vector_commands(&rect_el).unwrap();
    let rcontours = path_commands_to_ae_contours(&rcmds, [0.0, 0.0]);
    assert_eq!(rcontours.len(), 1);
    assert!(rcontours[0].closed);
    assert_eq!(rcontours[0].vertices.len(), 8);
}

#[test]
fn aep_export_preserves_vectors_gradients_precomps_footage_and_compiles_scripts_to_ae_expressions()
{
    let mut project = Project::blank();
    project.name = "AE Interchange Suite".into();
    project.width = 800;
    project.height = 600;
    project.fps = 24;

    // Add a PNG bitmap resource with custom WebP publish compression (`T-251`)
    let mut img = image::RgbaImage::new(16, 16);
    for px in img.pixels_mut() {
        *px = image::Rgba([255, 80, 40, 255]);
    }
    let mut png_bytes = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut png_bytes),
        image::ImageFormat::Png,
    )
    .unwrap();
    let mut res =
        ResourceUnit::from_bytes("res-bmp", "sprites/badge.png", "image/png", &png_bytes).unwrap();
    res.bitmap_export = Some(BitmapExportSettings {
        compression: BitmapCompression::Webp,
        use_imported_jpeg_data: true,
        jpeg_quality: 85,
        allow_smoothing: true,
    });
    project.resources.push(res);

    // Add a nested Symbol (`sym-wheel`) with negative coordinates around (0, 0) + loop script `gotoAndPlay("spin")`
    let wheel_sym = Symbol {
        id: "sym-wheel".into(),
        name: "Wheel".into(),
        folder: "Rigs".into(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 12,
            layers: vec![Layer {
                id: "wl-1".into(),
                name: "Spokes".into(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    label: "spin".into(),
                    elements: vec![Element {
                        id: "spoke".into(),
                        name: "spoke_shape".into(),
                        transform: Transform {
                            x: -40.0,
                            y: -40.0,
                            ..Transform::default()
                        },
                        kind: ElementKind::Ellipse {
                            width: 80.0,
                            height: 80.0,
                            fill: [240, 200, 40, 255],
                            stroke: [20, 20, 30, 255],
                            stroke_width: 3.0,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                }],
                properties: Default::default(),
            }],
            scripts: vec![FrameScript {
                frame: 11,
                script: Script {
                    language: ScriptLanguage::ActionScript2,
                    source: "gotoAndPlay(\"spin\");".into(),
                },
            }],
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    };
    project.symbols.push(wheel_sym);

    // Configure Root scene with:
    // 1. Gradient PaintedPath vector shape + PropertyExpression (`sin(time * tau * freq) * amp`)
    // 2. Shape Tween morph layer (`TweenKind::Shape`)
    // 3. Nested `sym-wheel` instance with AS2 `onClipEvent(enterFrame) { _rotation += 6; _x += 3; }`
    // 4. Dynamic Text layer driven by frame script `hud_txt.text = "Frame " + _currentframe;`
    // 5. Bitmap layer referencing `res-bmp`
    let root_id = project.root.clone();
    let root_sym = project.symbol_mut(&root_id).unwrap();
    root_sym.timeline.frame_count = 24;
    root_sym.timeline.scripts.push(FrameScript {
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript2,
            source: "hud_txt.text = \"Frame \" + _currentframe;".into(),
        },
    });
    root_sym.timeline.layers = vec![
        Layer {
            id: "l-grad".into(),
            name: "Aurora Vector".into(),
            visible: true,
            locked: false,
            keyframes: vec![Keyframe {
                frame: 0,
                elements: vec![Element {
                    id: "el-grad".into(),
                    name: "aurora".into(),
                    transform: Transform {
                        x: 100.0,
                        y: 120.0,
                        ..Transform::default()
                    },
                    filters: vec![FilterSpec::Glow {
                        color: 0x00FFCC,
                        alpha: 0.8,
                        blur_x: 12.0,
                        blur_y: 12.0,
                        strength: 1.5,
                        quality: 2,
                        inner: false,
                        knockout: false,
                    }],
                    kind: ElementKind::PaintedPath {
                        commands: vec![
                            PathCommand::MoveTo { x: 0.0, y: 0.0 },
                            PathCommand::CubicTo {
                                cx1: 80.0,
                                cy1: -40.0,
                                cx2: 160.0,
                                cy2: 40.0,
                                x: 240.0,
                                y: 0.0,
                            },
                            PathCommand::LineTo { x: 240.0, y: 90.0 },
                            PathCommand::LineTo { x: 0.0, y: 90.0 },
                            PathCommand::Close,
                        ],
                        fill_rule: FillRule::NonZero,
                        fill: [0; 4],
                        stroke: [255, 255, 255, 200],
                        fill_gradient: Some(GradientPaint {
                            kind: GradientKind::Linear,
                            matrix: [0.15, 0.0, 0.0, 0.15, 120.0, 45.0],
                            stops: vec![
                                GradientStop {
                                    ratio: 0,
                                    color: [20, 220, 180, 255],
                                },
                                GradientStop {
                                    ratio: 255,
                                    color: [40, 80, 255, 255],
                                },
                            ],
                            focal_point_ratio: 0.0,
                            ..GradientPaint::default()
                        }),
                        stroke_gradient: None,
                        stroke_width: 2.0,
                    },
                    ..Element::default()
                }],
                property_expressions: vec![PropertyExpression {
                    property: TweenProperty::Y,
                    expression: "sin(time * tau * freq) * amp".into(),
                    enabled: true,
                    amplitude: 18.0,
                    frequency: 1.5,
                    ..PropertyExpression::default()
                }],
                ..Keyframe::default()
            }],
            properties: Default::default(),
        },
        Layer {
            id: "l-morph".into(),
            name: "Star Morph".into(),
            visible: true,
            locked: false,
            keyframes: vec![
                Keyframe {
                    frame: 0,
                    tween: Some(Easing::EaseInOut),
                    tween_kind: TweenKind::Shape,
                    elements: vec![Element {
                        id: "m1".into(),
                        name: String::new(),
                        transform: Transform {
                            x: 300.0,
                            y: 200.0,
                            ..Transform::default()
                        },
                        kind: ElementKind::Rectangle {
                            width: 60.0,
                            height: 60.0,
                            fill: [255, 90, 120, 255],
                            stroke: [0; 4],
                            stroke_width: 0.0,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                },
                Keyframe {
                    frame: 12,
                    elements: vec![Element {
                        id: "m2".into(),
                        name: String::new(),
                        transform: Transform {
                            x: 360.0,
                            y: 220.0,
                            ..Transform::default()
                        },
                        kind: ElementKind::Ellipse {
                            width: 90.0,
                            height: 90.0,
                            fill: [255, 90, 120, 255],
                            stroke: [0; 4],
                            stroke_width: 0.0,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                },
            ],
            properties: Default::default(),
        },
        Layer {
            id: "l-wheel".into(),
            name: "Wheel Rig".into(),
            visible: true,
            locked: false,
            keyframes: vec![Keyframe {
                frame: 0,
                elements: vec![Element {
                    id: "inst-wheel".into(),
                    name: "wheel_mc".into(),
                    transform: Transform {
                        x: 200.0,
                        y: 400.0,
                        ..Transform::default()
                    },
                    scripts: vec![ObjectScript {
                        event: ObjectEvent::EnterFrame,
                        script: Script {
                            language: ScriptLanguage::ActionScript2,
                            source: "onClipEvent(enterFrame) { _rotation += 6; _x += 3; }".into(),
                        },
                    }],
                    kind: ElementKind::Instance {
                        symbol_id: "sym-wheel".into(),
                        playback: InstancePlayback::Independent,
                    },
                    ..Element::default()
                }],
                ..Keyframe::default()
            }],
            properties: Default::default(),
        },
        Layer {
            id: "l-hud".into(),
            name: "HUD".into(),
            visible: true,
            locked: false,
            keyframes: vec![Keyframe {
                frame: 0,
                elements: vec![
                    Element {
                        id: "el-bmp".into(),
                        name: "badge".into(),
                        transform: Transform {
                            x: 40.0,
                            y: 40.0,
                            ..Transform::default()
                        },
                        kind: ElementKind::Bitmap {
                            resource_id: "res-bmp".into(),
                            width: 16,
                            height: 16,
                            smoothing: true,
                        },
                        ..Element::default()
                    },
                    Element {
                        id: "el-txt".into(),
                        name: "hud_txt".into(),
                        transform: Transform {
                            x: 70.0,
                            y: 56.0,
                            ..Transform::default()
                        },
                        kind: ElementKind::Text {
                            text: "Frame 1".into(),
                            font_family: "Helvetica".into(),
                            font_size: 20.0,
                            color: [255, 255, 255, 255],
                        },
                        ..Element::default()
                    },
                ],
                ..Keyframe::default()
            }],
            properties: Default::default(),
        },
    ];

    project.upgrade_format_for_features();
    project.validate().expect("valid test project");

    let dir = std::env::temp_dir().join(format!("nf-aep-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let aep_out = dir.join("aurora_interchange.aep");

    let report = export_project_aep(&project, &aep_out, &AepOptions::default())
        .expect("export_project_aep succeeds");

    assert!(report.aep_path.exists());
    assert!(report.aepx_path.as_ref().unwrap().exists());
    assert!(report.jsx_path.as_ref().unwrap().exists());
    assert_eq!(report.composition_count, 2);
    assert!(report.vector_shape_layer_count >= 3);
    assert_eq!(report.text_layer_count, 1);
    assert_eq!(report.precomp_layer_count, 1);
    assert_eq!(report.footage_layer_count, 1);
    assert!(
        report.expression_count >= 5,
        "expected Y PropertyExpression + wheel Pos/Rot/TimeRemap + HUD SourceText expressions, got {}",
        report.expression_count
    );

    // Verify per-bitmap WebP export into `<stem>_Footage/` (`T-251` + `T-246`)
    assert_eq!(report.footage_files.len(), 1);
    assert_eq!(
        report.footage_files[0].extension().and_then(|e| e.to_str()),
        Some("webp")
    );

    // Verify RIFX / Egg! binary container magic & chunks
    let aep_bytes = fs::read(&report.aep_path).unwrap();
    assert_eq!(&aep_bytes[0..4], b"RIFX");
    assert_eq!(&aep_bytes[8..12], b"Egg!");
    assert!(aep_bytes.windows(4).any(|w| w == b"Cdta"));
    assert!(aep_bytes.windows(4).any(|w| w == b"shap"));
    assert!(aep_bytes.windows(4).any(|w| w == b"expr"));

    // Verify `.aepx` XML contains VectorGroup Bezier paths & translated Expressions
    let xml = fs::read_to_string(report.aepx_path.as_ref().unwrap()).unwrap();
    assert!(xml.contains("<AfterEffectsProject"));
    assert!(xml.contains("<BezierPath"));
    assert!(xml.contains("<Expression property=\"Rotation\">"));
    assert!(xml.contains("<Expression property=\"TimeRemap\">"));
    assert!(xml.contains("<Expression property=\"SourceText\">"));

    // Verify `.jsx` ExtendScript parses AND executes cleanly against an AE ExtendScript DOM mock,
    // and verify every generated AE JavaScript Expression evaluates to exact runtime values!
    let jsx_path = report.jsx_path.as_ref().unwrap();
    let jsx_text = fs::read_to_string(jsx_path).unwrap();
    assert!(jsx_text.contains("ADBE Vector Shape - Group"));
    assert!(jsx_text.contains("ADBE Vector Graphic - G-Fill"));
    assert!(jsx_text.contains("ADBE Time Remapping"));

    let harness_path = dir.join("verify_ae_dom_and_expressions.cjs");
    let harness = r#"
const fs = require('fs');
const vm = require('vm');
const jsxFile = process.argv[2];
const code = fs.readFileSync(jsxFile, 'utf8');

const expressions = [];
function makeProp(name) {
  const children = {};
  return {
    name,
    value: { text: '' },
    numKeys: 0,
    expression: '',
    property(childName) {
      if (!children[childName]) children[childName] = makeProp(childName);
      return children[childName];
    },
    addProperty(childName) {
      const p = makeProp(childName);
      children[childName + '_' + Object.keys(children).length] = p;
      return p;
    },
    setValue(v) { this.value = v; },
    setValueAtTime(_t, v) { this.numKeys += 1; this.value = v; },
    setInterpolationTypeAtKey() {},
    setTemporalEaseAtKey() {},
    children,
  };
}

const createdComps = [];
const sandbox = {
  Math, Number, String, Boolean, Array, Object, parseFloat, parseInt, isNaN, isFinite,
  $: { fileName: jsxFile },
  Folder: function(p) { this.fsName = p; this.exists = true; },
  File: function(p) { this.fsName = p; this.parent = { fsName: require('path').dirname(p) }; this.exists = fs.existsSync(p); },
  ImportOptions: function(f) { this.file = f; },
  Shape: function() { this.vertices = []; this.inTangents = []; this.outTangents = []; this.closed = false; },
  MarkerValue: function(c) { this.comment = c; this.chapter = ''; this.duration = 0; },
  KeyframeEase: function(s, i) { this.speed = s; this.influence = i; },
  KeyframeInterpolationType: { HOLD: 'HOLD', BEZIER: 'BEZIER', LINEAR: 'LINEAR' },
  ParagraphJustification: { LEFT_JUSTIFY: 0, CENTER_JUSTIFY: 1, RIGHT_JUSTIFY: 2 },
  AutoOrientType: { ALONG_PATH: 1 },
  BlendingMode: new Proxy({}, { get: (_t, k) => k }),
  TrackMatteType: new Proxy({}, { get: (_t, k) => k }),
  app: {
    beginUndoGroup() {},
    endUndoGroup() {},
    newProject() {},
    project: {
      file: null,
      save() {},
      importFile(io) { return { file: io.file, mainSource: { loop: 1 }, parentFolder: null }; },
      items: {
        addFolder(name) { return { name, parentFolder: null }; },
        addComp(name, width, height, aspect, duration, frameRate) {
          const layers = [];
          const comp = {
            name, width, height, aspect, duration, frameRate, bgColor: [0,0,0],
            markerProperty: makeProp('Marker'),
            addGuide() {},
            openInViewer() {},
            layers: {
              items: layers,
              _mk(kind) {
                const root = makeProp('LayerRoot');
                const lyr = {
                  kind, name: '', startTime: 0, inPoint: 0, outPoint: duration,
                  enabled: true, locked: false, threeDLayer: false, timeRemapEnabled: false,
                  property: (n) => root.property(n),
                  _root: root,
                };
                layers.unshift(lyr);
                return lyr;
              },
              addShape() { return this._mk('shape'); },
              addText(t) { const l = this._mk('text'); l.property('ADBE Text Properties').property('ADBE Text Document').value = { text: t }; return l; },
              addBoxText() { return this._mk('boxText'); },
              add(item) { return this._mk('av:' + (item.name || 'footage')); },
              addNull() { return this._mk('null'); },
              addCamera(n) { const l = this._mk('camera'); l.name = n; return l; },
            },
          };
          createdComps.push(comp);
          return comp;
        },
      },
    },
  },
};

vm.runInNewContext(code, sandbox, { filename: 'aurora_interchange.jsx' });
if (createdComps.length !== 2) throw new Error('Expected 2 comps, got ' + createdComps.length);

// Collect all non-empty expressions across all layers and evaluate them at frame 0 and frame 6!
function collectExprs(comp, layer, prop, path) {
  if (prop.expression && prop.expression.trim().length > 0) {
    expressions.push({ comp: comp.name, layer: layer.name, prop: prop.name, value: prop.value, code: prop.expression });
  }
  for (const k of Object.keys(prop.children)) {
    collectExprs(comp, layer, prop.children[k], path + '/' + k);
  }
}
for (const c of createdComps) {
  for (const l of c.layers.items) {
    collectExprs(c, l, l._root, '');
  }
}
if (expressions.length < 5) throw new Error('Expected >= 5 expressions, got ' + expressions.length);

function evalAeExpr(exprObj, frame, fps) {
  const time = frame / fps;
  const baseVal = Array.isArray(exprObj.value) ? exprObj.value.slice() : (typeof exprObj.value === 'number' ? exprObj.value : [200, 400]);
  const ctx = {
    Math, Number, String, Boolean, Array, Object,
    time,
    inPoint: 0,
    outPoint: 1,
    value: baseVal,
    thisComp: { frameDuration: 1 / fps, duration: 24 / fps, width: 800, height: 600 },
    thisLayer: { index: 1, inPoint: 0, outPoint: 1 },
    timeToFrames: (t) => Math.floor((t === undefined ? time : t) * fps + 1e-6),
    framesToTime: (f) => f / fps,
    clamp: (v, a, b) => Math.min(Math.max(v, a), b),
    linear: (t, t1, t2, v1, v2) => v1 + ((t - t1) / Math.max(1e-6, t2 - t1)) * (v2 - v1),
    ease: (t, t1, t2, v1, v2) => v1 + ((t - t1) / Math.max(1e-6, t2 - t1)) * (v2 - v1),
    wiggle: () => baseVal,
  };
  return vm.runInNewContext(exprObj.code, ctx);
}

// 1. Verify wheel_mc Rotate Z expression (`_rotation += 6` => +36 deg at frame 6)
const wheelRot = expressions.find(e => e.layer.includes('wheel_mc') && e.prop === 'ADBE Rotate Z');
if (!wheelRot) throw new Error('Missing wheel_mc rotation expression in ' + JSON.stringify(expressions.map(e => e.layer + ':' + e.prop)));
wheelRot.value = 0;
const rot6 = evalAeExpr(wheelRot, 6, 24);
if (Math.abs(rot6 - 36) > 1e-3) throw new Error('Expected wheel_mc rot@6 = 36, got ' + rot6);

// 2. Verify wheel_mc Time Remap loop (`gotoAndPlay("spin")` at frame 11 -> wraps frame 14 back into [0..11])
const wheelTr = expressions.find(e => e.layer.includes('wheel_mc') && e.prop === 'ADBE Time Remapping');
if (!wheelTr) throw new Error('Missing wheel_mc Time Remapping expression');
const tr14 = evalAeExpr(wheelTr, 14, 24);
if (!(tr14 >= 0 && tr14 < 12 / 24)) throw new Error('Expected looped TimeRemap in [0, 0.5), got ' + tr14);

// 3. Verify hud_txt SourceText expression (`"Frame " + _currentframe` -> "Frame 7" at frame index 6)
const hudTxt = expressions.find(e => e.layer.includes('hud_txt') && e.prop === 'ADBE Text Document');
if (!hudTxt) throw new Error('Missing hud_txt SourceText expression');
const txt6 = evalAeExpr(hudTxt, 6, 24);
if (txt6 !== 'Frame 7') throw new Error('Expected hud_txt@6 = "Frame 7", got ' + JSON.stringify(txt6));

// 4. Verify Aurora Vector Y PropertyExpression (`sin(time * tau * freq) * amp`)
const auroraPos = expressions.find(e => e.layer.includes('Aurora Vector') && e.prop === 'ADBE Position');
if (!auroraPos) throw new Error('Missing Aurora Vector Position expression');
auroraPos.value = [100, 120];
const pos4 = evalAeExpr(auroraPos, 4, 24);
if (!Array.isArray(pos4) || Math.abs(pos4[0] - 100) > 1e-3 || Math.abs(pos4[1] - 120) < 1.0) {
  throw new Error('Expected oscillating Y on Aurora Vector, got ' + JSON.stringify(pos4));
}
"#;
    fs::write(&harness_path, harness).unwrap();
    let output = Command::new("node")
        .arg(&harness_path)
        .arg(jsx_path)
        .output()
        .expect("node must be available to run AE ExtendScript + Expression verification");
    assert!(
        output.status.success(),
        "AE ExtendScript & JS Expression runtime verification failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn t259_export_hardening_smoothing_false_banner_clicktag_and_aep_edge_cases() {
    // 1. `apply_bitmap_export_settings`: `allow_smoothing == false` must turn smoothing OFF even when element defaulted to `true`!
    let mut proj = Project::blank();
    let rgba = image::RgbaImage::from_pixel(2, 2, image::Rgba([240, 120, 40, 128]));
    let mut png_bytes = Vec::new();
    rgba.write_to(
        &mut std::io::Cursor::new(&mut png_bytes),
        image::ImageFormat::Png,
    )
    .unwrap();
    let mut res =
        ResourceUnit::from_bytes("bmp_sharp", "icons/---.png", "image/png", &png_bytes).unwrap();
    res.bitmap_export = Some(BitmapExportSettings {
        compression: BitmapCompression::LosslessPng,
        allow_smoothing: false,
        ..BitmapExportSettings::default()
    });
    proj.resources.push(res);
    let mut webp_bytes = Vec::new();
    rgba.write_to(
        &mut std::io::Cursor::new(&mut webp_bytes),
        image::ImageFormat::WebP,
    )
    .unwrap();
    proj.resources.push(
        ResourceUnit::from_bytes("bmp_webp_raw", "icons/raw.webp", "image/webp", &webp_bytes)
            .unwrap(),
    );
    proj.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: "bmp_el".into(),
            name: "constructor".into(),
            kind: ElementKind::Bitmap {
                resource_id: "bmp_sharp".into(),
                width: 2,
                height: 2,
                smoothing: true,
            },
            ..Element::default()
        });

    let mut derived = proj.clone();
    let (_, _) = crate::apply_bitmap_export_settings(&mut derived, 80);
    let ElementKind::Bitmap { smoothing, .. } =
        &derived.symbols[0].timeline.layers[0].keyframes[0].elements[0].kind
    else {
        panic!("expected bitmap");
    };
    assert!(
        !*smoothing,
        "allow_smoothing: false must turn bitmap smoothing OFF"
    );

    // 2. Banner `click_tag_url` rejects `javascript:` and escapes `</script>` in valid `https://` URLs
    let mut banner_proj = Project::blank();
    banner_proj.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: "cta".into(),
            kind: ElementKind::Rectangle {
                width: 100.0,
                height: 40.0,
                fill: [255, 80, 40, 255],
                stroke: [0, 0, 0, 0],
                stroke_width: 0.0,
            },
            ..Element::default()
        });
    let dir = std::env::temp_dir().join(format!("nf_aep_t259_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let js_err = crate::export_native_banner_html(
        &banner_proj,
        dir.join("bad_banner.html"),
        &crate::BannerOptions {
            click_tag_url: Some("javascript:alert(1)".into()),
            ..crate::BannerOptions::default()
        },
    );
    assert!(js_err.is_err(), "javascript: clickTag must be rejected");

    let safe_rep = crate::export_native_banner_html(
        &banner_proj,
        dir.join("safe_banner.html"),
        &crate::BannerOptions {
            click_tag_url: Some("https://example.com/?q=</script>".into()),
            ..crate::BannerOptions::default()
        },
    )
    .unwrap();
    let safe_html = fs::read_to_string(&safe_rep.html_path).unwrap();
    assert!(
        !safe_html.contains("?q=</script>"),
        "inline script literal must escape </script>"
    );

    // 3. AEP export handles WebP->PNG footage transcode, Object.prototype-colliding names (`constructor`),
    //    `var` / identifier-boundary `_currentframe`, and pipe/newline marker sanitization.
    proj.symbols[0].folder = "constructor/toString".into();
    proj.symbols[0].timeline.scripts.push(FrameScript {
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript2,
            source: "var my_currentframe = 10;\n// pipe | comment\n".into(),
        },
    });
    let report = export_project_aep(&proj, dir.join("hardened.aep"), &AepOptions::default())
        .expect("export hardened AEP");
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == "AE_WEBP_TRANSCODED_PNG"),
        "WebP bitmap should transcode to PNG footage: {:?}",
        report.diagnostics
    );
    let jsx = fs::read_to_string(report.jsx_path.unwrap()).unwrap();
    assert!(jsx.contains("Object.prototype.hasOwnProperty.call"));
    let _ = fs::remove_dir_all(&dir);
}
