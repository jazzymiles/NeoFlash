# Backlog

Independent `BITMAP-HTML5-20261027T0345Z` acceptance on `1bd0e3c` confirms bounded PNG/shared pixel edits, independent clone/copy/scroll, AS2 attachment, disposal and Restart. Only 24 positively observed declarations receive partial evidence; diagnostic-only AS3 BitmapData construction remains needs_review. JPEG, rasterization options and native image authoring are not inferred. See [the independent record](VERIFICATION.md#independent-bitmap-verification-of-1bd0e3c). Dynamic TextField/TextFormat is already in progress and needs its own batch evidence.

Independent c39a29e acceptance (`RESOURCES-HTML5-20261027T0323Z`) confirms preserved embedded resources, visible AS2/AS3 data loading, same-origin GET/POST and suppression of two held responses after close/Restart. Exact members remain partial. Cross-origin CORS, direct-file opening and native resource import/Undo are not inferred. See [the independent record](VERIFICATION.md#independent-resource-and-loading-verification-of-c39a29e). Bitmap was subsequently checked in the separate 1bd0e3c record.

Independent 798e28b acceptance (`LINKED-INTERFACES-HTML5-20261027T0243Z`) confirms bounded linked MovieClip/valid AS2 attachMovie and AS3 interface/accessor workflows, real input and Restart. The deliberate missing-link AS2 probe reproduces the documented host-error catch boundary and stops its frame; this remains a partial-contract follow-up. Only 25 exact declarations receive this evidence. See [the independent record](VERIFICATION.md#independent-linked-movieclip-and-interface-verification-of-798e28b). Current resource/loader work inherits no pass.

Independent 2b5deff verification accepts the bounded dynamic display/Graphics and ByteArray workflows, adjacent diagnostics and Restart. Exact members retain partial scope; linked timelines and external binary interoperability remain follow-up work. See [the separate record](VERIFICATION.md#independent-dynamic-display-and-binary-verification-of-2b5deff).

Independent ed3c39b verification accepts the bounded geometry/timer workflow and pending-callback cleanup on publication Restart; other members keep their individual status. See [the separate verification](VERIFICATION.md#independent-geometrytimer-verification-of-ed3c39b).

Independent a05a08d collections/plain-text acceptance is recorded separately in [VERIFICATION.md](VERIFICATION.md#independent-collectionstext-verification-of-a05a08d). Its Stage pointer failure belongs to that frozen build. Independent a3fedc3 verification now closes FAMILY-INPUT-001 for the observed mouse route and accepts bounded AS2 class/input behavior; see [the separate record](VERIFICATION.md#independent-as2input-verification-of-a3fedc3).

Priority is based on the stated product goal, with explicit evidence required for completion.

Current execution policy (user update, 27 October): breadth first, useful partial support across whole subsystems. Work and test in coherent batches, not one method at a time. P0 labels below identify risks, not a mandate to finish every edge case before the next subsystem. Prefer one representative integration/browser acceptance and a shared regression pass per delivery; defer non-blocking cases explicitly.

## Shared pointer correction delivered; aggregate observer follow-up open

On frozen `01f1381`, `FAMILY-INPUT-001` drops trusted mouse `click` events with `isPrimary=false` before EventDispatcher delivery. Reproduced in the original linked-class and Stage examples; Enter, keyboard and authored Click remain positive controls. The shared route is now corrected, and actual coordinate clicks/Restart passed in the original AS3 class and Stage examples alongside the AS2 input batch. Other engine results are not inferred. FAMILY-HARNESS-001 still records one unlocated aggregate MutationObserver exception; continue family implementation while its source is investigated. See [VERIFICATION.md](VERIFICATION.md#independent-family-verification-of-01f1381).

## Delivered family batch; native acceptance pending

- Project program: source files, AS3 packages/imports/classes, reusable fields/methods, document and symbol linkage, Sources editing, save/reopen and clipboard preservation.
- MovieClip / DisplayObject: timeline controls, bounds/dimensions, coordinate conversion, child traversal, identity, callbacks and class construction.
- Stage / EventDispatcher: stage geometry and input, listener management, propagation, event objects, keyboard/pointer/resize delivery.
- Integration: one shared regression run and a combined ordinary export/WASM scenario passed. The aggregate in-app browser batch loaded all eight exports; representative class, geometry, input and restart interactions passed. A shared Restart/static-state defect was fixed and rechecked in the existing scenario. Native Sources editing and detached Actions feedback remain pending; further edge cases do not hold up the next family.

Array/String constructors, mutation, searches, Flash sorting and data-processing callbacks now share a bounded profile across handlers and project classes. Plain authored TextField mutation/color/line queries and Canvas measurements are also implemented; the combined data-to-text example passed browser observation and Restart. Reused scenarios keep verification cost controlled. AS2 project classes and legacy Key/Mouse/Stage input now pass the combined host scenario and representative browser interactions. Point/Rectangle/Matrix and Timer/TimerEvent/getTimer now pass one combined exported workflow and Restart. The dynamic Sprite/Shape/empty MovieClip graph, solid Graphics and bounded ByteArray now pass the combined exported workflow. Explicit linked MovieClips/AS2 attachMovie and project AS3 accessors/interfaces now have shared compiler/core/export scenarios; representative browser interactions/Restart passed and native observations remain pending in VERIFICATION.md. Follow-up batches cover richer TextField/fonts and bitmap/resource documents, media, authoring operations and responsive DOM components; runtime identity remains independent of parentage. Catalogue entries only change status when their bounded behavior exists; a family module alone does not mark all of its declarations implemented.

## Portable resources delivered; next text and visual families

Portable project resources, AS3 URLRequest/URLLoader/URLVariables and AS2 LoadVars now share one binary-to-text workflow, including actual HTTP POST, cancellation, errors and Restart. Resource storage, export packaging and clipboard preservation are implemented; browser/native observations are separately recorded. The integrated bitmap family builds on this storage and the shared scene graph: PNG/JPEG placement, shared BitmapData pixel edits, independent clones/copies/scrolling and AS2 attachBitmap. Its own evidence is recorded separately; general Loader, bitmap fills, filters, draw/encode and additional image formats remain follow-ups. Dynamic TextField/TextFormat and authored TextBox now provide explicit layout boxes, shared Rust wrapping/alignment, uniform format and independent defaults, with their own verification record. Rich character runs, HTML/input/IME, auto-size/scrolling, fuller text metrics and font embedding remain explicit follow-ups.

## P0 — HTML5 correctness

- Known built-in properties must implement their contract or diagnose unsupported behavior: visibility must agree across pixels, scene, hit testing and semantic targets; text, dimensions and callback assignments must not become silent custom fields. Preserve genuine user fields and callbacks.
- Preserve XFL Include Hidden Layers policy for root/nested timelines and distinguish placement lifetime from XML slot order. Check reordering, replacement, blank gaps, load events, clocks and overrides; reject ambiguous transfers.
- Separate AS2/AS3 value profiles: parseInt radix behavior, Number.toFixed limits, typed slot writes/defaults through every access path, rotation normalization and public void return values.
- For each feature batch, retain representative source projects, ordinary HTML5 publish and aggregate browser results with build/runtime hashes. Mark documented expectations separately from original Flash measurements; record tested browser engines explicitly. Do not repeat a separate full acceptance cycle for every method.

Delivered bounded portions, with actual Chrome/in-app browser checks in VERIFICATION.md:

- [x] Boolean MovieClip visibility, inherited hiding, hidden clocks/Load, semantic target removal/restoration and fresh subtree state on same-path symbol replacement.
- [x] Explicit XFL hidden-layer publish policy, fresh IDs after blank gaps and diagnostic rejection of ambiguous adjacent continuity. Unsupported excluded-layer scripts/labels/duration are refused, not discarded.
- [x] Separate documented AS2/AS3 parseInt profiles and AS3 primitive toFixed precision 0–20/range behavior. Full primitive conversion and historical formatting are still open.

Still open: broader owner-aware unsupported API diagnostics (including TextField mutation and callbacks outside the supported subset), typed handler slots/defaults across aliases, complete visibility conversion, construction/lifecycle conformance and rotated dimension setters. The delivered family includes linked AS3 typed fields, rotation normalization, public void timeline results and unrotated dimension setters; these are bounded partial support.

## Completed foundation

- [x] Complete recorded desktop and browser smoke checks; fix discovered defects.
- [x] Publish the tested initial source tree to the authorized public GitHub repository.
- [x] Inspect the user's local legacy corpus and record aggregate format/compatibility results.

## P1 — essential authoring quality

- [x] D-003–D-007 implementation: bounded tween arrows, original vector icons, Light/Dark palette, visibility/F9/reset, native Tools/Timeline/Actions windows, synchronized timeline rows and compact-panel scrolling. Automated layout/state/input tests pass; native acceptance is recorded separately in VERIFICATION.md.
- Drag-to-dock/tab/split layouts, named workspace presets and broader VoiceOver/IME/multi-monitor acceptance.
- After HTML5 P0: contextual Document/Tool/Shape/Instance/Frame/Tween Properties, frame-state distinctions beyond color, restored breadcrumb context, library type/preview and explicit pivots. These are workflow refinements from the classic Flash reference review, not claimed completed features.
- D-008: display Check Script success/error directly inside docked and detached Actions, with source position and accessible text; clear stale results on target/source changes. Independent native QA reproduced missing feedback in detached AS3 frame editing; verify both themes and frame/object targets after fixing.

- [x] Cross-layer multi-selection, Shift/marquee/Select All, whole-selection duplicate/delete and V/Q object editing, with automated native-input regression coverage. Live native visual checks of this increment remain pending.
- Custom registration points/pivots, skew, nonuniform multi-object transforms and groups.
- [x] Object Copy/Cut/Paste and Paste in Place across frames/documents, including shared symbol closure, instance identity, scripts, text-focus routing and one-step Undo. Core/egui tests and observed macOS copy/cut/paste, cross-document transfer, text focus, save/reopen and Test Movie checks pass.
- [x] Preserve paste shortcut modifiers at key press in a pinned egui-winit adapter, with release-order, repeated-batch and text-recipient regressions. Physical macOS keyboard/menu delivery was observed after unlocking; an initial foreign-content event could not be reproduced after fresh copies.
- Timeline clipboard for selected frame ranges and layer-preserving paste modes.
- Drag/copy/reverse frame spans, keyframe movement, multi-frame edit and layer folders/reordering.
- [x] Bezier paths, pen/pencil, direct point editing, compound fills and HTML5 geometry.
- Brush/pressure, spline fitting, point insertion/deletion, tangent constraints, shape splitting/merging, gradients, masks and guides.
- [x] Tessellate native rectangle/ellipse fills and strokes before applying their full affine transforms, matching the Canvas geometry convention. Tests cover scaled, reflected and nested transforms; live native visual comparison remains pending.
- Autosave/recovery, file-change detection, unsaved project protection and multiple documents.
- Display and edit custom easing curves visually, including overshoot; property-level motion curves.
- Timeline virtualization and resource-aware undo for large documents.
- System fonts: weight/style/variable axes, shaping/fallback and refresh after font activation.
- Font embedding/subsetting and outline export with explicit font policies.

## P1 — universal HTML5 and scripting

- DOM layer with semantic text, inputs, keyboard focus and responsive layout.
- [x] D-001 HTML5 criteria: authored Button/Image metadata, independent semantic targets, Tab focus and Enter/Space activation exactly once, verified in Chrome and independently in the in-app browser. Live native-panel authoring and VoiceOver remain pending; this does not close general web accessibility.
- [x] D-002 preview criteria: Fit/100%/zoom/pan at 390×844, authored geometry preserved and playback controls reachable; verified with the keyboard example. Touch-device and Safari checks remain pending.
- JS/TS module imports, type checking, code completion, diagnostics navigation and debugger.
- Full AS2/AS3 parser/type/coercion conformance, classes/prototypes, E4X/namespaces and API mappings.
- [x] Function declarations/expressions, lexical locals, recursion and closures in the documented ActionScript subset; executable coercion/evaluation-order regressions.
- [x] Structured switch/do control flow, AS3 labels, explicit throws, selected typed catches and finally in the documented subset, with safe HTML5 failure reporting and a compiler/host/WASM recovery example. Full language/error conformance and AS2 ambiguous catch-scope forms remain pending.
- Sound/video/image import; synchronization, browser input and lifecycle.
- Runtime event conformance for nested clips, construction, re-entry, bubbling, scope and removed objects.

## P1 — legacy

- [x] Licensed public FLA corpus with pinned hashes/provenance, specific refusal expectations, exact original XFL/FLA imports and real-WASM example checks.

- [x] One real, unchanged CS6 FLA imports and publishes with the original 17-line trace, stable stopped root and repeatable Restart in actual HTML5; bounded convex solid geometry, AS2 callback/label support and strict XML guards have regressions.
- Extend XFL geometry, library assets, masks, tweens, color transforms and text formatting with fixtures.
- [x] Binary FLA CFB validation, stream inventory, save-target detection and bounded MFC structure inspection against the private corpus.
- [x] Preserve bounded binary shape/style/edge records with exact source bytes and aggregate-only inspection; verify the private corpus without publishing artwork.
- [x] Preserve bounded Contents prefixes, scene/library/media descriptors, MFC identities and raw settings; resolve all 289 descriptors in the 19 accepted local containers while retaining unread suffix bytes.
- [x] Preserve typed timeline identity/ownership/partial fields and shape associations from one container snapshot; read the supported Flash 3 Contents suffix to exact EOF. Synthetic fixtures and aggregate corpus verification establish the documented reader boundary.
- [x] Pure, bounded Flash 3 solid contour adapter with original fixtures, exact endpoint assembly, affine path coordinates and explicit topology/paint diagnostics. This does not enable binary document import.
- Binary FLA document conversion: document-wide semantic acceptance, Contents extensions/suffix, interpreted linkage, more paints/contours, object/media payloads and historical actions. Investigate one corpus container rejected by strict directory validation. Lossless FLA round-trip also requires retained document provenance and metadata.
- Evaluate Ruffle as optional SWF playback/compatibility module. Keep new HTML5 publication independent of mandatory SWF.
- Reproducible private-corpus tests: per-file diagnostics, rendered frames, input scenarios, comparison provenance.

## P2 — product finish

- Native macOS menus/document integration, shortcuts, accessibility tree, IME, trackpad conventions.
- Localization, advanced workspace layouts, performance profiling and extension API.
- Signed/notarized releases, Intel build verification, browser support matrix and updater policy.
