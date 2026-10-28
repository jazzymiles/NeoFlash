# AS2 text, snapshot and interaction batches

This package extends the existing AS2 display publication with four coherent families: styled uniform text, static-text snapshots, legacy input preferences/callbacks, and TextField surfaces/scrolling/listeners. It is useful partial support, not complete AVM1 text or interaction conformance. Editor UI, FLA import and AS3 capability registrations are outside this work.

## Working behavior

**StyleSheet and uniform HTML.** `TextField.StyleSheet` owns ordinary primitive style records and supports `clear`, `getStyle`, `getStyleNames`, `setStyle`, `parseCSS`, `transform`, `load` and `onLoad`. Rust parses simple tag and `.class` selectors, comma-separated rules, comments and dashed property names. Supported render mappings are font family/size, RGB color, weight/style, underline, text alignment, margins, indent and leading. Unknown CSS can be retained as data, but trying to render unsupported properties produces a diagnostic. CSS parsing fails atomically; successful style mutations reflow attached fields.

AS2 `html`, `htmlText`, `styleSheet` and `condenseWhite` use the existing Rust XML model and a bounded interpreter for balanced `p`, `span`, `b`/`strong`, `i`/`em`, `u`, `font`, `textformat` and `br` markup. Visible text must have one uniform format. Mixed rich runs, unsupported tags/attributes and malformed markup produce explicit diagnostics. `htmlText` remains literal text unless `html` is enabled or a stylesheet is attached. A stylesheet also applies to text assignments, following the referenced AVM1 behavior. Original HTML is retained until text changes; subsequent reads produce normalized uniform markup rather than byte-for-byte source preservation.

AS2 `TextFormat` adds rendered `leftMargin`, `rightMargin`, `indent`, `blockIndent` and `underline`, and `getTextExtent`. Rust owns wrapping, paragraph offsets and underline geometry. Font widths come from the publication's existing browser measurement facility. Extent ascent/descent use NeoFlash's existing layout profile; they do not claim original Flash shaping or metrics. Existing whole-field get/set format APIs round-trip the added fields. Fonts are browser/system fonts under the existing publication contract; no font embedding is added.

**TextSnapshot.** `MovieClip.getTextSnapshot()` captures actual direct authored static-text children, excluding dynamic TextFields and text in nested clips. The nine TextSnapshot methods cover count, UTF-16 range extraction, ASCII-insensitive search, selected ranges/text, selection color, character run geometry and nearest-character hit testing. Rust owns the snapshot, selections and geometry. Selection is painted by the ordinary runtime renderer, follows current placement transforms and stops painting stale character geometry when a source field's text revision changes. Captured text remains immutable. Per-character advances use browser measurements corrected to measured line width; kerning is approximate. Limits are 16,384 UTF-16 units per snapshot, 64 snapshots, 65,536 total captured units and 4,096 entries per run-info request. Restart invalidates handles.

**Legacy input.** AS2 MovieClip/Button/TextField preferences use Rust-owned optional flags and tab indices. `enabled` suppresses button-like callbacks and hand cursors; MovieClip `focusEnabled` controls programmatic focus. `tabEnabled`, `tabIndex` and MovieClip `tabChildren` participate in deterministic live, visible, eligible ordering. When any candidate has an explicit index, only indexed candidates participate; otherwise placement order is derived from coordinates. Legacy Tab traversal activates after an explicit AS2 preference write, preserving the existing default path for AS3 content. MovieClip mouse callbacks broadcast to registered on-stage clips. MovieClip/Button key callbacks go to the focused target; they do not imply `onClipEvent(keyDown/keyUp)` broadcast compatibility. Implicit MovieClip handlers and explicit Mouse/Key listener registration are separate delivery routes. A clip using both receives both calls; no identity-based deduplication is applied. A focused clip registered with Key likewise retains both routes; the current profile does not reproduce Flash's keyboard-focus-highlight gate or exact cross-route ordering. Pressed pointer boundary crossings drive `onDragOut`/`onDragOver`. `onUnload` covers removal of a still-live dynamic runtime object; authored timeline destruction and historical unload ordering remain outside the profile.

**TextField viewport and listeners.** `background`, `backgroundColor`, `border`, `borderColor`, `scroll`, `hscroll`, `maxscroll`, `maxhscroll` and `bottomScroll` use Rust surface/layout state and affect the actual canvas and native text-input surface. Scroll positions clamp to layout limits; underline and glyph geometry move together. Direct `onScroller` and registered listeners receive the field identity when a programmatic clamped scroll value changes. `addListener`/`removeListener` reuse the existing Rust broadcaster and also receive native `onChanged` callbacks. Automatic native wheel-to-Rust scroll synchronization and `autoSize` are not implemented by this package.

## Ownership and isolation

- `crates/neoflash-core/src/as2_text.rs`: CSS/HTML interpretation and extent requests.
- `crates/neoflash-core/src/text.rs`: shared owned format/layout, surfaces and viewport geometry with unchanged defaults.
- `crates/neoflash-core/src/runtime_graph/as2_snapshot.rs`: static capture, search, selection, geometry and selection paint.
- `crates/neoflash-core/src/runtime_graph/as2_input.rs`: legacy interaction state, focus eligibility and tab order.
- `crates/neoflash-web/src/as2_text.rs` and the existing `as2_display.rs`: typed WASM request/result transport.
- `web/as2-text.js`: native reference/format objects, browser loading, callbacks and the existing Rust broadcaster adapter.
- `web/display.js`, `web/text.js`, `web/text-input.js`: existing browser input/render facilities and reference marshalling.

The compiler/runtime resolves new legacy properties through per-access AS2 references. It does not permanently mark a shared object as AS2. Standard AS3 TextFormat construction and unsupported AS3 HTML properties retain their existing contract. New default fields preserve existing Rust text layout behavior. No editor source file is changed. The native input mentioned above is the HTML5 publication's browser input element.

## Provenance

Selected behavior and algorithms were read and adapted from Ruffle v0.7.1, pinned revision [`89a7049965f3090e706265670df309a5128233ac`](https://github.com/ruffle-rs/ruffle/tree/89a7049965f3090e706265670df309a5128233ac), under the selected MIT license:

- `core/src/avm1/globals/style_sheet.rs`: style mapping and stylesheet API behavior.
- `core/src/avm1/globals/text_field.rs` and `core/src/display_object/edit_text.rs`: HTML flag/styleSheet behavior and text properties.
- `core/src/avm1/globals/text_format.rs`: paragraph format and extent behavior.
- `core/src/avm1/globals/text_snapshot.rs`: direct static-text snapshot operations and case-insensitive search profile.
- `core/src/display_object.rs`: legacy interaction properties.
- `core/src/player.rs`, `core/src/display_object/interactive.rs` and `core/src/avm1/globals/as_broadcaster.rs`: clip events and explicit singleton notifications are separate dispatch paths; there is no deduplication between the two.

These ideas are adapted to NeoFlash's own bounded text model, Rust XML parser, lifetime-stamped references, renderer and event routes. No complete Ruffle engine or foreign-language runtime is included. The code is not described as wholly original: the Ruffle attribution and full MIT notice remain in `THIRD_PARTY_NOTICES.txt` and are bundled with HTML5 publications. Downloaded reference files remain ignored under `target/verification/as2-two-hour/text-upstream/`.

## Accounting

The baseline is `b3edf75`. Counts are catalogue declarations, including owner aliases, not independent algorithms or complete compatibility claims. Exact IDs and scope notes are in the coordinator's ignored `target/verification/as2-two-hour/display-inventory.json` and are intended for the shared catalogue audit.

| Change against baseline | Methods | Properties | Events | Constructors | Total |
| --- | ---: | ---: | ---: | ---: | ---: |
| New useful partial declarations | 20 | 28 | 14 | 1 | 63 |
| Existing declarations extended | 4 | 0 | 1 | 0 | 5 |
| Existing behavior source-reviewed | 3 | 10 | 13 | 0 | 26 |

No constant is added. The five extensions are TextField's four format get/set APIs and listener delivery for the existing `onChanged` event. The 26 source reviews cover already-working focus/button callbacks, local pointer/drag/hit-area/cursor behavior, filter and transform facades. They are explicitly not new implementations. Existing source review does not imply a new owner-specific browser pass. Coordinator-owned compiler event syntax, Selection target resolution and other agents' network/binding APIs are excluded from these totals.

## Verification and limits of evidence

Three grouped native family scenarios were added, each run once during development: `as2_text`, `as2_text_snapshot`, and `as2_input`; all three passed. The text scenario was then extended with surface/scroll assertions for the coordinator's shared delivery pass rather than rerun separately. A subsequent `cargo check -p neoflash-core -p neoflash-web --all-targets` passed after updating one existing TextFormatPatch fixture initializer with defaults. No independent full suite or WASM build was run by this agent.

`examples/as2-display-batch.nflash` remains one ordinary authored AS2 example. It extends the previous 20 checks to **41**: 40 synchronous checks plus one asynchronous CSS-load check. Pointer/key/Tab interactions have separate visible counters and are not added to the assertion count. The CSS is an embedded format-3 project resource. The example displays styled/underlined text, snapshot selection, a scrolled bordered text surface and independently interactive clips.

The deliberate mixed-format diagnostic is checked in the native family scenario, not caught inside the AS2 publication: AS2 catches currently handle authored thrown values while unsupported host diagnostics abort with their original message. The browser example uses supported uniform bold markup for that assertion.

The coordinator observed the actual exported publication at **41/41** checks and exercised pointer press, A key input and Tab focus movement in the browser. A screenshot was captured by that shared delivery pass. This is representative family evidence, not a browser pass for every owner alias or limit. The existing 24-family shared regression had 23 immediate passes; the AS2 input fixture was then corrected for the evidenced dual Mouse callback route and its focused rerun passed, including Restart. The existing `as2-project-input` fixture explicitly registers its MovieClip document class with Mouse while also defining `onMouseDown`; its mouse counter therefore now expects two calls per press, including after Restart. Pinned Ruffle `player.rs` separately queues the clip event and `NotifyListeners`, and `as_broadcaster.rs` calls the registered object without filtering clips. This expectation corrects previously missing implicit callbacks rather than introducing a deduplication policy. No comparison against an original Flash player has been performed.
