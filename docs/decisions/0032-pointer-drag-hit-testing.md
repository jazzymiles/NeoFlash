# Pointer targeting, drag state and script hit tests

Status: accepted for the bounded 2D pointer profile, 2026-10-27.

## Ownership and coordinate spaces

Input flags and hit-area references belong to the existing Rust display identity and publication lifetime. The browser owns native pointer routing and one active drag operation. It must not introduce a second scene graph. Local mouse coordinates and parent-space drag movement use the current composed transforms, including nested transformed containers. A supplied drag rectangle is copied; script mutation of that value does not change the active constraints. The profile uses strict booleans, finite coordinates within ±1e12 and nonnegative rectangle sizes. lockCenter uses the registration point; the ordinary mode retains its parent-space grab offset.

AS3 startDrag remains active until explicitly stopped or replaced by another drag. A mouse release is delivered to script, which can call stopDrag; releasing a button is not itself permission to change that API contract. Pointer capture/outside release, cancellation, detached/removed objects, Restart and publication failure need explicit cleanup. The first profile cancels active dragging on reparenting, hiding, detachment/removal, pointer cancellation, window blur and Restart. Single primary mouse/pen pointer and two-dimensional geometry form the first profile; multi-touch and 3D remain separate work. Reparent cancellation and fixed grab offsets are bounded host policies, not claims of every historical player behavior.

## Targeting and geometry

Input picking and script hit tests have different contracts. mouseEnabled/mouseChildren affect event routing, while hitTestObject tests intersecting bounds and point queries use Stage coordinates. A supported precise query must inspect geometry/pixels, not silently replace shapeFlag=true with a bounding box. Shape queries use vector fill/stroke geometry, TextField layout boxes, source bitmap alpha and a decoded-video rectangle. Display ColorTransform/alpha and input flags do not replace source geometry. Filter expansion is not hit geometry. Transparent fill versus absent fill, exact glyph/antialias coverage and all mask/scroll combinations remain partial.

A Sprite hitArea can refer to an invisible sprite with its own transform. The donor does not disappear from the display model merely because another sprite references it; it can still intercept input unless its mouseEnabled is false. This differs from mask ownership. Input flags apply to native Canvas clicks, semantic activation and hover/cursor routing. The shared pick result carries two identities: an eligible authored owner for authored Click scripts, and the resolved InteractiveObject for AS3 events. A noninteractive leaf retains its authored path when its nearest interactive ancestor is the selected target; mouseChildren/disabled retargeting suppresses the old branch. Native text events forwarded directly and then bubbled to the window are deduplicated per DOM event/type.

AS2 has its own callback and overload profile: onPress, onRelease/onReleaseOutside, rollover callbacks, startDrag numeric bounds, hitTest and _xmouse/_ymouse. The supported _droptarget slash path requires named placements without slash/dot in names. Legacy enabled reports an explicit diagnostic; trackAsMenu and complete button/key behavior remain pending. Do not infer historical target-path/coercion semantics from JavaScript.

## Delivery verification

The grouped actual-WASM workflow and real browser gestures passed. Browser evidence covers transformed dragging, invisible hit-area margin, parent/child/disabled targeting, outside release and a fresh drag after Restart. Restart while captured and cursor values retain controlled-host evidence. One shared semantic-activation regression and cursor cleanup were repaired during delivery; verification provenance is recorded [separately](../VERIFICATION.md#pointer-drag-and-targeting-workflow).

Extend existing grouped core/compiler checks and use one ordinary-export/real-WASM workflow. One example should connect both dialects, transformed-parent dragging, bounds, a drop target, disabled/grouped child input and Restart. Only actual browser pointer interaction proves capture/routing and rendered motion; a controlled adapter cannot establish native input fidelity. Run one shared delivery pass; do not create separate browser cycles for every method.

References: Adobe/AIR [Sprite](https://airsdk.dev/reference/actionscript/3.0/flash/display/Sprite.html), [DisplayObject](https://airsdk.dev/reference/actionscript/3.0/flash/display/DisplayObject.html), and Adobe [AS2 MovieClip reference mirror](https://open-flash.github.io/mirrors/as2-language-reference/MovieClip.html). Original-player and cross-browser equivalence require separate evidence.
