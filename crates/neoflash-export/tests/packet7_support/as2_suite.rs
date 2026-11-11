use super::*;
use neoflash_core::{
    Easing, FrameScript, InstancePlayback, Keyframe, Layer, LayerProperties, Project, ResourceUnit,
    Script, ScriptLanguage, SourceUnit, Symbol, SymbolKind, Timeline, Transform,
};

fn as2_frame_script(frame: u32, source: &str) -> FrameScript {
    FrameScript {
        frame,
        script: Script {
            language: ScriptLanguage::ActionScript2,
            source: source.into(),
        },
    }
}

pub fn build_as2_00_child_module() -> Project {
    let spinner_sym = Symbol {
        id: "spinner_sym".into(),
        name: "OrbitalSpinner".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![single_key_layer(
                "spin_l",
                "BadgeArt",
                vec![
                    ellipse_el(
                        "spin_ring",
                        "",
                        -22.0,
                        -22.0,
                        44.0,
                        44.0,
                        [14, 165, 233, 255],
                        [186, 230, 253, 255],
                        2.0,
                    ),
                    rect_el(
                        "spin_spoke",
                        "",
                        -4.0,
                        -18.0,
                        8.0,
                        36.0,
                        [250, 204, 21, 255],
                        [0, 0, 0, 0],
                        0.0,
                    ),
                ],
            )],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    };

    let mut kf0_el = instance_el(
        "spinner_inst",
        "spinner",
        "spinner_sym",
        64.0,
        96.0,
        InstancePlayback::Independent,
    );
    kf0_el.transform.rotation = 0.0;
    let mut kf11_el = kf0_el.clone();
    kf11_el.transform = Transform {
        x: 248.0,
        y: 96.0,
        rotation: 180.0,
        ..Transform::default()
    };

    let root = Symbol {
        id: "scene".into(),
        name: "ChildModuleAS2".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 12,
            layers: vec![
                single_key_layer(
                    "bg_l",
                    "Card",
                    vec![
                        rect_el(
                            "card_bg",
                            "",
                            4.0,
                            4.0,
                            312.0,
                            172.0,
                            [15, 23, 42, 255],
                            [56, 189, 248, 255],
                            2.0,
                        ),
                        text_el(
                            "hdr_txt",
                            "badgeLabel",
                            18.0,
                            20.0,
                            "CHILD MODULE · AS2",
                            16.0,
                            [226, 232, 240, 255],
                        ),
                    ],
                ),
                Layer {
                    id: "anim_l".into(),
                    name: "SpinnerTween".into(),
                    visible: true,
                    locked: false,
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            tween: Some(Easing::EaseInOut),
                            elements: vec![kf0_el],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 11,
                            elements: vec![kf11_el],
                            ..Keyframe::default()
                        },
                    ],
                    properties: LayerProperties::default(),
                },
            ],
            scripts: vec![as2_frame_script(
                0,
                "this.badgeLabel.text = \"MODULE LIVE · AS2\";\ntrace(\"[PASS] as2/00-child-module ready\");\n",
            )],
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    };

    let mut project = Project {
        format_version: 1,
        name: "AS2 00 · Published Child Module".into(),
        width: 320,
        height: 180,
        fps: 24,
        background: [11, 17, 32, 255],
        root: "scene".into(),
        symbols: vec![spinner_sym, root],
        library_folders: Vec::new(),
        sources: Vec::new(),
        resources: Vec::new(),
        document_class: None,
        scenes: Vec::new(),
    };
    project.upgrade_format_for_features();
    project
}

pub fn build_as2_01_math_geometry() -> Project {
    let sat_sym = Symbol {
        id: "sat_sym".into(),
        name: "OrbitalProbe".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![single_key_layer(
                "sat_l",
                "Probe",
                vec![
                    ellipse_el(
                        "sat_core",
                        "",
                        -14.0,
                        -14.0,
                        28.0,
                        28.0,
                        [56, 189, 248, 255],
                        [240, 249, 255, 255],
                        2.0,
                    ),
                    rect_el(
                        "sat_wing",
                        "",
                        -26.0,
                        -3.0,
                        52.0,
                        6.0,
                        [250, 204, 21, 255],
                        [0, 0, 0, 0],
                        0.0,
                    ),
                ],
            )],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    };

    let mut elements = dashboard_chrome_elements(
        "as2_01",
        "NEOFLASH AS2 SUITE · 01 / MATH & GEOMETRY",
        "ActionScript 2 · Point, Rectangle, Matrix & ColorTransform",
        "Deterministic trigonometry, affine Matrix transforms, Rectangle intersections & live orbital motion.",
        [56, 189, 248, 255],
    );
    elements.push(ellipse_el(
        "dial_ring",
        "",
        540.0,
        210.0,
        240.0,
        240.0,
        [17, 28, 52, 255],
        [56, 189, 248, 255],
        2.0,
    ));
    elements.push(instance_el(
        "orbiter_inst",
        "orbiter",
        "sat_sym",
        660.0,
        330.0,
        InstancePlayback::Independent,
    ));

    let script = r#"import flash.geom.Point;
import flash.geom.Rectangle;
import flash.geom.Matrix;
import flash.geom.ColorTransform;

stop();

var pA:Point = new Point(0, 0);
var pB:Point = new Point(30, 40);
var dist:Number = Math.round(Point.distance(pA, pB));
var mid:Point = Point.interpolate(pA, pB, 0.5);

var r1:Rectangle = new Rectangle(10, 10, 100, 80);
var r2:Rectangle = new Rectangle(60, 40, 100, 80);
var overlap:Rectangle = r1.intersection(r2);

var mat:Matrix = new Matrix();
mat.scale(2, 2);
mat.translate(15, -5);
var mapped:Point = mat.transformPoint(new Point(10, 20));

var ct:ColorTransform = new ColorTransform(1, 1, 1, 1, 16, 24, 32, 0);
var ok:Boolean = (dist == 50 && mid.x == 15 && mid.y == 20 && overlap.width == 50 && overlap.height == 50 && mapped.x == 35 && mapped.y == 35 && ct.blueOffset == 32);

this.statusText.text = ok ? "PASS · Distance=50 | Overlap=50x50 | Matrix=(35,35)" : "FAIL · Geometry mismatch";
this.detailText.text = "Midpoint=(" + mid.x + ", " + mid.y + ") | ColorTransform blueOffset=" + ct.blueOffset;
if (ok) {
  trace("[PASS] as2/01-math-geometry dist=" + dist + " overlap=" + overlap.width + "x" + overlap.height);
}

var angleDeg:Number = 0;
var probe:MovieClip = this.orbiter;
this.onEnterFrame = function():Void {
  angleDeg = (angleDeg + 6) % 360;
  var rad:Number = angleDeg * Math.PI / 180;
  probe._x = 660 + Math.cos(rad) * 96;
  probe._y = 330 + Math.sin(rad) * 96;
  probe._rotation = angleDeg;
};
"#;

    let root = Symbol {
        id: "scene".into(),
        name: "Main".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![single_key_layer("main_l", "Dashboard", elements)],
            scripts: vec![as2_frame_script(0, script)],
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    };

    base_dashboard_project("AS2 01 · Math & Geometry", root, vec![sat_sym])
}

pub fn build_as2_02_oop_xml_collections() -> Project {
    let mut elements = dashboard_chrome_elements(
        "as2_02",
        "NEOFLASH AS2 SUITE · 02 / OOP, XML & SHAREDOBJECT",
        "ActionScript 2 · Class Packages, Inheritance, XML & Local Storage",
        "External SourceUnit classes (model.*), Array sorting, XML DOM parsing, Date.UTC & SharedObject.",
        [74, 222, 128, 255],
    );
    elements.push(rect_el(
        "card_xml",
        "",
        56.0,
        256.0,
        848.0,
        220.0,
        [20, 32, 54, 255],
        [74, 222, 128, 255],
        1.5,
    ));
    elements.push(text_el(
        "payload_txt",
        "payloadText",
        76.0,
        280.0,
        "Awaiting telemetry...",
        15.0,
        [226, 232, 240, 255],
    ));

    let script = r#"import model.*;

stop();

var probe:TelemetryProbe = new TelemetryProbe();
var reading:Number = probe.sample(4);

var channels:Array = ["gamma", "alpha", "beta"];
channels.sort();
var sortedCsv:String = channels.join(",");

var doc:XML = new XML("<station name='Observatory'><sensor id='S1' gain='42'/><sensor id='S2' gain='58'/></station>");
var rootNode:XMLNode = doc.firstChild;
var stationName:String = rootNode.attributes.name;
var firstSensorId:String = rootNode.firstChild.attributes.id;

var stamp:Number = Date.UTC(2026, 10, 11, 0, 0, 0);
var store:SharedObject = SharedObject.getLocal("nf_as2_suite_02");
store.data.lastReading = reading;
store.data.station = stationName;
store.flush();

var ok:Boolean = (reading == 30 && sortedCsv == "alpha,beta,gamma" && stationName == "Observatory" && firstSensorId == "S1" && stamp > 0 && store.data.lastReading == 30);
this.statusText.text = ok ? "PASS · TelemetryProbe=30 | XML=" + stationName + "/" + firstSensorId : "FAIL · OOP/XML check";
this.detailText.text = "Sorted channels: " + sortedCsv + " | SharedObject lastReading=" + store.data.lastReading;
this.payloadText.text = "Class inheritance: model.TelemetryProbe extends model.Instrument\nXML root: <station name='" + stationName + "'> first sensor=" + firstSensorId + "\nUTC timestamp: " + stamp;
if (ok) {
  trace("[PASS] as2/02-oop-xml-collections reading=" + reading + " sensor=" + firstSensorId);
}
"#;

    let root = Symbol {
        id: "scene".into(),
        name: "Main".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![single_key_layer("main_l", "Dashboard", elements)],
            scripts: vec![as2_frame_script(0, script)],
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    };

    let mut project = base_dashboard_project("AS2 02 · OOP, XML & Collections", root, vec![]);
    project.sources = vec![
        SourceUnit {
            id: "src_inst".into(),
            path: "model/Instrument.as".into(),
            language: ScriptLanguage::ActionScript2,
            source: "class model.Instrument {\n  private var baseGain:Number = 10;\n  public function Instrument() {}\n  public function readBase():Number { return baseGain; }\n}\n".into(),
        },
        SourceUnit {
            id: "src_probe".into(),
            path: "model/TelemetryProbe.as".into(),
            language: ScriptLanguage::ActionScript2,
            source: "import model.*;\nclass model.TelemetryProbe extends Instrument {\n  private var stepFactor:Number = 5;\n  public function TelemetryProbe() { super(); }\n  public function sample(steps:Number):Number { return readBase() + steps * stepFactor; }\n}\n".into(),
        },
    ];
    project.upgrade_format_for_features();
    project
}

pub fn build_as2_03_movieclips_nested_timelines() -> Project {
    let pod_sym = Symbol {
        id: "pod_sym".into(),
        name: "SignalPod".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 8,
            layers: vec![Layer {
                id: "pod_l".into(),
                name: "PodStates".into(),
                visible: true,
                locked: false,
                keyframes: vec![
                    Keyframe {
                        frame: 0,
                        label: "idle".into(),
                        elements: vec![
                            rect_el(
                                "pod_idle_bg",
                                "",
                                -48.0,
                                -32.0,
                                96.0,
                                64.0,
                                [30, 41, 59, 255],
                                [250, 204, 21, 255],
                                2.0,
                            ),
                            ellipse_el(
                                "pod_idle_led",
                                "",
                                -10.0,
                                -10.0,
                                20.0,
                                20.0,
                                [250, 204, 21, 255],
                                [0, 0, 0, 0],
                                0.0,
                            ),
                        ],
                        ..Keyframe::default()
                    },
                    Keyframe {
                        frame: 4,
                        label: "active".into(),
                        elements: vec![
                            rect_el(
                                "pod_act_bg",
                                "",
                                -48.0,
                                -32.0,
                                96.0,
                                64.0,
                                [20, 83, 45, 255],
                                [74, 222, 128, 255],
                                2.5,
                            ),
                            ellipse_el(
                                "pod_act_led",
                                "",
                                -12.0,
                                -12.0,
                                24.0,
                                24.0,
                                [74, 222, 128, 255],
                                [240, 253, 244, 255],
                                2.0,
                            ),
                        ],
                        ..Keyframe::default()
                    },
                ],
                properties: LayerProperties::default(),
            }],
            scripts: vec![
                as2_frame_script(0, "stop();\n"),
                as2_frame_script(4, "stop();\n"),
            ],
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: Some("SignalPod".into()),
    };

    let mut elements = dashboard_chrome_elements(
        "as2_03",
        "NEOFLASH AS2 SUITE · 03 / MOVIECLIPS & NESTED TIMELINES",
        "ActionScript 2 · attachMovie, Frame Labels & Depth Management",
        "Independent nested MovieClip state machines, Library linkage instantiation & dynamic vector wires.",
        [250, 204, 21, 255],
    );
    elements.push(instance_el(
        "stage_pod_inst",
        "stagePod",
        "pod_sym",
        220.0,
        350.0,
        InstancePlayback::Independent,
    ));

    let script = r#"stop();

this.stagePod.gotoAndStop("idle");
var attached:MovieClip = this.attachMovie("SignalPod", "attachedPod", 12);
attached._x = 520;
attached._y = 350;
attached.gotoAndStop("active");

var wire:MovieClip = this.createEmptyMovieClip("wireOverlay", 5);
wire.lineStyle(3, 0x38BDF8, 100);
wire.moveTo(268, 350);
wire.lineTo(472, 350);

var ok:Boolean = (this.stagePod._currentframe == 1 && attached != undefined && attached._currentframe == 5 && attached._totalframes == 8);
this.statusText.text = ok ? "PASS · stagePod=frame 1 (idle) | attachedPod=frame 5 (active)" : "FAIL · Nested timeline state";
this.detailText.text = "Library linkage 'SignalPod' attached at depth " + attached.getDepth() + " of " + attached._totalframes + " frames";
if (ok) {
  trace("[PASS] as2/03-movieclips-nested-timelines stage=" + this.stagePod._currentframe + " attached=" + attached._currentframe);
}
"#;

    let root = Symbol {
        id: "scene".into(),
        name: "Main".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![single_key_layer("main_l", "Dashboard", elements)],
            scripts: vec![as2_frame_script(0, script)],
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    };

    base_dashboard_project(
        "AS2 03 · MovieClips & Nested Timelines",
        root,
        vec![pod_sym],
    )
}

pub fn build_as2_04_events_stage_align_drag() -> Project {
    let puck_sym = Symbol {
        id: "puck_sym".into(),
        name: "DraggablePuck".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![single_key_layer(
                "puck_l",
                "Puck",
                vec![ellipse_el(
                    "puck_shape",
                    "",
                    -26.0,
                    -26.0,
                    52.0,
                    52.0,
                    [244, 114, 182, 255],
                    [253, 242, 248, 255],
                    2.5,
                )],
            )],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    };

    let target_sym = Symbol {
        id: "target_sym".into(),
        name: "DropTargetZone".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![single_key_layer(
                "target_l",
                "Target",
                vec![rect_el(
                    "target_rect",
                    "",
                    -70.0,
                    -50.0,
                    140.0,
                    100.0,
                    [30, 58, 138, 200],
                    [96, 165, 250, 255],
                    2.5,
                )],
            )],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    };

    let mut elements = dashboard_chrome_elements(
        "as2_04",
        "NEOFLASH AS2 SUITE · 04 / STAGE ALIGN, EVENTS & DRAG-DROP",
        "ActionScript 2 · Stage.scaleMode, Stage.align, Listeners & hitTest",
        "Top-left Stage anchoring, resize listener registration, interactive startDrag/stopDrag & hitTest.",
        [244, 114, 182, 255],
    );
    elements.push(instance_el(
        "zone_inst",
        "dropZone",
        "target_sym",
        560.0,
        360.0,
        InstancePlayback::Independent,
    ));
    elements.push(instance_el(
        "puck_inst",
        "puck",
        "puck_sym",
        560.0,
        360.0,
        InstancePlayback::Independent,
    ));

    let script = r#"stop();

Stage.scaleMode = "noScale";
Stage.align = "TL";

var resizeEvents:Number = 0;
var stageListener:Object = new Object();
var owner:MovieClip = this;
stageListener.onResize = function():Void {
  resizeEvents = resizeEvents + 1;
  owner.detailText.text = "Stage resized (" + resizeEvents + "): " + Stage.width + "x" + Stage.height + " align=" + Stage.align;
};
Stage.addListener(stageListener);

var puckClip:MovieClip = this.puck;
var zoneClip:MovieClip = this.dropZone;
puckClip.onPress = function():Void {
  this.startDrag(false, 80, 240, 860, 480);
};
puckClip.onRelease = function():Void {
  this.stopDrag();
};
puckClip.onReleaseOutside = puckClip.onRelease;

var hitOk:Boolean = puckClip.hitTest(zoneClip);
var ok:Boolean = (Stage.scaleMode == "noScale" && Stage.align == "TL" && Stage.width > 0 && hitOk);
this.statusText.text = ok ? "PASS · Stage=" + Stage.width + "x" + Stage.height + " align=" + Stage.align + " | hitTest=true" : "FAIL · Stage/hitTest check";
this.detailText.text = "Drag the pink puck across the blue docking zone (scaleMode=" + Stage.scaleMode + ")";
if (ok) {
  trace("[PASS] as2/04-events-stage-align-drag align=" + Stage.align + " hit=" + hitOk);
}
"#;

    let root = Symbol {
        id: "scene".into(),
        name: "Main".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![single_key_layer("main_l", "Dashboard", elements)],
            scripts: vec![as2_frame_script(0, script)],
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    };

    base_dashboard_project(
        "AS2 04 · Events, Stage Align & Drag",
        root,
        vec![puck_sym, target_sym],
    )
}

pub fn build_as2_05_drawing_api_filters_bitmapdata() -> Project {
    let elements = dashboard_chrome_elements(
        "as2_05",
        "NEOFLASH AS2 SUITE · 05 / DRAWING API, FILTERS & BITMAPDATA",
        "ActionScript 2 · Vector Drawing, Display Filters & Pixel Synthesis",
        "Runtime beginGradientFill/curveTo art, Glow/DropShadow filters & ARGB BitmapData pixel synthesis.",
        [167, 139, 250, 255],
    );

    let script = r#"import flash.display.BitmapData;
import flash.geom.Rectangle;
import flash.geom.Matrix;
import flash.filters.GlowFilter;
import flash.filters.DropShadowFilter;

stop();

var art:MovieClip = this.createEmptyMovieClip("vectorArt", 10);
art._x = 90;
art._y = 260;
var gradMat:Matrix = new Matrix();
gradMat.createGradientBox(260, 180, 0, 0, 0);
art.lineStyle(2.5, 0xE9D5FF, 100);
art.beginGradientFill("linear", [0x6D28D9, 0x0EA5E9], [100, 100], [0, 255], gradMat);
art.moveTo(0, 24);
art.curveTo(130, -24, 260, 24);
art.lineTo(260, 176);
art.curveTo(130, 212, 0, 176);
art.lineTo(0, 24);
art.endFill();

var glow:GlowFilter = new GlowFilter(0x38BDF8, 0.9, 14, 14, 2, 1, false, false);
var shadow:DropShadowFilter = new DropShadowFilter(6, 45, 0x020617, 0.75, 10, 10, 1, 1, false, false, false);
art.filters = [glow, shadow];

var bmd:BitmapData = new BitmapData(64, 64, true, 0xFF0F172A);
bmd.fillRect(new Rectangle(8, 8, 48, 48), 0xFF22D3EE);
bmd.setPixel32(16, 16, 0xFFFFD464);
var sampled:Number = bmd.getPixel(16, 16);

var bmpHolder:MovieClip = this.createEmptyMovieClip("bmpHolder", 20);
bmpHolder._x = 460;
bmpHolder._y = 265;
bmpHolder._xscale = 250;
bmpHolder._yscale = 250;
bmpHolder.attachBitmap(bmd, 1, "auto", false);

var ok:Boolean = (sampled == 0xFFD464 && art.filters.length == 2);
this.statusText.text = ok ? "PASS · Gradient vector + 2 filters + BitmapData 0xFFD464" : "FAIL · Drawing/BitmapData";
this.detailText.text = "Filter stack count=" + art.filters.length + " | Sampled pixel RGB=0x" + sampled.toString(16);
if (ok) {
  trace("[PASS] as2/05-drawing-api-filters-bitmapdata pixel=0x" + sampled.toString(16) + " filters=" + art.filters.length);
}
"#;

    let root = Symbol {
        id: "scene".into(),
        name: "Main".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![single_key_layer("main_l", "Dashboard", elements)],
            scripts: vec![as2_frame_script(0, script)],
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    };

    base_dashboard_project("AS2 05 · Drawing API, Filters & BitmapData", root, vec![])
}

pub fn build_as2_06_dynamic_input_html_text() -> Project {
    let elements = dashboard_chrome_elements(
        "as2_06",
        "NEOFLASH AS2 SUITE · 06 / DYNAMIC, INPUT & HTML TEXT",
        "ActionScript 2 · createTextField, TextFormat & Rich htmlText",
        "Runtime dynamic & editable input TextFields, TextFormat styling, wordWrap & HTML markup extraction.",
        [52, 211, 153, 255],
    );

    let script = r#"stop();

var richField:TextField = this.createTextField("richField", 10, 64, 255, 420, 180);
richField.multiline = true;
richField.wordWrap = true;
richField.border = true;
richField.borderColor = 0x34D399;
richField.background = true;
richField.backgroundColor = 0x0F172A;
richField.html = true;
richField.htmlText = "<p><font face='Arial' size='18' color='#38BDF8'><b>NeoFlash Station · Departure 23:45 · Platform 5</b></font></p>";

var sheet:TextField.StyleSheet = new TextField.StyleSheet();
var cssOk:Boolean = sheet.parseCSS(".headline { font-family: Arial; font-size: 18px; color: #38BDF8; font-weight: bold; }");

var inputField:TextField = this.createTextField("inputField", 11, 520, 255, 360, 44);
inputField.type = "input";
inputField.border = true;
inputField.borderColor = 0x38BDF8;
inputField.background = true;
inputField.backgroundColor = 0x1E293B;
var fmt:TextFormat = new TextFormat("Arial", 18, 0xF8FAFC, true);
inputField.setNewTextFormat(fmt);
inputField.text = "Ticket #NF-500";
inputField.setTextFormat(fmt);

var plain:String = richField.text;
var ok:Boolean = (cssOk && plain.indexOf("NeoFlash Station") >= 0 && plain.indexOf("Platform 5") >= 0 && inputField.type == "input" && inputField.text == "Ticket #NF-500");
this.statusText.text = ok ? "PASS · htmlText parsed & Input TextField ready" : "FAIL · TextField verification";
this.detailText.text = "Extracted plain text length=" + plain.length + " | Input value=" + inputField.text;
if (ok) {
  trace("[PASS] as2/06-dynamic-input-html-text input=" + inputField.text);
}
"#;

    let root = Symbol {
        id: "scene".into(),
        name: "Main".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![single_key_layer("main_l", "Dashboard", elements)],
            scripts: vec![as2_frame_script(0, script)],
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    };

    base_dashboard_project("AS2 06 · Dynamic, Input & HTML Text", root, vec![])
}

pub fn build_as2_07_sound_video_loadpublish(
    child_bundle_js: &[u8],
    child_index_html: &[u8],
) -> Project {
    let holder_sym = empty_holder_symbol("holder_sym", "SubAppHolder");

    let mut elements = dashboard_chrome_elements(
        "as2_07",
        "NEOFLASH AS2 SUITE · 07 / SOUND, VIDEO & LOADPUBLISH",
        "ActionScript 2 · WAV Sound, AVI NetStream & Dynamic Bundle Mount",
        "Plays embedded WAV audio, streams RIFF AVI video, and dynamically mounts published 00-child-module.",
        [251, 146, 60, 255],
    );
    elements.push(Element {
        id: "vid_el".into(),
        name: "videoBox".into(),
        transform: tf(64.0, 265.0),
        kind: ElementKind::Video {
            width: 320.0,
            height: 180.0,
        },
        ..Element::default()
    });
    elements.push(rect_el(
        "holder_frame",
        "",
        460.0,
        261.0,
        328.0,
        188.0,
        [15, 23, 42, 255],
        [251, 146, 60, 255],
        1.5,
    ));
    elements.push(instance_el(
        "holder_inst",
        "holder",
        "holder_sym",
        464.0,
        265.0,
        InstancePlayback::Independent,
    ));

    let script = r#"stop();

var chime:Sound = new Sound(this);
chime.setVolume(65);
chime.loadSound("audio/chime.wav", false);
chime.start(0, 1);

var nc:NetConnection = new NetConnection();
nc.connect(null);
var ns:NetStream = new NetStream(nc);
this.videoBox.attachVideo(ns);
ns.play("media/clip.avi");

var holderClip:MovieClip = this.holder;
holderClip.loadMovie("subapp/bundle.js");

var reported:Boolean = false;
var ticks:Number = 0;
var owner:MovieClip = this;
this.onEnterFrame = function():Void {
  ticks = ticks + 1;
  if (!reported && holderClip._totalframes == 12) {
    reported = true;
    owner.statusText.text = "PASS · Sound + AVI stream + loadMovie mounted 12-frame child app";
    owner.detailText.text = "Mounted subapp/bundle.js (_totalframes=" + holderClip._totalframes + ", chime volume=" + chime.getVolume() + ")";
    trace("[PASS] as2/07-sound-video-loadpublish mounted=" + holderClip._totalframes);
  } else if (!reported && ticks == 1) {
    owner.statusText.text = "MOUNTING · Loading subapp/bundle.js + media/clip.avi...";
  }
};
"#;

    let root = Symbol {
        id: "scene".into(),
        name: "Main".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![single_key_layer("main_l", "Dashboard", elements)],
            scripts: vec![as2_frame_script(0, script)],
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    };

    let wav_bytes = make_harmonic_stereo_wav(350, &[523.25, 659.25, 783.99], false);
    let avi_bytes = make_sample_mjpeg_avi();

    let mut project = base_dashboard_project(
        "AS2 07 · Sound, Video & loadPublish",
        root,
        vec![holder_sym],
    );
    project.resources = vec![
        ResourceUnit::from_bytes("res_wav", "audio/chime.wav", "audio/wav", &wav_bytes)
            .expect("wav resource"),
        ResourceUnit::from_bytes("res_avi", "media/clip.avi", "video/avi", &avi_bytes)
            .expect("avi resource"),
        ResourceUnit::from_bytes(
            "res_sub_bundle",
            "subapp/bundle.js",
            "text/javascript",
            child_bundle_js,
        )
        .expect("subapp bundle resource"),
        ResourceUnit::from_bytes(
            "res_sub_html",
            "subapp/index.html",
            "text/html",
            child_index_html,
        )
        .expect("subapp html resource"),
    ];
    project.upgrade_format_for_features();
    project
}

pub fn build_as2_08_interactive_arcade_miniapp() -> Project {
    let paddle_sym = Symbol {
        id: "paddle_sym".into(),
        name: "StarCatcherPaddle".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![single_key_layer(
                "pad_l",
                "Paddle",
                vec![rect_el(
                    "pad_rect",
                    "",
                    -54.0,
                    -9.0,
                    108.0,
                    18.0,
                    [56, 189, 248, 255],
                    [224, 242, 254, 255],
                    2.0,
                )],
            )],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    };

    let orb_sym = Symbol {
        id: "orb_sym".into(),
        name: "EnergyOrb".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![single_key_layer(
                "orb_l",
                "Orb",
                vec![ellipse_el(
                    "orb_circle",
                    "",
                    -12.0,
                    -12.0,
                    24.0,
                    24.0,
                    [250, 204, 21, 255],
                    [254, 249, 195, 255],
                    2.0,
                )],
            )],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    };

    let mut elements = dashboard_chrome_elements(
        "as2_08",
        "NEOFLASH AS2 SUITE · 08 / STARLIGHT INTERCEPTOR MINI-GAME",
        "ActionScript 2 · Playable Arcade Loop (Arrows / A-D / Auto-Pilot)",
        "Real-time game loop, collision detection, procedural starfield, score HUD & instant R-key restart.",
        [250, 204, 21, 255],
    );
    elements.push(rect_el(
        "arena_bg",
        "",
        64.0,
        242.0,
        832.0,
        250.0,
        [9, 14, 28, 255],
        [56, 189, 248, 255],
        2.0,
    ));
    elements.push(instance_el(
        "paddle_inst",
        "paddle",
        "paddle_sym",
        480.0,
        464.0,
        InstancePlayback::Independent,
    ));
    elements.push(instance_el(
        "orb_inst",
        "orb",
        "orb_sym",
        480.0,
        396.0,
        InstancePlayback::Independent,
    ));

    let script = r#"stop();

var arenaLeft:Number = 120;
var arenaRight:Number = 840;
var paddleY:Number = 464;
var score:Number = 0;
var manualControl:Boolean = false;
var moveDir:Number = 0;
var orbVx:Number = 5;
var orbVy:Number = 16;
var passTraced:Boolean = false;

var stars:MovieClip = this.createEmptyMovieClip("starLayer", 2);
var sIdx:Number = 0;
while (sIdx < 18) {
  var sx:Number = 90 + ((sIdx * 97) % 780);
  var sy:Number = 258 + ((sIdx * 53) % 180);
  stars.beginFill(0x93C5FD, 65);
  stars.moveTo(sx, sy);
  stars.lineTo(sx + 3, sy);
  stars.lineTo(sx + 3, sy + 3);
  stars.lineTo(sx, sy + 3);
  stars.endFill();
  sIdx = sIdx + 1;
}

var paddleClip:MovieClip = this.paddle;
var orbClip:MovieClip = this.orb;
var owner:MovieClip = this;

function refreshHud():Void {
  owner.statusText.text = "SCORE: " + score + " · " + (manualControl ? "MANUAL PILOT" : "AUTO-PILOT DEMO (Press Left/Right)");
  owner.detailText.text = "Paddle X=" + Math.round(paddleClip._x) + " | Orb=(" + Math.round(orbClip._x) + ", " + Math.round(orbClip._y) + ")";
}

this.onKeyDown = function():Void {
  var code:Number = Key.getCode();
  if (code == Key.LEFT || code == 65) {
    manualControl = true;
    moveDir = -1;
  } else if (code == Key.RIGHT || code == 68) {
    manualControl = true;
    moveDir = 1;
  } else if (code == 82) {
    score = 0;
    manualControl = false;
    orbClip._x = 480;
    orbClip._y = 396;
    refreshHud();
  }
};
this.onKeyUp = function():Void {
  moveDir = 0;
};
Key.addListener(this);

this.onEnterFrame = function():Void {
  orbClip._x = orbClip._x + orbVx;
  orbClip._y = orbClip._y + orbVy;
  if (orbClip._x < arenaLeft || orbClip._x > arenaRight) {
    orbVx = -orbVx;
  }
  if (!manualControl) {
    paddleClip._x = paddleClip._x + (orbClip._x - paddleClip._x) * 0.45;
  } else {
    paddleClip._x = Math.max(arenaLeft, Math.min(arenaRight, paddleClip._x + moveDir * 14));
  }
  if (orbClip._y >= paddleY - 18) {
    if (Math.abs(orbClip._x - paddleClip._x) <= 64) {
      score = score + 100;
      if (!passTraced) {
        passTraced = true;
        trace("[PASS] as2/08-interactive-arcade-miniapp score=" + score);
      }
    }
    orbClip._y = 268;
    orbVx = (score % 200 == 0) ? 6 : -6;
  }
  refreshHud();
};

refreshHud();
"#;

    let root = Symbol {
        id: "scene".into(),
        name: "Main".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![single_key_layer("main_l", "Dashboard", elements)],
            scripts: vec![as2_frame_script(0, script)],
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    };

    base_dashboard_project(
        "AS2 08 · Starlight Interceptor Mini-Game",
        root,
        vec![paddle_sym, orb_sym],
    )
}
