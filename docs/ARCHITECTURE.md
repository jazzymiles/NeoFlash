# Architecture

## Data and ownership

`neoflash-core` owns a versioned `Project`: stage settings, library symbols, layers, sparse keyframes, element definitions, script source, and symbol references. IDs persist across keyframes and serialization. Import reserves loaded IDs to prevent collisions with newly created objects. Indexes in storage are zero-based; UI and script frame numbers are one-based.

The document has no GUI objects, GPU handles or machine-specific resource paths. `.nflash` is readable JSON. Version 2 adds logical source files and class linkage; version 3 adds immutable resource bytes with portable logical paths; version 4 layer roles and library folders; version 5 skew, color transforms, gradient paths and Flash easing; version 6 tween kinds, motion ◆ property keys, motion guides, Orient to path, scenes, frame sounds and ruler guides. The version is raised only when a document starts using a newer feature, so older readers refuse only documents that use one. Older document versions remain readable. Unsupported document versions and invalid geometry/references are rejected, not guessed.

Vector paths use explicit line/quadratic/cubic commands and compound contours. The core owns validation, analytic bounds, bounded flattening and hit testing. Native rendering tessellates with lyon; the Canvas host constructs Path2D. Both apply nested affine transforms to fills and strokes. See [the vector decision](decisions/0002-vector-geometry.md) for gesture, interpolation and rendering limits.

## Authoring versus execution

`evaluate(project, symbol, frame)` is pure. It samples authored transforms/easing, combines nested affine transforms and yields a leaf display list. Scrubbing and onion skin use it without running scripts. The editor currently paints this list through egui, including transformed text meshes.

## Timeline engine

A keyframe's `tween` and `tween_kind` choose how its span interpolates. Classic tweens interpolate the poses of elements with the same ID and follow a motion guide's first stroke when their layer is guided (`LayerProperties.motion_guide`). Shape tweens morph the layer's shapes (`shape_tween.rs`: pairing by ID then order, arc-length resampling of different contours, paint blending). Motion tweens interpolate each transform property through the ◆ property keyframes that key it (`Keyframe.keyed_properties`), with the span ease remapping time. All of this lives in `evaluate.rs`, so the editor, the runtime and HTML5 share one implementation. Range edits (`timeline_edit.rs`) keep motion keys consistent; the editor's `commit_history` records which properties an edit keyed and raises the format version.

Scenes are separate MovieClip symbols listed in `Project.scenes`. The editor edits each one; export and the WebAssembly player concatenate them into one main timeline (`scenes.rs`), as SWF publishing does, and keep scene ranges for `currentScene`, scene-aware `goto` and `nextScene`/`prevScene`.

Frame sounds (`Keyframe.sound`) are decided in Rust: the `Player` emits sound commands (play, stop, stream start/stop, stop all) when frames are entered, never during pure evaluation. Hosts only decode and schedule them: Web Audio in `web/audio.js`, and rodio with Symphonia decoders in the native editor preview. See [timeline sound](decisions/0037-timeline-sound.md) and [NF2 wiring plan §7](NF2-WIRING-PLAN.md#7-timeline-current-state-and-remaining-work).

`Player` owns runtime state keyed by instance paths: frame, playback flag, placement lifetime and property overrides. Independent MovieClips keep advancing when their parent stops. Graphic playback follows its parent. A queue carries frame/load/enterFrame/click script dispatches; stale dispatches are discarded when scripts replace/detach instances. Self-goto queues are bounded by the host. This is a defined initial lifecycle, not a claim that all Flash Player edge cases match.

## Script compilation and execution

`neoflash-script` parses JS and transforms TS with Oxc, in a strict function-body context matching the browser host. Type checking and package/module bundling are separate future work. The AS2/AS3 frontend has a real tokenizer/parser/AST and explicit unsupported-feature diagnostics. There is no string-substitution claim of full ActionScript translation.

Structured control flow uses validated enclosing jump targets, lexical catch environments and conservative completion analysis. Supported try/finally constructs lower directly to JavaScript, preserving return/break/continue exits. Explicit AS2 throws use a shared branded envelope across handlers; catch-all AS2 clauses do not absorb arbitrary host failures. The HTML5 boundary unwraps that envelope and formats arbitrary thrown values without replacing their cause. See [structured ActionScript and exceptions](decisions/0006-actionscript-control-flow.md) and the compatibility matrix for the exact language boundary.

`neoflash-web` compiles the same Rust `Player` to WebAssembly. The browser host provides Canvas painting, pointer input, script object proxies, helper APIs and a browser error surface. Only Test Movie/published output executes project code at present. A future native runtime must share the same event contract rather than invent a different interpreter path.

## Publishing

The Rust exporter validates all data and compiles scripts before publishing. It writes HTML, the browser host, WASM bindings and an external data/script bundle containing the WASM bytes. No SWF, remote CDN or runtime build service is required. It checks destination ownership before replacing a previous export.

Canvas is the first renderer. DOM-based accessible text, forms, responsive containers and semantic web documents are planned alongside Canvas, not represented as completed by the initial exporter. Font files are not embedded yet; explicit warnings describe browser fallback.

Optional placement accessibility metadata is evaluated by the same Rust runtime. A separate browser semantic layer represents named Button/Image targets and preserves their placement lifetimes; keyboard activation enters the existing Click event path. This is the first semantic layer, while rich DOM components remain planned. See [authored web semantics](decisions/0003-authored-web-semantics.md).

## Desktop

`eframe/egui 0.33.3` is pinned as the initial tested UI baseline with wgpu/Metal rendering. Custom panels implement a dense classic workspace on the NF2 design system (`crates/neoflash-ui`). Native dialogs use rfd; installed fonts use font-kit/CoreText. Editor playback of timeline sounds uses rodio (Core Audio output) with Symphonia MP3/WAV decoders (MPL-2.0, unmodified, editor-only; HTML5 exports do not contain them). Stage rulers, ruler guides (`Timeline.guides`, document data) and the grid are editor overlays in `stage_view.rs`; snapping math is in `snapping.rs` ([details](NF2-RULERS-GUIDES-GRID.md)). A native `.app` wrapper sets macOS 15 as the deployment target. AppKit integration (menus, document lifecycle, accessibility and richer input) needs further work.

A single pinned egui-winit source patch retains the initiating key event immediately before its paste payload. The editor consumes that adjacent event as paste intent before normal shortcut/text routing, so final frame modifiers and release order cannot change Paste in Place. Upstream provenance/licenses and the removable patch are recorded in `vendor/egui-winit/NEOFLASH-PATCH.md`.

`WorkspacePrefs` stores bounded versioned JSON through eframe storage, separately from Project/history. Theme, panel visibility and docking, sizes, detached-window geometries, timeline view options (short rows, snap playhead, loop playback, preview thumbnails) and the saved stage view defaults are persisted; arbitrary egui memory and script buffers are not. Semantic palette tokens drive widgets and manual painters. Tools/Timeline/Actions render the same content functions into docked panels or immediate native viewports, with stable content/text IDs and one mutable Editor. Deferred text clipboard events retain their viewport as well as their authored target. Native screen enumeration uses safe AppKit bindings and global logical coordinates; recovery only adjusts detached panels on creation or display topology changes. See [native workspace](decisions/0007-native-workspace.md).

A second pinned eframe patch initializes AccessKit for every child wgpu window, using the existing per-window event and output routing. Rendering an icon does not by itself establish native accessibility; both its tree and activation require separate checks. Provenance and feature-on/off checks live in `vendor/eframe/NEOFLASH-PATCH.md`.

Commands currently group snapshot history around edits. Drag gestures are grouped; history is bounded. Resource bytes use shared immutable backing across snapshots, and dirty-state comparison uses a saved model snapshot without reserializing data every frame. Larger scene graphs will still require a measured transition to delta commands. Autosave/recovery and multi-document editing are not implemented yet.

Selection is editor state scoped to a symbol and stable layer/element IDs. Gestures capture the authored frame, view transform and original sampled poses, then apply changes from that snapshot. Keys and history begin only with an effective edit; cancellation and a return to the original pose leave neither behind. V selects/moves and Q exposes object transform handles. Multiple selections support rotation and uniform scaling; the TRS document model does not silently approximate skew. See [selection and gestures](decisions/0004-selection-and-gestures.md).

The core clipboard model captures ordered sampled placements and their reachable symbol definitions in bounded, versioned text. It prepares a complete validated destination before the editor commits one history transaction. A live document-session identity permits reuse of current library definitions; another document receives consistently remapped symbol/layer/placement IDs. The editor routes text and object commands separately, preserving the text recipient across menu focus changes. See [clipboard transfer](decisions/0005-clipboard-transfer.md).

## Imports and compatibility

The importer is independent of native GUI and publishing. It inspects container signatures, converts a documented XFL subset and binary FLA saved by Flash 3 through CS4, and reports every approximation with a location; unsupported XFL content blocks import rather than disappearing. Binary FLA, SWF bytecode/runtime and restoration of an editable source project are different compatibility tracks. See [IMPORTS.md](IMPORTS.md) and [decision 0036](decisions/0036-legacy-fla-partial-import.md).

Opt-in binary preservation reads Contents, archive object/reference tables and typed timeline fields from one immutable container snapshot. Partial ancestors, source-action presence and original byte spans remain explicit. Archive identity and byte consumption do not establish cross-frame placement identity or an editable document; conversion remains a separate validated adapter.

The pure solid-contour adapter accepts a strict Flash 3 record/context subset. It assembles exact integer endpoints before applying the shape's full affine matrix to core path coordinates, retaining edge provenance separately. Bounded intersection/nesting checks reject ambiguous topology and paint; this API does not infer layers, timeline placement identities or document-wide import eligibility.

## Verification boundaries

Tests cover core state, persistence, script compilation, import fixtures, publisher errors, and actual compiled WASM behavior in Node. Native UI and browser rendering require separate observed checks. Desktop and browser text use different shaping backends at this stage; identical transforms do not establish pixel-perfect typography.

The publication data-loading service resolves embedded resources before normal browser HTTP(S), shares the EventDispatcher/ByteArray services and cancels requests on Restart/disposal. Clipboard preserves the complete resource snapshot because URL dependencies can be computed at runtime. See [portable resources and loading](decisions/0013-portable-resources-and-loading.md).
