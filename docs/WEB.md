# Interactive HTML5 projects

NeoFlash publishes a fixed-size authored stage using the shared Rust/WASM timeline engine and a Canvas renderer. Semantic targets describe individual authored buttons and images to browsers. Rich DOM text, inputs, responsive layout and arbitrary web components remain planned.

## Describe an object

Select an object or symbol instance and open the **Accessibility** panel. Enable **Expose this object to assistive technology**, then provide a short meaningful name, an optional description and a role:

- **Button** is an action. It participates in keyboard focus unless disabled. Enter and Space activate its Click handler through the same runtime event path as pointer input.
- **Image** describes visual content. It is available to assistive technology but does not enter the keyboard focus sequence.

Reading/focus order is an authored ordering value. Smaller numbers come first; equal values retain authored display order. The browser uses ordinary sequential DOM focus rather than positive HTML `tabindex` values. Disable a button when its action is unavailable. A disabled button also blocks click activation in its descendants.

Use **Edit Click action** to switch to the selected object's Actions handler. For example:

```js
this.rotation += 15;
trace("Activated once");
```

Names are not inferred from an object's artwork or script. Existing projects have no semantic targets until the author enables them. Frame and object script language support is described in COMPATIBILITY.md.

## Animation and focus

Metadata belongs to a placement and follows its keyframes. Nested instances expose separate targets even when they share a library symbol. Converting an object to a symbol preserves one semantic target on the instance; it does not create a duplicate inside the symbol.

Targets with zero effective opacity or singular transforms are excluded by the runtime. The browser also needs visible display geometry to place a focus indicator. A button keeps its DOM identity while its current placement moves, and its focus outline follows the displayed bounds. Removing and recreating the same object path creates a new lifetime. Delayed activation from an earlier lifetime is rejected, so it cannot operate a replacement object.

Pointer input continues to use Canvas geometry, including path holes and strokes. The semantic DOM layer must not turn the transparent bounding-box corners into clickable artwork.

## Preview scale

Preview viewing controls change only how the fixed authored stage is displayed. **Fit** shows the complete stage; **100%** displays one authored pixel per CSS pixel. At a larger scale, scrolling/panning reveals the rest of the stage while transport controls stay reachable. This is distinct from responsive authoring: choosing a preview zoom does not resize or rearrange source elements.

The scale menu includes 25%, 50%, 75%, 100%, 150%, 200% and 400%. Enable **Pan** to drag the view, use the middle mouse button, or scroll with a trackpad/touch gesture. A keyboard-focused button scrolls into view. Canvas backing resolution is bounded separately from the viewing scale, so very large stages or high zoom levels can lose raster sharpness.

The original example [keyboard-controls.nflash](../examples/keyboard-controls.nflash) contains two independently named counter instances, a described image and a disabled control. Each counter uses an AS3 function and persistent instance state. The visible trace/status region announces script output; the frame counter does not produce a live announcement on every frame.

## Current verification boundary

Core tests cover source persistence, focus ordering, disabled/hidden targets, nested placements and lifetime protection. Native authoring tests cover key insertion, Undo and symbol conversion. Actual browser keyboard, focus and narrow-viewport checks must be recorded separately in VERIFICATION.md. Passing generated-markup tests alone does not establish screen-reader usability. VoiceOver, other assistive technologies, Safari and more semantic roles still require dedicated verification.
