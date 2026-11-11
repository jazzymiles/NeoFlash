use super::*;
use neoflash_core::{
    Easing, FrameScript, InstancePlayback, Keyframe, Layer, LayerProperties, Project, ResourceUnit,
    Script, ScriptLanguage, SourceUnit, Symbol, SymbolKind, Timeline, Transform,
};

fn as3_frame_script(frame: u32, source: &str) -> FrameScript {
    FrameScript {
        frame,
        script: Script {
            language: ScriptLanguage::ActionScript3,
            source: source.into(),
        },
    }
}

pub fn build_as3_00_child_module() -> Project {
    let spinner_sym = Symbol {
        id: "spinner_sym".into(),
        name: "OrbitalSpinnerAS3".into(),
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
                        [99, 102, 241, 255],
                        [224, 231, 255, 255],
                        2.0,
                    ),
                    rect_el(
                        "spin_spoke",
                        "",
                        -4.0,
                        -18.0,
                        8.0,
                        36.0,
                        [52, 211, 153, 255],
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
        name: "ChildModuleAS3".into(),
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
                            [129, 140, 248, 255],
                            2.0,
                        ),
                        text_el(
                            "hdr_txt",
                            "badgeLabel",
                            18.0,
                            20.0,
                            "CHILD MODULE · AS3",
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
            scripts: vec![as3_frame_script(
                0,
                "this.badgeLabel.text = \"MODULE LIVE · AS3\";\ntrace(\"[PASS] as3/00-child-module ready\");\n",
            )],
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    };

    let mut project = Project {
        format_version: 1,
        name: "AS3 00 · Published Child Module".into(),
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

pub fn build_as3_01_math_geometry() -> Project {
    let sat_sym = Symbol {
        id: "sat_sym".into(),
        name: "OrbitalProbeAS3".into(),
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
                        [129, 140, 248, 255],
                        [238, 242, 255, 255],
                        2.0,
                    ),
                    rect_el(
                        "sat_wing",
                        "",
                        -26.0,
                        -3.0,
                        52.0,
                        6.0,
                        [52, 211, 153, 255],
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
        "as3_01",
        "NEOFLASH AS3 SUITE · 01 / MATH & GEOMETRY",
        "ActionScript 3 · Point, Rectangle, Matrix & ColorTransform",
        "Typed flash.geom algebra, Matrix point transforms, Rectangle unions/intersections & ENTER_FRAME orbit.",
        [129, 140, 248, 255],
    );
    elements.push(ellipse_el(
        "dial_ring",
        "",
        540.0,
        210.0,
        240.0,
        240.0,
        [17, 28, 52, 255],
        [129, 140, 248, 255],
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

    let script = r#"import flash.events.Event;
import flash.geom.Point;
import flash.geom.Rectangle;
import flash.geom.Matrix;
import flash.geom.ColorTransform;

stop();

var pA:Point = new Point(0, 0);
var pB:Point = new Point(30, 40);
var dist:int = int(Math.round(Point.distance(pA, pB)));
var mid:Point = Point.interpolate(pA, pB, 0.5);

var r1:Rectangle = new Rectangle(10, 10, 100, 80);
var r2:Rectangle = new Rectangle(60, 40, 100, 80);
var overlap:Rectangle = r1.intersection(r2);

var mat:Matrix = new Matrix();
mat.scale(2, 2);
mat.translate(15, -5);
var mapped:Point = mat.transformPoint(new Point(10, 20));

var ct:ColorTransform = new ColorTransform(1, 1, 1, 1, 12, 24, 36, 0);
var ok:Boolean = (dist == 50 && mid.x == 15 && mid.y == 20 && overlap.width == 50 && overlap.height == 50 && mapped.x == 35 && mapped.y == 35 && ct.blueOffset == 36);

this.statusText.text = ok ? "PASS · Distance=50 | Overlap=50x50 | Matrix=(35,35)" : "FAIL · Geometry mismatch";
this.detailText.text = "Midpoint=(" + mid.x + ", " + mid.y + ") | ColorTransform blueOffset=" + ct.blueOffset;
if (ok) {
  trace("[PASS] as3/01-math-geometry dist=" + dist + " overlap=" + overlap.width + "x" + overlap.height);
}

var angleDeg:Number = 0;
var probe:Object = this.orbiter;
function onTick(event:Event):void {
  angleDeg = (angleDeg + 6) % 360;
  var rad:Number = angleDeg * Math.PI / 180;
  probe.x = 660 + Math.cos(rad) * 96;
  probe.y = 330 + Math.sin(rad) * 96;
  probe.rotation = angleDeg;
}
addEventListener(Event.ENTER_FRAME, onTick);
"#;

    let root = Symbol {
        id: "scene".into(),
        name: "Main".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![single_key_layer("main_l", "Dashboard", elements)],
            scripts: vec![as3_frame_script(0, script)],
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    };

    base_dashboard_project("AS3 01 · Math & Geometry", root, vec![sat_sym])
}

pub fn build_as3_02_oop_xml_collections() -> Project {
    let mut elements = dashboard_chrome_elements(
        "as3_02",
        "NEOFLASH AS3 SUITE · 02 / PACKAGES, XML, BYTEARRAY & SHAREDOBJECT",
        "ActionScript 3 · Typed OOP Inheritance, XML, ByteArray & Storage",
        "Document class app.Main, package model.* overrides, XML E4X/DOM, ByteArray UTF & SharedObject.",
        [52, 211, 153, 255],
    );
    elements.push(rect_el(
        "card_xml",
        "",
        56.0,
        256.0,
        848.0,
        220.0,
        [20, 32, 54, 255],
        [52, 211, 153, 255],
        1.5,
    ));
    elements.push(text_el(
        "payload_txt",
        "payloadText",
        76.0,
        280.0,
        "Awaiting AS3 document class...",
        15.0,
        [226, 232, 240, 255],
    ));

    let root = Symbol {
        id: "scene".into(),
        name: "Main".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![single_key_layer("main_l", "Dashboard", elements)],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    };

    let main_src = r#"package app {
  import flash.display.MovieClip;
  import flash.net.SharedObject;
  import flash.utils.ByteArray;
  import model.*;

  public dynamic class Main extends MovieClip {
    public function Main() {
      super();
      stop();

      var probe:TelemetryProbe = new TelemetryProbe();
      var reading:int = probe.sample(5);

      var tags:Array = ["zeta", "alpha", "omega"];
      tags.sort();
      var sortedCsv:String = tags.join(",");

      var xmlDoc:XML = new XML("<station name='Observatory'><sensor id='S1'>42</sensor></station>");
      var stationName:String = String(xmlDoc.@name);

      var bytes:ByteArray = new ByteArray();
      bytes.writeUTFBytes("NEOFLASH");
      bytes.position = 0;
      var decoded:String = bytes.readUTFBytes(8);

      var rx:RegExp = /^NEO[A-Z]+$/;
      var rxOk:Boolean = rx.test(decoded);

      var stamp:Number = Date.UTC(2026, 10, 11, 0, 0, 0);
      var store:SharedObject = SharedObject.getLocal("nf_as3_suite_02");
      store.data.reading = reading;
      store.flush();

      var ok:Boolean = (reading == 30 && sortedCsv == "alpha,omega,zeta" && stationName == "Observatory" && decoded == "NEOFLASH" && rxOk && stamp > 0 && store.data.reading == 30);
      this.statusText.text = ok ? "PASS · Override=30 | XML=" + stationName + " | ByteArray=" + decoded : "FAIL · AS3 OOP/XML/ByteArray";
      this.detailText.text = "Sorted=" + sortedCsv + " | RegExp=" + rxOk + " | SharedObject reading=" + store.data.reading;
      this.payloadText.text = "Document class: app.Main extends MovieClip\nOverride: model.TelemetryProbe.sample(5) => " + reading + "\nByteArray UTF-8 round-trip: " + decoded + " (" + bytes.length + " bytes)";
      if (ok) {
        trace("[PASS] as3/02-oop-xml-collections reading=" + reading + " bytes=" + decoded);
      }
    }
  }
}
"#;

    let mut project = base_dashboard_project("AS3 02 · OOP, XML & Collections", root, vec![]);
    project.document_class = Some("app.Main".into());
    project.sources = vec![
        SourceUnit {
            id: "src_inst".into(),
            path: "model/Instrument.as".into(),
            language: ScriptLanguage::ActionScript3,
            source: "package model {\n  public class Instrument {\n    protected var baseGain:int = 10;\n    public function Instrument() {}\n    public function sample(step:int):int { return baseGain + step; }\n  }\n}\n".into(),
        },
        SourceUnit {
            id: "src_probe".into(),
            path: "model/TelemetryProbe.as".into(),
            language: ScriptLanguage::ActionScript3,
            source: "package model {\n  public class TelemetryProbe extends Instrument {\n    public function TelemetryProbe() { super(); }\n    public override function sample(step:int):int { return super.sample(step) * 2; }\n  }\n}\n".into(),
        },
        SourceUnit {
            id: "src_main".into(),
            path: "app/Main.as".into(),
            language: ScriptLanguage::ActionScript3,
            source: main_src.into(),
        },
    ];
    project.upgrade_format_for_features();
    project
}

pub fn build_as3_03_movieclips_nested_timelines() -> Project {
    let pod_sym = Symbol {
        id: "pod_sym".into(),
        name: "SignalPodAS3".into(),
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
                as3_frame_script(0, "stop();\n"),
                as3_frame_script(4, "stop();\n"),
            ],
            guides: Vec::new(),
        },
        class_name: Some("app.SignalPod".into()),
        linkage_id: Some("signal-pod".into()),
    };

    let mut elements = dashboard_chrome_elements(
        "as3_03",
        "NEOFLASH AS3 SUITE · 03 / LINKED MOVIECLIPS & DISPLAY LIST",
        "ActionScript 3 · Linked Symbol Classes, Frame Labels & Sprite Display List",
        "Instantiates linked app.SignalPod MovieClips via new SignalPod(), frame labels & Sprite connectors.",
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

    let root = Symbol {
        id: "scene".into(),
        name: "Main".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![single_key_layer("main_l", "Dashboard", elements)],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    };

    let main_src = r#"package app {
  import flash.display.MovieClip;
  import flash.display.Sprite;

  public dynamic class Main extends MovieClip {
    public function Main() {
      super();
      stop();

      var leftPod:MovieClip = this.stagePod as MovieClip;
      leftPod.gotoAndStop("idle");

      var rightPod:SignalPod = new SignalPod();
      rightPod.name = "dynamicPod";
      rightPod.x = 520;
      rightPod.y = 350;
      addChild(rightPod);
      rightPod.gotoAndStop("active");

      var wire:Sprite = new Sprite();
      wire.graphics.lineStyle(3, 0x818CF8, 1.0);
      wire.graphics.moveTo(268, 350);
      wire.graphics.lineTo(472, 350);
      addChild(wire);

      var ok:Boolean = (leftPod.currentFrame == 1 && leftPod.currentLabel == "idle" && rightPod.currentFrame == 5 && rightPod.currentLabel == "active" && rightPod.totalFrames == 8);
      this.statusText.text = ok ? "PASS · stagePod=idle(1) | new SignalPod()=active(5)" : "FAIL · Linked AS3 MovieClip check";
      this.detailText.text = "numChildren=" + numChildren + " | dynamicPod totalFrames=" + rightPod.totalFrames + " label=" + rightPod.currentLabel;
      if (ok) {
        trace("[PASS] as3/03-movieclips-nested-timelines stage=" + leftPod.currentFrame + " dynamic=" + rightPod.currentFrame);
      }
    }
  }
}
"#;

    let pod_src = "package app {\n  import flash.display.MovieClip;\n  public dynamic class SignalPod extends MovieClip {\n    public function SignalPod() { super(); }\n  }\n}\n";

    let mut project = base_dashboard_project(
        "AS3 03 · MovieClips & Nested Timelines",
        root,
        vec![pod_sym],
    );
    project.document_class = Some("app.Main".into());
    project.sources = vec![
        SourceUnit {
            id: "src_pod".into(),
            path: "app/SignalPod.as".into(),
            language: ScriptLanguage::ActionScript3,
            source: pod_src.into(),
        },
        SourceUnit {
            id: "src_main".into(),
            path: "app/Main.as".into(),
            language: ScriptLanguage::ActionScript3,
            source: main_src.into(),
        },
    ];
    project.upgrade_format_for_features();
    project
}

pub fn build_as3_04_events_stage_align_drag() -> Project {
    let puck_sym = Symbol {
        id: "puck_sym".into(),
        name: "DraggablePuckAS3".into(),
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
        name: "DropTargetZoneAS3".into(),
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
        "as3_04",
        "NEOFLASH AS3 SUITE · 04 / STAGE ALIGN, EVENTS & DRAG-DROP",
        "ActionScript 3 · stage.scaleMode, stage.align, MouseEvent & hitTestObject",
        "Configures NO_SCALE + TOP_LEFT Stage layout, Event.RESIZE listener, MouseEvent drag & hitTestObject.",
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

    let script = r#"import flash.display.MovieClip;
import flash.display.StageAlign;
import flash.display.StageScaleMode;
import flash.events.Event;
import flash.events.MouseEvent;
import flash.geom.Rectangle;

stop();

var puckClip:MovieClip = this.puck as MovieClip;
var zoneClip:MovieClip = this.dropZone as MovieClip;
var owner:Object = this;

if (stage != null) {
  stage.scaleMode = StageScaleMode.NO_SCALE;
  stage.align = StageAlign.TOP_LEFT;
  stage.addEventListener(Event.RESIZE, function(e:Event):void {
    owner.detailText.text = "Stage resized: " + stage.stageWidth + "x" + stage.stageHeight + " align=" + stage.align;
  });
}

puckClip.buttonMode = true;
puckClip.addEventListener(MouseEvent.MOUSE_DOWN, function(e:MouseEvent):void {
  puckClip.startDrag(false, new Rectangle(80, 240, 780, 240));
});
puckClip.addEventListener(MouseEvent.MOUSE_UP, function(e:MouseEvent):void {
  puckClip.stopDrag();
});

var hitOk:Boolean = puckClip.hitTestObject(zoneClip);
var alignVal:String = (stage != null) ? stage.align : "TL";
var scaleVal:String = (stage != null) ? stage.scaleMode : "noScale";
var ok:Boolean = (alignVal == StageAlign.TOP_LEFT && scaleVal == StageScaleMode.NO_SCALE && hitOk);
this.statusText.text = ok ? "PASS · stage.align=" + alignVal + " scaleMode=" + scaleVal + " | hitTestObject=true" : "FAIL · AS3 Stage/hitTest";
this.detailText.text = "Drag the pink puck across the blue docking zone (stageWidth=" + (stage != null ? stage.stageWidth : 960) + ")";
if (ok) {
  trace("[PASS] as3/04-events-stage-align-drag align=" + alignVal + " hit=" + hitOk);
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
            scripts: vec![as3_frame_script(0, script)],
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    };

    base_dashboard_project(
        "AS3 04 · Events, Stage Align & Drag",
        root,
        vec![puck_sym, target_sym],
    )
}

pub fn build_as3_05_drawing_api_filters_bitmapdata() -> Project {
    let elements = dashboard_chrome_elements(
        "as3_05",
        "NEOFLASH AS3 SUITE · 05 / GRAPHICS API, FILTERS & BITMAPDATA",
        "ActionScript 3 · Sprite.graphics, Glow/DropShadow & Bitmap/BitmapData",
        "Typed GradientType.LINEAR vector drawing, filter arrays & ARGB BitmapData rendered via flash.display.Bitmap.",
        [167, 139, 250, 255],
    );

    let script = r#"import flash.display.Bitmap;
import flash.display.BitmapData;
import flash.display.GradientType;
import flash.display.Sprite;
import flash.filters.DropShadowFilter;
import flash.filters.GlowFilter;
import flash.geom.Matrix;
import flash.geom.Rectangle;

stop();

var art:Sprite = new Sprite();
art.x = 90;
art.y = 260;
var gradMat:Matrix = new Matrix();
gradMat.createGradientBox(260, 180, 0, 0, 0);
art.graphics.lineStyle(2.5, 0xE9D5FF, 1.0);
art.graphics.beginGradientFill(GradientType.LINEAR, [0x4F46E5, 0x06B6D4], [1.0, 1.0], [0, 255], gradMat);
art.graphics.moveTo(0, 24);
art.graphics.curveTo(130, -24, 260, 24);
art.graphics.lineTo(260, 176);
art.graphics.curveTo(130, 212, 0, 176);
art.graphics.lineTo(0, 24);
art.graphics.endFill();

var glow:GlowFilter = new GlowFilter(0x38BDF8, 0.9, 14, 14, 2, 1, false, false);
var shadow:DropShadowFilter = new DropShadowFilter(6, 45, 0x020617, 0.75, 10, 10, 1, 1, false, false, false);
art.filters = [glow, shadow];
addChild(art);

var bmd:BitmapData = new BitmapData(64, 64, true, 0xFF0F172A);
bmd.fillRect(new Rectangle(8, 8, 48, 48), 0xFF34D399);
bmd.setPixel32(16, 16, 0xFFFFD464);
var sampled:uint = bmd.getPixel(16, 16);

var bmp:Bitmap = new Bitmap(bmd);
bmp.x = 460;
bmp.y = 265;
bmp.scaleX = 2.5;
bmp.scaleY = 2.5;
addChild(bmp);

var ok:Boolean = (sampled == 0xFFD464 && art.filters.length == 2);
this.statusText.text = ok ? "PASS · Sprite.graphics + 2 filters + Bitmap 0xFFD464" : "FAIL · AS3 Graphics/BitmapData";
this.detailText.text = "Filter stack count=" + art.filters.length + " | Sampled pixel RGB=0x" + sampled.toString(16);
if (ok) {
  trace("[PASS] as3/05-drawing-api-filters-bitmapdata pixel=0x" + sampled.toString(16) + " filters=" + art.filters.length);
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
            scripts: vec![as3_frame_script(0, script)],
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    };

    base_dashboard_project("AS3 05 · Graphics API, Filters & BitmapData", root, vec![])
}

pub fn build_as3_06_dynamic_input_html_text() -> Project {
    let elements = dashboard_chrome_elements(
        "as3_06",
        "NEOFLASH AS3 SUITE · 06 / DYNAMIC, INPUT & HTML TEXT",
        "ActionScript 3 · TextField, TextFieldType.INPUT, TextFormat & htmlText",
        "Creates styled dynamic & editable TextFields on the display list with HTML markup and formatting.",
        [52, 211, 153, 255],
    );

    let script = r#"import flash.text.TextField;
import flash.text.TextFieldType;
import flash.text.TextFormat;

stop();

var richField:TextField = new TextField();
richField.x = 64;
richField.y = 255;
richField.width = 420;
richField.height = 180;
richField.multiline = true;
richField.wordWrap = true;
richField.border = true;
richField.borderColor = 0x34D399;
richField.background = true;
richField.backgroundColor = 0x0F172A;
richField.htmlText = "<b>NeoFlash Observatory</b><br/><font color='#818CF8'>Eclipse Crossing 00:00</font> · <u>Gate A</u>";
addChild(richField);

var inputField:TextField = new TextField();
inputField.x = 520;
inputField.y = 255;
inputField.width = 360;
inputField.height = 44;
inputField.type = TextFieldType.INPUT;
inputField.border = true;
inputField.borderColor = 0x818CF8;
inputField.background = true;
inputField.backgroundColor = 0x1E293B;
var fmt:TextFormat = new TextFormat("Arial", 18, 0xF8FAFC, true);
inputField.defaultTextFormat = fmt;
inputField.text = "Pass #AS3-900";
inputField.setTextFormat(fmt);
addChild(inputField);

var plain:String = richField.text;
var ok:Boolean = (plain.indexOf("NeoFlash Observatory") >= 0 && plain.indexOf("Gate A") >= 0 && inputField.type == TextFieldType.INPUT && inputField.text == "Pass #AS3-900");
this.statusText.text = ok ? "PASS · htmlText parsed & TextFieldType.INPUT ready" : "FAIL · AS3 TextField verification";
this.detailText.text = "Extracted plain text length=" + plain.length + " | Input value=" + inputField.text;
if (ok) {
  trace("[PASS] as3/06-dynamic-input-html-text input=" + inputField.text);
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
            scripts: vec![as3_frame_script(0, script)],
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    };

    base_dashboard_project("AS3 06 · Dynamic, Input & HTML Text", root, vec![])
}

pub fn build_as3_07_sound_video_loadpublish(
    child_bundle_js: &[u8],
    child_index_html: &[u8],
) -> Project {
    let holder_sym = empty_holder_symbol("holder_sym", "SubAppHolderAS3");

    let mut elements = dashboard_chrome_elements(
        "as3_07",
        "NEOFLASH AS3 SUITE · 07 / SOUND, VIDEO & LOADER LOADPUBLISH",
        "ActionScript 3 · SoundChannel, AVI Video & Loader Published-Bundle Mount",
        "Plays embedded WAV via SoundTransform, streams RIFF AVI via NetStream, and mounts 00-child-module via Loader.",
        [251, 146, 60, 255],
    );
    elements.push(rect_el(
        "vid_frame",
        "",
        60.0,
        261.0,
        328.0,
        188.0,
        [15, 23, 42, 255],
        [56, 189, 248, 255],
        1.5,
    ));
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

    let script = r#"import flash.display.Loader;
import flash.display.MovieClip;
import flash.events.Event;
import flash.media.Sound;
import flash.media.SoundChannel;
import flash.media.SoundTransform;
import flash.media.Video;
import flash.net.NetConnection;
import flash.net.NetStream;
import flash.net.URLRequest;

stop();

var chime:Sound = new Sound(new URLRequest("audio/chime.wav"));
var channel:SoundChannel = chime.play(0, 1, new SoundTransform(0.65, 0.0));

var video:Video = new Video(320, 180);
video.x = 64;
video.y = 265;
addChild(video);

var nc:NetConnection = new NetConnection();
nc.connect(null);
var ns:NetStream = new NetStream(nc);
ns.client = { onMetaData: function(info:Object):void {} };
video.attachNetStream(ns);
ns.play("media/clip.avi");

var holderClip:MovieClip = this.holder as MovieClip;
var subLoader:Loader = new Loader();
holderClip.addChild(subLoader);
var owner:Object = this;

subLoader.contentLoaderInfo.addEventListener(Event.COMPLETE, function(e:Event):void {
  var w:int = int(subLoader.contentLoaderInfo.width);
  var h:int = int(subLoader.contentLoaderInfo.height);
  owner.statusText.text = "PASS · Sound + AVI NetStream + Loader mounted " + w + "x" + h + " child app";
  owner.detailText.text = "Mounted subapp/bundle.js via flash.display.Loader (channel active=" + (channel != null) + ")";
  trace("[PASS] as3/07-sound-video-loadpublish mounted=" + w + "x" + h);
});

this.statusText.text = "MOUNTING · Loading subapp/bundle.js via Loader...";
subLoader.load(new URLRequest("subapp/bundle.js"));
"#;

    let root = Symbol {
        id: "scene".into(),
        name: "Main".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![single_key_layer("main_l", "Dashboard", elements)],
            scripts: vec![as3_frame_script(0, script)],
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    };

    let wav_bytes = make_harmonic_stereo_wav(350, &[587.33, 739.99, 880.0], false);
    let avi_bytes = make_sample_mjpeg_avi();

    let mut project = base_dashboard_project(
        "AS3 07 · Sound, Video & loadPublish",
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

pub fn build_as3_08_interactive_arcade_miniapp() -> Project {
    let paddle_sym = Symbol {
        id: "paddle_sym".into(),
        name: "NeonDefenderPaddle".into(),
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
                    [129, 140, 248, 255],
                    [224, 231, 255, 255],
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
        name: "PhotonOrb".into(),
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
                    [52, 211, 153, 255],
                    [209, 250, 229, 255],
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
        "as3_08",
        "NEOFLASH AS3 SUITE · 08 / NEON DEFENDER MINI-GAME",
        "ActionScript 3 · Playable Arcade Loop (Arrows / A-D / Auto-Pilot)",
        "Typed ENTER_FRAME loop, KeyboardEvent controls, Sprite starfield, collision scoring & R-key restart.",
        [52, 211, 153, 255],
    );
    elements.push(rect_el(
        "arena_bg",
        "",
        64.0,
        242.0,
        832.0,
        250.0,
        [9, 14, 28, 255],
        [129, 140, 248, 255],
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

    let script = r#"import flash.display.MovieClip;
import flash.display.Sprite;
import flash.events.Event;
import flash.events.KeyboardEvent;
import flash.ui.Keyboard;

stop();

var arenaLeft:Number = 120;
var arenaRight:Number = 840;
var paddleY:Number = 464;
var score:int = 0;
var manualControl:Boolean = false;
var moveDir:int = 0;
var orbVx:Number = 5;
var orbVy:Number = 16;
var passTraced:Boolean = false;

var stars:Sprite = new Sprite();
var sIdx:int = 0;
while (sIdx < 18) {
  var sx:Number = 90 + ((sIdx * 97) % 780);
  var sy:Number = 258 + ((sIdx * 53) % 180);
  stars.graphics.beginFill(0xA5B4FC, 0.65);
  stars.graphics.drawRect(sx, sy, 3, 3);
  stars.graphics.endFill();
  sIdx = sIdx + 1;
}
addChild(stars);

var paddleClip:MovieClip = this.paddle as MovieClip;
var orbClip:MovieClip = this.orb as MovieClip;
var owner:Object = this;

function refreshHud():void {
  owner.statusText.text = "SCORE: " + score + " · " + (manualControl ? "MANUAL PILOT" : "AUTO-PILOT DEMO (Press Left/Right)");
  owner.detailText.text = "Paddle X=" + Math.round(paddleClip.x) + " | Orb=(" + Math.round(orbClip.x) + ", " + Math.round(orbClip.y) + ")";
}

function onKeyDown(event:KeyboardEvent):void {
  var code:uint = event.keyCode;
  if (code == Keyboard.LEFT || code == 65) {
    manualControl = true;
    moveDir = -1;
  } else if (code == Keyboard.RIGHT || code == 68) {
    manualControl = true;
    moveDir = 1;
  } else if (code == 82) {
    score = 0;
    manualControl = false;
    orbClip.x = 480;
    orbClip.y = 396;
    refreshHud();
  }
}

function onKeyUp(event:KeyboardEvent):void {
  moveDir = 0;
}

if (stage != null) {
  stage.focus = this;
  stage.addEventListener(KeyboardEvent.KEY_DOWN, onKeyDown);
  stage.addEventListener(KeyboardEvent.KEY_UP, onKeyUp);
}

addEventListener(Event.ENTER_FRAME, function(e:Event):void {
  orbClip.x = orbClip.x + orbVx;
  orbClip.y = orbClip.y + orbVy;
  if (orbClip.x < arenaLeft || orbClip.x > arenaRight) {
    orbVx = -orbVx;
  }
  if (!manualControl) {
    paddleClip.x = paddleClip.x + (orbClip.x - paddleClip.x) * 0.45;
  } else {
    paddleClip.x = Math.max(arenaLeft, Math.min(arenaRight, paddleClip.x + moveDir * 14));
  }
  if (orbClip.y >= paddleY - 18) {
    if (Math.abs(orbClip.x - paddleClip.x) <= 64) {
      score = score + 100;
      if (!passTraced) {
        passTraced = true;
        trace("[PASS] as3/08-interactive-arcade-miniapp score=" + score);
      }
    }
    orbClip.y = 268;
    orbVx = (score % 200 == 0) ? 6 : -6;
  }
  refreshHud();
});

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
            scripts: vec![as3_frame_script(0, script)],
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    };

    base_dashboard_project(
        "AS3 08 · Neon Defender Mini-Game",
        root,
        vec![paddle_sym, orb_sym],
    )
}
