# ADR 0001 — shared document and runtime, classic native editor

Date: 2026-10-26. Status: accepted for the development foundation, subject to measured UX review.

## Context

The product must preserve classic Flash's authoring model while publishing new work to HTML5. Supporting operators alone does not reproduce Flash's object lifecycle and runtime semantics. Legacy SWF execution does not itself provide an editable project or a modern web document.

## Decision

Use a Rust workspace with independent core, script, import, desktop, WebAssembly and export crates. Share timeline/runtime implementation between native and web. Separate side-effect-free authoring evaluation from execution. Use a tested egui/eframe baseline for the first custom classic workspace, with CoreText font discovery and macOS dialogs. Prefer permissive open-source components; project license is MIT OR Apache-2.0.

Keep generated files out of source control. Do not nest versioned project copies. Keep user-provided legacy files local unless publication rights and intent are separately established.

## Consequences

The first browser renderer is Canvas and is not yet sufficient for semantic responsive websites. Installed fonts are not automatically transferable to visitors. Full AS2/AS3 and binary FLA compatibility are separate large engineering tracks. A thin JS host is necessary for browser APIs and project JS/TS, despite Rust being the implementation language of the editor/core/compiler/exporter.

## Sources evaluated

- [egui goals and implementation](https://github.com/emilk/egui)
- [wgpu native/web backends](https://github.com/gfx-rs/wgpu)
- [Apple CoreText available-font enumeration](https://developer.apple.com/documentation/coretext/ctfontcollectioncreatefromavailablefonts(_:))
- [Ruffle compatibility](https://ruffle.rs/compatibility) and [source/license](https://github.com/ruffle-rs/ruffle)
- [ActionScript operators](https://help.adobe.com/en_US/FlashPlatform/reference/actionscript/3/operators.html)
- [Adobe HTML5 conversion limitations](https://helpx.adobe.com/au/animate/desktop/interactivity/creating-publishing-html5-canvas-document.html)
