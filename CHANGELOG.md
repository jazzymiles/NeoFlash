# Changelog

## Unreleased

- Added shared line/quadratic/cubic vector paths, compound contours, fill rules, geometry validation, analytic bounds and bounded hit testing.
- Added desktop pen, pencil and point editing with grouped undo, plus native fill/stroke tessellation and HTML5 Path2D rendering.
- Added interpolation for paths with matching command topology and a shareable vector example. Arbitrary Flash shape tweening remains unsupported.
- Added strict binary FLA container/stream inspection, save-target revision detection and bounded MFC timeline structure reading. All binary FLA editable conversions remain explicitly blocked until their contents can be preserved.

## 0.1.0 — development preview, 2026-10-26

- Started the open-source Rust workspace for macOS 15+ with a classic Flash-inspired editor.
- Added versioned documents, library symbols, instance transforms, nested timelines, keyframes, easing and deterministic authored evaluation.
- Added undo/redo, JSON save/open, basic shapes/text and installed-font discovery.
- Added Actions, JS validation, TS transformation and an explicitly bounded AS2/AS3 parser.
- Added shared Rust/WASM playback and standalone HTML5 publication with script diagnostics.
- Added initial legacy format inspection and a strict documented XFL import subset.
- Added unit/integration tests, build/check/package scripts, CI configuration and product documentation.

This preview does not provide full Flash UI/tool parity, complete AS2/AS3, general FLA/SWF import, font embedding, DOM layout or production packaging.
