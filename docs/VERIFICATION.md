# Verification record

Date: 2026-10-26. This record covers the initial development preview, not complete Flash compatibility.

## Automated checks

`./scripts/check.sh` passes on Apple Silicon, macOS 15.8, Rust 1.99.1:

- Formatting and Clippy (`--workspace --all-targets -- -D warnings`).
- 58 Rust tests: 14 core, 16 editor, 16 script, 8 importer and 4 exporter.
- 8 browser-host/actual-WASM tests in Node.
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

## Observed HTML5 checks

A real Chrome tab loaded the generated export through a temporary server bound only to `127.0.0.1`, serving only `dist/demo`:

- Animation advanced through frames.
- Pause held the current frame.
- Clicking the orbit ran its object handler, rotated the instance and emitted `Orbit rotated`.
- Restart resumed playback after a pause.
- No application errors appeared in the console; the expected font-fallback warning was present.

The browser tool rejects direct `file://` navigation, so direct-file behavior is not claimed as visually verified. The exporter embeds WASM bytes and all local scripts to support an autonomous folder; this packaging is covered by tests. Safari and clean-machine typography still need explicit verification.

## Legacy corpus

Read-only inspection covered 42 local FLA/SWF files: 20 compound binary FLA; 22 SWF (20 FWS + 2 CWS, versions 3–8). All were recognized, with no parser errors, and all correctly report that editable conversion is not yet implemented. No private source/content/derived artwork was uploaded. Projector EXEs and the legacy HTML file were not executed.

Per-file reports, screenshots, logs and temporary test projects remain in ignored `target/verification/`. Only original small XFL fixtures and the NeoFlash example are public.

## Artifacts

- Application: `dist/NeoFlash.app`, release build, ad-hoc signed; deployment minimum macOS 15.
- Browser example: `dist/demo/index.html`.
- Editable example: `examples/nested-orbit.nflash`.
- Build commands: `scripts/package-macos.sh`, `scripts/build-web.sh`.

No notarization, Intel verification, full ActionScript conformance, generic binary FLA/SWF conversion or production readiness is claimed.
