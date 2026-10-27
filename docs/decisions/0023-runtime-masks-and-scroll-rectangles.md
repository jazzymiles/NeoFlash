# Runtime vector masks and scroll rectangles

Status: accepted for the bounded runtime profile, 2026-10-27.

## Clipping belongs to the display graph

Masks refer to live object identities, not instance names or authored placement paths. Runtime state owns mask assignment, single-owner reuse, copied scroll rectangles and cleanup. Each snapshot carries a shared clip table; draws refer to clip IDs instead of copying complete mask geometry for every descendant. Components inside one vector mask form a union, while a target's inherited clip chain forms an intersection.

Vector masks use filled geometry, including transparent fills and compound holes. Paint color, opacity and strokes do not define the uncached vector mask. The source subtree is consumed as mask geometry and does not independently draw or receive input. Dynamic vector sources and static supported vector subtrees form the initial profile. Bitmap/text mask sources, alpha masks/cacheAsBitmap, animated authored source timelines, masks of masks, overlapping source subtrees and scroll rectangles inside mask sources require separate implementation. Invalid graph changes diagnose before mutation. The profile caps active mask/scroll entries at 256 and inherited clip chains at 32. Authored mask geometry reserves at most 100,000 commands in addition to the existing shared dynamic Graphics budget.

Scroll rectangles are copied on write and read. The first cross-dialect profile accepts integral finite x/y/width/height with nonnegative sizes, and applies changes immediately. Fractional and historical deferred-rounding behavior is explicitly outside this profile. A base transform stays unchanged; content coordinates include the negative scroll offset. Viewport geometry uses the base transform and intersects ancestor clips, so pixels, coordinate conversion and bounds share the same model.

AS3/JavaScript absence is null. The AS2 compiler maps an absent native scrollRect to undefined without changing local variables or ordinary object fields. AS2 setMask implements the observed Boolean-success behavior for direct object/null assignment; the old documentation's Void signature and unsupported String target paths remain distinguished.

## One renderer and one interaction boundary

The browser composites mixed-rule mask components using shared Path2D geometry and two reusable bounded surfaces, then intersects nested clips. Pointer routing tests each clip union with the same paths. Semantic targets use visible bounds from the already clipped render, with bounded tiled alpha readback only for leaves that contribute to authored semantics. Fully clipped targets and mask holes cannot produce independent ghost buttons. Raster antialiasing and full accessibility conformance remain separate evidence.

The new clipped-scene workflow combines five linked cards in a scrolling viewport, a linked panel clipped by a compound/circular mask and its parent viewport, and AS2 setMask/scrollRect. One advancement moves the views/mask, another removes masks, and Restart recreates all instances. Extend the existing shared test runner; avoid separate acceptance cycles per setter.

Primary references: [AIR DisplayObject](https://airsdk.dev/reference/actionscript/3.0/flash/display/DisplayObject.html), [masking display objects](https://airsdk.dev/docs/development/display/display-programming/manipulating-display-objects/masking-display-objects), and the separately identified [pinned AVM1 setMask implementation](https://github.com/ruffle-rs/ruffle/blob/fe6cfc545405b888ebb478ce1c211d63147ca49b/core/src/avm1/globals/movie_clip.rs#L1144). Original-player comparison is not inferred from these references.
