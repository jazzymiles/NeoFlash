# Changelog

## 2026-10-28 — AS2 runtime, browser services and language batch

- Add partial Stage/content-menu, Camera/Microphone, LocalConnection/AMF0 HTTP, bitmap PrintJob, primitive-box, project-include and automatic Locale profiles; extend progressive NetStream buffering/completion.
- Separate 102 new declarations (78 method/property paths), seven changes to existing behavior and 61 stale audit corrections. The bounded audit covers 170 unique AS2 IDs: partial entries rise 1,355 → 1,518 and missing entries fall 900 → 737. AS3, native editor UI, FLA and legacy components remain outside this batch.
- Keep 90 execution fixtures and 24 project workflows at their existing counts; add four grouped native tests and extend existing families. Compiler/WASM, synthetic capture, shared native checks and the unchanged 90/24 suites passed; five ordinary HTML5 families have bounded browser evidence. Hardware capture and visual print preview remain unverified. Observations and publication are tracked in [the sprint report](docs/AS2-RUNTIME-TWO-HOUR.md).
- Record the interrupted session and recovery separately from productive implementation time. This batch does not restart wider project work.

## 2026-10-28 — Second AS2 two-hour subsystem batch

- Add 149 catalogue method/property paths (52 methods, 97 properties) across progressive video, SOAP/XML services, display/text, transitions, relative depth and TextField.variable; add bounded AS2 concrete class accessors and with scopes.
- Count 258 new declarations, nine extensions and 43 corrections of existing support separately. AS2 partial entries rise 1,054 → 1,355 (+301); missing entries fall to 900. AS3 entries are unchanged.
- Record seven ordinary HTML5 families with real browser behavior. Keep the shared 90 execution fixtures and 24 project workflows unchanged, with one shared run; add only three grouped Rust test functions and extend existing family checks.
- Keep all wider work paused. See [the delivery report](docs/AS2-TWO-HOUR-2.md) for exact scopes, comparison, provenance and limitations.

## 2026-10-28 — AS2 two-hour subsystem batch

- Add 216 catalogue API paths: 142 methods, 71 properties and three global functions across data/binding, text/display, bitmap, file, Locale, Object, Sound and variable-loading families.
- Keep 278 new declarations, 12 extensions and 30 corrections of existing support separate. Two catalogue entries mislabeled as constants are reported as instance fields. The exact AS2 partial count rises 745 → 1,054 (+309); AS3 is unchanged.
- Keep the existing 27 compiler tests and 90 execution fixtures passing without count growth; add 12 grouped Rust tests. The shared workflow run passed 23/24, and the affected input family passed its focused rerun. Eight ordinary HTML5 families have observed browser results; remote publication/CI remain pending.
- Preserve Rust algorithm ownership, individual licensed adaptations and explicit partial limits. No editor UI, FLA or whole-project restart is included.
- See [exact counts, browser evidence, boundaries and comparison](docs/AS2-TWO-HOUR.md). Native file-save download and audible audio balance remain unverified.

## 2026-10-28 — bounded AS2 runtime batch

- Add 71 catalogue method/global/property paths, 13 source constructs, eight constants, one event and six static namespaces; keep AS3 declarations unchanged.
- Extend 13 existing declarations, move parseInt into Rust and correct 77 stale catalogue decisions. Exact AS2 partial count rises 563 → 745.
- Deliver five real HTML5 projects, including an actual XML HTTP POST, with three new grouped Rust tests and unchanged 90 execution fixtures / 24 project workflows.
- Fix AS2 `<>` token isolation from AS3 XMLList syntax and baseline all-target Clippy test lints.
- See [counts, limits, verification and efficiency](docs/AS2-RUNTIME-HOUR.md). The rest of the project remains paused.

## 2026-10-28 — second bounded AS runtime batch

- Add four rendered Rust filter families, 22 event payload classes, DateTimeFormatter and NumberFormatter/NumberParseResult; migrate Date arithmetic to Rust.
- Deliver 198 new partial member paths and 54 migrations, with four real HTML5 examples and controlled shared regression checks.
- Merge completed scripting experiments and refresh exact declaration audit entries with preserved evidence; keep UI/FLA and unfinished TextField work paused.
- See [the runtime-hour report](docs/AS-RUNTIME-HOUR-2.md) for counts, provenance and explicit boundaries.

## Unreleased

- Completed the bounded AS2/AS3 runtime hour: 192 new partial method/function/property paths and 117 existing entries moved to Rust across String/URI, geometry/Color, easing/Tween/CustomEase, callback timers, AsBroadcaster, ByteArray and Math. Five ordinary HTML5 examples passed grouped browser execution and Restart; binary text uses 38 codec profiles. [Counts, costs, reuse and limitations](docs/AS-RUNTIME-HOUR.md). Editor, FLA and the wider project remain paused.

- Completed a bounded Rust reuse experiment: 10 new AS2 globals, 43 AS3 geometry methods and nine properties; 14 existing AS2 String members moved to Rust. Individual licensed algorithms replace engine integration. Both ordinary HTML5 examples passed browser execution and Restart; see [the measured result](docs/AS-REUSE-EXPERIMENT.md). The wider project remains paused.

- Added a bounded core-language batch: AS2/AS3 comma/void expressions, sparse arrays and Object construction; AS3 primitive is/as, scalar constructors, RegExp literals, numeric formatting and URI codecs; AS2 int/random and Boolean omission behavior. Existing grouped scenarios cover the batch; full catalogue reassessment and browser acceptance remain separate.

- Independently checked frozen 01f2261 pointer/drag (`POINTER-HTML5-20261027T1113Z`): real donor-margin and rotated-AS2 dragging, child/disabled targeting, outside release, bounded negative/positive geometry controls and fresh Restart. The actual transforms, unmeasured cursor effects and unresolved first linked-target miss are retained explicitly; body and semantic controls pass within that scope.

- Added a coordinated web-container family: AS2/AS3 ExternalInterface, page callbacks with synchronous results, browser GET/form POST navigation and response-free sends. Ordinary authored JavaScript supplies the HTML controls; generation ownership and native browser policies remain explicit.

- Independently checked frozen f42ad94 file video (`VIDEO-HTML5-20261027T1042Z`): actual AS3/AS2 MP4 decoding, metadata, pause/seek/resume/completion, post-completion retained frames and AS3 clear, plus HTTP playback interrupted by Restart and fresh playback. Audio quality, exact timing, active-close races and native authoring remain outside this evidence.

- Added a coordinated pointer family: AS2/AS3 dragging in transformed containers, script hit tests, input targeting flags and hitArea identities, cursor preferences and legacy release/rollover callbacks. Rust owns input state; the browser owns pointer capture and drag lifetime.

- Added file-video playback through AS3 Video/NetStream/local NetConnection and the AS2 authored-Video adapter. Shared Rust display ownership now includes Video placements, with explicit native insertion/Properties placeholders; browser decoding, transport, audio gating and publication lifetime have their own HTML5 profile.

- Independently checked frozen 6687ced class/packages (`CLASSES-HTML5-20261027T1007Z`): isolated base/derived private state, protected/package updates, dynamic-only enumeration/public JSON, bounded sealed/private-route refusals, AS2 inherited-private updates and Restart. Positive final/import use does not imply every rejection, reflection or initialization rule.

- Independently checked frozen 6f4f66a image processing (`IMAGE-PIPELINE-HTML5-20261027T0943Z`): full initial source/tint and PNG round-trip pixel evidence, bulk transfer/cursors and the documented partial-write EOF path. Subsequent palette/Restart phases retain screenshots/AX only where logs were unavailable; no repeated full-byte or complete-run zero-error claim is made.

- Added a coordinated source-class/package family: AS3 lexical access, separate private slots, protected dispatch, package-internal collaboration, final/dynamic declarations and wildcard imports. AS2 retains its distinct private/inheritance behavior. One component workflow connects these rules to public serialization, dynamic metadata and Restart.

- Independently checked frozen 11a59df Functions (`FUNCTIONS-HTML5-20261027T0914Z`): corrected real commands, optional/rest/supplied arguments, bound versus ordinary receivers, arity refusals and Restart. An arguments.length instrumentation change altered the original probe contract; only that family was corrected/re-exported, with original evidence retained. No complete VM invocation or reflection claim follows.

- Added a shared BitmapData image pipeline: bulk ARGB/ByteArray transfer, colorTransform, applyFilter/generateFilterRect, PNG encoding and read-only source-subtree draw through the same scene painter. Rust retains pixel ownership, partial setPixels EOF follows consumed complete words, and one palette workflow connects PNG to Loader and Restart.

- Independently verified frozen 5395375 filters (`FILTERS-HTML5-20261027T0844Z`): visible whole-card styles, copied settings and bounded refusals, unchanged shared bitmap pixels, observed source hits/halo exclusion and clear/restore/Restart, with one common browser regression. Native/filtered editing, clip composition, HiDPI/cache bounds and exact Flash raster behavior remain unverified.

- Added a coordinated source-function family: AS3 scalar optional defaults and rest, supplied arguments, Function.call/apply and shared project signatures; AS2 retains separate argument and receiver behavior. One command workflow connects callbacks to visible scene state and Restart. Full VM global scope, reflection and generalized default expressions remain separate work.

- Independently checked c10e19e text forms (`FORMS-HTML5-20261027T0811Z`): real typing/paste, bounded restrict/maxChars, focus/AS2 Selection, cancelled input, saved reload/Restart and active-edit reset. The malformed-AMF catch(Error) regression now catches generic Error and continues; EOFError/full Flash fidelity is not claimed. The initial four-ready/one-preparation-failure batch and targeted corrected export remain separate.

- Added ordered display-object filters across AS2/AS3: Blur, Glow, DropShadow and ColorMatrix, copied filter values, shared Rust RGBA processing and nested subtree rendering with deferred owner clipping. One gallery combines styles, shared bitmap sources, legacy input, clear/restore and Restart.

- Added the scripted HTML5 text-input family: shared Rust editing/selection transactions, native browser fields, user-only restrict/maxChars, password masking, TextFieldType, TextEvent/FocusEvent and AS2 Selection/callbacks. One saved form connects the two dialects, focus, editing and publication lifetime.

- Independently checked eddcc30 SharedObject/AMF: real reload/Restart/path isolation/clear and supported graph values pass in a separate catch(*) control. The original five-case batch retains one runtime error: P1 AMF-TYPED-CATCH-001 records malformed readObject throwing a raw string that bypasses catch(Error). Full typed recovery, codec interoperability and native storage are not inferred.

- Independently verified the 039afe6 image-loading delivery: embedded PNG/HTTP JPEG, loadBytes snapshots, canonical LoaderInfo metadata/order, retained pixels through replacement/unload/re-adoption, real Loader/AS2 input and two pending HTTP responses suppressed after close→Restart and Restart, with one shared browser regression. Pending close alone, progress events, general reentry/GC and native authoring remain unverified.

- Independently verified the 05d5d43 clipping delivery: vector masks/even-odd holes, copied integral AS2/AS3 scroll rectangles, real positive/excluded pointer hits, linked-instance continuity and Restart, with adjacent self-mask/fractional-scroll diagnostics and one shared browser regression. Parent right-edge intersection, source retirement, clipping budgets and full accessibility remain unverified.

- Added bounded Rust AMF0/AMF3 values, ByteArray readObject/writeObject and objectEncoding, plus local AS3/AS2 SharedObject persistence. Shared references, cycles, dates, special numbers and nested bytes use real AMF; browser origin/path-scoped storage has explicit flush, clear and Restart behavior.

- Added bounded runtime image loading with AS3 Loader/LoaderInfo and AS2 MovieClipLoader, shared resource/HTTP transport, PNG/JPEG decoding, owned content, asynchronous events, cancellation and unload. Retained AS3 Bitmap references survive unload; AS2 loading replaces original MovieClip artwork. One gallery workflow covers the family.

- Added runtime vector mask assignment and copied scrollRect state, shared clip tables, clipped Canvas/pointer/semantic paths and AS2 setMask/undefined absence handling. One linked-scene workflow combines scrolling, compound mask unions, nested viewport intersections and Restart. Integer immediate scroll rectangles and vector sources are the first profile; native mask tools and bitmap-alpha masking remain pending.

- Independently verified the 08f1d9c Graphics delivery: representative linear/radial paints and gradient stroke, independent copied paint, Vector/cubic paths, real pointer/keyboard lifecycle and Restart, with adjacent reflect/incomplete-path diagnostics and one shared browser regression. Unequal-corner drawRoundRectComplex is a NeoFlash extension; exact rasterization, alpha edges, budgets and full coercion are not inferred.

- Independently verified the 5be13d9 bounded E4X delivery: selector/literal/targeted-update semantics, interpolation order, namespace/QName selection, safe ambiguous refusal and Restart, with 24 independently parsed structures. A corrected handler probe establishes a narrow XML/XMLList typed bridge only on this snapshot. One five-project browser batch preserves typed collections, Transform, text and earlier collections; full E4X and uncalled APIs are not inferred.

- Added runtime gradient Graphics across AS2/AS3: linear/radial fills and strokes, copied matrices/stops, shared cubic and rounded geometry, Vector drawPath and independent Graphics.copyFrom. The existing dynamic-display workflow combines palette edits, copied paint, hierarchy changes and Restart. Pad/RGB is the initial profile; bitmap fills, masks/filters and native gradient authoring remain separate work.

- Added an E4X source layer: child/attribute/descendant/qualified selectors, editable target lists, Namespace/QName values and XML/XMLList literals with ordered interpolation. A buffered lexer handles XML text inside handlers and project methods. Staged edits reject ambiguous targets without changing the tree; the existing catalog workflow now combines these features with the AS2 DOM regression. Filters, scoped default namespaces and full XML/value equality remain explicit follow-ups.

- Independently verified the f4fcb71 XML delivery: exact embedded catalog bytes, AS2/AS3 tree copy/mutation/serialization, Unicode and namespace preservation across fifteen independently parsed trees, visible updates, adjacent diagnostics and Restart. Direct failure-only/delegated APIs remain needs_review; E4X syntax and operators are not inferred. One five-project browser batch preserves typed collections, Transform, text and earlier collections.

- Added shared bounded Rust XML parsing, AS2 XML/XMLNode DOM and AS3 XML/XMLList data facades: portable loading, traversal, copied/moved trees, serialization and publication lifetime. XML settings and typed values cross handler/project boundaries. Namespaces survive roundtrip; unsupported E4X and declarations diagnose. AS3 int()/uint() conversions support common data-processing source with existing numeric coercions.

- Independently verified the 6396fd0 typed collection delivery: class Vector/Dictionary models drive live inventory cards and JSON with preserved identity/listeners through reuse, bounded fixed/bounds/type refusals, strong key controls and Restart. One five-project browser batch preserves Transform, text, data and collection behavior. Weak GC, arbitrary mutation/reentry/order and native enumeration remain outside the evidence.

- Added a typed-container family: AS3 Vector and Dictionary, identity-preserving indexed operations, for-in/AS3 for-each, member deletion/membership and Vector JSON encoding. One inventory project connects typed models, linked-card reuse and report generation; fixed traits and unsupported native enumeration have explicit boundaries. Weak Dictionary storage uses actual weak references; garbage-collection timing and full AVM conformance remain unverified.

- Independently verified the 852c8b7 affine/color delivery: matrix/skew and world-pose preservation, distinct bitmap tints, parent-alpha pixels/hits, transform-copy ownership and Restart, with one shared browser regression. Recorded P1 TRANSFORM-AS2-RGB-001: canonical AS2 rgb writes remain a silent custom field; the adjacent color control works. Exact declarations retain their bounded status.

- Independently verified the 7ed1605 data-processing delivery: real JSON/RegExp/String reporting, public getter serialization and safe diagnostics, UTC leap/month/year boundaries checked against Python timestamps, AS2 Date and Restart. One five-project browser batch preserves Sound, text, linked-timeline and collection behavior. Exact declarations remain partial; locale/DST/parser, full PCRE and general JSON class semantics are not inferred.

- Added runtime 2D Transform and ColorTransform: exact skew-preserving matrices, scalar edits, nested RGBA transforms, independent colorized views of shared BitmapData, AS2/AS3 constructors and linked-object lifecycle. Stage-coordinate bounds and finite numeric limits are explicit; native authoring skew, window-coordinate mapping and 3D remain unfinished.

- Independently verified the d8f580a Sound delivery: preserved MP3 bytes, embedded AS3 and HTTP AS2 loading, real silent channel completion with bounded repeats, transform-copy/refusal behavior, Stop and active-playback Restart. One five-project browser batch preserves text, bitmap, linked-timeline and collection behavior. Audible quality, WAV native decode and full mixer/timeline audio remain outside this evidence.

- Added one data-processing family: AS2/AS3 Date, AS3 constructor-based RegExp with String matching/replacement/splitting, bounded JSON callbacks and public class getter serialization, and typed Date/RegExp plus resolved AS3 is/as operations. A combined embedded-data project renders UTC planning dates and a serialized report. Modern browser date/regex behavior is explicitly partial; regex literals, historical locale/PCRE parity and unsupported native JSON encodings remain follow-ups.

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
