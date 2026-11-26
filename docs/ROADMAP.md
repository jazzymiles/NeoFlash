# Roadmap

NeoFlash provides an open-source Flash-inspired studio in Rust for interactive websites, web applications, 2D games, and animation, with HTML5 + WebAssembly as its primary publishing target.

**[Beta 1 (`0.1.0-beta.1`)](https://github.com/jazzymiles/NeoFlash/releases/tag/v0.1.0-beta.1) ships Developer ID signed, Apple-notarized builds for macOS 15+ (Apple Silicon) and macOS 12+ (Intel), alongside Windows x64 and Ubuntu 24.04 x64 Technical Previews.** What already works today is recorded in the [detailed capability registry](progress/DETAILS.md), [ActionScript declaration audit](progress/ACTIONSCRIPT.md), and [compatibility matrix](COMPATIBILITY.md). This page tracks remaining product, runtime, and platform work.

## Remaining Product & Platform Work

| Area | Open work beyond `0.1.0-beta.1` |
| --- | --- |
| **Drawing & stage tools** | Artistic/pattern brush presets, advanced Pencil stroke styling, predictive stroke-smoothing preview, partial-fill erasing (Eraser `Cut strokes` and contour Lasso `Slice merge fills` already work), editable live IK skin weights/spring persistence (`Bind`), and richer direct Stage gestures for `Deco` / `Spray Brush` symbol templates. |
| **Timelines, text & media** | Large-scale (>100,000 expanded nodes) document memory/Undo budgets, full OpenType GSUB/GPOS complex-script shaping, OS-native IME candidate-window parity across platforms, richer HTML/CSS text authoring controls, and real-time audio stream re-synchronization when a browser drops frames. |
| **Interactive web & ads** | Independent DOM browse-order reordering and richer raster-island ARIA semantics in `Web App · DOM` mode, plus validating `Criteo · HTML5 ZIP` and `Web Banner · Native` creatives against live ad-network validator endpoints. |
| **Code authoring & debugging** | Deeper cross-file static type-flow inference, exact attribution of deduplicated shared-handler occurrences in Source Maps, and connecting the private Chromium `Test Movie` CDP debugger pipe on Windows. |
| **ActionScript & SWF conformance** | Historical Flash VM coercion, prototype-mutation, and frame-construction edge cases where compiled JS differs from AVM1/AVM2; remaining SWF tag/opcode long tail in `SwfBytecodePlayer` and HTML5 `Loader`; and external RTMP / Adobe Media Server gateway support (`SharedObject.getRemote` / `connect` currently synchronize over the self-hosted `neoflash-sync` WebSocket relay). |
| **Design & legacy import** | Deeper nested Figma Auto Layout reflow, Photoshop adjustment-layer parity beyond `lfx2`/`lrFX` layer styles, PostScript EPS edge cases, and narrowing host font-metric / anti-aliasing residuals in multi-frame Adobe Animate visual comparisons. |
| **Platform, accessibility & updates** | Live post-release updater handoff & rollback verification (`Preferences ▸ Updates` currently downloads SHA-256-verified archives for manual install; unattended self-replacement is post-Beta), signed Windows installer & interactive physical Windows 11 x64 desktop sign-off, Linux AppImage & broader-distro packaging beyond Ubuntu 24.04 `.deb`/`.tar.gz`, physical Wacom & Apple Sidecar hardware certification, spoken macOS VoiceOver verification, activating the optional Beta feedback endpoint, and direct cloud TLS transport for AI provider profiles. |

<a id="as3-flash-player-foundations-first"></a>
## AS3: Flash Player Foundations First

ActionScript 2 (`1,236 / 1,238` Working) and ActionScript 3 (`4,162 / 4,164` Working) are tracked across the same eight Flash Player target areas (`0` Missing declarations in both languages; only 2 RTMP/FMS `SharedObject` declarations remain `Partial` in each). Deeper behavioral edge cases follow this priority order:

| Stage | Scope | Remaining conformance focus |
| --- | --- | --- |
| **1. Mathematics & numeric values** | `Math`, `Number`/`int`/`uint`, formatting, 2D `Point`/`Rectangle`/`Matrix` | Rare coercion/arity edge cases and historical player rounding switches. |
| **2. Objects, language & collections** | Classes, interfaces, `Array`, `Vector.<T>`, `Dictionary`, `String`, `Date`, `RegExp`, `XML`/E4X, `ByteArray` | Exotic prototype mutations, full PCRE backtracking parity, and full ECMA-357 E4X corner cases. |
| **3. MovieClip & timeline** | Multi-scene timelines, labels, nested clocks, `SimpleButton`, phased frame lifecycle | Streaming-progress edge cases and rare reentrant `goto` construction ordering. |
| **4. Display list, events & input** | `DisplayObjectContainer`, event phases, masks, `scrollRect`, `Stage` scale/align/fullscreen, keyboard/mouse/focus | Native per-child callback timing during bulk `removeChildren` and automatic keyboard tab ordering edge cases. |
| **5. Graphics, bitmaps & effects** | `Graphics`, gradients, bitmap fills, `BitmapData` (incl. PNG/JPEG/JPEG-XR encode), 8 filter families, 14 blend modes + Pixel Bender `SHADER` | Exact Flash software-rasterizer pixel parity on subpixel strokes, noise seeds, and complex filter chains. |
| **6. Text & fonts** | `TextField`, `TextFormat`, `StyleSheet`, embedded TrueType/OpenType/WOFF1 fonts, Flash Text Engine (`flash.text.engine.*`), IME | Host-independent glyph metric parity with Flash's proprietary text rasterizer. |
| **7. Local sound & video** | `Sound`, `SoundChannel`, 2×2 `SoundTransform`, `SoundMixer`, progressive `NetStream` & `Video`, cue points | Dropped-frame stream audio resynchronization and unbundled legacy video codecs in browsers. |
| **8. External services & Player APIs** | `URLLoader`, `SharedObject` + AMF0/AMF3 `.sol` + `neoflash-sync`, `ExternalInterface`, `Loader` SWF, `Worker`, `Socket`, `ContextMenu`, `PrintJob`, `Camera`/`Microphone`, `Stage3D`, DRM | Browser sandbox boundaries (raw TCP sockets, OS binary self-update, hardware EME CDMs) and native RTMP/FMS servers. |

**Outside the Flash Player target (never counted in percentages):** AIR-only desktop/mobile APIs (`1,448` AS3 entries), Flex `mx.*`/`spark.*`, `fl.*` UI components, TLF `flashx.textLayout.*`, and external libraries (`37,990` AS3 and `1,069` AS2 entries) remain unscheduled far-future tracks unless priorities explicitly change. Note that JSFL (`docs/JSFL-COMPATIBILITY.md`) is authoring-time editor automation, not a published Player runtime API.

## Principles & Validation

- Compatible open-source algorithms may be adapted into the shared Rust core with preserved licenses and pinned provenance (`docs/*-PROVENANCE.md`, `THIRD_PARTY_NOTICES.txt`); complete third-party ActionScript engines are never embedded.
- Unsupported content produces actionable, located diagnostics rather than silent omission.
- Compiler and unit tests alone do not establish user-visible completion: editor workflows require desktop verification and HTML5 features require real browser verification ([verification record](VERIFICATION.md), [known issues](BACKLOG.md)).
