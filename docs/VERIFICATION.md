# Verification record

Date: 2026-10-26. This record covers the development preview and vector/binary-inspection increment, not complete Flash compatibility.

## Automated checks

`./scripts/check.sh` passes on Apple Silicon, macOS 15.8, Rust 1.99.1:

- Formatting and Clippy (`--workspace --all-targets -- -D warnings`).
- 86 Rust tests: 23 core, 27 editor, 16 script, 16 importer and 4 exporter.
- 13 browser-host/actual-WASM/cache tests in Node.
- 7 execution fixtures using JavaScript emitted by the script compiler.
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

## Legacy corpus

Initial signature inspection recognized 42 local FLA/SWF files: 20 compound binary FLA; 22 SWF (20 FWS + 2 CWS, versions 3–8). The new strict CFB reader accepted 19 binary containers and rejected one for adjacent red directory nodes; this requires writer-compatibility investigation and does not establish that the original application cannot open it. Accepted save targets: Flash 3 × 6, Flash 5 × 10, Flash MX × 2 and Flash 8 × 1. These identify the saved format, not the last editing application.

The reader indexed 20 scene streams, 246 symbol streams and 23 media streams. Seven timeline streams were consumed fully at the supported structural level. Contents, geometry and other unsupported payloads still block every editable binary conversion. See IMPORTS.md for exact boundaries. No private source/content/derived artwork was uploaded. Projector EXEs and the legacy HTML file were not executed.

Per-file reports, screenshots, logs and temporary test projects remain in ignored `target/verification/`. Only original small XFL fixtures and the NeoFlash example are public.

## Artifacts

- Application: `dist/NeoFlash.app`, release build, ad-hoc signed; deployment minimum macOS 15.
- Browser example: `dist/demo/index.html`.
- Vector browser example: `dist/vector-demo/index.html`.
- Editable example: `examples/nested-orbit.nflash`.
- Editable vector example: `examples/vector-paths.nflash`.
- Build commands: `scripts/package-macos.sh`, `scripts/build-web.sh`.

No notarization, Intel verification, full ActionScript conformance, generic binary FLA/SWF conversion or production readiness is claimed.
