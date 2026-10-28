# AS2 Stage, display and content context menus

AS2 viewport and context-menu policy use the shared Rust display graph and StageState. Browser code supplies viewport/fullscreen facts, DOM controls and live object/callback references.

## Stage and display profile

- `Stage.align` and `scaleMode` drive actual Canvas viewport placement. `showAll`, `noBorder` and `exactFit` retain authored logical dimensions; `noScale` uses viewport dimensions and currently requires `align="TL"`. Rust validates inputs and returns scale, offsets and logical dimensions. Pointer picking, text overlays and semantic controls use the resulting X/Y scales. Reset restores the ordinary preview transform.
- `displayState` requests the browser Fullscreen API from an actual user gesture. `onFullScreen` reports actual browser transitions; rejected requests are actionable errors. Width/height use browser monitor dimensions in CSS pixels. `fullScreenSourceRect` is defensively copied and bounded inside the authored stage. Original Flash physical-pixel, browser permission and concurrent-request parity are not claimed.
- `showMenu=false` suppresses the native Canvas context menu and the supported default items in authored menus. Custom authored items remain available. `wmodeGPU` is not implemented.
- AS2-only `ColorTransform.rgb` and `Rectangle.containsRectangle` reuse the existing Rust geometry operations.
- `trackAsMenu` transfers a live press between participating MovieClip/Button targets. `_focusrect=true` paints an explicit yellow focus rectangle; unspecified legacy defaults and exact Flash highlighting remain separate work. Callback removal/Restart cannot leave a stale pressed target in the transfer path.
- `MovieClip.forceSmoothing` affects actual PNG/JPEG content loaded into that clip. It does not override SWF or video rendering. AS2 `Video.width/height` now expose decoded dimensions; underscore dimensions continue to resize the viewport.

## Content menus

`ContextMenu` and `ContextMenuItem` use Rust caption/flag validation, a 15-item limit, duplicate normalization, visible item selection and ordered row planning. The browser adapter displays an actual accessible menu over the publication, supports keyboard navigation/Escape, disabled items and separators, and invokes `onSelect` with the live owning display object and selected menu/item. MovieClip, Button and Canvas-rendered TextField `menu` properties retain references; parent lookup finds the closest assigned menu. Reset removes the menu and invalidates old objects/callbacks.

Menu copies retain callback references and independently copy item values and supported flags. Arbitrary extra dynamic properties, native input-edit menus, original branding restrictions and exact legacy coercion are not included. Captions require 1–100 UTF-16 units of visible text without control characters; invalid values diagnose instead of silently disappearing. Duplicate comparison ignores Unicode case, punctuation and whitespace within this bounded profile.

`builtInItems` contains the supported Boolean flags `play`, `rewind`, `forward_back` and `print`, initially true. They display actual Play/Stop, Rewind, Previous/Next frame and browser Print publication actions. `hideBuiltInItems()` sets these flags false and removes those rows. Browser printing concerns the containing publication page. `zoom`, `quality` and `loop` access diagnoses explicitly; no fake Boolean support or player plug-in settings are provided. These differences are why the API remains partial.

## Example and verification boundary

[as2-stage-display-batch.nflash](../examples/as2-stage-display-batch.nflash) combines decoded Video and loaded PNGs, sizing controls, fullscreen, focus/menu dragging, geometry and authored context menus. Representative browser observations covered fullscreen entry/Escape exit, sizing, menu-press transfer, context-menu actions and loaded-image smoothing. Browser permissions and the documented display limits still apply.

## Provenance

Contract references are Adobe's mirrored [Stage](https://open-flash.github.io/mirrors/as2-language-reference/Stage.html), [MovieClip](https://open-flash.github.io/mirrors/as2-language-reference/MovieClip.html), [ContextMenu](https://open-flash.github.io/mirrors/as2-language-reference/ContextMenu.html) and [ContextMenuItem](https://open-flash.github.io/mirrors/as2-language-reference/ContextMenuItem.html) reference pages. The new policy is project Rust code; no Adobe implementation was copied. Existing shared display and geometry adaptations retain their attribution and pinned provenance in `THIRD_PARTY_NOTICES.txt` and the earlier subsystem records. Downloaded references remain under ignored `target/`.
