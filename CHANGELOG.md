# Changelog

## 2026-10-28 — Drawing tools and symbol authoring

- Add ten tools, Modify commands and Align/Transform/Color panels, including exact spacing and group-aware layout.
- Use compact numeric fields and linked timeline FPS/duration; retain the frame count when timing changes.
- Add empty symbol creation, Button state editing and AS2/AS3 input/type linkage, plus Graphic first-frame playback.
- Move floating Edit/Dock controls into the header menu and distinguish tab dragging from whole-group dragging. Preserve skew during transform gestures.

## 2026-10-28 — Layers and configurable authoring workspace

- Keep graphic imports visible on the page, including Finder drops and placement while a hidden, locked or special layer is selected. Add Library-to-Stage dragging and a direct Import graphics control.
- Add per-layer outline switches with editable colors, preserved in the document and excluded from HTML5 appearance.
- Separate fill/stroke RGB and alpha editing, apply drawing defaults consistently and group property gestures into one Undo. Align panel forms and use a compact macOS UI font.

- Add vector mask layers shared by the native stage and HTML5, editor-only guide layers, layer folders and a full layer context menu with grouped editing operations.
- Add compact timeline controls, stronger grid/control contrast, smaller left-aligned layer names, row dividers, inline rename and frame zoom while retaining seconds, numeric onion ranges and opacity. Layer columns resize by dragging; layer reordering preserves folders and mask relationships.
- Give dock areas visible resize grips and align menu shortcuts in a separate right-hand column.
- Replace fixed panel locations with four-sided docks, resizable groups, tabs and native floating panels. Support transfer between windows and tab/split groups inside floating windows. Add a compact symbol dialog and a library with preview, folders, search, sorting and visible linkage.
- Import SVG/PNG/JPEG through the File menu or by dropping files onto the stage as one Undo step. Solid SVG paths stay editable; richer paint is rendered with the original SVG retained.

See the [compatibility matrix](docs/COMPATIBILITY.md) for current support and limitations, and the [declaration audit](docs/progress/ACTIONSCRIPT.md) for ActionScript coverage. Runtime features below have partial support unless explicitly stated otherwise.

## 2026-10-28 — Timeline editing

- Show held content, blank intervals and the timeline end; improve keyframe dragging with a destination preview, edge scrolling and a separate scrollbar.
- Open frame Actions from the selected timeline context, identify the symbol/frame and focus the code editor. Opening Actions pauses preview playback.
- Mark active onion skin in the stage header. Frame-range operations and independent layer lengths remain unfinished.

## 2026-10-28 — Runtime and browser services

- Add AS2 Stage/context menus, Camera/Microphone, LocalConnection, AMF0 HTTP connections and bitmap PrintJob profiles. Hardware capture and visual system print-preview acceptance remain unverified.
- Extend progressive NetStream buffering and completion, SOAP/XML services, transitions, relative depth and TextField.variable binding.
- Add AS2 data collections/binding, styled text and snapshots, bitmap operations, FileReference/FileReferenceList, Locale tables, dynamic Object accessors/watchpoints, Sound metadata/channel matrices and timeline variable loading.
- Add AS2 primitive boxes, project includes, concrete class accessors and with scopes. Keep AS2 `<>` token handling distinct from AS3 XMLList syntax.
- Add rendered filter profiles, event payload classes, DateTimeFormatter and NumberFormatter/NumberParseResult; move shared date arithmetic into Rust.

## Unreleased

### Authoring

- Add pen, pencil and point editing, compound vector paths, fill rules and interpolation between compatible paths.
- Add cross-layer selection, marquee, grouped duplicate/delete/Undo and Free Transform with proportional/center resizing and multi-object rotation.
- Add object Copy/Cut/Paste and Paste in Place across documents, preserving nested symbols, scripts, resources and accessibility metadata.
- Add Light/Dark themes, persistent panel visibility, detachable native panels and workspace recovery across macOS displays. Preserve Actions cursor state and text clipboard routing during panel relocation.
- Improve timeline scrolling, tween indicators, keyframe release handling and drawing/selection lifecycle. Keep native strokes consistent under nested transforms.
- Add project source editing and checks within Actions, linked classes, portable resource files and authored PNG/JPEG image placement.

### Scripting and data

- Add AS3 numeric Class aliases, int/uint bounds and bound numeric method values; ordinary object property/enumerability controls, public source-class reflection, source-class prototype/constructor identity and dense Array insertAt/removeAt. Share Array Class identity across handlers and project code and retain array receivers in captured trait method values. Extend the existing inventory example with visible calculations, metadata and collection processing.
- Add ActionScript functions, closures, structured control flow, exceptions, optional/rest arguments and Function.call/apply. Improve HTML5 diagnostics for arbitrary thrown values.
- Add AS2/AS3 project classes and imports, linked MovieClip construction, AS3 access control, final/dynamic classes, interfaces and inherited accessors. Preserve distinct AS2 class and receiver semantics.
- Extend Array/String, numeric/URI operations, Date, JSON, RegExp, finite geometry, easing/Tween/CustomEase, callback timers and AsBroadcaster. Individual upstream algorithms are adapted into Rust with attribution.
- Add AS3 Vector/Dictionary and enumeration, shared XML parsing, AS2 XML/XMLNode, AS3 XML/XMLList, namespace-aware E4X selectors/literals and bounded targeted edits.
- Add ByteArray numeric/text IO and codecs, AMF0/AMF3 values and local AS2/AS3 SharedObject persistence, including supported shared references and cycles. Correct malformed-AMF errors so generic Error handlers can recover.

### Display, media and HTML5

- Add runtime display objects, dynamic hierarchy changes, linked timelines, events and Stage input. Expose local MovieClip label/playback/loaded-frame metadata, typed FrameLabel values and a single unnamed Scene snapshot. Add MovieClip buttonMode label transitions and enabled while retaining ordinary mouse/key events, plus subtree timeline stopping and validated runtime-child range removal. Correct Stage mouse-click delivery while retaining secondary-pointer filtering.
- Add transformed dragging, hit tests, hitArea, input flags and legacy pointer callbacks. Hidden objects retain their clocks and state; symbol replacement creates fresh instance state.
- Add runtime Graphics gradients/paths, vector masks, scroll rectangles, exact 2D transforms and nested color transforms.
- Add shared BitmapData pixels, filters, channel operations, capture, bulk byte transfers and PNG output. Add AS3 Loader/LoaderInfo and AS2 MovieClipLoader with cancellation and unload behavior.
- Add dynamic TextField/TextFormat layout and scripted HTML5 text entry, selection/focus, input restrictions and AS2 Selection callbacks.
- Add portable resource and HTTP(S) loading, scripted Sound playback and file-video playback through Video/NetStream. Browser gesture, origin and transport rules apply.
- Add ExternalInterface page callbacks, browser navigation and form submission. Add authored accessibility metadata, semantic keyboard controls and preview zoom/pan.
- Reset publication-owned scripts, timers, input and asynchronous resources on Restart.

### Import, coverage and tooling

- Separate AS3 reporting into ordered Flash Player foundations, later media/services, AIR-only APIs, external libraries and unresolved availability. Show purpose and all four audit statuses without changing the complete catalogue denominator or treating existing partial APIs as new work.
- Add direct native Flash MX/CS4 FLA import for the supported `Clock.fla` and `300x600.fla` profiles. Preserve nested symbols/layers, classic tweens, masks, text, bitmap media and original ActionScript; embed the exact FLA and source provenance. Missing installed fonts are nonblocking.
- Extend document version 5 with authored affine skew, gradient paths, color transforms and Flash easing. Imported buttons retain separate Up/Over/Down/Hit frames; Hit geometry remains invisible during playback.
- Add a bounded AS1 timeline profile for Clock, retaining its language/source and resolving dynamic names with SWF6 case rules. Unsupported binary records and script features continue to diagnose.
- Expand XFL preservation, hidden-layer handling and AS2 frame-label/callback support. Import one licensed real CS6 FLA as an editable project; general FLA/SWF import remains incomplete.
- Add a pinned, licensed public FLA corpus with source provenance.
- Base ActionScript coverage on the complete declaration catalogue, with separate implemented-subset, partial, missing and needs-review states. Keep the generated dashboard, daily history and repository summary consistent and in English.
- Add grouped browser-export tooling and fix Rust toolchain lookup in HTML5 verification launchers.

## 0.1.0 — development preview, 2026-10-26

- Start the open-source Rust workspace for macOS 15+ with a classic Flash-inspired editor.
- Add versioned documents, library symbols, nested timelines, keyframes, easing and deterministic authored evaluation.
- Add undo/redo, JSON save/open, basic shapes/text and installed-font discovery.
- Add Actions, JavaScript validation, TypeScript transformation and an explicitly bounded AS2/AS3 parser.
- Add shared Rust/WASM playback, standalone HTML5 publication, initial legacy inspection and a documented XFL subset.
- Add build/check/package scripts, CI and product documentation.
