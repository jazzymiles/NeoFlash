use super::*;
use crate::panels::help_manual::{
    illustration_canvas, mode_card, paint_clipped_text, paragraph, section_overline,
};
use eframe::egui::{self, Rect, Stroke, StrokeKind, Ui, vec2};
use neoflash_ui::{
    TextStyle,
    components::{self as c, BadgeType},
    icons::i16,
    tokens::space,
};

// ── Chapter 11: AS1 & AS2 Fundamentals & Timeline ──

pub(crate) fn chapter_as1_as2_fundamentals(
    ui: &mut Ui,
    diagram_tab: &mut usize,
    nav_target: &mut Option<usize>,
) {
    paragraph(
        ui,
        "ActionScript 1.0 (Flash 5–MX) and ActionScript 2.0 (Flash MX 2004–CS6) run on the AVM1 execution model. Scripts can be placed directly on Timeline keyframes, on Button instances via on(...), on MovieClip instances via onClipEvent(...), or organized into external .as classes linked to Library symbols.",
    );

    let tabs = [
        (0, "Timeline Scope (_root / _parent / this)"),
        (1, "AVM1 Depth Zones & Prototype Chain"),
    ];
    c::Segmented::show(ui, diagram_tab, &tabs, false);
    ui.add_space(space::s2());

    if *diagram_tab == 0 {
        illustration_canvas(
            ui,
            162.0,
            "Illustration 11.1: AVM1 Target Path Hierarchy (_level0 / _root → parent MovieClip → nested child MovieClip) and Timeline vs Instance script attachment points.",
            |painter, ctx, rect| {
                let p = neoflash_ui::palette(ctx);
                let y_mid = rect.center().y + 6.0;
                let pad = 12.0;
                let gap = 22.0;
                let bw = ((rect.width() - 2.0 * pad - 2.0 * gap) / 3.0).max(90.0);
                let bh = 86.0;
                let specs = [
                    (
                        "_level0 / _root",
                        "Main Timeline",
                        "stop(); gotoAndPlay(\"intro\");",
                    ),
                    (
                        "_root.hero_mc",
                        "Parent MovieClip",
                        "this.onEnterFrame = step;",
                    ),
                    ("hero_mc.blade_mc", "Nested Child Clip", "_parent._x += 4;"),
                ];
                let mut boxes = [Rect::NOTHING; 3];
                for (i, (title, sub, code_line)) in specs.iter().enumerate() {
                    let x = rect.left() + pad + i as f32 * (bw + gap);
                    let b = Rect::from_min_size(egui::pos2(x, y_mid - bh * 0.5), vec2(bw, bh));
                    boxes[i] = b;
                    painter.rect_filled(b, 6.0, p.bg_panel);
                    painter.rect_stroke(
                        b,
                        6.0,
                        Stroke::new(
                            1.5_f32,
                            if i == 1 {
                                p.accent_default
                            } else {
                                p.border_default
                            },
                        ),
                        StrokeKind::Inside,
                    );
                    let inner = b.shrink(6.0);
                    paint_clipped_text(
                        painter,
                        ctx,
                        inner,
                        egui::pos2(inner.left(), b.top() + 18.0),
                        egui::Align2::LEFT_CENTER,
                        title,
                        TextStyle::MonoSmall,
                        p.accent_default,
                    );
                    paint_clipped_text(
                        painter,
                        ctx,
                        inner,
                        egui::pos2(inner.left(), b.top() + 38.0),
                        egui::Align2::LEFT_CENTER,
                        sub,
                        TextStyle::BodyStrong,
                        p.text_primary,
                    );
                    paint_clipped_text(
                        painter,
                        ctx,
                        inner,
                        egui::pos2(inner.left(), b.top() + 62.0),
                        egui::Align2::LEFT_CENTER,
                        code_line,
                        TextStyle::MonoSmall,
                        p.syntax_function,
                    );
                }
                for i in 0..2 {
                    let start = egui::pos2(boxes[i].right() + 2.0, y_mid - 8.0);
                    let end = egui::pos2(boxes[i + 1].left() - 2.0, y_mid - 8.0);
                    painter.line_segment([start, end], Stroke::new(1.5_f32, p.accent_default));
                    let rev_s = egui::pos2(boxes[i + 1].left() - 2.0, y_mid + 12.0);
                    let rev_e = egui::pos2(boxes[i].right() + 2.0, y_mid + 12.0);
                    painter.line_segment([rev_s, rev_e], Stroke::new(1.2_f32, p.warning_default));
                }
                paint_clipped_text(
                    painter,
                    ctx,
                    rect.shrink(6.0),
                    egui::pos2(rect.center().x, rect.top() + 14.0),
                    egui::Align2::CENTER_CENTER,
                    "AVM1 Target Paths: Dot Syntax (_root.hero_mc.blade_mc) & Relative Scope (this / _parent)",
                    TextStyle::Small,
                    p.text_primary,
                );
            },
        );
    } else {
        illustration_canvas(
            ui,
            162.0,
            "Illustration 11.2: Flash Player AVM1 Depth Zones: Timeline Author-Time Zone (-16383..-1), Dynamic Script Zone (0..1048575), and Reserved Zone (1048576..2130690045).",
            |painter, ctx, rect| {
                let p = neoflash_ui::palette(ctx);
                let zones = [
                    (
                        "1. Timeline Zone (-16383..-1)",
                        "Author-time layer instances",
                        "swapDepths() before removeMovieClip()",
                        p.timeline_classic_tween,
                    ),
                    (
                        "2. Dynamic Zone (0..1,048,575)",
                        "attachMovie / duplicateMovieClip",
                        "getNextHighestDepth() >= 0",
                        p.timeline_motion_tween,
                    ),
                    (
                        "3. Reserved Zone (1,048,576+)",
                        "V2 Components & PopUpManager",
                        "removeMovieClip() ignores >= 1,048,576",
                        p.bg_panel,
                    ),
                ];
                let pad = 12.0;
                let h = 38.0;
                for (i, (title, sub, note, fill_c)) in zones.iter().enumerate() {
                    let y = rect.top() + 22.0 + i as f32 * (h + 8.0);
                    let row = Rect::from_min_max(
                        egui::pos2(rect.left() + pad, y),
                        egui::pos2(rect.right() - pad, y + h),
                    );
                    painter.rect_filled(row, 4.0, *fill_c);
                    painter.rect_stroke(
                        row,
                        4.0,
                        Stroke::new(1.0_f32, p.border_default),
                        StrokeKind::Inside,
                    );
                    let split_x = row.left() + row.width() * 0.54;
                    let left_clip = Rect::from_min_max(
                        row.min + vec2(space::s2(), 2.0),
                        egui::pos2(split_x, row.bottom() - 2.0),
                    );
                    let right_clip = Rect::from_min_max(
                        egui::pos2(split_x + 6.0, row.top() + 2.0),
                        row.max - vec2(space::s2(), 2.0),
                    );
                    paint_clipped_text(
                        painter,
                        ctx,
                        left_clip,
                        egui::pos2(left_clip.left(), row.center().y - 7.0),
                        egui::Align2::LEFT_CENTER,
                        title,
                        TextStyle::BodyStrong,
                        p.text_primary,
                    );
                    paint_clipped_text(
                        painter,
                        ctx,
                        left_clip,
                        egui::pos2(left_clip.left(), row.center().y + 8.0),
                        egui::Align2::LEFT_CENTER,
                        sub,
                        TextStyle::Small,
                        p.text_secondary,
                    );
                    paint_clipped_text(
                        painter,
                        ctx,
                        right_clip,
                        egui::pos2(right_clip.right(), row.center().y),
                        egui::Align2::RIGHT_CENTER,
                        note,
                        TextStyle::MonoSmall,
                        p.text_primary,
                    );
                }
            },
        );
    }

    section_overline(ui, "Global Timeline Navigation & Frame Control");
    api_signature_table(
        ui,
        "Core AS1 / AS2 Timeline Functions & MovieClip Methods",
        [
            "Function / Method",
            "Target / Parameters",
            "Runtime Behavior",
        ],
        &[
            (
                "stop()",
                "Current Timeline / mc.stop()",
                "Halts playhead advancement on the active timeline while nested child MovieClips continue running.",
            ),
            (
                "play()",
                "Current Timeline / mc.play()",
                "Resumes sequential frame playback at the document frame rate (fps).",
            ),
            (
                "gotoAndPlay(frame)",
                "1-based frame index or \"label\"",
                "Jumps playhead to frame number, frame label, or (\"Scene\", frame) and continues playing.",
            ),
            (
                "gotoAndStop(frame)",
                "1-based frame index or \"label\"",
                "Moves playhead to the target frame or label and stops immediately.",
            ),
            (
                "nextFrame() / prevFrame()",
                "Current Timeline / mc",
                "Steps 1 frame forward or backward and implicitly stops the playhead.",
            ),
            (
                "stopAllSounds()",
                "Global Audio Mixer",
                "Silences all currently playing Event and Stream sounds across the player.",
            ),
        ],
    );

    mode_card(
        ui,
        Some(i16::CODE),
        "Button on(...) Handlers vs MovieClip onClipEvent(...) vs Dynamic Callbacks",
        "3 Event Styles",
        BadgeType::Accent,
        "F9 Actions",
        &[
            "1) Button Instance Scripts — on(press, release, releaseOutside, rollOver, rollOut, dragOver, dragOut, keyPress \"<Space>\"): Attached directly to a Button symbol instance on Stage; `this` inside a Button `on(release)` block resolves to the parent Timeline containing the button!",
            "2) MovieClip Instance Scripts — onClipEvent(load, unload, enterFrame, mouseDown, mouseUp, mouseMove, keyDown, keyUp, data): Attached to a MovieClip instance on Stage; `this` resolves to that MovieClip instance itself.",
            "3) Frame Callback Properties (Preferred AS2 Pattern): Assign `mc.onRelease = function() { ... };` or `mc.onEnterFrame = function() { ... };` inside a timeline keyframe on an `Actions` layer so all code stays centralized and searchable.",
        ],
    );

    mode_card(
        ui,
        Some(i16::LAYERS),
        "Dynamic MovieClip Instantiation, Depths & Prototype Inheritance",
        "AVM1 Object Model",
        BadgeType::Accent,
        "Linkage ID",
        &[
            "attachMovie(idName, newName, depth, initObject): Instantiates a Library symbol exported for ActionScript (`Linkage Identifier`) as a child of the target MovieClip at `depth` (commonly `this.getNextHighestDepth()`).",
            "createEmptyMovieClip(name, depth) & duplicateMovieClip(newName, depth, initObj): Creates an empty container clip for runtime vector drawing (`lineStyle`/`beginFill`) or clones an existing stage clip.",
            "removeMovieClip(): Removes a clip at positive depth `0..1048575`. To remove an author-time timeline clip (negative depth), first call `mc.swapDepths(this.getNextHighestDepth())`.",
            "AS1 Prototype vs AS2 Class Linkage: In AS1, register a constructor with `Object.registerClass(\"EnemySymbol\", Enemy)` and inherit via `Enemy.prototype = new MovieClip()`. In AS2, declare `class com.game.Enemy extends MovieClip` and set the symbol's AS2 Class in Library Properties.",
        ],
    );

    mode_card(
        ui,
        Some(i16::BOOKMARK),
        "ActionScript 1.0 (Flash 4 / Flash 5) Slash Paths, Legacy Operators & Directives",
        "AS1 Legacy",
        BadgeType::Warning,
        "Flash 4–6",
        &[
            "Slash & Colon Target Paths: `/hero/arm:angle` addresses variable `angle` inside `_root.hero.arm`; `../:score` addresses `score` on `_parent`; `tellTarget(\"/hero\") { gotoAndPlay(2); }` switches the implicit timeline target.",
            "getProperty & setProperty Constants: `getProperty(target, prop)` and `setProperty(target, prop, value)` accept all 19 classic property tokens (`_x`, `_y`, `_xscale`, `_yscale`, `_currentframe`, `_totalframes`, `_alpha`, `_visible`, `_width`, `_height`, `_rotation`, `_target`, `_framesloaded`, `_name`, `_droptarget`, `_url`, `_highquality`, `_focusrect`, `_soundbuftime`).",
            "Flash 4 String & Logical Operators: String comparison (`eq`, `ne`, `lt`, `gt`, `le`, `ge`), string concatenation (`add`), logical operators (`and`, `or`, `not`, `<>`), and byte/multibyte string functions (`length()`, `substring()`, `chr()`, `ord()`, `mbchr()`, `mbord()`, `mblength()`, `mbsubstring()`).",
            "Pre-Class Component Directives: `#include \"path/file.as\"` splices external script text at compile time; `#initclip [order]` ... `#endinitclip` executes component registration once on the frame before symbol construction.",
        ],
    );

    section_overline(
        ui,
        "Complete AS1 / AS2 function Statement, return, arguments & Function Class Reference",
    );
    api_signature_table(
        ui,
        "AS1 / AS2 function Statement, arguments Object & Function Class (Full Surface)",
        [
            "Construct / Member",
            "Syntax & Signature",
            "Runtime Semantics",
        ],
        &[
            (
                "function (Named)",
                "function name(p1:Type, ...):ReturnType { ... }",
                "Declares a reusable named function; hoisted within its frame script or outer function so callers above or below can invoke it.",
            ),
            (
                "function (Literal)",
                "var fn:Function = function(p1:Type):ReturnType { ... };",
                "Evaluates an anonymous function expression at assignment time (not hoisted) for callbacks, `Array.sort`, timers, and handlers.",
            ),
            (
                "return",
                "return expression;  |  return;",
                "Halts the current function call immediately and yields `expression` to the caller (or `undefined` when omitted / `:Void`).",
            ),
            (
                "arguments.length / [i]",
                "arguments.length:Number · arguments[0..n-1]",
                "Implicit local array-like call object exposing all runtime arguments passed to any function, enabling variadic helpers (`sumAll`).",
            ),
            (
                "arguments.callee / caller",
                "arguments.callee:Function · arguments.caller:Function",
                "`callee` references the currently executing function (for anonymous recursion); `caller` references the invoking function frame (or `null` at top level).",
            ),
            (
                "Function.call()",
                "fn.call(thisObject:Object, arg1, arg2, ...):Object",
                "Invokes `fn` with `this` explicitly bound to `thisObject` (`MovieClip` or `Object`), passing individual arguments (`null` uses global/timeline scope).",
            ),
            (
                "Function.apply()",
                "fn.apply(thisObject:Object, argsArray:Array):Object",
                "Invokes `fn` with `this` bound to `thisObject`, unpacking `argsArray` (or `arguments`) as positional parameters.",
            ),
            (
                "Function.prototype",
                "Ctor.prototype.method = fn · new Ctor(...) · __proto__",
                "Attaches shared methods inherited by every `new Ctor(...)` instance and symbols linked via `Object.registerClass(id, Ctor)`.",
            ),
            (
                "var Scope & Closures",
                "var local:Type = val  vs  this.prop = val",
                "`var` inside a function binds to its activation object and survives inside returned nested closures; `this.prop` persists on the timeline or MovieClip.",
            ),
        ],
    );

    mode_card(
        ui,
        Some(i16::CODE),
        "Quick-Typing Chords (<ESC>fn, <ESC>rt, <ESC>vr, <ESC>tr, <ESC>st) & Test Movie Output",
        "[AS1/AS2]",
        BadgeType::Accent,
        "F9 / ⌘↵ / F2",
        &[
            "Classic <ESC> + 2-Key Chords in Actions: Press Escape followed by `fn` (`function name ( ) { ... }`), `rt` (`return ( );`), `vr` (`var ;`), `tr` (`trace ( message );`), `st` (`stop ( );`), `on` (`on ( event )`), or `oc` (`onClipEvent ( event )`) — see Ch. 10 for all 50 chords!",
            "Live trace() in Test Movie & In-Editor Play: Every `trace(...)` call on Frame 1, inside `onEnterFrame`, or inside button/timer callbacks streams directly to NeoFlash's Output panel (F2).",
        ],
    );

    code_block(
        ui,
        &CodeSample {
            title: "AS2 Case 1 & 2 — Named function Declarations (Hoisting & return) + Anonymous Function Literals",
            lang_badge: "[AS1/AS2]",
            code: AS2_FN_DECLARATIONS_AND_LITERALS_SAMPLE,
        },
    );

    code_block(
        ui,
        &CodeSample {
            title: "AS2 Case 3 & 4 — Variadic arguments (length, callee, caller) + Function.call() & Function.apply()",
            lang_badge: "[AS1/AS2]",
            code: AS2_FN_ARGUMENTS_CALL_APPLY_SAMPLE,
        },
    );

    code_block(
        ui,
        &CodeSample {
            title: "AS2 Case 5 & 6 — Constructor Functions, Function.prototype Inheritance & Lexical Closures",
            lang_badge: "[AS1/AS2]",
            code: AS2_FN_PROTOTYPE_AND_CLOSURES_SAMPLE,
        },
    );

    code_block(
        ui,
        &CodeSample {
            title: "AS2 Case 7 — Complete 1-Frame Game Pattern (onPress/onRelease, onEnterFrame, Key, setInterval & trace)",
            lang_badge: "[AS1/AS2]",
            code: AS2_ONE_FRAME_GAME_LOOP_SAMPLE,
        },
    );

    code_block(
        ui,
        &CodeSample {
            title: "AS2 Frame Script — Spawning Library Symbols, EnterFrame Physics & Hit Testing",
            lang_badge: "[AS1/AS2]",
            code: "// Frame 1 on 'Actions' layer (1-based timeline indexing)
stop();
var score:Number = 0;
var gravity:Number = 0.65;

// Spawn a linked Library symbol ('CoinSymbol') at the next available positive depth
var depth:Number = this.getNextHighestDepth();
var coin:MovieClip = this.attachMovie(\"CoinSymbol\", \"coin_\" + depth, depth, {_x: 320, _y: 180});

hero_mc.vy = 0;
hero_mc.onEnterFrame = function():Void {
    this.vy += _root.gravity;
    this._y += this.vy;
    if (Key.isDown(Key.RIGHT)) { this._x += 5; this._xscale = 100; }
    if (Key.isDown(Key.LEFT))  { this._x -= 5; this._xscale = -100; }
    // Bounding-box or shape-flag (true) collision test
    if (this.hitTest(_root.coin._x, _root.coin._y, true)) {
        _root.score += 100;
        _root.score_txt.text = \"Score: \" + _root.score;
        _root.coin.removeMovieClip();
    }
};",
        },
    );

    crate::panels::help_manual::inline_cross_links(
        ui,
        nav_target,
        &[
            (6, "7. Timeline, Layers & Masks"),
            (11, "12. AS2 Core Classes & Built-Ins"),
            (15, "16. AS1/AS2 ↔ AS3 ↔ JS/TS Migration Guide"),
        ],
    );
}

// ── Chapter 12: AS2 Core Classes & Built-Ins ──

pub(crate) fn chapter_as2_core_classes(
    ui: &mut Ui,
    _diagram_tab: &mut usize,
    nav_target: &mut Option<usize>,
) {
    paragraph(
        ui,
        "Flash MX 2004 and Flash 8 expanded ActionScript 2.0 with a rich standard library spanning display control, dynamic typography, procedural vector drawing, pixel-level BitmapData manipulation, 2D affine matrices, audio mixing, and XML/LoadVars networking.",
    );

    section_overline(
        ui,
        "Interactive Diagram — ActionScript 2.0 Built-In Class & Package Architecture",
    );
    illustration_canvas(
        ui,
        156.0,
        "Illustration 12.1: The 4 pillars of the ActionScript 2.0 runtime library in NeoFlash: Display & Stage, flash.geom / BitmapData (Flash 8), Input & Sound, and Data / Persistence.",
        |painter, ctx, rect| {
            let p = neoflash_ui::palette(ctx);
            let cols = [
                (
                    "Display & Text",
                    &[
                        "MovieClip (Drawing API)",
                        "Button (onRelease/enabled)",
                        "TextField & TextFormat",
                        "Stage (scaleMode / align)",
                        "Color (setRGB / setTransform)",
                    ],
                ),
                (
                    "flash.geom & BitmapData",
                    &[
                        "BitmapData (draw / perlin)",
                        "Matrix (a,b,c,d,tx,ty)",
                        "Point (length / interpolate)",
                        "Rectangle (union / intersects)",
                        "ColorTransform & Filters",
                    ],
                ),
                (
                    "Input & Audio",
                    &[
                        "Key (isDown / addListener)",
                        "Mouse (hide / onMouseWheel)",
                        "Sound (attach / loadSound)",
                        "Selection (setFocus / Caret)",
                        "System.capabilities",
                    ],
                ),
                (
                    "Data, Net & Timers",
                    &[
                        "LoadVars (sendAndLoad)",
                        "XML & XMLNode",
                        "MovieClipLoader (listeners)",
                        "SharedObject.getLocal()",
                        "setInterval / clearInterval",
                    ],
                ),
            ];
            let pad = 10.0;
            let cw = (rect.width() - pad * 5.0) / 4.0;
            let ch = rect.height() - 20.0;
            for (i, (heading, items)) in cols.iter().enumerate() {
                let x = rect.left() + pad + i as f32 * (cw + pad);
                let card = Rect::from_min_size(egui::pos2(x, rect.top() + 10.0), vec2(cw, ch));
                painter.rect_filled(card, 5.0, p.bg_panel);
                painter.rect_stroke(
                    card,
                    5.0,
                    Stroke::new(1.0_f32, p.border_subtle),
                    StrokeKind::Inside,
                );
                let inner = card.shrink(6.0);
                paint_clipped_text(
                    painter,
                    ctx,
                    inner,
                    egui::pos2(inner.left(), card.top() + 14.0),
                    egui::Align2::LEFT_CENTER,
                    heading,
                    TextStyle::BodyStrong,
                    p.accent_default,
                );
                for (j, item) in items.iter().enumerate() {
                    paint_clipped_text(
                        painter,
                        ctx,
                        inner,
                        egui::pos2(inner.left(), card.top() + 36.0 + j as f32 * 20.0),
                        egui::Align2::LEFT_CENTER,
                        item,
                        TextStyle::MonoSmall,
                        p.text_primary,
                    );
                }
            }
        },
    );

    api_signature_table(
        ui,
        "AS2 Core Classes — Essential Properties & Methods",
        ["Class", "Key Members", "Usage & Notes"],
        &[
            (
                "Function & arguments",
                "Function.call(thisObj, ...), Function.apply(thisObj, arr), prototype; arguments.length, arguments.callee, arguments.caller",
                "First-class callable objects, explicit `this` receiver binding, variadic parameter inspection, anonymous recursion, and prototype chains.",
            ),
            (
                "setInterval & clearInterval",
                "setInterval(fn:Function, ms:Number, ...args):Number · setInterval(obj, \"method\", ms) · clearInterval(id)",
                "Schedules repeating function or object-method callbacks at millisecond intervals independent of timeline fps.",
            ),
            (
                "MovieClip",
                "_x, _y, _xscale, _yscale, _rotation, _alpha, _visible, _currentframe, _totalframes",
                "In AS2, _alpha and _xscale/_yscale use 0..100 percentages; _framesloaded tracks streaming progress.",
            ),
            (
                "MovieClip (Drawing)",
                "lineStyle(w, rgb, a), beginFill(rgb, a), beginGradientFill(...), moveTo, lineTo, curveTo, endFill, clear",
                "Procedural vector drawing directly inside any MovieClip; curveTo(cx, cy, ax, ay) draws a quadratic Bézier.",
            ),
            (
                "TextField & TextFormat",
                "createTextField(name, depth, x, y, w, h), text, htmlText, setTextFormat(fmt), setNewTextFormat(fmt)",
                "Supports border, background, wordWrap, multiline, embedFonts, autoSize, and scroll / maxscroll.",
            ),
            (
                "Sound",
                "new Sound(target_mc), attachSound(id), loadSound(url, isStreaming), start(offsetSec, loops), stop(), setVolume(0..100), setPan(-100..100)",
                "Passing a target_mc scopes volume/pan to that timeline; omitting it controls global audio.",
            ),
            (
                "Key & Mouse",
                "Key.isDown(code), Key.getCode(), Key.addListener(obj), Mouse.hide(), Mouse.show(), Mouse.addListener(obj)",
                "Listeners receive onKeyDown / onKeyUp and onMouseDown / onMouseUp / onMouseMove / onMouseWheel.",
            ),
            (
                "flash.display.BitmapData",
                "new BitmapData(w, h, transparent, fillARGB), draw(mc, matrix, colorTransform, blend), getPixel32, setPixel32, perlinNoise, copyPixels",
                "Introduced in Flash 8; attach to a MovieClip via mc.attachBitmap(bmp, depth, \"auto\", smoothing).",
            ),
            (
                "LoadVars & SharedObject",
                "lv.load(url), lv.sendAndLoad(url, target, method), lv.onLoad(ok); SharedObject.getLocal(\"save\")",
                "LoadVars parses URL-encoded key/value pairs; SharedObject.data persists structured local state via flush().",
            ),
            (
                "Stage (Scale, Align & Fullscreen)",
                "Stage.scaleMode (\"showAll\"|\"noScale\"|\"exactFit\"|\"noBorder\"), Stage.align (\"\"|\"T\"|\"B\"|\"L\"|\"R\"|\"TL\"|\"TR\"|\"BL\"|\"BR\"), Stage.displayState, Stage.addListener(obj)",
                "Supports all 9 anchor alignments, High-DPI Canvas scaling with exact inverse pointer hit-testing (`_xmouse`, `_ymouse`), `onResize`, and fullscreen toggle when `Allow fullscreen` is enabled.",
            ),
            (
                "loadMovie & MovieClipLoader",
                "mc.loadMovie(url), mcl.loadClip(url, target), onLoadInit(mc) — SWF, MP4, AVI, MP3, WAV, SVG, WebP, GIF, BMP, TIFF & `loadPublish`",
                "Loads vector/raster/audio/AVI/SWF assets and mounts published NeoFlash HTML5 child bundles (`index.html`, `bundle.js`, or folder URL) directly into `target`.",
            ),
        ],
    );

    code_block(
        ui,
        &CodeSample {
            title: "AS2 Procedural Vector Drawing, Dynamic Text & Flash 8 BitmapData",
            lang_badge: "[AS1/AS2]",
            code: "import flash.display.BitmapData;
import flash.geom.Matrix;

// 1. Create an empty MovieClip and draw a rounded badge with the AS2 Drawing API
var canvas:MovieClip = this.createEmptyMovieClip(\"badge_mc\", this.getNextHighestDepth());
canvas.lineStyle(2, 0x6C93FF, 100);
canvas.beginFill(0x1F2A48, 95);
canvas.moveTo(20, 20);
canvas.lineTo(180, 20);
canvas.lineTo(180, 70);
canvas.lineTo(20, 70);
canvas.lineTo(20, 20);
canvas.endFill();

// 2. Create a Dynamic TextField and style it with TextFormat
this.createTextField(\"label_txt\", this.getNextHighestDepth(), 32, 32, 140, 28);
var fmt:TextFormat = new TextFormat(\"Inter\", 14, 0xFFFFFF, true);
label_txt.text = \"READY PLAYER ONE\";
label_txt.setTextFormat(fmt);

// 3. Rasterize vector art into a Flash 8 BitmapData buffer
var bmp:BitmapData = new BitmapData(200, 90, true, 0x00000000);
bmp.draw(canvas, new Matrix());",
        },
    );

    crate::panels::help_manual::inline_cross_links(
        ui,
        nav_target,
        &[
            (8, "9. Filters, Text & Audio"),
            (10, "11. AS1 & AS2 Fundamentals & Timeline"),
            (14, "15. AS3 Graphics, Filters, Sound & Data"),
        ],
    );
}

// ── Chapter 13: AS3 Architecture & Display List ──

pub(crate) fn chapter_as3_display_list(
    ui: &mut Ui,
    diagram_tab: &mut usize,
    nav_target: &mut Option<usize>,
) {
    paragraph(
        ui,
        "ActionScript 3.0 (introduced with Flash Player 9 / AVM2) replaces AVM1's depth-numbered MovieClip tree and silent failure semantics with a strongly typed, class-based architecture, explicit package namespaces, traits-backed sealed classes, and a unified Display List scene graph.",
    );

    let tabs = [
        (0, "Display List Class Hierarchy"),
        (1, "Local ↔ Global Coordinate Spaces"),
    ];
    c::Segmented::show(ui, diagram_tab, &tabs, false);
    ui.add_space(space::s2());

    if *diagram_tab == 0 {
        illustration_canvas(
            ui,
            188.0,
            "Illustration 13.1: The ActionScript 3.0 flash.display Inheritance Hierarchy: EventDispatcher → DisplayObject → InteractiveObject → DisplayObjectContainer → Sprite → MovieClip, alongside leaf display nodes (Shape, Bitmap, TextField, SimpleButton).",
            |painter, ctx, rect| {
                let p = neoflash_ui::palette(ctx);
                let pad_x = 12.0;
                let top_row = [
                    ("1. EventDispatcher", "addEventListener / dispatchEvent"),
                    ("2. DisplayObject", "x, y, scaleX, rotation, alpha"),
                    (
                        "3. InteractiveObject",
                        "mouseEnabled, tabEnabled, focusRect",
                    ),
                ];
                let w3 = (rect.width() - pad_x * 4.0) / 3.0;
                let row1_y = rect.top() + 10.0;
                let row1_h = 48.0;
                for (i, (cls, members)) in top_row.iter().enumerate() {
                    let x = rect.left() + pad_x + i as f32 * (w3 + pad_x);
                    let node = Rect::from_min_size(egui::pos2(x, row1_y), vec2(w3, row1_h));
                    painter.rect_filled(node, 5.0, p.bg_panel);
                    painter.rect_stroke(
                        node,
                        5.0,
                        Stroke::new(1.2_f32, p.border_default),
                        StrokeKind::Inside,
                    );
                    let inner = node.shrink(6.0);
                    paint_clipped_text(
                        painter,
                        ctx,
                        inner,
                        egui::pos2(node.center().x, node.top() + 15.0),
                        egui::Align2::CENTER_CENTER,
                        cls,
                        TextStyle::BodyStrong,
                        p.accent_default,
                    );
                    paint_clipped_text(
                        painter,
                        ctx,
                        inner,
                        egui::pos2(node.center().x, node.top() + 34.0),
                        egui::Align2::CENTER_CENTER,
                        members,
                        TextStyle::MonoSmall,
                        p.text_secondary,
                    );
                    if i + 1 < top_row.len() {
                        painter.line_segment(
                            [
                                egui::pos2(node.right() + 2.0, node.center().y),
                                egui::pos2(node.right() + pad_x - 2.0, node.center().y),
                            ],
                            Stroke::new(1.5_f32, p.accent_default),
                        );
                    }
                }

                let mid_row = [
                    (
                        "4. DisplayObjectContainer",
                        "addChild, removeChild, numChildren",
                    ),
                    (
                        "5. Sprite → MovieClip",
                        "graphics, buttonMode, currentFrame",
                    ),
                ];
                let w2 = (rect.width() - pad_x * 3.0) / 2.0;
                let row2_y = row1_y + row1_h + 12.0;
                let row2_h = 48.0;
                for (i, (cls, members)) in mid_row.iter().enumerate() {
                    let x = rect.left() + pad_x + i as f32 * (w2 + pad_x);
                    let node = Rect::from_min_size(egui::pos2(x, row2_y), vec2(w2, row2_h));
                    painter.rect_filled(node, 5.0, p.bg_panel);
                    painter.rect_stroke(
                        node,
                        5.0,
                        Stroke::new(1.5_f32, p.accent_default),
                        StrokeKind::Inside,
                    );
                    let inner = node.shrink(6.0);
                    paint_clipped_text(
                        painter,
                        ctx,
                        inner,
                        egui::pos2(node.center().x, node.top() + 15.0),
                        egui::Align2::CENTER_CENTER,
                        cls,
                        TextStyle::BodyStrong,
                        p.accent_default,
                    );
                    paint_clipped_text(
                        painter,
                        ctx,
                        inner,
                        egui::pos2(node.center().x, node.top() + 34.0),
                        egui::Align2::CENTER_CENTER,
                        members,
                        TextStyle::MonoSmall,
                        p.text_secondary,
                    );
                    if i == 0 {
                        painter.line_segment(
                            [
                                egui::pos2(node.right() + 2.0, node.center().y),
                                egui::pos2(node.right() + pad_x - 2.0, node.center().y),
                            ],
                            Stroke::new(1.5_f32, p.accent_default),
                        );
                    }
                }

                let leaves = [
                    "Shape (vector leaf)",
                    "Bitmap (BitmapData)",
                    "TextField (text/input)",
                    "SimpleButton (4-state)",
                ];
                let leaf_y = rect.bottom() - 34.0;
                let lw = (rect.width() - pad_x * 5.0) / 4.0;
                for (i, ltxt) in leaves.iter().enumerate() {
                    let x = rect.left() + pad_x + i as f32 * (lw + pad_x);
                    let lr = Rect::from_min_size(egui::pos2(x, leaf_y), vec2(lw, 24.0));
                    painter.rect_filled(lr, 4.0, p.accent_subtle);
                    paint_clipped_text(
                        painter,
                        ctx,
                        lr.shrink(4.0),
                        lr.center(),
                        egui::Align2::CENTER_CENTER,
                        ltxt,
                        TextStyle::MonoSmall,
                        p.text_primary,
                    );
                }
            },
        );
    } else {
        illustration_canvas(
            ui,
            168.0,
            "Illustration 13.2: Coordinate Space Mapping between Stage Global Coordinates (0,0 top-left) and a transformed child Sprite's Local Space via localToGlobal(pt) and globalToLocal(pt).",
            |painter, ctx, rect| {
                let p = neoflash_ui::palette(ctx);
                let stage_box = Rect::from_min_size(
                    rect.min + vec2(20.0, 18.0),
                    vec2(rect.width() - 40.0, rect.height() - 36.0),
                );
                painter.rect_filled(stage_box, 4.0, p.bg_stage);
                painter.rect_stroke(
                    stage_box,
                    4.0,
                    Stroke::new(1.0_f32, p.border_default),
                    StrokeKind::Inside,
                );
                let stage_inner = stage_box.shrink(6.0);
                let g_origin = stage_box.min + vec2(18.0, 18.0);
                painter.circle_filled(g_origin, 4.0, p.guide);
                paint_clipped_text(
                    painter,
                    ctx,
                    stage_inner,
                    g_origin + vec2(10.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    "Stage Global (0, 0)",
                    TextStyle::MonoSmall,
                    p.guide,
                );

                let local_box =
                    Rect::from_center_size(stage_box.center() + vec2(24.0, 4.0), vec2(210.0, 74.0));
                painter.rect_filled(local_box, 4.0, p.accent_subtle);
                painter.rect_stroke(
                    local_box,
                    4.0,
                    Stroke::new(1.5_f32, p.accent_default),
                    StrokeKind::Inside,
                );
                let l_origin = local_box.min + vec2(16.0, 16.0);
                let pt = local_box.center() + vec2(32.0, 8.0);
                painter.circle_filled(l_origin, 4.0, p.warning_default);
                painter.circle_filled(pt, 4.5, p.success_default);
                painter.line_segment([g_origin, pt], Stroke::new(1.5_f32, p.guide));
                painter.line_segment([l_origin, pt], Stroke::new(1.5_f32, p.warning_default));
                paint_clipped_text(
                    painter,
                    ctx,
                    local_box.shrink(4.0),
                    l_origin + vec2(8.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    "container Local (0, 0)",
                    TextStyle::MonoSmall,
                    p.warning_default,
                );
                paint_clipped_text(
                    painter,
                    ctx,
                    stage_inner,
                    egui::pos2(stage_box.center().x, stage_box.bottom() - 12.0),
                    egui::Align2::CENTER_CENTER,
                    "globalPt = container.localToGlobal(new Point(lx, ly))",
                    TextStyle::MonoSmall,
                    p.text_primary,
                );
            },
        );
    }

    mode_card(
        ui,
        Some(i16::CLASS),
        "AVM2 Language Semantics — Strong Typing, Vector.<T>, Dictionary & Sealed Classes",
        "AVM2 Core",
        BadgeType::Accent,
        "AS3",
        &[
            "Numeric Primitives: `int` (signed 32-bit integer, default `0`), `uint` (unsigned 32-bit integer for ARGB colours/bitmasks, default `0`), and `Number` (IEEE-754 double-precision float, default `NaN`).",
            "Typed Collections: `Vector.<T>` provides contiguous, fixed-type arrays (`new <Sprite>[a, b]`, optional `fixed = true`), while `flash.utils.Dictionary(weakKeys)` maps object references directly to values without stringifying keys.",
            "Sealed vs Dynamic Classes & Namespaces: Classes are sealed by default (accessing an undeclared property throws `ReferenceError 1069` at runtime or compiler error `1119`), except `MovieClip` and classes explicitly marked `dynamic`. Access control uses `public`, `private`, `protected`, and `internal`.",
        ],
    );

    api_signature_table(
        ui,
        "DisplayObjectContainer & DisplayObject — Scene Graph Operations",
        ["Method / Property", "Signature / Type", "Behavior"],
        &[
            (
                "addChild / addChildAt",
                "(child:DisplayObject, index:int):DisplayObject",
                "Instantiates or reparents `child` onto this container's display list (automatically removing it from any previous parent) and dispatches `Event.ADDED` + `Event.ADDED_TO_STAGE`.",
            ),
            (
                "removeChild / removeChildAt",
                "(child:DisplayObject) / (index:int)",
                "Detaches `child` from the display list and shifts higher-index siblings down to keep child indices `0..numChildren - 1` contiguous.",
            ),
            (
                "setChildIndex / swapChildren",
                "(child, index:int) / (child1, child2)",
                "Reorders z-stacking without removing or re-dispatching `ADDED_TO_STAGE`.",
            ),
            (
                "getObjectsUnderPoint",
                "(point:Point):Array",
                "Returns all display objects underneath `point` (expressed in Stage global coordinates).",
            ),
            (
                "getBounds / getRect",
                "(targetCoordinateSpace:DisplayObject):Rectangle",
                "`getBounds` includes stroke widths; `getRect` measures fill bounds only, transformed into `targetCoordinateSpace`.",
            ),
            (
                "hitTestObject / hitTestPoint",
                "(obj:DisplayObject) / (x, y, shapeFlag:Boolean)",
                "Tests bounding-box intersection (`hitTestObject`) or global point vs actual vector/pixel silhouette (`shapeFlag = true`).",
            ),
        ],
    );

    code_block(
        ui,
        &CodeSample {
            title: "AS3 Functions — Default Parameters, ...rest, Closures & Function.call / apply",
            lang_badge: "[AS3]",
            code: AS3_FUNCTIONS_REST_CALL_CLOSURES_SAMPLE,
        },
    );

    code_block(
        ui,
        &CodeSample {
            title: "AS3 Document Class — Building a Display List Hierarchy & Coordinate Transform",
            lang_badge: "[AS3]",
            code: "package {
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

            // Convert muzzle tip from local ship coordinates to Stage global coordinates
            var tipGlobal:Point = ship.localToGlobal(new Point(0, -18));
            trace(\"Ship nose in Stage coordinates:\", tipGlobal.x, tipGlobal.y);
        }
    }
}",
        },
    );

    crate::panels::help_manual::inline_cross_links(
        ui,
        nav_target,
        &[
            (5, "6. Symbols, 9-Slice & Library"),
            (13, "14. AS3 Events, Input & Lifecycle"),
            (14, "15. AS3 Graphics, Filters, Sound & Data"),
        ],
    );
}

// ── Chapter 14: AS3 Events, Input & Lifecycle ──

pub(crate) fn chapter_as3_events_lifecycle(
    ui: &mut Ui,
    diagram_tab: &mut usize,
    nav_target: &mut Option<usize>,
) {
    paragraph(
        ui,
        "Every interactive object and asynchronous subsystem in ActionScript 3.0 inherits from flash.events.EventDispatcher. Display list events travel through a deterministic W3C three-phase propagation path (Capture → Target → Bubbling), and frame updates follow a strict multi-step lifecycle on every tick.",
    );

    let tabs = [
        (0, "3-Phase Event Flow (Capture → Target → Bubble)"),
        (1, "Deterministic Frame Tick Lifecycle"),
    ];
    c::Segmented::show(ui, diagram_tab, &tabs, false);
    ui.add_space(space::s2());

    if *diagram_tab == 0 {
        illustration_canvas(
            ui,
            164.0,
            "Illustration 14.1: W3C 3-Phase Event Propagation across the Display List: Phase 1 Capture (Stage → Panel, useCapture=true), Phase 2 At Target (Button), and Phase 3 Bubbling (Panel → Stage, bubbles=true).",
            |painter, ctx, rect| {
                let p = neoflash_ui::palette(ctx);
                let nodes = [
                    ("1. Stage", "event.currentTarget"),
                    ("2. root / HUD_mc", "DisplayObjectContainer"),
                    ("3. dialog_mc", "Parent Sprite"),
                    ("4. play_btn (Target)", "event.target"),
                ];
                let pad = 12.0;
                let w = (rect.width() - pad * 5.0) / 4.0;
                let mid_y = rect.center().y;
                for (i, (title, sub)) in nodes.iter().enumerate() {
                    let x = rect.left() + pad + i as f32 * (w + pad);
                    let box_r = Rect::from_min_size(egui::pos2(x, mid_y - 26.0), vec2(w, 52.0));
                    let is_target = i == 3;
                    painter.rect_filled(
                        box_r,
                        5.0,
                        if is_target {
                            p.warning_subtle
                        } else {
                            p.bg_panel
                        },
                    );
                    painter.rect_stroke(
                        box_r,
                        5.0,
                        Stroke::new(
                            1.5_f32,
                            if is_target {
                                p.warning_default
                            } else {
                                p.accent_default
                            },
                        ),
                        StrokeKind::Inside,
                    );
                    let inner = box_r.shrink(5.0);
                    paint_clipped_text(
                        painter,
                        ctx,
                        inner,
                        egui::pos2(box_r.center().x, box_r.top() + 18.0),
                        egui::Align2::CENTER_CENTER,
                        title,
                        TextStyle::BodyStrong,
                        if is_target {
                            p.warning_default
                        } else {
                            p.text_primary
                        },
                    );
                    paint_clipped_text(
                        painter,
                        ctx,
                        inner,
                        egui::pos2(box_r.center().x, box_r.top() + 36.0),
                        egui::Align2::CENTER_CENTER,
                        sub,
                        TextStyle::MonoSmall,
                        p.text_secondary,
                    );
                }
                painter.line_segment(
                    [
                        egui::pos2(rect.left() + 40.0, rect.top() + 26.0),
                        egui::pos2(rect.right() - 40.0, rect.top() + 26.0),
                    ],
                    Stroke::new(2.0_f32, p.guide),
                );
                paint_clipped_text(
                    painter,
                    ctx,
                    rect.shrink(8.0),
                    egui::pos2(rect.center().x, rect.top() + 14.0),
                    egui::Align2::CENTER_CENTER,
                    "Phase 1: CAPTURING_PHASE (Stage → Parent, useCapture = true) ──►",
                    TextStyle::MonoSmall,
                    p.guide,
                );
                painter.line_segment(
                    [
                        egui::pos2(rect.right() - 40.0, rect.bottom() - 28.0),
                        egui::pos2(rect.left() + 40.0, rect.bottom() - 28.0),
                    ],
                    Stroke::new(2.0_f32, p.success_default),
                );
                paint_clipped_text(
                    painter,
                    ctx,
                    rect.shrink(8.0),
                    egui::pos2(rect.center().x, rect.bottom() - 14.0),
                    egui::Align2::CENTER_CENTER,
                    "◄── Phase 3: BUBBLING_PHASE (Target → Stage, useCapture = false, bubbles = true)",
                    TextStyle::MonoSmall,
                    p.success_default,
                );
            },
        );
    } else {
        illustration_canvas(
            ui,
            164.0,
            "Illustration 14.2: Flash Player / NeoFlash AVM2 Frame Execution Order on every playhead advancement or tick.",
            |painter, ctx, rect| {
                let p = neoflash_ui::palette(ctx);
                let steps = [
                    ("1. ENTER_FRAME", "Broadcast to all DisplayObjects"),
                    ("2. Construct", "Instantiate new timeline children"),
                    ("3. FRAME_CONSTRUCTED", "Inspect newly created instances"),
                    ("4. Frame Scripts", "Execute timeline keyframe code"),
                    ("5. EXIT_FRAME", "Post-script cleanup & layout"),
                    ("6. RENDER", "Fires if stage.invalidate() called"),
                ];
                let cols = 3;
                let rows = 2;
                let pad_x = 12.0;
                let pad_y = 12.0;
                let cw = (rect.width() - pad_x * 4.0) / cols as f32;
                let ch = (rect.height() - pad_y * 3.0) / rows as f32;
                for (idx, (step, desc)) in steps.iter().enumerate() {
                    let r_i = idx / cols;
                    let c_i = idx % cols;
                    let x = rect.left() + pad_x + c_i as f32 * (cw + pad_x);
                    let y = rect.top() + pad_y + r_i as f32 * (ch + pad_y);
                    let cell = Rect::from_min_size(egui::pos2(x, y), vec2(cw, ch));
                    painter.rect_filled(cell, 5.0, p.bg_panel);
                    painter.rect_stroke(
                        cell,
                        5.0,
                        Stroke::new(1.2_f32, p.accent_default),
                        StrokeKind::Inside,
                    );
                    let inner = cell.shrink(6.0);
                    paint_clipped_text(
                        painter,
                        ctx,
                        inner,
                        egui::pos2(inner.left(), cell.center().y - 8.0),
                        egui::Align2::LEFT_CENTER,
                        step,
                        TextStyle::MonoSmall,
                        p.accent_default,
                    );
                    paint_clipped_text(
                        painter,
                        ctx,
                        inner,
                        egui::pos2(inner.left(), cell.center().y + 10.0),
                        egui::Align2::LEFT_CENTER,
                        desc,
                        TextStyle::Small,
                        p.text_primary,
                    );
                }
            },
        );
    }

    api_signature_table(
        ui,
        "EventDispatcher & Core AS3 Event Classes",
        [
            "Method / Event Class",
            "Key Constants & Properties",
            "Behavior & Best Practices",
        ],
        &[
            (
                "addEventListener",
                "(type, listener, useCapture=false, priority=0, useWeakReference=false)",
                "Higher `priority` listeners execute first within the same phase; `useWeakReference=true` prevents listener memory leaks on short-lived objects.",
            ),
            (
                "event.target vs currentTarget",
                "Object / EventDispatcher",
                "`event.target` is the innermost dispatcher that originated the event (e.g. a child label inside a button); `event.currentTarget` is the object whose listener is currently running.",
            ),
            (
                "stopPropagation vs Immediate",
                "stopPropagation() / stopImmediatePropagation()",
                "`stopPropagation()` lets remaining listeners on the current node finish before halting onward flow; `stopImmediatePropagation()` halts even sibling listeners on the same node.",
            ),
            (
                "Event (Lifecycle)",
                "ENTER_FRAME, FRAME_CONSTRUCTED, EXIT_FRAME, RENDER, ADDED_TO_STAGE, REMOVED_FROM_STAGE, RESIZE, COMPLETE",
                "Always wait for `ADDED_TO_STAGE` before reading `stage.stageWidth` or registering global `stage` keyboard listeners.",
            ),
            (
                "MouseEvent",
                "CLICK, DOUBLE_CLICK, MOUSE_DOWN, MOUSE_UP, MOUSE_MOVE, MOUSE_OVER, MOUSE_OUT, ROLL_OVER, ROLL_OUT, MOUSE_WHEEL",
                "`ROLL_OVER`/`ROLL_OUT` do not bubble when moving between children of the same container; set `doubleClickEnabled = true` to receive `DOUBLE_CLICK`.",
            ),
            (
                "KeyboardEvent & TimerEvent",
                "KEY_DOWN, KEY_UP (keyCode, charCode, shiftKey, ctrlKey); TimerEvent.TIMER, TIMER_COMPLETE",
                "Attach `KeyboardEvent` listeners to `stage` so key presses register regardless of which child display object currently holds focus.",
            ),
        ],
    );

    code_block(
        ui,
        &CodeSample {
            title: "AS3 Event Handling — Stage Keyboard Input, Mouse Dragging & Timer Loop",
            lang_badge: "[AS3]",
            code: "import flash.events.Event;
import flash.events.MouseEvent;
import flash.events.KeyboardEvent;
import flash.events.TimerEvent;
import flash.ui.Keyboard;
import flash.utils.Timer;

// 1. Mouse interaction with target vs currentTarget awareness
card_mc.buttonMode = true;
card_mc.mouseChildren = false; // Ensure event.target is always card_mc
card_mc.addEventListener(MouseEvent.MOUSE_DOWN, function(e:MouseEvent):void {
    card_mc.startDrag();
});
stage.addEventListener(MouseEvent.MOUSE_UP, function(e:MouseEvent):void {
    card_mc.stopDrag();
});

// 2. Global Stage keyboard listener
stage.addEventListener(KeyboardEvent.KEY_DOWN, function(e:KeyboardEvent):void {
    if (e.keyCode == Keyboard.SPACE) {
        trace(\"Spacebar pressed! Shift held:\", e.shiftKey);
    }
});

// 3. Precision Timer (fires every 500 ms, 10 times)
var spawner:Timer = new Timer(500, 10);
spawner.addEventListener(TimerEvent.TIMER, function(e:TimerEvent):void {
    trace(\"Wave tick:\", spawner.currentCount);
});
spawner.start();",
        },
    );

    crate::panels::help_manual::inline_cross_links(
        ui,
        nav_target,
        &[
            (12, "13. AS3 Architecture & Display List"),
            (14, "15. AS3 Graphics, Filters, Sound & Data"),
            (15, "16. AS1/AS2 ↔ AS3 ↔ JS/TS Migration Guide"),
        ],
    );
}

// ── Chapter 15: AS3 Graphics, Filters, Sound & Data ──

pub(crate) fn chapter_as3_graphics_filters_sound(
    ui: &mut Ui,
    _diagram_tab: &mut usize,
    nav_target: &mut Option<usize>,
) {
    paragraph(
        ui,
        "ActionScript 3.0 exposes hardware-accelerated and software-deterministic APIs for retained vector paths (`flash.display.Graphics`), GPU/CPU display filters (`flash.filters.*`), multi-channel audio synthesis and spectrum analysis (`flash.media.*`), binary `ByteArray` streams, and E4X (`XML`).",
    );

    section_overline(
        ui,
        "Interactive Diagram — Vector Graphics Pipeline, Live Filter Pass & SoundChannel Mixer",
    );
    illustration_canvas(
        ui,
        156.0,
        "Illustration 15.1: Left — flash.display.Graphics command stream (strokes, fills, curves, drawTriangles). Centre — DisplayObject.filters & BitmapData pass. Right — Sound → SoundChannel → SoundTransform → SoundMixer.",
        |painter, ctx, rect| {
            let p = neoflash_ui::palette(ctx);
            let w3 = rect.width() / 3.0;
            let panels = [
                (
                    "1. flash.display.Graphics",
                    &[
                        "lineStyle(2, 0x6C93FF, 1.0)",
                        "beginGradientFill(LINEAR, ...)",
                        "moveTo / lineTo / curveTo",
                        "cubicCurveTo(cx1,cy1,cx2,cy2,x,y)",
                        "drawRoundRect / drawTriangles",
                    ],
                    p.accent_default,
                ),
                (
                    "2. Filters & BitmapData",
                    &[
                        "DropShadowFilter / GlowFilter",
                        "BlurFilter(blurX, blurY, quality)",
                        "ColorMatrixFilter (4x5 RGBA)",
                        "BitmapData.perlinNoise / draw",
                        "copyPixels / paletteMap / threshold",
                    ],
                    p.warning_default,
                ),
                (
                    "3. Audio & ByteArray I/O",
                    &[
                        "ch = sound.play(startMs, loops)",
                        "ch.soundTransform = new SoundTransform(vol, pan)",
                        "SoundMixer.computeSpectrum(ba, true)",
                        "ByteArray (compress / readFloat)",
                        "JSON.parse / XML E4X (@attr, ..tag)",
                    ],
                    p.success_default,
                ),
            ];
            for (i, (title, lines, accent)) in panels.iter().enumerate() {
                let cell = Rect::from_min_size(
                    egui::pos2(rect.left() + 8.0 + i as f32 * w3, rect.top() + 10.0),
                    vec2(w3 - 16.0, rect.height() - 20.0),
                );
                painter.rect_filled(cell, 5.0, p.bg_panel);
                painter.rect_stroke(cell, 5.0, Stroke::new(1.2_f32, *accent), StrokeKind::Inside);
                let inner = cell.shrink(6.0);
                paint_clipped_text(
                    painter,
                    ctx,
                    inner,
                    egui::pos2(inner.left(), cell.top() + 16.0),
                    egui::Align2::LEFT_CENTER,
                    title,
                    TextStyle::BodyStrong,
                    *accent,
                );
                for (j, ln) in lines.iter().enumerate() {
                    paint_clipped_text(
                        painter,
                        ctx,
                        inner,
                        egui::pos2(inner.left(), cell.top() + 38.0 + j as f32 * 20.0),
                        egui::Align2::LEFT_CENTER,
                        ln,
                        TextStyle::MonoSmall,
                        p.text_primary,
                    );
                }
            }
        },
    );

    api_signature_table(
        ui,
        "AS3 Graphics, Geometry, Filters, Sound & Serialization Reference",
        ["Package / Class", "Core APIs", "Key Details"],
        &[
            (
                "flash.display.Graphics",
                "clear, lineStyle, beginFill, beginGradientFill, beginBitmapFill, drawRect, drawRoundRect, drawCircle, drawEllipse, moveTo, lineTo, curveTo, cubicCurveTo, drawPath, drawTriangles, endFill",
                "Note: AS3 `alpha` arguments in `beginFill(color, alpha)` and `lineStyle` are `0.0..1.0` (unlike AS2's `0..100`).",
            ),
            (
                "flash.geom.*",
                "Point, Rectangle, Matrix(a,b,c,d,tx,ty), Matrix3D, ColorTransform, Transform",
                "`matrix.createGradientBox(w, h, rot, tx, ty)` prepares linear/radial gradient UV space for `beginGradientFill`.",
            ),
            (
                "flash.filters.*",
                "BlurFilter, DropShadowFilter, GlowFilter, BevelFilter, GradientGlowFilter, ColorMatrixFilter, ConvolutionFilter",
                "Mutating an element of `mc.filters` in-place has no effect until you reassign the array: `mc.filters = [blur, glow];`.",
            ),
            (
                "flash.media.Sound*",
                "Sound, SoundChannel (position, leftPeak, rightPeak, stop, SOUND_COMPLETE), SoundTransform(volume, pan), SoundMixer",
                "`sound.play()` returns a new `SoundChannel` handle for each playing voice; `SoundDataEvent.SAMPLE_DATA` synthesizes dynamic PCM.",
            ),
            (
                "flash.utils.ByteArray",
                "endian, position, length, writeUTFBytes, readUTFBytes, writeInt, readFloat, compress(), uncompress()",
                "Supports `Endian.BIG_ENDIAN` and `LITTLE_ENDIAN` plus `CompressionAlgorithm.ZLIB`, `DEFLATE`, and `LZMA`.",
            ),
            (
                "XML (E4X) & JSON",
                "new XML(str), xml.item.(@id == \"hero\"), xml..node, JSON.parse(str), JSON.stringify(obj)",
                "E4X (`..` descendant axis, `@` attribute axis, predicate filters `.(@price < 50)`) queries XML trees natively.",
            ),
            (
                "Stage & Fullscreen (AS3)",
                "StageScaleMode (SHOW_ALL, NO_SCALE, EXACT_FIT, NO_BORDER), StageAlign (TOP, BOTTOM, LEFT, RIGHT, TOP_LEFT, ...), StageDisplayState.FULL_SCREEN",
                "High-DPI Canvas layout keeps `stage.mouseX`/`mouseY` and hit-testing aligned across all 9 anchor origins and fullscreen transitions.",
            ),
            (
                "Loader (`loadPublish` & Media) + AE Expressions",
                "new Loader(), contentLoaderInfo.addEventListener(Event.COMPLETE, fn), load(new URLRequest(url))",
                "Loads `SWF`, `MP4`, `AVI`, `MP3`, `WAV`, `SVG`, `WebP`, `GIF`, `BMP`, `TIFF`, and published NeoFlash HTML5 child bundles (`loadPublish`); `enterFrame`/frame drivers also compile to After Effects JS Expressions on `.aep` export.",
            ),
        ],
    );

    code_block(
        ui,
        &CodeSample {
            title: "AS3 Vector Graphics, Live Filters, SoundChannel & E4X XML Query",
            lang_badge: "[AS3]",
            code: "import flash.display.Sprite;
import flash.filters.GlowFilter;
import flash.media.Sound;
import flash.media.SoundChannel;
import flash.media.SoundTransform;

// 1. Draw a smooth badge and attach a live GlowFilter
var orb:Sprite = new Sprite();
orb.graphics.lineStyle(2, 0x86A5FF, 1.0);
orb.graphics.beginFill(0x3C6BF0, 0.85);
orb.graphics.drawCircle(0, 0, 36);
orb.graphics.endFill();
orb.filters = [new GlowFilter(0x6C93FF, 0.9, 16, 16, 2, 2)];
addChild(orb);

// 2. Play a Sound with stereo panning
var sfx:Sound = new Sound();
var channel:SoundChannel = sfx.play(0, 1, new SoundTransform(0.8, -0.25));

// 3. E4X XML attribute & descendant filtering
var inventory:XML = <items>
    <weapon id=\"plasma\" dmg=\"45\"/>
    <weapon id=\"railgun\" dmg=\"90\"/>
</items>;
var heavy:XMLList = inventory.weapon.(Number(@dmg) >= 50);
trace(\"Selected weapon:\", heavy.@id); // railgun",
        },
    );

    crate::panels::help_manual::inline_cross_links(
        ui,
        nav_target,
        &[
            (7, "8. Tweens, Motion & IK"),
            (8, "9. Filters, Text & Audio"),
            (12, "13. AS3 Architecture & Display List"),
        ],
    );
}
