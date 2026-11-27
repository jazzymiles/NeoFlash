# NeoFlash Example Projects

Structured `.nflash` source projects and licensed `.fla` / XFL / `.swf` corpus fixtures organized by topic.

## Directory Layout

| Folder | Contents |
| --- | --- |
| [`games/`](games/) | Playable AS2, AS3, and TypeScript/DOM games (`doom-as2`, `fallout-as2`, `xcom-as3`, `simcity-ts`, `tetris`, `arkanoid`, `snake`, and `saved-game`). |
| [`video-and-audio/`](video-and-audio/) | Embedded & progressive video (`NetConnection`/`NetStream`), frame-synced timeline audio (`Stream`/`Event`/`Start`/`Stop`), and scripted `Sound` playback. |
| [`animation/`](animation/) | Multi-scene cartoons (*Midnight Express* Episodes 1–6), Classic/Motion/Shape tweens, motion guides, vector paths, masks, and symbol replacement. |
| [`as2/`](as2/) | Structured ActionScript 2.0 reference suite (`00`–`08`), classic OOP input (`as2-project-input`), Rust string reuse (`reuse-as2`), and [`as2/batches/`](as2/batches/) API batches. |
| [`as3/`](as3/) | Structured ActionScript 3.0 reference suite (`00`–`08`), packages/interfaces/typed collections/linked MovieClips, and [`as3/runtime/`](as3/runtime/) runtime API suites. |
| [`graphics-and-filters/`](graphics-and-filters/) | Authoring workspace & SVG import (`authoring-workspace` + `authoring-badge.svg`), `BitmapData` pipelines, `Loader` galleries, display filters, gradients, and 2D transforms. |
| [`ui-and-text/`](ui-and-text/) | Typography & `TextFormat` layout, editable input forms, drag-and-drop hit testing, keyboard/accessible controls, `Stage` events, and `_visible`/`visible` coercion. |
| [`web-and-ads/`](web-and-ads/) | Self-contained `< 8 KB` Criteo HTML5 ad (`moon-tide-300x250`), `ExternalInterface` page integration, `URLLoader`/`LoadVars`, JSON/RegExp/Date pipelines, and E4X/XML catalogs. |
| [`fla-corpus/`](fla-corpus/) | `21/21` licensed public Flash 5–CS6 `.fla`, XFL, and `.swf` fixtures with visual & bytecode-trace parity oracles ([details](fla-corpus/README.md)). |

[`browser-batch.json`](browser-batch.json) lists the 34 interactive browser-review scenarios consumed by `scripts/prepare-browser-batch.py`.

---

## Playable Games (`examples/games/`)

| File | Language | Description |
| --- | --- | --- |
| `games/doom-as2.nflash` | AS2 | *DOOM: E1M1 Hangar* — authored 100% via `neoflash-editor --headless` (`batch` + `import` + `jsfl`). 320-column DDA raycaster (`MovieClip.beginBitmapFill` + `ColorTransform`-lit `BitmapData` textures), 1D Z-buffered billboards, 1-click `Mouse.lock()` 360° mouselook & hold-to-fire, radial doorway corner-sliding, proximity auto-doors, hitscan Pistol & Pump Shotgun, MP3 SFX (`Sound.attachSound`), embedded `Courier New Bold` HUD, and `M`/`Tab` wireframe Automap. |
| `games/fallout-as2.nflash` | AS2 | *FALLOUT: Vault 13 Cavern Exit* — authored 100% via `neoflash-editor --headless`. Scrollable `36×28` (`1,008`-hex) 6-sided hexagonal cavern across 5 zones with 6-direction Hex A\* pathfinding, axial LoS & tactical cover, real-time exploration + 9-AP turn-based combat, V.A.T.S. called-shot modal, Sawed-Off Shotgun & 2-hex Frag Grenades, chain-detonating Rad-Drums, hackable Security Terminal, Pip-Boy Automap, `GlowFilter` sludge, `XML` catalog, `SharedObject` quick-save/load (`F5`/`F9`), and 12 PCM WAV SFX. |
| `games/xcom-as3.nflash` | AS3 | *X-COM: Sector 7 Crash Site* — authored 100% via `neoflash-editor --headless`. 7-class multi-package ActionScript 3.0 turn-based squad tactics (`ITacticalActor`, `TacticalEvent extends Event`, `Vector.<int>`, `Dictionary`, `BitmapData` `18×18` motion scanner, `GlowFilter` plasma combat, directional Night Sight Cones, A\* `TU` pathfinding, Overwatch reaction fire, right-click fire-mode cycling, `SharedObject` quick-save/load, and 6 PCM WAV SFX). |
| `games/simcity-ts.nflash` | TypeScript (`DOM` / `Canvas`) | *Micropolis 2000: Isometric City Simulator* — authored 100% via `neoflash-editor --headless`. 4-module generic TypeScript (`Grid2D<T>`) isometric city builder with river/bay terrain, BFS power-grid propagation, multi-stage R/C/I towers, road traffic, Power/Value/Smog heatmaps, annual Municipal Budget & Tax modal, and 5 PCM WAV SFX. |
| `games/tetris-as2.nflash` / `games/tetris-as3.nflash` | AS2 / AS3 | Classic falling-block puzzle in both dialects (also used to verify deterministic `Save As` `AS2 ↔ AS3` & `AS2/AS3 → JS` conversion). |
| `games/arkanoid-as2.nflash` / `games/arkanoid-as3.nflash` | AS2 / AS3 | Brick-breaker arcade game with paddle physics, level state, and keyboard/mouse controls. |
| `games/snake-as2.nflash` / `games/snake-as3.nflash` | AS2 / AS3 | Grid arcade game exercising frame/timer loops, dynamic display objects, and score HUDs. |
| `games/saved-game.nflash` | AS2 / AS3 | `SharedObject` score/profile persistence and `ByteArray` AMF0/AMF3 cyclic object-graph serialization. |

## Video & Audio (`examples/video-and-audio/`)

- `video-and-audio/video-playback.nflash` — Embedded H.264 MP4 & optional HTTP progressive video playback (`Video`, `NetConnection`, `NetStream`, pause/seek/close, and Restart lifecycle).
- `video-and-audio/timeline-sound.nflash` — Frame-attached timeline sounds (`Stream`, `Event`, `Start`, `Stop`, stereo envelopes, and `SoundMixer.stopAll()`).
- `video-and-audio/sound-playback.nflash` — Complete-file `Sound` / `SoundChannel` MP3 playback and completion callbacks across AS2 and AS3.
- Related dialect suites: `as2/07-sound-video-loadpublish.nflash`, `as3/07-sound-video-loadpublish.nflash`, `as2/batches/as2-media-batch.nflash`, `as2/batches/as2-capture-batch.nflash`, and `as2/batches/as2-sound-batch.nflash`.

## Animation & Series (`examples/animation/`)

### Animated Series — *Midnight Express*

- `animation/midnight-express.nflash` .. `animation/midnight-express-3.nflash` — Episodes 1–3 (multi-scene Classic/Motion/Shape tweens, shape hints, motion guides, masks, blend modes, stereo stream audio).
- `animation/midnight-express-4.nflash` — Episode 4 (*The Aurora Puppet & IK Odyssey*).
- `animation/midnight-express-5.nflash` — Episode 5 (*The Celestial Observatory & Eclipse Crossing*): 3 scenes (`180` frames @ `24 fps`), guided `orient_to_path` viaduct, Glow/Blur/DropShadow filters, and 3-scene stereo `WAV` stream soundtrack; verified across HTML5, MP4, SVG/AI/PNG sequences, Starling XML Texture Atlas, SWF, and After Effects `.aep`.
- `animation/midnight-express-6.nflash` — Episode 6 (*The Aurora Borealis & Polar Starlight Viaduct*): 3 polar scenes (`180` frames @ `24 fps`, `960×540`), `9-slice` scaling plaque, masked Shape-tween aurora, `◆` property-keyed beacon, and stereo `WAV` stream soundtrack.

### Timeline, Vectors & Masks

- `animation/timeline-tweens.nflash` — Multi-scene Classic, Motion, Shape, and guided tweens.
- `animation/nested-orbit.nflash` — Independent nested `MovieClip` timelines and playback controls (*Ideas in motion*).
- `animation/vector-paths.nflash` — Bézier curves, compound fills, and transformed vector geometry.
- `animation/clipped-scene.nflash` — Compound vector masks, `scrollRect`, `setMask`, and preserved linked-instance identity.
- `animation/symbol-replacement.nflash` — Same-path symbol replacement across keyframes.

## ActionScript 2 (`examples/as2/`)

| File | Capabilities Covered |
| --- | --- |
| `as2/00-child-module.nflash` | 12-frame animated sub-application dynamically mounted by `07-sound-video-loadpublish.nflash`. |
| `as2/01-math-geometry.nflash` | `Math`, `flash.geom.Point`, `Rectangle`, `Matrix`, `ColorTransform`, and `onEnterFrame` orbital motion. |
| `as2/02-oop-xml-collections.nflash` | Multi-file `SourceUnit` classes (`model.Instrument` / `model.TelemetryProbe`), inheritance, `Array`, `XML`, `Date.UTC`, and `SharedObject`. |
| `as2/03-movieclips-nested-timelines.nflash` | Nested `MovieClip` state timelines, frame labels (`idle`, `active`), `attachMovie` linkage, and `createEmptyMovieClip`. |
| `as2/04-events-stage-align-drag.nflash` | `Stage.scaleMode = "noScale"`, `Stage.align = "TL"`, `Stage.addListener` (`onResize`), `startDrag`/`stopDrag`, and `hitTest`. |
| `as2/05-drawing-api-filters-bitmapdata.nflash` | `beginGradientFill`/`curveTo` Drawing API, `GlowFilter` + `DropShadowFilter`, and `BitmapData` pixel synthesis (`attachBitmap`). |
| `as2/06-dynamic-input-html-text.nflash` | `createTextField`, editable `"input"` fields, `TextFormat`, `wordWrap`, and rich `htmlText`. |
| `as2/07-sound-video-loadpublish.nflash` | `Sound` (`WAV`), `NetConnection`/`NetStream` (`AVI`), and `loadMovie("subapp/bundle.js")` child bundle mounting (`loadPublish`). |
| `as2/08-interactive-arcade-miniapp.nflash` | *Starlight Interceptor* — playable arcade mini-game with keyboard/auto-pilot control, starfield Drawing API, collision scoring, and `R` restart. |
| `as2/as2-project-input.nflash` | Inherited AS2 classes, linked `MovieClip` instances, shared state, and `Key`/`Mouse`/`Stage` input listeners. |
| `as2/reuse-as2.nflash` | Rust-backed AS2 string processing and runtime verification suite. |
| `as2/batches/*.nflash` | 23 focused AS2 runtime batches (`bitmap`, `capture`, `connections`, `data`, `depth-manager`, `display`, `display-filters`, `files` + `as2-files-fixture.txt`, `interfaces`, `language`, `legacy`, `locale`, `media`, `object`, `platform`, `runtime-language`, `services` + `as2-services.wsdl`, `sound`, `stage-display`, `text-variable`, `transitions`, `variables`, `xml-data`). |

## ActionScript 3 (`examples/as3/`)

| File | Capabilities Covered |
| --- | --- |
| `as3/00-child-module.nflash` | 12-frame animated AS3 sub-application dynamically mounted via `Loader` in `07-sound-video-loadpublish.nflash`. |
| `as3/01-math-geometry.nflash` | Typed `flash.geom.Point`, `Rectangle`, `Matrix`, `ColorTransform`, and `Event.ENTER_FRAME` orbital motion. |
| `as3/02-oop-xml-collections.nflash` | Document class `app.Main`, `package model` `override` inheritance, `Array`, `XML`, `ByteArray`, `RegExp`, `Date.UTC`, and `SharedObject`. |
| `as3/03-movieclips-nested-timelines.nflash` | Linked symbol class `app.SignalPod`, `new SignalPod()`, `Sprite` display list, and frame-label state transitions. |
| `as3/04-events-stage-align-drag.nflash` | `stage.scaleMode`, `stage.align`, `Event.RESIZE`, `MouseEvent` drag-and-drop, and `hitTestObject`. |
| `as3/05-drawing-api-filters-bitmapdata.nflash` | `Sprite.graphics.beginGradientFill`, `GlowFilter` + `DropShadowFilter`, and `Bitmap` + `BitmapData` pixel synthesis. |
| `as3/06-dynamic-input-html-text.nflash` | `TextField`, `TextFieldType.INPUT`, `TextFormat`, `wordWrap`, and rich `htmlText`. |
| `as3/07-sound-video-loadpublish.nflash` | `Sound`/`SoundChannel`/`SoundTransform` (`WAV`), `Video`/`NetStream` (`AVI`), and `flash.display.Loader` child bundle mounting (`loadPublish`). |
| `as3/08-interactive-arcade-miniapp.nflash` | *Neon Defender* — playable AS3 arcade mini-game with `KeyboardEvent` & auto-pilot modes, `Sprite` starfield, collision scoring, and `R` restart. |
| `as3/project-program.nflash` | Document classes, linked component initialization order, and `EventDispatcher` workflows. |
| `as3/class-packages.nflash` / `as3/interfaces-accessors.nflash` / `as3/function-workflow.nflash` | Package visibility, `protected`/`internal` members, `interface` contracts, `get`/`set` accessors, and bound method dispatch. |
| `as3/typed-collections.nflash` / `as3/linked-movieclips.nflash` / `as3/geometry-timer.nflash` / `as3/binary-data.nflash` | Typed `Vector.<T>`, object-keyed `Dictionary`, linked `MovieClip` timelines, `Matrix`/`Timer`, and `ByteArray` UTF/binary codecs. |
| `as3/reuse-as3.nflash` / `as3/as-intrinsics.nflash` / `as3/script-recovery.nflash` | `Matrix3D`/`Vector3D` geometry reuse, AS2/AS3 intrinsic profiles, and runtime script-error recovery. |
| `as3/runtime/*.nflash` | 9 Rust-backed runtime suites (`runtime-binary`, `runtime-date`, `runtime-easing`, `runtime-event-families`, `runtime-filter-extensions`, `runtime-geometry`, `runtime-math`, `runtime-number-format`, `runtime-strings`). |

## Graphics, Filters & UI (`examples/graphics-and-filters/`, `examples/ui-and-text/`, `examples/web-and-ads/`)

- **Graphics & filters (`examples/graphics-and-filters/`):** `authoring-workspace.nflash` (+ `authoring-badge.svg`), `bitmap-pixels.nflash`, `image-pipeline.nflash`, `loader-gallery.nflash`, `filter-gallery.nflash`, `dynamic-display.nflash`, `display-transforms.nflash`, `display-family.nflash`.
- **UI, text & input (`examples/ui-and-text/`):** `text-layout.nflash`, `text-form.nflash`, `collections-text.nflash`, `drag-drop.nflash`, `keyboard-controls.nflash`, `events-stage.nflash`, `visibility.nflash`, `visibility-coercion.nflash`, `visibility-coercion-unsupported.nflash`.
- **Web, data & HTML5 ads (`examples/web-and-ads/`):** `moon-tide-300x250.nflash` (`< 8 KB` Criteo HTML5 ZIP ad), `web-integration.nflash`, `resource-loading.nflash`, `data-processing.nflash`, `xml-catalog.nflash`.
