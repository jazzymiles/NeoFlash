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

Pointer input continues to use Canvas geometry, including path holes and strokes. Runtime mask unions and inherited scroll rectangles also constrain picking. Semantic bounds come from the already clipped artwork; fully clipped targets are omitted. The semantic DOM layer must not turn the transparent bounding-box corners into clickable artwork.

## Preview scale

Preview viewing controls change only how the fixed authored stage is displayed. **Fit** shows the complete stage; **100%** displays one authored pixel per CSS pixel. At a larger scale, scrolling/panning reveals the rest of the stage while transport controls stay reachable. This is distinct from responsive authoring: choosing a preview zoom does not resize or rearrange source elements.

The scale menu includes 25%, 50%, 75%, 100%, 150%, 200% and 400%. Enable **Pan** to drag the view, use the middle mouse button, or scroll with a trackpad/touch gesture. A keyboard-focused button scrolls into view. Canvas backing resolution is bounded separately from the viewing scale, so very large stages or high zoom levels can lose raster sharpness.

The original example [keyboard-controls.nflash](../examples/keyboard-controls.nflash) contains two independently named counter instances, a described image and a disabled control. Each counter uses an AS3 function and persistent instance state. The visible trace/status region announces script output; the frame counter does not produce a live announcement on every frame.

## Current verification boundary

Core tests cover source persistence, focus ordering, disabled/hidden targets, nested placements and lifetime protection. Native authoring tests cover key insertion, Undo and symbol conversion. Actual browser keyboard, focus and narrow-viewport checks must be recorded separately in VERIFICATION.md. Passing generated-markup tests alone does not establish screen-reader usability. VoiceOver, other assistive technologies, Safari and more semantic roles still require dedicated verification.

## Resource files and data requests

Use Library → Resource files to import portable bytes, then load their exact project path from AS3 URLLoader or AS2 LoadVars. The exported bundle contains these files and can serve them without network access, including when opening its index.html directly. Text, URL-encoded variables and ByteArray data use the shared publication services. Resource imports and metadata/removal participate in Undo; renaming a logical path requires updating the script that uses it.

Requests for external HTTP(S) URLs use browser access rules and require a reachable server. A local relative file that was not imported is not an implicit desktop-file request. Restart and cancellation suppress callbacks from abandoned operations. The compatibility matrix documents supported methods, fields, limits and error events. PNG/JPEG resources can be placed on Stage with **Place image**. BitmapData supports bounded pixel edits, clipped copies, cloning and scrolling; several Bitmap instances can share one live buffer. Restart rebuilds the runtime buffers from immutable project resources. Other image formats, general Loader, sound/video playback and font embedding remain separate work. See [bitmap-pixels.nflash](../examples/bitmap-pixels.nflash) for one combined image workflow.

## Text fields and formatting

AS3 can create a TextField and add it to a container; AS2 can use createTextField at the next supported depth. Set a logical width/height, multiline/wordWrap and a uniform TextFormat. Current formatting and insertion defaults are separate; whole-text assignment applies defaults, while unsupported mixed-format insertions fail before changing the content. The native Text tool produces authored TextBox elements with the same shared line calculation.

Rust computes wrapping, UTF-16 line ranges and left/center/right positions from batches of native or Canvas font measurements. Drawing, bounds, hit regions and supported metrics reuse the resulting layout. The original [text-layout.nflash](../examples/text-layout.nflash) contains independent AS2/AS3 cards and a control that changes the first card’s width/alignment. This is Canvas text with explicit boxes; DOM forms, selection/input/IME, rich HTML and embedded fonts remain separate capabilities.

## Scripted audio

AS3 Sound loads a portable resource path or an HTTP(S) URL through URLRequest; AS2 uses Sound.loadSound(path, false). This first profile decodes complete MP3 or mono/stereo PCM/float WAV files before playback. Channels have position, stop, repeats and copied SoundTransform volume/pan values. AS2 exposes start/stop, volume/pan, onLoad and onSoundComplete. See examples/sound-playback.nflash for both dialects sharing one original MP3 resource.

A real click on an authored Play control can enable audio. If a script requests playback before a browser gesture, the preview exposes **Enable audio** and queues the channel. The same control then mutes or unmutes the publication. Restart stops its channels, cancels loading and creates fresh script owners. The public example deliberately starts at zero volume; change its volume variable when authoring an audible project.

Compressed input keeps the resource service's 16 MiB limit. Frame/chunk inspection reserves decoded float PCM at the actual output sample rate before native decoding; the publication permits up to 128 MiB of live/reserved PCM, 32 active/pending channels and 256 Sound objects before Restart. Native decoder working memory remains browser-owned. Unsupported containers, free-format MP3 and changing channel layouts produce a loading error. Streaming, ID3 access, sample generation/extraction, exact historical mixing matrices, sound linkage, audio tracks and timeline synchronization remain planned. This browser playback service does not add sound to side-effect-free editor scrubbing.

## Data processing in ActionScript

The data-processing example loads an embedded JSON planning file through URLLoader, uses AS3 JSON/RegExp/Date to build the interface, and serializes an ordinary project class including a public getter. Its independent AS2 Date card advances in UTC with the same control. Restart returns to the original records and dates. Native JSON and RegExp are AS3 APIs; Date is available in both dialects.

These families use captured browser-backed adapters. Regex patterns currently use new RegExp(pattern, flags); slash literals remain unsupported. Date timezone/formatting follows the modern backend profile. JSON includes parse/stringify callbacks and bounded class serialization; it does not automatically serialize every host object. See COMPATIBILITY.md for precise supported forms and limits.

### Runtime display transforms

`examples/display-transforms.nflash` combines a nested linked card, a skewed matrix, scalar rotation/scale updates, vector/text color and two independent colorized views of shared BitmapData. Its Cycle control reparents the same card and returns it to the rig; Restart restores original objects and transforms. An AS2 strip uses the same geometry family. The runtime profile and remaining window-coordinate/native-authoring boundaries are documented in [the decision](decisions/0018-runtime-display-transforms.md).

### Typed collections

`examples/typed-collections.nflash` connects AS3 Vector models, Dictionary keys that are live card instances, for-in/for-each, deletion/membership and JSON serialization. Process batch updates stock and reuses the shipped card for an incoming model; construction stays at three linked instances. An independent AS2 for-in readout totals the original named properties. Restart restores the original models, keys and cards.

## XML catalogs

[xml-catalog.nflash](../examples/xml-catalog.nflash) loads one portable XML resource through AS3 URLLoader and AS2 XML.load. The AS3 side uses E4X selectors, edits target lists, inserts XML/XMLList literals and serializes a new tree; the AS2 side clones, moves and removes DOM nodes. Both retain namespace declarations and decoded entity text. Update both catalogs and Restart exercise publication lifetime. Filters, scoped default XML namespace and complete historical XML/E4X semantics remain pending.
