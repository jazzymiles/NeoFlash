# NeoFlash Example Projects

This folder contains structured reference projects demonstrating NeoFlash authoring, scripting, dynamic runtime loading, and multi-format publishing.

## Structured ActionScript Suites (`T-252`)

### ActionScript 2 (`examples/as2/`)

| File | Capabilities Covered |
| --- | --- |
| `00-child-module.nflash` | Standalone 12-frame animated sub-application compiled and mounted dynamically by `07-sound-video-loadpublish.nflash`. |
| `01-math-geometry.nflash` | `Math`, `flash.geom.Point`, `Rectangle`, `Matrix`, `ColorTransform`, and `onEnterFrame` orbital animation. |
| `02-oop-xml-collections.nflash` | Multi-file `SourceUnit` classes (`model.Instrument` / `model.TelemetryProbe`), inheritance, `Array`, `XML`, `Date.UTC`, and `SharedObject`. |
| `03-movieclips-nested-timelines.nflash` | Nested `MovieClip` state timelines, frame labels (`idle`, `active`), `attachMovie` Library linkage, and `createEmptyMovieClip`. |
| `04-events-stage-align-drag.nflash` | `Stage.scaleMode = "noScale"`, `Stage.align = "TL"`, `Stage.addListener` (`onResize`), `startDrag`/`stopDrag`, and `hitTest`. |
| `05-drawing-api-filters-bitmapdata.nflash` | `beginGradientFill`/`curveTo` Drawing API, `GlowFilter` + `DropShadowFilter`, and `BitmapData` pixel synthesis (`attachBitmap`). |
| `06-dynamic-input-html-text.nflash` | `createTextField`, editable `"input"` fields, `TextFormat`, `wordWrap`, and rich `htmlText`. |
| `07-sound-video-loadpublish.nflash` | `Sound` (`WAV` playback), `NetConnection`/`NetStream` (`AVI` video), and `loadMovie("subapp/bundle.js")` mounting a published NeoFlash bundle (`loadPublish`). |
| `08-interactive-arcade-miniapp.nflash` | *Starlight Interceptor* — playable arcade mini-game with keyboard/auto-pilot control, starfield Drawing API, collision scoring, and `R`-key restart. |

### ActionScript 3 (`examples/as3/`)

| File | Capabilities Covered |
| --- | --- |
| `00-child-module.nflash` | Standalone 12-frame animated AS3 sub-application dynamically mounted via `Loader` in `07-sound-video-loadpublish.nflash`. |
| `01-math-geometry.nflash` | Typed `flash.geom.Point`, `Rectangle`, `Matrix`, `ColorTransform`, and `Event.ENTER_FRAME` orbital motion. |
| `02-oop-xml-collections.nflash` | Document class `app.Main`, `package model` `override` inheritance, `Array`, `XML`, `ByteArray`, `RegExp`, `Date.UTC`, and `SharedObject`. |
| `03-movieclips-nested-timelines.nflash` | Linked symbol class `app.SignalPod`, `new SignalPod()`, `Sprite` display list, and frame-label state transitions. |
| `04-events-stage-align-drag.nflash` | `stage.scaleMode`, `stage.align`, `Event.RESIZE`, `MouseEvent` drag-and-drop, and `hitTestObject`. |
| `05-drawing-api-filters-bitmapdata.nflash` | `Sprite.graphics.beginGradientFill`, `GlowFilter` + `DropShadowFilter`, and `Bitmap` + `BitmapData` pixel synthesis. |
| `06-dynamic-input-html-text.nflash` | `TextField`, `TextFieldType.INPUT`, `TextFormat`, `wordWrap`, and rich `htmlText`. |
| `07-sound-video-loadpublish.nflash` | `Sound`/`SoundChannel`/`SoundTransform` (`WAV`), `Video`/`NetStream` (`AVI`), and `flash.display.Loader` mounting a published NeoFlash bundle (`loadPublish`). |
| `08-interactive-arcade-miniapp.nflash` | *Neon Defender* — playable AS3 arcade mini-game with `KeyboardEvent` & auto-pilot modes, `Sprite` starfield, collision scoring, and `R`-key restart. |

## Animated Series — *Midnight Express*

- `midnight-express.nflash` — Episode 1
- `midnight-express-2.nflash` — Episode 2
- `midnight-express-3.nflash` — Episode 3
- `midnight-express-4.nflash` — Episode 4 (*The Aurora Puppet & IK Odyssey*)
- `midnight-express-5.nflash` — Episode 5 (*The Celestial Observatory & Eclipse Crossing*, `T-256`): 3 scenes (`180` frames at `24 fps`), nested locomotive & comet symbols, linear/radial gradients, mask beam, guided viaduct `orient_to_path` Classic tween, Motion & Shape tweens, Glow/Blur/DropShadow filters, and 3-scene stereo `WAV` stream soundtrack; verified across HTML5, MP4, SVG/AI/PNG sequences, Starling XML Texture Atlas, SWF, and After Effects `.aep`.
