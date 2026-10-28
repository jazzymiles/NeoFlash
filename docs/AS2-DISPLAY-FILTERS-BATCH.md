# AS2 display composition, text layout and image loading

AS2 HTML5 publications support bounded display composition, uniform text layout and MovieClip image loading. Partial support does not imply original Flash pixel or virtual-machine conformance.

## Display composition

AS2 MovieClip and Button `blendMode` and `cacheAsBitmap`, and MovieClip `opaqueBackground`, now affect rendered output. Rust validates and stores the state on lifetime-stamped runtime objects, emits nested raster groups, transforms background colors, and decides composition modes. The browser host creates bounded Canvas surfaces and performs browser composition. Supported blend names are `normal`, `layer`, `multiply`, `screen`, `lighten`, `darken`, `difference`, `add`, `overlay` and `hardlight`, with their corresponding legacy numeric aliases. Unsupported subtract/invert/alpha/erase modes diagnose before mutation.

Raster groups share the existing filter pipeline, geometry, masks, scroll rectangles and invalidation keys. Destination blend state survives the clipping surface's final blit. `cacheAsBitmap` retains a reusable raster until its content or geometry changes; filters also imply effective caching. `opaqueBackground` fills the conservative source bounding rectangle before drawing the subtree. This is Stage-pixel caching rather than original Flash's local texture optimization. Existing budgets remain: 128 combined effect groups, 4 Mi-pixels per surface, 8 Mi-pixels in the pool, 16 Mi-pixels of frame work and 16 surfaces. Native DOM TextField editing or selection inside a filtered/cached/blended subtree is explicitly unsupported because the DOM overlay cannot reproduce that raster composition.

BlurFilter, GlowFilter, DropShadowFilter, ColorMatrixFilter and the supported extension filters use shared Rust kernels. BitmapFilter is a supplied base whose clone method operates on supported concrete instances. Filter values are copied by the display facade.

## Uniform text layout

AS2 TextFormat adds rendered `letterSpacing`, `kerning`, `bullet`, `tabStops`, `url` and `target`. Rust owns spacing-aware advances, wrapping, positioned tab runs, bullet geometry, line hit testing and extent results. Canvas supplies actual installed/browser font measurements, kerning and glyph drawing. Explicit spacing/kerning require the corresponding browser Canvas facility and diagnose when unavailable; this is not a claim that every browser on macOS 15 supports those facilities.

Custom tab stops are bounded to 64 increasing nonnegative positions and currently require `wordWrap=false`. After the final custom stop, the profile repeats a four-font-size interval. Bullets indent paragraphs and render a circle at the first line. Bullets and custom tabs do not support native input editing or native selection: input fields reject those formats and dynamic paragraph fields remain on Canvas when clicked, rather than being replaced by an incorrectly formatted textarea. Ordinary uniform text keeps the existing input/selection path.

`TextField.autoSize` supports `none`, `left`, `center` and `right` (and the legacy Boolean alias). Rust updates width/height and the affine translation needed to preserve the selected anchor; word-wrapped fields retain their width. Existing text/format writes and native changes trigger reflow. `mouseWheelEnabled` controls real wheel scrolling. Rust converts bounded pixel/line/page deltas, clamps to the current layout, and the existing `onScroller` route fires when the position changes. Wheel normalization is a documented browser profile, not historical OS/Flash delta identity.

Uniform HTML supports anchors with `href`/`target` and font spacing/kerning attributes. Stylesheet transformation supports letter spacing and kerning. TextFormat links use laid-out line hits and the existing browser navigation policy while preserving user activation. Relative HTTP(S) links and supported browser targets work; `asfunction` links remain unsupported. The existing single-format boundary remains: mixed rich runs diagnose rather than being flattened. Original HTML may be retained until a text mutation; normalized HTML is a partial representation, not a full serializer for every paragraph property. No font embedding, complete installed-font enumeration or Flash hinting engine is added.

## MovieClip image methods

`loadMovie`, `unloadMovie`, `getBytesLoaded` and `getBytesTotal` are AS2 MovieClip adapters over the existing MovieClipLoader lifecycle, Rust image decoder and display replacement. They support actual PNG/JPEG image content, embedded resources and permitted HTTP(S) requests, optional GET/POST primitive object variables, cancellation, unload and reload. Placement stays on the existing target object. Progress reports encoded bytes from an actual tracked load, and unload resets those counts.

Root/level replacement and SWF execution are outside the profile. Loading unsupported content produces an actionable diagnostic. Reading byte counts for an authored clip that has never had an image load also diagnoses; the implementation does not invent SWF byte totals. No global loadMovie/loadMovieNum aliases or root replacement are claimed.

## Implementation and provenance

- `crates/neoflash-core/src/runtime_graph/as2_render.rs` owns AS2 render state and validation; `runtime_graph/filters.rs` emits the shared groups.
- `crates/neoflash-core/src/text.rs`, `as2_text.rs` and `runtime_graph/text.rs` own text format, layout, link/wheel interpretation and anchored sizing.
- `crates/neoflash-web/src/as2_display.rs` and `as2_text.rs` carry typed JSON requests/results across WASM.
- `web/display.js`, `web/filters.js`, `web/text.js`, `web/as2-text.js` and `web/text-input.js` marshal references and browser facilities. `web/loader.js` adapts the existing resource lifecycle.
- RuntimeDrawKind's text payload is boxed to keep the enum size bounded after adding format state; serde output is unchanged.

No new external implementation or complete engine is embedded. This profile reuses NeoFlash's already adapted Rust filter and AS2 text components. Existing Ruffle-derived filter algebra, pinned revision and MIT notice are documented in [the filter provenance record](RUNTIME-FILTERS-PROVENANCE.md); the AS2 text reference/adaptation history is in [the text profile](AS2-TEXT-DISPLAY-TWO-HOUR.md). Those notices continue to ship with HTML5 exports. Newly added composition state and paragraph algorithms are integrated into NeoFlash's model rather than copying a foreign runtime. Downloaded references and exploratory artifacts remain ignored under `target/`.

Functional references include the archived [AS2 TextFormat declaration reference](https://open-flash.github.io/mirrors/as2-language-reference/TextFormat.html), [MovieClip reference](https://open-flash.github.io/mirrors/as2-language-reference/MovieClip.html), and the browser's [Canvas letterSpacing](https://developer.mozilla.org/en-US/docs/Web/API/CanvasRenderingContext2D/letterSpacing) and [fontKerning](https://developer.mozilla.org/en-US/docs/Web/API/CanvasRenderingContext2D/fontKerning) contracts.

## Example and verification boundary

[as2-display-filters-batch.nflash](../examples/as2-display-filters-batch.nflash) displays blended/clipped shapes, a cached opaque subtree, filter cards, custom tabs/bullets, anchored text, links and a wheel-scrollable field. Representative browser observations covered PNG load/unload/reload, movement within the cached group, wheel scrolling and a local hyperlink. This does not verify every blend mode, filter or text layout.
