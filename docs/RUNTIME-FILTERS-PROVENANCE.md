# Rust filter extensions

28 October 2026. This bounded package adds ConvolutionFilter, BevelFilter,
GradientGlowFilter and GradientBevelFilter to both ActionScript dialects through
the existing Rust filter pipeline. BitmapFilterType supplies the three placement
constants. These are rendered filters, not data-only placeholder classes.

## Source and adaptation

Bevel highlight/shadow and inner/outer/full composition follow the algebra in
[Ruffle's bevel shader](https://github.com/ruffle-rs/ruffle/blob/07d3e430631333f80f754df01a968bdcd80d98c5/render/wgpu/shaders/filter/bevel.wgsl),
pinned revision `07d3e430631333f80f754df01a968bdcd80d98c5`. Its MIT license option is
selected. The exact Ruffle copyright and MIT permission notice are already in
`THIRD_PARTY_NOTICES.txt`, which accompanies exported projects.

The shader expressions were understood and rewritten as Rust CPU operations on
NeoFlash's straight-RGBA buffers. Existing bounded separable blur, raster/work
budgets and subtree composition are reused. No shader, GPU backend, Ruffle engine
or external runtime is embedded. Browser code stores copied filter values and
passes specifications to Rust; it contains no convolution, bevel or gradient
pixel algorithms. All downloaded reference material remains under ignored
`target/verification/as-runtime-hour-2/filters/upstream/`.

Convolution is a new Rust implementation of the documented weighted-neighbour
formula. Gradient interpolation and mask mapping are new Rust implementations of
the documented stop/placement profiles, sharing the adapted bevel compositor.
Public contracts were checked against the official reference:

- [ConvolutionFilter](https://airsdk.dev/reference/actionscript/3.0/flash/filters/ConvolutionFilter.html)
- [BevelFilter](https://airsdk.dev/reference/actionscript/3.0/flash/filters/BevelFilter.html)
- [GradientGlowFilter](https://airsdk.dev/reference/actionscript/3.0/flash/filters/GradientGlowFilter.html)
- [GradientBevelFilter](https://airsdk.dev/reference/actionscript/3.0/flash/filters/GradientBevelFilter.html)
- [BitmapFilterType](https://airsdk.dev/reference/actionscript/3.0/flash/filters/BitmapFilterType.html)

## Delivered profile and counts

| Shared class | New properties | New methods |
| --- | ---: | ---: |
| ConvolutionFilter | 9 | 1 (`clone`) |
| BevelFilter | 12 | 1 (`clone`) |
| GradientGlowFilter | 11 | 1 (`clone`) |
| GradientBevelFilter | 11 | 1 (`clone`) |
| **Total** | **43** | **4** |

This is **47 new shared member paths**, plus four constructors and three
BitmapFilterType constants counted separately. Both AS2 and AS3 expose those
same implementations; they are not 94 independent implementations. No existing
member is relabeled as a migration and no language construct is added.

Convolution includes preserved or filtered alpha, edge clamp or replacement
color/alpha, bias, divisor and detached matrix copies. Zero divisor uses one.
Bevel includes direction, highlight/shadow colors and alpha, blur, strength,
placement and knockout. Gradient effects add matching copied color/alpha/ratio
arrays, linear stop interpolation, direction and the same composition options.
Gradient bevel places the base fill at ratio 128, highlight below and shadow
above it. Every concrete class clones its values. Display filter arrays preserve
existing copy/atomic-assignment behavior; BitmapData.applyFilter and
 generateFilterRect use the same Rust kernel.

## Explicit partial boundaries

- This retains NeoFlash's logical-pixel, repeated box-blur backend. It does not
  promise original Flash Gaussian/subpixel/rounding, color space or stage-density
  identity. Historical AVM coercion and native subclassing remain incomplete.
- Convolution dimensions are integral 0..15, matrix length must equal their
  product when applied, and coefficients/divisor/bias are finite within
  +/-65536. A zero dimension with an empty matrix is a no-op. Automatic historical
  coefficient-array resizing/truncation on property mutation is not implemented;
  inconsistent dimensions fail before display or bitmap mutation.
- Gradients accept 0..16 entries with equal array lengths and nondecreasing
  integral ratios 0..255; empty gradients are no-ops. Invalid combined state
  rejects at application. Unequal arrays are not silently truncated.
- Bevel/gradient effects retain the existing requirement for a transparent
  BitmapData destination. Display-object composition is supported normally.
- Existing eight-filter chains, raster, pool and work budgets apply. Effects do
  not add hit targets, and cacheAsBitmap/native filter authoring are not added.
- DisplacementMapFilter, ShaderFilter and authoring UI remain outside this batch.

## Representative evidence

One grouped Rust scenario, `filters::tests::filter_extension_family`, covers
convolution channels/edges, preserved input, bevel light/shadow/knockout, both
gradient kernels, padding and invalid-state rejection. The ordinary project
`examples/runtime-filter-extensions.nflash` uses all four classes in authored
AS3 and AS2, reads/writes all 43 properties in one family loop, clones each class,
checks array and display copy isolation, exercises real BitmapData pixels and
cycles visible styles through alternate/clear/restore. Its browser execution and
Restart are recorded by the coordinator's delivery report; creating this
example alone is not a browser pass.
