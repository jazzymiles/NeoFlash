# AS2 text, snapshots and interaction

AS2 display publication supports four related profiles: styled uniform text, static-text snapshots, legacy input preferences/callbacks, and TextField surfaces/scrolling/listeners. It is useful partial support, not complete AVM1 text or interaction conformance.

## Working behavior

**StyleSheet and uniform HTML.** `TextField.StyleSheet` owns ordinary primitive style records and supports `clear`, `getStyle`, `getStyleNames`, `setStyle`, `parseCSS`, `transform`, `load` and `onLoad`. Rust parses simple tag and `.class` selectors, comma-separated rules, comments and dashed property names. Supported render mappings are font family/size, RGB color, weight/style, underline, text alignment, margins, indent and leading. Unknown CSS can be retained as data, but trying to render unsupported properties produces a diagnostic. CSS parsing fails atomically; successful style mutations reflow attached fields.

AS2 `html`, `htmlText`, `styleSheet` and `condenseWhite` use the existing Rust XML model and a bounded interpreter for balanced `p`, `span`, `b`/`strong`, `i`/`em`, `u`, `font`, `textformat` and `br` markup. Visible text must have one uniform format. Mixed rich runs, unsupported tags/attributes and malformed markup produce explicit diagnostics. `htmlText` remains literal text unless `html` is enabled or a stylesheet is attached. A stylesheet also applies to text assignments, following the referenced AVM1 behavior. Original HTML is retained until text changes; subsequent reads produce normalized uniform markup rather than byte-for-byte source preservation.

AS2 `TextFormat` adds rendered `leftMargin`, `rightMargin`, `indent`, `blockIndent` and `underline`, and `getTextExtent`. Rust owns wrapping, paragraph offsets and underline geometry. Font widths come from the publication's existing browser measurement facility. Extent ascent/descent use NeoFlash's existing layout profile; they do not claim original Flash shaping or metrics. Existing whole-field get/set format APIs round-trip the added fields. Fonts are browser/system fonts under the existing publication contract; no font embedding is added.

**TextSnapshot.** `MovieClip.getTextSnapshot()` captures actual direct authored static-text children, excluding dynamic TextFields and text in nested clips. The nine TextSnapshot methods cover count, UTF-16 range extraction, ASCII-insensitive search, selected ranges/text, selection color, character run geometry and nearest-character hit testing. Rust owns the snapshot, selections and geometry. Selection is painted by the ordinary runtime renderer, follows current placement transforms and stops painting stale character geometry when a source field's text revision changes. Captured text remains immutable. Per-character advances use browser measurements corrected to measured line width; kerning is approximate. Limits are 16,384 UTF-16 units per snapshot, 64 snapshots, 65,536 total captured units and 4,096 entries per run-info request. Restart invalidates handles.

**Legacy input.** AS2 MovieClip/Button/TextField preferences use Rust-owned optional flags and tab indices. `enabled` suppresses button-like callbacks and hand cursors; MovieClip `focusEnabled` controls programmatic focus. `tabEnabled`, `tabIndex` and MovieClip `tabChildren` participate in deterministic live, visible, eligible ordering. When any candidate has an explicit index, only indexed candidates participate; otherwise placement order is derived from coordinates. Legacy Tab traversal activates after an explicit AS2 preference write, preserving the existing default path for AS3 content. MovieClip mouse callbacks broadcast to registered on-stage clips. MovieClip/Button key callbacks go to the focused target; they do not imply `onClipEvent(keyDown/keyUp)` broadcast compatibility. Implicit MovieClip handlers and explicit Mouse/Key listener registration are separate delivery routes. A clip using both receives both calls; no identity-based deduplication is applied. A focused clip registered with Key likewise retains both routes; the current profile does not reproduce Flash's keyboard-focus-highlight gate or exact cross-route ordering. Pressed pointer boundary crossings drive `onDragOut`/`onDragOver`. `onUnload` covers removal of a still-live dynamic runtime object; authored timeline destruction and historical unload ordering remain outside the profile.

**TextField viewport and listeners.** `background`, `backgroundColor`, `border`, `borderColor`, `scroll`, `hscroll`, `maxscroll`, `maxhscroll` and `bottomScroll` use Rust surface/layout state and affect the actual canvas and native text-input surface. Scroll positions clamp to layout limits; underline and glyph geometry move together. Direct `onScroller` and registered listeners receive the field identity when a programmatic clamped scroll value changes. `addListener`/`removeListener` reuse the existing Rust broadcaster and also receive native `onChanged` callbacks. Wheel scrolling and anchored `autoSize` are described in the [TextField compatibility notes](COMPATIBILITY.md#textfield-textformat-and-uniform-layout).

## Ownership and isolation

- `crates/neoflash-core/src/as2_text.rs`: CSS/HTML interpretation and extent requests.
- `crates/neoflash-core/src/text.rs`: shared owned format/layout, surfaces and viewport geometry with unchanged defaults.
- `crates/neoflash-core/src/runtime_graph/as2_snapshot.rs`: static capture, search, selection, geometry and selection paint.
- `crates/neoflash-core/src/runtime_graph/as2_input.rs`: legacy interaction state, focus eligibility and tab order.
- `crates/neoflash-web/src/as2_text.rs` and the existing `as2_display.rs`: typed WASM request/result transport.
- `web/as2-text.js`: native reference/format objects, browser loading, callbacks and the existing Rust broadcaster adapter.
- `web/display.js`, `web/text.js`, `web/text-input.js`: existing browser input/render facilities and reference marshalling.

The compiler/runtime resolves new legacy properties through per-access AS2 references. It does not permanently mark a shared object as AS2. Standard AS3 TextFormat construction and unsupported AS3 HTML properties retain their existing contract. New default fields preserve existing Rust text layout behavior. The native input mentioned above is the HTML5 publication's browser input element.

## Provenance

Selected behavior and algorithms were read and adapted from Ruffle v0.7.1, pinned revision [`89a7049965f3090e706265670df309a5128233ac`](https://github.com/ruffle-rs/ruffle/tree/89a7049965f3090e706265670df309a5128233ac), under the selected MIT license:

- `core/src/avm1/globals/style_sheet.rs`: style mapping and stylesheet API behavior.
- `core/src/avm1/globals/text_field.rs` and `core/src/display_object/edit_text.rs`: HTML flag/styleSheet behavior and text properties.
- `core/src/avm1/globals/text_format.rs`: paragraph format and extent behavior.
- `core/src/avm1/globals/text_snapshot.rs`: direct static-text snapshot operations and case-insensitive search profile.
- `core/src/display_object.rs`: legacy interaction properties.
- `core/src/player.rs`, `core/src/display_object/interactive.rs` and `core/src/avm1/globals/as_broadcaster.rs`: clip events and explicit singleton notifications are separate dispatch paths; there is no deduplication between the two.

These ideas are adapted to NeoFlash's own bounded text model, Rust XML parser, lifetime-stamped references, renderer and event routes. No complete Ruffle engine or foreign-language runtime is included. The code is not described as wholly original: the Ruffle attribution and full MIT notice remain in `THIRD_PARTY_NOTICES.txt` and are bundled with HTML5 publications. Downloaded reference files remain ignored under `target/verification/as2-two-hour/text-upstream/`.

## Example and verification boundary

[as2-display-batch.nflash](../examples/as2-display-batch.nflash) demonstrates uniform text, snapshots, scrolling and legacy interaction. Representative browser checks covered visible text and pointer, keyboard and Tab focus behavior. Full original-player text/input conformance has not been established. AS2 native-operation diagnostics are not generally recoverable through authored catch; malformed or unsupported rich text can terminate the script.

Coverage decisions and declaration counts are maintained in the [complete ActionScript audit](progress/ACTIONSCRIPT.md). These profiles describe partial support, not full Flash compatibility.
