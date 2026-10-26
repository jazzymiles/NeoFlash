# Verification record

Date: 2026-10-26. This record covers the development preview, vector and object tools, authored web semantics, ActionScript functions and binary geometry/Contents preservation, not complete Flash compatibility.

## Automated checks

`./scripts/check.sh` passes on Apple Silicon, macOS 15.8, Rust 1.99.1:

- Formatting and Clippy (`--workspace --all-targets -- -D warnings`).
- 169 Rust tests: 30 core, 78 editor, 20 script, 37 importer and 4 exporter.
- 27 browser-host/actual-WASM/cache tests in Node, including semantic bounds, DOM identity, focus restoration, pan/zoom and lifetime-checked keyboard activation.
- 30 execution fixtures using JavaScript emitted by the script compiler, plus an integration check using the actual host proxy/dispatch code with a lifecycle stub.
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

Eleven Contents tests cover versioned prefixes, truncated input, descriptor/class/object identities, selected-scene references, missing/duplicate/wrong-role streams, Unicode and compact string boundaries, nested metadata and collection budgets. Raw settings retain fractional FPS and color words; unread suffix bytes remain exact. Passing these tests establishes preservation within the documented boundary, not complete document conversion.

## Observed HTML5 checks

A real Chrome tab loaded the generated export through a temporary server bound only to `127.0.0.1`, serving only `dist/demo`:

- Animation advanced through frames.
- Pause held the current frame.
- Clicking the orbit ran its object handler, rotated the instance and emitted `Orbit rotated`.
- Restart resumed playback after a pause.
- No application errors appeared in the console; the expected font-fallback warning was present.

The browser tool rejects direct `file://` navigation, so direct-file behavior is not claimed as visually verified. The exporter embeds WASM bytes and all local scripts to support an autonomous folder; this packaging is covered by tests. Safari and clean-machine typography still need explicit verification.

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

## Legacy corpus

Initial signature inspection recognized 42 local FLA/SWF files: 20 compound binary FLA; 22 SWF (20 FWS + 2 CWS, versions 3–8). The new strict CFB reader accepted 19 binary containers and rejected one for adjacent red directory nodes; this requires writer-compatibility investigation and does not establish that the original application cannot open it. Accepted save targets: Flash 3 × 6, Flash 5 × 10, Flash MX × 2 and Flash 8 × 1. These identify the saved format, not the last editing application.

The reader indexed 20 scene streams, 246 symbol streams and 23 media streams. The later geometry-preservation increment increased fully consumed timeline streams from seven to 75 and retained 394 complete shape bodies, including geometry inherited by enclosing records that may later stop. These contain 501 fills, 208 strokes, 2,916 straight edges, 4,180 quadratic edges and 706 auxiliary curves. The Contents increment resolves 289 descriptors and reaches raw document settings in all 19 accepted containers. Every file stops explicitly at the unread Contents suffix; unsupported scene/object/media/action payloads also remain. Inspection emits only aggregate counts; exact geometry, descriptor bytes and settings are available only through opt-in preservation APIs. Every editable binary conversion remains blocked. See IMPORTS.md for exact boundaries. No private source/content/derived artwork was uploaded. Projector EXEs and the legacy HTML file were not executed.

Per-file reports, screenshots, logs and temporary test projects remain in ignored `target/verification/`. Only original synthetic fixtures and NeoFlash examples are public.

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
