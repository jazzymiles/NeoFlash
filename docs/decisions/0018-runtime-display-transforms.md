# Runtime display matrices and color transforms

Status: accepted first partial profile, 2026-10-27.

## Shared runtime state

Display transforms belong to the Rust runtime graph and stable ObjectRef identities. An exact local affine matrix must survive skew, reflection, detach/reparent and linked-symbol namespaces. Reconstructing it from position/rotation/scale would discard skew. Scalar writes change the stored matrix's columns or translation while retaining the directions needed when a scale crosses zero. Retirement and publication reset retire the associated overrides.

The portable authoring document and pure evaluator remain unchanged. This increment adds script-controlled transforms to HTML5; it does not establish native script execution, an authoring skew tool or new document serialization. Native authoring already has its own affine painter and transform gestures.

## Color and alpha ownership

A runtime color transform contains four multipliers and four offsets. Parent and local transforms compose before a paint is evaluated. The display alpha property addresses the local alpha multiplier; it is not applied a second time through Canvas globalAlpha. Positive alpha offsets must be able to reveal otherwise transparent paint, and visibility/hit/accessibility decisions must use the resulting paint alpha.

Vector and text paint colors are transformed in copied runtime draw payloads. Dynamic Graphics preserves absent fill/stroke separately from an explicitly transparent paint, so an alpha offset cannot invent a fill inside a stroke-only path. BitmapData remains the shared mutable source; colorized display views use read-only transformed RGBA bytes and a separate bounded browser upload cache. Changing one view's tint must not alter its sibling or the source buffer.

## API and evidence boundaries

Transform is a live object-bound facade; Matrix and ColorTransform reads are independent copies which only affect the display object when assigned back. Local and concatenated 2D values share the graph used for coordinate conversion, bounds and rendering. Nonfinite inputs, stale objects and unsupported 3D operations diagnose explicitly. Historical fixed-point rounding, full native slot/coercion behavior and browser-window resizing semantics require separate evidence.

One ordinary project combines nested skew, later scalar edits, shared pixels with independent tints, vector/text color, alpha offsets, linked identity and Restart. Existing compiler/core/host scenarios carry the additional assertions. One shared regression and one browser workflow are the delivery gate; individual members do not each receive their own acceptance cycle.

Primary contracts: [Transform](https://airsdk.dev/reference/actionscript/3.0/flash/geom/Transform.html) and [ColorTransform](https://airsdk.dev/reference/actionscript/3.0/flash/geom/ColorTransform.html). Documentation describes expectations; this is not an original Flash differential result.

The combined ordinary-export/WASM workflow, one shared regression and actual browser Cycle/Restart passed; see [the delivery record](../VERIFICATION.md#runtime-transform-and-colortransform-batch). The bitmap upload cache retains at most 256 variants and an estimated 128 MiB for RGBA bytes plus Canvas pixels, with LRU eviction; browser/GPU bookkeeping is not exactly measurable.
