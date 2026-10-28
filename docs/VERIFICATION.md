# Verification record

## How to read this record

These are scoped observations from **27–28 October 2026**, retained for the development preview. They describe the named revisions and workflows; later implementations do not inherit their results. Current contracts and declaration decisions are in the [compatibility matrix](COMPATIBILITY.md) and [ActionScript audit](progress/ACTIONSCRIPT.md). The [normalized audit](progress/actionscript-audit.json) preserves per-entry provenance.

Ordinary HTML5 exports were exercised with the shared Rust/WASM runtime. Unless explicitly named otherwise, browser observations used the Codex in-app browser without independent engine/version identification. Original Flash was not executed. DOM readiness, automated execution and observed interaction are separate evidence; startup-only projects do not receive full behavior acceptance. Screenshot comparisons establish consistency within the observed browser profile, not Flash raster equivalence.

This page keeps tested behavior and meaningful limits. Raw logs, artifact fingerprints, screenshots and delivery notes remain local under ignored `target/`.

<a id="automated-checks"></a>
## Automated verification

The recorded shared checks ran on Apple Silicon, macOS 15.8 and Rust 1.99.1. They cover formatting and Clippy, native Rust behavior, the macOS paste adapter, actual WASM and browser-host execution in Node, emitted-script fixtures, project workflows, public FLA corpus integrity/refusals, export preparation and demo export. Earlier failed gates and their affected reruns do not constitute an uninterrupted all-green pass.

Use `./scripts/check.sh` for a complete delivery check. After a concrete project-family failure, `NEOFLASH_WORKFLOWS=id1,id2 node crates/neoflash-script/tests/project-execute.mjs` selects affected families; that selection is not a complete delivery check. Browser and native UI behavior still require direct observation. The Metal dependency's `block 0.1.6` future-compatibility notice was not a current compilation error.

## Open findings and scoped fixes

| Finding | Recorded status and boundary |
| --- | --- |
| `FAMILY-HARNESS-001` | An unlocated aggregate-page `MutationObserver.observe` exception remains unresolved. Clean standalone examples or zero dashboard alerts do not close it. |
| `TRANSFORM-AS2-RGB-001` | AS2 `ColorTransform.rgb` silently stores a custom field without changing the tint in the checked revision; the adjacent `.color` path works. |
| `RESOURCE-FIXTURE-INPUT-001` | The resource example's overlaid label blocks its GET control; clicking the uncovered rectangle works. A general pointer regression was not established. |
| `LINKED-AS2-HOST-ERROR-001` | The checked AS2 missing-link host error escapes authored `try/catch`; the valid linked workflow passes separately. |
| `IMAGE-OBSERVATION-001` | Later image-pipeline logs were unavailable while screenshots/accessibility observations still showed completed actions. Missing logs are not zero-error evidence. |
| Native Sources / `D-008` | Source-specific feedback was implemented after the older detached-Actions finding; live Sources editing, feedback, Undo and reopen acceptance remain pending in this record. |
| `FAMILY-INPUT-001` | The shared mouse guard failed at `01f1381` and `a05a08d`. The observed mouse route was corrected and accepted at `a3fedc3`; this is not universal pointer conformance. |
| `AMF-TYPED-CATCH-001` | Malformed AMF escaped `catch(Error)` at `eddcc30`. The narrow codec-boundary correction was independently accepted at `c10e19e`; native error classes/error IDs remain unverified. |

Other recorded gaps include AS2 defaults/Number/owner-aware behavior, the first unresolved nested linked-target miss in the pointer example, and a clipping-example label overlap. These observations are historical evidence, not a fresh assessment of subsequent code.

<a id="web-container-and-navigation-workflow"></a>
## Web page integration

Real HTML buttons called AS3/AS2 callbacks, Canvas controls called receiver-preserving page functions, and native GET/form POST plus response-free sends produced actual local HTTP receipts. Restart recreated callbacks and the page controls without duplicate activation. Automated actual-WASM checks separately covered returned records, callback replacement/removal and stale wrappers.

Cross-origin policy, popup defaults, other engines, native authoring and complete exception/coercion behavior were not verified. See [ADR 0033](decisions/0033-web-container-integration.md).

<a id="independent-pointer-verification-of-01f2261"></a>
<a id="pointer-drag-and-targeting-workflow"></a>
## Pointer dragging and targeting

Audit `POINTER-HTML5-20261027T1113Z`, revision `01f2261`: actual drags exercised an invisible hit-area margin, parent-space limits, child/disabled modes, AS2 outside release and fresh Restart. The AS3 parent was unrotated, the AS2 parent was −4°, and both used unit scale. Shape-versus-bounds checks and a refused negative-width constraint accompanied working drags. Source handlers explicitly called `stopDrag`; release alone was not shown to terminate dragging.

Linked-card body clicks and keyboard activation passed, but an initial nested-target miss remained unresolved. Native cursor effects, arbitrary transforms, active-capture Restart, lockCenter, reparent/detach cancellation, precise glyph/alpha/filter hits, touch and pen were not independently accepted. Controlled-host capture/cursor tests are supplementary. See [ADR 0032](decisions/0032-pointer-drag-hit-testing.md).

<a id="independent-video-verification-of-f42ad94"></a>
<a id="file-video-and-local-stream-workflow"></a>
## Video playback

Audit `VIDEO-HTML5-20261027T1042Z`, revision `f42ad94`: a four-second H.264 clip with silent AAC reached AS3-created and AS2-authored Video views. Real controls verified metadata, pre-completion pause, seek, resume, completion, post-completion Close retaining frames, AS3-only clear and Restart during HTTP playback. Screenshots confirmed retained paused/final frames and changed seek/resume frames.

This short complete file does not establish audible output, precise synchronization, large-file transport, arbitrary codecs, active-close/rebinding races, smoothing, complex masks/transforms/filters or native Video authoring. See [ADR 0031](decisions/0031-video-streams.md).

<a id="independent-classpackage-verification-of-6687ced"></a>
<a id="source-class-access-and-package-workflow"></a>
<a id="independent-function-verification-of-11a59df"></a>
<a id="source-function-arguments-and-command-workflow"></a>
## Classes, packages and functions

Audit `CLASSES-HTML5-20261027T1007Z`, revision `6687ced`: visible component workflows verified separate base/derived private state, protected/package collaboration, dynamic enumeration/public JSON, bounded sealed refusals, positive import/final use, distinct AS2 inherited-private behavior and Restart. Rejected final overrides, wildcard ambiguity, every namespace/signature collision and complete reflection/static initialization were not independently accepted.

Audit `FUNCTIONS-HTML5-20261027T0914Z`, revision `11a59df`: corrected Default/Batch/Borrow controls verified optional/rest/supplied arguments, tested ordinary versus bound receivers, arity refusals and Restart. An added `arguments.length` probe had changed the initial fixture's allow-extra contract; the corrected result does not erase that altered probe. Complete VM global scope, prototypes/reflection, dynamic Function construction and arbitrary receiver/coercion combinations remain outside the evidence. See [class/package decisions](decisions/0030-class-packages.md) and [function decisions](decisions/0028-source-function-arguments.md).

<a id="independent-image-pipeline-verification-of-6f4f66a"></a>
<a id="bitmapdata-image-pipeline"></a>
<a id="independent-image-loader-verification-of-039afe6"></a>
<a id="runtime-loader-and-moviecliploader-image-batch"></a>
## Bitmap processing and image loading

Audit `IMAGE-PIPELINE-HTML5-20261027T0943Z`, revision `6f4f66a`: the initial source/tint oracle, captured/round-trip/loaded PNG equality, bulk cursor/word checks and partial `setPixels` EOF path were observed. Later palette phases and Restart have screenshot/accessibility evidence, but full PNG traces exist only for the initial phase. Unavailable later logs do not support repeated full-byte comparisons or a whole-run clean-console claim.

Audit `LOADER-HTML5-20261027T0709Z`, revision `039afe6`: embedded PNG, HTTP JPEG, snapshotted `loadBytes`, metadata/event order, replacement/unload, detached-image re-adoption, input and Restart were observed. Held HTTP responses produced no stale callbacks after close→Restart or Restart. Pending HTTP close **alone** remains needs review because Restart intervened; progress-event delivery was not observed.

Exact color/raster conversion, every Matrix/ColorTransform/clip combination, active text capture, non-normal blends, SWF/GIF loading, application domains, complete garbage-collection/reentry semantics and native bitmap authoring remain outside these checks. See [image-pipeline](decisions/0029-bitmap-image-pipeline.md) and [Loader](decisions/0024-runtime-image-loading.md) decisions.

<a id="independent-filter-verification-of-5395375"></a>
<a id="runtime-display-filter-batch"></a>
## Filters

Audit `FILTERS-HTML5-20261027T0844Z`, revision `5395375`: Blur, Glow, DropShadow and ColorMatrix visibly affected complete vector/text/bitmap cards. Alternate/clear/restore/Restart states, copied settings, unchanged shared source pixels, source clicks and excluded halo clicks were observed. Separate renderer-adapter checks cover cache/clip ordering, not browser pixels.

Filtered native editing, actual browser mask/scroll combinations, HiDPI/performance/cache exhaustion, every flag/coercion and exact Flash kernels were not verified. This run does not accept later BitmapData filtering work. See [ADR 0027](decisions/0027-runtime-display-filters.md).

<a id="independent-forms-verification-of-c10e19e"></a>
<a id="text-input-focus-and-saved-form-batch"></a>
## Text input and saved forms

Audit `FORMS-HTML5-20261027T0811Z`, revision `c10e19e`: actual first-click focus, typing/paste, user-only restriction/length limits, multiline UTF-16 selection, Tab transfer, cancellable input, AS3 events and AS2 Selection/change callbacks were observed. Programmatic replacement bypassed user-only filters. Save/reload/Restart retained values; active Restart removed the editing control and discarded the observed unsaved edit. Clearing the named save restored defaults.

The malformed-AMF regression reached generic typed `catch(Error)` with its cursor and input unchanged, closing `AMF-TYPED-CATCH-001` only for that codec boundary. Full EOFError/errorID fidelity is not established. IME, password/selectable modes, complex clipping/occlusion/transforms, all Unicode boundaries, reentrant focus, complete keyboard/accessibility and native input-field authoring remain unaccepted. See [ADR 0026](decisions/0026-text-input-and-focus.md).

<a id="independent-storage-and-amf-verification-of-eddcc30"></a>
<a id="sharedobject-persistence-and-amf-value-batch"></a>
## Saved data and AMF

Audit `STORAGE-AMF-HTML5-20261027T0728Z`, revision `eddcc30`: Save/reload/Restart, two pathname scopes and clear preserved bounded AS2/AS3 graph values. Aliases/cycles, selected special values and nested bytes survived; AS2 and AS3 clear retain different data-object identity behavior. An independent byte reader decoded the fixture's AMF0/AMF3 streams and trailing UTF data.

The original malformed-read fixture failed because a raw string escaped `catch(Error)`; its separate `catch(*)` positive control did not fix or erase that failure. The later forms record provides the scoped correction. Broad AMF interoperability, typed/externalizable/XML/Vector/Dictionary reconstruction, native `.sol`, remote synchronization, real quota denial, concurrent tabs and all secure/localPath variants were not accepted. See [ADR 0025](decisions/0025-shared-objects-and-amf.md).

<a id="independent-clipping-verification-of-05d5d43"></a>
<a id="runtime-masks-and-scroll-rectangles-batch"></a>
## Masks and scroll rectangles

Audit `CLIPPING-HTML5-20261027T0652Z`, revision `05d5d43`: vector masks with even-odd holes, copied integral scroll rectangles, visible/excluded clicks, AS2 direct-object `setMask`, retained linked-card state and Restart were observed. Self-mask and fractional-scroll refusals left a working scene. Screenshot and coordinate comparisons supported the observed restoration/offset behavior.

The outer parent viewport edge was not isolated independently. Source reuse/retirement, budgets, arbitrary reentry, exact edge pixels, full accessible clipping, alpha/bitmap/text masks and animated source timelines remain outside this acceptance. See [ADR 0023](decisions/0023-runtime-masks-and-scroll-rectangles.md).

<a id="independent-graphics-verification-of-08f1d9c"></a>
<a id="gradient-graphics-and-shared-path-batch"></a>
## Runtime Graphics

Audit `GRAPHICS-HTML5-20261027T0619Z`, revision `08f1d9c`: linear/radial fills and strokes, independent copied paint, Vector/cubic paths, real input, reparent/detach lifetime and Restart were observed. Unsupported reflect spreading and incomplete path data produced diagnostics beside the positive scene. `drawRoundRectComplex` is a NeoFlash extension and supplies no Adobe catalogue promotion.

The accepted gradients use pad/RGB. Other spreading/interpolation, all focal cases, alpha hits, exact stroke/raster behavior, resource limits and native gradient authoring were not accepted. Bounds remaining unchanged after an error do not prove complete command-buffer atomicity. See [ADR 0022](decisions/0022-runtime-gradient-graphics.md).

<a id="independent-e4x-verification-of-5be13d9"></a>
<a id="e4x-selectors-editable-targets-and-literal-batch"></a>
<a id="independent-xml-verification-of-f4fcb71"></a>
<a id="xml-parser-dom-and-xmllist-batch"></a>
## XML and E4X

Audit `XML-HTML5-20261027T0545Z`, revision `f4fcb71`: exact embedded XML bytes, AS2 DOM and AS3 tree copy/mutation/serialization, Unicode and expanded namespaces produced the expected visible catalog and Restart state. Fifteen serialized logical trees matched independent parsing. AS2 `parseXML` had failure-only evidence; direct `XML.setChildren` was not established by delegated XMLList use. An unqualified handler probe was inconclusive.

Audit `E4X-HTML5-20261027T0608Z`, revision `5be13d9`: bounded selectors, XML/XMLList literals, ordered interpolation, targeted creation, Namespace/QName selection and content-preserving ambiguous-write refusal were observed. Twenty-four logical structures matched independent parsing. Corrected handler access established a narrow typed XML bridge; it does not rewrite the earlier inconclusive result.

Pretty-print setters, direct methods not invoked, full filters/default namespaces/dynamic names, multi-parent/descendant writes, XML equality/addition and historical parser/coercion behavior remain unverified or unsupported. See [XML](decisions/0020-xml-tree-and-language-facades.md) and [E4X](decisions/0021-e4x-references-and-literals.md) decisions.

<a id="independent-typed-collection-verification-of-6396fd0"></a>
<a id="typed-collections-and-enumeration-batch"></a>
<a id="independent-transform-verification-of-852c8b7"></a>
<a id="runtime-transform-and-colortransform-batch"></a>
## Typed collections and display transforms

Audit `TYPED-COLLECTIONS-HTML5-20261027T0525Z`, revision `6396fd0`: specialized Vector models, identity-keyed Dictionary, JSON public getters and AS2 record enumeration produced the expected inventory. Recycled cards retained listeners and identity; fixed/bounds/type refusals accompanied working controls and Restart. Weak-key GC, arbitrary mutation/order/reentry, other specializations and native enumeration remain outside the evidence.

The bounded coordinator review [COORD-AS3-FOUNDATIONS-20261028](progress/reviews/as3-foundations-20261028.json) extends this same inventory workflow with numeric Class aliases/limits and bound formatting, dynamic property visibility, public source-class reflection without getter execution, and dense Array insertion/removal. Actual exported WASM/Canvas in the in-app browser showed totals 13, 16 and 17 with formatted roots 3.61, 4.00 and 4.12; object/reflection indicators remained true and Restart restored the initial inventory and history. The shared project runner collected all 24 families; a TextField newline expectation was corrected from LF to the established CR result, then only the affected family was rerun successfully. Source fingerprints and the working-tree/coordinator boundary are in the review. Native/interface/bytecode reflection, sparse mutation and full VM/rounding conformance are outside this evidence.

The follow-up [COORD-AS3-OBJECT-IDENTITY-20261028](progress/reviews/as3-object-identity-20261028.json) covers ordinary source-class Class aliases, prototype/constructor links, inherited/unrelated isPrototypeOf results, valueOf identity and default short labels with preserved custom overrides. The same exported inventory displayed `identity true` initially, after two actions and after Restart. Its shared regression collection passed 140 Rust tests, 91 emitted-script fixtures, 25 project workflows and 63 web tests. These observations are scoped to the captured AS3 implementation; later shared-tree Button changes are not independently accepted by this review.

Audit `TRANSFORMS-HTML5-20261027T0502Z`, revision `852c8b7`: affine/skew preservation, world-pose-preserving reparenting, independent bitmap tints, alpha visibility/hits, copied values and Restart were observed. World matrices agreed with independent arithmetic. AS2 `.rgb` failed silently while `.color` worked (`TRANSFORM-AS2-RGB-001`). Exact rendering, every transform combination and native editing were not established. See [collections](decisions/0019-typed-collections-and-enumeration.md) and [transforms](decisions/0018-runtime-display-transforms.md) decisions.

<a id="independent-data-processing-verification-of-7ed1605"></a>
<a id="date-regexp-and-json-data-processing-batch"></a>
## Dates, JSON and regular expressions

Audit `DATA-HTML5-20261027T0452Z`, revision `7ed1605`: embedded JSON, RegExp/String processing and public getter serialization produced a visible report. AS2/AS3 numeric UTC dates advanced across selected leap-day, year and month boundaries, with nine AS3 timestamps matching an independent UTC oracle; Restart restored the report.

Local calendar/DST, parsing/locale, full PCRE behavior/backtracking limits and arbitrary JSON class/coercion semantics were not verified. Native RegExp serialization required an explicit replacer. Later syntax additions do not inherit this check. See [ADR 0017](decisions/0017-data-processing-builtins.md).

<a id="independent-audio-verification-of-d8f580a"></a>
<a id="scripted-sound-channels-and-silent-browser-decoding"></a>
## Audio

Audit `AUDIO-HTML5-20261027T0429Z`, revision `d8f580a`: embedded AS3 and HTTP AS2 MP3 loading used native browser decoding and source completion. Zero-volume channels demonstrated the distinct repeat-count profiles, transform copies/refusals, Stop and Restart during active playback. A separate native-browser/offline probe verified bounded PCM decoding and silent output; it is supplementary evidence.

Audible quality/pan, WAV native decoding, nonzero offsets/gain, precise clocks, pending-decode races, full mixing, streaming and native timeline audio were not accepted. Later ID3/linkage additions receive no automatic acceptance. See [ADR 0016](decisions/0016-scripted-audio.md).

<a id="independent-text-layout-verification-of-fba17ae"></a>
<a id="text-fields-uniform-formats-and-shared-layout"></a>
<a id="independent-bitmap-verification-of-1bd0e3c"></a>
<a id="bitmap-resources-and-shared-pixel-buffers"></a>
## Text layout and shared bitmaps

Audit `TEXT-LAYOUT-HTML5-20261027T0409Z`, revision `fba17ae`: dynamic AS2/AS3 fields showed width-driven wrapping/alignment at unchanged object scale, independent current/default uniform formats, copied values, content-preserving mixed/range refusals and Restart. Browser metrics are not a Flash font oracle. Rich/mixed runs, portable fonts and live native text authoring were not accepted; later input has its own record.

Audit `BITMAP-HTML5-20261027T0345Z`, revision `1bd0e3c`: exact PNG bytes and selected decoded samples supported shared-view painting, independent clone/copy/scroll, AS2 attachment, disposal and Restart. Direct AS3 construction had only a negative diagnostic and remained needs review. This record does not establish JPEG, alpha-hit, general rasterization or later filter/encode behavior. See [text](decisions/0015-text-fields-and-layout.md) and [bitmap](decisions/0014-bitmap-pixels.md) decisions.

<a id="independent-resource-and-loading-verification-of-c39a29e"></a>
<a id="portable-resources-and-data-loading-batch"></a>
## Portable resources and HTTP data

Audit `RESOURCES-HTML5-20261027T0323Z`, revision `c39a29e`: version-3 embedded text/form/binary bytes were preserved without HTTP requests. Actual GET/POST/sendAndLoad requests retained repeated values and headers, updated visible data and produced useful missing-URL diagnostics. Held responses after close or Restart produced no obsolete callbacks within the observed race.

The resource-label hit limitation remains `RESOURCE-FIXTURE-INPUT-001`. Direct-file opening was not accepted for this resource workflow; cross-origin CORS, redirects, all historical event/error contracts and native Library/Undo/clipboard remain unverified. See [ADR 0013](decisions/0013-portable-resources-and-loading.md).

<a id="independent-linked-movieclip-and-interface-verification-of-798e28b"></a>
<a id="linked-movieclip-and-as3-contract-batch"></a>
<a id="independent-dynamic-display-and-binary-verification-of-2b5deff"></a>
<a id="dynamic-display-graphics-and-bytearray--27-october"></a>
## Linked timelines, interfaces and dynamic display

Audit `LINKED-INTERFACES-HTML5-20261027T0243Z`, revision `798e28b`: linked AS3 MovieClips had independent authored children/clocks/counts, retained state across detach/reattach and reset on Restart. AS2 `attachMovie` exposed its initObject during construction. Interface/accessor controls preserved independent models and showed nominal conversion/getter-only refusals. Full initial frame-action ordering was not established. The invalid AS2 missing-link case exposed `LINKED-AS2-HOST-ERROR-001` separately from the positive workflow.

Audit `DYNAMIC-BINARY-HTML5-20261027T0220Z`, revision `2b5deff`: runtime cards retained Graphics, state and listeners through reparent/order/detach, ignored input while absent, and restarted fresh. A cycle refusal preserved the observed graph. ByteArray numeric/UTF copying, cursor behavior and the Unicode packet repeated after Restart; EOF/compression diagnostics accompanied working output. Compression, external byte-layout interoperability and native EOFError fidelity were not established. See [display graph](decisions/0012-runtime-display-graph.md) and [project-program](decisions/0011-project-programs-and-browser-families.md) decisions.

<a id="independent-geometrytimer-verification-of-ed3c39b"></a>
<a id="geometry-and-timer-family--27-october"></a>
<a id="independent-as2input-verification-of-a3fedc3"></a>
<a id="as2-project-classes-and-classic-input--27-october"></a>
<a id="independent-collectionstext-verification-of-a05a08d"></a>
<a id="array-string-and-plain-text-families--27-october"></a>
<a id="independent-family-verification-of-01f1381"></a>
<a id="integrated-project-display-and-event-families--27-october"></a>
## Timers, project classes and initial input

Audit `GEOMETRY-TIMER-HTML5-20261027T0152Z`, revision `ed3c39b`: four callbacks moved a marker through the expected typed geometry, completed once and restarted without an abandoned-session tick. Singular inversion and negative delay diagnosed beside the positive workflow. Positive `Matrix.invert`, public Timer reset/stop and exact scheduling were not independently exercised.

Audit `AS2-INPUT-HTML5-20261027T0140Z`, revision `a3fedc3`: project inheritance/imports, separate instance fields, shared static state, mouse/key callbacks and Restart were observed. The shared mouse guard correction restored the tested AS3 class and Stage capture/target/bubble routes. Logical resize and legacy listener removal were not browser-accepted.

Audits `COLLECTIONS-TEXT-HTML5-20261027T0120Z` (`a05a08d`) and `FAMILY-HTML5-20261027T0057Z` (`01f1381`) preserve earlier bounded class, geometry, keyboard, collection and text results alongside the reproduced mouse failure. They do not represent complete-package acceptance. The initial data-to-text profile does not establish every Array/String method or later rich text. Native Sources editing/Undo/reopen remained pending.

<a id="observed-desktop-checks"></a>
<a id="selection-and-free-transform-increment"></a>
## Desktop authoring and transforms

Native macOS checks observed the classic layout and system fonts, rectangle drawing with Undo, text creation, F8 symbol conversion, Save, Actions editing/checking, Library entry, nested scrubbing and onion skin. The saved document contained the new text and symbol.

Headless editor gestures additionally cover Pen/Pencil/Subselection, cross-layer selection, free transforms, reflected/group operations, snapping, cancellation, grouped Undo and frame/tool/view changes. Geometry tests cover compound holes and transformed primitive bounds. These do not replace live native observation: Pen/Pencil/Subselection, V/Q gesture combinations and Accessibility-panel interactions remained pending for their respective builds. Full UX, VoiceOver, IME, multi-display and performance acceptance is not claimed.

<a id="script-and-preservation-regressions"></a>
<a id="clipboard-increment"></a>
<a id="control-flow-solid-contours-and-paste-intent--27-october"></a>
## Clipboard, control flow and binary preservation

Automated compiler/host checks cover closures, recursion, hoisting, shadowing, typed arguments/returns, structured exceptions and visible trace delivery. Binary tests exercise truncated records, exact byte retention, references/ownership, budgets and actionable unsupported boundaries; a deterministic malformed-input probe found no panic in its sampled inputs. These tests establish bounded preservation, not complete binary-document conversion or visual fidelity.

Clipboard model/runtime/editor checks cover nested symbol closure, timelines/scripts, identity remapping, collisions, atomic validation, centered/in-place paste and text-field ownership. Actual macOS Copy/Cut/Paste, menu routing, cross-document transfer and native Save/Open were observed after the initiating-key modifier fix. Actions text operations preserved artwork; Safari Test Movie exercised the exported recovery handlers and Restart. An earlier foreign clipboard value was not reproducible, so exhaustive clipboard timing/platform conformance is not claimed.

The strict solid-contour adapter accepted a small subset of inspected Flash 3 shape candidates and diagnosed the rest. Candidate-level success did not enable editable binary FLA import. See [clipboard](CLIPBOARD.md) and [import boundaries](IMPORTS.md).

<a id="native-workspace-increment--27-october"></a>
## Native workspace

The 28 October authoring batch added ten tools, Modify commands, Align/Transform/Color panels, compact timing fields and Button/Graphic controls. One shared core/editor pass covered the existing families; an outdated default-panel visibility expectation was corrected and rerun alone. A later clipboard review found and fixed version inference for grouped/skewed artwork; only its existing round-trip family was replayed for the affected group, affine and easing cases. Workspace Clippy, Rust/WASM packaging and standalone HTML5 export passed. No authoring test families were added.

Native observations confirmed FPS/duration coupling with unchanged frame count, Brush with Undo, the Align controls, dragging an Align tab into a native floating window, returning it through Dock Top, and importing a PNG through the File menu onto the Stage. Finder-to-Stage dragging and dragging back between separate windows did **not** produce confirmed acceptance in this session. The routing model supports edge/tab/group destinations, but that is not evidence that every native gesture works. Drops on floating panels now receive explicit Stage guidance.

The exported authoring example ran in the in-app browser: a Button called AS2 and AS3 click handlers, satisfied `is SimpleButton`, and accepted a click in its invisible Hit margin. Graphic Loop/Play once with a starting frame and Restart were observed; no browser errors were recorded. This is a bounded linked/authored Button workflow, not arbitrary SimpleButton state-object API coverage. The local Flash CS4 book informed tool, group, symbol and panel behavior; implementation limits remain in [Workspace controls](WORKSPACE.md).

Native checks observed persistent detached Actions/Dark theme, accessible Check Script controls, F9 reopening/focus, docking, readable Light theme, compact Timeline footer reachability and synchronized scrolling in a large authored fixture. Detached tool choice and text selection/clipboard state survived docking in their checked build.

Native authoring checks also observed importing a local PNG onto the Stage, drawing with separate fill/stroke alpha and stroke width, layer-column resizing, drag reordering, custom layer-outline color, resizing the docked Timeline by its edge, and detaching Library by its header. A saved project reopened with its reordered layers and outline setting. Its ordinary HTML5 export displayed the embedded PNG and translucent drawing with solid artwork despite editor outlines, advanced frames and paused without runtime errors. Font fallback and embedded-resource notices remained visible.

Headless checks cover preferences/recovery, visibility/focus, contrast, clipboard viewport routing, native-window tab/split models and stable layout. Finder-to-Stage dragging and every cross-window docking destination still need direct native acceptance. A child-window startup panic was corrected by initializing accessibility before showing the child. Physical monitor relocation/disconnect, VoiceOver speech, IME and complete desktop accessibility remain unverified. Later dock/tab/split implementations do not inherit these earlier observations.

<a id="observed-html5-checks"></a>
<a id="vector-path-increment"></a>
<a id="authored-semantics-and-preview-scale"></a>
## Browser rendering, keyboard controls and preview scale

Actual **Chrome** checks observed nested animation, Pause, object Click scripts and Restart. Vector examples verified Bézier strokes, compound/even-odd/concave fills and transformed path input. The manual Canvas path suite exercised winding, transparent fills, curves, closing strokes, caps and caller-transform independence; Node tests separately cover cache budgets. This does not establish exhaustive geometry/performance or native drawing gestures.

Chrome keyboard checks observed named buttons, one activation per Enter/Space, retained focus during animation, disabled/image semantics and focus scrolling. A narrow viewport showed the complete fixed stage in Fit and a scrollable full-size stage at 100%; Pan moved the viewport without firing scripts. A separate in-app-browser check included 200% focus scrolling and Fit reset. Full VoiceOver, physical touch and cross-engine accessibility remain unverified; fixed-stage scaling is not responsive layout authoring.

A later native Test Movie opened and exercised one local export in **Safari**. That specific check does not verify every standalone resource/media workflow or clean-machine typography. Export packaging tests establish embedded WASM and local scripts; font embedding remains absent.

<a id="real-fla-to-html5--27-october"></a>
## Real FLA import and HTML5

One unchanged Adobe CS6 FLA from the licensed public corpus imported as an editable project. Independent source comparison checked scripts, solid contours/strokes, labels, translation and a blank key. Its ordinary HTML5 publication displayed the expected square and exactly matched the pinned original 17-line reference trace, including a second block after Restart. The original SWF was not executed for this check.

Importer regressions guard fragmented text/CDATA preservation, namespace shadowing and unsupported metadata. This proves one real FLA workflow, not generic binary FLA/SWF conversion or complete ActionScript compatibility. See the [public corpus](../examples/fla-corpus/README.md) and [import limits](IMPORTS.md).

<a id="html5-correctness-visibility-placement-and-numeric-profiles--27-october"></a>
## Visibility, replacement and numeric profiles

Actual **Chrome** and in-app-browser checks observed Boolean visibility removing pixels/hit targets while clocks continued, retained hidden state, fresh nested symbol-replacement state, accepted hidden-layer publishing and fresh placements after imported blank gaps. AS2 `parseInt("0777")` returned 511, AS3 returned 777, and bounded AS3 `toFixed` precision/refusals preserved adjacent custom-function behavior.

Additional visibility coercion diagnostics were checked in Chrome. Unsupported inputs produced contextual errors; the AS2 host-error boundary was not treated as complete typed exception support. Reference expectations were documentation-based, with no original Flash execution. Safari/Firefox, complete owner/type contracts and universal event ordering were not accepted by these checks.

<a id="legacy-corpus"></a>
<a id="artifacts"></a>
## Legacy inspection and build boundaries

Local corpus inspection recognized FLA/SWF signatures, accepted a bounded set of compound containers and retained typed timeline/shape/reference data with explicit partial-graph boundaries. Unsupported payloads, shared/cyclic edges and unread suffixes remained diagnosed or preserved. Inspection counts are not editable-import compatibility; binary-document conversion remained blocked in that profile. Private corpus content and derived artwork were not published.

Release macOS bundles and shared WASM built successfully in the recorded increments; ad-hoc signatures passed strict verification. The product requires macOS 15+. Notarization, Intel acceptance, full ActionScript conformance, generic FLA/SWF conversion and production readiness are not established. Reproducible build commands are in the [README](../README.md); public fixture provenance remains with the [corpus](../examples/fla-corpus/README.md).
