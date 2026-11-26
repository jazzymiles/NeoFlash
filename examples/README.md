# NeoFlash Example Projects

Structured `.nflash` source projects demonstrating authoring, multi-language scripting, dynamic bundle loading, and multi-format publishing.

## Playable Games & Interactive Showcases

| File | Language | Description |
| --- | --- | --- |
| `doom-as2.nflash` | AS2 | *DOOM: E1M1 Hangar* — authored 100% via `neoflash-editor --headless` (`batch` + `import` + `jsfl`). 320-column DDA raycaster (`MovieClip.beginBitmapFill` + `ColorTransform`-lit `BitmapData` textures), 1D Z-buffered billboards, 1-click `Mouse.lock()` 360° mouselook & hold-to-fire, radial doorway corner-sliding, proximity auto-doors, hitscan Pistol & Pump Shotgun, MP3 SFX (`Sound.attachSound`), embedded `Courier New Bold` HUD, and `M`/`Tab` wireframe Automap. |
| `fallout-as2.nflash` | AS2 | *FALLOUT: Vault 13 Cavern Exit* — authored 100% via `neoflash-editor --headless`. Scrollable `36×28` (`1,008`-hex) 6-sided hexagonal cavern across 5 zones with 6-direction Hex A\* pathfinding, axial LoS & tactical cover, real-time exploration + 9-AP turn-based combat, V.A.T.S. called-shot modal, Sawed-Off Shotgun & 2-hex Frag Grenades, chain-detonating Rad-Drums, hackable Security Terminal, Pip-Boy Automap, `GlowFilter` sludge, `XML` catalog, `SharedObject` quick-save/load (`F5`/`F9`), and 12 PCM WAV SFX. |
| `tetris-as2.nflash` / `tetris-as3.nflash` | AS2 / AS3 | Classic falling-block puzzle in both dialects (also used to verify deterministic `Save As` `AS2 ↔ AS3` & `AS2/AS3 → JS` conversion). |
| `arkanoid-as2.nflash` / `arkanoid-as3.nflash` | AS2 / AS3 | Brick-breaker arcade game with paddle physics, level state, and keyboard/mouse controls. |
| `snake-as2.nflash` / `snake-as3.nflash` | AS2 / AS3 | Grid arcade game exercising frame/timer loops, dynamic display objects, and score HUDs. |
| `moon-tide-300x250.nflash` | — | Self-contained 300×250 animated HTML5 ad (`< 8 KB` ZIP via `Publish settings ▸ Profile: Criteo · HTML5 ZIP`). |

## Core Feature Workflows

- **Timelines & vectors:** `authoring-workspace.nflash`, `vector-paths.nflash`, `nested-orbit.nflash`, `timeline-tweens.nflash`, `clipped-scene.nflash`, `symbol-replacement.nflash`
- **Text, UI & storage:** `text-layout.nflash`, `text-form.nflash`, `keyboard-controls.nflash`, `drag-drop.nflash`, `saved-game.nflash`, `web-integration.nflash`
- **Media, bitmaps & data:** `timeline-sound.nflash`, `sound-playback.nflash`, `video-playback.nflash`, `bitmap-pixels.nflash`, `image-pipeline.nflash`, `filter-gallery.nflash`, `loader-gallery.nflash`, `resource-loading.nflash`, `data-processing.nflash`, `xml-catalog.nflash`
- **Classes & collections:** `project-program.nflash`, `as2-project-input.nflash`, `class-packages.nflash`, `interfaces-accessors.nflash`, `linked-movieclips.nflash`, `typed-collections.nflash`, `collections-text.nflash`, `geometry-timer.nflash`, `binary-data.nflash`

## Structured ActionScript Suites

### ActionScript 2 (`examples/as2/`)

| File | Capabilities Covered |
| --- | --- |
| `00-child-module.nflash` | 12-frame animated sub-application dynamically mounted by `07-sound-video-loadpublish.nflash`. |
| `01-math-geometry.nflash` | `Math`, `flash.geom.Point`, `Rectangle`, `Matrix`, `ColorTransform`, and `onEnterFrame` orbital motion. |
| `02-oop-xml-collections.nflash` | Multi-file `SourceUnit` classes (`model.Instrument` / `model.TelemetryProbe`), inheritance, `Array`, `XML`, `Date.UTC`, and `SharedObject`. |
| `03-movieclips-nested-timelines.nflash` | Nested `MovieClip` state timelines, frame labels (`idle`, `active`), `attachMovie` linkage, and `createEmptyMovieClip`. |
| `04-events-stage-align-drag.nflash` | `Stage.scaleMode = "noScale"`, `Stage.align = "TL"`, `Stage.addListener` (`onResize`), `startDrag`/`stopDrag`, and `hitTest`. |
| `05-drawing-api-filters-bitmapdata.nflash` | `beginGradientFill`/`curveTo` Drawing API, `GlowFilter` + `DropShadowFilter`, and `BitmapData` pixel synthesis (`attachBitmap`). |
| `06-dynamic-input-html-text.nflash` | `createTextField`, editable `"input"` fields, `TextFormat`, `wordWrap`, and rich `htmlText`. |
| `07-sound-video-loadpublish.nflash` | `Sound` (`WAV`), `NetConnection`/`NetStream` (`AVI`), and `loadMovie("subapp/bundle.js")` child bundle mounting (`loadPublish`). |
| `08-interactive-arcade-miniapp.nflash` | *Starlight Interceptor* — playable arcade mini-game with keyboard/auto-pilot control, starfield Drawing API, collision scoring, and `R` restart. |

### ActionScript 3 (`examples/as3/`)

| File | Capabilities Covered |
| --- | --- |
| `00-child-module.nflash` | 12-frame animated AS3 sub-application dynamically mounted via `Loader` in `07-sound-video-loadpublish.nflash`. |
| `01-math-geometry.nflash` | Typed `flash.geom.Point`, `Rectangle`, `Matrix`, `ColorTransform`, and `Event.ENTER_FRAME` orbital motion. |
| `02-oop-xml-collections.nflash` | Document class `app.Main`, `package model` `override` inheritance, `Array`, `XML`, `ByteArray`, `RegExp`, `Date.UTC`, and `SharedObject`. |
| `03-movieclips-nested-timelines.nflash` | Linked symbol class `app.SignalPod`, `new SignalPod()`, `Sprite` display list, and frame-label state transitions. |
| `04-events-stage-align-drag.nflash` | `stage.scaleMode`, `stage.align`, `Event.RESIZE`, `MouseEvent` drag-and-drop, and `hitTestObject`. |
| `05-drawing-api-filters-bitmapdata.nflash` | `Sprite.graphics.beginGradientFill`, `GlowFilter` + `DropShadowFilter`, and `Bitmap` + `BitmapData` pixel synthesis. |
| `06-dynamic-input-html-text.nflash` | `TextField`, `TextFieldType.INPUT`, `TextFormat`, `wordWrap`, and rich `htmlText`. |
| `07-sound-video-loadpublish.nflash` | `Sound`/`SoundChannel`/`SoundTransform` (`WAV`), `Video`/`NetStream` (`AVI`), and `flash.display.Loader` child bundle mounting (`loadPublish`). |
| `08-interactive-arcade-miniapp.nflash` | *Neon Defender* — playable AS3 arcade mini-game with `KeyboardEvent` & auto-pilot modes, `Sprite` starfield, collision scoring, and `R` restart. |

## Animated Series — *Midnight Express*

- `midnight-express.nflash` .. `midnight-express-3.nflash` — Episodes 1–3 (multi-scene Classic/Motion/Shape tweens, shape hints, motion guides, masks, blend modes, stereo stream audio).
- `midnight-express-4.nflash` — Episode 4 (*The Aurora Puppet & IK Odyssey*).
- `midnight-express-5.nflash` — Episode 5 (*The Celestial Observatory & Eclipse Crossing*): 3 scenes (`180` frames @ `24 fps`), guided `orient_to_path` viaduct, Glow/Blur/DropShadow filters, and 3-scene stereo `WAV` stream soundtrack; verified across HTML5, MP4, SVG/AI/PNG sequences, Starling XML Texture Atlas, SWF, and After Effects `.aep`.
- `midnight-express-6.nflash` — Episode 6 (*The Aurora Borealis & Polar Starlight Viaduct*): 3 polar scenes (`180` frames @ `24 fps`, `960×540`), `9-slice` scaling plaque, masked Shape-tween aurora, `◆` property-keyed beacon, and stereo `WAV` stream soundtrack.
