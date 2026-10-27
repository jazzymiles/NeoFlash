# Changelog

## Unreleased

- Added bounded complete-file Sound playback: AS3 Sound/SoundChannel/SoundTransform, AS2 Sound load/start/stop/volume/pan and completion, portable-resource loading, browser gesture gating and visible audio controls. MP3/WAV metadata preflight reserves decoded PCM before native allocation; Restart retires prior sessions. One original MP3 publication passes ordinary export/real WASM with a silent injected scheduler; a separate silent in-app-browser pass verifies actual MP3 decoding/offline PCM, native channel lifecycle and ordinary AS2/AS3 Play/Stop/Restart. Streaming, ID3/sampleData/extract, full mixing parity and native timeline audio are not claimed.

- Independently verified the fba17ae dynamic text/layout delivery: real width-driven reflow, uniform current/default format ownership, content-preserving refusal, whole-text replacement and Restart. One five-project browser batch preserves bitmap, resource, linked-timeline and older authored-text behavior. Exact per-declaration coverage and limits are recorded separately.

- Independently verified bounded PNG/shared BitmapData workflows on 1bd0e3c: exact resource bytes and selected pixel oracle, shared versus independent images, AS2 attachment, disposal and Restart. One four-project browser batch preserved resource, linked-timeline and collection/text behavior; 24 exact declarations gain partial evidence, while positive AS3 BitmapData construction remains unverified.

- Added a bounded dynamic TextField/TextFormat family: AS3 construction, AS2 append-depth creation, independent logical boxes and transforms, multiline wrapping, whole-field current/default formats, font styles and left/center/right alignment. Rust owns shared line layout while frontends measure installed fonts. One original text-layout project passes ordinary export and real-WASM execution with independent AS3/AS2 cards, shared layout/draw observations, atomic mixed-run refusal and Restart. The in-app-browser workflow visibly passed reflow/alignment, independent fields and Restart; native interactive editing remains pending. Rich mixed runs, HTML/input/IME, embedded fonts and Flash font-engine parity remain outside this profile.

- Independently accepted bounded resource loading on c39a29e (RESOURCES-HTML5-20261027T0323Z): exact embedded bytes, real same-origin AS2/AS3 POST/GET and held-response suppression after close/Restart. Native and cross-origin observations remain separate.

- Added authored PNG/JPEG image placements and shared Rust BitmapData storage, with AS3 Bitmap views, pixel reads/writes, fill/copy/clone/scroll/disposal, smoothing/snapping and bounded AS2 attachBitmap. Browser rendering uses revision-stamped copies of Rust pixels. The original bitmap-pixels project passes one ordinary-export/real-WASM scenario with an embedded pixel-art PNG, shared and independent views, both dialects, Canvas uploads, disposal and Restart. The same workflow passed in-app-browser observation; native image-panel acceptance remains pending. General Loader, filters, draw, bitmap fills and complete Flash rasterization/coercion remain outside this profile.

- Independently verified linked MovieClip/valid AS2 attachMovie and AS3 interface/accessor workflows on 798e28b (LINKED-INTERFACES-HTML5-20261027T0243Z): separate state/timelines, detach/reattach, real input, clamp-to-nine and Restart. The documented AS2 host-error recovery boundary remains partial; 25 exact catalogue declarations retain bounded evidence.

- Added version-3 portable data resources with canonical paths/base64, bounded embedded bytes, history sharing and resource-aware clipboard preservation. Resource-bearing documents require the new version to prevent older readers silently discarding their bytes. Bitmap/font/media decoding remains separate work.
- Added bounded AS3 URLRequest/URLVariables/URLLoader and data-loading events, plus AS2 LoadVars with actual receiver callbacks. Embedded data loads asynchronously offline; external GET/POST uses ordinary browser HTTP(S)/CORS. Close, replacement, Restart and disposal cancel obsolete work. The original resource-loading example passed the combined ordinary-export/WASM scenario with text/form/binary data, actual local HTTP POST, cancellation and Restart. The embedded pipeline, same-origin HTTP GET and Restart also passed in the in-app browser. Native resource-panel and direct-file browser observation remain pending.

- Added public AS3 get/set accessors and project interfaces: typed property descriptors, inherited accessor halves and super access, exact public implementation signatures, multiple interface parents and nominal interface slots/casts. Read-only/write-only access fails explicitly; AS2, native overrides and full type-system conformance stay bounded. One original model/component publication passes ordinary export, real WASM/host, input and Restart; representative interactions and Restart also passed in the shared in-app-browser batch.
- Added explicitly exported MovieClip library timelines with independent namespaces, nested clocks/scripts and linked class construction. AS3 class allocation and bounded AS2 attachMovie use the same shared runtime; attachMovie accepts append-only depth and a bounded data-only initObject. Export linkage/base mismatches diagnose, and clipboard closure preserves runtime-linked exports. The combined linked-movieclips real-WASM host scenario passes; representative interactions and Restart also passed in the shared in-app-browser batch.

- Independently verified the 2b5deff dynamic display/Graphics and bounded ByteArray delivery in ordinary HTML5: stable reparent/detach state, real drawing/input, binary/Unicode output, adjacent diagnostics and Restart. Exact catalogue evidence remains partial; external binary interoperability and linked dynamic timelines are not claimed.

- Added runtime-created Sprite/Shape/MovieClip objects and project subclasses, typed owner-supplied Graphics, solid path drawing, dynamic child reparent/reorder/remove and captured lifecycle routes in the shared Rust/WASM scene. Authored children remain immutable; AS2 empty clips use append-only depth. Linked timelines are covered by the subsequent increment above; complete Flash lifecycle/paint semantics remain pending. The combined dynamic-display pointer/keyboard/lifecycle and Restart workflow passed in the in-app browser.

- Independently verified the bounded geometry/timer workflow on ed3c39b, including typed display results, live text and cancellation of pending work on publication Restart. Exact per-declaration limits and the remaining aggregate observer exception are recorded separately in VERIFICATION.md.

- Added bounded AS3 ByteArray/Endian imports, independent indexed binary buffers, numeric/UTF IO, copying and publication cleanup. Invalid sizes, overruns and invalid UTF preserve buffer state; unsupported AMF/compression/code pages and native subclass initialization fail explicitly. One original binary-data project passes ordinary export, real WASM/host, packet encoding/decoding, live text and Restart; the ordinary browser workflow also passed.

- Added shared finite Point/Rectangle/Matrix geometry, exact AS2/AS3 project imports and matching typed display-coordinate results. Added publication-owned AS3 Timer/TimerEvent, session getTimer, listener delivery, completion and Restart cleanup. The original geometry-timer project passed four transformed steps, live text and Restart in the ordinary-export/WASM scenario and in-app browser. Historical coercion, timing, 3D geometry and native inheritance conformance remain incomplete.

- Corrected shared Stage click delivery for mouse click events with isPrimary=false while retaining secondary-pointer filtering. Original AS3 class/Stage coordinate scenarios and Restart pass; an unlocated aggregate observer exception remains tracked separately.

- Added bounded AS2 project classes with matching qualified source paths, exact imports, same-dialect inheritance/super, erased annotations, scalar prototype defaults, shared unbound methods and static state. Linked authored instances use the shared program; AS2 root document linkage is a NeoFlash extension, and full AVM1 semantics remain incomplete.
- Added captured AS2 Key/Mouse/Stage singletons, object listener receivers/snapshots, keyboard state, cursor visibility, logical resize and reset/lifetime cleanup. One original two-instance example passes the combined ordinary-export/WASM/host/input scenario; coordinate clicks, keyboard and Restart passed in the in-app browser.

- Fixed Cargo lookup in HTML5 verification launchers for CI runners that provide the Rust toolchain on PATH, and retained actionable process-launch errors.

- Added bounded Array/String adapters shared by AS handlers and project methods: indexed mutation, Flash sort/sortOn flags, AS3 callbacks/search, UTF-16 text processing and AS3 literal-pattern replacement. Native method overrides keep their receiver/evaluation order; unsupported native extraction and richer coercion/prototype profiles diagnose.
- Added live plain authored TextField text/color overrides, append/replace and line queries, with UTF-16 ranges and partial Canvas metrics. The collection example now displays AS2/AS3 leaderboard results on the stage. Authored content remains unchanged; rich text, wrapping/input and portable fonts remain pending. The combined leaderboard/text example passed bounded browser observation and Restart. Independent a05a08d verification separately confirms the bounded data-to-text profile and Restart; see VERIFICATION.md.

- Added embedded project source units and version-2 document/symbol linkage, with a bounded AS3 public-class compiler: exact imports, inheritance/super, typed instance/static fields, initialized const fields and stable bound methods. Linked classes initialize per placement lifetime. The later AS2 profile is recorded above; JS/TS project modules remain unsupported.
- Added a coherent MovieClip/DisplayObject family: lifetime-aware hierarchy and child lookup, names, bounds and coordinate conversion, supported dimension setters, finite rotation readback and void timeline results. Added bounded AS2 onEnterFrame/onPress callbacks without claiming the complete Flash lifecycle.
- Added shared EventDispatcher/Event/MouseEvent/KeyboardEvent facades, capture/target/bubble, listener priority/mutation, cancellation and reset cleanup. Added Stage dimensions, frame rate, focus and bounded pointer/keyboard input; automatic Flash resize modes, fullscreen and IME remain pending.
- Added Sources editing within Actions, including source import/removal, linkage fields, project checks and feedback tied to the current source. Native verification of this workflow remains pending.
- Added a no-build batch export tool and one review dashboard, with independent case failures, artifact hashes and separate export/runtime/manual acceptance states. The eight original family/regression examples export and reach ready DOM state in the in-app browser; representative class/display/event interactions pass. Restart now recreates the program and static state. One shared automated run and a focused Restart regression cover the changes; native Sources acceptance remains pending.

- Added runtime `visible` / `_visible`: hidden subtrees retain clocks, scripts and instance state while rendering, pointer hits and semantic controls agree. Replacing a symbol at the same placement resets its subtree and preserves unrelated instances.
- Read XFL hidden-layer publication policy from the selected profile, keep artwork when included, and reject unsupported hidden scripts/labels or ambiguous adjacent placement continuity. Blank-gap re-entry receives fresh identities. Tightened XML wrapper/namespace auditing.
- Added separate bounded AS2/AS3 `parseInt` profiles and AS3 primitive `toFixed` precision/range handling, with explicit unsupported method-extraction boundaries. Ordinary published examples were exercised in Chrome and the in-app browser; original Flash runtime conformance is not claimed.

- Corrected AS2/AS3 coverage to use all 46,047 documented declarations and their per-entry audit instead of 35/36 coarse capability groups. The page now separates implemented subsets, partial implementation, missing entries and review gaps, preserves catalogue/evidence hashes, checks source freshness and excludes the obsolete AS family series.
- Switched the progress dashboard, daily history, README summary and detailed capability registry to English, preserving all statuses and counting rules.
- Added a licensed public FLA regression corpus: 15 unchanged external source files with pinned provenance, plus three original XFL/FLA projects with exact native imports and actual-WASM behavior checks.
- Imported one real Adobe CS6 FLA from the pinned Ruffle corpus and verified its original 17-line AS2 trace in published HTML5, including Restart. Added bounded convex straight-edge DOMShape conversion, source-script retention and audited metadata handling; 14 external cases still refuse unsupported content.
- Added bounded AS2 label resolution and dynamically assigned timeline callbacks, with explicit native goto-alias diagnostics and independent strict AS3/JS behavior. Preserved split XML text/CDATA fragments and rejected namespaced attributes and unchecked metadata subtrees.

- Added persistent Light/Dark workspace themes, panel visibility, F9 reveal and layout reset; Tools, Timeline and Actions can detach into native windows and dock again without copying the document. Window recovery accounts for connected macOS displays.
- Replaced missing-glyph controls with accessible vector icons, bounded tween arrowheads to their own rows, synchronized layer/frame vertical scrolling, and made compact panels scroll or wrap.
- Kept Actions cursor state through panel relocation and bound deferred text clipboard commands to their originating viewport.

- Added reviewed AS2/AS3 and product capability inventories, compact README/public coverage, detailed local evidence, and Git-backed daily progress charts published automatically to GitHub Pages. Partial support is counted separately from completed scope.

- Added structured ActionScript switch/do control flow, AS3 labels, synchronous exceptions and selected typed error support, with explicit per-language boundaries and executable cleanup/scope regressions.
- Fixed HTML5 failure reporting for arbitrary thrown values; added an editable AS2/AS3 recovery example and integration checks through the actual compiler, browser host and WASM Player.
- Preserved initiating paste-key modifiers in a pinned egui-winit adapter, removing the Shift/V release-order ambiguity while retaining one text paste event.
- Added an opt-in Flash 3 solid-contour adapter with exact endpoint assembly, affine path coordinates, bounded topology certification and provenance. General binary FLA document conversion remains blocked.

- Added object Copy/Cut/Paste and Paste in Place across frames/documents, preserving sampled placements, shared nested symbols, timeline scripts and accessibility metadata. Complete candidate validation precedes each paste; text-focused commands retain their recipient and pause playback.
- Added opt-in typed binary FLA timelines with archive identity, explicit child ownership, partial record fields and shape associations. Added bounded Flash 3 publish/compatibility suffix reading to exact EOF and proven empty historical-action envelopes. Editable binary conversion remains blocked.
- Added cross-layer multi-selection, Shift selection, marquee, Select All, duplicate/delete and grouped Undo; V remains the selection tool and Q provides Free Transform.
- Added rotated/reflected resize handles, center/proportional resizing, continuous rotation and uniform multi-object transforms. Gestures preserve sampled keyframe poses and avoid creating keys or Undo entries for unchanged results.
- Fixed final-release keyframe movement, fast pointer batches, selection/drawing lifecycle across frame/tool/view changes, Undo menu availability for pending Pen paths, and native rectangle/ellipse strokes under nested transforms.
- Added bounded binary FLA Contents descriptors, shared MFC identities and raw document settings, preserving unread bytes through an opt-in API. The private corpus reaches settings in all 19 accepted containers; editable binary conversion remains blocked.
- Added ActionScript function declarations and expressions, lexical locals, recursion, closures and selected AS3 argument/return coercions, with explicit diagnostics for unsupported function semantics.
- Added optional authored names, descriptions, Button/Image roles and focus order to placements, an Accessibility panel and lifetime-checked activation in the shared runtime.
- Added semantic HTML5 controls, persistent keyboard focus, visible script trace output, and Fit/100%/zoom/pan preview controls. The original keyboard example exercises independent AS3 instance counters.
- Added bounded preservation of binary FLA shape matrices, fills, strokes, edge deltas and auxiliary curves. Inspection still does not claim editable document conversion.
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
