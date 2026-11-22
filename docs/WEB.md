# Interactive HTML5 projects

NeoFlash publishes an authored stage using the shared Rust/WASM timeline engine with Canvas or Web App · DOM rendering. Both modes expose placement metadata to browser accessibility APIs; DOM mode also provides native text and form controls. See the [compatibility matrix](COMPATIBILITY.md) for layout and rendering limits.

## Custom HTML publish profiles

A publish profile can supply an HTML wrapper. Place `$TITLE` in the title element and `$PROJECT_JSON` inside a `<script type="application/json">` element. NeoFlash expands tokens only where they appear in the template. The published title and decoded JSON preserve authored text, including literal `$WIDTH` and `</script>`.

The export CLI and the headless editor retain the profile wrapper when you add an assets folder or pack a sprite sheet. Its script URLs and embedded project JSON follow the published files. Use `--clean` for the generated player page. An explicit `--render-mode canvas` also selects the generated Canvas page, including for projects saved in DOM mode.

## Describe an object

Select an object or symbol instance and open the **Accessibility** panel. Enable **Expose this object to assistive technology**, then provide a short meaningful name, an optional description and a role:

- **Button** is an action. It participates in keyboard focus unless disabled. Enter and Space activate its Click handler through the same runtime event path as pointer input.
- **Image** describes visual content. It is available to assistive technology but does not enter the keyboard focus sequence.

Keyboard order follows positive authored `tabIndex` values, with stable order for ties. Canvas semantic reading order additionally follows the authored reading-order field; DOM browse order follows the rendered document hierarchy. `tabEnabled=false` removes a control from Tab traversal without disabling pointer activation. Input TextFields, SimpleButtons, button-mode clips and authored semantic buttons participate by default. Explicit Tab settings survive unrelated property changes. Disable a button when its action is unavailable; disabled buttons also block click activation in their descendants.

Use **Edit Click action** to switch to the selected object's Actions handler. For example:

```js
this.rotation += 15;
trace("Activated once");
```

Names are not inferred from artwork. Text can supply an automatic label from its current runtime content; explicit names override it. Authored role, description, shortcut and live-region metadata share one target with the runtime placement in both publication modes. Clearing metadata removes its old browser attributes. Silent containers hide descendant semantics and Tab stops; simple containers expose their own semantics while suppressing descendants. Frame and object script language support is described in COMPATIBILITY.md.

## Animation and focus

Metadata belongs to a placement and follows its keyframes. Nested instances expose separate targets even when they share a library symbol. Converting an object to a symbol preserves one semantic target on the instance; it does not create a duplicate inside the symbol.

Targets with zero effective opacity or singular transforms are excluded by the runtime. The browser also needs visible display geometry to place a focus indicator. A button keeps its DOM identity while its current placement moves, and its focus outline follows the displayed bounds. Native control focus and programmatic `Stage.focus` use the same focus-event path. Removing and recreating the same object path creates a new lifetime. Delayed activation from an earlier lifetime is rejected, so it cannot operate a replacement object.

Pointer input continues to use Canvas geometry, including path holes and strokes. Runtime mask unions and inherited scroll rectangles also constrain picking. Semantic bounds come from the already clipped artwork; fully clipped targets are omitted. The semantic DOM layer must not turn the transparent bounding-box corners into clickable artwork.

## Preview scale

While a button or artwork is held down, automatic stage resizing waits until the pointer is released or the gesture is cancelled. This keeps the first click on target when unlocking audio changes the height of the preview controls.

Preview viewing controls change only how the fixed authored stage is displayed. **Fit** shows the complete stage; **100%** displays one authored pixel per CSS pixel. At a larger scale, scrolling/panning reveals the rest of the stage while transport controls stay reachable. This is distinct from responsive authoring: choosing a preview zoom does not resize or rearrange source elements.

The scale menu includes 25%, 50%, 75%, 100%, 150%, 200% and 400%. Enable **Pan** to drag the view, use the middle mouse button, or scroll with a trackpad/touch gesture. A keyboard-focused button scrolls into view. Canvas backing resolution is bounded separately from the viewing scale, so very large stages or high zoom levels can lose raster sharpness.

The original example [keyboard-controls.nflash](../examples/keyboard-controls.nflash) contains two independently named counter instances, a described image and a disabled control. Each counter uses an AS3 function and persistent instance state. The visible trace/status region announces script output; the frame counter does not produce a live announcement on every frame.

## Current verification boundary

Core tests cover source persistence, focus ordering, disabled/hidden targets, nested placements and lifetime protection. Native authoring tests cover key insertion, Undo and symbol conversion. The shared browser family checks live accessibility-tree metadata, Tab/Enter, focus events, disabled/hidden controls, clearing metadata and retired controls in Canvas and DOM. Passing these checks does not establish spoken screen-reader usability. VoiceOver, Safari, separate DOM browse ordering and semantic coverage of raster islands still need dedicated work. Custom `AccessibilityImplementation` currently supplies only placement name/value through child ID 0; custom child trees, roles, states, selection and default actions are not bridged.

## Resource files and data requests

Use Library → Resource files to import portable bytes, then load their exact project path from AS3 URLLoader or AS2 LoadVars. The exported bundle contains these files and can serve them without network access, including when opening its index.html directly. Text, URL-encoded variables and ByteArray data use the shared publication services. Resource imports and metadata/removal participate in Undo; renaming a logical path requires updating the script that uses it.

Requests for external HTTP(S) URLs use browser access rules and require a reachable server. A local relative file that was not imported is not an implicit desktop-file request. Restart and cancellation suppress callbacks from abandoned operations. The compatibility matrix documents supported methods, fields, limits and error events. PNG/JPEG resources can be placed on Stage with **Place image**. BitmapData supports bounded pixel edits, clipped copies, cloning and scrolling; several Bitmap instances can share one live buffer. Finder-imported embedded video spans publish with their original bytes and play through native browser MP4/WebM/Ogg decoding; visual playback starts browser-muted until activation. Stream spans follow play/pause/scrub and retire at span exit, Event spans continue independently until a matching Stop, and Restart clears every authored video channel. Embedded, local External and URL-backed Progressive streams can also decode on the native Stage through the bounded on-demand frame ring. Progressive downloads run outside the UI thread with size, timeout, redirect and short-lived cache limits; automatic document requests reject private, loopback, link-local and reserved destinations unless the project/server is explicitly trusted. Text fields marked **Embed font** use exact stored Family/Style metadata and an embedded `@font-face`; Full Embed carries the complete face, while TrueType Subset emits the required glyph closure and conservatively keeps a full face for dynamic/input text or unsupported font layouts. Restart rebuilds these services from immutable project resources. Other image formats and browser codec support remain separate limits. See [bitmap-pixels.nflash](../examples/bitmap-pixels.nflash) for one combined image workflow.

## Text fields and formatting

AS3 can create a TextField and add it to a container; AS2 can use createTextField at the next supported depth. Set a logical width/height, multiline/wordWrap and TextFormat values over bounded UTF-16 ranges. Current formatting and insertion defaults are separate; whole-text assignment applies defaults. The native Text tool produces authored TextBox elements with the same shared line calculation and supports inline UTF-16 caret/range editing, mixed-run character formatting and paragraph-range updates.

Rust computes wrapping, UTF-16 line ranges and left/center/right positions from batches of native or Canvas font measurements. Drawing, bounds, hit regions and supported metrics reuse the resulting layout. The original [text-layout.nflash](../examples/text-layout.nflash) contains independent AS2/AS3 cards and a control that changes the first card’s width/alignment. This is Canvas text with explicit boxes; scripted selection/input, rich HTML and embedded fonts have bounded profiles, while DOM forms and complete IME/grapheme conformance remain separate capabilities.

## Scripted audio

AS3 Sound loads a portable resource path or an HTTP(S) URL through URLRequest; AS2 uses Sound.loadSound(path, false). This first profile decodes complete MP3 or mono/stereo PCM/float WAV files before playback. Channels have position, stop, repeats and copied SoundTransform volume/pan values. AS2 exposes start/stop, volume/pan, onLoad and onSoundComplete. See examples/sound-playback.nflash for both dialects sharing one original MP3 resource.

A real click on an authored Play control can enable audio. If a script requests playback before a browser gesture, the preview exposes **Enable audio** and queues the channel. The same control then mutes or unmutes the publication. Restart stops its channels, cancels loading and creates fresh script owners. The public example deliberately starts at zero volume; change its volume variable when authoring an audible project.

Compressed input keeps the resource service's 16 MiB limit. Frame/chunk inspection reserves decoded float PCM at the actual output sample rate before native decoding; the publication permits up to 128 MiB of live/reserved PCM, 32 active/pending channels and 256 Sound objects before Restart. Native decoder working memory remains browser-owned. Unsupported containers, free-format MP3 and changing channel layouts produce a loading error. Streaming, ID3 access, sample generation/extraction, exact historical mixing matrices and sound linkage remain planned. Scrubbing in the editor stays silent.

## Timeline sounds

Keyframes can carry a library sound with Flash sync: **Event** plays on every entry and overlaps, **Start** does not overlap itself, **Stop** stops the sound and **Stream** follows the playhead — it starts at the right offset after a jump, restarts when the timeline loops and stops at the end of its span or when that timeline stops. Repeat counts, Loop and the preset effects (left/right channel, fades) apply. The Rust player decides what plays; the page only decodes each resource once and schedules it with Web Audio, so editor playback and HTML5 agree. Before the first gesture the preview shows **Enable audio**: Event/Start sounds are skipped until then and streams resume at their timeline position. AS2 `stopAllSounds()` and AS3 `SoundMixer.stopAll()` also stop timeline sounds. HTML5 timeline sounds are MP3 or WAV; streams are not re-synchronised if the browser drops frames, and custom envelopes are not available yet. See [examples/timeline-sound.nflash](../examples/timeline-sound.nflash) and the [compatibility notes](COMPATIBILITY.md#timeline-sound).

## Scenes

A document with several scenes is published as one main timeline, scene after scene. Scripts see the scenes through `currentScene`, `scenes`, `gotoAndPlay(frame, scene)` and `nextScene()`/`prevScene()`; AS3 frame numbers are relative to the current scene. Guide layers and ruler guides are never published.

## Data processing in ActionScript

The data-processing example loads an embedded JSON planning file through URLLoader, uses AS3 JSON/RegExp/Date to build the interface, and serializes an ordinary project class including a public getter. Its independent AS2 Date card advances in UTC with the same control. Restart returns to the original records and dates. Native JSON and RegExp are AS3 APIs; Date is available in both dialects.

These families use captured browser-backed adapters. Regex patterns currently use new RegExp(pattern, flags); slash literals remain unsupported. Date timezone/formatting follows the modern backend profile. JSON includes parse/stringify callbacks and bounded class serialization; it does not automatically serialize every host object. See COMPATIBILITY.md for precise supported forms and limits.

### Runtime display transforms

`examples/display-transforms.nflash` combines a nested linked card, a skewed matrix, scalar rotation/scale updates, vector/text color and two independent colorized views of shared BitmapData. Its Cycle control reparents the same card and returns it to the rig; Restart restores original objects and transforms. An AS2 strip uses the same geometry family. The runtime profile and remaining window-coordinate/native-authoring boundaries are documented in [the decision](decisions/0018-runtime-display-transforms.md).

### Typed collections

`examples/typed-collections.nflash` connects AS3 Vector models, Dictionary keys that are live card instances, for-in/for-each, deletion/membership and JSON serialization. Process batch updates stock and reuses the shipped card for an incoming model; construction stays at three linked instances. An independent AS2 for-in readout totals the original named properties. Restart restores the original models, keys and cards.

## XML catalogs

[xml-catalog.nflash](../examples/xml-catalog.nflash) loads one portable XML resource through AS3 URLLoader and AS2 XML.load. The AS3 side uses E4X selectors, edits target lists, inserts XML/XMLList literals and serializes a new tree; the AS2 side clones, moves and removes DOM nodes. Both retain namespace declarations and decoded entity text. Update both catalogs and Restart exercise publication lifetime. Filters, scoped default XML namespace and complete historical XML/E4X semantics remain pending.

## Scripted image loading

[loader-gallery.nflash](../examples/loader-gallery.nflash) loads original image resources through AS3 Loader/LoaderInfo, ByteArray loadBytes and AS2 MovieClipLoader. Loaded images use the shared Rust pixel buffers and normal scene transforms/input. Loader owns its content child; unload detaches that content, so saved AS3 Bitmap references remain usable. AS2 loadClip replaces the original target artwork and unloadClip leaves it empty. The same resource/HTTP rules and cancellation lifetime apply as for data loading. SWF/GIF loading and Flash security/application-domain emulation remain unsupported.

## Local saved state

Use SharedObject.getLocal for named application/game state, and call flush after meaningful changes. AS3 returns SharedObjectFlushStatus.FLUSHED; AS2 returns true. The saved-game example demonstrates both dialects and ByteArray AMF0/AMF3 object records. Reloading the same HTTP(S) publication or using Restart restores successfully saved data. Clear removes only that named record. Browser access/quota failures must be handled by project code; file/opaque origins cannot use this persistence profile. A best-effort pagehide flush is not a durability guarantee, and Restart discards unflushed changes. Explicit AMF0/AMF3 .sol exchange is available through fromSolBytes, importSolBytes and exportSolBytes.

### Share live state between published movies

Run the optional Rust relay and serve the HTML5 publication from the allowed origin. This command listens on loopback port 8090. The storage directory enables persistent rooms.

```sh
cargo run --locked -p neoflash-sync -- --origin http://localhost:8000 --store ./target/shared-rooms
```

For an AS3 movie served from `http://localhost:8000`, subscribe after the connection opens. Each subscriber receives the current values in the first SyncEvent. Use `setProperty` for top-level changes and `setDirty` after editing a nested value.

```actionscript
var endpoint:String = "ws://localhost:8090/game";
var connection:NetConnection = new NetConnection();
var room:SharedObject = SharedObject.getRemote("scoreboard", endpoint, true);
room.addEventListener(SyncEvent.SYNC, function(event:SyncEvent):void {
    trace(room.data.score);
});
connection.addEventListener(NetStatusEvent.NET_STATUS, function(event:NetStatusEvent):void {
    if (event.info.code == "NetConnection.Connect.Success") room.connect(connection);
});
connection.connect(endpoint);
// In an input handler after the first sync: room.setProperty("score", 42);
```

AS2 uses `NetConnection.onStatus`, `SharedObject.onSync` and direct `room.data.score = 42` assignments. Both dialects support property deletion, named client callbacks through `send` and batched dirty properties. After the server confirms `clear()`, the room is empty and the caller leaves its subscription. A remote `flush()` returns `"pending"` and reports completion through `SharedObject.Flush.Success` or `SharedObject.Flush.Failed`. Reconnect and subscribe again after a connection closes.

Applications share a room by endpoint path and object name. Concurrent writes to different properties merge, and the last update received by the server wins for a single property. Every client receives updates in server order. A temporary room expires when its last subscriber leaves. A persistent room restores its committed state on reconnect and across relay restarts when the same storage directory is used.

For HTTPS publications, expose the relay through a TLS reverse proxy and use `wss://`. Repeat `--origin` for each trusted browser publication. Each permitted client can read and write every room it can reach. Private applications need authentication and access control at the server or proxy. The endpoint speaks the NeoFlash WebSocket protocol. Adobe Media Server / RTMP endpoints require a separate compatible gateway. Client-side offline copies of remote rooms and replay of offline edits are outside this profile. The relay accepts AMF0 and AMF3 fields and stores up to 1,024 properties and 256 KiB of encoded state per room. Wire values that require a custom `IExternalizable` reader are rejected. Objects share identity within a field's AMF graph.

## Mountable HTML5 modules

Every standard HTML5 publication (`Full`, `Balanced`, `Compact` and `Shared`, in both `Canvas` and `Web App · DOM` modes) emits an ES module entry `module.mjs` (`export { mount, bundle, manifest, loadBundle, ensureRuntime }`) alongside standalone `index.html`. Host web apps can import multiple publications on one page and mount each into its own container element (`const app = await mount(container, { autoplay, scaleMode, align, parameters, onTrace, onError })`), sharing compiled `WebAssembly.Module` code across instances while keeping each instance's `WebPlayer`, `<canvas>`/DOM subtree, audio/video channels, timers and event listeners isolated until `app.destroy()` is called. Child publications can also be loaded dynamically via `loadPublish("child/module.mjs")`.
