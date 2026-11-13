//! Comprehensive ActionScript 1.0, 2.0 & 3.0 plus JavaScript (ES2023+), TypeScript, and
//! Browser / Web API Language & API Reference chapters (`Chapters 11–19` of
//! `Help ▸ Scripting Reference…`), interactive cross-links, tag filters, shared JS/TS
//! Actions toolbox catalog, and the full NF2-styled + SVG-illustrated HTML5 browser manual.
//!
//! Built strictly from `neoflash_ui` (NF2) tokens, typography, icons, and components.

use super::help_manual::{
    AUTHORING_CHAPTER_COUNT, HELP_BROWSER_TOPICS, HELP_CHAPTERS, illustration_canvas, mode_card,
    paint_clipped_text, paragraph, section_overline, sub_bullet,
};
use eframe::egui::{self, Rect, Stroke, StrokeKind, Ui, vec2};
use neoflash_ui::{
    Palette, TextStyle, Theme,
    components::{self as c, Badge, BadgeType, Button},
    icons::{self, i16},
    tokens::{radius, space},
    typography::rich,
};
use std::fmt::Write as _;

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

/// A copyable, syntax-tinted code block for ActionScript 1.0 / 2.0 / 3.0 and JS/TS examples.
pub(crate) struct CodeSample {
    pub title: &'static str,
    pub lang_badge: &'static str,
    pub code: &'static str,
}

pub(crate) fn code_block(ui: &mut Ui, sample: &CodeSample) {
    let p = neoflash_ui::palette(ui.ctx());
    let r = egui::CornerRadius::same(radius::MD as u8);
    egui::Frame::new()
        .fill(p.bg_field)
        .stroke(Stroke::new(1.0_f32, p.border_subtle))
        .corner_radius(r)
        .inner_margin(egui::Margin::same(space::S3 as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = space::S2;
                icons::show(ui, i16::CODE, p.accent_default);
                ui.add(
                    egui::Label::new(
                        rich(sample.title, TextStyle::BodyStrong).color(p.text_primary),
                    )
                    .wrap(),
                );
                ui.add(Badge::new(sample.lang_badge, BadgeType::Accent));
                if ui.add(Button::ghost("Copy snippet")).clicked() {
                    ui.ctx().copy_text(sample.code.to_owned());
                }
            });
            ui.add_space(space::S1);
            c::divider(ui, false);
            ui.add_space(space::S1);
            for (line_idx, line) in sample.code.lines().enumerate() {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = space::S2;
                    let ln = format!("{:>2}", line_idx + 1);
                    ui.label(rich(&ln, TextStyle::MonoSmall).color(p.text_disabled));
                    let trimmed = line.trim_start();
                    let line_color = if trimmed.starts_with("//") || trimmed.starts_with("/*") {
                        p.syntax_comment
                    } else if trimmed.starts_with("package")
                        || trimmed.starts_with("import ")
                        || trimmed.starts_with("export ")
                        || trimmed.starts_with("public class")
                        || trimmed.starts_with("class ")
                        || trimmed.starts_with("interface ")
                        || trimmed.starts_with("type ")
                        || trimmed.starts_with("async ")
                        || trimmed.starts_with("function ")
                        || trimmed.starts_with("on(")
                        || trimmed.starts_with("onClipEvent")
                    {
                        p.syntax_keyword
                    } else if trimmed.starts_with("var ")
                        || trimmed.starts_with("const ")
                        || trimmed.starts_with("let ")
                    {
                        p.syntax_type
                    } else {
                        p.text_primary
                    };
                    let code_w = (ui.available_width() - space::S1).max(80.0);
                    ui.vertical(|ui| {
                        ui.set_max_width(code_w);
                        ui.add(
                            egui::Label::new(rich(line, TextStyle::MonoSmall).color(line_color))
                                .wrap(),
                        );
                    });
                });
            }
        });
    ui.add_space(space::S2);
}

/// Renders a structured 3-column API quick-reference table using NF2 tokens.
/// Uses dynamic wrapped row heights instead of fixed 18pt slots so multi-line signatures and
/// descriptions never collide vertically at narrow widths or 1.5×/2.0× UI scales.
pub(crate) fn api_signature_table(
    ui: &mut Ui,
    title: &str,
    headers: [&str; 3],
    rows: &[(&str, &str, &str)],
) {
    let p = neoflash_ui::palette(ui.ctx());
    let r = egui::CornerRadius::same(radius::MD as u8);
    egui::Frame::new()
        .fill(p.bg_panel)
        .stroke(Stroke::new(1.0_f32, p.border_subtle))
        .corner_radius(r)
        .inner_margin(egui::Margin::same(space::S3 as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                icons::show(ui, i16::BOOKMARK, p.accent_default);
                ui.add(
                    egui::Label::new(rich(title, TextStyle::BodyStrong).color(p.text_primary))
                        .wrap(),
                );
            });
            ui.add_space(space::S1);
            let avail = ui.available_width();
            let col0 = (avail * 0.27).max(96.0);
            let col1 = (avail * 0.27).max(96.0);
            let col2 = (avail - col0 - col1 - space::S3 * 2.0).max(110.0);
            egui::Grid::new(egui::Id::new(("help-api-table", title)))
                .num_columns(3)
                .spacing(vec2(space::S2, space::S2))
                .striped(true)
                .show(ui, |ui| {
                    ui.label(rich(headers[0], TextStyle::SmallStrong).color(p.accent_default));
                    ui.label(rich(headers[1], TextStyle::SmallStrong).color(p.accent_default));
                    ui.label(rich(headers[2], TextStyle::SmallStrong).color(p.accent_default));
                    ui.end_row();
                    for (sig, ret_or_target, desc) in rows {
                        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                            ui.set_min_width(col0);
                            ui.set_max_width(col0);
                            ui.add(
                                egui::Label::new(
                                    rich(*sig, TextStyle::MonoSmall).color(p.syntax_function),
                                )
                                .wrap(),
                            );
                        });
                        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                            ui.set_min_width(col1);
                            ui.set_max_width(col1);
                            ui.add(
                                egui::Label::new(
                                    rich(*ret_or_target, TextStyle::MonoSmall).color(p.syntax_type),
                                )
                                .wrap(),
                            );
                        });
                        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                            ui.set_min_width(col2);
                            ui.set_max_width(col2);
                            ui.add(
                                egui::Label::new(
                                    rich(*desc, TextStyle::Small).color(p.text_secondary),
                                )
                                .wrap(),
                            );
                        });
                        ui.end_row();
                    }
                });
        });
    ui.add_space(space::S2);
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
    ui.add_space(space::S2);

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
                        row.min + vec2(8.0, 2.0),
                        egui::pos2(split_x, row.bottom() - 2.0),
                    );
                    let right_clip = Rect::from_min_max(
                        egui::pos2(split_x + 6.0, row.top() + 2.0),
                        row.max - vec2(8.0, 2.0),
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

    code_block(
        ui,
        &CodeSample {
            title: "AS2 Frame Script — Spawning Library Symbols, EnterFrame Loop & Hit Testing",
            lang_badge: "ActionScript 2.0",
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
            lang_badge: "ActionScript 2.0",
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
    ui.add_space(space::S2);

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
            title: "AS3 Document Class — Building a Display List Hierarchy & Coordinate Transform",
            lang_badge: "ActionScript 3.0",
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
    ui.add_space(space::S2);

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
            lang_badge: "ActionScript 3.0",
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
            lang_badge: "ActionScript 3.0",
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
        "Compiler ⌥F2",
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
            "⌃Space Hints",
            &[
                "All 25 Classic AS1/AS2 Variable Suffixes for Automatic Code Hints: `_mc` (MovieClip), `_txt` (TextField), `_btn` (Button), `_array` (Array), `_str` (String), `_sound` (Sound), `_xml` (XML), `_xmlnode` (XMLNode), `_xmlsocket` (XMLSocket), `_so` (SharedObject), `_nc` (NetConnection), `_ns` (NetStream), `_video` (Video), `_fmt` (TextFormat), `_lv` (LoadVars), `_lc` (LocalConnection), `_color` (Color), `_date` (Date), `_err` (Error), `_cam` (Camera), `_mic` (Microphone), `_mcl` (MovieClipLoader), `_pj` (PrintJob), `_cm` (ContextMenu), `_cmi` (ContextMenuItem).",
                "Comment Type Directives: Place `// MovieClip hero;` or `/* flash.display.Sprite enemy; */` in frame scripts to activate full member completion on untyped variables.",
                "Actions Toolbox & Code Hints (F9): The Actions panel left toolbox and `⌃Space` completion engine query the audited AS2/AS3 declaration catalog, the curated JS/TS/Web API catalog, and your project's `.as` / `.js` / `.ts` source files in real time.",
            ],
        );
    }

    sub_bullet(
        ui,
        "Switch Document Script Language",
        Some("⌘J"),
        "Open Document ▸ Document Settings… (⌘J) or the Actions panel header selector to choose ActionScript 3.0, ActionScript 2.0, JavaScript, or TypeScript.",
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
    ui.add_space(space::S2);

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
            title: "JavaScript Frame Script — Timeline Control, Event Listeners, Async & JSX",
            lang_badge: "JavaScript (ES2023+)",
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
        "NeoFlash includes a fast, zero-dependency Rust TypeScript front-end and bounded static type checker (`crates/neoflash-script/src/ts_check.rs`). You get immediate compile-time diagnostics in the Compiler Errors panel (`⌥F2`) and source-mapped JavaScript output without installing Node.js or `tsc`.",
    );

    let tabs = [
        (0, "TypeScript Pipeline (Parse → ts_check → Strip → Map)"),
        (1, "Checked Constructs vs Bounded Checker Scope"),
    ];
    c::Segmented::show(ui, diagram_tab, &tabs, false);
    ui.add_space(space::S2);

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
                        "Compiler Errors (⌥F2)",
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
        "Source Maps, Compiler Errors (⌥F2) & Bounded Checker Honesty",
        "Diagnostics",
        BadgeType::Accent,
        "⌥F2 Errors",
        &[
            "Exact Source Spans & Source Maps: Every TypeScript syntax or type error reports its 1-based line and column in the Compiler Errors panel (`⌥F2`). Double-clicking a diagnostic jumps the Actions editor directly to that line.",
            "Bounded Checker Honesty: `ts_check` is purpose-built for authoring scripts, interfaces, classes, unions, and utility types inside NeoFlash. It intentionally does not claim to be a full external `tsc` compiler and does not read `tsconfig.json` or `node_modules/@types`.",
        ],
    );

    code_block(
        ui,
        &CodeSample {
            title: "TypeScript Script — Interfaces, Discriminated Unions, Generics & Utility Types",
            lang_badge: "TypeScript",
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
        "When published to HTML5 (`File ▸ Publish`, `⇧F12`), NeoFlash bundles run inside a standard web browser alongside the WebAssembly runtime (`neoflash_web_bg.wasm`), canvas host (`player.js`), and Web Audio scheduler (`audio.js`). JavaScript and TypeScript scripts can interact with browser Web APIs subject to standard browser security, CORS, CSP, and autoplay policies.",
    );

    let tabs = [
        (0, "HTML5 Host Architecture (WASM + Canvas 2D + Web Audio)"),
        (
            1,
            "Browser Security Boundaries (CORS / CSP / Autoplay / Storage)",
        ),
    ];
    c::Segmented::show(ui, diagram_tab, &tabs, false);
    ui.add_space(space::S2);

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
            lang_badge: "TypeScript + Web API",
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
                 <text x=\"518\" y=\"40\" fill=\"var(--accent-default)\" font-size=\"11\" font-weight=\"600\">4. SourceMap (⌥F2)</text>\
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
            out.push_str(
                "<h3>NeoFlash 0.1.0-beta.1 — Release Identity, Install, Manual Update, Media &amp; Known Issues</h3>\
                 <table class=\"ref-table\"><thead><tr><th>Area</th><th>0.1.0-beta.1 Specification</th><th>Details &amp; Operational Guidance</th></tr></thead><tbody>\
                 <tr><td><code>Release Identity</code></td><td><code>NeoFlash 0.1.0-beta.1</code> · macOS 15+ Apple Silicon · Windows/Linux x64 Technical Preview</td><td>Windows is an unsigned portable ZIP; Linux is an Ubuntu 24.04-compatible portable tar.gz.</td></tr>\
                 <tr><td><code>Install &amp; Manual Update</code></td><td>Replace <code>NeoFlash.app</code> in <code>/Applications</code></td><td>Preferences, custom workspaces, recent files, and crash-recovery snapshots in <code>~/Library/Application Support/NeoFlash</code> are preserved across manual updates.</td></tr>\
                 <tr><td><code>Bundled Media &amp; Licences</code></td><td><code>Contents/Resources/media/bin/</code> or <code>media/bin/</code></td><td><code>ffmpeg</code>/<code>ffprobe</code>, exact application/Rust/media licence evidence and checksum manifests are included; corresponding source is supplied beside the platform archive. Set <code>NEOFLASH_FFMPEG</code> / <code>NEOFLASH_FFPROBE</code> only for custom overrides.</td></tr>\
                 <tr><td><code>Backups &amp; Bug Reports</code></td><td>Versioned <code>.nflash</code> JSON + <code>About NeoFlash</code></td><td>Keep <code>.nflash</code> projects in Git or regular file backups. Attach a minimal <code>.nflash</code> repro, <code>Output</code> panel diagnostics, and the build commit from <code>NeoFlash &gt; About NeoFlash</code> when reporting bugs.</td></tr>\
                 <tr><td><code>Known Issues &amp; Limits</code></td><td>VoiceOver, Intel macOS, Auto-Update, Windows/Linux acceptance, Tablet (<code>T-235</code>)</td><td>The unsigned Windows preview may trigger SmartScreen and lacks the connected private debugger. Linux is checked on Ubuntu 24.04 x64 with X11/Wayland and Vulkan/GLES, not packaged as AppImage/deb. Physical Windows 11 and broader Linux visual acceptance remain pending; legacy support is bounded and physical tablet hardware is experimental.</td></tr>\
                 </tbody></table>",
            );
        }
        10 => {
            out.push_str(
                "<h3>AS1 / AS2 Timeline &amp; Instance Handlers Quick Reference</h3>\
                 <table class=\"ref-table\"><thead><tr><th>Construct / API</th><th>Scope</th><th>Behavior</th></tr></thead><tbody>\
                 <tr><td><code>play / stop / gotoAndPlay / gotoAndStop</code></td><td>Timeline &amp; MovieClip</td><td>1-based frame index or frame label navigation on <code>this</code>, <code>_root</code>, <code>_parent</code>, or child clips.</td></tr>\
                 <tr><td><code>on(release | press | rollOver | keyPress)</code></td><td>Button / MovieClip instance</td><td>Attached classic object handlers executing in the owning timeline or clip scope.</td></tr>\
                 <tr><td><code>onClipEvent(load | enterFrame | mouseDown)</code></td><td>MovieClip instance</td><td>Lifecycle and per-frame hooks with <code>this</code> bound to the placed MovieClip instance.</td></tr>\
                 <tr><td><code>attachMovie / duplicateMovieClip / removeMovieClip</code></td><td>MovieClip depth zone</td><td>Dynamic symbol instantiation and teardown across AVM1 dynamic depths.</td></tr>\
                 </tbody></table>\
                 <h3>AS1 / AS2 Timeline &amp; Instance Code Example</h3>\
                 <pre class=\"code-block\"><code>stop();\nvar depth:Number = this.getNextHighestDepth();\nvar coin:MovieClip = this.attachMovie(\"CoinSymbol\", \"coin_\" + depth, depth, {_x: 320, _y: 180});\nhero_mc.onEnterFrame = function():Void {\n    if (Key.isDown(Key.RIGHT)) { this._x += 5; }\n    if (this.hitTest(_root.coin._x, _root.coin._y, true)) {\n        _root.coin.removeMovieClip();\n    }\n};</code></pre>",
            );
        }
        11 => {
            out.push_str(
                "<h3>AS2 Core Runtime Classes Quick Reference</h3>\
                 <table class=\"ref-table\"><thead><tr><th>Class / Object</th><th>Package</th><th>Key Members</th></tr></thead><tbody>\
                 <tr><td><code>MovieClip</code> (Drawing API)</td><td>Top-level</td><td><code>createEmptyMovieClip</code>, <code>lineStyle</code>, <code>beginFill</code>, <code>beginGradientFill</code>, <code>moveTo</code>, <code>lineTo</code>, <code>curveTo</code>, <code>endFill</code>.</td></tr>\
                 <tr><td><code>TextField &amp; TextFormat</code></td><td>Top-level</td><td><code>createTextField</code>, <code>text</code>, <code>htmlText</code>, <code>setTextFormat</code>, <code>setNewTextFormat</code>, <code>autoSize</code>, <code>wordWrap</code>.</td></tr>\
                 <tr><td><code>BitmapData / Matrix / ColorTransform</code></td><td><code>flash.display</code>, <code>flash.geom</code></td><td><code>draw</code>, <code>copyPixels</code>, <code>fillRect</code>, <code>getPixel32</code>, <code>perlinNoise</code>, <code>threshold</code>.</td></tr>\
                 <tr><td><code>Sound / Stage / SharedObject / LoadVars</code></td><td>Top-level</td><td><code>attachSound</code>, <code>loadSound</code>, <code>Stage.scaleMode</code>, <code>Stage.align</code>, <code>Stage.displayState</code>, <code>getLocal</code>.</td></tr>\
                 </tbody></table>\
                 <h3>AS2 Procedural Drawing, TextFormat &amp; BitmapData Example</h3>\
                 <pre class=\"code-block\"><code>import flash.display.BitmapData;\nimport flash.geom.Matrix;\nvar canvas:MovieClip = this.createEmptyMovieClip(\"badge_mc\", this.getNextHighestDepth());\ncanvas.lineStyle(2, 0x6C93FF, 100);\ncanvas.beginFill(0x1F2A48, 95);\ncanvas.moveTo(20, 20); canvas.lineTo(180, 20); canvas.lineTo(180, 70); canvas.lineTo(20, 70); canvas.endFill();\nthis.createTextField(\"label_txt\", this.getNextHighestDepth(), 32, 32, 140, 28);\nlabel_txt.text = \"READY PLAYER ONE\";\nlabel_txt.setTextFormat(new TextFormat(\"Inter\", 14, 0xFFFFFF, true));</code></pre>",
            );
        }
        12 => {
            out.push_str(
                "<h3>AS3 Display List Hierarchy Quick Reference</h3>\
                 <table class=\"ref-table\"><thead><tr><th>Class</th><th>Package</th><th>Capabilities</th></tr></thead><tbody>\
                 <tr><td><code>DisplayObject</code></td><td><code>flash.display</code></td><td><code>x</code>, <code>y</code>, <code>scaleX</code>, <code>scaleY</code>, <code>rotation</code>, <code>alpha</code>, <code>visible</code>, <code>filters</code>, <code>mask</code>, <code>localToGlobal</code>, <code>getBounds</code>.</td></tr>\
                 <tr><td><code>InteractiveObject</code></td><td><code>flash.display</code></td><td><code>mouseEnabled</code>, <code>tabEnabled</code>, <code>tabIndex</code>, <code>doubleClickEnabled</code>, keyboard &amp; pointer focus.</td></tr>\
                 <tr><td><code>DisplayObjectContainer / Sprite</code></td><td><code>flash.display</code></td><td><code>addChild</code>, <code>addChildAt</code>, <code>removeChild</code>, <code>swapChildren</code>, <code>graphics</code>, <code>buttonMode</code>, <code>startDrag</code>.</td></tr>\
                 <tr><td><code>MovieClip &amp; Stage</code></td><td><code>flash.display</code></td><td><code>currentFrame</code>, <code>currentLabel</code>, <code>totalFrames</code>, <code>stage.scaleMode</code>, <code>stage.align</code>, <code>stage.displayState</code>.</td></tr>\
                 </tbody></table>\
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
                 <h3>JavaScript Frame Script &amp; JSX Example</h3>\
                 <pre class=\"code-block\"><code>stop();\ntrace(\"Entering level:\", this.currentFrame, \"of\", this.totalFrames);\nconst badge = &lt;hud-badge label=\"SCORE\" value={0} /&gt;;\nthis.start_btn.addEventListener(\"click\", (event) =&gt; {\n    trace(\"Clicked:\", event.target.name);\n    gotoAndPlay(\"gameplay\");\n});</code></pre>",
            );
        }
        17 => {
            out.push_str(
                "<h3>TypeScript &amp; Built-In Checker (<code>ts_check</code>) Reference Table</h3>\
                 <table class=\"ref-table\"><thead><tr><th>Construct / Utility</th><th>Provenance</th><th>Static Verification &amp; Runtime Lowering</th></tr></thead><tbody>\
                 <tr><td><code>interface Name { ... }</code></td><td><code>[TypeScript]</code></td><td>Checked for required/optional (<code>?</code>) members and inheritance; erased at runtime.</td></tr>\
                 <tr><td><code>type Alias&lt;T&gt; = A | B</code></td><td><code>[TypeScript]</code></td><td>Unions (<code>|</code>), intersections (<code>&amp;</code>), tuples, and generic aliases; erased at runtime.</td></tr>\
                 <tr><td><code>enum Kind { A, B }</code></td><td><code>[TypeScript]</code></td><td>Checked for member access and lowered to a runtime lookup object.</td></tr>\
                 <tr><td><code>Record / Partial / Pick / Omit</code></td><td><code>[TypeScript]</code></td><td>Built-in utility types validated by <code>ts_check</code> without external <code>tsc</code> or <code>node_modules</code>.</td></tr>\
                 </tbody></table>\
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

pub(crate) fn render_browser_help_html_for_route(
    initial_theme: Theme,
    initial_mode: Option<super::help_manual::HelpManualMode>,
    active_page: Option<usize>,
    active_tag: Option<&str>,
) -> String {
    use super::help_manual::HelpManualMode;
    let hash = char::from(35u8);
    let clamped_page = active_page.map(|p| p.min(HELP_CHAPTERS.len().saturating_sub(1)));
    let initial_mode_slug = match initial_mode {
        None => "ALL",
        Some(HelpManualMode::AuthoringHelp) => "AUTHORING",
        Some(HelpManualMode::ScriptingReference) => "SCRIPTING",
    };
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
    let mut html = String::with_capacity(98_304);
    let _ = write!(
        html,
        "<!doctype html><html lang=\"en\" data-initial-theme=\"{}\" data-initial-mode=\"{initial_mode_slug}\"><head><meta charset=\"utf-8\">\
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
         nav.sidebar a{display:block;padding:6px 10px;border-radius:6px;color:var(--text-primary);text-decoration:none;font-size:12px;font-weight:500;overflow-wrap:anywhere}\
         nav.sidebar a:hover{background:var(--bg-hover);color:var(--accent-default)}\
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

    // Pure-CSS tag filter rules using :has() so selecting a tag radio highlights the pill and filters chapters
    for tag in HELP_ALL_TAGS {
        let _ = write!(
            html,
            ".help-shell:has(input[data-tag=\"{tag}\"]:checked) label[data-tag-pill=\"{tag}\"]{{background:var(--accent-subtle);color:var(--accent-default);border-color:var(--accent-default)}}\
             .help-shell:has(input[data-tag=\"{tag}\"]:checked) section.chapter-card:not([data-tags~=\"{tag}\"]){{display:none}}\
             .help-shell:has(input[data-tag=\"{tag}\"]:checked) nav.sidebar li.chapter-nav-item:not([data-tags~=\"{tag}\"]){{display:none}}"
        );
    }
    html.push_str(
        ".help-shell:has(input[data-tag=\"ALL\"]:checked) label[data-tag-pill=\"ALL\"]{background:var(--accent-subtle);color:var(--accent-default);border-color:var(--accent-default)}\
         .help-shell:has(input[data-mode=\"ALL\"]:checked) label[data-mode-pill=\"ALL\"],\
         .help-shell:has(input[data-mode=\"AUTHORING\"]:checked) label[data-mode-pill=\"AUTHORING\"],\
         .help-shell:has(input[data-mode=\"SCRIPTING\"]:checked) label[data-mode-pill=\"SCRIPTING\"]{background:var(--accent-subtle);color:var(--accent-default);border-color:var(--accent-default)}\
         .help-shell:has(input[data-mode=\"AUTHORING\"]:checked) section.chapter-card[data-section=\"scripting\"],\
         .help-shell:has(input[data-mode=\"AUTHORING\"]:checked) nav.sidebar li[data-section=\"scripting\"]{display:none}\
         .help-shell:has(input[data-mode=\"SCRIPTING\"]:checked) section.chapter-card[data-section=\"authoring\"],\
         .help-shell:has(input[data-mode=\"SCRIPTING\"]:checked) nav.sidebar li[data-section=\"authoring\"]{display:none}",
    );
    html.push_str("</style></head><body>");

    // Theme, section-mode and tag filters remain 100% declarative (zero <script>).
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

    html.push_str(
        "<header class=\"top-header\" id=\"top\"><div>\
         <h1>NeoFlash Help &amp; Reference (0.1.0-beta.1 · macOS + Windows/Linux x64 previews)</h1>\
         <p>Illustrated NF2 Authoring Manual (Chapters 1–10) &amp; Scripting Reference: AS1/AS2, AS3, JavaScript, TypeScript &amp; Web APIs (Chapters 11–19) · Manual macOS update preserves ~/Library/Application Support/NeoFlash · Bundled ffmpeg/ffprobe and licence evidence are included.</p>\
         </div><div style=\"display:flex;align-items:center;gap:8px;flex-wrap:wrap\">",
    );
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

    // Interactive Section Mode + Tag Filter Bar
    let _ = write!(
        html,
        "<div class=\"tag-bar\" role=\"toolbar\" aria-label=\"Filter by section and topic tag\">\
         <span class=\"tag-bar-label\">Section:</span>\
         <label for=\"nf2-mode-ALL\" data-mode-pill=\"ALL\" class=\"tag-pill\">All Chapters ({})</label>\
         <label for=\"nf2-mode-AUTHORING\" data-mode-pill=\"AUTHORING\" class=\"tag-pill\">Authoring Manual (1–{})</label>\
         <label for=\"nf2-mode-SCRIPTING\" data-mode-pill=\"SCRIPTING\" class=\"tag-pill\">Scripting Reference ({}–{})</label>\
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
        let _ = write!(
            html,
            "<li class=\"chapter-nav-item\" data-section=\"{section_slug}\" data-tags=\"{tags_joined}\"><a href=\"{hash}chapter-{}\"{nav_cls}>",
            index + 1
        );
        super::help_manual::escape_html(chapter.title, &mut html);
        html.push_str("</a></li>");
    }
    html.push_str("</ol></nav><main>");

    // When opened on a specific chapter (e.g. Chapter 14 inside Scripting Reference), render that
    // opened chapter first when it isn't already the first visible chapter in the active section, OR
    // keep natural order and highlight it — natural order with `#chapter-{n}` URL fragment scrolls
    // directly to it while keeping sequential reading intact.
    for (index, chapter) in HELP_CHAPTERS.iter().enumerate() {
        let tags_joined = chapter.tags.join(" ");
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
            "<section class=\"{card_cls}\" id=\"chapter-{}\" data-section=\"{section_slug}\" data-tags=\"{tags_joined}\"><div class=\"chapter-head\"><h2>",
            index + 1
        );
        super::help_manual::escape_html(chapter.title, &mut html);
        let _ = write!(
            html,
            "</h2><span class=\"badge\">{mode_badge} · Chapter {} of {}</span></div><p class=\"chapter-sub\">",
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
