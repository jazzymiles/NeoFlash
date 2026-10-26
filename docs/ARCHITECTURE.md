# Architecture

## Data and ownership

`neoflash-core` owns a versioned `Project`: stage settings, library symbols, layers, sparse keyframes, element definitions, script source, and symbol references. IDs persist across keyframes and serialization. Import reserves loaded IDs to prevent collisions with newly created objects. Indexes in storage are zero-based; UI and script frame numbers are one-based.

The document has no GUI objects, GPU handles or machine-specific resource paths. `.nflash` is readable JSON in this initial version; future external assets will use project-relative references and migrations. Unsupported document versions and invalid geometry/references are rejected, not guessed.

Vector paths use explicit line/quadratic/cubic commands and compound contours. The core owns validation, analytic bounds, bounded flattening and hit testing. Native rendering tessellates with lyon; the Canvas host constructs Path2D. Both apply nested affine transforms to fills and strokes. See [the vector decision](decisions/0002-vector-geometry.md) for gesture, interpolation and rendering limits.

## Authoring versus execution

`evaluate(project, symbol, frame)` is pure. It samples authored transforms/easing, combines nested affine transforms and yields a leaf display list. Scrubbing and onion skin use it without running scripts. The editor currently paints this list through egui, including transformed text meshes.

`Player` owns runtime state keyed by instance paths: frame, playback flag, placement lifetime and property overrides. Independent MovieClips keep advancing when their parent stops. Graphic playback follows its parent. A queue carries frame/load/enterFrame/click script dispatches; stale dispatches are discarded when scripts replace/detach instances. Self-goto queues are bounded by the host. This is a defined initial lifecycle, not a claim that all Flash Player edge cases match.

## Script compilation and execution

`neoflash-script` parses JS and transforms TS with Oxc, in a strict function-body context matching the browser host. Type checking and package/module bundling are separate future work. The AS2/AS3 frontend has a real tokenizer/parser/AST and explicit unsupported-feature diagnostics. There is no string-substitution claim of full ActionScript translation.

`neoflash-web` compiles the same Rust `Player` to WebAssembly. The browser host provides Canvas painting, pointer input, script object proxies, helper APIs and a browser error surface. Only Test Movie/published output executes project code at present. A future native runtime must share the same event contract rather than invent a different interpreter path.

## Publishing

The Rust exporter validates all data and compiles scripts before publishing. It writes HTML, the browser host, WASM bindings and an external data/script bundle containing the WASM bytes. No SWF, remote CDN or runtime build service is required. It checks destination ownership before replacing a previous export.

Canvas is the first renderer. DOM-based accessible text, forms, responsive containers and semantic web documents are planned alongside Canvas, not represented as completed by the initial exporter. Font files are not embedded yet; explicit warnings describe browser fallback.

Optional placement accessibility metadata is evaluated by the same Rust runtime. A separate browser semantic layer represents named Button/Image targets and preserves their placement lifetimes; keyboard activation enters the existing Click event path. This is the first semantic layer, while rich DOM components remain planned. See [authored web semantics](decisions/0003-authored-web-semantics.md).

## Desktop

`eframe/egui 0.33.3` is pinned as the initial tested UI baseline with wgpu/Metal rendering. Custom panels implement a dense classic workspace. Native dialogs use rfd; installed fonts use font-kit/CoreText. A native `.app` wrapper sets macOS 15 as the deployment target. AppKit integration (menus, document lifecycle, accessibility and richer input) needs further work.

Commands currently group snapshot history around edits. Drag gestures are grouped; history is bounded. Larger documents will require a measured transition to delta commands and resource-aware history. Autosave/recovery and multi-document editing are not implemented yet.

Selection is editor state scoped to a symbol and stable layer/element IDs. Gestures capture the authored frame, view transform and original sampled poses, then apply changes from that snapshot. Keys and history begin only with an effective edit; cancellation and a return to the original pose leave neither behind. V selects/moves and Q exposes object transform handles. Multiple selections support rotation and uniform scaling; the TRS document model does not silently approximate skew. See [selection and gestures](decisions/0004-selection-and-gestures.md).

## Imports and compatibility

The importer is independent of native GUI and publishing. It inspects container signatures and supports only a documented XFL subset. Unsupported content blocks import, with a source location, rather than disappearing. Binary FLA, SWF bytecode/runtime and restoration of an editable source project are different compatibility tracks. See IMPORTS.md.

## Verification boundaries

Tests cover core state, persistence, script compilation, import fixtures, publisher errors, and actual compiled WASM behavior in Node. Native UI and browser rendering require separate observed checks. Desktop and browser text use different shaping backends at this stage; identical transforms do not establish pixel-perfect typography.
