//! Scripting Reference (`Help ▸ Scripting Reference…`): the API browser over the ActionScript 2 / 3
//! declaration index and the JavaScript / TypeScript catalogue, the guide chapters (11–19 of the
//! manual), the shared JS/TS Actions toolbox catalogue, and the SVG-illustrated HTML5 browser
//! edition of Help and the reference.
//!
//! The window shell is `help_manual.rs`.

use super::help_manual::{
    AUTHORING_CHAPTER_COUNT, HELP_BROWSER_TOPICS, HELP_CHAPTERS, LanguageScope, fit_text_to_width,
    illustration_canvas, mode_card, outline_heading, paint_clipped_text, paragraph,
    plain_chapter_label, section_overline, sub_bullet, text_width,
};
use crate::code_tools::CodeCommand;
use crate::script_api::{self, Member};
use crate::theme::ScrollTheme as _;
use eframe::egui::{self, Rect, Stroke, StrokeKind, Ui, vec2};
use neoflash_core::ScriptLanguage;
use neoflash_ui::{
    Palette, TextStyle, Theme,
    components::{self as c, Badge, BadgeType, Button},
    icons::{self, i16},
    tokens::{radius, space},
    typography::{font, rich},
};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::sync::{Arc, OnceLock};

/// Formats an `egui::Color32` as a CSS `rgb(...)` or `rgba(...)` functional color string.
/// Uses decimal channel notation so no literal hex strings exist in source.
pub(crate) fn css_rgb(color: egui::Color32) -> String {
    let [r, g, b, a] = color.to_srgba_unmultiplied();
    if a == 255 {
        format!("rgb({r}, {g}, {b})")
    } else {
        format!("rgba({r}, {g}, {b}, {:.3})", f32::from(a) / 255.0)
    }
}

/// All filterable topic tags across the 19 Help & Scripting Reference chapters.
pub(crate) const HELP_ALL_TAGS: &[&str] = &[
    "Authoring",
    "Stage",
    "Vector",
    "Tools",
    "Symbols",
    "Timeline",
    "Animation",
    "Text",
    "Audio",
    "Export",
    "AS1",
    "AS2",
    "AS3",
    "JavaScript",
    "TypeScript",
    "WebAPI",
    "Migration",
    "DisplayList",
    "Events",
    "Graphics",
];

/// Provenance and item kind for the shared JavaScript / TypeScript / Web API toolbox and manual.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JsTsEntryKind {
    Binding,
    Function,
    Keyword,
    TypeConstruct,
    Class,
    Member(&'static str),
}

/// Single source-of-truth entry for JavaScript, TypeScript, NeoFlash runtime bindings, and
/// Browser / Web APIs used by both the `Actions` toolbox tree (`actions.rs`) and the
/// `Scripting Reference` chapters (`help_as_reference.rs`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct JsTsApiEntry {
    pub category: &'static str,
    pub name: &'static str,
    pub signature: &'static str,
    pub provenance: &'static str,
    pub availability: &'static str,
    pub summary: &'static str,
    pub insertion: &'static str,
    pub ts_only: bool,
    pub kind: JsTsEntryKind,
}

pub(crate) const JS_TS_CATEGORIES: &[(&str, &str, bool)] = &[
    (
        "NeoFlash Context & Timeline",
        "Frame/object script bindings (this, event, trace, timeline & display controls)",
        false,
    ),
    (
        "Modules & JSX",
        "Deterministic project modules (import/export) and JSX/TSX (__nfJsx)",
        false,
    ),
    (
        "ECMAScript & Async",
        "ES2023+ classes, collections, JSON, Promise, async/await, and Error handling",
        false,
    ),
    (
        "TypeScript & Checker",
        "Interfaces, type aliases, generics, unions, and built-in ts_check utilities",
        true,
    ),
    (
        "Browser DOM & Canvas 2D",
        "Published HTML5 host DOM, CanvasRenderingContext2D, timers & input events",
        false,
    ),
    (
        "Web Fetch, Audio & Storage",
        "Browser fetch, Web Audio (AudioContext), HTMLMediaElement & localStorage",
        false,
    ),
];

pub(crate) const JS_TS_API_ENTRIES: &[JsTsApiEntry] = &[
    // ── Category 1: NeoFlash Context & Timeline ──
    JsTsApiEntry {
        category: "NeoFlash Context & Timeline",
        name: "this",
        signature: "this: MovieClip | DisplayObject",
        provenance: "NeoFlash binding",
        availability: "Frame scripts (owning MovieClip) & Object scripts (attached instance)",
        summary: "Implicit receiver in NeoFlash frame and object scripts; exposes x, y, scaleX, scaleY, rotation, alpha, visible, currentFrame, totalFrames, and child instances.",
        insertion: "this.",
        ts_only: false,
        kind: JsTsEntryKind::Binding,
    },
    JsTsApiEntry {
        category: "NeoFlash Context & Timeline",
        name: "event",
        signature: "event: NeoFlashEvent ({ type, target, currentTarget, keyCode, stageX, stageY })",
        provenance: "NeoFlash binding",
        availability: "Object event handlers & addEventListener callbacks",
        summary: "Structured event payload passed to object scripts and listener callbacks with target, currentTarget, pointer coordinates, and key state.",
        insertion: "event.target",
        ts_only: false,
        kind: JsTsEntryKind::Binding,
    },
    JsTsApiEntry {
        category: "NeoFlash Context & Timeline",
        name: "trace",
        signature: "trace(...values: unknown[]): void",
        provenance: "NeoFlash binding",
        availability: "Authoring Test Movie (Output panel F2) & HTML5 browser console",
        summary: "Formats and appends diagnostic output to the NeoFlash Output panel (F2) and forwards to console.log in published HTML5 bundles.",
        insertion: "trace(\"\");",
        ts_only: false,
        kind: JsTsEntryKind::Function,
    },
    JsTsApiEntry {
        category: "NeoFlash Context & Timeline",
        name: "play",
        signature: "play(): void",
        provenance: "NeoFlash binding",
        availability: "Global frame script helper & MovieClip.play() method",
        summary: "Resumes playhead advancement on the active timeline or target MovieClip.",
        insertion: "play();",
        ts_only: false,
        kind: JsTsEntryKind::Function,
    },
    JsTsApiEntry {
        category: "NeoFlash Context & Timeline",
        name: "stop",
        signature: "stop(): void",
        provenance: "NeoFlash binding",
        availability: "Global frame script helper & MovieClip.stop() method",
        summary: "Halts playhead advancement on the active timeline while nested child MovieClips continue running on their own clocks.",
        insertion: "stop();",
        ts_only: false,
        kind: JsTsEntryKind::Function,
    },
    JsTsApiEntry {
        category: "NeoFlash Context & Timeline",
        name: "gotoAndPlay",
        signature: "gotoAndPlay(frameOrLabel: number | string, scene?: string): void",
        provenance: "NeoFlash binding",
        availability: "Global frame script helper & MovieClip.gotoAndPlay() (1-based frame index)",
        summary: "Jumps the timeline playhead to a 1-based frame number or authored frame label (optionally in a named Scene) and continues playback.",
        insertion: "gotoAndPlay(1);",
        ts_only: false,
        kind: JsTsEntryKind::Function,
    },
    JsTsApiEntry {
        category: "NeoFlash Context & Timeline",
        name: "gotoAndStop",
        signature: "gotoAndStop(frameOrLabel: number | string, scene?: string): void",
        provenance: "NeoFlash binding",
        availability: "Global frame script helper & MovieClip.gotoAndStop() (1-based frame index)",
        summary: "Moves the timeline playhead to a 1-based frame number or authored frame label and stops immediately.",
        insertion: "gotoAndStop(1);",
        ts_only: false,
        kind: JsTsEntryKind::Function,
    },
    JsTsApiEntry {
        category: "NeoFlash Context & Timeline",
        name: "nextFrame",
        signature: "nextFrame(): void",
        provenance: "NeoFlash binding",
        availability: "Global frame script helper & MovieClip.nextFrame()",
        summary: "Advances the timeline playhead by one frame and stops playback.",
        insertion: "nextFrame();",
        ts_only: false,
        kind: JsTsEntryKind::Function,
    },
    JsTsApiEntry {
        category: "NeoFlash Context & Timeline",
        name: "prevFrame",
        signature: "prevFrame(): void",
        provenance: "NeoFlash binding",
        availability: "Global frame script helper & MovieClip.prevFrame()",
        summary: "Steps the timeline playhead back by one frame and stops playback.",
        insertion: "prevFrame();",
        ts_only: false,
        kind: JsTsEntryKind::Function,
    },
    JsTsApiEntry {
        category: "NeoFlash Context & Timeline",
        name: "stopAllSounds",
        signature: "stopAllSounds(): void",
        provenance: "NeoFlash binding",
        availability: "Authoring preview & HTML5 Web Audio scheduler",
        summary: "Immediately silences all active Event, Start, and Stream timeline sounds.",
        insertion: "stopAllSounds();",
        ts_only: false,
        kind: JsTsEntryKind::Function,
    },
    JsTsApiEntry {
        category: "NeoFlash Context & Timeline",
        name: "NeoFlash.loadPublish",
        signature: "NeoFlash.loadPublish(url: string, target?: MovieClip | DisplayObjectContainer): Promise<DisplayObject>",
        provenance: "NeoFlash binding",
        availability: "HTML5 & desktop runtime child-publish loader",
        summary: "Dynamically loads and mounts another published NeoFlash HTML5 bundle (`index.html`, `bundle.js`, or directory URL) into a target container with isolated timeline/script execution.",
        insertion: "NeoFlash.loadPublish(\"subapp/bundle.js\", this);",
        ts_only: false,
        kind: JsTsEntryKind::Function,
    },
    JsTsApiEntry {
        category: "NeoFlash Context & Timeline",
        name: "getChildByName",
        signature: "this.getChildByName(name: string): DisplayObject | null",
        provenance: "NeoFlash binding",
        availability: "DisplayObjectContainer / MovieClip instances",
        summary: "Looks up a named child instance on the current timeline or container.",
        insertion: "this.getChildByName(\"instanceName\")",
        ts_only: false,
        kind: JsTsEntryKind::Function,
    },
    JsTsApiEntry {
        category: "NeoFlash Context & Timeline",
        name: "addEventListener",
        signature: "target.addEventListener(type: string, listener: (event: any) => void): void",
        provenance: "NeoFlash binding",
        availability: "MovieClip, Sprite, Button, Stage & DOM EventTargets",
        summary: "Registers an event listener for frame lifecycle (\"enterFrame\"), pointer (\"click\", \"pointerdown\"), or keyboard events.",
        insertion: "this.addEventListener(\"enterFrame\", (event) => {\n    \n});",
        ts_only: false,
        kind: JsTsEntryKind::Function,
    },
    // ── Category 2: Modules & JSX ──
    JsTsApiEntry {
        category: "Modules & JSX",
        name: "import",
        signature: "import { symbol } from \"./module\";",
        provenance: "Project module",
        availability: "Project .js/.jsx/.ts/.tsx files (relative paths ./ and ../ only; no Node/npm packages)",
        summary: "Imports bindings from another script module in the NeoFlash project graph; bundled deterministically in topological order.",
        insertion: "import { } from \"./module\";",
        ts_only: false,
        kind: JsTsEntryKind::Keyword,
    },
    JsTsApiEntry {
        category: "Modules & JSX",
        name: "export",
        signature: "export function name() { ... } | export class Name { ... }",
        provenance: "Project module",
        availability: "Project .js/.jsx/.ts/.tsx module files",
        summary: "Exports functions, classes, constants, or types so other project modules and timeline scripts can import them.",
        insertion: "export function helper() {\n    \n}",
        ts_only: false,
        kind: JsTsEntryKind::Keyword,
    },
    JsTsApiEntry {
        category: "Modules & JSX",
        name: "__nfJsx",
        signature: "__nfJsx(tag: string | Function, props: Record<string, any> | null, ...children: any[])",
        provenance: "NeoFlash binding",
        availability: "Compiled automatically from <Tag prop={val}>...</Tag> in .jsx and .tsx files",
        summary: "NeoFlash's built-in JSX/TSX factory; lowers declarative element trees without requiring React or external bundlers.",
        insertion: "const view = <hud-panel title=\"Score\">{score}</hud-panel>;",
        ts_only: false,
        kind: JsTsEntryKind::Function,
    },
    // ── Category 3: ECMAScript & Async ──
    JsTsApiEntry {
        category: "ECMAScript & Async",
        name: "function",
        signature: "function name(p1, p2 = defaultVal, ...rest) { return value; } | (a, b) => a + b",
        provenance: "ECMAScript",
        availability: "Standard ES2023+ hoisted declarations, function expressions, closures & arrow functions",
        summary: "Defines reusable callable blocks, lexical closures, higher-order callbacks, and explicit receiver methods.",
        insertion: "function name() {\n    \n}",
        ts_only: false,
        kind: JsTsEntryKind::Keyword,
    },
    JsTsApiEntry {
        category: "ECMAScript & Async",
        name: "Function",
        signature: "fn.call(thisArg, ...args) | fn.apply(thisArg, argsArray) | fn.bind(thisArg, ...args)",
        provenance: "ECMAScript",
        availability: "Standard ES2023+ Function.prototype receiver-binding methods",
        summary: "Invokes or binds a function with an explicit `this` MovieClip/object receiver and individual or array-packed arguments.",
        insertion: "fn.call(this)",
        ts_only: false,
        kind: JsTsEntryKind::Class,
    },
    JsTsApiEntry {
        category: "ECMAScript & Async",
        name: "return",
        signature: "return expression; | return;",
        provenance: "ECMAScript",
        availability: "Inside any function, method, or arrow block body",
        summary: "Exits the active function call immediately and returns `expression` (or `undefined` when omitted).",
        insertion: "return;",
        ts_only: false,
        kind: JsTsEntryKind::Keyword,
    },
    JsTsApiEntry {
        category: "ECMAScript & Async",
        name: "Promise",
        signature: "new Promise<T>((resolve, reject) => { ... }) | Promise.all(iterable)",
        provenance: "ECMAScript",
        availability: "Standard ES2023+ runtime & browser host microtask queue",
        summary: "Represents the eventual completion or failure of an asynchronous operation; works seamlessly with async/await and fetch().",
        insertion: "new Promise((resolve, reject) => {\n    resolve(true);\n})",
        ts_only: false,
        kind: JsTsEntryKind::Class,
    },
    JsTsApiEntry {
        category: "ECMAScript & Async",
        name: "Promise.all",
        signature: "Promise.all(values: Iterable<Promise<T>>): Promise<T[]>",
        provenance: "ECMAScript",
        availability: "Standard ES2023+ Promise combinator",
        summary: "Awaits multiple promises in parallel and resolves to an array of their results.",
        insertion: "await Promise.all([])",
        ts_only: false,
        kind: JsTsEntryKind::Member("Promise"),
    },
    JsTsApiEntry {
        category: "ECMAScript & Async",
        name: "async / await",
        signature: "async function loadData(): Promise<void> { const res = await task(); }",
        provenance: "ECMAScript",
        availability: "Standard ES2023+ asynchronous functions and arrow functions",
        summary: "Suspends execution until a Promise settles without blocking the main animation or rendering loop.",
        insertion: "async function run() {\n    \n}",
        ts_only: false,
        kind: JsTsEntryKind::Keyword,
    },
    JsTsApiEntry {
        category: "ECMAScript & Async",
        name: "Map",
        signature: "new Map<K, V>()",
        provenance: "ECMAScript",
        availability: "Standard ES2023+ keyed collection",
        summary: "Insertion-ordered key-value dictionary supporting any key type (.get, .set, .has, .delete).",
        insertion: "new Map()",
        ts_only: false,
        kind: JsTsEntryKind::Class,
    },
    JsTsApiEntry {
        category: "ECMAScript & Async",
        name: "Set",
        signature: "new Set<T>()",
        provenance: "ECMAScript",
        availability: "Standard ES2023+ unique value collection",
        summary: "Insertion-ordered set of unique values (.add, .has, .delete, .size).",
        insertion: "new Set()",
        ts_only: false,
        kind: JsTsEntryKind::Class,
    },
    JsTsApiEntry {
        category: "ECMAScript & Async",
        name: "JSON",
        signature: "JSON.parse(text: string) | JSON.stringify(value: any, replacer?, space?)",
        provenance: "ECMAScript",
        availability: "Standard ES2023+ & NeoFlash runtime",
        summary: "Parses and serializes structured JSON payloads for save states, level descriptors, and network APIs.",
        insertion: "JSON.stringify({})",
        ts_only: false,
        kind: JsTsEntryKind::Class,
    },
    JsTsApiEntry {
        category: "ECMAScript & Async",
        name: "Math",
        signature: "Math.sin(x) | Math.cos(x) | Math.atan2(y, x) | Math.hypot(dx, dy) | Math.random()",
        provenance: "ECMAScript",
        availability: "Standard ES2023+ & deterministic NeoFlash math intrinsics",
        summary: "IEEE-754 floating-point trigonometry, clamping, rounding, and pseudo-random generation.",
        insertion: "Math.hypot(dx, dy)",
        ts_only: false,
        kind: JsTsEntryKind::Class,
    },
    JsTsApiEntry {
        category: "ECMAScript & Async",
        name: "Error",
        signature: "throw new Error(message: string) | try { ... } catch (err) { ... }",
        provenance: "ECMAScript",
        availability: "Standard ES2023+ structured exception handling",
        summary: "Constructs and catches runtime errors with stack traces mapped back via NeoFlash source maps.",
        insertion: "throw new Error(\"\");",
        ts_only: false,
        kind: JsTsEntryKind::Class,
    },
    // ── Category 4: TypeScript & Checker ──
    JsTsApiEntry {
        category: "TypeScript & Checker",
        name: "function (typed)",
        signature: "function calcDamage(base: number, crit?: boolean): number | type Handler = (x: number) => void",
        provenance: "TypeScript",
        availability: "Built-in NeoFlash TypeScript checker (ts_check); parameter/return annotations erased at runtime",
        summary: "Declares statically checked parameter types, optional (?) / default parameters, return types, union arguments, and callable type aliases.",
        insertion: "function compute(value: number): number {\n    return value;\n}",
        ts_only: true,
        kind: JsTsEntryKind::TypeConstruct,
    },
    JsTsApiEntry {
        category: "TypeScript & Checker",
        name: "interface",
        signature: "interface Name { readonly id: string; x: number; y?: number; }",
        provenance: "TypeScript",
        availability: "Built-in NeoFlash TypeScript checker (ts_check); erased at runtime",
        summary: "Declares a structural object contract checked for property existence, optionality (?), and member types before stripping.",
        insertion: "interface ActorState {\n    id: string;\n    x: number;\n    y: number;\n}",
        ts_only: true,
        kind: JsTsEntryKind::TypeConstruct,
    },
    JsTsApiEntry {
        category: "TypeScript & Checker",
        name: "type",
        signature: "type Alias<T> = string | number | T;",
        provenance: "TypeScript",
        availability: "Built-in NeoFlash TypeScript checker (ts_check); erased at runtime",
        summary: "Defines a named union, intersection, tuple, function signature, or generic type alias.",
        insertion: "type FrameTarget = number | string;",
        ts_only: true,
        kind: JsTsEntryKind::TypeConstruct,
    },
    JsTsApiEntry {
        category: "TypeScript & Checker",
        name: "enum",
        signature: "enum State { Idle = \"idle\", Running = \"running\" }",
        provenance: "TypeScript",
        availability: "TypeScript numeric & string enums lowered to runtime lookup objects",
        summary: "Declares a set of named numeric or string constants accessible at both compile time and runtime.",
        insertion: "enum GamePhase {\n    Title = \"title\",\n    Playing = \"playing\",\n}",
        ts_only: true,
        kind: JsTsEntryKind::TypeConstruct,
    },
    JsTsApiEntry {
        category: "TypeScript & Checker",
        name: "Record",
        signature: "Record<K extends string | number, V>",
        provenance: "TypeScript",
        availability: "Built-in ts_check utility type",
        summary: "Constructs an object type whose property keys are K and whose property values are V.",
        insertion: "Record<string, number>",
        ts_only: true,
        kind: JsTsEntryKind::TypeConstruct,
    },
    JsTsApiEntry {
        category: "TypeScript & Checker",
        name: "Partial",
        signature: "Partial<T> | Required<T> | Readonly<T>",
        provenance: "TypeScript",
        availability: "Built-in ts_check utility type",
        summary: "Marks all properties of interface or object type T as optional (or required/readonly).",
        insertion: "Partial<ActorState>",
        ts_only: true,
        kind: JsTsEntryKind::TypeConstruct,
    },
    JsTsApiEntry {
        category: "TypeScript & Checker",
        name: "Pick / Omit",
        signature: "Pick<T, K extends keyof T> | Omit<T, K extends keyof T>",
        provenance: "TypeScript",
        availability: "Built-in ts_check utility type",
        summary: "Projects a subset of properties from type T by selecting or excluding keys K.",
        insertion: "Pick<ActorState, \"x\" | \"y\">",
        ts_only: true,
        kind: JsTsEntryKind::TypeConstruct,
    },
    // ── Category 5: Browser DOM & Canvas 2D ──
    JsTsApiEntry {
        category: "Browser DOM & Canvas 2D",
        name: "CanvasRenderingContext2D",
        signature: "ctx: CanvasRenderingContext2D (canvas.getContext(\"2d\"))",
        provenance: "Browser host",
        availability: "Published HTML5 browser host (requires DOM <canvas> element)",
        summary: "HTML5 2D rendering context for immediate-mode paths, fills, strokes, text, images, transforms, and pixel buffers.",
        insertion: "/** @type {CanvasRenderingContext2D} */\nconst ctx = canvas.getContext(\"2d\");",
        ts_only: false,
        kind: JsTsEntryKind::Class,
    },
    JsTsApiEntry {
        category: "Browser DOM & Canvas 2D",
        name: "fillRect",
        signature: "ctx.fillRect(x: number, y: number, width: number, height: number): void",
        provenance: "Browser host",
        availability: "CanvasRenderingContext2D in HTML5 browser host",
        summary: "Paints a filled axis-aligned rectangle using the current ctx.fillStyle.",
        insertion: "ctx.fillRect(0, 0, 100, 60);",
        ts_only: false,
        kind: JsTsEntryKind::Member("CanvasRenderingContext2D"),
    },
    JsTsApiEntry {
        category: "Browser DOM & Canvas 2D",
        name: "beginPath / arc / fill",
        signature: "ctx.beginPath(); ctx.arc(x, y, radius, startAngle, endAngle); ctx.fill();",
        provenance: "Browser host",
        availability: "CanvasRenderingContext2D in HTML5 browser host",
        summary: "Starts a sub-path, appends a circular arc in radians, and fills the contour.",
        insertion: "ctx.beginPath();\nctx.arc(60, 60, 24, 0, Math.PI * 2);\nctx.fill();",
        ts_only: false,
        kind: JsTsEntryKind::Member("CanvasRenderingContext2D"),
    },
    JsTsApiEntry {
        category: "Browser DOM & Canvas 2D",
        name: "drawImage",
        signature: "ctx.drawImage(image: CanvasImageSource, dx: number, dy: number, dw?: number, dh?: number): void",
        provenance: "Browser host",
        availability: "CanvasRenderingContext2D in HTML5 browser host (subject to CORS taint rules)",
        summary: "Draws an HTMLImageElement, HTMLCanvasElement, HTMLVideoElement, or ImageBitmap onto the 2D canvas.",
        insertion: "ctx.drawImage(image, 0, 0);",
        ts_only: false,
        kind: JsTsEntryKind::Member("CanvasRenderingContext2D"),
    },
    JsTsApiEntry {
        category: "Browser DOM & Canvas 2D",
        name: "save / restore / setTransform",
        signature: "ctx.save(); ctx.setTransform(a, b, c, d, e, f); ctx.restore();",
        provenance: "Browser host",
        availability: "CanvasRenderingContext2D in HTML5 browser host",
        summary: "Pushes/pops the 2D drawing state stack and applies a 2×3 affine transformation matrix.",
        insertion: "ctx.save();\nctx.translate(x, y);\nctx.restore();",
        ts_only: false,
        kind: JsTsEntryKind::Member("CanvasRenderingContext2D"),
    },
    JsTsApiEntry {
        category: "Browser DOM & Canvas 2D",
        name: "document.querySelector",
        signature: "document.querySelector(selector: string): Element | null",
        provenance: "Browser host",
        availability: "Published HTML5 browser host DOM (not available in headless non-browser tests)",
        summary: "Returns the first DOM Element matching a CSS selector for custom HTML overlays or canvas host lookup.",
        insertion: "document.querySelector(\"#stage-canvas\")",
        ts_only: false,
        kind: JsTsEntryKind::Function,
    },
    JsTsApiEntry {
        category: "Browser DOM & Canvas 2D",
        name: "requestAnimationFrame",
        signature: "requestAnimationFrame(callback: (timestampMs: number) => void): number",
        provenance: "Browser host",
        availability: "Published HTML5 browser host & NeoFlash frame scheduler",
        summary: "Schedules a callback before the browser's next display repaint, passing a high-resolution DOMHighResTimeStamp.",
        insertion: "requestAnimationFrame((nowMs) => {\n    \n});",
        ts_only: false,
        kind: JsTsEntryKind::Function,
    },
    JsTsApiEntry {
        category: "Browser DOM & Canvas 2D",
        name: "setTimeout / clearTimeout",
        signature: "setTimeout(handler: () => void, delayMs: number): number",
        provenance: "Browser host",
        availability: "Published HTML5 browser host & NeoFlash timer queue",
        summary: "Schedules a one-shot callback after delayMs milliseconds; cancel with clearTimeout(id).",
        insertion: "setTimeout(() => {\n    \n}, 250);",
        ts_only: false,
        kind: JsTsEntryKind::Function,
    },
    // ── Category 6: Web Fetch, Audio & Storage ──
    JsTsApiEntry {
        category: "Web Fetch, Audio & Storage",
        name: "fetch",
        signature: "fetch(input: string | Request, init?: RequestInit): Promise<Response>",
        provenance: "Browser host",
        availability: "HTML5 browser host (requires CORS headers & CSP connect-src permission)",
        summary: "Performs an asynchronous HTTP/HTTPS request and returns a Promise resolving to a Response (.json(), .text(), .arrayBuffer()).",
        insertion: "const response = await fetch(\"./data/level.json\");\nconst data = await response.json();",
        ts_only: false,
        kind: JsTsEntryKind::Function,
    },
    JsTsApiEntry {
        category: "Web Fetch, Audio & Storage",
        name: "localStorage",
        signature: "localStorage.getItem(key: string): string | null | localStorage.setItem(key, value)",
        provenance: "Browser host",
        availability: "HTML5 browser host (same-origin ~5 MB quota; may throw in strict private mode)",
        summary: "Reads and writes persistent key-value strings scoped to the published page's origin.",
        insertion: "localStorage.setItem(\"neoflash.save\", JSON.stringify({ score: 100 }));",
        ts_only: false,
        kind: JsTsEntryKind::Class,
    },
    JsTsApiEntry {
        category: "Web Fetch, Audio & Storage",
        name: "AudioContext",
        signature: "new AudioContext() | audioCtx.resume(): Promise<void>",
        provenance: "Browser host",
        availability: "HTML5 browser host (requires user gesture click/key press before audio unmutes)",
        summary: "Web Audio API graph context used by web/audio.js and custom procedural audio nodes.",
        insertion: "const audioCtx = new AudioContext();\nawait audioCtx.resume();",
        ts_only: false,
        kind: JsTsEntryKind::Class,
    },
];

/// Filters `JS_TS_API_ENTRIES` by language (`include_ts`) and case-insensitive substring query.
pub(crate) fn search_js_ts_entries(query: &str, include_ts: bool) -> Vec<&'static JsTsApiEntry> {
    let q = query.trim().to_ascii_lowercase();
    JS_TS_API_ENTRIES
        .iter()
        .filter(|entry| {
            if entry.ts_only && !include_ts {
                return false;
            }
            if q.is_empty() {
                return true;
            }
            entry.name.to_ascii_lowercase().contains(&q)
                || entry.signature.to_ascii_lowercase().contains(&q)
                || entry.category.to_ascii_lowercase().contains(&q)
                || entry.provenance.to_ascii_lowercase().contains(&q)
                || entry.summary.to_ascii_lowercase().contains(&q)
        })
        .collect()
}

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

    super::help_manual::inline_cross_links(
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

    super::help_manual::inline_cross_links(
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

    super::help_manual::inline_cross_links(
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

    super::help_manual::inline_cross_links(
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

    super::help_manual::inline_cross_links(
        ui,
        nav_target,
        &[
            (7, "8. Tweens, Motion & IK"),
            (8, "9. Filters, Text & Audio"),
            (12, "13. AS3 Architecture & Display List"),
        ],
    );
}

// ── Chapter 16: AS1/AS2 ↔ AS3 ↔ JS/TS Migration Guide ──

pub(crate) fn chapter_as_migration_guide(
    ui: &mut Ui,
    _diagram_tab: &mut usize,
    nav_target: &mut Option<usize>,
) {
    paragraph(
        ui,
        "NeoFlash is uniquely designed to author and publish ActionScript 1.0/2.0, ActionScript 3.0, JavaScript (ES2023+), and TypeScript projects side by side. Use this side-by-side equivalence guide when porting legacy FLA scripts to AS3 or modern HTML5 JS/TS.",
    );

    section_overline(
        ui,
        "Interactive Diagram — Multi-Language Execution Bridge to the Shared Rust / WASM Engine",
    );
    illustration_canvas(
        ui,
        142.0,
        "Illustration 16.1: All four NeoFlash scripting languages compile and bind to the same deterministic Rust scene graph, timeline evaluator, filter compositor, and Web Audio scheduler.",
        |painter, ctx, rect| {
            let p = neoflash_ui::palette(ctx);
            let langs = [
                ("AS1 / AS2 (AVM1)", "_root, _x, onEnterFrame, attachMovie"),
                ("AS3 (AVM2)", "stage, x, addEventListener, addChild"),
                (
                    "JavaScript / TypeScript",
                    "this, mc.x, addEventListener, import",
                ),
            ];
            let pad = 12.0;
            let lw = (rect.width() - pad * 4.0) / 3.0;
            for (i, (name, ex)) in langs.iter().enumerate() {
                let x = rect.left() + pad + i as f32 * (lw + pad);
                let top_box = Rect::from_min_size(egui::pos2(x, rect.top() + 10.0), vec2(lw, 52.0));
                painter.rect_filled(top_box, 5.0, p.bg_panel);
                painter.rect_stroke(
                    top_box,
                    5.0,
                    Stroke::new(1.2_f32, p.accent_default),
                    StrokeKind::Inside,
                );
                let inner = top_box.shrink(5.0);
                paint_clipped_text(
                    painter,
                    ctx,
                    inner,
                    egui::pos2(top_box.center().x, top_box.top() + 18.0),
                    egui::Align2::CENTER_CENTER,
                    name,
                    TextStyle::BodyStrong,
                    p.accent_default,
                );
                paint_clipped_text(
                    painter,
                    ctx,
                    inner,
                    egui::pos2(top_box.center().x, top_box.top() + 36.0),
                    egui::Align2::CENTER_CENTER,
                    ex,
                    TextStyle::MonoSmall,
                    p.text_secondary,
                );
                painter.line_segment(
                    [
                        egui::pos2(top_box.center().x, top_box.bottom() + 2.0),
                        egui::pos2(top_box.center().x, rect.bottom() - 44.0),
                    ],
                    Stroke::new(1.5_f32, p.accent_default),
                );
            }
            let core_bar = Rect::from_min_max(
                egui::pos2(rect.left() + pad, rect.bottom() - 42.0),
                egui::pos2(rect.right() - pad, rect.bottom() - 10.0),
            );
            painter.rect_filled(core_bar, 5.0, p.accent_subtle);
            painter.rect_stroke(
                core_bar,
                5.0,
                Stroke::new(1.5_f32, p.accent_default),
                StrokeKind::Inside,
            );
            paint_clipped_text(
                painter,
                ctx,
                core_bar.shrink(6.0),
                core_bar.center(),
                egui::Align2::CENTER_CENTER,
                "NeoFlash Shared Rust / WebAssembly Runtime (Display List · Timeline · Vector/Filter Renderer · Web Audio)",
                TextStyle::BodyStrong,
                p.text_primary,
            );
        },
    );

    api_signature_table(
        ui,
        "Rosetta Stone — AS1/AS2 vs AS3 vs JavaScript/TypeScript Equivalents",
        [
            "ActionScript 1.0 / 2.0",
            "ActionScript 3.0",
            "JavaScript / TypeScript & Scale Note",
        ],
        &[
            (
                "function (arguments / callee)",
                "function (a:int = 0, ...rest):Number",
                "`function (a = 0, ...rest)` or `(a, ...rest) => ...` (typed rest arrays replace `arguments.callee`).",
            ),
            (
                "Enemy.prototype.step = fn",
                "class Enemy { public function step():void }",
                "`class Enemy { step() { ... } }` (AS3 methods auto-bind `this`; JS/TS use arrow fields or `.bind(this)`).",
            ),
            (
                "fn.call(mc, a) / fn.apply(mc, [a])",
                "fn.call(mc, a) / fn.apply(mc, [a])",
                "`fn.call(mc, a)` / `fn.apply(mc, [a])` / `fn.bind(mc)` across all dialects.",
            ),
            (
                "_x, _y, _rotation, _visible",
                "x, y, rotation, visible",
                "`mc.x`, `mc.y`, `mc.rotation`, `mc.visible` (leading underscores removed in AS3 & JS/TS).",
            ),
            (
                "_xscale, _yscale (0..100%)",
                "scaleX, scaleY (0.0..1.0)",
                "`mc.scaleX = 1.0` equals `100%`! Dividing AS2 `_xscale` by 100 converts to AS3/JS scale.",
            ),
            (
                "_alpha (0..100%)",
                "alpha (0.0..1.0)",
                "`mc.alpha = 0.5` equals AS2 `_alpha = 50`.",
            ),
            (
                "_currentframe, _totalframes",
                "currentFrame, totalFrames",
                "`mc.currentFrame`, `mc.totalFrames` (1-based frame index across all languages).",
            ),
            (
                "_root / _parent / _name",
                "root / parent / name",
                "`this.root` / `this.parent` / `mc.name`.",
            ),
            (
                "mc.onRelease = fn",
                "mc.addEventListener(MouseEvent.CLICK, fn)",
                "`mc.addEventListener(\"click\", fn)` or `mc.on(\"click\", fn)`.",
            ),
            (
                "mc.onEnterFrame = fn",
                "mc.addEventListener(Event.ENTER_FRAME, fn)",
                "`mc.addEventListener(\"enterFrame\", fn)`.",
            ),
            (
                "attachMovie(\"LinkID\", \"n\", d)",
                "var c = new LinkClass(); addChild(c);",
                "Instantiate the exported symbol class and call `this.addChild(c)`.",
            ),
            (
                "removeMovieClip(mc)",
                "mc.parent.removeChild(mc)",
                "`mc.parent.removeChild(mc)`.",
            ),
            (
                "getURL(url, \"_blank\")",
                "navigateToURL(new URLRequest(url), \"_blank\")",
                "`window.open(url, \"_blank\")`.",
            ),
            (
                "getTimer()",
                "flash.utils.getTimer()",
                "`performance.now()` (milliseconds elapsed since movie start).",
            ),
        ],
    );

    mode_card(
        ui,
        Some(i16::WARNING),
        "Top 5 Migration Pitfalls When Porting Legacy AS2 FLA Projects to AS3 or JS/TS",
        "Checklist",
        BadgeType::Warning,
        "⌥F2",
        &[
            "1) Percentage vs Unit Ranges: AS2 `_alpha = 50` and `_xscale = 200` become `alpha = 0.5` and `scaleX = 2.0` in AS3 and JS/TS. Setting `alpha = 50` in AS3/JS clamps to `1.0` (fully opaque)!",
            "2) Depth Numbers vs Contiguous Display List Indices: AS2 allows sparse depths (`swapDepths(9999)`), whereas AS3/JS `addChildAt(child, index)` requires `0 <= index <= numChildren` (out-of-bounds indices throw `RangeError`).",
            "3) Button Instance Scope (`this`): In an AS1/AS2 Button `on(release)` script, `this` refers to the parent MovieClip timeline; in an AS3 or JS/TS `click` listener method on a class, `this` is the class instance (or arrow-function lexical receiver).",
            "4) Drawing Target Separation: In AS2, you draw directly on a MovieClip (`mc.beginFill(...)`); in AS3, drawing commands live on `.graphics` (`mc.graphics.beginFill(...)`); in HTML5 Canvas 2D, use `CanvasRenderingContext2D` (`ctx.beginPath(); ctx.fill();`).",
            "5) Strict Null & Undefined Checks: In AS2, calling a method on an `undefined` target path (`_root.missing_mc.play()`) silently does nothing; in AS3 and JS/TS, calling a method on `null`/`undefined` throws `TypeError` unless guarded with optional chaining (`?.`).",
        ],
    );

    if let (Some(as2_api), Some(as3_api)) = (
        neoflash_script::language_service::api(neoflash_core::ScriptLanguage::ActionScript2),
        neoflash_script::language_service::api(neoflash_core::ScriptLanguage::ActionScript3),
    ) {
        let summary_title = format!(
            "Built-In Scripting Catalog Summary — AS1/AS2 ({} Classes · {} Members), AS3 ({} Classes · {} Members) & JS/TS ({} Entries)",
            as2_api.classes.len(),
            as2_api.members.len(),
            as3_api.classes.len(),
            as3_api.members.len(),
            JS_TS_API_ENTRIES.len(),
        );
        mode_card(
            ui,
            Some(i16::CODE),
            &summary_title,
            "Live API Index",
            BadgeType::Accent,
            "⌃Space",
            &[
                "All 25 Classic AS1/AS2 Variable Suffixes for Automatic Code Hints: `_mc` (MovieClip), `_txt` (TextField), `_btn` (Button), `_array` (Array), `_str` (String), `_sound` (Sound), `_xml` (XML), `_xmlnode` (XMLNode), `_xmlsocket` (XMLSocket), `_so` (SharedObject), `_nc` (NetConnection), `_ns` (NetStream), `_video` (Video), `_fmt` (TextFormat), `_lv` (LoadVars), `_lc` (LocalConnection), `_color` (Color), `_date` (Date), `_err` (Error), `_cam` (Camera), `_mic` (Microphone), `_mcl` (MovieClipLoader), `_pj` (PrintJob), `_cm` (ContextMenu), `_cmi` (ContextMenuItem).",
                "Comment Type Directives: Place `// MovieClip hero;` or `/* flash.display.Sprite enemy; */` in frame scripts to activate full member completion on untyped variables.",
                "Actions Toolbox & Code Hints (F9): The Actions panel left toolbox and code-hint completion engine query the audited AS2/AS3 declaration catalog, the curated JS/TS/Web API catalog, and your project's `.as` / `.js` / `.ts` source files in real time.",
            ],
        );
    }

    sub_bullet(
        ui,
        "Switch Document Script Language",
        Some("⌘J"),
        "Open Document ▸ Document Settings… or the Actions panel header selector to choose ActionScript 3.0, ActionScript 2.0, JavaScript, or TypeScript.",
    );

    super::help_manual::inline_cross_links(
        ui,
        nav_target,
        &[
            (9, "10. Scripting, Import & Publish"),
            (10, "11. AS1 & AS2 Fundamentals & Timeline"),
            (12, "13. AS3 Architecture & Display List"),
            (16, "17. JavaScript (ES2023+ & NeoFlash Runtime)"),
            (17, "18. TypeScript & Built-In Type Checker"),
            (
                18,
                "19. Browser & Web APIs (Canvas / DOM / Fetch / Audio / Storage)",
            ),
        ],
    );
}

// ── Chapter 17: JavaScript (ES2023+ & NeoFlash Runtime) ──

pub(crate) fn chapter_javascript_runtime(
    ui: &mut Ui,
    diagram_tab: &mut usize,
    nav_target: &mut Option<usize>,
) {
    paragraph(
        ui,
        "JavaScript in NeoFlash combines modern ECMAScript (ES2023+) with deterministic bindings to the NeoFlash Timeline and Display List. You can write concise frame scripts on timeline keyframes, attach object scripts to Stage instances, or structure multi-file project modules (.js / .jsx) using standard relative ES module imports.",
    );

    let tabs = [
        (0, "Frame, Object & Module Contexts (this / event)"),
        (1, "Project Module Graph & JSX (__nfJsx) Lowering"),
    ];
    c::Segmented::show(ui, diagram_tab, &tabs, false);
    ui.add_space(space::s2());

    if *diagram_tab == 0 {
        illustration_canvas(
            ui,
            168.0,
            "Illustration 17.1: JavaScript Execution Contexts in NeoFlash: (1) Frame Script (`this` = owning MovieClip timeline), (2) Object / Event Script (`this` = instance, `event` = NeoFlashEvent), and (3) Project Module (`import` / `export` isolated ESM scope).",
            |painter, ctx, rect| {
                let p = neoflash_ui::palette(ctx);
                let pad = 12.0;
                let w3 = (rect.width() - pad * 4.0) / 3.0;
                let cards = [
                    (
                        "1. Frame Script Context",
                        "NeoFlash binding",
                        "this -> owning MovieClip",
                        "play(); stop(); gotoAndPlay(2);",
                        p.accent_default,
                    ),
                    (
                        "2. Object / Event Context",
                        "NeoFlash binding",
                        "this -> target DisplayObject",
                        "event.type · event.stageX/Y",
                        p.warning_default,
                    ),
                    (
                        "3. Project Module (.js/.jsx)",
                        "Project module",
                        "Top-level module scope",
                        "import { step } from \"./ai\";",
                        p.success_default,
                    ),
                ];
                for (i, (title, prov, line1, line2, accent)) in cards.iter().enumerate() {
                    let x = rect.left() + pad + i as f32 * (w3 + pad);
                    let card =
                        Rect::from_min_size(egui::pos2(x, rect.top() + 12.0), vec2(w3, 102.0));
                    painter.rect_filled(card, 5.0, p.bg_panel);
                    painter.rect_stroke(
                        card,
                        5.0,
                        Stroke::new(1.4_f32, *accent),
                        StrokeKind::Inside,
                    );
                    let inner = card.shrink(6.0);
                    paint_clipped_text(
                        painter,
                        ctx,
                        inner,
                        egui::pos2(inner.left(), card.top() + 16.0),
                        egui::Align2::LEFT_CENTER,
                        title,
                        TextStyle::BodyStrong,
                        *accent,
                    );
                    paint_clipped_text(
                        painter,
                        ctx,
                        inner,
                        egui::pos2(inner.left(), card.top() + 36.0),
                        egui::Align2::LEFT_CENTER,
                        prov,
                        TextStyle::SmallStrong,
                        p.text_secondary,
                    );
                    paint_clipped_text(
                        painter,
                        ctx,
                        inner,
                        egui::pos2(inner.left(), card.top() + 58.0),
                        egui::Align2::LEFT_CENTER,
                        line1,
                        TextStyle::MonoSmall,
                        p.text_primary,
                    );
                    paint_clipped_text(
                        painter,
                        ctx,
                        inner,
                        egui::pos2(inner.left(), card.top() + 80.0),
                        egui::Align2::LEFT_CENTER,
                        line2,
                        TextStyle::MonoSmall,
                        p.syntax_function,
                    );
                }
                let footer = Rect::from_min_max(
                    egui::pos2(rect.left() + pad, rect.bottom() - 36.0),
                    egui::pos2(rect.right() - pad, rect.bottom() - 8.0),
                );
                painter.rect_filled(footer, 4.0, p.accent_subtle);
                paint_clipped_text(
                    painter,
                    ctx,
                    footer.shrink(6.0),
                    footer.center(),
                    egui::Align2::CENTER_CENTER,
                    "Provenance Tags: [NeoFlash binding] · [ECMAScript] · [Project module] · [Browser host]",
                    TextStyle::MonoSmall,
                    p.text_primary,
                );
            },
        );
    } else {
        illustration_canvas(
            ui,
            168.0,
            "Illustration 17.2: Deterministic Project Module Graph (`crates/neoflash-script/src/modules.rs`) and JSX/TSX Lowering (`<Hud score={s}/>` → `__nfJsx(\"Hud\", { score: s })`).",
            |painter, ctx, rect| {
                let p = neoflash_ui::palette(ctx);
                let pad = 12.0;
                let w3 = (rect.width() - pad * 4.0) / 3.0;
                let stages = [
                    (
                        "1. Authored Sources",
                        "./math.js · ./hud.jsx · frame1.js",
                        "Relative ESM imports (./, ../)",
                    ),
                    (
                        "2. Module Linker & JSX",
                        "Topological sort + cycle check",
                        "<Card x={10}/> -> __nfJsx(...)",
                    ),
                    (
                        "3. Deterministic Bundle",
                        "Single source-mapped program",
                        "Runs in Preview & HTML5 WASM",
                    ),
                ];
                for (i, (head, b1, b2)) in stages.iter().enumerate() {
                    let x = rect.left() + pad + i as f32 * (w3 + pad);
                    let box_r =
                        Rect::from_min_size(egui::pos2(x, rect.top() + 26.0), vec2(w3, 96.0));
                    painter.rect_filled(box_r, 5.0, p.bg_panel);
                    painter.rect_stroke(
                        box_r,
                        5.0,
                        Stroke::new(1.3_f32, p.accent_default),
                        StrokeKind::Inside,
                    );
                    let inner = box_r.shrink(6.0);
                    paint_clipped_text(
                        painter,
                        ctx,
                        inner,
                        egui::pos2(inner.left(), box_r.top() + 20.0),
                        egui::Align2::LEFT_CENTER,
                        head,
                        TextStyle::BodyStrong,
                        p.accent_default,
                    );
                    paint_clipped_text(
                        painter,
                        ctx,
                        inner,
                        egui::pos2(inner.left(), box_r.top() + 48.0),
                        egui::Align2::LEFT_CENTER,
                        b1,
                        TextStyle::MonoSmall,
                        p.text_primary,
                    );
                    paint_clipped_text(
                        painter,
                        ctx,
                        inner,
                        egui::pos2(inner.left(), box_r.top() + 72.0),
                        egui::Align2::LEFT_CENTER,
                        b2,
                        TextStyle::Small,
                        p.text_secondary,
                    );
                    if i + 1 < stages.len() {
                        painter.line_segment(
                            [
                                egui::pos2(box_r.right() + 2.0, box_r.center().y),
                                egui::pos2(box_r.right() + pad - 2.0, box_r.center().y),
                            ],
                            Stroke::new(1.5_f32, p.accent_default),
                        );
                    }
                }
            },
        );
    }

    section_overline(ui, "JavaScript Runtime Bindings & Provenance Reference");
    api_signature_table(
        ui,
        "NeoFlash JavaScript Bindings, Modules & ES2023+ Intrinsics",
        [
            "Binding / Syntax",
            "Provenance & Target",
            "Behavior & Scope",
        ],
        &[
            (
                "this",
                "[NeoFlash binding] MovieClip | DisplayObject",
                "In a frame script, `this` is the timeline's MovieClip (`this.x`, `this.currentFrame`, `this.hero_mc`). In an object script, `this` is the attached instance.",
            ),
            (
                "event",
                "[NeoFlash binding] NeoFlashEvent",
                "Passed to object scripts and `addEventListener(type, (event) => ...)` callbacks (`event.type`, `event.target`, `event.currentTarget`, `event.stageX`, `event.stageY`, `event.keyCode`).",
            ),
            (
                "trace(...values)",
                "[NeoFlash binding] Output (F2) & console",
                "Logs formatted values to the authoring Output panel (`F2`) during Test Movie and forwards to `console.log` in published HTML5 bundles.",
            ),
            (
                "play / stop / gotoAndPlay / gotoAndStop",
                "[NeoFlash binding] Timeline & MovieClip",
                "Available both as top-level frame helpers (`gotoAndPlay(\"start\")`) and as methods on any MovieClip (`this.panel_mc.gotoAndStop(3)`). Frame numbers are 1-based.",
            ),
            (
                "import / export",
                "[Project module] Relative ESM (./, ../, /)",
                "Resolves `.js`, `.jsx`, `.ts`, `.tsx`, and `index.*` files inside the project's virtual module graph. Bare `npm` specifiers (`import \"lodash\"`) and Node `require()` are rejected with actionable diagnostics.",
            ),
            (
                "<Tag prop={v}>...</Tag>",
                "[NeoFlash binding] __nfJsx(tag, props, ...children)",
                "JSX and TSX elements compile directly to `__nfJsx(...)` calls without needing external Babel/Vite toolchains.",
            ),
            (
                "Promise / async / await",
                "[ECMAScript] ES2023+ Async & Microtasks",
                "Full support for `new Promise(...)`, `Promise.all(...)`, `async function`, `await`, `try / catch / finally`, and structured `Error` stack traces.",
            ),
            (
                "Map / Set / JSON / Math",
                "[ECMAScript] ES2023+ Standard Library",
                "Includes ES2023 classes, `#private` fields, optional chaining (`?.`), nullish coalescing (`??`), destructuring, spread (`...`), `Map`, `Set`, `JSON`, and `Math`.",
            ),
        ],
    );

    mode_card(
        ui,
        Some(i16::CODE),
        "Provenance & Sandbox Boundaries — What JavaScript in NeoFlash Is (and Is Not)",
        "Explicit Boundary",
        BadgeType::Warning,
        "No Node / npm",
        &[
            "[NeoFlash binding]: `this`, `event`, `trace`, `play`, `stop`, `gotoAndPlay`, `gotoAndStop`, `nextFrame`, `prevFrame`, `stopAllSounds`, `getChildByName`, and `__nfJsx` are provided directly by the NeoFlash runtime.",
            "[Project module]: `import` and `export` link files inside your NeoFlash project (`./utils.js`, `../shared/math.ts`). NeoFlash does not run Node.js, `npm install`, `node_modules` resolution, CommonJS `require()`, `process`, or `fs`.",
            "[ECMAScript] vs [Browser host]: Pure ECMAScript (`Promise`, `Map`, `Set`, `JSON`, `Math`) runs identically in headless validation and the browser; DOM/Web APIs (`document`, `CanvasRenderingContext2D`, `fetch`, `localStorage`, `AudioContext`) belong to the published HTML5 browser host (see Chapter 19).",
        ],
    );

    code_block(
        ui,
        &CodeSample {
            title: "JavaScript Functions — Declarations, Arrow Callbacks, ...rest, Closures & call / apply",
            lang_badge: "[JS]",
            code: JS_FUNCTIONS_CLOSURES_AND_CALL_SAMPLE,
        },
    );

    code_block(
        ui,
        &CodeSample {
            title: "JavaScript Frame Script — Timeline Control, Event Listeners, Async & JSX",
            lang_badge: "[JS]",
            code: "// Frame 1 script on 'Actions' layer (JavaScript / JSX mode)
stop();
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
        },
    );

    super::help_manual::inline_cross_links(
        ui,
        nav_target,
        &[
            (9, "10. Scripting, Import & Publish"),
            (15, "16. AS1/AS2 ↔ AS3 ↔ JS/TS Migration Guide"),
            (17, "18. TypeScript & Built-In Type Checker"),
            (
                18,
                "19. Browser & Web APIs (Canvas / DOM / Fetch / Audio / Storage)",
            ),
        ],
    );
}

// ── Chapter 18: TypeScript & Built-In Type Checker ──

pub(crate) fn chapter_typescript_checker(
    ui: &mut Ui,
    diagram_tab: &mut usize,
    nav_target: &mut Option<usize>,
) {
    paragraph(
        ui,
        "NeoFlash includes a fast, zero-dependency Rust TypeScript front-end and bounded static type checker (`crates/neoflash-script/src/ts_check.rs`). You get immediate compile-time diagnostics in the Compiler Errors panel and source-mapped JavaScript output without installing Node.js or `tsc`.",
    );

    let tabs = [
        (0, "TypeScript Pipeline (Parse → ts_check → Strip → Map)"),
        (1, "Checked Constructs vs Bounded Checker Scope"),
    ];
    c::Segmented::show(ui, diagram_tab, &tabs, false);
    ui.add_space(space::s2());

    if *diagram_tab == 0 {
        illustration_canvas(
            ui,
            168.0,
            "Illustration 18.1: NeoFlash TypeScript Compilation Pipeline: SWC AST Parse (`.ts`/`.tsx`) → Built-In `ts_check` Static Validation → Type Erasure & Enum Lowering → SourceMap Line/Column Mapping.",
            |painter, ctx, rect| {
                let p = neoflash_ui::palette(ctx);
                let pad = 10.0;
                let w4 = (rect.width() - pad * 5.0) / 4.0;
                let steps = [
                    (
                        "1. Parse (.ts/.tsx)",
                        "interface / type / enum",
                        "Generics & JSX AST",
                        p.accent_default,
                    ),
                    (
                        "2. Built-In ts_check",
                        "Assignability & members",
                        "Arity & return checks",
                        p.warning_default,
                    ),
                    (
                        "3. Strip & Lower",
                        "Erase type annotations",
                        "Lower enums & __nfJsx",
                        p.success_default,
                    ),
                    (
                        "4. SourceMap + Run",
                        "1-based line/col spans",
                        "Compiler Errors",
                        p.accent_default,
                    ),
                ];
                for (i, (title, l1, l2, col)) in steps.iter().enumerate() {
                    let x = rect.left() + pad + i as f32 * (w4 + pad);
                    let card =
                        Rect::from_min_size(egui::pos2(x, rect.top() + 24.0), vec2(w4, 98.0));
                    painter.rect_filled(card, 5.0, p.bg_panel);
                    painter.rect_stroke(card, 5.0, Stroke::new(1.3_f32, *col), StrokeKind::Inside);
                    let inner = card.shrink(6.0);
                    paint_clipped_text(
                        painter,
                        ctx,
                        inner,
                        egui::pos2(inner.left(), card.top() + 18.0),
                        egui::Align2::LEFT_CENTER,
                        title,
                        TextStyle::BodyStrong,
                        *col,
                    );
                    paint_clipped_text(
                        painter,
                        ctx,
                        inner,
                        egui::pos2(inner.left(), card.top() + 46.0),
                        egui::Align2::LEFT_CENTER,
                        l1,
                        TextStyle::MonoSmall,
                        p.text_primary,
                    );
                    paint_clipped_text(
                        painter,
                        ctx,
                        inner,
                        egui::pos2(inner.left(), card.top() + 70.0),
                        egui::Align2::LEFT_CENTER,
                        l2,
                        TextStyle::Small,
                        p.text_secondary,
                    );
                }
            },
        );
    } else {
        illustration_canvas(
            ui,
            168.0,
            "Illustration 18.2: Comparison of Verified Built-In `ts_check` Rules vs Explicit Bounded Checker Scope.",
            |painter, ctx, rect| {
                let p = neoflash_ui::palette(ctx);
                let pad = 12.0;
                let w2 = (rect.width() - pad * 3.0) / 2.0;
                let left = Rect::from_min_size(
                    egui::pos2(rect.left() + pad, rect.top() + 12.0),
                    vec2(w2, rect.height() - 24.0),
                );
                let right = Rect::from_min_size(
                    egui::pos2(left.right() + pad, rect.top() + 12.0),
                    vec2(w2, rect.height() - 24.0),
                );
                for (box_r, title, color, rows) in [
                    (
                        left,
                        "Verified by Built-In ts_check [TypeScript]",
                        p.success_default,
                        &[
                            "Primitives: number, string, boolean, null, undefined, void",
                            "Interfaces (extends, optional ?, readonly), classes & enums",
                            "Unions (A | B), intersections (A & B), arrays (T[] / Array<T>)",
                            "Generics & utilities: Promise<T>, Record, Partial, Pick, Omit",
                            "Function parameter arity, argument types & return types",
                        ][..],
                    ),
                    (
                        right,
                        "Explicit Bounded Scope (Not Full tsc)",
                        p.warning_default,
                        &[
                            "No external Node.js, tsc binary, or tsconfig.json required",
                            "No arbitrary npm @types/* or .d.ts ambient package loading",
                            "Escape hatches (`any` and `unknown`) bypass member checks",
                            "Complex recursive conditional/mapped types are erased safely",
                            "Runtime semantics remain pure standard ECMAScript",
                        ][..],
                    ),
                ] {
                    painter.rect_filled(box_r, 5.0, p.bg_panel);
                    painter.rect_stroke(
                        box_r,
                        5.0,
                        Stroke::new(1.3_f32, color),
                        StrokeKind::Inside,
                    );
                    let inner = box_r.shrink(6.0);
                    paint_clipped_text(
                        painter,
                        ctx,
                        inner,
                        egui::pos2(inner.left(), box_r.top() + 16.0),
                        egui::Align2::LEFT_CENTER,
                        title,
                        TextStyle::BodyStrong,
                        color,
                    );
                    for (j, r_txt) in rows.iter().enumerate() {
                        paint_clipped_text(
                            painter,
                            ctx,
                            inner,
                            egui::pos2(inner.left(), box_r.top() + 38.0 + j as f32 * 20.0),
                            egui::Align2::LEFT_CENTER,
                            r_txt,
                            TextStyle::MonoSmall,
                            p.text_primary,
                        );
                    }
                }
            },
        );
    }

    section_overline(ui, "TypeScript Syntax & Built-In Checker Coverage");
    api_signature_table(
        ui,
        "TypeScript Constructs, Utility Types & Diagnostics",
        [
            "Construct / Utility",
            "Provenance & Erasure",
            "Static Check Performed by ts_check",
        ],
        &[
            (
                "interface Name { ... }",
                "[TypeScript] Erased at compile time",
                "Verifies required properties, optional `?` fields, `extends` inheritance chains, and structural object literals.",
            ),
            (
                "type Alias<T> = A | B",
                "[TypeScript] Erased at compile time",
                "Resolves union (`|`), intersection (`&`), tuple (`[number, string]`), literal, and generic type aliases.",
            ),
            (
                "enum Kind { A, B = \"b\" }",
                "[TypeScript] Lowered to JS object",
                "Checks enum member access (`Kind.A`) and emits a self-contained runtime lookup object.",
            ),
            (
                "Array<T> / T[] / Promise<T>",
                "[TypeScript] Built-in generic types",
                "Checks element types on array literals/methods and unwraps `Promise<T>` across `async` returns.",
            ),
            (
                "Record<K, V> / Map<K, V> / Set<T>",
                "[TypeScript] Built-in generic utilities",
                "Validates dictionary value types and collection type parameters.",
            ),
            (
                "Partial<T> / Required<T> / Readonly<T>",
                "[TypeScript] Built-in mapped utilities",
                "Transforms property optionality on object/interface types during assignability checks.",
            ),
            (
                "Pick<T, K> / Omit<T, K>",
                "[TypeScript] Built-in projection utilities",
                "Filters interface `T` to only the keys in string-literal union `K` (or excludes `K`).",
            ),
            (
                "as Type / <Type>expr / x!: T",
                "[TypeScript] Erased assertion syntax",
                "Type assertions (`as`), non-null assertions (`!`), and parameter properties (`constructor(public x: number)`) compile cleanly.",
            ),
        ],
    );

    mode_card(
        ui,
        Some(i16::WARNING),
        "Source Maps, Compiler Errors & Bounded Checker Honesty",
        "Diagnostics",
        BadgeType::Accent,
        "⌥F2",
        &[
            "Exact Source Spans & Source Maps: Every TypeScript syntax or type error reports its 1-based line and column in the Compiler Errors panel. Double-clicking a diagnostic jumps the Actions editor directly to that line.",
            "Bounded Checker Honesty: `ts_check` is purpose-built for authoring scripts, interfaces, classes, unions, and utility types inside NeoFlash. It intentionally does not claim to be a full external `tsc` compiler and does not read `tsconfig.json` or `node_modules/@types`.",
        ],
    );

    code_block(
        ui,
        &CodeSample {
            title: "TypeScript Functions — Typed Signatures, Function Aliases, Generics & Closures",
            lang_badge: "[TS]",
            code: TS_TYPED_AND_GENERIC_FUNCTIONS_SAMPLE,
        },
    );

    code_block(
        ui,
        &CodeSample {
            title: "TypeScript Script — Interfaces, Discriminated Unions, Generics & Utility Types",
            lang_badge: "[TS]",
            code: "interface Actor {
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
        },
    );

    super::help_manual::inline_cross_links(
        ui,
        nav_target,
        &[
            (9, "10. Scripting, Import & Publish"),
            (15, "16. AS1/AS2 ↔ AS3 ↔ JS/TS Migration Guide"),
            (16, "17. JavaScript (ES2023+ & NeoFlash Runtime)"),
            (
                18,
                "19. Browser & Web APIs (Canvas / DOM / Fetch / Audio / Storage)",
            ),
        ],
    );
}

// ── Chapter 19: Browser & Web APIs (Canvas / DOM / Fetch / Audio / Storage) ──

pub(crate) fn chapter_browser_web_apis(
    ui: &mut Ui,
    diagram_tab: &mut usize,
    nav_target: &mut Option<usize>,
) {
    paragraph(
        ui,
        "When published to HTML5 (`File ▸ Publish`), NeoFlash bundles run inside a standard web browser alongside the WebAssembly runtime (`neoflash_web_bg.wasm`), canvas host (`player.js`), and Web Audio scheduler (`audio.js`). JavaScript and TypeScript scripts can interact with browser Web APIs subject to standard browser security, CORS, CSP, and autoplay policies.",
    );

    let tabs = [
        (0, "HTML5 Host Architecture (WASM + Canvas 2D + Web Audio)"),
        (
            1,
            "Browser Security Boundaries (CORS / CSP / Autoplay / Storage)",
        ),
    ];
    c::Segmented::show(ui, diagram_tab, &tabs, false);
    ui.add_space(space::s2());

    if *diagram_tab == 0 {
        illustration_canvas(
            ui,
            168.0,
            "Illustration 19.1: Published HTML5 Runtime Architecture: NeoFlash WASM Player + JS/TS Scripts bridging to DOM/CanvasRenderingContext2D, Web Audio (`AudioContext`), `fetch()`, and `localStorage`.",
            |painter, ctx, rect| {
                let p = neoflash_ui::palette(ctx);
                let pad = 12.0;
                let w3 = (rect.width() - pad * 4.0) / 3.0;
                let cols = [
                    (
                        "1. DOM & Canvas 2D",
                        "[Browser host]",
                        "document.querySelector(...)",
                        "CanvasRenderingContext2D",
                        p.accent_default,
                    ),
                    (
                        "2. Timers & Input",
                        "[Browser host]",
                        "requestAnimationFrame(fn)",
                        "PointerEvent / KeyboardEvent",
                        p.warning_default,
                    ),
                    (
                        "3. Fetch, Audio & Storage",
                        "[Browser host]",
                        "fetch(url) · localStorage",
                        "AudioContext · HTMLVideoElement",
                        p.success_default,
                    ),
                ];
                for (i, (title, prov, l1, l2, col)) in cols.iter().enumerate() {
                    let x = rect.left() + pad + i as f32 * (w3 + pad);
                    let card =
                        Rect::from_min_size(egui::pos2(x, rect.top() + 12.0), vec2(w3, 102.0));
                    painter.rect_filled(card, 5.0, p.bg_panel);
                    painter.rect_stroke(card, 5.0, Stroke::new(1.3_f32, *col), StrokeKind::Inside);
                    let inner = card.shrink(6.0);
                    paint_clipped_text(
                        painter,
                        ctx,
                        inner,
                        egui::pos2(inner.left(), card.top() + 16.0),
                        egui::Align2::LEFT_CENTER,
                        title,
                        TextStyle::BodyStrong,
                        *col,
                    );
                    paint_clipped_text(
                        painter,
                        ctx,
                        inner,
                        egui::pos2(inner.left(), card.top() + 36.0),
                        egui::Align2::LEFT_CENTER,
                        prov,
                        TextStyle::SmallStrong,
                        p.text_secondary,
                    );
                    paint_clipped_text(
                        painter,
                        ctx,
                        inner,
                        egui::pos2(inner.left(), card.top() + 58.0),
                        egui::Align2::LEFT_CENTER,
                        l1,
                        TextStyle::MonoSmall,
                        p.text_primary,
                    );
                    paint_clipped_text(
                        painter,
                        ctx,
                        inner,
                        egui::pos2(inner.left(), card.top() + 80.0),
                        egui::Align2::LEFT_CENTER,
                        l2,
                        TextStyle::MonoSmall,
                        p.syntax_function,
                    );
                }
                let bar = Rect::from_min_max(
                    egui::pos2(rect.left() + pad, rect.bottom() - 36.0),
                    egui::pos2(rect.right() - pad, rect.bottom() - 8.0),
                );
                painter.rect_filled(bar, 4.0, p.accent_subtle);
                paint_clipped_text(
                    painter,
                    ctx,
                    bar.shrink(6.0),
                    bar.center(),
                    egui::Align2::CENTER_CENTER,
                    "HTML5 Bundle: index.html + player.js + audio.js + neoflash_web_bg.wasm + project.json",
                    TextStyle::MonoSmall,
                    p.text_primary,
                );
            },
        );
    } else {
        illustration_canvas(
            ui,
            168.0,
            "Illustration 19.2: Browser Host Security & Policy Boundaries: CORS & CSP for `fetch`/Canvas images, User-Gesture Autoplay for Web Audio/Video, and Same-Origin Quota for `localStorage`.",
            |painter, ctx, rect| {
                let p = neoflash_ui::palette(ctx);
                let pad = 10.0;
                let cols = 2;
                let rows = 2;
                let cw = (rect.width() - pad * 3.0) / cols as f32;
                let ch = (rect.height() - pad * 3.0) / rows as f32;
                let rules = [
                    (
                        "1. CORS & Tainted Canvas",
                        "Cross-origin fetch/drawImage requires Access-Control-Allow-Origin",
                    ),
                    (
                        "2. Content-Security-Policy (CSP)",
                        "connect-src governs fetch(); script-src 'wasm-unsafe-eval' for WASM",
                    ),
                    (
                        "3. Audio/Video User Gesture",
                        "AudioContext.resume() & video.play() require a click/key gesture",
                    ),
                    (
                        "4. Origin Storage Sandbox",
                        "localStorage is ~5 MB per origin; no direct disk (fs) access",
                    ),
                ];
                for (idx, (head, body)) in rules.iter().enumerate() {
                    let r_i = idx / cols;
                    let c_i = idx % cols;
                    let x = rect.left() + pad + c_i as f32 * (cw + pad);
                    let y = rect.top() + pad + r_i as f32 * (ch + pad);
                    let cell = Rect::from_min_size(egui::pos2(x, y), vec2(cw, ch));
                    painter.rect_filled(cell, 5.0, p.bg_panel);
                    painter.rect_stroke(
                        cell,
                        5.0,
                        Stroke::new(1.2_f32, p.warning_default),
                        StrokeKind::Inside,
                    );
                    let inner = cell.shrink(6.0);
                    paint_clipped_text(
                        painter,
                        ctx,
                        inner,
                        egui::pos2(inner.left(), cell.center().y - 9.0),
                        egui::Align2::LEFT_CENTER,
                        head,
                        TextStyle::BodyStrong,
                        p.warning_default,
                    );
                    paint_clipped_text(
                        painter,
                        ctx,
                        inner,
                        egui::pos2(inner.left(), cell.center().y + 10.0),
                        egui::Align2::LEFT_CENTER,
                        body,
                        TextStyle::Small,
                        p.text_primary,
                    );
                }
            },
        );
    }

    section_overline(ui, "Browser & Web API Reference (HTML5 Host)");
    api_signature_table(
        ui,
        "DOM, CanvasRenderingContext2D, Fetch, Web Audio, Timers & Storage",
        [
            "Web API / Interface",
            "Provenance & Signature",
            "Availability & Browser Boundary",
        ],
        &[
            (
                "CanvasRenderingContext2D",
                "[Browser host] canvas.getContext(\"2d\")",
                "`fillRect`, `strokeRect`, `clearRect`, `beginPath`, `moveTo`, `lineTo`, `quadraticCurveTo`, `bezierCurveTo`, `arc`, `fill`, `stroke`, `drawImage`, `save`, `restore`, `setTransform`. Cross-origin images without `crossOrigin = \"anonymous\"` taint `getImageData`.",
            ),
            (
                "document & DOM Elements",
                "[Browser host] document.querySelector(sel)",
                "`getElementById`, `querySelector`, `createElement`, `classList`, `style`, and `getBoundingClientRect()`. Available in published HTML5 pages; guard with `typeof document !== \"undefined\"` if sharing code with headless tests.",
            ),
            (
                "fetch & Response",
                "[Browser host] await fetch(url, init)",
                "Asynchronous HTTP/HTTPS requests returning `Promise<Response>` (`.json()`, `.text()`, `.arrayBuffer()`). Must satisfy browser CORS headers and `Content-Security-Policy: connect-src`.",
            ),
            (
                "localStorage & sessionStorage",
                "[Browser host] localStorage.getItem / setItem",
                "Synchronous string key-value storage scoped to the page origin (~5 MB limit). Wrap in `try { ... } catch` because strict browser privacy modes or `file://` origins may block storage.",
            ),
            (
                "setTimeout / requestAnimationFrame",
                "[Browser host] Timer & Frame Callbacks",
                "`setTimeout(fn, ms)`, `setInterval(fn, ms)`, and `requestAnimationFrame((nowMs) => ...)` schedule async callbacks. Background browser tabs throttle timers and pause `requestAnimationFrame`.",
            ),
            (
                "AudioContext & HTMLMediaElement",
                "[Browser host] Web Audio & <audio>/<video>",
                "Modern browsers block unmuted audio/video autoplay until the user interacts with the page (`pointerdown`, `click`, or `keydown`). Call `audioCtx.resume()` inside a user-gesture handler.",
            ),
            (
                "Pointer, Mouse & Keyboard Events",
                "[Browser host] window / canvas EventTarget",
                "`pointerdown`, `pointermove`, `pointerup`, `wheel`, `keydown`, `keyup`, and `resize` map cleanly to Stage coordinates via `canvas.getBoundingClientRect()`.",
            ),
        ],
    );

    mode_card(
        ui,
        Some(i16::WARNING),
        "Browser Sandbox Rules — CORS, CSP, User Gestures & Headless Guarding",
        "Web Boundaries",
        BadgeType::Warning,
        "HTML5 Publish",
        &[
            "Serve Over HTTP/HTTPS (Not file://): Opening a published `index.html` via `file://` blocks `fetch(\"./project.json\")` and WebAssembly streaming in most browsers. Use a local HTTP server or NeoFlash's Test Movie / Open in Browser command.",
            "User Gesture Required for Audio & Fullscreen: Always trigger `audioCtx.resume()`, `video.play()`, or `element.requestFullscreen()` inside a click or key press callback.",
            "Guarding Browser-Only Globals: When writing shared utility modules imported by both frame scripts and browser overlays, check `if (typeof window !== \"undefined\")` before touching `window`, `document`, or `localStorage`.",
        ],
    );

    code_block(
        ui,
        &CodeSample {
            title: "TypeScript Browser Host — CanvasRenderingContext2D, Fetch JSON & LocalStorage",
            lang_badge: "[TS] [JS]",
            code: "interface HighScoreEntry {
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
        },
    );

    super::help_manual::inline_cross_links(
        ui,
        nav_target,
        &[
            (9, "10. Scripting, Import & Publish"),
            (15, "16. AS1/AS2 ↔ AS3 ↔ JS/TS Migration Guide"),
            (16, "17. JavaScript (ES2023+ & NeoFlash Runtime)"),
            (17, "18. TypeScript & Built-In Type Checker"),
        ],
    );
}

// ── Scripting Reference: API browser model ──
//
// One view model for the three reference languages. ActionScript 2 and 3 come from the generated
// declaration index (`script_api`), JavaScript / TypeScript from `JS_TS_API_ENTRIES`.

/// Pseudo-class of the top-level functions, properties and constants.
pub(crate) const API_GLOBAL_KEY: &str = "<global>";
/// Pseudo-class of operators, statements and directives.
pub(crate) const API_LANGUAGE_KEY: &str = "<language>";

/// Support status of a declaration in NeoFlash (`docs/progress/actionscript-catalog.json`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Support {
    Working,
    Partial,
    Missing,
}

impl Support {
    fn of(status: &str) -> Option<Self> {
        match status {
            "subset" => Some(Self::Working),
            "partial" => Some(Self::Partial),
            "missing" => Some(Self::Missing),
            _ => None,
        }
    }
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Working => "Working",
            Self::Partial => "Partial",
            Self::Missing => "Missing",
        }
    }
    fn color(self, p: &Palette) -> egui::Color32 {
        match self {
            Self::Working => p.success_default,
            Self::Partial => p.warning_default,
            Self::Missing => p.text_disabled,
        }
    }
}

/// Member filter of the class column.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum MemberFilter {
    #[default]
    All,
    Properties,
    Methods,
    Events,
}

/// Group a member is listed under.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum MemberGroup {
    Properties,
    Methods,
    Events,
    Language,
}

impl MemberGroup {
    const ALL: [Self; 4] = [
        Self::Properties,
        Self::Methods,
        Self::Events,
        Self::Language,
    ];
    const fn label(self) -> &'static str {
        match self {
            Self::Properties => "Properties",
            Self::Methods => "Methods",
            Self::Events => "Events",
            Self::Language => "Language elements",
        }
    }
    fn passes(self, filter: MemberFilter) -> bool {
        match filter {
            MemberFilter::All => true,
            MemberFilter::Properties => self == Self::Properties,
            MemberFilter::Methods => self == Self::Methods,
            MemberFilter::Events => self == Self::Events,
        }
    }
}

/// Link from a member to another declaration of the same language.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ApiLink {
    pub label: String,
    pub class_key: String,
    pub member_key: Option<String>,
}

/// One member of a class (or one JavaScript / TypeScript entry).
#[derive(Clone, Debug)]
pub(crate) struct ApiMember {
    /// Unique inside its class.
    pub key: String,
    pub name: String,
    /// Name as written in code: `play()`, `currentFrame`.
    pub label: String,
    /// `method`, `static constant`, `NeoFlash binding`, …
    pub kind: String,
    pub group: MemberGroup,
    /// Text after the name in the member list.
    pub row_signature: String,
    /// Declaration shown in the detail code box, wrapped by parameters.
    pub declaration: String,
    /// `(name, type and default)`.
    pub params: Vec<(String, String)>,
    pub returns: String,
    /// `None` when the catalogue has no status for the declaration.
    pub support: Option<Support>,
    /// Catalogue note (partial) or availability (JavaScript / TypeScript).
    pub note: String,
    pub description: String,
    pub example: String,
    pub insertion: CodeCommand,
    pub see_also: Vec<ApiLink>,
}

/// One class, interface, pseudo-class or JavaScript / TypeScript category.
#[derive(Clone, Debug)]
pub(crate) struct ApiClass {
    pub key: String,
    pub name: String,
    /// `flash.display`, `Top level`, `JavaScript / TypeScript`.
    pub package: String,
    /// Superclass chain, nearest first, with the key of every indexed parent.
    pub chain: Vec<(String, Option<String>)>,
    /// `subset`, `partial`, `members`, or empty when the catalogue has no class status.
    pub status: String,
    pub description: String,
    /// What Insert at cursor writes when no member is selected.
    pub insertion: Option<CodeCommand>,
    pub members: Vec<ApiMember>,
}

impl ApiClass {
    pub(crate) fn member(&self, key: &str) -> Option<&ApiMember> {
        self.members.iter().find(|m| m.key == key)
    }
    /// `(working, partial, missing)` among the members that carry a status.
    pub(crate) fn support_counts(&self) -> (usize, usize, usize) {
        let count = |s| self.members.iter().filter(|m| m.support == Some(s)).count();
        (
            count(Support::Working),
            count(Support::Partial),
            count(Support::Missing),
        )
    }
}

pub(crate) struct ApiPackage {
    pub label: String,
    /// `(class key, name)`.
    pub classes: Vec<(String, String)>,
}

pub(crate) struct ApiIndex {
    /// Packages in tree order. `flat` trees (JavaScript / TypeScript) have one unnamed package.
    pub packages: Vec<ApiPackage>,
    pub flat: bool,
    pub classes: HashMap<String, Arc<ApiClass>>,
    pub class_count: usize,
    pub member_count: usize,
    /// Members with a catalogue status, for the coverage report.
    #[cfg_attr(not(test), allow(dead_code))]
    pub status_count: usize,
}

impl ApiIndex {
    pub(crate) fn class(&self, key: &str) -> Option<&Arc<ApiClass>> {
        self.classes.get(key)
    }
    /// Class the browser opens on: `MovieClip` where the language has one.
    pub(crate) fn default_class(&self) -> Option<&str> {
        ["flash.display.MovieClip", "MovieClip"]
            .into_iter()
            .find(|key| self.classes.contains_key(*key))
            .or_else(|| {
                self.packages
                    .iter()
                    .flat_map(|p| &p.classes)
                    .map(|(key, _)| key.as_str())
                    .next()
            })
    }
    pub(crate) fn package_of(&self, class_key: &str) -> Option<&str> {
        self.packages
            .iter()
            .find(|p| p.classes.iter().any(|(key, _)| key == class_key))
            .map(|p| p.label.as_str())
    }
}

/// The language whose declaration index backs `scope`; `None` for JavaScript / TypeScript.
pub(crate) fn scope_script_language(scope: LanguageScope) -> Option<ScriptLanguage> {
    match scope {
        LanguageScope::As1As2 => Some(ScriptLanguage::ActionScript2),
        LanguageScope::As3 => Some(ScriptLanguage::ActionScript3),
        LanguageScope::JsTs | LanguageScope::All => None,
    }
}

/// API index of a reference language. `All` is not a reference language and reads as JavaScript.
pub(crate) fn api_index(scope: LanguageScope) -> &'static ApiIndex {
    static AS2: OnceLock<ApiIndex> = OnceLock::new();
    static AS3: OnceLock<ApiIndex> = OnceLock::new();
    static JS_TS: OnceLock<ApiIndex> = OnceLock::new();
    match scope_script_language(scope) {
        Some(language) => {
            let cell = if language == ScriptLanguage::ActionScript3 {
                &AS3
            } else {
                &AS2
            };
            cell.get_or_init(|| {
                script_api::api(language)
                    .map(|api| build_as_index(api, language))
                    .unwrap_or_else(build_js_ts_index)
            })
        }
        None => JS_TS.get_or_init(build_js_ts_index),
    }
}

/// `flash.filters.flash.filters.BlurFilter` (an ActionScript 2 reference quirk) → `flash.filters.BlurFilter`.
fn canonical_class_key(raw: &str) -> String {
    let package = script_api::package_of(raw);
    let parts: Vec<&str> = package.split('.').collect();
    if parts.len() >= 2 && parts.len().is_multiple_of(2) {
        let half = parts.len() / 2;
        if parts[..half] == parts[half..] {
            return format!(
                "{}.{}",
                parts[..half].join("."),
                script_api::short_name(raw)
            );
        }
    }
    raw.to_owned()
}

fn is_language_kind(kind: &str) -> bool {
    matches!(
        kind,
        "operator" | "statement" | "directive" | "keyword" | "special_type" | "compiler_directive"
    )
}

fn member_group(kind: &str) -> MemberGroup {
    match kind {
        "property" | "constant" => MemberGroup::Properties,
        "method" | "function" | "constructor" => MemberGroup::Methods,
        "event" | "callback" => MemberGroup::Events,
        _ => MemberGroup::Language,
    }
}

/// Splits a documented parameter list at its top-level commas.
fn split_params(params: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0_i32;
    let mut quote: Option<char> = None;
    let mut current = String::new();
    for ch in params.chars() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(ch),
            (None, '(' | '[' | '<' | '{') => depth += 1,
            (None, ')' | ']' | '>' | '}') => depth -= 1,
            (None, ',') if depth <= 0 => {
                parts.push(current.trim().to_owned());
                current.clear();
                continue;
            }
            _ => {}
        }
        current.push(ch);
    }
    if !current.trim().is_empty() {
        parts.push(current.trim().to_owned());
    }
    parts
}

/// `(name, description)` of one documented parameter: `scene:String = null` →
/// `("scene", "String, optional (default null)")`.
fn describe_param(raw: &str) -> (String, String) {
    let mut text = raw.trim();
    let bracketed = text.starts_with('[') && text.ends_with(']');
    if bracketed {
        text = text[1..text.len() - 1].trim();
    }
    if let Some(rest) = text.strip_prefix("...") {
        let name = rest.trim().split(':').next().unwrap_or("").trim();
        let name = if name.is_empty() { "rest" } else { name };
        return (
            format!("...{name}"),
            "Any number of further arguments".into(),
        );
    }
    let (declared, default) = match text.split_once('=') {
        Some((declared, default)) => (declared.trim(), Some(default.trim())),
        None => (text, None),
    };
    let (name, ty) = match declared.split_once(':') {
        Some((name, ty)) => (name.trim(), ty.trim()),
        None => (declared, ""),
    };
    let mut description = if ty.is_empty() {
        String::new()
    } else {
        ty.to_owned()
    };
    match default {
        Some(default) => {
            if !description.is_empty() {
                description.push_str(", ");
            }
            let _ = write!(description, "optional (default {default})");
        }
        None if bracketed => {
            if !description.is_empty() {
                description.push_str(", ");
            }
            description.push_str("optional");
        }
        None => {}
    }
    (name.to_owned(), description)
}

fn lower_first(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Leading word of a camel-case name: `gotoAndPlay` → `goto`, `_xscale` → `_xscale`.
fn leading_word(name: &str) -> &str {
    let end = name
        .char_indices()
        .skip(1)
        .find(|(_, c)| c.is_uppercase())
        .map_or(name.len(), |(i, _)| i);
    &name[..end]
}

struct AsClassDraft {
    key: String,
    name: String,
    package: String,
    pseudo: bool,
    status: String,
    parents: Vec<String>,
    members: Vec<Member>,
}

fn build_as_index(api: &'static script_api::LanguageApi, language: ScriptLanguage) -> ApiIndex {
    const TOP_LEVEL: &str = "Top level";
    let mut groups: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for raw in api.classes.keys() {
        groups
            .entry(canonical_class_key(raw))
            .or_default()
            .push(raw.as_str());
    }

    let mut drafts: Vec<AsClassDraft> = Vec::new();
    // Top-level declarations: the language keeps them under an empty owner, `<language>` or
    // the `Top Level` package entry.
    let mut top_level: Vec<Member> = ["", "<language>", "Top Level"]
        .into_iter()
        .flat_map(|owner| script_api::own_members(&[], api, owner))
        .filter(|m| m.kind != "package")
        .collect();
    top_level.sort_by_key(|a| a.name.to_lowercase());
    let (language_elements, globals): (Vec<Member>, Vec<Member>) = top_level
        .into_iter()
        .partition(|m| is_language_kind(&m.kind));
    if !globals.is_empty() {
        drafts.push(AsClassDraft {
            key: API_GLOBAL_KEY.into(),
            name: "Global functions and properties".into(),
            package: TOP_LEVEL.into(),
            pseudo: true,
            status: String::new(),
            parents: Vec::new(),
            members: globals,
        });
    }
    if !language_elements.is_empty() {
        drafts.push(AsClassDraft {
            key: API_LANGUAGE_KEY.into(),
            name: "Operators and statements".into(),
            package: TOP_LEVEL.into(),
            pseudo: true,
            status: String::new(),
            parents: Vec::new(),
            members: language_elements,
        });
    }
    for (key, raws) in &groups {
        if key == "Top Level" {
            continue;
        }
        let mut members: Vec<Member> = Vec::new();
        for raw in raws {
            members.extend(script_api::own_members(&[], api, raw));
            members.extend(script_api::missing_members(api, raw).cloned());
        }
        // Prefer the audited class entry over a `members` placeholder for the same class.
        let Some(info) = raws
            .iter()
            .filter_map(|raw| api.classes.get(*raw))
            .min_by_key(|info| info.status == "members")
        else {
            continue;
        };
        // A package entry (`flash.utils`) owns its package-level functions.
        let container = info.status == "members"
            && !members.is_empty()
            && members
                .iter()
                .all(|m| matches!(m.kind.as_str(), "function" | "package"));
        members.retain(|m| m.kind != "package");
        if container {
            if members.is_empty() {
                continue;
            }
            drafts.push(AsClassDraft {
                key: key.clone(),
                name: "Package functions".into(),
                package: key.clone(),
                pseudo: true,
                status: String::new(),
                parents: Vec::new(),
                members,
            });
            continue;
        }
        let package = script_api::package_of(key);
        drafts.push(AsClassDraft {
            key: key.clone(),
            name: script_api::short_name(key).to_owned(),
            package: if package.is_empty() {
                TOP_LEVEL.into()
            } else {
                package.to_owned()
            },
            pseudo: false,
            status: info.status.clone(),
            parents: info.parents.clone(),
            members,
        });
    }

    // Short class name → keys, to link types and parents.
    let mut by_short: HashMap<String, Vec<String>> = HashMap::new();
    for draft in drafts.iter().filter(|d| !d.pseudo) {
        by_short
            .entry(draft.name.clone())
            .or_default()
            .push(draft.key.clone());
    }
    let known: std::collections::HashSet<String> = drafts.iter().map(|d| d.key.clone()).collect();
    let resolve = |name: &str, near: &str| -> Option<String> {
        let canonical = canonical_class_key(name);
        if known.contains(&canonical) {
            return Some(canonical);
        }
        let candidates = by_short.get(script_api::short_name(name))?;
        candidates
            .iter()
            .find(|key| script_api::package_of(key) == near)
            .or_else(|| candidates.iter().find(|key| !key.contains('.')))
            .or_else(|| (candidates.len() == 1).then(|| &candidates[0]))
            .cloned()
    };

    let mut packages: BTreeMap<String, Vec<(u8, String, String)>> = BTreeMap::new();
    let mut classes = HashMap::new();
    let (mut member_count, mut status_count) = (0, 0);
    for draft in drafts {
        let near = script_api::package_of(&draft.key).to_owned();
        let mut used: HashMap<String, usize> = HashMap::new();
        let mut members: Vec<ApiMember> = draft
            .members
            .iter()
            .map(|m| {
                let mut view = as_member_view(m, language, &draft);
                let seen = used.entry(view.key.clone()).or_insert(0);
                *seen += 1;
                if *seen > 1 {
                    let _ = write!(view.key, "|{seen}");
                }
                // See also: the class of the value type (`Vector.<Point>` → `Point`).
                let ty = m.ty.trim_end_matches('>').rsplit('<').next().unwrap_or("");
                if let Some(class_key) = resolve(ty, &near).filter(|key| *key != draft.key) {
                    view.see_also.push(ApiLink {
                        label: script_api::short_name(&class_key).to_owned(),
                        class_key,
                        member_key: None,
                    });
                }
                view
            })
            .collect();
        members.sort_by(|a, b| {
            (a.group, a.name.to_lowercase(), &a.key).cmp(&(b.group, b.name.to_lowercase(), &b.key))
        });
        // See also: up to four members sharing the leading word (`gotoAndPlay` → `gotoAndStop`).
        let names: Vec<(String, String, String)> = members
            .iter()
            .map(|m| (m.key.clone(), m.name.clone(), m.label.clone()))
            .collect();
        for member in &mut members {
            let word = leading_word(&member.name);
            if word.len() < 3 || word.len() == member.name.len() {
                continue;
            }
            let siblings: Vec<ApiLink> = names
                .iter()
                .filter(|(key, name, _)| *key != member.key && leading_word(name) == word)
                .take(4)
                .map(|(key, _, label)| ApiLink {
                    label: label.clone(),
                    class_key: draft.key.clone(),
                    member_key: Some(key.clone()),
                })
                .collect();
            member.see_also.splice(0..0, siblings);
        }
        member_count += members.len();
        status_count += members.iter().filter(|m| m.support.is_some()).count();
        let chain = draft
            .parents
            .iter()
            .map(|parent| {
                (
                    script_api::short_name(parent).to_owned(),
                    resolve(parent, &near),
                )
            })
            .collect();
        let description =
            if draft.pseudo {
                match draft.key.as_str() {
                API_GLOBAL_KEY => {
                    "Functions, properties and constants available in every script without a class."
                        .to_owned()
                }
                API_LANGUAGE_KEY => {
                    "Operators, statements and directives of the language.".to_owned()
                }
                _ => format!("Functions declared directly in the {} package.", draft.package),
            }
            } else {
                script_api::keyword_reference_summary(language, &draft.name)
                    .unwrap_or_default()
                    .to_owned()
            };
        let insertion = (!draft.pseudo).then(|| {
            super::actions_sidebar::toolbox_insertion(
                &Member {
                    owner: draft.key.clone(),
                    name: draft.name.clone(),
                    kind: "class".into(),
                    ty: String::new(),
                    is_static: false,
                    params: None,
                    group: None,
                    limitation: None,
                    status: String::new(),
                },
                language,
            )
        });
        let order = match draft.key.as_str() {
            API_GLOBAL_KEY => 0,
            API_LANGUAGE_KEY => 1,
            _ if draft.pseudo => 2,
            _ => 3,
        };
        packages.entry(draft.package.clone()).or_default().push((
            order,
            draft.key.clone(),
            draft.name.clone(),
        ));
        classes.insert(
            draft.key.clone(),
            Arc::new(ApiClass {
                key: draft.key,
                name: draft.name,
                package: draft.package,
                chain,
                status: draft.status,
                description,
                insertion,
                members,
            }),
        );
    }
    let mut ordered: Vec<ApiPackage> = Vec::new();
    for (label, mut entries) in packages {
        entries.sort_by_key(|a| (a.0, a.2.to_lowercase()));
        let package = ApiPackage {
            label: label.clone(),
            classes: entries
                .into_iter()
                .map(|(_, key, name)| (key, name))
                .collect(),
        };
        if label == TOP_LEVEL {
            ordered.insert(0, package);
        } else {
            ordered.push(package);
        }
    }
    ApiIndex {
        class_count: classes.len(),
        packages: ordered,
        flat: false,
        classes,
        member_count,
        status_count,
    }
}

fn as_member_key(m: &Member) -> String {
    format!(
        "{}|{}|{}",
        m.name,
        m.kind,
        if m.is_static { "static" } else { "instance" }
    )
}

fn as_member_view(m: &Member, language: ScriptLanguage, class: &AsClassDraft) -> ApiMember {
    let as3 = language == ScriptLanguage::ActionScript3;
    let kind = m.kind.as_str();
    let callable = m.params.is_some();
    let constructor = kind == "constructor";
    let returns = if callable && !constructor {
        m.ty.clone()
    } else {
        String::new()
    };
    let param_parts = m.params.as_deref().map(split_params).unwrap_or_default();
    let ty_suffix = |separator: &str| {
        if m.ty.is_empty() {
            String::new()
        } else {
            format!("{separator}{}", m.ty)
        }
    };
    let row_signature = match &m.params {
        Some(params) => format!(
            "({}){}",
            params.trim(),
            if returns.is_empty() {
                String::new()
            } else {
                format!(":{returns}")
            }
        ),
        None => {
            let flag = match kind {
                "constant" => "constant",
                "event" | "callback" => "event",
                _ if m.is_static => "static",
                _ => "",
            };
            let ty = ty_suffix(": ");
            match (ty.is_empty(), flag.is_empty()) {
                (false, false) => format!("{ty}  ·  {flag}"),
                (false, true) => ty,
                (true, _) => flag.to_owned(),
            }
        }
    };
    let access = if class.pseudo { "" } else { "public " };
    let stat = if m.is_static { "static " } else { "" };
    let owner = class.name.as_str();
    let declaration = match (&m.params, kind) {
        (Some(params), _) => {
            let head = if constructor {
                format!("{access}function {}", m.name)
            } else {
                format!("{access}{stat}function {}", m.name)
            };
            let ret = if returns.is_empty() {
                String::new()
            } else {
                format!(":{returns}")
            };
            if param_parts.len() > 1 {
                format!("{head}(\n    {}\n){ret}", param_parts.join(",\n    "))
            } else {
                format!("{head}({}){ret}", params.trim())
            }
        }
        (None, "constant") if class.pseudo => format!("const {}{}", m.name, ty_suffix(":")),
        (None, "constant") => format!("public static const {}{}", m.name, ty_suffix(":")),
        (None, "property") => format!("{access}{stat}var {}{}", m.name, ty_suffix(":")),
        (None, "event" | "callback") if as3 && !class.pseudo => {
            format!("addEventListener(\"{}\", listener)", m.name)
        }
        (None, "event" | "callback") if !class.pseudo => {
            format!("{}.{} = function() {{}}", lower_first(owner), m.name)
        }
        _ => m.name.clone(),
    };
    let arg_names: Vec<String> = param_parts
        .iter()
        .map(|part| describe_param(part).0)
        .filter(|name| !name.is_empty())
        .collect();
    let args = arg_names.join(", ");
    let receiver = lower_first(owner);
    let example = if class.pseudo {
        if callable {
            format!("{}({args});", m.name)
        } else {
            m.name.clone()
        }
    } else {
        match (kind, callable) {
            ("constructor", _) => format!("var {receiver}:{owner} = new {owner}({args});"),
            (_, true) if m.is_static => format!("{owner}.{}({args});", m.name),
            (_, true) => format!("{receiver}.{}({args});", m.name),
            ("event" | "callback", false) if as3 => {
                format!("{receiver}.addEventListener(\"{}\", listener);", m.name)
            }
            ("event" | "callback", false) => {
                format!("{receiver}.{} = function() {{\n}};", m.name)
            }
            (_, false) if m.is_static || kind == "constant" => format!("{owner}.{}", m.name),
            (_, false) => format!("{receiver}.{}", m.name),
        }
    };
    let description = script_api::keyword_reference_summary(language, &m.name)
        .unwrap_or_default()
        .to_owned();
    let kind_label = match (m.is_static, kind) {
        (_, "special_type") => "special type".to_owned(),
        (true, "constant") => "constant".to_owned(),
        (true, _) => format!("static {kind}"),
        (false, _) => kind.to_owned(),
    };
    ApiMember {
        key: as_member_key(m),
        name: m.name.clone(),
        label: if is_language_kind(kind) {
            m.name.clone()
        } else {
            m.label()
        },
        kind: kind_label,
        group: member_group(kind),
        row_signature,
        declaration,
        params: param_parts
            .iter()
            .map(|part| describe_param(part))
            .collect(),
        returns,
        support: Support::of(&m.status),
        note: m.limitation.clone().unwrap_or_default(),
        description,
        example,
        insertion: super::actions_sidebar::toolbox_insertion(m, language),
        see_also: Vec::new(),
    }
}

fn build_js_ts_index() -> ApiIndex {
    let mut classes = HashMap::new();
    let mut package = ApiPackage {
        label: String::new(),
        classes: Vec::new(),
    };
    let mut member_count = 0;
    for &(category, description, _) in JS_TS_CATEGORIES {
        let entries: Vec<&'static JsTsApiEntry> = JS_TS_API_ENTRIES
            .iter()
            .filter(|e| e.category == category)
            .collect();
        if entries.is_empty() {
            continue;
        }
        let key_of = |e: &JsTsApiEntry| format!("{}|{}", e.category, e.name);
        let mut members: Vec<ApiMember> = entries
            .iter()
            .map(|entry| {
                let (group, kind) = match entry.kind {
                    JsTsEntryKind::Binding => (MemberGroup::Properties, "binding".to_owned()),
                    JsTsEntryKind::Function => (MemberGroup::Methods, "function".to_owned()),
                    JsTsEntryKind::Member(owner) => {
                        (MemberGroup::Methods, format!("member of {owner}"))
                    }
                    JsTsEntryKind::Keyword => (MemberGroup::Language, "keyword".to_owned()),
                    JsTsEntryKind::TypeConstruct => (MemberGroup::Language, "type".to_owned()),
                    JsTsEntryKind::Class => (MemberGroup::Language, "class".to_owned()),
                };
                // Related entries: a class and its members.
                let related = |other: &&&JsTsApiEntry| match (entry.kind, other.kind) {
                    (JsTsEntryKind::Class, JsTsEntryKind::Member(owner)) => owner == entry.name,
                    (JsTsEntryKind::Member(owner), JsTsEntryKind::Class) => owner == other.name,
                    (JsTsEntryKind::Member(a), JsTsEntryKind::Member(b)) => {
                        a == b && other.name != entry.name
                    }
                    _ => false,
                };
                let see_also = entries
                    .iter()
                    .filter(related)
                    .take(6)
                    .map(|other| ApiLink {
                        label: other.name.to_owned(),
                        class_key: category.to_owned(),
                        member_key: Some(key_of(other)),
                    })
                    .collect();
                let row_signature = entry
                    .signature
                    .strip_prefix(entry.name)
                    .filter(|rest| rest.starts_with(['(', ':', '<']))
                    .unwrap_or(entry.signature)
                    .to_owned();
                ApiMember {
                    key: key_of(entry),
                    name: entry.name.to_owned(),
                    label: entry.name.to_owned(),
                    kind: if entry.ts_only {
                        format!("{kind}  ·  TypeScript only")
                    } else {
                        kind
                    },
                    group,
                    row_signature,
                    declaration: entry.signature.to_owned(),
                    params: Vec::new(),
                    returns: String::new(),
                    support: None,
                    note: format!(
                        "{}. Available in: {}.",
                        entry.provenance, entry.availability
                    ),
                    description: entry.summary.to_owned(),
                    example: entry.insertion.to_owned(),
                    insertion: super::actions::js_ts_toolbox_insertion(entry),
                    see_also,
                }
            })
            .collect();
        members.sort_by_key(|m| m.group);
        member_count += members.len();
        package
            .classes
            .push((category.to_owned(), category.to_owned()));
        classes.insert(
            category.to_owned(),
            Arc::new(ApiClass {
                key: category.to_owned(),
                name: category.to_owned(),
                package: "JavaScript / TypeScript".into(),
                chain: Vec::new(),
                status: String::new(),
                description: format!("{description}."),
                insertion: None,
                members,
            }),
        );
    }
    ApiIndex {
        class_count: classes.len(),
        packages: vec![package],
        flat: true,
        classes,
        member_count,
        status_count: 0,
    }
}

/// One search result of the API tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ApiHit {
    pub class_key: String,
    pub member_key: Option<String>,
    /// `MovieClip`, `MovieClip.gotoAndPlay()`.
    pub label: String,
    /// Package of a class hit, kind of a member hit.
    pub meta: String,
    pub support: Option<Support>,
    /// 0 exact name, 1 prefix, 2 substring.
    pub rank: u8,
}

/// Classes and members of `scope` whose name matches `query`, ordered exact name, prefix,
/// substring; classes before members inside a rank. `Class.member` narrows members to a class.
pub(crate) fn api_search(scope: LanguageScope, query: &str) -> Vec<ApiHit> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let index = api_index(scope);
    let rank = |name: &str, needle: &str| -> Option<u8> {
        let name = name.to_lowercase();
        if name == needle {
            Some(0)
        } else if name.starts_with(needle) {
            Some(1)
        } else if name.contains(needle) {
            Some(2)
        } else {
            None
        }
    };
    let dotted = q
        .rsplit_once('.')
        .filter(|(owner, member)| !owner.is_empty() && !member.is_empty());
    let mut hits = Vec::new();
    for (class_key, _) in index.packages.iter().flat_map(|p| &p.classes) {
        let Some(class) = index.class(class_key) else {
            continue;
        };
        let pseudo = class.insertion.is_none();
        let class_name = class.name.to_lowercase();
        if !pseudo || index.flat {
            let class_rank = rank(&class.name, &q)
                .or_else(|| class.key.to_lowercase().contains(&q).then_some(2));
            if let Some(rank) = class_rank {
                hits.push(ApiHit {
                    class_key: class.key.clone(),
                    member_key: None,
                    label: class.name.clone(),
                    meta: class.package.clone(),
                    support: None,
                    rank,
                });
            }
        }
        let owner_matches = dotted
            .is_some_and(|(owner, _)| class_name == owner || class.key.to_lowercase() == owner);
        for member in &class.members {
            let by_owner = dotted
                .filter(|_| owner_matches)
                .and_then(|(_, name)| rank(&member.name, name));
            let Some(rank) = [by_owner, rank(&member.name, &q)]
                .into_iter()
                .flatten()
                .min()
            else {
                continue;
            };
            hits.push(ApiHit {
                class_key: class.key.clone(),
                member_key: Some(member.key.clone()),
                label: if pseudo {
                    member.label.clone()
                } else {
                    format!("{}.{}", class.name, member.label)
                },
                meta: member.kind.clone(),
                support: member.support,
                rank,
            });
        }
    }
    hits.sort_by(|a, b| {
        (
            a.rank,
            a.member_key.is_some(),
            a.label.to_lowercase(),
            &a.class_key,
        )
            .cmp(&(
                b.rank,
                b.member_key.is_some(),
                b.label.to_lowercase(),
                &b.class_key,
            ))
    });
    hits
}

/// Selection and view options of the API tab for one reference language.
#[derive(Clone, Debug, Default)]
pub(crate) struct ApiSelection {
    pub class: Option<String>,
    pub member: Option<String>,
    /// Open packages.
    pub open: BTreeSet<String>,
    /// Keyboard row in the tree column.
    pub cursor: usize,
}

/// State of the API tab, kept with the window state.
#[derive(Clone, Debug, Default)]
pub(crate) struct ApiBrowserState {
    as2: ApiSelection,
    as3: ApiSelection,
    js_ts: ApiSelection,
    pub filter: MemberFilter,
    pub hide_missing: bool,
    /// ↑↓ move in the member list instead of the tree.
    pub focus_members: bool,
    /// Bring the selected rows into view on the next frame.
    pub reveal_tree: bool,
    pub reveal_member: bool,
    hits: Option<(LanguageScope, String, Arc<Vec<ApiHit>>)>,
}

impl ApiBrowserState {
    pub(crate) fn selection(&self, scope: LanguageScope) -> &ApiSelection {
        match scope {
            LanguageScope::As1As2 => &self.as2,
            LanguageScope::As3 => &self.as3,
            LanguageScope::JsTs | LanguageScope::All => &self.js_ts,
        }
    }
    pub(crate) fn selection_mut(&mut self, scope: LanguageScope) -> &mut ApiSelection {
        match scope {
            LanguageScope::As1As2 => &mut self.as2,
            LanguageScope::As3 => &mut self.as3,
            LanguageScope::JsTs | LanguageScope::All => &mut self.js_ts,
        }
    }
    /// Search results for `query`, computed once per language and query.
    pub(crate) fn hits(&mut self, scope: LanguageScope, query: &str) -> Arc<Vec<ApiHit>> {
        let query = query.trim();
        if let Some((cached_scope, cached_query, hits)) = &self.hits
            && *cached_scope == scope
            && cached_query == query
        {
            return Arc::clone(hits);
        }
        let hits = Arc::new(api_search(scope, query));
        self.hits = Some((scope, query.to_owned(), Arc::clone(&hits)));
        self.selection_mut(scope).cursor = 0;
        hits
    }
    /// Opens `class_key` (and `member_key`) and reveals it in the tree.
    pub(crate) fn select(
        &mut self,
        scope: LanguageScope,
        class_key: &str,
        member_key: Option<&str>,
    ) {
        let index = api_index(scope);
        if index.class(class_key).is_none() {
            return;
        }
        let package = index.package_of(class_key).map(str::to_owned);
        let selection = self.selection_mut(scope);
        selection.class = Some(class_key.to_owned());
        selection.member = member_key.map(str::to_owned);
        if let Some(package) = package {
            selection.open.insert(package);
        }
        self.reveal_tree = true;
        self.reveal_member = member_key.is_some();
        self.focus_members = member_key.is_some();
    }
    /// First use of a language: open its default class so the window never starts empty.
    pub(crate) fn ensure_selection(&mut self, scope: LanguageScope) {
        if self.selection(scope).class.is_some() {
            return;
        }
        if let Some(key) = api_index(scope).default_class() {
            let key = key.to_owned();
            self.select(scope, &key, None);
            self.focus_members = false;
        }
    }
    pub(crate) fn selected_class(&self, scope: LanguageScope) -> Option<&'static Arc<ApiClass>> {
        api_index(scope).class(self.selection(scope).class.as_deref()?)
    }
    pub(crate) fn selected_member(&self, scope: LanguageScope) -> Option<&'static ApiMember> {
        let class = self.selected_class(scope)?;
        class.member(self.selection(scope).member.as_deref()?)
    }
    /// Members of the selected class that pass the filter row, in list order.
    pub(crate) fn visible_members(&self, scope: LanguageScope) -> Vec<&'static ApiMember> {
        self.selected_class(scope)
            .map(|class| {
                class
                    .members
                    .iter()
                    .filter(|m| m.group.passes(self.filter))
                    .filter(|m| !(self.hide_missing && m.support == Some(Support::Missing)))
                    .collect()
            })
            .unwrap_or_default()
    }
}

// ── Scripting Reference: rows, blocks and the three API columns ──

/// Tooltip of a disabled Insert at cursor.
pub(crate) const NO_SCRIPT_EDITOR: &str =
    "Open a script in the Actions panel to insert at its cursor";

/// What the columns ask the window to do after the frame.
#[derive(Default)]
pub(crate) struct RefActions {
    pub insert: Option<CodeCommand>,
    pub open_in_browser: bool,
}

/// List keys of the frame, taken by the window before any field sees them.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RefKeys {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub enter: bool,
}

/// `1234567` → `1 234 567`.
pub(crate) fn group_thousands(value: usize) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(' ');
        }
        out.push(ch);
    }
    out
}

/// Left text of the footer status row on the API tab.
pub(crate) fn api_footer_text(scope: LanguageScope) -> String {
    let index = api_index(scope);
    let (classes, members) = if index.flat {
        ("categories", "entries")
    } else {
        ("classes", "members")
    };
    format!(
        "{} · {} {classes} · {} {members}",
        scope.reference_label(),
        group_thousands(index.class_count),
        group_thousands(index.member_count)
    )
}

/// One list row of the Reference (tree row, search hit, member, outline entry): `row()` high.
pub(crate) struct RefRow<'a> {
    pub label: &'a str,
    pub style: TextStyle,
    pub depth: usize,
    /// Disclosure chevron of a package: `Some(open)`.
    pub chevron: Option<bool>,
    pub icon: Option<neoflash_ui::Icon>,
    /// Status dot column: `Some(None)` keeps the column empty.
    pub dot: Option<Option<Support>>,
    /// Signature after the label, `Mono Small`, shortened with an ellipsis.
    pub trailing: &'a str,
    /// Right-aligned secondary text.
    pub meta: &'a str,
    pub selected: bool,
    /// Keyboard row that is not the selection.
    pub cursor: bool,
    pub dimmed: bool,
}

impl<'a> RefRow<'a> {
    pub(crate) fn new(label: &'a str) -> Self {
        Self {
            label,
            style: TextStyle::Body,
            depth: 0,
            chevron: None,
            icon: None,
            dot: None,
            trailing: "",
            meta: "",
            selected: false,
            cursor: false,
            dimmed: false,
        }
    }

    pub(crate) fn paint(&self, ui: &Ui, rect: Rect, response: &egui::Response) {
        if !ui.is_rect_visible(rect) {
            return;
        }
        let p = neoflash_ui::palette(ui.ctx());
        let corner = egui::CornerRadius::same(radius::sm() as u8);
        if self.selected {
            ui.painter().rect_filled(rect, corner, p.bg_selected);
        } else if response.hovered() || self.cursor {
            ui.painter().rect_filled(rect, corner, p.bg_hover);
        }
        let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        let cy = rect.center().y;
        let mut x =
            rect.left() + space::s3() + self.depth as f32 * neoflash_ui::metrics::LAYER_INDENT;
        if let Some(open) = self.chevron {
            icons::paint(
                &painter,
                ui.ctx(),
                Rect::from_center_size(egui::pos2(x + 6.0, cy), vec2(12.0, 12.0)),
                if open { i16::CHEVDOWN } else { i16::CHEVRIGHT },
                p.text_secondary,
            );
            x += 12.0 + space::s2();
        }
        if let Some(icon) = self.icon {
            icons::paint(
                &painter,
                ui.ctx(),
                Rect::from_center_size(egui::pos2(x + 8.0, cy), vec2(16.0, 16.0)),
                icon,
                if self.selected {
                    p.accent_default
                } else {
                    p.text_secondary
                },
            );
            x += 16.0 + space::s2();
        }
        if let Some(dot) = self.dot {
            if let Some(support) = dot {
                painter.circle_filled(egui::pos2(x + 3.0, cy), 3.0, support.color(p));
            }
            x += 6.0 + space::s2();
        }
        let mut right = rect.right() - space::s3();
        if !self.meta.is_empty() {
            let galley = painter.layout_no_wrap(
                self.meta.to_owned(),
                font(TextStyle::Small),
                p.text_tertiary,
            );
            right -= galley.size().x;
            painter.galley(
                egui::pos2(right, cy - galley.size().y / 2.0),
                galley,
                p.text_tertiary,
            );
            right -= space::s2();
        }
        let color = if self.dimmed {
            p.text_tertiary
        } else if self.selected || self.style == TextStyle::Mono {
            p.text_primary
        } else {
            p.text_secondary
        };
        let label = fit_text_to_width(ui, self.label, self.style, (right - x).max(0.0));
        let galley = painter.layout_no_wrap(label, font(self.style), color);
        let label_w = galley.size().x;
        painter.galley(egui::pos2(x, cy - galley.size().y / 2.0), galley, color);
        x += label_w + space::s2();
        if !self.trailing.is_empty() && right - x > 24.0 {
            let text = fit_text_to_width(ui, self.trailing, TextStyle::MonoSmall, right - x);
            let galley = painter.layout_no_wrap(text, font(TextStyle::MonoSmall), p.text_tertiary);
            painter.galley(
                egui::pos2(x, cy - galley.size().y / 2.0),
                galley,
                p.text_tertiary,
            );
        }
        let (label, selected) = (self.label.to_owned(), self.selected);
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, &label)
        });
    }

    /// Adds the row at the cursor, as wide as the column.
    pub(crate) fn show(&self, ui: &mut Ui) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(
            vec2(ui.available_width(), neoflash_ui::metrics::row()),
            egui::Sense::click(),
        );
        self.paint(ui, rect, &response);
        response
    }
}

/// Overline caption of a block in the class and detail columns.
pub(crate) fn group_caption(ui: &mut Ui, text: &str) {
    let p = neoflash_ui::palette(ui.ctx());
    ui.add_space(space::s3());
    ui.label(rich(text, TextStyle::Overline).color(p.text_tertiary));
    ui.add_space(space::s1());
}

/// Code box: `bg/field`, 1 px `border/subtle`, syntax-coloured `Mono`, wrapped to the column.
pub(crate) fn code_box(ui: &mut Ui, code: &str) -> egui::Response {
    let p = neoflash_ui::palette(ui.ctx());
    let pad = vec2(space::s3(), space::s2());
    let width = ui.available_width().max(2.0 * pad.x + 8.0);
    let mut job = c::code::highlight(ui.ctx(), code);
    job.wrap.max_width = width - 2.0 * pad.x;
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let (rect, response) = ui.allocate_exact_size(
        vec2(width, galley.size().y + 2.0 * pad.y),
        egui::Sense::hover(),
    );
    if ui.is_rect_visible(rect) {
        let corner = egui::CornerRadius::same(radius::md() as u8);
        ui.painter().rect_filled(rect, corner, p.bg_field);
        ui.painter().rect_stroke(
            rect,
            corner,
            Stroke::new(1.0_f32, p.border_subtle),
            StrokeKind::Inside,
        );
        ui.painter()
            .with_clip_rect(rect.intersect(ui.clip_rect()))
            .galley(rect.min + pad, galley, p.text_primary);
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, code));
    response
}

/// Whether Insert at cursor has a script editor to write to (set by the window every frame).
fn insert_target_id() -> egui::Id {
    egui::Id::new("neoflash-reference-insert-target")
}
fn article_insert_id() -> egui::Id {
    egui::Id::new("neoflash-reference-article-insert")
}
pub(crate) fn set_insert_available(ctx: &egui::Context, available: bool) {
    ctx.data_mut(|d| d.insert_temp(insert_target_id(), available));
}
fn insert_available(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(insert_target_id()))
        .unwrap_or(false)
}
/// Code a guide sample asked to insert during this frame.
pub(crate) fn take_article_insert(ctx: &egui::Context) -> Option<String> {
    ctx.data_mut(|d| d.remove_temp::<String>(article_insert_id()))
}

/// `Insert at cursor`, disabled with a tooltip while no script editor is open.
fn insert_button(ui: &mut Ui, primary: bool) -> bool {
    let available = insert_available(ui.ctx());
    let button = if primary {
        Button::primary("Insert at cursor")
    } else {
        Button::new("Insert at cursor")
    };
    let response = ui.add_enabled(available, button);
    if available {
        return response.clicked();
    }
    // egui does not hover disabled widgets, so the reason is shown from `contains_pointer`.
    let ctx = ui.ctx();
    if response.contains_pointer() && !ctx.input(|i| i.pointer.any_down()) {
        let delay = f64::from(ctx.style().interaction.tooltip_delay);
        let still = f64::from(ctx.input(|i| i.pointer.time_since_last_movement()));
        if still < delay {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(delay - still + 0.01));
        } else {
            c::paint_tooltip_bubble(ctx, response.rect, NO_SCRIPT_EDITOR, None);
        }
    }
    false
}

/// `Copy` and `Insert at cursor` under a guide code sample.
fn code_sample_actions(ui: &mut Ui, code: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::s2();
        if ui.add(Button::new("Copy").icon(i16::COPY)).clicked() {
            ui.ctx().copy_text(code.to_owned());
        }
        if insert_button(ui, false) {
            ui.ctx()
                .data_mut(|d| d.insert_temp(article_insert_id(), code.to_owned()));
        }
    });
}

/// Status pill of a class or member.
fn support_badge(ui: &mut Ui, support: Support) {
    let tone = match support {
        Support::Working => BadgeType::Success,
        Support::Partial => BadgeType::Warning,
        Support::Missing => BadgeType::Neutral,
    };
    ui.add(Badge::new(support.label(), tone));
}

/// "In NeoFlash" line: the catalogue status in words, with its note when there is one.
fn support_message(ui: &mut Ui, support: Support, note: &str) {
    let text = match support {
        Support::Working => {
            "Working. The documented behaviour runs in Test Movie and HTML5 publish; only browser or sandbox limits may remain.".to_owned()
        }
        Support::Partial if note.is_empty() => {
            "Partial. A runtime path exists; part of the documented behaviour is missing.".to_owned()
        }
        Support::Partial => format!("Partial. {note}"),
        Support::Missing => {
            "Missing. NeoFlash has no runtime path for it yet; the compiler reports its use.".to_owned()
        }
    };
    ui.add(match support {
        Support::Working => c::InlineMessage::success(&text),
        Support::Partial => c::InlineMessage::warning(&text),
        Support::Missing => c::InlineMessage::info(&text),
    });
}

/// `Mono Small` text that opens another declaration: accent and underlined while hovered.
fn mono_link(ui: &mut Ui, text: &str, color: egui::Color32) -> egui::Response {
    let p = neoflash_ui::palette(ui.ctx());
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font(TextStyle::MonoSmall), color);
    let (rect, response) = ui.allocate_exact_size(galley.size(), egui::Sense::click());
    let hovered = response.hovered();
    let tint = if hovered { p.accent_default } else { color };
    ui.painter()
        .galley_with_override_text_color(rect.min, galley, tint);
    if hovered {
        ui.painter().hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            Stroke::new(1.0_f32, p.accent_default),
        );
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let label = text.to_owned();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Link, true, &label));
    response
}

fn member_icon(member: &ApiMember) -> neoflash_ui::Icon {
    match member.group {
        MemberGroup::Properties => i16::VARIABLE,
        MemberGroup::Events => i16::FLAG,
        MemberGroup::Methods | MemberGroup::Language => i16::CODE,
    }
}

/// Tree column of the API tab: packages → classes, or the flat search results.
pub(crate) fn api_tree_column(
    ui: &mut Ui,
    state: &mut ApiBrowserState,
    scope: LanguageScope,
    query: &str,
    keys: RefKeys,
) {
    enum Row<'a> {
        Package(&'a ApiPackage, bool),
        Class {
            key: &'a str,
            name: &'a str,
            depth: usize,
        },
        Hit(&'a ApiHit),
    }
    let p = neoflash_ui::palette(ui.ctx());
    let index = api_index(scope);
    let searching = !query.trim().is_empty();
    let hits = searching.then(|| state.hits(scope, query));
    let selection = state.selection(scope).clone();
    let mut rows: Vec<Row> = Vec::new();
    if let Some(hits) = &hits {
        rows.extend(hits.iter().map(Row::Hit));
    } else {
        for package in &index.packages {
            let open = index.flat || selection.open.contains(&package.label);
            if !index.flat {
                rows.push(Row::Package(package, open));
            }
            if open {
                rows.extend(package.classes.iter().map(|(key, name)| Row::Class {
                    key,
                    name,
                    depth: usize::from(!index.flat),
                }));
            }
        }
    }
    let is_selected = |row: &Row| match row {
        Row::Package(..) => false,
        Row::Class { key, .. } => selection.class.as_deref() == Some(key),
        Row::Hit(hit) => {
            selection.class.as_deref() == Some(hit.class_key.as_str())
                && selection.member == hit.member_key
        }
    };
    let count = rows.len();
    let mut cursor = selection.cursor.min(count.saturating_sub(1));
    let mut reveal = std::mem::take(&mut state.reveal_tree);
    if reveal && let Some(at) = rows.iter().position(&is_selected) {
        cursor = at;
    }
    // What the frame does to the row under the keyboard or the pointer.
    enum Act {
        Select(usize, bool),
        Toggle(usize),
    }
    let mut act: Option<Act> = None;
    if !state.focus_members && count > 0 {
        if keys.down || keys.up {
            cursor = if keys.down {
                (cursor + 1).min(count - 1)
            } else {
                cursor.saturating_sub(1)
            };
            reveal = true;
            if !matches!(rows[cursor], Row::Package(..)) {
                act = Some(Act::Select(cursor, false));
            }
        } else if keys.enter {
            act = Some(match rows[cursor] {
                Row::Package(..) => Act::Toggle(cursor),
                _ => Act::Select(cursor, true),
            });
        } else if keys.right {
            match rows[cursor] {
                Row::Package(_, false) => act = Some(Act::Toggle(cursor)),
                Row::Package(..) => {}
                _ => act = Some(Act::Select(cursor, true)),
            }
        } else if keys.left && matches!(rows[cursor], Row::Package(_, true)) {
            act = Some(Act::Toggle(cursor));
        }
    }

    let row_h = neoflash_ui::metrics::row();
    let pitch = row_h + 1.0;
    let pad = space::s1();
    egui::ScrollArea::vertical()
        .id_salt(("reference-api-tree", scope.html_slug(), searching))
        .auto_shrink([false, false])
        .themed()
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if searching {
                let summary = if count == 0 {
                    format!(
                        "No classes or members of {} match “{}”.",
                        scope.reference_label(),
                        query.trim()
                    )
                } else {
                    format!(
                        "{} {}",
                        group_thousands(count),
                        if count == 1 { "match" } else { "matches" }
                    )
                };
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(
                        (pad + space::s3()) as i8,
                        space::s2() as i8,
                    ))
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(
                                rich(&summary, TextStyle::Small).color(p.text_tertiary),
                            )
                            .wrap(),
                        );
                    });
            }
            let width = ui.available_width();
            let (area, _) = ui.allocate_exact_size(
                vec2(width, count as f32 * pitch + 2.0 * pad),
                egui::Sense::hover(),
            );
            let row_rect = |i: usize| {
                Rect::from_min_size(
                    egui::pos2(area.left() + pad, area.top() + pad + i as f32 * pitch),
                    vec2(width - 2.0 * pad, row_h),
                )
            };
            let clip = ui.clip_rect();
            let first =
                (((clip.top() - area.top() - pad) / pitch).floor().max(0.0) as usize).min(count);
            let last =
                (((clip.bottom() - area.top() - pad) / pitch).ceil().max(0.0) as usize).min(count);
            for (i, row) in rows.iter().enumerate().take(last).skip(first) {
                let rect = row_rect(i);
                let selected = is_selected(row);
                let at_cursor = i == cursor && !state.focus_members;
                let (id, spec) = match row {
                    Row::Package(package, open) => (
                        ui.id().with(("package", &package.label)),
                        RefRow {
                            chevron: Some(*open),
                            icon: Some(i16::PACKAGE),
                            ..RefRow::new(&package.label)
                        },
                    ),
                    Row::Class { key, name, depth } => (
                        ui.id().with(("class", key)),
                        RefRow {
                            depth: *depth,
                            icon: Some(if index.flat {
                                i16::PACKAGE
                            } else if key.starts_with('<') || *name == "Package functions" {
                                i16::CODE
                            } else {
                                i16::CLASS
                            }),
                            ..RefRow::new(name)
                        },
                    ),
                    Row::Hit(hit) => (
                        ui.id().with(("hit", &hit.class_key, &hit.member_key)),
                        RefRow {
                            icon: Some(match &hit.member_key {
                                None => i16::CLASS,
                                Some(key) => index
                                    .class(&hit.class_key)
                                    .and_then(|class| class.member(key))
                                    .map_or(i16::CODE, member_icon),
                            }),
                            meta: if hit.member_key.is_none() {
                                &hit.meta
                            } else {
                                ""
                            },
                            ..RefRow::new(&hit.label)
                        },
                    ),
                };
                let response = ui.interact(rect, id, egui::Sense::click());
                RefRow {
                    selected,
                    cursor: at_cursor && !selected,
                    ..spec
                }
                .paint(ui, rect, &response);
                if response.clicked() {
                    cursor = i;
                    state.focus_members = false;
                    act = Some(match row {
                        Row::Package(..) => Act::Toggle(i),
                        Row::Hit(hit) => Act::Select(i, hit.member_key.is_some()),
                        Row::Class { .. } => Act::Select(i, false),
                    });
                }
            }
            if reveal && count > 0 {
                ui.scroll_to_rect(row_rect(cursor).expand(row_h), None);
            }
        });

    // Apply after the rows are painted: the borrow of `rows` ends here.
    let outcome = act.map(|act| match act {
        Act::Toggle(i) => match &rows[i] {
            Row::Package(package, _) => (Some(package.label.clone()), None, false),
            _ => (None, None, false),
        },
        Act::Select(i, to_members) => match &rows[i] {
            Row::Class { key, .. } => (None, Some(((*key).to_owned(), None)), to_members),
            Row::Hit(hit) => (
                None,
                Some((hit.class_key.clone(), hit.member_key.clone())),
                to_members,
            ),
            Row::Package(..) => (None, None, false),
        },
    });
    drop(rows);
    let target = state.selection_mut(scope);
    target.cursor = cursor;
    if let Some((toggle, select, to_members)) = outcome {
        if let Some(package) = toggle
            && !target.open.remove(&package)
        {
            target.open.insert(package);
        }
        if let Some((class, member)) = select {
            let changed = target.class.as_deref() != Some(class.as_str());
            if changed || member.is_some() {
                target.member = member;
            }
            target.class = Some(class);
            state.reveal_member = true;
            if to_members {
                state.focus_members = true;
                if state.selection(scope).member.is_none()
                    && let Some(first) = state.visible_members(scope).first()
                {
                    state.selection_mut(scope).member = Some(first.key.clone());
                }
            }
        }
    }
}

/// Middle column of the API tab: the selected class and its members.
pub(crate) fn api_class_column(
    ui: &mut Ui,
    state: &mut ApiBrowserState,
    scope: LanguageScope,
    keys: RefKeys,
    actions: &mut RefActions,
) {
    let p = neoflash_ui::palette(ui.ctx());
    let Some(class) = state.selected_class(scope) else {
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(
                space::s5() as i8,
                space::s4() as i8,
            ))
            .show(ui, |ui| {
                ui.label(
                    rich("Select a class in the tree.", TextStyle::Body).color(p.text_tertiary),
                );
            });
        return;
    };
    let members = state.visible_members(scope);
    // ↑↓ in the member list; ← returns to the tree.
    if state.focus_members {
        let at = state
            .selection(scope)
            .member
            .as_deref()
            .and_then(|key| members.iter().position(|m| m.key == key));
        let next = if keys.down {
            Some(at.map_or(0, |i| (i + 1).min(members.len().saturating_sub(1))))
        } else if keys.up {
            Some(at.map_or(0, |i| i.saturating_sub(1)))
        } else {
            None
        };
        if let Some(member) = next.and_then(|i| members.get(i)) {
            state.selection_mut(scope).member = Some(member.key.clone());
            state.reveal_member = true;
        }
        if keys.left {
            state.focus_members = false;
            state.reveal_tree = true;
        }
    }
    let selected_member = state.selection(scope).member.clone();
    let reveal = std::mem::take(&mut state.reveal_member);
    let mut navigate: Option<String> = None;
    let mut clicked: Option<String> = None;

    egui::ScrollArea::vertical()
        .id_salt(("reference-api-class", scope.html_slug(), &class.key))
        .auto_shrink([false, false])
        .themed()
        .show(ui, |ui| {
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(space::s5() as i8, space::s4() as i8))
                .show(ui, |ui| {
                    let width = ui.available_width();
                    ui.set_width(width);
                    ui.spacing_mut().item_spacing = vec2(space::s2(), neoflash_ui::metrics::row_gap());

                    // Name, status, Open in browser. The button shrinks to its icon before the
                    // name is cut.
                    let support = Support::of(&class.status);
                    let badge_text = match class.status.as_str() {
                        "members" => "Members only",
                        _ => support.map_or("", Support::label),
                    };
                    let badge_w = if badge_text.is_empty() {
                        0.0
                    } else {
                        text_width(ui, badge_text, TextStyle::Small) + 12.0 + space::s2()
                    };
                    let button_w =
                        text_width(ui, "Open in browser", TextStyle::Body) + 16.0 + 4.0 * space::s2();
                    let fixed = 16.0 + space::s2() + badge_w + space::s2();
                    let name_w = text_width(ui, &class.name, TextStyle::Heading);
                    let full_button = fixed + name_w + button_w <= width;
                    let name_max = width
                        - fixed
                        - if full_button {
                            button_w
                        } else {
                            neoflash_ui::metrics::control() + space::s2()
                        };
                    ui.horizontal(|ui| {
                        ui.set_min_height(neoflash_ui::metrics::control());
                        icons::show(
                            ui,
                            if class.insertion.is_some() {
                                i16::CLASS
                            } else {
                                i16::PACKAGE
                            },
                            p.accent_default,
                        );
                        let name = fit_text_to_width(ui, &class.name, TextStyle::Heading, name_max);
                        let heading =
                            ui.label(rich(&name, TextStyle::Heading).color(p.text_primary));
                        if name != class.name {
                            c::tooltip(heading, &class.name, None);
                        }
                        match (support, class.status.as_str()) {
                            (Some(support), _) => support_badge(ui, support),
                            (None, "members") => {
                                c::tooltip(
                                    ui.add(Badge::new(badge_text, BadgeType::Neutral)),
                                    "The class declaration has no status of its own; the members listed here do",
                                    None,
                                );
                            }
                            _ => {}
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let open = if full_button {
                                ui.add(Button::ghost("Open in browser").icon(i16::EXTERNAL))
                            } else {
                                ui.add(c::IconButton::new(i16::EXTERNAL, "Open in browser"))
                            };
                            actions.open_in_browser |= open.clicked();
                        });
                    });

                    // Package and superclass chain; indexed parents open on click.
                    // Wrapped rows are as high as the text, not as a control.
                    let control_h = ui.spacing().interact_size.y;
                    ui.spacing_mut().interact_size.y = TextStyle::MonoSmall.spec().line_height;
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(space::s1(), 2.0);
                        let small = |ui: &mut Ui, text: &str| {
                            ui.label(rich(text, TextStyle::MonoSmall).color(p.text_tertiary));
                        };
                        small(ui, &class.package);
                        if !class.chain.is_empty() {
                            small(ui, " · ");
                            small(ui, &class.name);
                        }
                        for (name, key) in &class.chain {
                            small(ui, "→");
                            match key {
                                Some(key) => {
                                    let link = c::tooltip(
                                        mono_link(ui, name, p.text_tertiary),
                                        &format!("Open {key}"),
                                        None,
                                    );
                                    if link.clicked() {
                                        navigate = Some(key.clone());
                                    }
                                }
                                None => small(ui, name),
                            }
                        }
                    });
                    ui.spacing_mut().interact_size.y = control_h;
                    if !class.description.is_empty() {
                        ui.add(
                            egui::Label::new(
                                rich(&class.description, TextStyle::DialogBody)
                                    .color(p.text_secondary),
                            )
                            .wrap(),
                        );
                    }

                    // Filter row; the counts move to their own line when the row is full.
                    ui.add_space(neoflash_ui::metrics::row_gap());
                    let (working, partial, missing) = class.support_counts();
                    let total = class.members.len();
                    let noun = if api_index(scope).flat { "entries" } else { "members" };
                    let counts = if working + partial + missing == 0 {
                        format!("{total} {noun}")
                    } else {
                        format!(
                            "{total} {noun} · {working} working · {partial} partial · {missing} missing"
                        )
                    };
                    let counts_w = text_width(ui, &counts, TextStyle::Small);
                    let mut counts_inline = false;
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = space::s3();
                        c::Segmented::show(
                            ui,
                            &mut state.filter,
                            &[
                                (MemberFilter::All, "All"),
                                (MemberFilter::Properties, "Properties"),
                                (MemberFilter::Methods, "Methods"),
                                (MemberFilter::Events, "Events"),
                            ],
                            false,
                        );
                        if working + partial + missing > 0 {
                            ui.add(c::Checkbox::new(&mut state.hide_missing, "Hide missing"));
                        }
                        if ui.available_width() >= counts_w + space::s3() {
                            counts_inline = true;
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(rich(&counts, TextStyle::Small).color(p.text_tertiary));
                                },
                            );
                        }
                    });
                    if !counts_inline {
                        ui.label(rich(&counts, TextStyle::Small).color(p.text_tertiary));
                    }

                    // One row per member, grouped.
                    ui.spacing_mut().item_spacing.y = 1.0;
                    let mut any = false;
                    for group in MemberGroup::ALL {
                        let rows: Vec<&&ApiMember> =
                            members.iter().filter(|m| m.group == group).collect();
                        if rows.is_empty() {
                            continue;
                        }
                        any = true;
                        group_caption(ui, group.label());
                        for member in rows {
                            let selected = selected_member.as_deref() == Some(member.key.as_str());
                            let response = RefRow {
                                style: TextStyle::Mono,
                                dot: Some(member.support),
                                trailing: &member.row_signature,
                                selected,
                                dimmed: member.support == Some(Support::Missing),
                                ..RefRow::new(&member.label)
                            }
                            .show(ui);
                            if response.clicked() {
                                clicked = Some(member.key.clone());
                            }
                            if selected && reveal {
                                ui.scroll_to_rect(
                                    response.rect.expand(neoflash_ui::metrics::row()),
                                    None,
                                );
                            }
                        }
                    }
                    if !any {
                        ui.add_space(space::s3());
                        ui.label(
                            rich(
                                "No members match the filter.",
                                TextStyle::Body,
                            )
                            .color(p.text_tertiary),
                        );
                    }
                });
        });
    if let Some(key) = clicked {
        state.selection_mut(scope).member = Some(key);
        state.focus_members = true;
    }
    if let Some(key) = navigate {
        state.select(scope, &key, None);
    }
}

/// Detail column of the API tab: the selected member, or a summary of the class.
pub(crate) fn api_detail_column(
    ui: &mut Ui,
    state: &mut ApiBrowserState,
    scope: LanguageScope,
    actions: &mut RefActions,
) {
    let p = neoflash_ui::palette(ui.ctx());
    let Some(class) = state.selected_class(scope) else {
        return;
    };
    let member = state.selected_member(scope);
    let mut navigate: Option<ApiLink> = None;
    egui::ScrollArea::vertical()
        .id_salt((
            "reference-api-detail",
            scope.html_slug(),
            &class.key,
            member.map(|m| &m.key),
        ))
        .auto_shrink([false, false])
        .themed()
        .show(ui, |ui| {
            egui::Frame::new()
                .inner_margin(egui::Margin::same(space::s4() as i8))
                .show(ui, |ui| {
                    let width = ui.available_width();
                    ui.set_width(width);
                    ui.spacing_mut().item_spacing =
                        vec2(space::s2(), neoflash_ui::metrics::row_gap());
                    let Some(member) = member else {
                        class_summary(ui, class, actions);
                        return;
                    };
                    // Name, status, kind.
                    ui.horizontal_wrapped(|ui| {
                        ui.add(
                            egui::Label::new(
                                rich(&member.label, TextStyle::Title).color(p.text_primary),
                            )
                            .wrap(),
                        );
                        if let Some(support) = member.support {
                            support_badge(ui, support);
                        }
                        ui.label(rich(&member.kind, TextStyle::Small).color(p.text_tertiary));
                    });
                    code_box(ui, &member.declaration);
                    if !member.description.is_empty() {
                        ui.add(
                            egui::Label::new(
                                rich(&member.description, TextStyle::DialogBody)
                                    .color(p.text_secondary),
                            )
                            .wrap(),
                        );
                    }
                    if !member.params.is_empty() {
                        group_caption(ui, "Parameters");
                        let name_w = neoflash_ui::metrics::field_s().min(width * 0.4);
                        for (name, text) in &member.params {
                            ui.horizontal_top(|ui| {
                                ui.spacing_mut().item_spacing.x = space::s2();
                                let shown = fit_text_to_width(ui, name, TextStyle::Mono, name_w);
                                let (rect, _) = ui.allocate_exact_size(
                                    vec2(name_w, TextStyle::Body.spec().line_height),
                                    egui::Sense::hover(),
                                );
                                ui.painter().text(
                                    rect.left_center(),
                                    egui::Align2::LEFT_CENTER,
                                    shown,
                                    font(TextStyle::Mono),
                                    p.text_primary,
                                );
                                let rest = ui.available_width().max(24.0);
                                ui.vertical(|ui| {
                                    ui.set_max_width(rest);
                                    ui.add(
                                        egui::Label::new(
                                            rich(text, TextStyle::Body).color(p.text_secondary),
                                        )
                                        .wrap(),
                                    );
                                });
                            });
                        }
                    }
                    if !member.returns.is_empty() {
                        group_caption(ui, "Returns");
                        ui.label(rich(&member.returns, TextStyle::Mono).color(p.text_primary));
                    }
                    group_caption(ui, "In NeoFlash");
                    match member.support {
                        Some(support) => support_message(ui, support, &member.note),
                        None if !member.note.is_empty() => {
                            ui.add(c::InlineMessage::info(&member.note));
                        }
                        None => {
                            ui.add(c::InlineMessage::info(
                                "The progress catalogue has no status for this declaration.",
                            ));
                        }
                    }
                    group_caption(ui, "Example");
                    code_box(ui, &member.example);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = space::s2();
                        if ui.add(Button::new("Copy").icon(i16::COPY)).clicked() {
                            ui.ctx().copy_text(member.example.clone());
                        }
                        if insert_button(ui, true) {
                            actions.insert = Some(member.insertion.clone());
                        }
                    });
                    if !member.see_also.is_empty() {
                        group_caption(ui, "See also");
                        ui.spacing_mut().interact_size.y = TextStyle::MonoSmall.spec().line_height;
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = vec2(space::s2(), space::s1());
                            for (i, link) in member.see_also.iter().enumerate() {
                                if i > 0 {
                                    ui.label(
                                        rich("·", TextStyle::MonoSmall).color(p.text_tertiary),
                                    );
                                }
                                if mono_link(ui, &link.label, p.accent_default).clicked() {
                                    navigate = Some(link.clone());
                                }
                            }
                        });
                    }
                });
        });
    if let Some(link) = navigate {
        state.select(scope, &link.class_key, link.member_key.as_deref());
    }
}

/// Detail column while no member is selected.
fn class_summary(ui: &mut Ui, class: &ApiClass, actions: &mut RefActions) {
    let p = neoflash_ui::palette(ui.ctx());
    ui.horizontal_wrapped(|ui| {
        ui.add(egui::Label::new(rich(&class.name, TextStyle::Title).color(p.text_primary)).wrap());
        if let Some(support) = Support::of(&class.status) {
            support_badge(ui, support);
        }
    });
    ui.add(
        egui::Label::new(
            rich(
                "Select a member to see its signature, parameters, support in NeoFlash and an example.",
                TextStyle::Body,
            )
            .color(p.text_tertiary),
        )
        .wrap(),
    );
    if let Some(support) = Support::of(&class.status) {
        group_caption(ui, "In NeoFlash");
        support_message(ui, support, "");
    }
    if let Some(insertion) = &class.insertion {
        ui.add_space(space::s3());
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::s2();
            if ui.add(Button::new("Copy").icon(i16::COPY)).clicked() {
                ui.ctx().copy_text(class.name.clone());
            }
            if insert_button(ui, true) {
                actions.insert = Some(insertion.clone());
            }
        });
    }
}

/// What ⌘↵ inserts on the API tab: the selected member, else the selected class.
pub(crate) fn api_selected_insertion(
    state: &ApiBrowserState,
    scope: LanguageScope,
) -> Option<CodeCommand> {
    match state.selected_member(scope) {
        Some(member) => Some(member.insertion.clone()),
        None => state.selected_class(scope)?.insertion.clone(),
    }
}

/// Topics that match the search field, listed above the article: title, signature, summary and
/// the sample of each curated topic and `<ESC>` chord of the selected language.
pub(crate) fn render_topic_hits(
    ui: &mut Ui,
    query: &str,
    scope: Option<LanguageScope>,
    project_lang: ScriptLanguage,
    nav_target: &mut Option<usize>,
) {
    let p = neoflash_ui::palette(ui.ctx());
    // Help (`None`) lists only its own authoring topics, never a scripting language.
    let (mut hits, _) =
        search_reference_hits(query, scope.unwrap_or(LanguageScope::All), project_lang);
    if scope.is_none() {
        hits.retain(|hit| hit.dialect_badge == "[Authoring]");
    }
    let heading = format!("Topics matching “{}”", query.trim());
    ui.add(egui::Label::new(rich(&heading, TextStyle::Title).color(p.text_primary)).wrap());
    if hits.is_empty() {
        ui.add(
            egui::Label::new(
                rich(
                    "No topic matches. The chapter list on the left shows every chapter that mentions it.",
                    TextStyle::Body,
                )
                .color(p.text_tertiary),
            )
            .wrap(),
        );
    }
    for hit in hits.iter().take(12) {
        ui.add_space(space::s3());
        ui.add(
            egui::Label::new(rich(&hit.title, TextStyle::BodyStrong).color(p.text_primary)).wrap(),
        );
        ui.add(
            egui::Label::new(rich(&hit.signature, TextStyle::MonoSmall).color(p.text_tertiary))
                .wrap(),
        );
        ui.add(
            egui::Label::new(rich(&hit.summary, TextStyle::Body).color(p.text_secondary)).wrap(),
        );
        if let Some(snippet) = hit.code_preview {
            let preview: String = snippet.lines().take(8).collect::<Vec<_>>().join("\n");
            code_box(ui, &preview);
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::s2();
            if let Some(snippet) = hit.code_preview {
                if ui.add(Button::new("Copy").icon(i16::COPY)).clicked() {
                    ui.ctx().copy_text(snippet.to_owned());
                }
                if insert_button(ui, false) {
                    ui.ctx()
                        .data_mut(|d| d.insert_temp(article_insert_id(), snippet.to_owned()));
                }
            }
            if let Some(chapter) = HELP_CHAPTERS.get(hit.chapter_idx) {
                let label = format!("Open “{}”", plain_chapter_label(chapter.sidebar_label));
                if ui.add(Button::ghost(&label)).clicked() {
                    *nav_target = Some(hit.chapter_idx);
                }
            }
        });
    }
    ui.add_space(space::s4());
    c::divider(ui, false);
    ui.add_space(space::s4());
}

// ── Full NF2-Styled & SVG-Illustrated Browser HTML Generator ──

fn write_palette_css_vars(p: &Palette, out: &mut String) {
    let vars = [
        ("--bg-app", p.bg_app),
        ("--bg-panel", p.bg_panel),
        ("--bg-raised", p.bg_raised),
        ("--bg-field", p.bg_field),
        ("--bg-hover", p.bg_hover),
        ("--bg-selected", p.bg_selected),
        ("--bg-canvas", p.bg_canvas),
        ("--bg-stage", p.bg_stage),
        ("--border-subtle", p.border_subtle),
        ("--border-default", p.border_default),
        ("--border-strong", p.border_strong),
        ("--text-primary", p.text_primary),
        ("--text-secondary", p.text_secondary),
        ("--text-tertiary", p.text_tertiary),
        ("--text-on-accent", p.text_on_accent),
        ("--accent-default", p.accent_default),
        ("--accent-hover", p.accent_hover),
        ("--accent-subtle", p.accent_subtle),
        ("--warning-default", p.warning_default),
        ("--warning-subtle", p.warning_subtle),
        ("--success-default", p.success_default),
        ("--danger-default", p.danger_default),
        ("--guide", p.guide),
        ("--tl-static", p.timeline_static),
        ("--tl-motion", p.timeline_motion_tween),
        ("--tl-classic", p.timeline_classic_tween),
        ("--tl-shape", p.timeline_shape_tween),
        ("--syn-fn", p.syntax_function),
        ("--syn-kw", p.syntax_keyword),
        ("--syn-ty", p.syntax_type),
        ("--syn-str", p.syntax_string),
        ("--syn-cmt", p.syntax_comment),
    ];
    for (name, color) in vars {
        let _ = write!(out, "{name}:{};", css_rgb(color));
    }
}

fn convert_svg_var_attrs_to_css_style(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() + 512);
    let mut rest = raw;
    while let Some(lt) = rest.find('<') {
        out.push_str(&rest[..lt]);
        let after_lt = &rest[lt..];
        let Some(gt_rel) = after_lt.find('>') else {
            out.push_str(after_lt);
            return out;
        };
        let tag = &after_lt[..=gt_rel];
        rest = &after_lt[gt_rel + 1..];
        if tag.starts_with("</")
            || tag.starts_with("<!")
            || (!tag.contains("fill=\"var(") && !tag.contains("stroke=\"var("))
        {
            out.push_str(tag);
            continue;
        }
        let self_closing = tag.ends_with("/>");
        let inner_end = if self_closing {
            tag.len() - 2
        } else {
            tag.len() - 1
        };
        let mut inner = tag[1..inner_end].to_owned();
        let mut styles: Vec<String> = Vec::new();
        for attr in ["fill", "stroke"] {
            let needle = format!("{attr}=\"var(");
            if let Some(start) = inner.find(&needle) {
                let val_start = start + attr.len() + 2;
                if let Some(val_end_rel) = inner[val_start..].find('"') {
                    let val = inner[val_start..val_start + val_end_rel].to_owned();
                    styles.push(format!("{attr}:{val}"));
                    let remove_end = val_start + val_end_rel + 1;
                    let remove_start = if start > 0 && inner.as_bytes()[start - 1] == b' ' {
                        start - 1
                    } else {
                        start
                    };
                    inner.replace_range(remove_start..remove_end, "");
                }
            } else if attr == "fill"
                && let Some(start) = inner.find("fill=\"none\"")
            {
                styles.push("fill:none".to_owned());
                let remove_end = start + "fill=\"none\"".len();
                let remove_start = if start > 0 && inner.as_bytes()[start - 1] == b' ' {
                    start - 1
                } else {
                    start
                };
                inner.replace_range(remove_start..remove_end, "");
            }
        }
        out.push('<');
        out.push_str(inner.trim_end());
        if !styles.is_empty() {
            out.push_str(" style=\"");
            out.push_str(&styles.join(";"));
            out.push('"');
        }
        if self_closing {
            out.push_str("/>");
        } else {
            out.push('>');
        }
    }
    out.push_str(rest);
    out
}

fn write_chapter_svg_illustration(index: usize, out: &mut String) {
    let svg_start = out.len();
    let caption = match index {
        0 => {
            "Illustration 1.1: Stage inside Pasteboard overlaid with NF2 Rulers, Ruler Corner (0,0), Cyan Guides, Stage Grid, and Selection Snap Ring."
        }
        1 => {
            "Illustration 2.1: Visual comparison of selection chrome across the 5 Stage object regimes: Merge Shape, Object (J), Primitive, Symbol (F8), and Text Block."
        }
        2 => {
            "Illustration 3.1–3.3: Selection (V) curve reshaping, Subselection (A) Bézier anchor/tangent handles, Free Transform (Q) 4 sub-modes, and Lasso / Magic Wand."
        }
        3 => {
            "Illustration 4.1: All 9 Pen Tool (P) pointer badges and how each modifies the active or selected Bézier path."
        }
        4 => {
            "Illustration 5.1: NF2 Brush calligraphic stroke, Gradient Transform Tool (F) 5 handles, and the 5 Brush Paint Modes."
        }
        5 => {
            "Illustration 6.1: Button Symbol 4 interaction frames (Up, Over, Down, Hit) and 9-Slice Scaling (scale9Grid) guides."
        }
        6 => {
            "Illustration 7.1: Authentic NF2 Timeline spans (Static, Motion ◆, Classic, Shape) and keyframe markers."
        }
        7 => {
            "Illustration 8.1: Stage Motion Path with ◆ property keyframes, Shape Hints (a–z), and Cubic-Bézier Ease Curve."
        }
        8 => {
            "Illustration 9.1: Timeline Sound stereo waveform with 8-handle volume envelope and live filter / TLF text pipeline."
        }
        9 => {
            "Illustration 10.1: Multi-format Import pipeline, 4-language Scripting compiler/debugger, and HTML5 + WASM + Sprite Sheet publishing."
        }
        10 => {
            "Illustration 11.1–11.2: AVM1 Target Path Hierarchy (_level0 / _root → parent → child) and Flash Player Depth Zones (-16383..1048575)."
        }
        11 => {
            "Illustration 12.1: The 4 pillars of the ActionScript 2.0 runtime library: Display & Text, flash.geom & BitmapData, Input & Audio, and Data/Persistence."
        }
        12 => {
            "Illustration 13.1–13.2: ActionScript 3.0 flash.display 2-tier Inheritance Hierarchy (EventDispatcher → DisplayObject → InteractiveObject → DisplayObjectContainer → Sprite → MovieClip) and Local ↔ Global coordinate spaces."
        }
        13 => {
            "Illustration 14.1–14.2: W3C 3-Phase Event Propagation (Capture → Target → Bubbling) and deterministic 6-step Frame Tick Lifecycle."
        }
        14 => {
            "Illustration 15.1: AS3 flash.display.Graphics vector stream, live Filters/BitmapData pass, and SoundChannel / ByteArray / E4X pipeline."
        }
        15 => {
            "Illustration 16.1: Multi-Language Execution Bridge mapping AS1/AS2, AS3, JavaScript, and TypeScript to the shared Rust / WebAssembly runtime."
        }
        16 => {
            "Illustration 17.1–17.2: JavaScript execution contexts (Frame `this`, Object `event`, Project Module `import`/`export`) and deterministic JSX (`__nfJsx`) lowering."
        }
        17 => {
            "Illustration 18.1–18.2: NeoFlash TypeScript compilation pipeline (Parse → Built-In `ts_check` → Strip & Lower → SourceMap) and verified vs bounded checker scope."
        }
        _ => {
            "Illustration 19.1–19.2: Published HTML5 Browser Host architecture (DOM, CanvasRenderingContext2D, fetch, Web Audio, localStorage) and CORS/CSP/Autoplay security boundaries."
        }
    };
    let view_h = if index == 12 { 156 } else { 136 };
    let _ = write!(
        out,
        "<figure class=\"diagram-card\"><svg xmlns=\"http://www.w3.org/2000/svg\" class=\"chapter-svg\" width=\"680\" height=\"{view_h}\" viewBox=\"0 0 680 {view_h}\" role=\"img\" aria-label=\"Chapter {} illustration\">",
        index + 1
    );
    let _ = write!(
        out,
        "<rect x=\"1\" y=\"1\" width=\"678\" height=\"{}\" rx=\"8\" fill=\"var(--bg-canvas)\" stroke=\"var(--border-subtle)\"/>",
        view_h - 2
    );
    match index {
        0 => {
            out.push_str(
                "<rect x=\"48\" y=\"26\" width=\"584\" height=\"94\" fill=\"var(--bg-stage)\" stroke=\"var(--border-default)\"/>\
                 <rect x=\"12\" y=\"8\" width=\"620\" height=\"18\" fill=\"var(--bg-panel)\"/>\
                 <rect x=\"12\" y=\"26\" width=\"36\" height=\"94\" fill=\"var(--bg-panel)\"/>\
                 <line x1=\"48\" y1=\"68\" x2=\"632\" y2=\"68\" stroke=\"var(--guide)\" stroke-width=\"1.5\"/>\
                 <line x1=\"260\" y1=\"26\" x2=\"260\" y2=\"120\" stroke=\"var(--guide)\" stroke-width=\"1.5\"/>\
                 <rect x=\"218\" y=\"46\" width=\"84\" height=\"44\" rx=\"4\" fill=\"var(--accent-subtle)\" stroke=\"var(--accent-default)\" stroke-width=\"1.5\"/>\
                 <circle cx=\"260\" cy=\"68\" r=\"6\" fill=\"none\" stroke=\"var(--accent-default)\" stroke-width=\"2\"/>\
                 <text x=\"316\" y=\"64\" fill=\"var(--text-primary)\" font-size=\"11\" font-family=\"monospace\">Stage + Rulers (0,0) + Guides + Snap Ring</text>",
            );
        }
        1 => {
            for (i, label) in [
                "1. Merge Shape",
                "2. Object (J)",
                "3. Primitive",
                "4. Symbol (F8)",
                "5. Text Block",
            ]
            .iter()
            .enumerate()
            {
                let x = 16 + i * 130;
                let _ = write!(
                    out,
                    "<rect x=\"{x}\" y=\"18\" width=\"118\" height=\"76\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--border-subtle)\"/>\
                     <rect x=\"{}\" y=\"32\" width=\"86\" height=\"44\" rx=\"3\" fill=\"var(--accent-subtle)\" stroke=\"var(--accent-default)\"/>\
                     <text x=\"{}\" y=\"114\" text-anchor=\"middle\" fill=\"var(--text-primary)\" font-size=\"11\">{label}</text>",
                    x + 16,
                    x + 59
                );
            }
        }
        2 => {
            out.push_str(
                "<path d=\"M 44 96 Q 170 18 296 96\" fill=\"none\" stroke=\"var(--accent-default)\" stroke-width=\"2.5\"/>\
                 <circle cx=\"170\" cy=\"56\" r=\"5\" fill=\"var(--warning-default)\"/>\
                 <line x1=\"340\" y1=\"12\" x2=\"340\" y2=\"124\" stroke=\"var(--border-subtle)\"/>\
                 <line x1=\"400\" y1=\"92\" x2=\"510\" y2=\"44\" stroke=\"var(--accent-default)\" stroke-width=\"2\"/>\
                 <line x1=\"510\" y1=\"44\" x2=\"620\" y2=\"92\" stroke=\"var(--accent-default)\" stroke-width=\"2\"/>\
                 <line x1=\"456\" y1=\"24\" x2=\"564\" y2=\"64\" stroke=\"var(--guide)\" stroke-width=\"1.2\"/>\
                 <rect x=\"506\" y=\"40\" width=\"8\" height=\"8\" fill=\"var(--accent-default)\"/>\
                 <text x=\"170\" y=\"120\" text-anchor=\"middle\" fill=\"var(--text-primary)\" font-size=\"11\">Selection (V) Curve Drag vs Subselection (A) Bézier Nodes</text>\
                 <text x=\"510\" y=\"120\" text-anchor=\"middle\" fill=\"var(--text-primary)\" font-size=\"11\">Free Transform (Q) · Scale · Rotate · Distort · Envelope</text>",
            );
        }
        3 => {
            for (i, state) in [
                "1. Pen x (Start)",
                "2. Pen (Next)",
                "3. Pen + (Add)",
                "4. Pen - (Del)",
                "5. Pen o (Close)",
                "6. Convert (^)",
            ]
            .iter()
            .enumerate()
            {
                let col = i % 3;
                let row = i / 3;
                let x = 20 + col * 216;
                let y = 18 + row * 54;
                let _ = write!(
                    out,
                    "<rect x=\"{x}\" y=\"{y}\" width=\"200\" height=\"42\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--border-default)\"/>\
                     <text x=\"{}\" y=\"{}\" fill=\"var(--text-primary)\" font-size=\"11\" font-family=\"monospace\">{state}</text>",
                    x + 14,
                    y + 25
                );
            }
        }
        4 => {
            out.push_str(
                "<rect x=\"20\" y=\"22\" width=\"190\" height=\"92\" rx=\"6\" fill=\"var(--bg-panel)\" stroke=\"var(--border-subtle)\"/>\
                 <path d=\"M 42 84 C 82 32, 138 104, 188 48\" fill=\"none\" stroke=\"var(--accent-default)\" stroke-width=\"10\" stroke-linecap=\"round\"/>\
                 <circle cx=\"336\" cy=\"68\" r=\"40\" fill=\"var(--accent-subtle)\" stroke=\"var(--accent-default)\" stroke-width=\"1.5\"/>\
                 <line x1=\"296\" y1=\"68\" x2=\"376\" y2=\"68\" stroke=\"var(--guide)\" stroke-width=\"1.5\"/>\
                 <rect x=\"448\" y=\"22\" width=\"212\" height=\"92\" rx=\"6\" fill=\"var(--bg-panel)\" stroke=\"var(--border-subtle)\"/>\
                 <text x=\"462\" y=\"48\" fill=\"var(--text-primary)\" font-size=\"11\">5 Brush Modes: Normal · Fills</text>\
                 <text x=\"462\" y=\"68\" fill=\"var(--text-primary)\" font-size=\"11\">Behind · Selection · Inside</text>\
                 <text x=\"462\" y=\"92\" fill=\"var(--text-secondary)\" font-size=\"11\">Gradient Transform (F): 5 Handles</text>",
            );
        }
        5 => {
            for (i, st) in ["Up", "Over", "Down", "Hit"].iter().enumerate() {
                let x = 32 + i * 68;
                let _ = write!(
                    out,
                    "<rect x=\"{x}\" y=\"44\" width=\"64\" height=\"36\" fill=\"var(--bg-panel)\" stroke=\"var(--border-default)\"/>\
                     <circle cx=\"{}\" cy=\"62\" r=\"4\" fill=\"var(--accent-default)\"/>\
                     <text x=\"{}\" y=\"34\" text-anchor=\"middle\" fill=\"var(--text-primary)\" font-size=\"11\">{st}</text>",
                    x + 32,
                    x + 32
                );
            }
            out.push_str(
                "<rect x=\"388\" y=\"26\" width=\"248\" height=\"84\" rx=\"10\" fill=\"var(--accent-subtle)\" stroke=\"var(--accent-default)\" stroke-width=\"1.5\"/>\
                 <line x1=\"436\" y1=\"20\" x2=\"436\" y2=\"116\" stroke=\"var(--guide)\" stroke-dasharray=\"4 3\"/>\
                 <line x1=\"588\" y1=\"20\" x2=\"588\" y2=\"116\" stroke=\"var(--guide)\" stroke-dasharray=\"4 3\"/>\
                 <line x1=\"380\" y1=\"48\" x2=\"644\" y2=\"48\" stroke=\"var(--guide)\" stroke-dasharray=\"4 3\"/>\
                 <line x1=\"380\" y1=\"88\" x2=\"644\" y2=\"88\" stroke=\"var(--guide)\" stroke-dasharray=\"4 3\"/>\
                 <text x=\"512\" y=\"72\" text-anchor=\"middle\" fill=\"var(--text-primary)\" font-size=\"11\" font-family=\"monospace\">9-Slice (scale9Grid)</text>",
            );
        }
        6 => {
            for (row_i, (label, fill_var)) in [
                ("Actions (a)", "var(--tl-static)"),
                ("Motion (◆)", "var(--tl-motion)"),
                ("Classic (→)", "var(--tl-classic)"),
                ("Shape (→)", "var(--tl-shape)"),
            ]
            .iter()
            .enumerate()
            {
                let y = 16 + row_i * 26;
                let _ = write!(
                    out,
                    "<rect x=\"18\" y=\"{y}\" width=\"128\" height=\"22\" fill=\"var(--bg-panel)\"/>\
                     <text x=\"26\" y=\"{}\" fill=\"var(--text-primary)\" font-size=\"11\">{label}</text>\
                     <rect x=\"152\" y=\"{y}\" width=\"508\" height=\"22\" fill=\"{fill_var}\" stroke=\"var(--border-subtle)\"/>\
                     <circle cx=\"166\" cy=\"{}\" r=\"3.5\" fill=\"var(--text-primary)\"/>\
                     <circle cx=\"642\" cy=\"{}\" r=\"3.5\" fill=\"var(--text-primary)\"/>",
                    y + 15,
                    y + 11,
                    y + 11
                );
            }
        }
        7 => {
            out.push_str(
                "<path d=\"M 36 104 Q 124 22 216 100\" fill=\"none\" stroke=\"var(--accent-default)\" stroke-width=\"2\" stroke-dasharray=\"4 3\"/>\
                 <polygon points=\"124,54 130,60 124,66 118,60\" fill=\"var(--warning-default)\"/>\
                 <rect x=\"256\" y=\"32\" width=\"164\" height=\"72\" rx=\"6\" fill=\"var(--accent-subtle)\" stroke=\"var(--accent-default)\"/>\
                 <circle cx=\"256\" cy=\"32\" r=\"9\" fill=\"var(--warning-default)\"/>\
                 <circle cx=\"420\" cy=\"32\" r=\"9\" fill=\"var(--success-default)\"/>\
                 <rect x=\"456\" y=\"24\" width=\"196\" height=\"88\" rx=\"4\" fill=\"var(--bg-panel)\" stroke=\"var(--border-subtle)\"/>\
                 <path d=\"M 468 102 C 520 102, 580 34, 640 34\" fill=\"none\" stroke=\"var(--accent-default)\" stroke-width=\"2\"/>\
                 <text x=\"338\" y=\"72\" text-anchor=\"middle\" fill=\"var(--text-primary)\" font-size=\"11\">Shape Hints (a-z) &amp; Ease Curve</text>",
            );
        }
        8 => {
            out.push_str(
                "<rect x=\"24\" y=\"24\" width=\"632\" height=\"88\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--border-default)\"/>\
                 <polyline points=\"36,96 140,38 510,44 644,94\" fill=\"none\" stroke=\"var(--accent-default)\" stroke-width=\"2\"/>\
                 <rect x=\"136\" y=\"34\" width=\"8\" height=\"8\" fill=\"var(--accent-default)\"/>\
                 <rect x=\"506\" y=\"40\" width=\"8\" height=\"8\" fill=\"var(--accent-default)\"/>\
                 <text x=\"340\" y=\"72\" text-anchor=\"middle\" fill=\"var(--text-primary)\" font-size=\"11\">7 Live Filters · 14 Blend Modes · TLF Multi-Column Text · 8-Point Stereo Sound Envelope</text>",
            );
        }
        9 => {
            out.push_str(
                "<rect x=\"24\" y=\"26\" width=\"196\" height=\"84\" rx=\"6\" fill=\"var(--bg-panel)\" stroke=\"var(--accent-default)\"/>\
                 <text x=\"122\" y=\"58\" text-anchor=\"middle\" fill=\"var(--accent-default)\" font-size=\"12\">FLA / XFL / SWF / PSD</text>\
                 <text x=\"122\" y=\"80\" text-anchor=\"middle\" fill=\"var(--text-secondary)\" font-size=\"11\">Importer + Diagnostics</text>\
                 <rect x=\"242\" y=\"26\" width=\"196\" height=\"84\" rx=\"6\" fill=\"var(--bg-panel)\" stroke=\"var(--accent-default)\"/>\
                 <text x=\"340\" y=\"58\" text-anchor=\"middle\" fill=\"var(--accent-default)\" font-size=\"12\">AS2 · AS3 · JS · TS</text>\
                 <text x=\"340\" y=\"80\" text-anchor=\"middle\" fill=\"var(--text-secondary)\" font-size=\"11\">Compiler + Debugger</text>\
                 <rect x=\"460\" y=\"26\" width=\"196\" height=\"84\" rx=\"6\" fill=\"var(--accent-subtle)\" stroke=\"var(--accent-default)\"/>\
                 <text x=\"558\" y=\"58\" text-anchor=\"middle\" fill=\"var(--text-primary)\" font-size=\"12\">HTML5 + WASM + Atlas</text>\
                 <text x=\"558\" y=\"80\" text-anchor=\"middle\" fill=\"var(--text-secondary)\" font-size=\"11\">Lottie · SVG · MP4 · WAV</text>",
            );
        }
        10 => {
            out.push_str(
                "<rect x=\"20\" y=\"26\" width=\"194\" height=\"84\" rx=\"6\" fill=\"var(--bg-panel)\" stroke=\"var(--border-default)\"/>\
                 <text x=\"32\" y=\"50\" fill=\"var(--accent-default)\" font-size=\"11\" font-family=\"monospace\">_level0 / _root</text>\
                 <text x=\"32\" y=\"72\" fill=\"var(--text-primary)\" font-size=\"11\">stop(); gotoAndPlay(2);</text>\
                 <text x=\"32\" y=\"92\" fill=\"var(--syn-fn)\" font-size=\"10\" font-family=\"monospace\">attachMovie(\"Id\", \"n\", d)</text>\
                 <line x1=\"214\" y1=\"68\" x2=\"244\" y2=\"68\" stroke=\"var(--accent-default)\" stroke-width=\"2\"/>\
                 <rect x=\"244\" y=\"26\" width=\"194\" height=\"84\" rx=\"6\" fill=\"var(--bg-panel)\" stroke=\"var(--accent-default)\" stroke-width=\"1.5\"/>\
                 <text x=\"256\" y=\"50\" fill=\"var(--accent-default)\" font-size=\"11\" font-family=\"monospace\">_root.hero_mc (this)</text>\
                 <text x=\"256\" y=\"72\" fill=\"var(--text-primary)\" font-size=\"11\">onClipEvent(enterFrame)</text>\
                 <text x=\"256\" y=\"92\" fill=\"var(--syn-fn)\" font-size=\"10\" font-family=\"monospace\">this.onRelease = fn;</text>\
                 <line x1=\"438\" y1=\"68\" x2=\"468\" y2=\"68\" stroke=\"var(--warning-default)\" stroke-width=\"2\"/>\
                 <rect x=\"468\" y=\"26\" width=\"192\" height=\"84\" rx=\"6\" fill=\"var(--bg-panel)\" stroke=\"var(--border-default)\"/>\
                 <text x=\"480\" y=\"50\" fill=\"var(--warning-default)\" font-size=\"11\" font-family=\"monospace\">hero_mc.blade_mc</text>\
                 <text x=\"480\" y=\"72\" fill=\"var(--text-primary)\" font-size=\"11\">_parent._x += 4;</text>\
                 <text x=\"480\" y=\"92\" fill=\"var(--syn-fn)\" font-size=\"10\" font-family=\"monospace\">swapDepths / remove</text>",
            );
        }
        11 => {
            for (i, (head, b1, b2)) in [
                (
                    "Display & Text",
                    "MovieClip · Button",
                    "TextField · Stage · Color",
                ),
                (
                    "flash.geom & Pixels",
                    "BitmapData (draw)",
                    "Matrix · Point · Rect",
                ),
                ("Input & Audio", "Key · Mouse", "Sound · Selection"),
                ("Net & Storage", "LoadVars · XML", "SharedObject.getLocal"),
            ]
            .iter()
            .enumerate()
            {
                let x = 16 + i * 164;
                let _ = write!(
                    out,
                    "<rect x=\"{x}\" y=\"22\" width=\"152\" height=\"92\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--border-default)\"/>\
                     <text x=\"{}\" y=\"44\" fill=\"var(--accent-default)\" font-size=\"11\" font-weight=\"600\">{head}</text>\
                     <text x=\"{}\" y=\"68\" fill=\"var(--text-primary)\" font-size=\"10\" font-family=\"monospace\">{b1}</text>\
                     <text x=\"{}\" y=\"88\" fill=\"var(--text-secondary)\" font-size=\"10\" font-family=\"monospace\">{b2}</text>",
                    x + 10,
                    x + 10,
                    x + 10
                );
            }
        }
        12 => {
            // 2-tier non-overlapping AS3 Display List Hierarchy + 4 leaf pills
            for (i, (cls, mem)) in [
                ("1. EventDispatcher", "addEventListener / dispatchEvent"),
                ("2. DisplayObject", "x, y, scaleX, rotation, alpha"),
                ("3. InteractiveObject", "mouseEnabled, tabEnabled"),
            ]
            .iter()
            .enumerate()
            {
                let x = 14 + i * 218;
                let _ = write!(
                    out,
                    "<rect x=\"{x}\" y=\"12\" width=\"206\" height=\"42\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--border-default)\"/>\
                     <text x=\"{}\" y=\"29\" text-anchor=\"middle\" fill=\"var(--accent-default)\" font-size=\"11\" font-weight=\"600\">{cls}</text>\
                     <text x=\"{}\" y=\"45\" text-anchor=\"middle\" fill=\"var(--text-secondary)\" font-size=\"9.5\" font-family=\"monospace\">{mem}</text>",
                    x + 103,
                    x + 103
                );
            }
            for (i, (cls, mem)) in [
                (
                    "4. DisplayObjectContainer",
                    "addChild, removeChild, numChildren",
                ),
                (
                    "5. Sprite → MovieClip",
                    "graphics, buttonMode, currentFrame",
                ),
            ]
            .iter()
            .enumerate()
            {
                let x = 14 + i * 332;
                let _ = write!(
                    out,
                    "<rect x=\"{x}\" y=\"62\" width=\"320\" height=\"42\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--accent-default)\" stroke-width=\"1.4\"/>\
                     <text x=\"{}\" y=\"79\" text-anchor=\"middle\" fill=\"var(--accent-default)\" font-size=\"11\" font-weight=\"600\">{cls}</text>\
                     <text x=\"{}\" y=\"95\" text-anchor=\"middle\" fill=\"var(--text-secondary)\" font-size=\"9.5\" font-family=\"monospace\">{mem}</text>",
                    x + 160,
                    x + 160
                );
            }
            for (i, leaf) in [
                "Shape (vector leaf)",
                "Bitmap (BitmapData)",
                "TextField (text/input)",
                "SimpleButton (4-state)",
            ]
            .iter()
            .enumerate()
            {
                let x = 14 + i * 166;
                let _ = write!(
                    out,
                    "<rect x=\"{x}\" y=\"114\" width=\"154\" height=\"28\" rx=\"4\" fill=\"var(--accent-subtle)\"/>\
                     <text x=\"{}\" y=\"132\" text-anchor=\"middle\" fill=\"var(--text-primary)\" font-size=\"10\" font-family=\"monospace\">{leaf}</text>",
                    x + 77
                );
            }
        }
        13 => {
            out.push_str(
                "<line x1=\"40\" y1=\"28\" x2=\"640\" y2=\"28\" stroke=\"var(--guide)\" stroke-width=\"2\"/>\
                 <text x=\"340\" y=\"18\" text-anchor=\"middle\" fill=\"var(--guide)\" font-size=\"11\" font-family=\"monospace\">Phase 1: CAPTURING_PHASE (Stage -&gt; Parent, useCapture=true)</text>\
                 <rect x=\"40\" y=\"42\" width=\"170\" height=\"46\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--accent-default)\"/>\
                 <text x=\"125\" y=\"69\" text-anchor=\"middle\" fill=\"var(--text-primary)\" font-size=\"11\">1. Stage / Root</text>\
                 <rect x=\"255\" y=\"42\" width=\"170\" height=\"46\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--accent-default)\"/>\
                 <text x=\"340\" y=\"69\" text-anchor=\"middle\" fill=\"var(--text-primary)\" font-size=\"11\">2. Parent Sprite</text>\
                 <rect x=\"470\" y=\"42\" width=\"170\" height=\"46\" rx=\"5\" fill=\"var(--warning-subtle)\" stroke=\"var(--warning-default)\" stroke-width=\"1.5\"/>\
                 <text x=\"555\" y=\"69\" text-anchor=\"middle\" fill=\"var(--warning-default)\" font-size=\"11\">3. Target (AT_TARGET)</text>\
                 <line x1=\"640\" y1=\"104\" x2=\"40\" y2=\"104\" stroke=\"var(--success-default)\" stroke-width=\"2\"/>\
                 <text x=\"340\" y=\"122\" text-anchor=\"middle\" fill=\"var(--success-default)\" font-size=\"11\" font-family=\"monospace\">Phase 3: BUBBLING_PHASE (Target -&gt; Stage, bubbles=true)</text>",
            );
        }
        14 => {
            out.push_str(
                "<rect x=\"18\" y=\"20\" width=\"204\" height=\"96\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--accent-default)\"/>\
                 <text x=\"30\" y=\"44\" fill=\"var(--accent-default)\" font-size=\"11\" font-weight=\"600\">1. Graphics Vector API</text>\
                 <text x=\"30\" y=\"66\" fill=\"var(--text-primary)\" font-size=\"10\" font-family=\"monospace\">beginGradientFill / lineStyle</text>\
                 <text x=\"30\" y=\"86\" fill=\"var(--text-primary)\" font-size=\"10\" font-family=\"monospace\">curveTo / cubicCurveTo / Path</text>\
                 <rect x=\"238\" y=\"20\" width=\"204\" height=\"96\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--warning-default)\"/>\
                 <text x=\"250\" y=\"44\" fill=\"var(--warning-default)\" font-size=\"11\" font-weight=\"600\">2. Filters &amp; Geometry</text>\
                 <text x=\"250\" y=\"66\" fill=\"var(--text-primary)\" font-size=\"10\" font-family=\"monospace\">Blur / Glow / DropShadow</text>\
                 <text x=\"250\" y=\"86\" fill=\"var(--text-primary)\" font-size=\"10\" font-family=\"monospace\">Matrix / ColorTransform</text>\
                 <rect x=\"458\" y=\"20\" width=\"204\" height=\"96\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--success-default)\"/>\
                 <text x=\"470\" y=\"44\" fill=\"var(--success-default)\" font-size=\"11\" font-weight=\"600\">3. Sound &amp; ByteArray</text>\
                 <text x=\"470\" y=\"66\" fill=\"var(--text-primary)\" font-size=\"10\" font-family=\"monospace\">SoundChannel / SoundMixer</text>\
                 <text x=\"470\" y=\"86\" fill=\"var(--text-primary)\" font-size=\"10\" font-family=\"monospace\">ByteArray / JSON / XML E4X</text>",
            );
        }
        15 => {
            out.push_str(
                "<rect x=\"18\" y=\"18\" width=\"204\" height=\"52\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--accent-default)\"/>\
                 <text x=\"120\" y=\"40\" text-anchor=\"middle\" fill=\"var(--accent-default)\" font-size=\"11\" font-weight=\"600\">AS1 / AS2 (AVM1)</text>\
                 <text x=\"120\" y=\"58\" text-anchor=\"middle\" fill=\"var(--text-secondary)\" font-size=\"10\" font-family=\"monospace\">_x · _alpha(100) · onRelease</text>\
                 <rect x=\"238\" y=\"18\" width=\"204\" height=\"52\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--accent-default)\"/>\
                 <text x=\"340\" y=\"40\" text-anchor=\"middle\" fill=\"var(--accent-default)\" font-size=\"11\" font-weight=\"600\">AS3 (AVM2)</text>\
                 <text x=\"340\" y=\"58\" text-anchor=\"middle\" fill=\"var(--text-secondary)\" font-size=\"10\" font-family=\"monospace\">x · alpha(1.0) · addEventListener</text>\
                 <rect x=\"458\" y=\"18\" width=\"204\" height=\"52\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--accent-default)\"/>\
                 <text x=\"560\" y=\"40\" text-anchor=\"middle\" fill=\"var(--accent-default)\" font-size=\"11\" font-weight=\"600\">JavaScript / TypeScript</text>\
                 <text x=\"560\" y=\"58\" text-anchor=\"middle\" fill=\"var(--text-secondary)\" font-size=\"10\" font-family=\"monospace\">this · mc.x · addEventListener</text>\
                 <rect x=\"18\" y=\"82\" width=\"644\" height=\"36\" rx=\"5\" fill=\"var(--accent-subtle)\" stroke=\"var(--accent-default)\"/>\
                 <text x=\"340\" y=\"104\" text-anchor=\"middle\" fill=\"var(--text-primary)\" font-size=\"11\" font-weight=\"600\">Shared NeoFlash Rust / WebAssembly Scene Graph &amp; Timeline Engine</text>",
            );
        }
        16 => {
            out.push_str(
                "<rect x=\"18\" y=\"18\" width=\"204\" height=\"68\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--accent-default)\"/>\
                 <text x=\"30\" y=\"38\" fill=\"var(--accent-default)\" font-size=\"11\" font-weight=\"600\">1. Frame &amp; Object Context</text>\
                 <text x=\"30\" y=\"56\" fill=\"var(--text-primary)\" font-size=\"10\" font-family=\"monospace\">this · event · trace()</text>\
                 <text x=\"30\" y=\"74\" fill=\"var(--syn-fn)\" font-size=\"10\" font-family=\"monospace\">play / stop / gotoAndPlay</text>\
                 <rect x=\"238\" y=\"18\" width=\"204\" height=\"68\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--warning-default)\"/>\
                 <text x=\"250\" y=\"38\" fill=\"var(--warning-default)\" font-size=\"11\" font-weight=\"600\">2. Project Modules &amp; JSX</text>\
                 <text x=\"250\" y=\"56\" fill=\"var(--text-primary)\" font-size=\"10\" font-family=\"monospace\">import / export (./module)</text>\
                 <text x=\"250\" y=\"74\" fill=\"var(--syn-fn)\" font-size=\"10\" font-family=\"monospace\">&lt;Tag/&gt; -&gt; __nfJsx(...)</text>\
                 <rect x=\"458\" y=\"18\" width=\"204\" height=\"68\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--success-default)\"/>\
                 <text x=\"470\" y=\"38\" fill=\"var(--success-default)\" font-size=\"11\" font-weight=\"600\">3. ES2023+ &amp; Async</text>\
                 <text x=\"470\" y=\"56\" fill=\"var(--text-primary)\" font-size=\"10\" font-family=\"monospace\">Promise · async / await</text>\
                 <text x=\"470\" y=\"74\" fill=\"var(--syn-fn)\" font-size=\"10\" font-family=\"monospace\">Map · Set · JSON · Math</text>\
                 <rect x=\"18\" y=\"94\" width=\"644\" height=\"28\" rx=\"4\" fill=\"var(--accent-subtle)\"/>\
                 <text x=\"340\" y=\"112\" text-anchor=\"middle\" fill=\"var(--text-primary)\" font-size=\"10.5\" font-family=\"monospace\">Provenance: [NeoFlash binding] · [Project module] · [ECMAScript] · [Browser host]</text>",
            );
        }
        17 => {
            out.push_str(
                "<rect x=\"16\" y=\"18\" width=\"152\" height=\"68\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--accent-default)\"/>\
                 <text x=\"26\" y=\"40\" fill=\"var(--accent-default)\" font-size=\"11\" font-weight=\"600\">1. Parse (.ts/.tsx)</text>\
                 <text x=\"26\" y=\"60\" fill=\"var(--text-primary)\" font-size=\"10\" font-family=\"monospace\">interface / type / enum</text>\
                 <rect x=\"180\" y=\"18\" width=\"152\" height=\"68\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--warning-default)\"/>\
                 <text x=\"190\" y=\"40\" fill=\"var(--warning-default)\" font-size=\"11\" font-weight=\"600\">2. Built-In ts_check</text>\
                 <text x=\"190\" y=\"60\" fill=\"var(--text-primary)\" font-size=\"10\" font-family=\"monospace\">Members / Unions / Arity</text>\
                 <rect x=\"344\" y=\"18\" width=\"152\" height=\"68\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--success-default)\"/>\
                 <text x=\"354\" y=\"40\" fill=\"var(--success-default)\" font-size=\"11\" font-weight=\"600\">3. Strip &amp; Lower</text>\
                 <text x=\"354\" y=\"60\" fill=\"var(--text-primary)\" font-size=\"10\" font-family=\"monospace\">Erase types + lower enum</text>\
                 <rect x=\"508\" y=\"18\" width=\"154\" height=\"68\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--accent-default)\"/>\
                 <text x=\"518\" y=\"40\" fill=\"var(--accent-default)\" font-size=\"11\" font-weight=\"600\">4. SourceMap</text>\
                 <text x=\"518\" y=\"60\" fill=\"var(--text-primary)\" font-size=\"10\" font-family=\"monospace\">1-based line &amp; col</text>\
                 <rect x=\"16\" y=\"94\" width=\"646\" height=\"28\" rx=\"4\" fill=\"var(--accent-subtle)\"/>\
                 <text x=\"340\" y=\"112\" text-anchor=\"middle\" fill=\"var(--text-primary)\" font-size=\"10.5\" font-family=\"monospace\">Utilities: Array&lt;T&gt; · Promise&lt;T&gt; · Record&lt;K,V&gt; · Partial&lt;T&gt; · Pick&lt;T,K&gt; · Omit&lt;T,K&gt;</text>",
            );
        }
        _ => {
            out.push_str(
                "<rect x=\"18\" y=\"18\" width=\"204\" height=\"68\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--accent-default)\"/>\
                 <text x=\"30\" y=\"38\" fill=\"var(--accent-default)\" font-size=\"11\" font-weight=\"600\">1. DOM &amp; Canvas 2D</text>\
                 <text x=\"30\" y=\"56\" fill=\"var(--text-primary)\" font-size=\"10\" font-family=\"monospace\">CanvasRenderingContext2D</text>\
                 <text x=\"30\" y=\"74\" fill=\"var(--syn-fn)\" font-size=\"10\" font-family=\"monospace\">fillRect · arc · drawImage</text>\
                 <rect x=\"238\" y=\"18\" width=\"204\" height=\"68\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--warning-default)\"/>\
                 <text x=\"250\" y=\"38\" fill=\"var(--warning-default)\" font-size=\"11\" font-weight=\"600\">2. Fetch &amp; Storage</text>\
                 <text x=\"250\" y=\"56\" fill=\"var(--text-primary)\" font-size=\"10\" font-family=\"monospace\">await fetch(url) (CORS)</text>\
                 <text x=\"250\" y=\"74\" fill=\"var(--syn-fn)\" font-size=\"10\" font-family=\"monospace\">localStorage.getItem/setItem</text>\
                 <rect x=\"458\" y=\"18\" width=\"204\" height=\"68\" rx=\"5\" fill=\"var(--bg-panel)\" stroke=\"var(--success-default)\"/>\
                 <text x=\"470\" y=\"38\" fill=\"var(--success-default)\" font-size=\"11\" font-weight=\"600\">3. Web Audio &amp; Timers</text>\
                 <text x=\"470\" y=\"56\" fill=\"var(--text-primary)\" font-size=\"10\" font-family=\"monospace\">AudioContext (user gesture)</text>\
                 <text x=\"470\" y=\"74\" fill=\"var(--syn-fn)\" font-size=\"10\" font-family=\"monospace\">requestAnimationFrame</text>\
                 <rect x=\"18\" y=\"94\" width=\"644\" height=\"28\" rx=\"4\" fill=\"var(--warning-subtle)\"/>\
                 <text x=\"340\" y=\"112\" text-anchor=\"middle\" fill=\"var(--text-primary)\" font-size=\"10.5\" font-family=\"monospace\">Browser Boundaries: HTTP/HTTPS CORS · CSP connect-src · Autoplay Gesture · ~5 MB Origin Quota</text>",
            );
        }
    }
    out.push_str("</svg><figcaption class=\"diagram-caption\">");
    super::help_manual::escape_html(caption, out);
    out.push_str("</figcaption></figure>");
    let normalized = convert_svg_var_attrs_to_css_style(&out[svg_start..]);
    out.replace_range(svg_start.., &normalized);
}

fn write_as_chapter_reference_html(index: usize, out: &mut String) {
    match index {
        9 => {
            #[cfg(target_os = "macos")]
            const INSTALL_ROW: &str = "<tr><td><code>Install &amp; Manual Update</code></td><td>Replace <code>NeoFlash.app</code> in <code>/Applications</code></td><td>Preferences, custom workspaces, recent files, and crash-recovery snapshots in <code>~/Library/Application Support/NeoFlash</code> are preserved across manual updates.</td></tr>";
            #[cfg(target_os = "windows")]
            const INSTALL_ROW: &str = "<tr><td><code>Install &amp; Manual Update</code></td><td>Extract the Windows x64 portable ZIP and launch <code>neoflash.exe</code></td><td>Preferences, custom workspaces, recent files, and crash-recovery snapshots in <code>%APPDATA%\\NeoFlash</code> are preserved across manual updates.</td></tr>";
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            const INSTALL_ROW: &str = "<tr><td><code>Install &amp; Manual Update</code></td><td>Extract the Linux x64 portable tar.gz archive and run <code>neoflash</code></td><td>Preferences, custom workspaces, recent files, and crash-recovery snapshots in <code>~/.local/share/NeoFlash</code> are preserved across manual updates.</td></tr>";
            #[cfg(target_os = "macos")]
            const MEDIA_ROW: &str = "<tr><td><code>Bundled Media &amp; Licences</code></td><td><code>Contents/Resources/media/bin/</code></td><td><code>ffmpeg</code>/<code>ffprobe</code>, exact application/Rust/media licence evidence and checksum manifests are included; corresponding source is supplied beside the platform archive. Set <code>NEOFLASH_FFMPEG</code> / <code>NEOFLASH_FFPROBE</code> only for custom overrides.</td></tr>";
            #[cfg(not(target_os = "macos"))]
            const MEDIA_ROW: &str = "<tr><td><code>Bundled Media &amp; Licences</code></td><td><code>media/bin/</code></td><td><code>ffmpeg</code>/<code>ffprobe</code>, exact application/Rust/media licence evidence and checksum manifests are included; corresponding source is supplied beside the platform archive. Set <code>NEOFLASH_FFMPEG</code> / <code>NEOFLASH_FFPROBE</code> only for custom overrides.</td></tr>";

            out.push_str(
                "<h3>NeoFlash 0.1.0-beta.1 — Release Identity, Install, Manual Update, Media &amp; Known Issues</h3>\
                 <table class=\"ref-table\"><thead><tr><th>Area</th><th>0.1.0-beta.1 Specification</th><th>Details &amp; Operational Guidance</th></tr></thead><tbody>\
                 <tr><td><code>Release Identity</code></td><td><code>NeoFlash 0.1.0-beta.1</code> · macOS 15+ Apple Silicon · Windows/Linux x64 Technical Preview</td><td>Windows is an unsigned portable ZIP; Linux is an Ubuntu 24.04-compatible portable tar.gz.</td></tr>",
            );
            out.push_str(INSTALL_ROW);
            out.push_str(MEDIA_ROW);
            out.push_str(
                "<tr><td><code>Backups &amp; Bug Reports</code></td><td>Versioned <code>.nflash</code> JSON + <code>About NeoFlash</code></td><td>Keep <code>.nflash</code> projects in Git or regular file backups. Attach a minimal <code>.nflash</code> repro, <code>Output</code> panel diagnostics, and the build commit from <code>NeoFlash &gt; About NeoFlash</code> when reporting bugs.</td></tr>\
                 <tr><td><code>Known Issues &amp; Limits</code></td><td>VoiceOver, Intel Macs, updates, Windows / Linux previews, graphics tablets</td><td>VoiceOver cannot read the editor interface yet. There is no build for Intel (x86_64) Macs. Updates are installed manually. The Windows preview is unsigned, so SmartScreen may warn before the first launch, and it has no connected debugger. The Linux preview is a portable archive tested on Ubuntu 24.04 x64 with X11 and Wayland, not an AppImage or deb package. Windows and Linux have had less testing than macOS. Import of legacy Flash files is partial. Graphics-tablet support is experimental.</td></tr>\
                 </tbody></table>\
                 <h3>Classic Flash &lt;ESC&gt; Two-Key Code Chords in Actions (All 50 Chords)</h3>\
                 <table class=\"ref-table\"><thead><tr><th>Chord</th><th>Inserted Template</th><th>Purpose</th></tr></thead><tbody>",
            );
            for &(chord, expansion, desc) in &CLASSIC_ESC_CHORDS {
                out.push_str("<tr><td><code>&lt;ESC&gt;");
                super::help_manual::escape_html(chord, out);
                out.push_str("</code></td><td><code>");
                super::help_manual::escape_html(expansion, out);
                out.push_str("</code></td><td>");
                super::help_manual::escape_html(desc, out);
                out.push_str("</td></tr>");
            }
            out.push_str("</tbody></table>");
        }
        10 => {
            out.push_str(
                "<h3>AS1 / AS2 Timeline, function Statement, arguments &amp; Function Class Reference</h3>\
                 <table class=\"ref-table\"><thead><tr><th>Construct / API</th><th>Scope &amp; Signature</th><th>Behavior</th></tr></thead><tbody>\
                 <tr><td><code>function name(p:Type):Return</code></td><td>Hoisted named declaration</td><td>Callable before or after its definition on the same frame; <code>return expr;</code> exits immediately (<code>Void</code> / omitted returns <code>undefined</code>).</td></tr>\
                 <tr><td><code>var fn:Function = function(...)</code></td><td>Anonymous expression</td><td>Evaluated at assignment time; passed as first-class callbacks to <code>Array.sort</code>, <code>setInterval</code>, and event handlers.</td></tr>\
                 <tr><td><code>arguments.length / callee / caller</code></td><td>Local call activation</td><td>Variadic argument array, anonymous self-recursion via <code>arguments.callee</code>, and caller inspection via <code>arguments.caller</code>.</td></tr>\
                 <tr><td><code>Function.call / Function.apply</code></td><td><code>fn.call(thisObj, ...)</code> / <code>fn.apply(thisObj, arr)</code></td><td>Invokes <code>fn</code> with explicit <code>this</code> MovieClip/object binding and individual or array-packed arguments.</td></tr>\
                 <tr><td><code>Function.prototype / new</code></td><td>Constructor inheritance</td><td>Attaches shared methods inherited via <code>__proto__</code> on <code>new Ctor()</code> and <code>Object.registerClass</code> symbols.</td></tr>\
                 <tr><td><code>var local</code> vs <code>this.prop</code></td><td>Lexical closures</td><td><code>var</code> binds to the function activation object (surviving inside nested closures); <code>this.prop</code> persists on the timeline.</td></tr>\
                 <tr><td><code>play / stop / gotoAndPlay / gotoAndStop</code></td><td>Timeline &amp; MovieClip</td><td>1-based frame index or frame label navigation on <code>this</code>, <code>_root</code>, <code>_parent</code>, or child clips.</td></tr>\
                 <tr><td><code>on(...) / onClipEvent(...)</code></td><td>Button / MovieClip instance</td><td>Attached classic object handlers and per-frame hooks.</td></tr>\
                 </tbody></table>\
                 <h3>AS2 Case 1 &amp; 2 — Named function Declarations, Hoisting, return &amp; Anonymous Literals</h3>\
                 <pre class=\"code-block\"><code>",
            );
            super::help_manual::escape_html(AS2_FN_DECLARATIONS_AND_LITERALS_SAMPLE, out);
            out.push_str(
                "</code></pre>\
                 <h3>AS2 Case 3 &amp; 4 — Variadic arguments (callee, caller) &amp; Function.call / Function.apply</h3>\
                 <pre class=\"code-block\"><code>",
            );
            super::help_manual::escape_html(AS2_FN_ARGUMENTS_CALL_APPLY_SAMPLE, out);
            out.push_str(
                "</code></pre>\
                 <h3>AS2 Case 5 &amp; 6 — Constructor Functions, Function.prototype &amp; Lexical Closures</h3>\
                 <pre class=\"code-block\"><code>",
            );
            super::help_manual::escape_html(AS2_FN_PROTOTYPE_AND_CLOSURES_SAMPLE, out);
            out.push_str(
                "</code></pre>\
                 <h3>AS2 Case 7 — Complete 1-Frame Game Pattern (onRelease, onEnterFrame, Key, setInterval &amp; trace)</h3>\
                 <pre class=\"code-block\"><code>",
            );
            super::help_manual::escape_html(AS2_ONE_FRAME_GAME_LOOP_SAMPLE, out);
            out.push_str("</code></pre>");
        }
        11 => {
            out.push_str(
                "<h3>AS2 Core Runtime Classes Quick Reference</h3>\
                 <table class=\"ref-table\"><thead><tr><th>Class / Object</th><th>Package</th><th>Key Members</th></tr></thead><tbody>\
                 <tr><td><code>Function &amp; arguments</code></td><td>Top-level</td><td><code>call(thisObj, ...)</code>, <code>apply(thisObj, argsArray)</code>, <code>prototype</code>, <code>arguments.length</code>, <code>arguments.callee</code>, <code>arguments.caller</code>.</td></tr>\
                 <tr><td><code>MovieClip</code> (Drawing API)</td><td>Top-level</td><td><code>createEmptyMovieClip</code>, <code>lineStyle</code>, <code>beginFill</code>, <code>beginGradientFill</code>, <code>moveTo</code>, <code>lineTo</code>, <code>curveTo</code>, <code>endFill</code>.</td></tr>\
                 <tr><td><code>TextField &amp; TextFormat</code></td><td>Top-level</td><td><code>createTextField</code>, <code>text</code>, <code>htmlText</code>, <code>setTextFormat</code>, <code>setNewTextFormat</code>, <code>autoSize</code>, <code>wordWrap</code>.</td></tr>\
                 <tr><td><code>BitmapData / Matrix / ColorTransform</code></td><td><code>flash.display</code>, <code>flash.geom</code></td><td><code>draw</code>, <code>copyPixels</code>, <code>fillRect</code>, <code>getPixel32</code>, <code>perlinNoise</code>, <code>threshold</code>.</td></tr>\
                 <tr><td><code>Sound / Stage / SharedObject / LoadVars</code></td><td>Top-level</td><td><code>attachSound</code>, <code>loadSound</code>, <code>Stage.scaleMode</code>, <code>Stage.align</code>, <code>Stage.displayState</code>, <code>getLocal</code>, <code>setInterval</code>, <code>clearInterval</code>.</td></tr>\
                 </tbody></table>\
                 <h3>AS2 Procedural Drawing, TextFormat &amp; BitmapData Example</h3>\
                 <pre class=\"code-block\"><code>import flash.display.BitmapData;\nimport flash.geom.Matrix;\nvar canvas:MovieClip = this.createEmptyMovieClip(\"badge_mc\", this.getNextHighestDepth());\ncanvas.lineStyle(2, 0x6C93FF, 100);\ncanvas.beginFill(0x1F2A48, 95);\ncanvas.moveTo(20, 20); canvas.lineTo(180, 20); canvas.lineTo(180, 70); canvas.lineTo(20, 70); canvas.endFill();\nthis.createTextField(\"label_txt\", this.getNextHighestDepth(), 32, 32, 140, 28);\nlabel_txt.text = \"READY PLAYER ONE\";\nlabel_txt.setTextFormat(new TextFormat(\"Inter\", 14, 0xFFFFFF, true));</code></pre>",
            );
        }
        12 => {
            out.push_str(
                "<h3>AS3 Display List &amp; Function Semantics Quick Reference</h3>\
                 <table class=\"ref-table\"><thead><tr><th>Class</th><th>Package</th><th>Capabilities</th></tr></thead><tbody>\
                 <tr><td><code>Function</code></td><td>Top-level</td><td>Default parameter values, typed <code>...rest</code> arrays, bound class method closures, <code>call(thisArg, ...)</code>, and <code>apply(thisArg, array)</code>.</td></tr>\
                 <tr><td><code>DisplayObject</code></td><td><code>flash.display</code></td><td><code>x</code>, <code>y</code>, <code>scaleX</code>, <code>scaleY</code>, <code>rotation</code>, <code>alpha</code>, <code>visible</code>, <code>filters</code>, <code>mask</code>, <code>localToGlobal</code>, <code>getBounds</code>.</td></tr>\
                 <tr><td><code>InteractiveObject</code></td><td><code>flash.display</code></td><td><code>mouseEnabled</code>, <code>tabEnabled</code>, <code>tabIndex</code>, <code>doubleClickEnabled</code>, keyboard &amp; pointer focus.</td></tr>\
                 <tr><td><code>DisplayObjectContainer / Sprite</code></td><td><code>flash.display</code></td><td><code>addChild</code>, <code>addChildAt</code>, <code>removeChild</code>, <code>swapChildren</code>, <code>graphics</code>, <code>buttonMode</code>, <code>startDrag</code>.</td></tr>\
                 <tr><td><code>MovieClip &amp; Stage</code></td><td><code>flash.display</code></td><td><code>currentFrame</code>, <code>currentLabel</code>, <code>totalFrames</code>, <code>stage.scaleMode</code>, <code>stage.align</code>, <code>stage.displayState</code>.</td></tr>\
                 </tbody></table>\
                 <h3>AS3 Functions — Default Parameters, ...rest, Closures &amp; Function.call / apply</h3>\
                 <pre class=\"code-block\"><code>",
            );
            super::help_manual::escape_html(AS3_FUNCTIONS_REST_CALL_CLOSURES_SAMPLE, out);
            out.push_str(
                "</code></pre>\
                 <h3>AS3 Display List &amp; Coordinate Space Example</h3>\
                 <pre class=\"code-block\"><code>package {\n    import flash.display.Sprite;\n    import flash.events.Event;\n    import flash.geom.Point;\n    public class Main extends Sprite {\n        public function Main() {\n            addEventListener(Event.ADDED_TO_STAGE, init);\n        }\n        private function init(e:Event):void {\n            var child:Sprite = new Sprite();\n            child.x = 120; child.y = 80;\n            addChild(child);\n            var globalPt:Point = child.localToGlobal(new Point(0, -18));\n            trace(\"Global position:\", globalPt.x, globalPt.y);\n        }\n    }\n}</code></pre>",
            );
        }
        13 => {
            out.push_str(
                "<h3>AS3 Event Model &amp; Frame Lifecycle Quick Reference</h3>\
                 <table class=\"ref-table\"><thead><tr><th>Event Class</th><th>Constants</th><th>Notes</th></tr></thead><tbody>\
                 <tr><td><code>Event</code></td><td><code>ADDED_TO_STAGE</code>, <code>ENTER_FRAME</code>, <code>RESIZE</code>, <code>COMPLETE</code></td><td>3-phase W3C propagation (Capture → Target → Bubble); deterministic frame tick order.</td></tr>\
                 <tr><td><code>MouseEvent</code></td><td><code>CLICK</code>, <code>MOUSE_DOWN</code>, <code>MOUSE_UP</code>, <code>MOUSE_MOVE</code>, <code>ROLL_OVER</code></td><td>Provides <code>localX/Y</code>, <code>stageX/Y</code>, <code>buttonDown</code>, <code>delta</code>; honors <code>Stage.align</code> &amp; fullscreen transforms.</td></tr>\
                 <tr><td><code>KeyboardEvent / FullScreenEvent</code></td><td><code>KEY_DOWN</code>, <code>KEY_UP</code>, <code>FULL_SCREEN</code></td><td>Stage-dispatched keyboard input and fullscreen transitions (<code>StageDisplayState</code>).</td></tr>\
                 <tr><td><code>Timer &amp; TimerEvent</code></td><td><code>TIMER</code>, <code>TIMER_COMPLETE</code></td><td>Repeatable millisecond intervals with <code>currentCount</code>, <code>reset()</code>, <code>start()</code>, and <code>stop()</code>.</td></tr>\
                 </tbody></table>\
                 <h3>AS3 3-Phase Events, Stage Keyboard &amp; Timer Example</h3>\
                 <pre class=\"code-block\"><code>import flash.events.MouseEvent;\nimport flash.events.KeyboardEvent;\nimport flash.events.TimerEvent;\nimport flash.utils.Timer;\ncard_mc.buttonMode = true;\ncard_mc.mouseChildren = false;\ncard_mc.addEventListener(MouseEvent.MOUSE_DOWN, function(e:MouseEvent):void { card_mc.startDrag(); });\nstage.addEventListener(MouseEvent.MOUSE_UP, function(e:MouseEvent):void { card_mc.stopDrag(); });\nvar timer:Timer = new Timer(500, 10);\ntimer.addEventListener(TimerEvent.TIMER, function(e:TimerEvent):void { trace(timer.currentCount); });\ntimer.start();</code></pre>",
            );
        }
        14 => {
            out.push_str(
                "<h3>AS3 Graphics, Filters, Media &amp; Data Quick Reference</h3>\
                 <table class=\"ref-table\"><thead><tr><th>Subsystem</th><th>Classes</th><th>Capabilities</th></tr></thead><tbody>\
                 <tr><td>Vector Graphics</td><td><code>Graphics</code>, <code>GradientType</code></td><td><code>drawRect</code>, <code>drawRoundRect</code>, <code>drawCircle</code>, <code>drawEllipse</code>, <code>cubicCurveTo</code>, <code>drawPath</code>.</td></tr>\
                 <tr><td>Live Filters</td><td><code>BlurFilter</code>, <code>GlowFilter</code>, <code>DropShadowFilter</code>, <code>ColorMatrixFilter</code></td><td>Composable per-display-object filter chains matching authored NF2 filter stacks.</td></tr>\
                 <tr><td>Media &amp; Loading</td><td><code>Sound</code>, <code>SoundChannel</code>, <code>Loader</code>, <code>URLRequest</code>, <code>NetStream</code></td><td>Runtime loading of SWF, published NeoFlash bundles, MP3, WAV, MP4, AVI, SVG, and bitmaps.</td></tr>\
                 <tr><td>Binary &amp; XML</td><td><code>ByteArray</code>, <code>XML</code>, <code>XMLList</code>, <code>SharedObject</code></td><td>Big/little-endian binary streams, E4X filtering, and persistent local state.</td></tr>\
                 </tbody></table>\
                 <h3>AS3 Graphics, GlowFilter, SoundChannel &amp; E4X XML Example</h3>\
                 <pre class=\"code-block\"><code>import flash.display.Sprite;\nimport flash.filters.GlowFilter;\nimport flash.media.Sound;\nimport flash.media.SoundTransform;\nvar orb:Sprite = new Sprite();\norb.graphics.beginFill(0x3C6BF0, 0.85);\norb.graphics.drawCircle(0, 0, 36);\norb.graphics.endFill();\norb.filters = [new GlowFilter(0x6C93FF, 0.9, 16, 16, 2, 2)];\naddChild(orb);\nvar xml:XML = &lt;items&gt;&lt;weapon id=\"railgun\" dmg=\"90\"/&gt;&lt;/items&gt;;\ntrace(xml.weapon.(Number(@dmg) &gt;= 50).@id);</code></pre>",
            );
        }
        15 => {
            out.push_str(
                "<h3>AS1/AS2 ↔ AS3 ↔ JS/TS Quick Equivalence Table</h3>\
                 <table class=\"ref-table\"><thead><tr><th>AS1 / AS2 (AVM1)</th><th>ActionScript 3.0 (AVM2)</th><th>JavaScript / TypeScript</th></tr></thead><tbody>\
                 <tr><td><code>_x, _y, _rotation, _visible</code></td><td><code>x, y, rotation, visible</code></td><td><code>mc.x, mc.y, mc.rotation, mc.visible</code></td></tr>\
                 <tr><td><code>_xscale, _yscale (0..100)</code></td><td><code>scaleX, scaleY (0.0..1.0)</code></td><td><code>mc.scaleX, mc.scaleY (0.0..1.0)</code></td></tr>\
                 <tr><td><code>_alpha (0..100)</code></td><td><code>alpha (0.0..1.0)</code></td><td><code>mc.alpha (0.0..1.0)</code></td></tr>\
                 <tr><td><code>mc.onRelease = fn</code></td><td><code>mc.addEventListener(MouseEvent.CLICK, fn)</code></td><td><code>mc.addEventListener(\"click\", fn)</code></td></tr>\
                 <tr><td><code>mc.onEnterFrame = fn</code></td><td><code>mc.addEventListener(Event.ENTER_FRAME, fn)</code></td><td><code>mc.addEventListener(\"enterFrame\", fn)</code></td></tr>\
                 <tr><td><code>attachMovie(\"Id\", \"n\", d)</code></td><td><code>addChild(new IdClass())</code></td><td><code>this.addChild(new IdClass())</code></td></tr>\
                 </tbody></table>",
            );
        }
        16 => {
            out.push_str(
                "<h3>JavaScript (ES2023+) Runtime Bindings &amp; Provenance Table</h3>\
                 <table class=\"ref-table\"><thead><tr><th>Binding / Syntax</th><th>Provenance</th><th>Behavior &amp; Scope</th></tr></thead><tbody>\
                 <tr><td><code>this</code></td><td><code>[NeoFlash binding]</code></td><td>Owning <code>MovieClip</code> in frame scripts; attached <code>DisplayObject</code> in object scripts.</td></tr>\
                 <tr><td><code>event</code></td><td><code>[NeoFlash binding]</code></td><td>Structured event payload (<code>type</code>, <code>target</code>, <code>currentTarget</code>, <code>stageX</code>, <code>stageY</code>, <code>keyCode</code>).</td></tr>\
                 <tr><td><code>trace(...values)</code></td><td><code>[NeoFlash binding]</code></td><td>Writes to Output panel (F2) in authoring and <code>console.log</code> in published HTML5.</td></tr>\
                 <tr><td><code>play / stop / gotoAndPlay / gotoAndStop</code></td><td><code>[NeoFlash binding]</code></td><td>1-based frame and label navigation on the active timeline or target MovieClip.</td></tr>\
                 <tr><td><code>import / export</code></td><td><code>[Project module]</code></td><td>Relative ESM imports (<code>./</code>, <code>../</code>) across project <code>.js/.jsx/.ts/.tsx</code> files (no Node/npm).</td></tr>\
                 <tr><td><code>&lt;Tag prop={v}/&gt;</code></td><td><code>[NeoFlash binding]</code></td><td>Compiled deterministically to <code>__nfJsx(tag, props, ...children)</code>.</td></tr>\
                 <tr><td><code>Promise / async / await</code></td><td><code>[ECMAScript]</code></td><td>Standard ES2023+ promises, async functions, <code>Map</code>, <code>Set</code>, <code>JSON</code>, and <code>Math</code>.</td></tr>\
                 </tbody></table>\
                 <h3>JavaScript Functions, Closures &amp; Function.call / apply Example</h3>\
                 <pre class=\"code-block\"><code>",
            );
            super::help_manual::escape_html(JS_FUNCTIONS_CLOSURES_AND_CALL_SAMPLE, out);
            out.push_str(
                "</code></pre>\
                 <h3>JavaScript Frame Script &amp; JSX Example</h3>\
                 <pre class=\"code-block\"><code>stop();\ntrace(\"Entering level:\", this.currentFrame, \"of\", this.totalFrames);\nconst badge = &lt;hud-badge label=\"SCORE\" value={0} /&gt;;\nthis.start_btn.addEventListener(\"click\", (event) =&gt; {\n    trace(\"Clicked:\", event.target.name);\n    gotoAndPlay(\"gameplay\");\n});</code></pre>",
            );
        }
        17 => {
            out.push_str(
                "<h3>TypeScript &amp; Built-In Checker (<code>ts_check</code>) Reference Table</h3>\
                 <table class=\"ref-table\"><thead><tr><th>Construct / Utility</th><th>Provenance</th><th>Static Verification &amp; Runtime Lowering</th></tr></thead><tbody>\
                 <tr><td><code>function (typed)</code></td><td><code>[TypeScript]</code></td><td>Checks parameter arity, optional (<code>?</code>) / default arguments, function type aliases, and return types; erased at runtime.</td></tr>\
                 <tr><td><code>interface Name { ... }</code></td><td><code>[TypeScript]</code></td><td>Checked for required/optional (<code>?</code>) members and inheritance; erased at runtime.</td></tr>\
                 <tr><td><code>type Alias&lt;T&gt; = A | B</code></td><td><code>[TypeScript]</code></td><td>Unions (<code>|</code>), intersections (<code>&amp;</code>), tuples, and generic aliases; erased at runtime.</td></tr>\
                 <tr><td><code>enum Kind { A, B }</code></td><td><code>[TypeScript]</code></td><td>Checked for member access and lowered to a runtime lookup object.</td></tr>\
                 <tr><td><code>Record / Partial / Pick / Omit</code></td><td><code>[TypeScript]</code></td><td>Built-in utility types validated by <code>ts_check</code> without external <code>tsc</code> or <code>node_modules</code>.</td></tr>\
                 </tbody></table>\
                 <h3>TypeScript Typed &amp; Generic Functions Example</h3>\
                 <pre class=\"code-block\"><code>",
            );
            super::help_manual::escape_html(TS_TYPED_AND_GENERIC_FUNCTIONS_SAMPLE, out);
            out.push_str(
                "</code></pre>\
                 <h3>TypeScript Interfaces, Unions &amp; Utility Types Example</h3>\
                 <pre class=\"code-block\"><code>interface Actor {\n    readonly id: string;\n    x: number;\n    y: number;\n    hp: number;\n}\ntype ActorPatch = Partial&lt;Pick&lt;Actor, \"x\" | \"y\" | \"hp\"&gt;&gt;;\nfunction applyPatch(actor: Actor, patch: ActorPatch): Actor {\n    return { id: actor.id, x: patch.x ?? actor.x, y: patch.y ?? actor.y, hp: patch.hp ?? actor.hp };\n}</code></pre>",
            );
        }
        18 => {
            out.push_str(
                "<h3>Browser &amp; Web APIs Reference Table (Published HTML5 Host)</h3>\
                 <table class=\"ref-table\"><thead><tr><th>Web API</th><th>Provenance</th><th>Availability &amp; Browser Security Boundary</th></tr></thead><tbody>\
                 <tr><td><code>CanvasRenderingContext2D</code></td><td><code>[Browser host]</code></td><td><code>fillRect</code>, <code>beginPath</code>, <code>arc</code>, <code>fill</code>, <code>drawImage</code>, <code>setTransform</code>; subject to CORS canvas taint rules.</td></tr>\
                 <tr><td><code>document.querySelector</code></td><td><code>[Browser host]</code></td><td>DOM lookup and overlay styling in published HTML5 pages; guard with <code>typeof document !== \"undefined\"</code>.</td></tr>\
                 <tr><td><code>fetch(url, init)</code></td><td><code>[Browser host]</code></td><td>Async HTTP/HTTPS requests returning <code>Promise&lt;Response&gt;</code>; requires HTTP(S) CORS headers &amp; CSP <code>connect-src</code>.</td></tr>\
                 <tr><td><code>localStorage</code></td><td><code>[Browser host]</code></td><td>Same-origin key-value persistence (~5 MB quota; wrap in <code>try/catch</code> for private browsing).</td></tr>\
                 <tr><td><code>AudioContext &amp; video.play()</code></td><td><code>[Browser host]</code></td><td>Requires a user gesture (click / keydown) before unmuted audio playback starts.</td></tr>\
                 </tbody></table>\
                 <h3>TypeScript + Browser Host Example</h3>\
                 <pre class=\"code-block\"><code>async function syncHighScore(canvas: HTMLCanvasElement, url: string): Promise&lt;void&gt; {\n    const ctx = canvas.getContext(\"2d\");\n    if (ctx) { ctx.fillRect(0, 0, 240, 48); }\n    const res = await fetch(url);\n    const top = await res.json();\n    localStorage.setItem(\"neoflash.topScore\", JSON.stringify(top));\n}</code></pre>",
            );
        }
        _ => {}
    }
}

const fn browser_theme_slug(theme: Theme) -> &'static str {
    match theme {
        Theme::Dark => "dark",
        Theme::Light => "light",
        Theme::MidNeutralGray => "mid-neutral-gray",
        Theme::DarkGraphite => "dark-graphite",
    }
}

const fn browser_theme_label(theme: Theme) -> &'static str {
    match theme {
        Theme::Dark => "Dark",
        Theme::Light => "Light",
        Theme::MidNeutralGray => "Mid Neutral Gray",
        Theme::DarkGraphite => "Dark Graphite",
    }
}

#[cfg(test)]
pub(crate) fn render_full_browser_help_html(initial_theme: Theme) -> String {
    render_browser_help_html_for_route(initial_theme, None, None, None)
}

fn chapter_html_lang_scopes(index: usize) -> &'static str {
    let mut as12 = LanguageScope::As1As2.matches_chapter(index);
    let mut as3 = LanguageScope::As3.matches_chapter(index);
    let mut jsts = LanguageScope::JsTs.matches_chapter(index);
    if index < AUTHORING_CHAPTER_COUNT {
        as12 = true;
        as3 = true;
        jsts = true;
    }
    match (as12, as3, jsts) {
        (true, true, true) => "ALL AS1_AS2 AS3 JS_TS",
        (true, false, false) => "ALL AS1_AS2",
        (false, true, false) => "ALL AS3",
        (false, false, true) => "ALL JS_TS",
        (true, true, false) => "ALL AS1_AS2 AS3",
        (true, false, true) => "ALL AS1_AS2 JS_TS",
        (false, true, true) => "ALL AS3 JS_TS",
        (false, false, false) => "ALL",
    }
}

pub(crate) fn render_browser_help_html_for_route(
    initial_theme: Theme,
    initial_mode: Option<super::help_manual::HelpManualMode>,
    active_page: Option<usize>,
    active_tag: Option<&str>,
) -> String {
    render_browser_help_html_for_project(
        initial_theme,
        initial_mode,
        active_page,
        active_tag,
        None,
        LanguageScope::All,
    )
}

pub(crate) fn render_browser_help_html_for_project(
    initial_theme: Theme,
    initial_mode: Option<super::help_manual::HelpManualMode>,
    active_page: Option<usize>,
    active_tag: Option<&str>,
    project_language: Option<neoflash_core::ScriptLanguage>,
    initial_lang_scope: LanguageScope,
) -> String {
    use super::help_manual::HelpManualMode;
    let hash = char::from(35u8);
    let clamped_page = active_page.map(|p| p.min(HELP_CHAPTERS.len().saturating_sub(1)));
    let initial_mode_slug = match initial_mode {
        None => "ALL",
        Some(HelpManualMode::AuthoringHelp) => "AUTHORING",
        Some(HelpManualMode::ScriptingReference) => "SCRIPTING",
    };
    let initial_lang_slug = initial_lang_scope.html_slug();
    let initial_tag = active_tag
        .filter(|t| HELP_ALL_TAGS.contains(t))
        .unwrap_or("ALL");
    let page_title = match initial_mode {
        Some(HelpManualMode::ScriptingReference) => {
            "NeoFlash Scripting Reference — NeoFlash Help &amp; Reference"
        }
        Some(HelpManualMode::AuthoringHelp) => {
            "NeoFlash Authoring Manual — NeoFlash Help &amp; Reference"
        }
        None => "NeoFlash Help &amp; Reference",
    };
    let mut html = String::with_capacity(114_688);
    let _ = write!(
        html,
        "<!doctype html><html lang=\"en\" data-initial-theme=\"{}\" data-initial-mode=\"{initial_mode_slug}\" data-initial-lang=\"{initial_lang_slug}\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <meta name=\"color-scheme\" content=\"dark light\">\
         <title>{page_title}</title><style>",
        browser_theme_slug(initial_theme)
    );
    html.push_str(":root{");
    let _ = write!(
        html,
        "color-scheme:{};",
        if initial_theme.is_dark() {
            "dark"
        } else {
            "light"
        }
    );
    write_palette_css_vars(initial_theme.palette(), &mut html);
    html.push('}');
    for theme in Theme::ALL {
        let _ = write!(
            html,
            "body:has(#nf2-theme-{}:checked){{color-scheme:{};",
            browser_theme_slug(theme),
            if theme.is_dark() { "dark" } else { "light" }
        );
        write_palette_css_vars(theme.palette(), &mut html);
        html.push('}');
    }
    html.push_str(
        "*{box-sizing:border-box}body{margin:0;font-family:Inter,-apple-system,BlinkMacSystemFont,sans-serif;font-size:13px;line-height:1.55;background:var(--bg-app);color:var(--text-primary)}\
         .sr-toggle{position:absolute;opacity:0;pointer-events:none}\
         .help-shell{min-height:100vh;background:var(--bg-app);color:var(--text-primary);display:flex;flex-direction:column}\
         .top-header{position:sticky;top:0;z-index:10;background:var(--bg-raised);border-bottom:1px solid var(--border-subtle);padding:12px 24px;display:flex;align-items:center;justify-content:space-between;gap:16px;flex-wrap:wrap}\
         .top-header h1{margin:0;font-size:16px;font-weight:600;color:var(--text-primary)}\
         .top-header p{margin:2px 0 0;font-size:12px;color:var(--text-secondary)}\
         .theme-btn{display:none;cursor:pointer;padding:6px 12px;border-radius:6px;background:var(--bg-panel);border:1px solid var(--border-default);color:var(--text-primary);font-size:12px;font-weight:500;user-select:none}\
         .theme-btn:hover{background:var(--bg-hover);border-color:var(--border-strong)}\
         body:has(#nf2-theme-dark:checked) .theme-from-dark,body:has(#nf2-theme-light:checked) .theme-from-light,body:has(#nf2-theme-mid-neutral-gray:checked) .theme-from-mid-neutral-gray,body:has(#nf2-theme-dark-graphite:checked) .theme-from-dark-graphite{display:inline-block}\
         .tag-bar{background:var(--bg-panel);border-bottom:1px solid var(--border-subtle);padding:8px 24px;display:flex;align-items:center;gap:6px;flex-wrap:wrap}\
         .tag-bar-label{font-size:11px;font-weight:600;text-transform:uppercase;letter-spacing:0.06em;color:var(--text-tertiary);margin-right:4px}\
         .tag-pill{cursor:pointer;padding:3px 10px;border-radius:999px;background:var(--bg-raised);border:1px solid var(--border-subtle);color:var(--text-secondary);font-size:11px;font-weight:500;user-select:none;text-decoration:none}\
         .tag-pill:hover{background:var(--bg-hover);color:var(--text-primary)}\
         .layout{display:grid;grid-template-columns:280px 1fr;gap:24px;max-width:1380px;width:100%;margin:0 auto;padding:20px 24px}\
         @media (max-width:860px){.layout{grid-template-columns:1fr}}\
         nav.sidebar{background:var(--bg-panel);border:1px solid var(--border-subtle);border-radius:8px;padding:12px;align-self:start;position:sticky;top:76px;max-height:calc(100vh - 96px);overflow-y:auto}\
         nav.sidebar ol{list-style:none;margin:0;padding:0;display:flex;flex-direction:column;gap:3px}\
         nav.sidebar .nav-group{margin:8px 6px 4px;font-size:10.5px;font-weight:700;text-transform:uppercase;letter-spacing:0.06em;color:var(--text-tertiary)}\
         nav.sidebar a{display:flex;align-items:center;justify-content:space-between;gap:6px;padding:6px 10px;border-radius:6px;color:var(--text-primary);text-decoration:none;font-size:12px;font-weight:500;overflow-wrap:anywhere}\
         nav.sidebar a:hover{background:var(--bg-hover);color:var(--accent-default)}\
         .nav-dialect{font-size:10.5px;color:var(--text-tertiary);font-family:'Roboto Mono',ui-monospace,monospace;flex-shrink:0}\
         main{display:flex;flex-direction:column;gap:20px;min-width:0}\
         section.chapter-card{background:var(--bg-panel);border:1px solid var(--border-subtle);border-radius:8px;padding:20px;scroll-margin-top:84px}\
         section.chapter-card:target,section.chapter-card.is-opened-chapter{border-color:var(--accent-default);box-shadow:0 0 0 1px var(--accent-default)}\
         nav.sidebar a.is-opened-nav{background:var(--accent-subtle);color:var(--accent-default);font-weight:600}\
         .chapter-head{display:flex;align-items:baseline;justify-content:space-between;gap:12px;flex-wrap:wrap;border-bottom:1px solid var(--border-subtle);padding-bottom:10px;margin-bottom:12px}\
         .chapter-head h2{margin:0;font-size:16px;font-weight:600;color:var(--text-primary)}\
         .chapter-sub{margin:4px 0 10px;color:var(--text-secondary);font-size:12px}\
         .chapter-tags{display:flex;gap:6px;flex-wrap:wrap;margin-bottom:12px}\
         .badge{display:inline-flex;align-items:center;gap:5px;padding:2px 8px;border-radius:999px;background:var(--accent-subtle);color:var(--accent-default);font-size:11px;font-weight:500;cursor:pointer}\
         .diagram-card{margin:12px 0;padding:0}\
         .chapter-svg{width:100%;height:auto;display:block;border-radius:8px}\
         .diagram-caption{margin-top:6px;font-size:11.5px;color:var(--text-secondary)}\
         ul.topic-list{margin:10px 0;padding-left:20px;color:var(--text-primary)}\
         ul.topic-list li{margin-bottom:6px}\
         pre.code-block{background:var(--bg-field);border:1px solid var(--border-subtle);border-radius:6px;padding:12px;overflow-x:auto;font-family:'Roboto Mono',ui-monospace,monospace;font-size:11.5px;line-height:1.45;color:var(--text-primary)}\
         table.ref-table{width:100%;border-collapse:collapse;margin:10px 0;font-size:12px}\
         table.ref-table th,table.ref-table td{text-align:left;padding:7px 10px;border-bottom:1px solid var(--border-subtle);vertical-align:top}\
         table.ref-table th{color:var(--accent-default);font-weight:600;background:var(--bg-raised)}\
         .cross-links{display:flex;align-items:center;gap:8px;flex-wrap:wrap;margin-top:14px;padding-top:10px;border-top:1px solid var(--border-subtle)}\
         .cross-link{display:inline-block;padding:4px 10px;border-radius:6px;background:var(--bg-raised);border:1px solid var(--border-default);color:var(--accent-default);text-decoration:none;font-size:11.5px;font-weight:500}\
         .cross-link:hover{background:var(--accent-subtle);border-color:var(--accent-default)}\
         .search-terms{margin-top:8px;font-size:11px;color:var(--text-tertiary)}",
    );

    // Pure-CSS tag & language-scope filter rules using :has()
    for tag in HELP_ALL_TAGS {
        let _ = write!(
            html,
            ".help-shell:has(input[data-tag=\"{tag}\"]:checked) label[data-tag-pill=\"{tag}\"]{{background:var(--accent-subtle);color:var(--accent-default);border-color:var(--accent-default)}}\
             .help-shell:has(input[data-tag=\"{tag}\"]:checked) section.chapter-card:not([data-tags~=\"{tag}\"]){{display:none}}\
             .help-shell:has(input[data-tag=\"{tag}\"]:checked) nav.sidebar li.chapter-nav-item:not([data-tags~=\"{tag}\"]){{display:none}}"
        );
    }
    for lang_slug in ["AS1_AS2", "AS3", "JS_TS"] {
        let _ = write!(
            html,
            ".help-shell:has(input[data-lang=\"{lang_slug}\"]:checked) label[data-lang-pill=\"{lang_slug}\"]{{background:var(--accent-subtle);color:var(--accent-default);border-color:var(--accent-default)}}\
             .help-shell:has(input[data-lang=\"{lang_slug}\"]:checked) section.chapter-card[data-section=\"scripting\"]:not([data-lang-scopes~=\"{lang_slug}\"]){{display:none}}\
             .help-shell:has(input[data-lang=\"{lang_slug}\"]:checked) nav.sidebar li.chapter-nav-item[data-section=\"scripting\"]:not([data-lang-scopes~=\"{lang_slug}\"]){{display:none}}"
        );
    }
    html.push_str(
        ".help-shell:has(input[data-lang=\"ALL\"]:checked) label[data-lang-pill=\"ALL\"]{background:var(--accent-subtle);color:var(--accent-default);border-color:var(--accent-default)}\
         .help-shell:has(input[data-tag=\"ALL\"]:checked) label[data-tag-pill=\"ALL\"]{background:var(--accent-subtle);color:var(--accent-default);border-color:var(--accent-default)}\
         .help-shell:has(input[data-mode=\"ALL\"]:checked) label[data-mode-pill=\"ALL\"],\
         .help-shell:has(input[data-mode=\"AUTHORING\"]:checked) label[data-mode-pill=\"AUTHORING\"],\
         .help-shell:has(input[data-mode=\"SCRIPTING\"]:checked) label[data-mode-pill=\"SCRIPTING\"]{background:var(--accent-subtle);color:var(--accent-default);border-color:var(--accent-default)}\
         .help-shell:has(input[data-mode=\"AUTHORING\"]:checked) section.chapter-card[data-section=\"scripting\"],\
         .help-shell:has(input[data-mode=\"AUTHORING\"]:checked) nav.sidebar li[data-section=\"scripting\"]{display:none}\
         .help-shell:has(input[data-mode=\"SCRIPTING\"]:checked) section.chapter-card[data-section=\"authoring\"],\
         .help-shell:has(input[data-mode=\"SCRIPTING\"]:checked) nav.sidebar li[data-section=\"authoring\"]{display:none}",
    );
    html.push_str("</style></head><body>");

    // Theme, section-mode, language-scope and tag filters remain 100% declarative (zero <script>).
    for theme in Theme::ALL {
        let _ = write!(
            html,
            "<input type=\"radio\" name=\"nf2-theme\" id=\"nf2-theme-{}\" class=\"sr-toggle\" aria-label=\"{} theme\"{}>",
            browser_theme_slug(theme),
            browser_theme_label(theme),
            if theme == initial_theme {
                " checked"
            } else {
                ""
            }
        );
    }
    html.push_str("<div class=\"help-shell\">");
    for mode_slug in ["ALL", "AUTHORING", "SCRIPTING"] {
        let _ = write!(
            html,
            "<input type=\"radio\" name=\"nf2-mode\" id=\"nf2-mode-{mode_slug}\" data-mode=\"{mode_slug}\" class=\"sr-toggle\" {}>",
            if mode_slug == initial_mode_slug {
                "checked"
            } else {
                ""
            }
        );
    }
    for lang_slug in ["ALL", "AS1_AS2", "AS3", "JS_TS"] {
        let _ = write!(
            html,
            "<input type=\"radio\" name=\"nf2-lang\" id=\"nf2-lang-{lang_slug}\" data-lang=\"{lang_slug}\" class=\"sr-toggle\" {}>",
            if lang_slug == initial_lang_slug {
                "checked"
            } else {
                ""
            }
        );
    }
    let _ = write!(
        html,
        "<input type=\"radio\" name=\"nf2-tag\" id=\"nf2-tag-ALL\" data-tag=\"ALL\" class=\"sr-toggle\" {}>",
        if initial_tag == "ALL" { "checked" } else { "" }
    );
    for tag in HELP_ALL_TAGS {
        let _ = write!(
            html,
            "<input type=\"radio\" name=\"nf2-tag\" id=\"nf2-tag-{tag}\" data-tag=\"{tag}\" class=\"sr-toggle\" {}>",
            if *tag == initial_tag { "checked" } else { "" }
        );
    }

    #[cfg(target_os = "macos")]
    const HEADER_SUMMARY: &str = "Illustrated NF2 Authoring Manual (Chapters 1–10) &amp; Scripting Reference: AS1/AS2, AS3, JavaScript, TypeScript &amp; Web APIs (Chapters 11–19) · Manual macOS update preserves ~/Library/Application Support/NeoFlash · Bundled ffmpeg/ffprobe and licence evidence are included.";
    #[cfg(target_os = "windows")]
    const HEADER_SUMMARY: &str = "Illustrated NF2 Authoring Manual (Chapters 1–10) &amp; Scripting Reference: AS1/AS2, AS3, JavaScript, TypeScript &amp; Web APIs (Chapters 11–19) · Manual Windows update preserves %APPDATA%\\NeoFlash · Bundled ffmpeg/ffprobe and licence evidence are included.";
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    const HEADER_SUMMARY: &str = "Illustrated NF2 Authoring Manual (Chapters 1–10) &amp; Scripting Reference: AS1/AS2, AS3, JavaScript, TypeScript &amp; Web APIs (Chapters 11–19) · Manual Linux update preserves ~/.local/share/NeoFlash · Bundled ffmpeg/ffprobe and licence evidence are included.";

    html.push_str(
        "<header class=\"top-header\" id=\"top\"><div>\
         <h1>NeoFlash Help &amp; Reference (0.1.0-beta.1 · macOS + Windows/Linux x64 previews)</h1>\
         <p>",
    );
    html.push_str(HEADER_SUMMARY);
    html.push_str(
        "</p></div><div style=\"display:flex;align-items:center;gap:8px;flex-wrap:wrap\">",
    );
    if let Some(proj_lang) = project_language {
        let _ = write!(
            html,
            "<span class=\"badge\">Current project: {}</span>",
            proj_lang.display_label()
        );
    }
    if let Some(page) = clamped_page {
        let _ = write!(
            html,
            "<a class=\"cross-link\" href=\"{hash}chapter-{}\">Opened: Chapter {} →</a>",
            page + 1,
            page + 1
        );
    }
    html.push_str(
        "<label for=\"nf2-theme-light\" class=\"theme-btn theme-from-dark\" aria-label=\"Switch Help theme from Dark to Light\">Theme: Dark</label>\
         <label for=\"nf2-theme-mid-neutral-gray\" class=\"theme-btn theme-from-light\" aria-label=\"Switch Help theme from Light to Mid Neutral Gray\">Theme: Light</label>\
         <label for=\"nf2-theme-dark-graphite\" class=\"theme-btn theme-from-mid-neutral-gray\" aria-label=\"Switch Help theme from Mid Neutral Gray to Dark Graphite\">Theme: Mid Neutral Gray</label>\
         <label for=\"nf2-theme-dark\" class=\"theme-btn theme-from-dark-graphite\" aria-label=\"Switch Help theme from Dark Graphite to Dark\">Theme: Dark Graphite</label>\
         </div></header>",
    );

    // Interactive Section Mode + Language Scope + Tag Filter Bar
    let _ = write!(
        html,
        "<div class=\"tag-bar\" role=\"toolbar\" aria-label=\"Filter by section, language scope and topic tag\">\
         <span class=\"tag-bar-label\">Section:</span>\
         <label for=\"nf2-mode-ALL\" data-mode-pill=\"ALL\" class=\"tag-pill\">All Chapters ({})</label>\
         <label for=\"nf2-mode-AUTHORING\" data-mode-pill=\"AUTHORING\" class=\"tag-pill\">Authoring Manual (1–{})</label>\
         <label for=\"nf2-mode-SCRIPTING\" data-mode-pill=\"SCRIPTING\" class=\"tag-pill\">Scripting Reference ({}–{})</label>\
         <span class=\"tag-bar-label\" style=\"margin-left:10px\">Language:</span>\
         <label for=\"nf2-lang-ALL\" data-lang-pill=\"ALL\" class=\"tag-pill\">All languages</label>\
         <label for=\"nf2-lang-AS1_AS2\" data-lang-pill=\"AS1_AS2\" class=\"tag-pill\">AS1 / AS2</label>\
         <label for=\"nf2-lang-AS3\" data-lang-pill=\"AS3\" class=\"tag-pill\">AS3</label>\
         <label for=\"nf2-lang-JS_TS\" data-lang-pill=\"JS_TS\" class=\"tag-pill\">JS / TS</label>\
         <span class=\"tag-bar-label\" style=\"margin-left:10px\">Tag:</span>",
        HELP_CHAPTERS.len(),
        AUTHORING_CHAPTER_COUNT,
        AUTHORING_CHAPTER_COUNT + 1,
        HELP_CHAPTERS.len()
    );
    let _ = write!(
        html,
        "<label for=\"nf2-tag-ALL\" data-tag-pill=\"ALL\" class=\"tag-pill\">All ({})</label>",
        HELP_CHAPTERS.len()
    );
    for tag in HELP_ALL_TAGS {
        let _ = write!(
            html,
            "<label for=\"nf2-tag-{tag}\" data-tag-pill=\"{tag}\" class=\"tag-pill\">{tag}</label>"
        );
    }
    html.push_str("</div>");

    html.push_str("<div class=\"layout\"><nav class=\"sidebar\" aria-label=\"Chapters\"><ol>");
    for (index, chapter) in HELP_CHAPTERS.iter().enumerate() {
        if index == 0 {
            html.push_str(
                "<li class=\"nav-group\" data-section=\"authoring\">Authoring Help (1–10)</li>",
            );
        } else if index == AUTHORING_CHAPTER_COUNT {
            html.push_str(
                "<li class=\"nav-group\" data-section=\"scripting\">Scripting Ref — ActionScript</li>",
            );
        } else if index == 16 {
            html.push_str(
                "<li class=\"nav-group\" data-section=\"scripting\">Scripting Ref — JS / TS / Web</li>",
            );
        }
        let section_slug = if index < AUTHORING_CHAPTER_COUNT {
            "authoring"
        } else {
            "scripting"
        };
        let nav_cls = if clamped_page == Some(index) {
            " class=\"is-opened-nav\""
        } else {
            ""
        };
        let tags_joined = chapter.tags.join(" ");
        let lang_scopes = chapter_html_lang_scopes(index);
        let _ = write!(
            html,
            "<li class=\"chapter-nav-item\" data-section=\"{section_slug}\" data-tags=\"{tags_joined}\" data-lang-scopes=\"{lang_scopes}\"><a href=\"{hash}chapter-{}\"{nav_cls}><span>",
            index + 1
        );
        super::help_manual::escape_html(chapter.title, &mut html);
        html.push_str("</span>");
        if !chapter.sidebar_badge.is_empty() {
            html.push_str("<span class=\"nav-dialect\">");
            super::help_manual::escape_html(chapter.sidebar_badge, &mut html);
            html.push_str("</span>");
        }
        html.push_str("</a></li>");
    }
    html.push_str("</ol></nav><main>");

    for (index, chapter) in HELP_CHAPTERS.iter().enumerate() {
        let tags_joined = chapter.tags.join(" ");
        let lang_scopes = chapter_html_lang_scopes(index);
        let section_slug = if index < AUTHORING_CHAPTER_COUNT {
            "authoring"
        } else {
            "scripting"
        };
        let card_cls = if clamped_page == Some(index) {
            "chapter-card is-opened-chapter"
        } else {
            "chapter-card"
        };
        let mode_badge = if index < AUTHORING_CHAPTER_COUNT {
            format!("Help Ch. {} of {}", index + 1, AUTHORING_CHAPTER_COUNT)
        } else {
            format!(
                "Scripting Ref Ch. {} of {}",
                index - AUTHORING_CHAPTER_COUNT + 1,
                HELP_CHAPTERS.len() - AUTHORING_CHAPTER_COUNT
            )
        };
        let _ = write!(
            html,
            "<section class=\"{card_cls}\" id=\"chapter-{}\" data-section=\"{section_slug}\" data-tags=\"{tags_joined}\" data-lang-scopes=\"{lang_scopes}\"><div class=\"chapter-head\"><h2>",
            index + 1
        );
        super::help_manual::escape_html(chapter.title, &mut html);
        html.push_str(
            "</h2><div style=\"display:flex;gap:6px;flex-wrap:wrap;align-items:center\">",
        );
        for &db in chapter.dialect_badges {
            html.push_str("<span class=\"badge\">");
            super::help_manual::escape_html(db, &mut html);
            html.push_str("</span>");
        }
        let _ = write!(
            html,
            "<span class=\"badge\">{mode_badge} · Chapter {} of {}</span></div></div><p class=\"chapter-sub\">",
            index + 1,
            HELP_CHAPTERS.len()
        );
        super::help_manual::escape_html(chapter.subtitle, &mut html);
        html.push_str("</p><div class=\"chapter-tags\">");
        for tag in chapter.tags {
            let _ = write!(
                html,
                "<label for=\"nf2-tag-{tag}\" class=\"badge\" title=\"Filter by {tag}\">{tag}</label>"
            );
        }
        html.push_str("</div>");

        // Crisp inline SVG diagram for every chapter
        write_chapter_svg_illustration(index, &mut html);

        html.push_str("<ul class=\"topic-list\">");
        for topic in HELP_BROWSER_TOPICS[index] {
            html.push_str("<li>");
            super::help_manual::escape_html(topic, &mut html);
            html.push_str("</li>");
        }
        html.push_str("</ul>");

        // Extended ActionScript / JavaScript / TypeScript / Web API code blocks & tables for chapters 11..=19
        write_as_chapter_reference_html(index, &mut html);

        // Interactive Cross-Links to related chapters
        html.push_str("<div class=\"cross-links\"><span class=\"tag-bar-label\">See also:</span>");
        for &rel_idx in chapter.related {
            if let Some(rel_ch) = HELP_CHAPTERS.get(rel_idx) {
                let _ = write!(
                    html,
                    "<a class=\"cross-link\" href=\"{hash}chapter-{}\">→ ",
                    rel_idx + 1
                );
                super::help_manual::escape_html(rel_ch.title, &mut html);
                html.push_str("</a>");
            }
        }
        let _ = write!(
            html,
            "<a class=\"cross-link\" href=\"{hash}top\">↑ Top</a></div>"
        );

        html.push_str("<p class=\"search-terms\"><strong>Search terms:</strong> ");
        super::help_manual::escape_html(chapter.keywords, &mut html);
        html.push_str("</p></section>");
    }
    html.push_str("</main></div></div></body></html>");
    html
}

#[cfg(test)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use neoflash_core::ScriptLanguage;

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
            Some(super::super::help_manual::HelpManualMode::ScriptingReference),
            Some(10),
            None,
            Some(ScriptLanguage::ActionScript2),
            LanguageScope::As1As2,
        );
        assert!(html.contains("Current project: ActionScript 2.0"));
        assert!(html.contains("data-initial-lang=\"AS1_AS2\""));
        assert!(
            html.contains(
                "id=\"nf2-lang-AS1_AS2\" data-lang=\"AS1_AS2\" class=\"sr-toggle\" checked"
            )
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
        assert_eq!(js.status_count, 0);

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

        // Missing declarations are listed with their status, never offered by the toolbox.
        let net_stream = api_index(LanguageScope::As1As2)
            .class("NetStream")
            .expect("NetStream");
        let attach = net_stream
            .members
            .iter()
            .find(|m| m.name == "attachAudio")
            .expect("NetStream.attachAudio");
        assert_eq!(attach.support, Some(Support::Missing));
        assert_eq!(attach.row_signature, "(theMicrophone:Microphone):Void");
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
        let constructor = point
            .members
            .iter()
            .find(|m| m.kind == "constructor")
            .expect("Point constructor");
        assert_eq!(constructor.example, "var point:Point = new Point(x, y);");
        let distance = point
            .members
            .iter()
            .find(|m| m.name == "distance")
            .expect("Point.distance");
        assert_eq!(distance.kind, "static method");
        assert_eq!(distance.example, "Point.distance(pt1, pt2);");
        assert_eq!(distance.returns, "Number");
        assert!(api_footer_text(LanguageScope::As3).starts_with("ActionScript 3 · "));
    }
}
