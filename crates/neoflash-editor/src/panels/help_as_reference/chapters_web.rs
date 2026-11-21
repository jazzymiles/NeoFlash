use super::*;
use crate::panels::help_manual::{
    illustration_canvas, mode_card, paint_clipped_text, paragraph, section_overline, sub_bullet,
};
use eframe::egui::{self, Rect, Stroke, StrokeKind, Ui, vec2};
use neoflash_ui::{
    TextStyle,
    components::{self as c, BadgeType},
    icons::i16,
    tokens::space,
};

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

    crate::panels::help_manual::inline_cross_links(
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

    crate::panels::help_manual::inline_cross_links(
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
trace(\"Updated hero:\", moved.id, moved.x, moved.hp); // Output: Updated hero: p1 160 95",
        },
    );

    crate::panels::help_manual::inline_cross_links(
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

    crate::panels::help_manual::inline_cross_links(
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
