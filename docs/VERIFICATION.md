# Verification record

Date: 2026-10-27. This record covers the development preview, vector/object tools and clipboard, authored web semantics, ActionScript functions/control flow/exceptions and binary geometry/Contents/timeline preservation plus a strict contour adapter, not complete Flash compatibility.

## Automated checks

`./scripts/check.sh` passes on Apple Silicon, macOS 15.8, Rust 1.99.1:

- Formatting and Clippy (`--workspace --all-targets -- -D warnings`).
- 279 workspace Rust tests: 46 core, 121 editor, 28 script, 80 importer and 4 exporter; four additional tests exercise the pinned egui-winit paste adapter.
- 41 browser-host/actual-WASM/cache and public-example tests in Node, including semantic bounds, DOM identity, focus restoration, pan/zoom, lifetime-checked activation, recovery and failure reporting.
- 62 execution fixtures using JavaScript emitted by the script compiler, plus an integration check using the actual host proxy/dispatch code with a lifecycle stub.
- 21 public-corpus CLI cases: seven successful imports and 14 explicit refusals, with source integrity and no-overwrite checks.
- Release WebAssembly build and standalone demo export.

Important regressions include independent nested clocks while a parent is stopped; affine nesting; dispatch order/stale-instance protection; JSON validation and graph cycles; grouped undo; key insertion at an interpolated pose; midpoint duplicate/delete; symbol conversion and recursive-placement rejection; key move/copy/collision behavior; fast and normal pointer gestures; macOS collection-font loading; and the Metal backend being enabled in the actual desktop build.

The build emits an upstream future-compatibility notice for the Metal dependency `block 0.1.6`. This is not a current compilation or Clippy error; dependency modernization remains tracked work.

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

This proves one real FLA workflow. General AS2/AS3 semantics, binary FLA conversion, broader geometry and the newly prioritized hidden-layer/placement/API correctness cases remain open.

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
