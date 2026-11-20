use super::api_browser_ui::code_sample_actions;
use super::*;
use crate::panels::help_manual::{
    HELP_BROWSER_TOPICS, HELP_CHAPTERS, LanguageScope, outline_heading, paragraph, section_overline,
};
use eframe::egui::{self, Rect, Stroke, StrokeKind, Ui, vec2};
use neoflash_ui::{
    TextStyle,
    tokens::{radius, space},
    typography::{font, rich},
};

/// Complete Classic Flash 50 two-key `<ESC>` code-insertion chords (`<ESC>` followed sequentially
/// by two characters inside the `Actions` or Project source editor).
pub(crate) const CLASSIC_ESC_CHORDS: [(&str, &str, &str); 50] = [
    ("br", "break;", "Exit current loop or switch"),
    ("ca", "call ( );", "Legacy Flash 4 frame-script call"),
    ("//", "// ", "Insert single-line comment"),
    ("co", "continue;", "Advance loop to next iteration"),
    ("da", "delete ( );", "Delete dynamic object property"),
    (
        "do",
        "do { ... } while ( condition );",
        "Post-condition do..while loop",
    ),
    (
        "dm",
        "duplicateMovieClip ( target, newname, depth );",
        "Clone stage MovieClip instance",
    ),
    ("el", "} else {", "Fallback branch after if block"),
    (
        "ei",
        "} else if ( condition ) {",
        "Chained conditional branch",
    ),
    (
        "fr",
        "for ( init; condition; next ) { }",
        "Counted for loop",
    ),
    (
        "fi",
        "for ( iterator in object ) { }",
        "Enumerate dynamic keys / children",
    ),
    (
        "fs",
        "fscommand ( command, arguments );",
        "Host player command bridge",
    ),
    (
        "fn",
        "function name ( ) { ... }",
        "Declare named function block",
    ),
    (
        "gu",
        "getURL ( url, window, method );",
        "Open URL or browser target",
    ),
    ("gp", "gotoAndPlay ( frame );", "Jump playhead and play"),
    ("gs", "gotoAndStop ( frame );", "Jump playhead and stop"),
    ("if", "if ( condition ) { }", "Conditional branch"),
    (
        "il",
        "ifFrameLoaded ( frame ) { ... }",
        "Legacy frame-stream check",
    ),
    (
        "in",
        "#include \"path\"",
        "Compile-time external script include",
    ),
    (
        "lm",
        "loadMovie ( url, target, method );",
        "Load SWF / media / bundle into clip",
    ),
    (
        "ln",
        "loadMovieNum ( url, level, method );",
        "Load asset into _levelN",
    ),
    (
        "lv",
        "loadVariables ( url, target, method );",
        "Load URL-encoded variables into clip",
    ),
    (
        "vn",
        "loadVariablesNum ( url, level, method );",
        "Load URL-encoded variables into _levelN",
    ),
    ("nf", "nextFrame ();", "Step forward 1 frame and stop"),
    ("ns", "nextScene ();", "Jump to next Scene"),
    (
        "on",
        "on ( event ) { ... }",
        "Button / instance pointer or key handler",
    ),
    (
        "oc",
        "onClipEvent ( event ) { }",
        "MovieClip load / enterFrame handler",
    ),
    ("pl", "play ( );", "Resume timeline playback"),
    ("pf", "prevFrame ( );", "Step back 1 frame and stop"),
    ("ps", "prevScene ( );", "Jump to previous Scene"),
    ("pr", "print ( target, type );", "Vector print job"),
    (
        "pb",
        "printAsBitmap ( target, type );",
        "Bitmap-rasterized print job",
    ),
    (
        "bn",
        "printAsBitmapNum ( level, type );",
        "Bitmap print _levelN",
    ),
    ("pn", "printNum ( level, type );", "Vector print _levelN"),
    (
        "rm",
        "removeMovieClip ( target );",
        "Remove dynamic-depth MovieClip",
    ),
    ("rt", "return ( );", "Return value from active function"),
    (
        "sv",
        "set ( name, value );",
        "Legacy dynamic variable assignment",
    ),
    (
        "sp",
        "setProperty ( target, property, value );",
        "Legacy MovieClip property setter",
    ),
    (
        "dr",
        "startDrag ( target, lockcenter, l, t, r, b );",
        "Begin interactive MovieClip drag",
    ),
    ("st", "stop ( );", "Halt active timeline playhead"),
    ("ss", "stopAllSounds ( );", "Silence all playing audio"),
    ("sd", "stopDrag ( );", "End active MovieClip drag"),
    (
        "tt",
        "tellTarget ( target ) { }",
        "Legacy slash/dot timeline target block",
    ),
    (
        "tq",
        "toggleHighQuality ( );",
        "Legacy anti-aliasing quality toggle",
    ),
    ("tr", "trace ( message );", "Log value to Output panel (F2)"),
    (
        "um",
        "unloadMovie ( target );",
        "Unload external clip content",
    ),
    ("un", "unloadMovieNum ( level );", "Unload _levelN content"),
    ("vr", "var ;", "Declare function-local or timeline variable"),
    ("wh", "while ( condition ) { }", "Pre-condition while loop"),
    (
        "wt",
        "with ( object ) { }",
        "Extend scope chain with target object",
    ),
];

pub(crate) const AS2_FN_DECLARATIONS_AND_LITERALS_SAMPLE: &str =
    "// 1. Hoisting: named function declarations can be called before or after their definition
var openingHit:Number = calcDamage(24, true);
trace(\"Opening hit:\", openingHit);

function calcDamage(base:Number, crit:Boolean):Number {
    if (base <= 0) {
        return 0; // Early return exits immediately
    }
    var multiplier:Number = crit ? 2 : 1;
    return base * multiplier;
}

// 2. Function literal / anonymous expression assigned to a Function variable (not hoisted)
var easeOutQuad:Function = function(t:Number):Number {
    return t * (2 - t);
};
trace(\"Ease at 50%:\", easeOutQuad(0.5));

// 3. Passing an anonymous function as a callback comparator to Array.sort()
var highScores:Array = [120, 450, 80, 310];
highScores.sort(function(a:Number, b:Number):Number {
    return b - a;
});
trace(\"Descending scores:\", highScores);";

pub(crate) const AS2_FN_ARGUMENTS_CALL_APPLY_SAMPLE: &str =
    "// 1. Variadic function inspecting arguments.length, arguments[i], and arguments.caller
function sumAll():Number {
    var total:Number = 0;
    for (var i:Number = 0; i < arguments.length; i++) {
        total += Number(arguments[i]);
    }
    if (arguments.caller != null) {
        trace(\"sumAll called from nested helper\");
    }
    return total;
}

function awardWaveBonus():Number {
    return sumAll(10, 25, 50, 15);
}
trace(\"Wave bonus:\", awardWaveBonus());

// 2. Anonymous recursion via arguments.callee
var factorial:Function = function(n:Number):Number {
    if (n <= 1) {
        return 1;
    }
    return n * Number(arguments.callee(n - 1));
};
trace(\"5! =\", factorial(5));

// 3. Function.call(thisObject, arg1, ...) & Function.apply(thisObject, argsArray)
function translateClip(dx:Number, dy:Number):Void {
    this._x += dx;
    this._y += dy;
}
translateClip.call(hero_mc, 16, -8);
var knockback:Array = [-24, 4];
translateClip.apply(hero_mc, knockback);";

pub(crate) const AS2_FN_PROTOTYPE_AND_CLOSURES_SAMPLE: &str =
    "// 1. Constructor function & Function.prototype shared methods (AS1 / AS2)
function Enemy(x:Number, y:Number, speed:Number) {
    this.x = x;
    this.y = y;
    this.speed = speed;
}
Enemy.prototype.step = function():Void {
    this.x -= this.speed;
};
var scout = new Enemy(240, 120, 3);
scout.step();
trace(\"Enemy x after step:\", scout.x);

// 2. Local var activation scope & lexical closures vs timeline properties (this.score)
this.score = 0;
function makeCounter(start:Number):Function {
    var count:Number = start; // Retained in the activation object captured by the inner function
    return function(delta:Number):Number {
        count += delta;
        return count;
    };
}
var nextWave:Function = makeCounter(1);
trace(\"Wave A:\", nextWave(1), \"Wave B:\", nextWave(1));";

pub(crate) const AS2_ONE_FRAME_GAME_LOOP_SAMPLE: &str =
    "// Single-frame AS2 game pattern on Frame 1 ('Actions' layer) — run with Test Movie!
stop();
var score:Number = 0;
var world:MovieClip = this.createEmptyMovieClip(\"world_mc\", this.getNextHighestDepth());

function spawnCoin(xp:Number, yp:Number):MovieClip {
    var d:Number = world.getNextHighestDepth();
    return world.attachMovie(\"CoinSymbol\", \"coin_\" + d, d, {_x: xp, _y: yp});
}

start_btn.onPress = function():Void {
    this._alpha = 75;
};
start_btn.onRelease = function():Void {
    this._alpha = 100;
    var c:MovieClip = spawnCoin(280, 160);
    trace(\"Spawned coin:\", c._name);
};

var waveInterval:Number = setInterval(function():Void {
    trace(\"Wave heartbeat — score:\", score);
    if (score >= 500) {
        clearInterval(waveInterval);
    }
}, 2000);

this.onEnterFrame = function():Void {
    var speed:Number = Key.isDown(Key.SHIFT) ? 7 : 4;
    if (Key.isDown(Key.RIGHT)) { hero_mc._x += speed; }
    if (Key.isDown(Key.LEFT))  { hero_mc._x -= speed; }
    if (Key.isDown(Key.UP))    { hero_mc._y -= speed; }
    if (Key.isDown(Key.DOWN))  { hero_mc._y += speed; }
};";

pub(crate) const AS3_FUNCTIONS_REST_CALL_CLOSURES_SAMPLE: &str = "import flash.display.MovieClip;
import flash.events.Event;

stop();

// 1. Named function with typed default parameter & return value
function calcDamage(base:Number, crit:Boolean = false):Number {
    if (base <= 0) {
        return 0;
    }
    return crit ? base * 2 : base;
}

// 2. Variadic ...rest array parameter (preferred over untyped AS2 arguments)
function sumAll(...values):Number {
    var total:Number = 0;
    for (var i:int = 0; i < values.length; i++) {
        total += Number(values[i]);
    }
    return total;
}

// 3. Lexical closure factory + Function.call(thisArg, ...) & Function.apply(thisArg, array)
function makeCounter(start:int = 0):Function {
    var count:int = start;
    return function(step:int = 1):int {
        count += step;
        return count;
    };
}
var nextWave:Function = makeCounter(1);
var moveClip:Function = function(dx:Number, dy:Number):void {
    this.x += dx;
    this.y += dy;
};
var actor:MovieClip = new MovieClip();
moveClip.call(actor, 12, -4);
moveClip.apply(actor, [8, 16]);
trace(\"AS3 fn results:\", calcDamage(25, true), sumAll(10, 20, 30), nextWave(), actor.x, actor.y);";

pub(crate) const JS_FUNCTIONS_CLOSURES_AND_CALL_SAMPLE: &str = "// ES2023+ Functions: declarations, arrow callbacks, ...rest, closures & call / apply
function calcDamage(base, crit = false) {
    if (base <= 0) return 0;
    return crit ? base * 2 : base;
}

const easeOutQuad = (t) => t * (2 - t);

function sumAll(...values) {
    return values.reduce((acc, n) => acc + n, 0);
}

function makeCounter(start = 0) {
    let count = start;
    return (step = 1) => {
        count += step;
        return count;
    };
}

function nudgeReceiver(dx, dy) {
    this.x = (this.x ?? 0) + dx;
    this.y = (this.y ?? 0) + dy;
}
const spriteState = { x: 40, y: 60 };
nudgeReceiver.call(spriteState, 10, -5);
nudgeReceiver.apply(spriteState, [5, 15]);
const nextId = makeCounter(100);
trace(calcDamage(20, true), easeOutQuad(0.5), sumAll(5, 10, 15), nextId(), spriteState.x, spriteState.y);";

pub(crate) const TS_TYPED_AND_GENERIC_FUNCTIONS_SAMPLE: &str = "type EasingFn = (t: number) => number;

function calcDamage(base: number, crit: boolean = false, bonus?: number): number {
    if (base <= 0) {
        return 0;
    }
    const mult: number = crit ? 2 : 1;
    return base * mult + (bonus ?? 0);
}

const easeOutQuad: EasingFn = (t: number): number => t * (2 - t);

function firstOrFallback(items: Array<string>, fallback: string): string {
    return items.length > 0 ? items[0] : fallback;
}

function makeCounter(start: number): (step: number) => number {
    let count: number = start;
    return (step: number): number => {
        count += step;
        return count;
    };
}

const nextId = makeCounter(0);
trace(calcDamage(30, true, 5), easeOutQuad(0.5), firstOrFallback([\"laser\", \"shield\"], \"none\"), nextId(1));";

/// Structured search index entry covering language statements, core classes, multi-case examples,
/// and authoring chords with explicit dialect badges (`[AS1/AS2]`, `[AS3]`, `[JS]`, `[TS]`).
pub(crate) struct ReferenceTopicEntry {
    pub dialect_badge: &'static str,
    pub chapter_idx: usize,
    pub title: &'static str,
    pub signature: &'static str,
    pub summary: &'static str,
    pub keywords: &'static str,
    pub code_preview: Option<&'static str>,
}

pub(crate) const REFERENCE_SEARCH_ENTRIES: &[ReferenceTopicEntry] = &[
    // ── ActionScript 1.0 / 2.0: Complete function & Function coverage (Cases 1–7) ──
    ReferenceTopicEntry {
        dialect_badge: "[AS1/AS2]",
        chapter_idx: 10,
        title: "Named function Declarations, Parameters, Hoisting & return Values",
        signature: "function calcDamage(base:Number, crit:Boolean):Number { return ...; }",
        summary: "Declares a reusable named function on a timeline frame or inside another function. Named declarations are hoisted so they can be called before or after their definition; `return expr;` exits immediately (omitting `return` or `:Void` returns `undefined`).",
        keywords: "function statement named declaration parameter return void hoisting calcdamage <esc>fn <esc>rt as1 as2",
        code_preview: Some(AS2_FN_DECLARATIONS_AND_LITERALS_SAMPLE),
    },
    ReferenceTopicEntry {
        dialect_badge: "[AS1/AS2]",
        chapter_idx: 10,
        title: "Function Literals / Anonymous Expressions & Higher-Order Callbacks",
        signature: "var easeOutQuad:Function = function(t:Number):Number { return t * (2 - t); };",
        summary: "Creates an unnamed `Function` value at assignment time (not hoisted). Pass function literals directly to `Array.sort(compareFn)`, `setInterval(fn, ms)`, `LoadVars.onLoad`, or dynamic MovieClip / Button callbacks.",
        keywords: "function literal anonymous expression callback easeoutquad array.sort first-class value as1 as2",
        code_preview: Some(AS2_FN_DECLARATIONS_AND_LITERALS_SAMPLE),
    },
    ReferenceTopicEntry {
        dialect_badge: "[AS1/AS2]",
        chapter_idx: 10,
        title: "arguments Object — arguments.length, arguments[i], callee & caller",
        signature: "arguments.length:Number · arguments[i] · arguments.callee:Function · arguments.caller:Function",
        summary: "Every AS1/AS2 function call binds an implicit local `arguments` array-like object: inspect `arguments.length` and `arguments[i]` for variadic functions (`sumAll`), call `arguments.callee` for anonymous recursion, and read `arguments.caller` for the invoking function.",
        keywords: "function arguments length callee caller variadic sumall recursion factorial as1 as2",
        code_preview: Some(AS2_FN_ARGUMENTS_CALL_APPLY_SAMPLE),
    },
    ReferenceTopicEntry {
        dialect_badge: "[AS1/AS2]",
        chapter_idx: 11,
        title: "Function.call(thisObject, ...) & Function.apply(thisObject, argsArray)",
        signature: "fn.call(thisObject:Object, arg1, arg2, ...):Object · fn.apply(thisObject:Object, argsArray:Array):Object",
        summary: "Methods of the built-in `Function` class that invoke any function with `this` explicitly bound to a target `MovieClip` or `Object` (`null` uses the global/timeline scope). Use `call` for comma-separated arguments and `apply` to forward an `Array` or `arguments`.",
        keywords: "function class call apply thisobject explicit receiver movieclip array forwarding as1 as2",
        code_preview: Some(AS2_FN_ARGUMENTS_CALL_APPLY_SAMPLE),
    },
    ReferenceTopicEntry {
        dialect_badge: "[AS1/AS2]",
        chapter_idx: 10,
        title: "Constructor Functions, new & Function.prototype Inheritance",
        signature: "function Enemy(x:Number, y:Number) { ... }; Enemy.prototype.step = function():Void { ... }; new Enemy(100, 80)",
        summary: "Invoking a function with `new` constructs a fresh `Object` whose `__proto__` links to `Constructor.prototype` and runs the function with `this` bound to the new instance. Pair with `Object.registerClass(\"LinkageId\", Constructor)` for Library MovieClips.",
        keywords: "function constructor new prototype __proto__ registerclass inheritance enemy object class as1 as2",
        code_preview: Some(AS2_FN_PROTOTYPE_AND_CLOSURES_SAMPLE),
    },
    ReferenceTopicEntry {
        dialect_badge: "[AS1/AS2]",
        chapter_idx: 10,
        title: "Variable Scope (var), Activation Objects, Nested Functions & Closures",
        signature: "function makeCounter(start:Number):Function { var count:Number = start; return function():Number { ... }; }",
        summary: "Variables declared with `var` inside a function live on that call's activation object rather than polluting the timeline (`this.score`). Nested functions capture the enclosing activation chain, forming stateful lexical closures.",
        keywords: "function scope local var activation object nested closure makecounter timeline this.score <esc>vr as1 as2",
        code_preview: Some(AS2_FN_PROTOTYPE_AND_CLOSURES_SAMPLE),
    },
    ReferenceTopicEntry {
        dialect_badge: "[AS1/AS2]",
        chapter_idx: 10,
        title: "Event Handler Callbacks & 1-Frame Flash Game Loop Pattern",
        signature: "stop(); btn.onPress / onRelease = function() { ... }; this.onEnterFrame = function() { ... }; setInterval(fn, ms);",
        summary: "Author an entire interactive game on Frame 1: halt the playhead with `stop()`, spawn containers via `createEmptyMovieClip` / `attachMovie`, poll `Key.isDown(...)` in `this.onEnterFrame`, schedule waves with `setInterval`/`clearInterval`, and inspect `trace()` live in Output (`F2`) during Test Movie!",
        keywords: "function 1-frame single frame game loop onenterframe onpress onrelease key.isdown setinterval clearinterval attachmovie createemptymovieclip trace test movie stop as1 as2",
        code_preview: Some(AS2_ONE_FRAME_GAME_LOOP_SAMPLE),
    },
    // ── ActionScript 3.0: Functions, ...rest, Function.call/apply, Bound Methods & Game Loop ──
    ReferenceTopicEntry {
        dialect_badge: "[AS3]",
        chapter_idx: 12,
        title: "AS3 Functions — Default Parameters, ...rest, Closures & Function.call / apply",
        signature: "function calcDamage(base:Number, crit:Boolean = false):Number · function sumAll(...values):Number",
        summary: "ActionScript 3.0 adds compile-time parameter/return type checking, default parameter values (`crit:Boolean = false`), typed `...rest` variadic arrays, lexical closures (`makeCounter`), and `Function.call` / `Function.apply`.",
        keywords: "function Function return default parameter rest arguments call apply closure makecounter calcdamage sumall as3",
        code_preview: Some(AS3_FUNCTIONS_REST_CALL_CLOSURES_SAMPLE),
    },
    ReferenceTopicEntry {
        dialect_badge: "[AS3]",
        chapter_idx: 12,
        title: "AS3 Class Methods (Bound `this` Closures) vs AS2 Function.prototype",
        signature: "public class Enemy extends Sprite { public function step():void { this.x -= this.speed; } }",
        summary: "In AS3 sealed classes, instance methods are automatically bound method closures (`var fn:Function = enemy.step` preserves `this == enemy` without manual `Function.call`), replacing AS1/AS2 `Enemy.prototype.step`.",
        keywords: "function class method bound this closure prototype enemy extends sprite movieclip as3",
        code_preview: None,
    },
    ReferenceTopicEntry {
        dialect_badge: "[AS3]",
        chapter_idx: 13,
        title: "AS3 Event Listeners, Timer & 1-Frame Game Loop (`Event.ENTER_FRAME`)",
        signature: "stop(); addEventListener(Event.ENTER_FRAME, onTick); stage.addEventListener(KeyboardEvent.KEY_DOWN, onKey);",
        summary: "Replaces AS2 `onEnterFrame = function()` and `setInterval` with strongly typed `EventDispatcher.addEventListener(Event.ENTER_FRAME, fn)`, `MouseEvent.CLICK`, `KeyboardEvent.KEY_DOWN`, and `flash.utils.Timer`, streaming `trace()` to Output (`F2`).",
        keywords: "function event listener addeventlistener enter_frame mouseevent keyboardevent timer 1-frame game loop trace as3",
        code_preview: None,
    },
    // ── JavaScript & TypeScript: Functions, Arrow Callbacks, Generics & Checker ──
    ReferenceTopicEntry {
        dialect_badge: "[JS]",
        chapter_idx: 16,
        title: "JavaScript Functions, Arrow Expressions (`=>`), `...rest`, Closures & `call`/`apply`/`bind`",
        signature: "function calcDamage(base, crit = false) · const fn = (t) => t * (2 - t) · fn.call(thisArg, ...args)",
        summary: "Supports hoisted `function` declarations, lexical-`this` arrow functions `(event) => ...`, `...rest` parameters, closures (`makeCounter`), `async function` / `await`, and `Function.prototype.call` / `apply` / `bind`.",
        keywords: "function Function arrow return rest closure call apply bind async await javascript js es2023",
        code_preview: Some(JS_FUNCTIONS_CLOSURES_AND_CALL_SAMPLE),
    },
    ReferenceTopicEntry {
        dialect_badge: "[TS]",
        chapter_idx: 17,
        title: "TypeScript Typed Functions, Optional/Default Parameters & Function Type Aliases",
        signature: "function calcDamage(base: number, crit?: boolean): number · type EasingFn = (t: number) => number",
        summary: "NeoFlash's built-in `ts_check` verifies parameter counts, optional (`?`) and default arguments, return types, higher-order closures, and function type aliases `(t: number) => number` before erasing annotations to pure JS.",
        keywords: "function Function return optional default parameter type alias arity closure ts_check typescript ts",
        code_preview: Some(TS_TYPED_AND_GENERIC_FUNCTIONS_SAMPLE),
    },
    // ── Authoring Quick-Typing Chords (<ESC> + 2 keys) ──
    ReferenceTopicEntry {
        dialect_badge: "[Authoring]",
        chapter_idx: 9,
        title: "Classic Flash <ESC> Two-Key Code Chords (<ESC>fn, <ESC>rt, <ESC>tr, <ESC>st — All 50 Chords)",
        signature: "<ESC>fn → function name ( ) { ... } · <ESC>rt → return ( ); · <ESC>tr → trace ( message ); · <ESC>st → stop ( );",
        summary: "Press Escape inside a focused Actions or Project source editor (with no hint popup open) followed by two keys to expand any of the 50 Classic Flash ActionScript quick-typing templates in one Undo step.",
        keywords: "function <esc> esc chord shortcut quick typing <esc>fn fn <esc>rt rt <esc>tr tr <esc>st st <esc>on <esc>oc <esc>if <esc>fr <esc>wh <esc>vr actions",
        code_preview: None,
    },
];

/// Concrete search hit returned by `search_reference_hits`, carrying an explicit dialect badge
/// (`[AS1/AS2]`, `[AS3]`, `[JS]`, `[TS]`, `[Authoring]`) and target chapter index.
pub(crate) struct ComputedSearchHit {
    pub dialect_badge: &'static str,
    pub chapter_idx: usize,
    pub title: String,
    pub signature: String,
    pub summary: String,
    pub code_preview: Option<&'static str>,
}

/// Checks whether `chapter_idx` matches `query_lower` across chapter metadata, browser summary
/// bullets, curated `REFERENCE_SEARCH_ENTRIES`, `JS_TS_API_ENTRIES`, and `<ESC>` chords.
pub(crate) fn chapter_matches_search_deep(chapter_idx: usize, query_lower: &str) -> bool {
    let q = query_lower.trim();
    if q.is_empty() {
        return true;
    }
    let Some(ch) = HELP_CHAPTERS.get(chapter_idx) else {
        return false;
    };
    if ch.title.to_ascii_lowercase().contains(q)
        || ch.sidebar_label.to_ascii_lowercase().contains(q)
        || ch.subtitle.to_ascii_lowercase().contains(q)
        || ch.keywords.to_ascii_lowercase().contains(q)
        || ch.tags.iter().any(|t| t.to_ascii_lowercase().contains(q))
    {
        return true;
    }
    if let Some(topics) = HELP_BROWSER_TOPICS.get(chapter_idx)
        && topics.iter().any(|t| t.to_ascii_lowercase().contains(q))
    {
        return true;
    }
    if REFERENCE_SEARCH_ENTRIES.iter().any(|entry| {
        entry.chapter_idx == chapter_idx
            && (entry.title.to_ascii_lowercase().contains(q)
                || entry.signature.to_ascii_lowercase().contains(q)
                || entry.summary.to_ascii_lowercase().contains(q)
                || entry.keywords.to_ascii_lowercase().contains(q))
    }) {
        return true;
    }
    if chapter_idx == 9 && !matching_esc_chords(q).is_empty() {
        return true;
    }
    false
}

fn matching_esc_chords(query_lower: &str) -> Vec<(&'static str, &'static str, &'static str)> {
    let q = query_lower.trim();
    if q.is_empty() {
        return Vec::new();
    }
    let stripped = q
        .strip_prefix("<esc>")
        .or_else(|| q.strip_prefix("esc "))
        .or_else(|| q.strip_prefix("esc+"))
        .map(str::trim);
    CLASSIC_ESC_CHORDS
        .iter()
        .copied()
        .filter(|(chord, expansion, desc)| {
            if let Some(code) = stripped {
                return code.is_empty()
                    || chord.eq_ignore_ascii_case(code)
                    || expansion.to_ascii_lowercase().contains(code)
                    || desc.to_ascii_lowercase().contains(code);
            }
            let full_chord = format!("<esc>{chord}");
            full_chord.contains(q)
                || (q.len() == 2 && chord.eq_ignore_ascii_case(q))
                || expansion.to_ascii_lowercase().contains(q)
                || (q == "esc" || q == "<esc>" || q.contains("chord"))
        })
        .collect()
}

/// Searches the curated guide topics and `<ESC>` chords, returning
/// `(in_scope_hits, hidden_other_dialect_count)`. Declarations are searched by [`api_search`].
pub(crate) fn search_reference_hits(
    query: &str,
    scope: LanguageScope,
    project_lang: neoflash_core::ScriptLanguage,
) -> (Vec<ComputedSearchHit>, usize) {
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        return (Vec::new(), 0);
    }

    let mut all_hits: Vec<ComputedSearchHit> = Vec::new();

    // 1. Curated multi-case topics (function / Function / arguments / call / apply / prototype / closures / game loops)
    for entry in REFERENCE_SEARCH_ENTRIES {
        if entry.title.to_ascii_lowercase().contains(&q)
            || entry.signature.to_ascii_lowercase().contains(&q)
            || entry.summary.to_ascii_lowercase().contains(&q)
            || entry.keywords.to_ascii_lowercase().contains(&q)
        {
            all_hits.push(ComputedSearchHit {
                dialect_badge: entry.dialect_badge,
                chapter_idx: entry.chapter_idx,
                title: entry.title.to_owned(),
                signature: entry.signature.to_owned(),
                summary: entry.summary.to_owned(),
                code_preview: entry.code_preview,
            });
        }
    }

    // 2. Matching Classic Flash <ESC> 2-key chords (up to 6 concise hits so `<ESC>fn` or `tr` is instant)
    for (chord, expansion, desc) in matching_esc_chords(&q).into_iter().take(6) {
        all_hits.push(ComputedSearchHit {
            dialect_badge: "[Authoring]",
            chapter_idx: 9,
            title: format!("Classic <ESC>{chord} Quick-Typing Chord"),
            signature: format!("<ESC>{chord}  →  {expansion}"),
            summary: format!(
                "{desc} (Actions & Project source editors; see full 50-chord table in Ch. 10)."
            ),
            code_preview: None,
        });
    }

    // Prioritize hits matching the current project's active language when viewing `All languages`!
    let preferred_scope = LanguageScope::from_script_language(project_lang);
    all_hits.sort_by_key(|hit| {
        let matches_proj =
            preferred_scope.matches_badge(hit.dialect_badge) && hit.dialect_badge != "[Authoring]";
        let has_code = hit.code_preview.is_some();
        (!matches_proj, !has_code)
    });

    let mut visible = Vec::new();
    let mut other_count = 0usize;
    for hit in all_hits {
        if scope.matches_badge(hit.dialect_badge) {
            visible.push(hit);
        } else {
            other_count += 1;
        }
    }
    (visible, other_count)
}

/// Renders the complete 50-chord Classic Flash `<ESC>` two-key code-insertion reference table
/// inside Chapter 10 (`10. Scripting, Import & Publish`).
pub(crate) fn render_esc_chords_reference(ui: &mut Ui) {
    section_overline(
        ui,
        "Classic Flash <ESC> Two-Key Code Chords in Actions (All 50 Chords)",
    );
    paragraph(
        ui,
        "Inside a focused Actions (F9) or Project source editor with no autocomplete popup open, press Escape followed sequentially by two characters (for example Escape, f, n) to insert the template with automatic indentation, first-placeholder selection, and single-step Undo.",
    );
    let rows: Vec<(String, &str, &str)> = CLASSIC_ESC_CHORDS
        .iter()
        .map(|&(chord, expansion, desc)| (format!("<ESC>{chord}"), expansion, desc))
        .collect();
    let borrowed: Vec<(&str, &str, &str)> = rows
        .iter()
        .map(|(c_str, exp, desc)| (c_str.as_str(), *exp, *desc))
        .collect();
    api_signature_table(
        ui,
        "All 50 Classic Flash <ESC> + 2-Key ActionScript Chords (<ESC>br .. <ESC>wt)",
        ["Chord", "Inserted Template", "Purpose"],
        &borrowed,
    );
}

/// A copyable, syntax-tinted code block for ActionScript 1.0 / 2.0 / 3.0 and JS/TS examples.
pub(crate) struct CodeSample {
    pub title: &'static str,
    pub lang_badge: &'static str,
    pub code: &'static str,
}

pub(crate) fn code_block(ui: &mut Ui, sample: &CodeSample) {
    let p = neoflash_ui::palette(ui.ctx());
    ui.add_space(space::s3());
    outline_heading(ui, 1, sample.title);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(space::s2(), space::s1());
        ui.add(
            egui::Label::new(rich(sample.title, TextStyle::SmallStrong).color(p.text_secondary))
                .wrap(),
        );
        let language = sample.lang_badge.replace(['[', ']'], "");
        if !language.is_empty() {
            ui.label(rich(language, TextStyle::Small).color(p.text_tertiary));
        }
    });
    ui.add_space(space::s1());
    code_box(ui, sample.code);
    ui.add_space(space::s1());
    code_sample_actions(ui, sample.code);
    ui.add_space(space::s2());
}

/// A three-column table: caption, header row, one wrapped row per entry. The first two columns
/// are code (`Mono Small`), the third is prose.
pub(crate) fn api_signature_table(
    ui: &mut Ui,
    title: &str,
    headers: [&str; 3],
    rows: &[(&str, &str, &str)],
) {
    let p = neoflash_ui::palette(ui.ctx());
    ui.add_space(space::s3());
    outline_heading(ui, 1, title);
    ui.add(egui::Label::new(rich(title, TextStyle::SmallStrong).color(p.text_secondary)).wrap());
    ui.add_space(space::s1());

    let width = ui.available_width().max(120.0);
    let pad = vec2(space::s3(), space::s1());
    let columns = [width * 0.27, width * 0.30, width * 0.43];
    let cell = |ui: &Ui, text: &str, column: usize, header: bool| {
        let (style, color) = match (header, column) {
            (true, _) => (TextStyle::SmallStrong, p.text_secondary),
            (false, 2) => (TextStyle::Small, p.text_secondary),
            (false, _) => (TextStyle::MonoSmall, p.text_primary),
        };
        ui.fonts_mut(|f| {
            f.layout(
                text.to_owned(),
                font(style),
                color,
                (columns[column] - 2.0 * pad.x).max(16.0),
            )
        })
    };
    let lay_row = |ui: &Ui, texts: [&str; 3], header: bool| {
        let galleys = [
            cell(ui, texts[0], 0, header),
            cell(ui, texts[1], 1, header),
            cell(ui, texts[2], 2, header),
        ];
        let height = galleys.iter().map(|g| g.size().y).fold(0.0, f32::max) + 2.0 * pad.y;
        (galleys, height)
    };
    let mut laid = vec![lay_row(ui, headers, true)];
    laid.extend(rows.iter().map(|&(a, b, d)| lay_row(ui, [a, b, d], false)));
    let total: f32 = laid.iter().map(|(_, height)| height).sum();
    let (rect, _) = ui.allocate_exact_size(vec2(width, total), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        let corner = radius::md() as u8;
        let line = Stroke::new(1.0_f32, p.border_subtle);
        let mut y = rect.top();
        for (i, (galleys, height)) in laid.into_iter().enumerate() {
            let row = Rect::from_min_size(egui::pos2(rect.left(), y), vec2(width, height));
            if i == 0 {
                painter.rect_filled(
                    row,
                    egui::CornerRadius {
                        nw: corner,
                        ne: corner,
                        sw: 0,
                        se: 0,
                    },
                    p.bg_app,
                );
            } else {
                painter.hline(rect.x_range(), y, line);
            }
            let mut x = rect.left();
            for (galley, column) in galleys.into_iter().zip(columns) {
                let color = galley
                    .job
                    .sections
                    .first()
                    .map_or(p.text_primary, |s| s.format.color);
                painter.galley(egui::pos2(x + pad.x, y + pad.y), galley, color);
                x += column;
            }
            y += height;
        }
        painter.rect_stroke(
            rect,
            egui::CornerRadius::same(corner),
            line,
            StrokeKind::Inside,
        );
    }
    ui.add_space(space::s2());
}
