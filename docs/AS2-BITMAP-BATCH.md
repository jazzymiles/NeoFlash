# AS2 bitmap and displacement batch

This two-hour-cycle package adds real pixel operations in the existing Rust arena. Only AS2 call routes are enabled. The AS3 BitmapData stubs and AS3 filter catalogue are unchanged. No UI, FLA conversion or complete third-party engine is introduced.

## Delivered scope and counting

Twelve previously absent AS2 BitmapData methods now have working partial implementations: `compare`, `copyChannel`, `floodFill`, `getColorBoundsRect`, `hitTest`, `loadBitmap`, `merge`, `noise`, `paletteMap`, `perlinNoise`, `pixelDissolve`, and `threshold`.

AS2 DisplacementMapFilter adds its constructor, nine properties (`alpha`, `color`, `componentX`, `componentY`, `mapBitmap`, `mapPoint`, `mode`, `scaleX`, `scaleY`) and `clone`. It produces an actual pixel effect through BitmapData `applyFilter`; it is not a data-only placeholder. Combined new-declaration count: **23** (13 methods, nine properties, one constructor).

Three existing methods expand: `copyPixels` accepts an alpha bitmap/point and merge-alpha; `applyFilter` handles AS2 displacement; `generateFilterRect` handles displacement bounds. Four other catalogue entries already worked and are reviewed separately: `dispose`, `getPixel`, `scroll`, `setPixel`. No code written in an earlier cycle is credited as new implementation. `colorTransform` was already implemented and partial; it is exercised without claiming another addition.

There are no AS2 BitmapDataChannel or DisplacementMapFilterMode classes in the reference catalogue. The AS2 API uses channel numbers 1/2/4/8 and mode strings; this delivery does not invent AS3 constants as AS2 coverage.

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

## Verification and ordinary example

Two grouped native tests cover the pixel family and displacement family. The first ran initially, then once more when alpha-mask and Perlin behavior were added; no per-method harnesses were created. Both passed. JavaScript syntax checks passed. `examples/as2-bitmap-batch.nflash` is an ordinary AS2 project with ten visible panels, a combined status row, bundled-image loading and a visible displacement-mode control. It exercises the families together and exposes limits in its own stage text.

The targeted ordinary export to `dist/as2-bitmap-batch/` is the compiler/schema check. The coordinator owns the shared WASM build and browser acceptance. An export alone is not recorded as a browser pass; final evidence is added by the delivery report.

## Provenance and licenses

Behavior contracts were checked against Adobe's [AS2 BitmapData reference](https://open-flash.github.io/mirrors/as2-language-reference/flash/display/BitmapData.html) and [AS2 DisplacementMapFilter reference](https://open-flash.github.io/mirrors/as2-language-reference/flash/filters/DisplacementMapFilter.html).

The following Ruffle sources were inspected and their algorithms adapted at immutable revision [`07d3e430631333f80f754df01a968bdcd80d98c5`](https://github.com/ruffle-rs/ruffle/tree/07d3e430631333f80f754df01a968bdcd80d98c5):

- `core/src/bitmap/operations.rs`: channel/palette/merge/difference and dissolve behavior.
- `core/src/bitmap/bitmap_data.rs`: Park-Miller random sequence.
- `render/wgpu/shaders/filter/displacement_map.wgsl`: channel displacement and edge-mode algebra, rewritten as a bounded Rust CPU operation.
- `core/src/bitmap/turbulence.rs`: W3C SVG turbulence port, used to design the smaller bounded gradient-noise implementation.

Ruffle's MIT option is selected; its existing full notice remains in `THIRD_PARTY_NOTICES.txt`. The turbulence material retains the W3C 2015 software/document notice, copyright and modification statement, appended to that same exported notice file. It is not relabeled as wholly original. Downloaded references and exploratory files remain ignored under `target/references/as2-two-hour/` and `target/verification/as2-two-hour/`.

The delivery browser pass exposed a sample predicate collision: pixels (0,0) and (30,20) both quantize to `0x808080` for the chosen noise seed. One focused check through the shipped Rust WASM confirmed deterministic equality of two complete generated images, 172 distinct colors and a full-size nonmatching color-bounds rectangle. The sample now checks the field-wide nonmatching bounds rather than assuming two individual pixels differ. The 14 grouped predicates and native test count are unchanged.
