# AS2 bitmap operations and displacement

AS2 BitmapData uses the shared Rust pixel arena for pixel operations and displacement filtering. The boundaries below describe the AS2 facade; they do not automatically extend the AS3 facade.

## Supported operations

AS2 BitmapData supports bounded `compare`, `copyChannel`, `floodFill`, `getColorBoundsRect`, `hitTest`, `loadBitmap`, `merge`, `noise`, `paletteMap`, `perlinNoise`, `pixelDissolve` and `threshold`.

AS2 DisplacementMapFilter provides `alpha`, `color`, `componentX`, `componentY`, `mapBitmap`, `mapPoint`, `mode`, `scaleX`, `scaleY` and `clone`. It produces a pixel effect through BitmapData `applyFilter`.

`copyPixels` accepts an alpha bitmap/point and merge-alpha; `applyFilter` and `generateFilterRect` support the bounded AS2 displacement profile.

AS2 uses channel numbers 1/2/4/8 and mode strings, without the AS3 BitmapDataChannel or DisplacementMapFilterMode classes.

## Architecture and reusable work

`pixels/as2.rs` owns clipped transfers, channel/mask algebra, connected fill, comparisons, threshold counting, alpha hit tests, noise/dissolve, displacement and typed arena references. `pixels/as2_perlin.rs` owns bounded seeded gradient noise. Pixel buffers, premultiplication, revisions, disposal, resource decoding and rendering cache are the existing shared infrastructure. Browser code marshals values/references and installs AS2 method variants; it performs no new pixel algorithms.

The resource loader resolves an **exact bundled image resource ID** to a fresh decoded pixel buffer. Missing IDs return null. Each load has independent pixels. This is the current document library mapping; historical SWF/FLA export-linkage tables are not claimed.

## Partial profile and boundaries

- Shared arena limits remain: 4096 per dimension, 256 records, 128 MiB live pixels. The existing nearest-integer premultiplication/unpremultiplication profile applies, including rounding at low alpha. Invalid/disposed access diagnoses instead of reproducing every historical AS2 sentinel return value.
- Coordinates and rectangles require signed integers; colors require unsigned ARGB. Region transfers clip source and destination, and read an immutable clipped source snapshot before mutation. This deliberately avoids overlap feedback rather than reproducing historical self-overlap traversal quirks.
- `copyChannel` accepts channel values 1,2,4,8. Opaque targets remain opaque. `merge` clamps channel weights to 0..256. `paletteMap` requires null/identity or a dense 256-entry uint table per channel; table sums wrap at 32 bits.
- `threshold` implements the six documented comparison strings using masked unsigned values. The return count includes matching pixels, not `copySource` fallbacks. Opaque targets retain their alpha invariant rather than historical opacity bugs.
- `compare` returns zero for equality, -3/-4 for width/height mismatch, or a separate difference bitmap. RGB deltas wrap per byte; an alpha-only delta yields white RGB with the alpha delta. Allocation remains subject to the arena budget.
- `hitTest` supports points, rectangles and BitmapData with supplied origins/alpha thresholds. It does not add shape or transformed-display hit testing. Point threshold zero is treated as one; rectangle/bitmap thresholds retain the bounded raw-alpha comparison profile.
- `floodFill` compares premultiplied arena pixels and uses four-connected fill. The bounded u32 queue marks a pixel when enqueued, preventing repeated queue entries.
- `noise` is deterministic Park-Miller noise with inclusive low/high bounds, selected channels and grayscale. `perlinNoise` supports 0..16 octaves, channel selection, grayscale, per-octave offsets, fractal/turbulence output and periodic stitching. It caps work at 64 Mi-samples. Nonzero bases range from 1/1024 through 1,000,000; offsets are finite within +/-1,000,000. Missing offsets default to zero. Compact lattice wrapping and negative-coordinate handling are an explicit profile, not a claim of exact Flash noise pixels.
- `pixelDissolve` uses the adapted bounded Feistel permutation and a returned continuation seed. It preserves the upstream observed first-pixel write, including a requested count of zero; it does not claim the documentation's historical 1/30 default or exact original-player permutation. Negative counts diagnose.
- Alpha-mask `copyPixels` combines source/mask alpha and optionally composites over the destination; mask samples outside the mask are transparent. Opaque targets composite and stay opaque. AS3 alpha-mask support is not enabled by this change.
- Displacement supports all four edge modes (`wrap`, `clamp`, `ignore`, `color`), integer map points, map channel selection, finite scales within +/-8192, color/alpha and independent filter-value clones. A null/outside map gives zero displacement. Sampling uses integer source pixels and is limited to 4 Mi-pixel work. BitmapData filtering is supported; **DisplayObject.filters explicitly rejects this class** until live map ownership/snapshots are integrated. No stage-scale/subpixel identity is claimed.
- Static `loadBitmap` resolves bundled PNG/JPEG resources by ID; it does not fetch network images, open local paths, or parse FLA/SWF linkage. Nonimage resources diagnose.

## Example

[as2-bitmap-batch.nflash](../examples/as2-bitmap-batch.nflash) combines pixel operations, bundled-image loading and visible displacement-mode controls. Its stage text describes the demonstrated limits.

## Provenance and licenses

Behavior contracts were checked against Adobe's [AS2 BitmapData reference](https://open-flash.github.io/mirrors/as2-language-reference/flash/display/BitmapData.html) and [AS2 DisplacementMapFilter reference](https://open-flash.github.io/mirrors/as2-language-reference/flash/filters/DisplacementMapFilter.html).

The following Ruffle sources were inspected and their algorithms adapted at immutable revision [`07d3e430631333f80f754df01a968bdcd80d98c5`](https://github.com/ruffle-rs/ruffle/tree/07d3e430631333f80f754df01a968bdcd80d98c5):

- `core/src/bitmap/operations.rs`: channel/palette/merge/difference and dissolve behavior.
- `core/src/bitmap/bitmap_data.rs`: Park-Miller random sequence.
- `render/wgpu/shaders/filter/displacement_map.wgsl`: channel displacement and edge-mode algebra, rewritten as a bounded Rust CPU operation.
- `core/src/bitmap/turbulence.rs`: W3C SVG turbulence port, used to design the smaller bounded gradient-noise implementation.

Ruffle's MIT option is selected; its existing full notice remains in `THIRD_PARTY_NOTICES.txt`. The turbulence material retains the W3C 2015 software/document notice, copyright and modification statement, appended to that same exported notice file. It is not relabeled as wholly original. Downloaded references and exploratory files remain ignored under `target/references/as2-two-hour/` and `target/verification/as2-two-hour/`.
