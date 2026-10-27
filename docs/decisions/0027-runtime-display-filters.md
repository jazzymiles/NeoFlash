# Ordered display-object filters

Status: accepted for the bounded HTML5 filter profile, 2026-10-27.

## Whole display subtrees

Filters belong to Rust display-object state. Assigning an array copies the ordered filter values; reading it returns independent values. Filter objects and ColorMatrix arrays likewise clone instead of exposing live state. BlurFilter, GlowFilter, DropShadowFilter and ColorMatrixFilter share BitmapFilter identity, with AS3 imports and the separate AS2 qualified API. Native subclasses and arbitrary filter objects are outside this profile.

A snapshot has a shared filter-group table. Each group names its owning object, nearest filtered ancestor, ordered filter specifications and deferred clip IDs. A draw references its innermost group. The browser rasterizes all the group's own primitives and child contents as one image, processes nested groups first, and applies the group's ordered filters once. Overlapping graphics, text and bitmaps therefore share the same effect. Original geometry remains the input and semantic target; a halo does not create another object or clickable region.

Owner and inherited mask/scroll clipping is deferred until the filtered group is composited. Descendant-only clipping happens within its source image, before the parent's filter. This order follows inspected Ruffle implementation evidence rather than a claimed original Flash differential. Vector mask geometry keeps its existing contract; filtered alpha masks are separate work.

## Shared bounded pixel processing

Rust processes RGBA values with original CPU algorithms. Blur is a repeated separable box filter in premultiplied color; glow/shadow use blurred alpha and their color, strength, inner/knockout and hideObject parameters. ColorMatrix applies twenty coefficients to straight RGBA channels. This is a useful portable profile, not the original Flash subpixel/rounding kernel. The host adds the declared transparent padding and supplies the resulting bounded raster.

Filter radii and shadow displacement use logical Stage pixels, independent of the object's affine transform. The first renderer rasterizes at one logical Stage pixel and scales the cached result with the Stage view; zoom/DPR does not silently change the declared radius. Higher-density text/filter raster fidelity is a separate improvement. Object rotation/skew changes the source shape but not the filter axes.

Limits include eight filters per object, 128 filtered owners, 512 specifications, four million raster pixels, an 8192-pixel side and 128 million channel-pass work units per kernel call. Blur dimensions are finite in 0..255, quality is an integer in 0..15, alpha is 0..1 and strength 0..255. Shadow distance is bounded to ±8192 logical pixels and finite angle to ±1e9 degrees. ColorMatrix coefficients are finite within ±65536. Unsupported or oversized assignments diagnose atomically; these explicit ranges do not emulate every AVM conversion/clamp.

The browser pool is limited to 8 Mi logical pixels (32 MiB RGBA), 16 surfaces and 16 Mi kernel pixels per frame. Group and clip cache keys are bounded to 16 MiB combined; the pool owns cached pixel results. These caches clear with publication lifetime. Filtered native TextField editing is an explicit composition boundary; the runtime must not silently overlay an unfiltered editor on a filtered subtree. Native filter authoring panels, BitmapData.applyFilter, gradient/convolution/displacement/shader filters, cacheAsBitmap semantics, full filter bounds APIs and exact player conformance remain separate work.

One gallery combines four visual styles, nested and sequential filters, copied values, preserved source pixels, both dialects, clear/restore and Restart. Core and compiler checks remain grouped; browser raster evidence is recorded separately from actual-WASM state/kernel checks.

References: AIR [creating and applying filters](https://airsdk.dev/docs/development/display/filtering-display-objects/creating-and-applying-filters), [BlurFilter](https://airsdk.dev/reference/actionscript/3.0/flash/filters/BlurFilter.html), [GlowFilter](https://airsdk.dev/reference/actionscript/3.0/flash/filters/GlowFilter.html), [DropShadowFilter](https://airsdk.dev/reference/actionscript/3.0/flash/filters/DropShadowFilter.html), and [ColorMatrixFilter](https://airsdk.dev/reference/actionscript/3.0/flash/filters/ColorMatrixFilter.html).

Implementation reference: Ruffle commit `fe6cfc545405b888ebb478ce1c211d63147ca49b`, [Stage view scaling](https://github.com/ruffle-rs/ruffle/blob/fe6cfc545405b888ebb478ce1c211d63147ca49b/core/src/display_object.rs#L940) and [subtree filtering followed by owner clipping](https://github.com/ruffle-rs/ruffle/blob/fe6cfc545405b888ebb478ce1c211d63147ca49b/core/src/display_object.rs#L1007). Original Flash was not executed for these reference decisions.
