# Verification record

Date: 2026-10-27. This record covers the development preview, vector/object tools and clipboard, authored web semantics, ActionScript functions/control flow/exceptions, the bounded project/display/events family batch and binary preservation. It does not establish complete Flash compatibility. Historical desktop/browser observations apply only to their named increments; current family browser evidence and pending native acceptance are recorded below.

## Independent family verification of 01f1381

Run `FAMILY-HTML5-20261027T0057Z` independently reused exporter/JS/WASM after all 69 recorded source hashes matched commit `01f13815a115626411c40ca04e8c13da1beadb71`. The ordinary seven-case export/browser batch confirmed selected class/typed-field/Restart, display geometry, AS2 callback, keyboard and regression behavior. It did **not accept the complete package**: real trusted mouse clicks are silently discarded by the shared `isPrimary === false` guard in `web/stage.js`, while semantic Enter activation succeeds. A diagnostic extension of the existing event scenario recorded `PointerEvent`, `isTrusted=true`, `isPrimary=false`, `pointerType=mouse`, `button=0`. The unchanged original class example reproduces the same failure. Legacy authored Click and AS2 onPress remain functional.

The run preserves successful bounded results and the shared `FAMILY-INPUT-001` P0 separately; the collection/plain-TextField changes made afterward inherit no pass. Browser: Codex in-app browser only, engine/version not separately identified; original Flash was not run (`documented_only`). Native Sources/Undo/reopen/D-008 remain pending because the existing native session has an unsaved document. One aggregate dashboard MutationObserver exception is separately recorded as `FAMILY-HARNESS-001`; DOM-ready and zero visible runtime alerts are not a clean-log guarantee. Main shared-suite results were retained as reported evidence without another full rerun.

Full source fixtures, expected/actual traces and fingerprints: `docs/actionscript-reference/implementation-evidence.json`, run `FAMILY-HTML5-20261027T0057Z`. Local artifacts: `target/html5-family-independent-20261027T0057Z/acceptance.json` and `browser/`. The normalized declaration audit carries only sanitized provenance and 45 explicit per-entry decisions; it is not a class-wide promotion.

## Automated checks

`./scripts/check.sh` passes on Apple Silicon, macOS 15.8, Rust 1.99.1:

- Formatting and Clippy (`--workspace --all-targets -- -D warnings`).
- 309 workspace Rust tests: 55 core, 121 editor, 35 script, 94 importer and 4 exporter; four additional tests exercise the pinned egui-winit paste adapter.
- 66 browser-host/actual-WASM/cache and public-example tests in Node, including the current display, Stage and EventDispatcher family, semantic bounds, DOM identity, focus restoration, pan/zoom, lifetime-checked activation, recovery and failure reporting.
- 90 execution fixtures using JavaScript emitted by the script compiler, plus an integration check using the actual host proxy/dispatch code with a lifecycle stub.
- One combined project-program scenario covers class resolution/inheritance/slots/events, ordinary export and linked instances through actual WASM and host code.
- Five batch-preparation tests cover independent failure/timeout continuation, fresh output runs, source/file hashes, argument and HTML-data handling, and preservation of unrelated output directories.
- 21 public-corpus CLI cases: seven successful imports and 14 explicit refusals, with source integrity and no-overwrite checks.
- Release WebAssembly build and standalone demo export.

Important regressions include independent nested clocks while a parent is stopped; affine nesting; dispatch order/stale-instance protection; JSON validation and graph cycles; grouped undo; key insertion at an interpolated pose; midpoint duplicate/delete; symbol conversion and recursive-placement rejection; key move/copy/collision behavior; fast and normal pointer gestures; macOS collection-font loading; and the Metal backend being enabled in the actual desktop build.

The build emits an upstream future-compatibility notice for the Metal dependency `block 0.1.6`. This is not a current compilation or Clippy error; dependency modernization remains tracked work.

## AS2 project classes and classic input — 27 October

The shared `scripts/check.sh` run passed once for this delivery: 309 workspace Rust tests, four adapter tests, 66 Node tests and 90 compiler fixtures, with AS2 added to the existing combined project scenario. Test counts did not increase. The scenario publishes the original `examples/as2-project-input.nflash` through the ordinary exporter and actual WASM/host; it covers inherited linked instances, shared prototypes/statics, erased annotations, keyboard/mouse/Stage listeners, lifetime cleanup and Restart. Log: `target/verification/as2-program-input-check.log`.

All ten ordinary exports reached ready DOM state in the in-app browser, run `20261027T013320Z-396cc02a`. Actual coordinate clicks on both AS2 cards incremented each once; Left/Right then produced two counts per instance and shared total four. Space reset both. H/S key-up traces were observed; cursor CSS was verified in the host scenario, not independently read from the iframe. Restart reported two fresh instances/zero shared state, and the next coordinate click produced one component callback and one Mouse callback.

The same run checked the shared pointer correction against the original AS3 class and Stage examples. Coordinate clicks produced `component click left 1 1` and `component click right 1 2`; Restart returned `project ready 2 0 0`. The Stage pad delivered capture/target/bubble, listener removal suppressed the next callback, and Right moved the pad. After Restart, the next click again reported `Pad 1 capture 1 phase 3`. This closes `FAMILY-INPUT-001` for this observed in-app-browser route; it does not establish other-engine or full pointer conformance.

No project runtime error overlays appeared. The aggregate console still captured one unlocated `MutationObserver.observe` Node-type exception at startup, despite the dashboard's own guarded iframe observer reporting zero observation errors. `FAMILY-HARNESS-001` remains open; its source is not established and the whole page is not claimed error-free. The remaining seven examples received startup checks only. Full manual checklists, original Flash comparison and native Sources/save/reopen acceptance were not claimed. Artifacts: `target/verification/as2-program-input-browser/observations.json`, `console.json`, `as2-restart.png` and `stage-restart.png`.

The optimized macOS package built and strict codesign verification passed. Executable SHA-256 `f3a847b94b7a02d38a3a743c49564c6ef040a0b65ee5b0474bc2927da61ad6a0`; WASM `27797ba59dc7f2b53f67499c4ee9cb1cddc2b1b873f80c6796d0c30c268c701d`; exporter `a6bb3319a8114b96225ce98c20d9adcb1e6b8809fd524c9b5ef08046cfe524ae`. Sources and build identity are retained in `target/verification/as2-program-input-build-evidence.json`. Native interaction is pending; the existing unsaved native session was preserved.

## Array, String and plain text families — 27 October

The next delivery extends existing scenarios instead of creating a per-method test matrix. Rust test and Node test counts remain unchanged; four coherent Array/String scenarios bring the compiler execution batch from 86 to 90. The existing project scenario also exercises the shared adapters in classes, local shadowing and single property lookup before argument evaluation. The existing display scenario covers live text/color mutation, UTF-16 length/ranges, line queries, measurement, owner guards and reset through actual WASM.

The shared run passed formatting, Clippy, Rust, corpus and WASM build stages. One Node scenario exposed a harness omission: supplying Canvas measurement also enabled an input-render callback, but the extracted headless harness had no renderer. A no-op renderer was added to that harness; production rendering was unchanged. The affected scenario passed, and the remaining compiler/project/batch/export stages then passed. Unrelated tests were not repeatedly rerun. Logs: `target/verification/collections-text-integrated-check.log` and `collections-text-final-check.log`.

The previous committed family CI run failed because several Node launchers assumed Cargo lived at `~/.cargo/bin/cargo`; that runner supplied it elsewhere on PATH. The launchers now share the existing environment/home/PATH fallback and expose process-launch errors. The affected export scenarios and project scenario passed with `CARGO=cargo` (log `target/verification/cargo-path-check.log`). This is a test-tool portability fix, not a change in published project behavior. The corrected `a05a08d` then passed the complete [GitHub Check workflow](https://github.com/jazzymiles/NeoFlash/actions/runs/40431457501) and Progress Pages.

All nine ordinary exports in `examples/browser-batch.json` reached ready DOM state in the in-app browser, with zero reported runtime errors (run `20261027T010839Z-00a9a758`). In the new combined example, the AS2 and AS3 controls both rendered Zoe:45, Ada:30 and Bob:20; AS3 additionally rendered Total:95. The AS2 result changed to cyan and AS3 to green. Restart visibly restored the authored labels and colors; repeating both actions produced the same result and traces. The other eight projects received startup checks in this run; their full interaction checklists were not repeated.

This verifies the ordinary data-to-text pipeline, not every Array/String method, original Flash equivalence, browser engine parity or full text layout. Plain text uses the existing Canvas font/line profile; HTML/CSS formatting, text input, wrapping and font embedding remain pending. Native Sources/editor acceptance also remains pending. Browser observations and screenshots are in `target/verification/collections-text-browser/observations.json`, `results.png` and `reset.png`.

The optimized package was rebuilt and strict codesign verification passed. Executable SHA-256: `859c36cbe933400100de87c503a88258a9c33b2cdaaa544cb82e81480d1e8183`; WASM: `27797ba59dc7f2b53f67499c4ee9cb1cddc2b1b873f80c6796d0c30c268c701d`; exporter: `5768d800603eba24131d3942ab842b67f5620143160f84d78131d6ef40c8d1c2`. Build/source identity is retained in `target/verification/collections-text-build-evidence.json`.

## Integrated project, display and event families — 27 October

The frozen family implementation passed one shared `scripts/check.sh` run. The local log is `target/verification/family-integrated-check.log`. The totals above describe that run, not a count of implemented Flash methods or a compatibility percentage. Representative family scenarios are used together; a separate browser/test cycle for every API is not required for this delivery.

The project scenario executes imports, project inheritance and super calls, typed fields, static/const members, stable bound methods and independent objects. Its second half publishes `examples/project-program.nflash` through the ordinary exporter and runs the emitted program with the actual WASM timeline and host: a linked document class accesses two initialized linked children, their instance counts remain separate, shared state changes, and Click/EnterFrame/Stage keyboard paths operate. Constructor failures remain failures on retry, and native member redeclaration is diagnosed. This is executable evidence for the bounded class profile, not complete AS3 initialization/type conformance.

The display scenario connects child identity/name lookup, transformed bounds, coordinate conversion, supported dimension changes, public void timeline results and AS2 callbacks to the same real host/WASM instance graph. Stage/events tests exercise capture/target/bubble, priority and phase snapshots, cancellation, redispatch cloning, stale/replaced lifetimes, focus, keyboard and mouse coordinates, and restart cleanup. Installed input listeners are driven through DOM EventTarget in Node; that does not substitute for actual browser input or pixel inspection.

The Sources editor is implemented in the Actions workspace: new/import/remove source files, source paths and languages, document/symbol linkage, project checks and source-specific feedback. Model tests cover version-2 save/open boundaries and linked-source clipboard conflicts. Native Sources interaction, detached-window feedback, source editing/Undo and reopen acceptance have not yet been performed for this build.

`scripts/prepare-browser-batch.py` uses an existing exporter and never builds code. It continues after an individual compile/export failure, records source/export hashes and logs, and creates one dashboard with iframes and manual checklists. It observes visible loading/status/error DOM with one readiness deadline per case, without runtime debug globals or automatic retry cycles. Export success, observed DOM readiness and manual behavior acceptance remain separate fields; the reviewer can download the observations JSON. Local-file frame isolation is reported as unobservable, with a local HTTP serving hint.

The ordinary batch manifest is `examples/browser-batch.json`. All eight cases exported successfully to `dist/acceptance/index.html`: project program, display family, Stage/events, visibility, numeric profiles, keyboard controls, vector paths and nested animation. Export fingerprints and per-case results are in `dist/acceptance/batch.json`; the export report establishes export success separately from the browser observations below.

| Acceptance item | Current evidence / remaining record |
| --- | --- |
| Shared automated family pass | Passed; `target/verification/family-integrated-check.log`. |
| Ordinary batch export | Eight of eight exported; manifest and generated batch report above. |
| Browser startup/rendering | In-app browser: all eight frames reached ready DOM state, with zero reported runtime errors. Screenshots and observations are recorded below; other browser engines are not covered by this batch. |
| Browser behavior | Representative linked-class counts/shared state/Space reset, display geometry traces, Stage capture/listener removal/ArrowRight and fresh Restart state observed. This does not complete every manual checklist item. |
| Native Sources/editor workflow | Pending live source editing, feedback, Undo, save/reopen and relevant panel checks. |
| Packaged artifact identity | Rebuilt and strict codesign verification passed; fingerprints below. Native launch/Sources workflow remains pending. |

The final observed batch run was `20261027T005223Z-a5d8adae`. Linked widgets produced left `1/1` and right `1/2`; Space reset their instance counts while retaining the shared click total. The shared Restart check exposed retained static state: the host now recreates the program and resolves the public root against the live program. The existing project scenario passed after this fix, and a fresh eight-case export/browser pass observed initial `2/0/0` state followed by left `1/1`; the Stage example restarted with a fresh count of one. The shared suite preceded this isolated fix; the focused scenario and browser check cover its final state. Evidence: `target/verification/family-browser/final-observations.json`, `batch.png`, `batch-top.png` and `target/verification/family-build-evidence.json`.

Final executable SHA-256: `4b89babde4423c038461f77ba9a66990cfcada80cea2adb25363271ab5adb033`; WASM: `a1f6d1a9f98d9d426959ebf80296d355552cbbf5bb19c4205aa8de6fda66ad17`; exporter: `b09e23caed7d503c3b6fba18c42395145929081d1e3fa520d21665270266fb77`.

Sources, screenshots and logs remain under ignored `target/verification/`. A missing resource, runtime error or failed manual case must remain visible in the batch report; successfully exporting the other cases does not turn it into a passed behavior check.

## Observed desktop checks

The optimized `dist/NeoFlash.app` was launched through the macOS UI:

- Classic panel layout, document scene and system-font text rendered successfully.
- A rectangle drawn by a native mouse drag appeared with the expected 200 × 100 document-space dimensions. One Undo removed it and restored the clean document state.
- Text creation and F8 symbol conversion worked in the running application.
- A test document was saved through the native Save dialog and its JSON was inspected; it contains the text and new symbol. The test file stays under ignored `target/verification/`.
- Actions accepted code entered by keyboard/paste, Check Script accepted it, and Undo restored the original handler.
- Library double-click entered Orbit; scrubbing selected frame 15/61, with onion skin enabled and nested content visible around the symbol editing origin.

Issues found during launch/input testing were fixed: missing Metal backend feature, invalid collection font index, drag origin selected after movement, and complete pointer gestures delivered in one repaint. Source-level review also fixed stale selections, empty timelines, midpoint edits and language selection on empty scripts.

This is a smoke check, not a complete native UX, accessibility, clipboard, IME, multi-display or performance audit. Reopening via native Open dialog, all font families and all gesture combinations have not been exhaustively tested. JSON save/open is covered by core tests.

The subsequent vector increment has headless egui gesture tests for pen curves/closing, pencil samples, transformed point edits, grouped Undo, cancellation, frame changes, closed seams, scaled picking and saving a pending draft. Native tessellation tests verify compound hole area. The new release bundle builds, but live native pen/pencil/subselection checks remain pending because the Mac is locked. Earlier desktop observations above describe the foundation build and must not be treated as verification of the new tools.

The Accessibility panel is covered by editor tests for sampled-pose key insertion, grouped Undo, stopping playback during an edit, locked-layer rejection and preserving exactly one semantic target during symbol conversion. Live native checks of this panel are also pending the Mac being unlocked.

### Selection and Free Transform increment

The 78 editor tests include pure transform mathematics, tessellated primitive geometry and actual egui input batches. New coverage exercises cross-layer eased-pose edits, Shift toggle/additive marquee, nested-instance selection, locked foreground objects, whole-selection duplicate/delete, final release outside the stage, cancellation and no-op history, and frame/tool/view changes. Timeline tests verify that a key moves to the release column while retaining its original layer. Real menu-click events also verify Undo of an open Pen draft with empty history, then Redo of the completed path.

Transform tests cover rotated and reflected objects, opposite-anchor and center resizing, aspect preservation, singular/finite limits, group scaling/rotation and angle snapping. Gesture regressions drive Q resize on a mirrored object, Option+Shift resizing, multiple-object scaling, and a two-turn rotation recorded as one Undo step. Point edits and object drags returning to their original sampled pose do not leave a keyframe or history entry. Pen/Pencil regressions cover batched outside release and changes of viewport during drawing.

Rectangle/ellipse tests check fill and stroke mesh areas/bounds under scaled, reflected and nested transforms, degenerate geometry, ellipse tessellation tolerance, and the meshes submitted to an egui Painter. These are geometry checks; they do not replace visual comparison on a display.

The optimized bundle was rebuilt and passed `codesign --verify --deep --strict`; its minimum system version is 15.0. Executable SHA-256: `edacc8de6d6721feb1472b934faf4b77d489d7198963995849832e3dcdbf9baf`. The Mac remains locked, so live mouse/keyboard and visual checks of this release remain pending. No previous native screenshot is offered as evidence for the new V/Q tools.

## Script and preservation regressions

ActionScript execution checks cover function hoisting, recursion, lexical closures, shadowing, typed arguments/returns, exact supported argument counts and independent instance state. Review found and fixed duplicate numeric conversion in typed postfix updates, escaped object-property keys, line terminators before postfix operators, conditional-branch assignment parsing, and AS trace bypassing the browser host's visible output. The host integration uses distinct trace and console observations, so writing directly to console cannot satisfy the visible-trace regression.

Binary geometry tests exercise every truncated prefix of original fixtures, exact source-byte retention, invalid style references, optional fields, coordinate wrapping and shared allocation budgets. A separate local deterministic malformed-input probe ran 20,000 inputs through the public shape decoder without panic or out-of-range diagnostic offsets. This probe complements the committed fixtures; it does not prove safety for all inputs or visual fidelity.

Thirteen Contents tests cover versioned prefixes, truncated input, descriptor/class/object identities, selected-scene references, missing/duplicate/wrong-role streams, Unicode and compact string boundaries, nested metadata and collection budgets. Flash 3 tests distinguish exact EOF, supported publish/tail records, unknown/nonempty shared-library data and trailing bytes. Raw settings retain fractional FPS and color words; unread suffix bytes remain exact.

Nine typed timeline tests cover ordered ownership, class/object/related references, partial ancestors, every truncated prefix, invalid/shared/cyclic child edges, frame durations, labels, source-action spans, action-presence boundaries and exact shape associations. Shared document retention limits preserve consistent partial graphs. An independent source review checked reference reservation, ownership, budgets, source-presence semantics and F3 EOF against the pinned primary implementation. These checks establish preservation within the documented boundary, not complete document conversion.

## Clipboard increment

Twelve core clipboard tests cover reachable symbol closure, painter order, repeated placement identities across keys, cross-document collisions, live same-document symbol edits, missing-definition recovery, sampled destination keys and atomic validation failure. Unknown/duplicate fields, unsupported versions, cycles, locked/hidden layers, invalid transforms, byte and item budgets are checked. Names, script source, labels, tween/playback data and accessibility metadata are retained.

The runtime regression uses the actual shared `Player` on a transferred nested project. It verifies separate instance lifetimes/clocks, sibling independence under stop/goto/property overrides, source/language/context of frame and object dispatch, click bubbling, reset, and the same transcript and scene after serialization/reopening. Scripts are dispatched as data by this core test; execution remains covered separately by the compiler/browser-host suite.

Ten editor clipboard tests use real egui events and menu clicks with an injected clipboard reader, never the user's OS clipboard. They cover one output per Copy/Cut, Cut and Paste history, centered/in-place modes, text-field ownership even for a valid object payload, deferred text delivery/cancellation, menu focus restoration, playback stopping before text delivery, empty/unavailable clipboard, malformed input, and no duplicate paste from a combined key/paste input batch. Review found the playback tick ordering issue and the tests now guard against it.

The original native paste event did not carry key-press modifiers. Its first V-release fallback could not recover Shift if it was released before V in the same batch. The later adapter increment below replaces that fallback with initiating-key metadata. Live OS clipboard exchange and the new menu's visual checks remain pending the Mac being unlocked.

The rebuilt bundle passed `codesign --verify --deep --strict` and retains deployment minimum macOS 15.0. Executable SHA-256: `c0ef61a7ac73089704684d4acf48a5a97140cb3b6b708ecd6b01837756dcd871`; packaged WASM: `9b564330387b99c739fb4326d9567c882f9234c06ebb048b2566a0c5cec2085d`.

The regenerated demo was served only from `dist/demo` over loopback and checked in the in-app browser. Animation advanced; Pause held frame 41; a click emitted `Orbit rotated`; Restart resumed near frame 3; a subsequent Pause held frame 70 and its object handler still worked. Captured console errors were empty, with the known nonembedded-font warning. An independent frozen-export review at 1280×720 also passed animation, pause, click and restart checks and verified the embedded WASM fingerprint. This is a runtime regression smoke check, not a live clipboard or transferred-project acceptance test. Screenshot: `target/verification/html5-clipboard-runtime.png`. Temporary tabs and the test server were closed.

## Control flow, solid contours and paste intent — 27 October

Structured control-flow fixtures exercise switch evaluation/fallthrough, do/continue, AS3 labels, throw identity, ordered typed catches, lexical catch captures, finally precedence and return coercion. Independent review found two additional defects, now guarded by fixtures: a method assigned to an outer receiver inside catch must remain callable afterward, and the Error facade's typed toString return must be coerced. Error message construction follows the pinned Adobe VM implementation, whose untyped message differs from the published documentation signature; this is recorded in the compatibility matrix.

The actual-host failure tests cover 13 raw payloads and the same 13 inside AS2 envelopes, including null/undefined, primitives, errors, hostile accessors/conversion and revoked proxies. A full host boot test first reproduced a startup error preventing the animation loop from being installed. The fix leaves failed projects stopped but restartable. The original recovery example also runs through the Rust compiler, real WASM Player and shipped host code, checking independent counters/clocks, cross-handler payload identity, activation and reset.

In the in-app browser, the example produced `AS3 Try again cleanup 1`, `AS3 Accepted cleanup 2`, independent `AS2 Try again cleanup 1`, then keyboard-triggered `AS3 Ready cleanup 3` and `AS2 Accepted cleanup 2`. Restart reset the counters. Separate exports displayed the original AS3 null and AS2 string failures with frame location, and a safe placeholder for a hostile JavaScript thrown object. A one-time startup failure recovered after Restart, advancing from frame 6 to 104 before Pause; its click handler still worked. No application errors occurred in the recovery example; expected font warnings remained. Temporary tabs and servers were closed.

Ten contour tests cover exact units/style sides, shuffled edges, quadratic reversal, alternating nested holes/islands, same-fill internal edges, full affine baking, malformed IR, unsupported paints, intersections, uncertainty and work/precision bounds. Review added a guard for distinct points collapsing at f64 precision. A tangent fixture also caught an invalid supporting-plane shortcut in the initial certificate; both hulls must have the tested plane as an extremum. The original adapter-derived project was visually checked at 1280×720 in HTML5: rectangular hole, quadratic lens and reflected/sheared parallelogram rendered correctly, with no console errors. This is a visual check, not an exact pixel comparison or complete binary FLA import.

The local aggregate candidate audit examined 136 nonempty shape records in six accepted Flash 3 containers. Seven passed the strict adapter (seven contours, 80 boundary edges; six had a complete enclosing record). The remaining candidates report 106 unsupported-feature, 22 unsupported-paint and one uncertain-intersection diagnostic. Empty shapes and other save targets were skipped. All binary documents remain blocked for editable import; candidate shape success does not establish document-wide compatibility. No private artwork was rendered or published.

Four adapter tests and twelve editor clipboard tests cover initiating-key metadata, either release order, later Shift changes, repeated/mixed batches, text delivery once and menu routing. After the Mac was unlocked, the old process was closed and the new bundle opened the recovery example through the native Open dialog. Keyboard/menu Paste in Place retained X=60 and X=123, verified from saved JSON. Copying two selected instances, creating a new document and pressing Paste produced two placements sharing one newly identified library symbol, retaining AS2 and AS3 handlers. The first automated key sequence received foreign clipboard text; isolated subsequent Copy and menu/keyboard Paste checks succeeded. Its cause was not established, so exhaustive timing/platform clipboard conformance is not claimed.

A subsequent native Actions check confirmed keyboard Copy/Paste and actual Edit-menu Copy/Paste with distinct text markers; Cut cleared selected text while retaining artwork. Save and native Open round-tripped the QA copy. Recursive JSON comparison against the cross-document fixture found only the added root-frame script; all artwork and identities were unchanged. Test Movie opened the local export directly in Safari, where the expected trace, independent AS2/AS3 recovery handlers and Restart worked. The initial foreign clipboard text was not reproducible after fresh Copy cycles. This is a specific local Safari acceptance, not a general browser conformance claim.

The optimized bundle passed `codesign --verify --deep --strict` with minimum macOS 15.0. Executable SHA-256: `6cd7ae793200a3153eba0391be855bb119a1616c60b0827e348f26789e487129`; packaged WASM: `9b564330387b99c739fb4326d9567c882f9234c06ebb048b2566a0c5cec2085d`. Test/build logs and original UI fixtures remain under ignored `target/verification/`. Screenshots include `html5-script-recovery.png` and `html5-binary-contours.png`.

## Native workspace increment — 27 October

The workspace-only release was rebuilt after the clean full suite and passed strict ad-hoc signature verification. Executable SHA-256: `f52b1598601c4fa8b24de1ac70984c3e5034aea049b30d7f7ea534f800f82274`; packaged WASM: `9b564330387b99c739fb4326d9567c882f9234c06ebb048b2566a0c5cec2085d`. These fingerprints identify this UI acceptance build, before the subsequent real-FLA runtime increment.

Actual macOS checks observed:

- A saved detached Actions window and Dark theme survived restart. Its editor, language selector and Check Script button appeared in the native accessibility tree; activating Check Script produced the accepted-syntax status in the root window. Closing the child and pressing F9 reopened and focused it; Dock returned it to the shared editor.
- Light theme rendered readable headings, controls and selection states. Earlier native inspection found an inverted strong-text color in both themes; semantic palette and contrast regressions now cover that defect.
- A native Timeline restored its 750×380 content size. At 320×180, its footer scrolled to the opacity control and a real click changed opacity from 0.30 to approximately 0.565. Restoring 750×380 showed the complete footer.
- An original 501-frame, 50-layer, 100-symbol fixture opened through the native Open dialog. Five- and sixty-frame tween arrowheads retained the same bounded size. Scrolling to layers 43–50 kept row labels aligned with keys and the ruler fixed. Library scrolling exposed later symbols. The fixture remained clean.
- Before the accessibility patch, Tools detached as a real window and choosing Pencil there remained selected after Dock. Actions text selection and native Copy/Delete/Paste also survived docking in that build. Those earlier observations establish shared state behavior, not child accessibility delivery.

Headless regressions additionally cover geometry recovery across display layouts, corrupt/future preferences, panel visibility and F9 during text editing, unchanged document/history/drafts, both theme contrast palettes, tool toggles and keyboard focus, stable Actions editor IDs, clipboard delivery to its originating viewport, compact footer reachability, and stable root layout over repeated frames. Eframe feature probes compile both with and without AccessKit.

The first patched native startup panicked because AccessKit requires initialization before a window is first shown. A first-panic backtrace identified this precondition; children now start hidden, receive the adapter, and then use their requested visibility. The subsequent launch and child actions above passed without clearing saved preferences. No panic suppression is installed.

Actual window relocation across monitors, display disconnect/recovery, VoiceOver speech, IME and the full browser/desktop accessibility matrix remain unverified. Drag-to-dock/tab/split layouts and named presets remain future work. Evidence stays in ignored `target/verification/`, including `workspace-timeline-scroll.png` and the clean suite/package logs.

## Observed HTML5 checks

A real Chrome tab loaded the generated export through a temporary server bound only to `127.0.0.1`, serving only `dist/demo`:

- Animation advanced through frames.
- Pause held the current frame.
- Clicking the orbit ran its object handler, rotated the instance and emitted `Orbit rotated`.
- Restart resumed playback after a pause.
- No application errors appeared in the console; the expected font-fallback warning was present.

In the initial Chrome/browser-tool check, direct `file://` navigation was unavailable. A later native Test Movie check opened and exercised a local export in Safari, as recorded above. The exporter embeds WASM bytes and all local scripts to support an autonomous folder; this packaging is covered by tests. Safari and clean-machine typography still need explicit verification.

### Vector path increment

The original editable example `examples/vector-paths.nflash` was exported to `dist/vector-demo` and inspected in a real Chrome tab, again served only over loopback:

- Cubic and quadratic Bézier strokes, an even-odd cutout and a concave filled path rendered correctly. The animated wave is inside a transformed symbol instance.
- A click inside the ring's hole left the trace unchanged; clicking the painted ring emitted `Even-odd fill clicked`.
- A click in the empty corner of the concave path left the trace unchanged; clicking its filled arm emitted `Concave fill clicked`.
- Clicking the thin Bézier stroke emitted `Bezier stroke clicked`, including through its instance transform.
- No application errors appeared in the console.

`web/tests/path-browser.html` ran **10 checks in actual browser Canvas**, using the same `web/path.js` geometry helper shipped by the exporter. All passed: concave fill and empty bounds, even-odd versus nonzero winding, transparent fill, quadratic and cubic stroke geometry, explicit versus implicit closing strokes, round end caps, and hit testing independent of the caller's DPR/transform. This is a manual browser suite; the Node command does not run it. The WASM suite separately verifies preservation of compound path commands/fill rules and nested affine transforms.

The path cache has both an entry limit and an 8 MiB conservative UTF-16 key budget, with oversized geometry bypassing retention. Node regression tests cover eviction and oversized-entry behavior. This is an estimated retained-input bound; browsers control native `Path2D` allocation.

Screenshots: `target/verification/html5-vector.jpg` and `target/verification/path-browser-tests.jpg`. These checks establish Chrome rendering and pointer behavior for the cases above; Safari, native vector editing gestures and exhaustive path/performance conformance remain separate checks. Temporary test tabs and servers were closed after verification.

### Authored semantics and preview scale

The original `examples/keyboard-controls.nflash` was exported to `dist/keyboard-demo` and tested in real Chrome over loopback HTTP, including a 390×844 viewport override:

- Tab focused the named Blue button; Enter produced `Blue 1`, Space produced `Blue 2`. Tab moved to Red; Enter/Space produced `Red 1` and `Red 2`. Each action incremented once and instance state remained independent. These handlers use the AS3 function compiler and the shared WASM runtime.
- The same DOM button retained focus while its affine bounds animated; its visible 3px outline followed the displayed artwork. The described image did not enter Tab order. The disabled button was exposed as disabled and Tab skipped it to Pause.
- At 390px width, Fit displayed the complete 960×600 stage at 358×223.75 CSS pixels. The transport and scale controls remained reachable.
- At 100%, the Canvas measured exactly 960×600 CSS pixels with a 992px-wide scrollable area. A real Pan drag changed horizontal scroll from 301 to 541 without changing the script trace. Keyboard focus then scrolled Blue into view.
- No application errors were captured. The known nonembedded-font warning remained.

An independent review of a frozen export confirmed the same interaction criteria in the in-app browser, including 200% keyboard focus scrolling, returning from Pan to pointer activation, and resetting Fit to zero scroll. Its release executable fingerprint was `f32d1841931a9f841373481d437223ff9944c67ea4534b8f523382481ae92af8`. This separately supports the HTML5 D-001 and D-002 acceptance criteria, not complete accessibility or native authoring verification.

Automated regressions additionally cover affine miter bounds, offstage descendants, transparent and provably degenerate fill-only artwork, role and lifetime replacement, inherited disabled state, focus restoration without unwanted scrolling, pan cancellation across resize/zoom/capture loss, and computing leaf geometry once per semantic update. Core checks reject singular ancestors even when affine multiplication introduces a small nonzero determinant through floating-point rounding.

Screenshots: `target/verification/html5-keyboard.jpg`, `html5-keyboard-390-fit.jpg`, and `html5-keyboard-390-pan.jpg`. Browser viewport overrides were restored, and test tabs/servers closed. Actual VoiceOver speech, Safari, physical touch input and direct `file://` navigation remain unverified. Narrow-screen Fit intentionally scales a fixed authored stage; responsive text/layout authoring remains separate work.

## Real FLA to HTML5 — 27 October

The unchanged Adobe CS6 archive `examples/fla-corpus/upstream/ruffle/avm1-frame-labels/test.fla` imports to `examples/fla-corpus/imported/ruffle-avm1-frame-labels.nflash`. An independent review compared all nine original script strings, both solid contours and 1px strokes, labels, translation and the blank key against the source ZIP. All upstream payload hashes remain unchanged.

The ordinary exporter published that native document. Actual in-app browser inspection observed the red square with black outline, root frame 3/7, and all 17 trace lines exactly matching the pinned upstream `output.txt`. Restart produced exactly a second 17-line block, with no console errors or warnings. The original SWF was not executed during this check; its pinned companion trace is the independent reference. The real-WASM/actual-host regression also checks extra stable ticks and reset.

Independent review found and fixed three XML acceptance holes: fragmented script/static text must retain every immediate text/CDATA node; namespaced attributes cannot shadow an unqualified XFL attribute; unchecked nonempty folder/swatch metadata must block import. Eight regressions cover these guards. The final importer suite has 80 tests, and the complete integrated suite above passed after these fixes. See the corpus VERIFICATION.md and IMPORTS.md for exact source provenance and geometry/metadata limits. Tests and browser evidence remain under ignored `target/real-fla/`.

The integrated optimized macOS bundle also built successfully: executable SHA-256 `d2614307f6539ef4c82e9a4bf422c8cf2a860f3ec2820c65dda6dd43941597c3`, packaged WASM `6888aff20b6ab0e4b73d3f953e45592907754609a4ae4fd0e1aeb4e22b657d2d`. The detailed workspace acceptance above identifies its earlier UI-only binary separately.

This proves one real FLA workflow. General AS2/AS3 semantics, binary FLA conversion and broader geometry remain open. The bounded hidden-layer/placement increment below is separate evidence.

## HTML5 correctness: visibility, placement and numeric profiles — 27 October

Original sources: `examples/visibility.nflash`, `examples/symbol-replacement.nflash`, `examples/as-intrinsics.nflash`, and importer fixtures `hidden-layers` / `instance-gap`. The hidden-off source changes only PublishSettings.xml InvisibleLayer from 1 to 0. All were published with the ordinary CLI exporter and served only from `dist/p0` over loopback HTTP. No test-only runtime globals or direct application-state evaluation were used for browser acceptance.

Actual checks in **Chrome** and the **Codex in-app browser**:

- Boolean AS2/AS3 visibility removes both card pixels and semantic buttons; showing restores them. Hidden clips continue EnterFrame ticks, independent clocks and new-child Load. Click counters and custom state survive hiding. In-app pointer clicks on the formerly painted blank area did not dispatch; local child visibility remained false when its parent was shown again.
- Same-path symbol replacement resets transforms, visibility, custom state and nested clocks at three levels. The visible trace reported `V=true/true/true X=80/0/0 customClear=true F=1/1 newRefs=true stale=true loads=3/3/3 sibling=false/31/8`; subsequent frames advanced while the sibling retained its hidden state, transform and field. Restart reproduced the result in the in-app browser.
- The imported blank-gap source stopped at frame 4/5 and displayed `fresh placements undefined undefined 1 1`, including after Restart in both browsers. The in-app console transport renders raw undefined arguments as `[object]`; assertions use the actual authored visible trace, which correctly says undefined.
- With InvisibleLayer=1, hidden root and nested artwork appeared as red and blue squares; with 0 both were absent and their visible backdrops remained. Runtime errors were absent.
- AS2 parseInt returned numeric 511 for `0777`; AS3 returned 777. AS3 toFixed accepted precision 20 and produced catchable RangeError1002 for 21, including NaN. Shadowed/local/custom functions retained their behavior. Reference expectations are **documented_only**, pinned to the implementations listed in the intrinsic test record; no original Flash runtime was executed.

A further Chrome run accepted all twelve lines of the final numeric fixture, including ordinary custom data fields and the explicit native-method extraction diagnostic; Restart repeated them exactly in Chrome and the in-app browser. The new visibility-coercion fixture exercised zero strings, invalid/null/undefined/NaN, numbers/booleans, signed decimals and modern Boolean conversion. Unsupported object and ambiguous string inputs showed contextual error overlays; AS2 finally ran without swallowing the host failure. These additional coercion cases were checked in Chrome, not separately in the in-app browser.

The automated suite supplements these observations with actual WASM/host activation, inherited visibility, reset, replacement lifetimes and strict importer diagnostics. Import refusal cases include ambiguous adjacent placement continuity, unsupported excluded-layer scripts/labels/duration, nested publish-profile impostors, foreign namespace/attribute shadowing, and nonempty media/fonts wrappers. A refused source does not produce a partial HTML5 project; the matching accepted source is exercised above.

Browser console errors were zero in the accepted fixtures. Named Arial references produce the expected nonembedded-font warning. In-app engine identity was not established; it is not counted as another independently named engine. These new P0 cases have not been accepted in Safari or Firefox. Screenshots, DOM traces, exact comparisons, source/runtime/export SHA-256 records and logs remain under ignored `target/verification/p0-browser/`.

At that P0 increment, rotation/public void results, class slots and owner-aware APIs remained open. The later integrated family section above records the new bounded rotation/void/class work; full visibility/type/owner-aware contracts remain incomplete. The separate native review of executable d2614307… confirmed bounded arrows, readable themes, detached panels and synchronized scroll, and found missing Check Script feedback in detached Actions (D-008). Source-specific feedback has since been implemented, but that older native review does not verify the new Sources UI or complete native accessibility.

## Legacy corpus

Initial signature inspection recognized 42 local FLA/SWF files: 20 compound binary FLA; 22 SWF (20 FWS + 2 CWS, versions 3–8). The new strict CFB reader accepted 19 binary containers and rejected one for adjacent red directory nodes; this requires writer-compatibility investigation and does not establish that the original application cannot open it. Accepted save targets: Flash 3 × 6, Flash 5 × 10, Flash MX × 2 and Flash 8 × 1. These identify the saved format, not the last editing application.

The reader indexes 20 scene streams, 246 symbol streams and 23 media streams. It now consumes 107 timeline streams and all six Flash 3 Contents streams completely. The other 13 Contents streams stop at their unsupported suffix. All 289 descriptors resolve and raw settings are reached in the 19 accepted containers.

Typed preservation retains 1,343 object identities, 2,802 pointer edges, 1,064 owned children and 426 shape associations. The shape bodies include 526 fills, 212 strokes, 2,971 straight edges, 4,322 quadratic edges and 706 auxiliary curves. Complete descendants may belong to an incomplete ancestor, so these counts do not prove a complete editable graph. Remaining timeline stops include 155 unsupported class payloads, two shared/cyclic child references, one nonempty historical action list and one nonempty compiled list. Inspection emits only aggregate counts; exact geometry, descriptors, settings and typed fields are opt-in private preservation APIs. Every editable binary conversion remains blocked. See IMPORTS.md for exact boundaries. No private source/content/derived artwork was uploaded. Projector EXEs and the legacy HTML file were not executed.

Per-file reports, screenshots, logs and temporary test projects remain in ignored `target/verification/`. Original examples and explicitly licensed upstream fixtures are public; the user-provided corpus stays private.

## Artifacts

- Application: `dist/NeoFlash.app`, release build, ad-hoc signed; deployment minimum macOS 15.
- Browser example: `dist/demo/index.html`.
- Vector browser example: `dist/vector-demo/index.html`.
- Keyboard browser example: `dist/keyboard-demo/index.html`.
- Editable example: `examples/nested-orbit.nflash`.
- Editable vector example: `examples/vector-paths.nflash`.
- Editable keyboard example: `examples/keyboard-controls.nflash`.
- Build commands: `scripts/package-macos.sh`, `scripts/build-web.sh`.

No notarization, Intel verification, full ActionScript conformance, generic binary FLA/SWF conversion or production readiness is claimed.
