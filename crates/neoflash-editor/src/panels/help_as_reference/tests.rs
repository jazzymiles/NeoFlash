use super::api_index::{describe_param, sanitize_raw_params, split_params};
use super::*;
use crate::panels::help_manual::LanguageScope;
use eframe::egui;
use neoflash_core::ScriptLanguage;
use neoflash_ui::Theme;

pub(crate) fn verify_all_reference_code_samples_compile() {
    use neoflash_core::ScriptLanguage;

    let samples: &[(&str, ScriptLanguage, &str)] = &[
        (
            "Ch.11 AS2 Named & Anonymous Functions",
            ScriptLanguage::ActionScript2,
            AS2_FN_DECLARATIONS_AND_LITERALS_SAMPLE,
        ),
        (
            "Ch.11 AS1 Untyped Prototype & Closures",
            ScriptLanguage::ActionScript1,
            "function Enemy(x, y, speed) {
    this.x = x;
    this.y = y;
    this.speed = speed;
}
Enemy.prototype.step = function() {
    this.x -= this.speed;
};
var scout = new Enemy(240, 120, 3);
scout.step();
function makeCounter(start) {
    var count = start;
    return function(delta) {
        count += delta;
        return count;
    };
}
var nextWave = makeCounter(1);
trace(scout.x, nextWave(1));",
        ),
        (
            "Ch.11 AS2 arguments + Function.call / apply",
            ScriptLanguage::ActionScript2,
            AS2_FN_ARGUMENTS_CALL_APPLY_SAMPLE,
        ),
        (
            "Ch.11 AS2 Prototype & Closures",
            ScriptLanguage::ActionScript2,
            AS2_FN_PROTOTYPE_AND_CLOSURES_SAMPLE,
        ),
        (
            "Ch.11 AS2 1-Frame Game Pattern",
            ScriptLanguage::ActionScript2,
            AS2_ONE_FRAME_GAME_LOOP_SAMPLE,
        ),
        (
            "Ch.13 AS3 Functions, ...rest, Closures & call/apply",
            ScriptLanguage::ActionScript3,
            AS3_FUNCTIONS_REST_CALL_CLOSURES_SAMPLE,
        ),
        (
            "Ch.17 JavaScript Functions, Closures & call/apply",
            ScriptLanguage::JavaScript,
            JS_FUNCTIONS_CLOSURES_AND_CALL_SAMPLE,
        ),
        (
            "Ch.18 TypeScript Typed & Generic Functions",
            ScriptLanguage::TypeScript,
            TS_TYPED_AND_GENERIC_FUNCTIONS_SAMPLE,
        ),
        (
            "Ch.11 AS2 Frame Script",
            ScriptLanguage::ActionScript2,
            "stop();
var score:Number = 0;
var gravity:Number = 0.65;
var depth:Number = this.getNextHighestDepth();
var coin:MovieClip = this.attachMovie(\"CoinSymbol\", \"coin_\" + depth, depth, {_x: 320, _y: 180});
hero_mc.vy = 0;
hero_mc.onEnterFrame = function():Void {
    this.vy += _root.gravity;
    this._y += this.vy;
    if (Key.isDown(Key.RIGHT)) { this._x += 5; this._xscale = 100; }
    if (Key.isDown(Key.LEFT))  { this._x -= 5; this._xscale = -100; }
    if (this.hitTest(_root.coin._x, _root.coin._y, true)) {
        _root.score += 100;
        _root.score_txt.text = \"Score: \" + _root.score;
        _root.coin.removeMovieClip();
    }
};",
        ),
        (
            "Ch.12 AS2 Core Classes",
            ScriptLanguage::ActionScript2,
            "import flash.display.BitmapData;
import flash.geom.Matrix;
var canvas:MovieClip = this.createEmptyMovieClip(\"badge_mc\", this.getNextHighestDepth());
canvas.lineStyle(2, 0x6C93FF, 100);
canvas.beginFill(0x1F2A48, 95);
canvas.moveTo(20, 20);
canvas.lineTo(180, 20);
canvas.lineTo(180, 70);
canvas.lineTo(20, 70);
canvas.endFill();
this.createTextField(\"label_txt\", this.getNextHighestDepth(), 32, 32, 140, 28);
label_txt.text = \"READY PLAYER ONE\";
var fmt:TextFormat = new TextFormat(\"Inter\", 14, 0xFFFFFF, true);
label_txt.setTextFormat(fmt);
var bmp:BitmapData = new BitmapData(200, 90, true, 0x00000000);
bmp.draw(canvas, new Matrix());",
        ),
        (
            "Ch.13 AS3 Display List",
            ScriptLanguage::ActionScript3,
            "package {
    import flash.display.Sprite;
    import flash.display.Shape;
    import flash.events.Event;
    import flash.geom.Point;

    public class Main extends Sprite {
        private var ship:Sprite = new Sprite();

        public function Main() {
            if (stage) init();
            else addEventListener(Event.ADDED_TO_STAGE, init);
        }

        private function init(e:Event = null):void {
            removeEventListener(Event.ADDED_TO_STAGE, init);
            var hull:Shape = new Shape();
            hull.graphics.beginFill(0x6C93FF, 1.0);
            hull.graphics.moveTo(0, -18);
            hull.graphics.lineTo(14, 14);
            hull.graphics.lineTo(-14, 14);
            hull.graphics.endFill();

            ship.addChild(hull);
            ship.x = stage.stageWidth * 0.5;
            ship.y = stage.stageHeight * 0.5;
            addChild(ship);

            var tipGlobal:Point = ship.localToGlobal(new Point(0, -18));
            trace(\"Ship nose in Stage coordinates:\", tipGlobal.x, tipGlobal.y);
        }
    }
}",
        ),
        (
            "Ch.14 AS3 Events",
            ScriptLanguage::ActionScript3,
            "import flash.events.Event;
import flash.events.MouseEvent;
import flash.events.KeyboardEvent;
import flash.events.TimerEvent;
import flash.ui.Keyboard;
import flash.utils.Timer;

card_mc.buttonMode = true;
card_mc.mouseChildren = false;
card_mc.addEventListener(MouseEvent.MOUSE_DOWN, function(e:MouseEvent):void {
    card_mc.startDrag();
});
stage.addEventListener(MouseEvent.MOUSE_UP, function(e:MouseEvent):void {
    card_mc.stopDrag();
});
stage.addEventListener(KeyboardEvent.KEY_DOWN, function(e:KeyboardEvent):void {
    if (e.keyCode == Keyboard.SPACE) {
        trace(\"Spacebar pressed! Shift held:\", e.shiftKey);
    }
});
var spawner:Timer = new Timer(500, 10);
spawner.addEventListener(TimerEvent.TIMER, function(e:TimerEvent):void {
    trace(\"Wave tick:\", spawner.currentCount);
});
spawner.start();",
        ),
        (
            "Ch.15 AS3 Graphics & E4X",
            ScriptLanguage::ActionScript3,
            "import flash.display.Sprite;
import flash.filters.GlowFilter;
import flash.media.Sound;
import flash.media.SoundChannel;
import flash.media.SoundTransform;

var orb:Sprite = new Sprite();
orb.graphics.lineStyle(2, 0x86A5FF, 1.0);
orb.graphics.beginFill(0x3C6BF0, 0.85);
orb.graphics.drawCircle(0, 0, 36);
orb.graphics.endFill();
orb.filters = [new GlowFilter(0x6C93FF, 0.9, 16, 16, 2, 2)];
addChild(orb);

var sfx:Sound = new Sound();
var channel:SoundChannel = sfx.play(0, 1, new SoundTransform(0.8, -0.25));

var inventory:XML = <items>
    <weapon id=\"plasma\" dmg=\"45\"/>
    <weapon id=\"railgun\" dmg=\"90\"/>
</items>;
var heavy:XMLList = inventory.weapon.(Number(@dmg) >= 50);
trace(\"Selected weapon:\", heavy.@id);",
        ),
        (
            "Ch.17 JavaScript Runtime & JSX",
            ScriptLanguage::JavaScript,
            "stop();
trace(\"Entering level:\", this.currentFrame, \"of\", this.totalFrames);

const state = { score: 0, combo: 1 };
const badge = <hud-badge label=\"SCORE\" value={state.score} />;

this.start_btn.addEventListener(\"click\", (event) => {
    trace(\"Clicked target:\", event.target.name, \"at\", event.stageX, event.stageY);
    gotoAndPlay(\"gameplay\");
});

this.hero_mc.addEventListener(\"enterFrame\", () => {
    this.hero_mc.x += 2.5;
    this.hero_mc.alpha = Math.min(1.0, (this.hero_mc.alpha ?? 0.8) + 0.01);
});",
        ),
        (
            "Ch.18 TypeScript & Built-In Checker",
            ScriptLanguage::TypeScript,
            "interface Actor {
    readonly id: string;
    x: number;
    y: number;
    hp: number;
}

interface ActorPatch {
    x?: number;
    y?: number;
    hp?: number;
}

function applyPatch(actor: Actor, patch: ActorPatch): Actor {
    return {
        id: actor.id,
        x: patch.x ?? actor.x,
        y: patch.y ?? actor.y,
        hp: patch.hp ?? actor.hp,
    };
}

const hero: Actor = { id: \"p1\", x: 120, y: 240, hp: 100 };
const moved: Actor = applyPatch(hero, { x: 160, hp: 95 });
trace(\"Updated hero:\", moved.id, moved.x, moved.hp);",
        ),
        (
            "Ch.19 TypeScript + Web APIs",
            ScriptLanguage::TypeScript,
            "interface HighScoreEntry {
    player: string;
    score: number;
}

async function syncHighScore(canvas: HTMLCanvasElement, url: string) {
    const ctx = canvas.getContext(\"2d\");
    if (ctx) {
        ctx.save();
        ctx.fillStyle = \"rgb(31, 42, 72)\";
        ctx.fillRect(0, 0, 240, 48);
        ctx.beginPath();
        ctx.arc(24, 24, 12, 0, Math.PI * 2);
        ctx.fill();
        ctx.restore();
    }

    try {
        const res = await fetch(url);
        const top: HighScoreEntry = await res.json();
        window.localStorage.setItem(\"neoflash.topScore\", JSON.stringify(top));
        trace(\"Synced top score:\", top.player, top.score);
    } catch (err) {
        trace(\"Offline or blocked by CORS; using cached score:\", err);
    }
}",
        ),
    ];

    let base_project = neoflash_core::Project::blank();
    let base_program = neoflash_script::compile_project(&base_project)
        .expect("blank project should compile cleanly");

    for (name, lang, code) in samples {
        if code.trim_start().starts_with("package") {
            let mut pkg_project = neoflash_core::Project::blank();
            pkg_project.sources.push(neoflash_core::SourceUnit {
                id: "src-main".to_string(),
                path: "Main.as".to_string(),
                language: *lang,
                source: (*code).to_owned(),
            });
            let compiled = neoflash_script::compile_project(&pkg_project);
            assert!(
                compiled.is_ok(),
                "{name} ({lang:?}) package source produced compile errors: {:?}",
                compiled.err()
            );
        } else {
            let script = neoflash_core::Script {
                language: *lang,
                source: (*code).to_owned(),
            };
            let compiled = neoflash_script::compile_with_program(&script, &base_program);
            assert!(
                compiled.is_ok(),
                "{name} ({lang:?}) produced compile errors: {:?}",
                compiled.err()
            );
        }
    }

    // Also verify multi-file JS/TS module bundling (`import` / `export`) from Chapter 17
    let mut project = neoflash_core::Project::blank();
    project.sources.push(neoflash_core::SourceUnit {
        id: "src-math".to_string(),
        path: "math.ts".to_string(),
        language: ScriptLanguage::TypeScript,
        source: "export function clamp(v: number, lo: number, hi: number): number { return Math.max(lo, Math.min(hi, v)); }".to_string(),
    });
    let program = neoflash_script::compile_project(&project)
        .expect("Ch.17/18 module project should compile cleanly");
    let handler_script = neoflash_core::Script {
        language: ScriptLanguage::TypeScript,
        source:
            "import { clamp } from \"./math\";\nconst x: number = clamp(140, 0, 100);\ntrace(x);"
                .to_string(),
    };
    let module_res = neoflash_script::compile_with_program(&handler_script, &program);
    assert!(
        module_res.is_ok(),
        "Ch.17/18 module graph example produced errors: {:?}",
        module_res.err()
    );
}

#[test]
fn help_as_reference_code_samples_all_compile() {
    verify_all_reference_code_samples_compile();
}

#[test]
fn help_as_reference_function_search_covers_all_as2_as3_js_ts_cases_and_esc_chords() {
    assert_eq!(CLASSIC_ESC_CHORDS.len(), 50);
    assert_eq!(CLASSIC_ESC_CHORDS[0].0, "br");
    assert_eq!(
        CLASSIC_ESC_CHORDS[12],
        (
            "fn",
            "function name ( ) { ... }",
            "Declare named function block"
        )
    );
    assert_eq!(CLASSIC_ESC_CHORDS[49].0, "wt");

    for query in ["function", "Function"] {
        let (as2_hits, other_count) =
            search_reference_hits(query, LanguageScope::As1As2, ScriptLanguage::ActionScript2);
        assert!(
            other_count >= 3,
            "AS1/AS2 scope should filter out AS3/JS/TS hits while reporting hidden count"
        );
        for hit in &as2_hits {
            assert!(
                matches!(hit.dialect_badge, "[AS1/AS2]" | "[Authoring]"),
                "unexpected dialect badge in AS1/AS2 scope: {}",
                hit.dialect_badge
            );
        }
        let combined_text: String = as2_hits
            .iter()
            .map(|h| {
                format!(
                    "{} {} {} {}",
                    h.title,
                    h.signature,
                    h.summary,
                    h.code_preview.unwrap_or("")
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        for required_case in [
            "calcDamage",
            "return",
            "easeOutQuad",
            "arguments.length",
            "arguments.callee",
            "arguments.caller",
            "call(",
            "apply(",
            "Enemy.prototype",
            "makeCounter",
            "onEnterFrame",
            "setInterval",
            "<ESC>fn",
        ] {
            assert!(
                combined_text.contains(required_case),
                "searching '{query}' in AS1/AS2 scope missed required case: {required_case}"
            );
        }

        let (as3_hits, _) =
            search_reference_hits(query, LanguageScope::As3, ScriptLanguage::ActionScript3);
        assert!(
            as3_hits
                .iter()
                .any(|h| h.dialect_badge == "[AS3]" && h.signature.contains("...values")),
            "searching '{query}' in AS3 scope must surface ...rest & AS3 Function reference"
        );

        let (jsts_hits, _) =
            search_reference_hits(query, LanguageScope::JsTs, ScriptLanguage::TypeScript);
        assert!(
            jsts_hits.iter().any(|h| h.dialect_badge == "[JS]")
                && jsts_hits.iter().any(|h| h.dialect_badge == "[TS]"),
            "searching '{query}' in JS/TS scope must surface both [JS] and [TS] hits"
        );
    }

    let (chord_hits, _) = search_reference_hits(
        "<ESC>fn",
        LanguageScope::As1As2,
        ScriptLanguage::ActionScript2,
    );
    assert!(
        chord_hits
            .iter()
            .any(|h| h.signature.contains("<ESC>fn") && h.signature.contains("function name")),
        "searching '<ESC>fn' must surface the <ESC>fn chord"
    );
}

#[test]
fn help_as_reference_browser_html_includes_project_language_scope_and_esc_chords() {
    let html = render_browser_help_html_for_project(
        Theme::Dark,
        Some(crate::panels::help_manual::HelpManualMode::ScriptingReference),
        Some(10),
        None,
        Some(ScriptLanguage::ActionScript2),
        LanguageScope::As1As2,
    );
    assert!(html.contains("Current project: ActionScript 2.0"));
    assert!(html.contains("data-initial-lang=\"AS1_AS2\""));
    assert!(
        html.contains("id=\"nf2-lang-AS1_AS2\" data-lang=\"AS1_AS2\" class=\"sr-toggle\" checked")
    );
    assert!(html.contains("[AS1/AS2]"));
    assert!(html.contains("[AS3]"));
    assert!(html.contains("[JS]"));
    assert!(html.contains("[TS]"));
    assert!(html.contains("calcDamage"));
    assert!(html.contains("arguments.callee"));
    assert!(html.contains("Enemy.prototype.step"));
    assert!(html.contains("&lt;ESC&gt;fn"));
    assert!(!html.contains("<script"));
}

#[test]
fn t389_api_search_orders_exact_prefix_substring_without_a_cap() {
    let hits = api_search(LanguageScope::As3, "gotoAndPlay");
    assert!(
        hits.iter()
            .any(|h| h.label == "MovieClip.gotoAndPlay()" && h.rank == 0),
        "{hits:?}"
    );

    let hits = api_search(LanguageScope::As3, "event");
    assert!(hits.len() > 200, "search is not capped: {}", hits.len());
    assert!(hits.windows(2).all(|pair| pair[0].rank <= pair[1].rank));
    assert_eq!((hits[0].label.as_str(), hits[0].rank), ("Event", 0));
    let rank_of = |label: &str| hits.iter().find(|h| h.label == label).map(|h| h.rank);
    assert_eq!(rank_of("EventDispatcher"), Some(1));
    assert_eq!(rank_of("MouseEvent"), Some(2));
    assert_eq!(rank_of("MovieClip.gotoAndPlay()"), None);

    // `Class.member` narrows the members to that class.
    let hits = api_search(LanguageScope::As1As2, "MovieClip.attachMovie");
    assert_eq!(hits[0].label, "MovieClip.attachMovie()");
    assert_eq!(hits[0].rank, 0);
    // ActionScript 2 keeps its timeline functions at the top level.
    let hits = api_search(LanguageScope::As1As2, "gotoAndPlay");
    assert!(
        hits.iter()
            .any(|h| h.class_key == API_GLOBAL_KEY && h.label == "gotoAndPlay()")
    );
    assert!(hits.iter().any(|h| h.label == "MovieClip.gotoAndPlay()"));

    // JavaScript / TypeScript searches its own catalogue.
    let hits = api_search(LanguageScope::JsTs, "trace");
    assert_eq!((hits[0].label.as_str(), hits[0].rank), ("trace", 0));
    assert!(api_search(LanguageScope::JsTs, "attachMovie").is_empty());
}

#[test]
fn t389_api_index_gives_every_actionscript_member_a_status_and_lists_missing_ones() {
    for scope in [LanguageScope::As1As2, LanguageScope::As3] {
        let index = api_index(scope);
        assert!(!index.flat);
        assert_eq!(index.status_count, index.member_count, "{scope:?}");
        assert_eq!(index.packages[0].label, "Top level");
        let listed: usize = index.packages.iter().map(|p| p.classes.len()).sum();
        assert_eq!(listed, index.class_count);
        for (key, _) in index.packages.iter().flat_map(|p| &p.classes) {
            let class = index.class(key).expect("listed class");
            let mut keys: Vec<&str> = class.members.iter().map(|m| m.key.as_str()).collect();
            keys.sort_unstable();
            keys.dedup();
            assert_eq!(
                keys.len(),
                class.members.len(),
                "{key} has duplicate member keys"
            );
        }
        // The reference repeats no package (`flash.filters.flash.filters`) and shows no
        // package declaration as a class.
        assert!(index.packages.iter().all(|p| {
            let parts: Vec<&str> = p.label.split('.').collect();
            parts.len() % 2 == 1 || parts[..parts.len() / 2] != parts[parts.len() / 2..]
        }));
    }
    let js = api_index(LanguageScope::JsTs);
    assert!(js.flat);
    assert_eq!(js.class_count, JS_TS_CATEGORIES.len());
    assert_eq!(js.member_count, JS_TS_API_ENTRIES.len());
    assert_eq!(js.status_count, js.member_count);

    let movie_clip = api_index(LanguageScope::As3)
        .class("flash.display.MovieClip")
        .expect("MovieClip");
    assert_eq!(movie_clip.package, "flash.display");
    assert_eq!(movie_clip.chain[0].0, "Sprite");
    assert_eq!(
        movie_clip.chain[0].1.as_deref(),
        Some("flash.display.Sprite")
    );
    let (working, partial, missing) = movie_clip.support_counts();
    assert_eq!(working + partial + missing, movie_clip.members.len());

    let net_stream = api_index(LanguageScope::As1As2)
        .class("NetStream")
        .expect("NetStream");
    let attach = net_stream
        .members
        .iter()
        .find(|m| m.name == "attachAudio")
        .expect("NetStream.attachAudio");
    assert_eq!(attach.support, Some(Support::Working));
    assert_eq!(attach.row_signature, "(theMicrophone:Microphone):Void");
    // Missing declarations (e.g. out-of-target components/extensions) are listed with their status.
    for scope in [LanguageScope::As1As2, LanguageScope::As3] {
        assert!(
            api_index(scope)
                .classes
                .values()
                .flat_map(|class| &class.members)
                .any(|m| m.support == Some(Support::Missing))
        );
    }
    // A partial declaration carries its catalogue note.
    assert!(
        api_index(LanguageScope::As3)
            .classes
            .values()
            .flat_map(|class| &class.members)
            .any(|m| m.support == Some(Support::Partial) && !m.note.is_empty())
    );
}

#[test]
fn t389_member_views_describe_parameters_and_examples() {
    assert_eq!(
        split_params("a:Array, b:Vector.<int>, c:String = \"x,y\""),
        ["a:Array", "b:Vector.<int>", "c:String = \"x,y\""]
    );
    assert_eq!(
        describe_param("scene:String = null"),
        ("scene".into(), "String, optional (default null)".into())
    );
    assert_eq!(
        describe_param("[value:Object]"),
        ("value".into(), "Object, optional".into())
    );
    assert_eq!(describe_param("... args").0, "...args");
    assert_eq!(group_thousands(5173), "5 173");
    assert_eq!(group_thousands(563), "563");
    assert_eq!(group_thousands(1_234_567), "1 234 567");

    let index = api_index(LanguageScope::As3);
    let point = index.class("flash.geom.Point").expect("Point");
    assert!(
        !point.example.is_empty(),
        "non-pseudo classes carry a class summary example"
    );
    let constructor = point
        .members
        .iter()
        .find(|m| m.kind == "constructor")
        .expect("Point constructor");
    assert_eq!(constructor.usage, "var point:Point = new Point(x, y);");
    let distance = point
        .members
        .iter()
        .find(|m| m.name == "distance")
        .expect("Point.distance");
    assert_eq!(distance.kind, "static method");
    assert_eq!(distance.usage, "Point.distance(pt1, pt2);");
    assert_eq!(distance.returns, "Number");

    let array = index.class("Array").expect("Array");
    let last_index_of = array
        .members
        .iter()
        .find(|m| m.name == "lastIndexOf")
        .expect("Array.lastIndexOf");
    assert_eq!(
        last_index_of.usage,
        "array.lastIndexOf(searchElement, fromIndex);"
    );
    assert_eq!(
        last_index_of.declaration,
        "public function lastIndexOf(\n    searchElement:*,\n    fromIndex:int = 0x7fffffff\n):int"
    );

    let byte_array = index.class("flash.utils.ByteArray").expect("ByteArray");
    let read_u8 = byte_array
        .members
        .iter()
        .find(|m| m.name == "readUnsignedByte")
        .expect("ByteArray.readUnsignedByte");
    assert_eq!(read_u8.usage, "byteArray.readUnsignedByte();");

    let mc = index.class("flash.display.MovieClip").expect("MovieClip");
    let current_frame = mc
        .members
        .iter()
        .find(|m| m.name == "currentFrame")
        .expect("MovieClip.currentFrame");
    assert_eq!(current_frame.usage, "movieClip.currentFrame");
    assert_eq!(
        current_frame.declaration,
        "public function get currentFrame():int"
    );
    assert_eq!(current_frame.row_signature, ": int  ·  read-only");

    let disp = index
        .class("flash.display.DisplayObject")
        .expect("DisplayObject");
    let prop_x = disp
        .members
        .iter()
        .find(|m| m.name == "x")
        .expect("DisplayObject.x");
    assert_eq!(prop_x.usage, "displayObject.x = value;");
    assert_eq!(prop_x.declaration, "public var x:Number");

    let mouse_ev = index.class("flash.events.MouseEvent").expect("MouseEvent");
    let click = mouse_ev
        .members
        .iter()
        .find(|m| m.name == "CLICK")
        .expect("MouseEvent.CLICK");
    assert_eq!(click.usage, "MouseEvent.CLICK");
    assert_eq!(
        click.declaration,
        "public static const CLICK:String = \"click\""
    );

    let math = index.class("Math").expect("Math");
    let pi = math
        .members
        .iter()
        .find(|m| m.name == "PI")
        .expect("Math.PI");
    assert_eq!(pi.usage, "Math.PI");
    assert!(
        pi.declaration
            .starts_with("public static const PI:Number = 3.14159"),
        "unexpected Math.PI declaration: {}",
        pi.declaration
    );

    let dispatcher = index
        .class("flash.events.EventDispatcher")
        .expect("EventDispatcher");
    let activate = dispatcher
        .members
        .iter()
        .find(|m| m.name == "activate")
        .expect("EventDispatcher.activate");
    assert_eq!(
        activate.usage,
        "eventDispatcher.addEventListener(\"activate\", listener);"
    );
    assert_eq!(
        activate.declaration,
        "[Event(name=\"activate\", type=\"flash.events.Event\")]"
    );

    let globals = index.class(API_GLOBAL_KEY).expect("AS3 globals");
    let nan = globals
        .members
        .iter()
        .find(|m| m.name == "NaN")
        .expect("NaN");
    assert_eq!(nan.usage, "NaN");
    let flash_utils = index.class("flash.utils").expect("flash.utils");
    let get_timer = flash_utils
        .members
        .iter()
        .find(|m| m.name == "getTimer")
        .expect("getTimer");
    assert_eq!(get_timer.usage, "getTimer();");

    let as2 = api_index(LanguageScope::As1As2);
    let as2_mc = as2.class("MovieClip").expect("AS2 MovieClip");
    let as2_cf = as2_mc
        .members
        .iter()
        .find(|m| m.name == "_currentframe")
        .expect("AS2 _currentframe");
    assert_eq!(
        as2_cf.declaration,
        "public var _currentframe:Number // read-only"
    );
    assert_eq!(as2_cf.row_signature, ": Number  ·  read-only");
    let as2_enter_frame = as2_mc
        .members
        .iter()
        .find(|m| m.name == "onEnterFrame")
        .expect("AS2 onEnterFrame");
    assert_eq!(
        as2_enter_frame.usage,
        "movieClip.onEnterFrame = function():Void {};"
    );
    assert_eq!(
        as2_enter_frame.declaration,
        "public onEnterFrame = function() {}"
    );

    // Quote-aware parameter sanitization & scraped overload truncation (T-440 #3 & #5).
    assert_eq!(sanitize_raw_params("):Object; Color.setTransform("), "");
    assert_eq!(
        sanitize_raw_params("offset:Number):Void; Color.setTransform("),
        "offset:Number"
    );
    assert_eq!(
        sanitize_raw_params("\"Flash JavaScript API command;\":String"),
        "\"Flash JavaScript API command;\":String"
    );
    let as2_color = as2.class("Color").expect("AS2 Color");
    let get_transform = as2_color
        .members
        .iter()
        .find(|m| m.name == "getTransform")
        .expect("Color.getTransform");
    assert_eq!(get_transform.row_signature, "():Object");
    assert_eq!(get_transform.usage, "color.getTransform();");
    assert_eq!(
        get_transform.declaration,
        "public function getTransform():Object"
    );
    let as2_globals = as2.class(API_GLOBAL_KEY).expect("AS2 globals");
    let mm_exec = as2_globals
        .members
        .iter()
        .find(|m| m.name == "MMExecute")
        .expect("AS2 MMExecute");
    assert_eq!(
        mm_exec.declaration,
        "function MMExecute(\"Flash JavaScript API command;\":String):String"
    );
    let video_ev = index
        .class("flash.events.VideoEvent")
        .expect("AS3 VideoEvent");
    let codec_info = video_ev
        .members
        .iter()
        .find(|m| m.name == "codecInfo")
        .expect("VideoEvent.codecInfo");
    assert_eq!(codec_info.usage, "videoEvent.codecInfo");
    assert_eq!(codec_info.declaration, "public const codecInfo:String");

    let js = api_index(LanguageScope::JsTs);
    let ctx_cat = js
        .class("NeoFlash Context & Timeline")
        .expect("NeoFlash Context & Timeline");
    let js_goto = ctx_cat
        .members
        .iter()
        .find(|m| m.name == "gotoAndPlay")
        .expect("JS gotoAndPlay");
    assert_eq!(
        js_goto.declaration,
        "gotoAndPlay(\n    frameOrLabel: number | string,\n    scene?: string\n): void"
    );
    let ecma_cat = js.class("ECMAScript & Async").expect("ECMAScript & Async");
    let js_json = ecma_cat
        .members
        .iter()
        .find(|m| m.name == "JSON")
        .expect("JSON");
    assert!(
        !js_json.declaration.contains('\n'),
        "multi-variant '|' JS/TS signatures stay single-line: {}",
        js_json.declaration
    );

    assert!(api_footer_text(LanguageScope::As3).starts_with("ActionScript 3 · "));
}

#[test]
fn t397_every_api_class_and_member_has_description_and_js_ts_status_coverage() {
    const FORBIDDEN_GLYPHS: &[char] = &['⌘', '⌥', '⇧', '⌃', '↵', '⌤', '⌫', '⌦', '⇥'];
    for scope in [
        LanguageScope::As1As2,
        LanguageScope::As3,
        LanguageScope::JsTs,
    ] {
        let index = api_index(scope);
        assert!(index.class_count > 0);
        assert!(index.member_count > 0);
        for (class_key, class) in &index.classes {
            let desc = class.description.trim();
            assert!(
                !desc.is_empty() && desc.len() >= 20,
                "{scope:?} class {class_key} has empty or trivial description: {desc:?}"
            );
            assert!(
                !desc.chars().any(|c| FORBIDDEN_GLYPHS.contains(&c)),
                "{scope:?} class {class_key} description contains literal shortcut glyph: {desc}"
            );
            for member in &class.members {
                let mdesc = member.description.trim();
                assert!(
                    !mdesc.is_empty() && mdesc.len() >= 16,
                    "{scope:?} member {class_key}.{} has empty or trivial description: {mdesc:?}",
                    member.key
                );
                assert!(
                    !mdesc.chars().any(|c| FORBIDDEN_GLYPHS.contains(&c)),
                    "{scope:?} member {class_key}.{} description contains literal shortcut glyph: {mdesc}",
                    member.key
                );
            }
        }
    }

    // 100% of JS/TS entries carry an explicit Support::Working or Support::Partial status.
    let js = api_index(LanguageScope::JsTs);
    assert_eq!(js.status_count, js.member_count);
    let mut js_working = 0usize;
    let mut js_partial = 0usize;
    for class in js.classes.values() {
        assert!(matches!(class.status.as_str(), "subset" | "partial"));
        for member in &class.members {
            match member.support {
                Some(Support::Working) => js_working += 1,
                Some(Support::Partial) => {
                    js_partial += 1;
                    assert!(
                        !member.note.trim().is_empty(),
                        "partial JS/TS entry {} must carry a boundary note",
                        member.name
                    );
                }
                other => panic!(
                    "unexpected JS/TS support status {other:?} on {}",
                    member.name
                ),
            }
        }
    }
    assert!(js_working >= 30 && js_partial >= 2);
    assert_eq!(js_working + js_partial, JS_TS_API_ENTRIES.len());

    let js_footer = api_footer_text(LanguageScope::JsTs);
    assert!(
        js_footer.contains(&format!("{js_working} working"))
            && js_footer.contains(&format!("{js_partial} partial")),
        "JS/TS footer must report working & partial counts: {js_footer}"
    );

    // Description search surfaces conceptual hits at rank 3 behind name hits (0..=2).
    let perlin_hits = api_search(LanguageScope::As3, "perlin");
    assert!(perlin_hits.iter().any(|h| h.rank <= 2));
    let bezier_hits = api_search(LanguageScope::As3, "bezier");
    assert!(
        bezier_hits
            .iter()
            .any(|h| h.rank == 3 && h.label == "Graphics.curveTo()"),
        "expected description rank-3 hits for 'bezier' in AS3: {bezier_hits:?}"
    );
    let cors_hits = api_search(LanguageScope::JsTs, "cors");
    assert!(
        cors_hits
            .iter()
            .any(|h| h.rank == 3 && (h.label == "fetch" || h.label == "drawImage")),
        "expected description/note rank-3 hits for 'cors' in JS/TS: {cors_hits:?}"
    );

    // Browser HTML edition surfaces JS/TS statuses, boundary notes, and AS2/AS3 descriptions.
    let html = render_browser_help_html_for_project(
        Theme::Dark,
        Some(crate::panels::help_manual::HelpManualMode::ScriptingReference),
        Some(16),
        None,
        Some(ScriptLanguage::TypeScript),
        LanguageScope::JsTs,
    );
    assert!(html.contains("class=\"status-working\">Working</span>"));
    assert!(html.contains("class=\"status-partial\">Partial</span>"));
    assert!(html.contains("SourceUnit"));
    assert!(html.contains("AuthoredPlayback"));
    assert!(html.contains(&api_footer_text(LanguageScope::JsTs)));
    assert!(
        html.contains("ActionScript 3.0 Display List, Top-Level &amp; Text Class Descriptions")
    );
    assert!(!html.contains("<script"));

    // Headless egui pass: detail pane renders class.description, member.description, and JS/TS badges.
    let ctx = egui::Context::default();
    let _ = crate::app::Editor::from_project(&ctx, neoflash_core::Project::blank());
    let mut state = ApiBrowserState::default();
    state.select(LanguageScope::As3, "flash.display.MovieClip", None);
    let class_out = ctx.run(egui::RawInput::default(), |ctx| {
        let mut actions = RefActions::default();
        egui::CentralPanel::default().show(ctx, |ui| {
            api_detail_column(ui, &mut state, LanguageScope::As3, &mut actions);
        });
    });
    let class_texts: String = class_out
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        class_texts.contains("multi-frame timeline"),
        "class_summary must render class.description: {class_texts}"
    );

    state.select(
        LanguageScope::JsTs,
        "Modules & JSX",
        Some("Modules & JSX|import"),
    );
    let js_out = ctx.run(egui::RawInput::default(), |ctx| {
        let mut actions = RefActions::default();
        egui::CentralPanel::default().show(ctx, |ui| {
            api_detail_column(ui, &mut state, LanguageScope::JsTs, &mut actions);
        });
    });
    let js_texts: String = js_out
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        js_texts.contains("Partial") && js_texts.contains("SourceUnit"),
        "JS/TS detail column must render Partial badge and boundary note: {js_texts}"
    );
}

#[test]
fn t440_api_detail_renders_usage_before_signature_before_example_and_all_copy_buttons_work() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let _ = crate::app::Editor::from_project(&ctx, neoflash_core::Project::blank());
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(520.0, 1400.0));

    let index = api_index(LanguageScope::As3);
    let array = index.class("Array").expect("Array");
    let member = array
        .members
        .iter()
        .find(|m| m.name == "lastIndexOf")
        .expect("Array.lastIndexOf");
    assert_ne!(member.usage, member.declaration);
    assert_ne!(member.usage, member.example);

    let mut state = ApiBrowserState::default();
    state.select(LanguageScope::As3, "Array", Some(&member.key));

    let run_pass = |state: &mut ApiBrowserState, events: Vec<egui::Event>| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            },
            |ctx| {
                let mut actions = RefActions::default();
                egui::CentralPanel::default().show(ctx, |ui| {
                    api_detail_column(ui, state, LanguageScope::As3, &mut actions);
                });
            },
        )
    };

    let _ = run_pass(&mut state, vec![]);
    let out = run_pass(&mut state, vec![]);

    let find_y = |needle: &str| -> f32 {
        out.shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t)
                    if t.galley.text() == needle
                        || t.galley.text().eq_ignore_ascii_case(needle) =>
                {
                    Some(t.pos.y)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("expected painted text {needle:?}"))
    };

    let y_usage_cap = find_y("Usage");
    let y_usage_code = find_y(&member.usage);
    let y_sig_cap = find_y("Signature");
    let y_sig_code = find_y(&member.declaration);
    let y_desc = find_y(&member.description);
    let y_params = find_y("Parameters");
    let y_returns = find_y("Returns");
    let y_status = find_y("In NeoFlash");
    let y_example_cap = find_y("Example");
    let y_example_code = find_y(&member.example);

    assert!(y_usage_cap < y_usage_code);
    assert!(y_usage_code < y_sig_cap);
    assert!(y_sig_cap < y_sig_code);
    assert!(y_sig_code < y_desc);
    assert!(y_desc < y_params);
    assert!(y_params < y_returns);
    assert!(y_returns < y_status);
    assert!(y_status < y_example_cap);
    assert!(y_example_cap < y_example_code);

    let copy_button_centers = |out: &egui::FullOutput| -> Vec<egui::Pos2> {
        let mut rects: Vec<egui::Rect> = out
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("accesskit")
            .nodes
            .iter()
            .filter_map(|(_, node)| {
                if node.role() == egui::accesskit::Role::Button && node.label() == Some("Copy") {
                    let b = node.bounds()?;
                    Some(egui::Rect::from_min_max(
                        egui::pos2(b.x0 as f32, b.y0 as f32),
                        egui::pos2(b.x1 as f32, b.y1 as f32),
                    ))
                } else {
                    None
                }
            })
            .collect();
        rects.sort_by(|a, b| a.min.y.total_cmp(&b.min.y));
        rects.into_iter().map(|r| r.center()).collect()
    };

    let centers = copy_button_centers(&out);
    assert_eq!(
        centers.len(),
        4,
        "expected Usage icon Copy, Signature icon Copy, Example icon Copy, and bottom Copy button"
    );

    let click_and_read_copied = |state: &mut ApiBrowserState, pos: egui::Pos2| -> String {
        let btn = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let _ = run_pass(state, vec![egui::Event::PointerMoved(pos)]);
        let _ = run_pass(state, vec![btn(true)]);
        let release_out = run_pass(state, vec![btn(false)]);
        release_out
            .platform_output
            .commands
            .iter()
            .find_map(|cmd| match cmd {
                egui::OutputCommand::CopyText(t) => Some(t.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("clicking Copy at {pos:?} must emit CopyText"))
    };

    assert_eq!(click_and_read_copied(&mut state, centers[0]), member.usage);
    assert_eq!(
        click_and_read_copied(&mut state, centers[1]),
        member.declaration
    );
    assert_eq!(
        click_and_read_copied(&mut state, centers[2]),
        member.example
    );
    assert_eq!(
        click_and_read_copied(&mut state, centers[3]),
        member.example
    );

    // Class summary (no member selected) also renders its Example code box with working Copy buttons.
    let point = index.class("flash.geom.Point").expect("Point");
    state.select(LanguageScope::As3, "flash.geom.Point", None);
    let _ = run_pass(&mut state, vec![]);
    let class_out = run_pass(&mut state, vec![]);
    let class_centers = copy_button_centers(&class_out);
    assert_eq!(
        class_centers.len(),
        2,
        "class_summary must expose Example header Copy icon button and bottom Copy button"
    );
    assert_eq!(
        click_and_read_copied(&mut state, class_centers[0]),
        point.example
    );
    assert_eq!(
        click_and_read_copied(&mut state, class_centers[1]),
        point.example
    );
}

#[test]
fn t440_browser_html_renders_declarations_examples_output_comments_and_selectable_css() {
    let html = render_full_browser_help_html(Theme::Dark);
    assert!(!html.contains("<script"));
    assert!(html.contains("Member &amp; Signature"));
    assert!(html.contains("Description &amp; Example"));
    assert!(html.contains("Description, Boundary &amp; Example"));
    assert!(html.contains("public function "));
    assert!(html.contains("// Output: Opening hit: 48"));
    assert!(html.contains("// Output: AS3 fn results: 50 60 2 20 12"));
    assert!(html.contains("// Output: 40 0.75 30 101 55 70"));
    assert!(html.contains("// Output: 65 0.75 laser 1"));
    assert!(html.contains("user-select:text"));
}
