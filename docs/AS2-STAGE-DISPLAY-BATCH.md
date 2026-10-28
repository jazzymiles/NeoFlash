# AS2 Stage, display and content context menus

This bounded AS2 package uses the shared Rust display graph and adds `StageState` for viewport and context-menu policy. Browser code supplies viewport/fullscreen facts, DOM controls and live object/callback references. Native editor UI, FLA and AS3 facade behavior are outside the delivery.

## Stage and display profile

- `Stage.align` and `scaleMode` drive actual Canvas viewport placement. `showAll`, `noBorder` and `exactFit` retain authored logical dimensions; `noScale` uses viewport dimensions and currently requires `align="TL"`. Rust validates inputs and returns scale, offsets and logical dimensions. Pointer picking, text overlays and semantic controls use the resulting X/Y scales. Reset restores the ordinary preview transform.
- `displayState` requests the browser Fullscreen API from an actual user gesture. `onFullScreen` reports actual browser transitions; rejected requests are actionable errors. Width/height use browser monitor dimensions in CSS pixels. `fullScreenSourceRect` is defensively copied and bounded inside the authored stage. Original Flash physical-pixel, browser permission and concurrent-request parity are not claimed.
- `showMenu=false` suppresses the native Canvas context menu and the supported default items in authored menus. Custom authored items remain available. `wmodeGPU` is not implemented.
- AS2-only `ColorTransform.rgb` and `Rectangle.containsRectangle` reuse the existing Rust geometry operations.
- `trackAsMenu` transfers a live press between participating MovieClip/Button targets. `_focusrect=true` paints an explicit yellow focus rectangle; unspecified legacy defaults and exact Flash highlighting remain separate work. Callback removal/Restart cannot leave a stale pressed target in the transfer path.
- `MovieClip.forceSmoothing` affects actual PNG/JPEG content loaded into that clip. It does not override SWF or video rendering. AS2 `Video.width/height` now expose decoded dimensions; underscore dimensions continue to resize the viewport.

Thirteen Video transform/identity/pointer aliases, Button `_name`, three Selection listener/event entries and three Transform getters were already implemented. Their 20 stale missing decisions are recorded as reviewed-existing corrections, not new code. Stage width/height/onResize are three extensions. An existing Selection.getFocus path defect found during browser acceptance was corrected by routing through the shared display target-path operation; it is a fourth extension/correction, not a new API.

## Content menus

`ContextMenu` and `ContextMenuItem` use Rust caption/flag validation, a 15-item limit, duplicate normalization, visible item selection and ordered row planning. The browser adapter displays an actual accessible menu over the publication, supports keyboard navigation/Escape, disabled items and separators, and invokes `onSelect` with the live owning display object and selected menu/item. MovieClip, Button and Canvas-rendered TextField `menu` properties retain references; parent lookup finds the closest assigned menu. Reset removes the menu and invalidates old objects/callbacks.

Menu copies retain callback references and independently copy item values and supported flags. Arbitrary extra dynamic properties, native input-edit menus, original branding restrictions and exact legacy coercion are not included. Captions require 1–100 UTF-16 units of visible text without control characters; invalid values diagnose instead of silently disappearing. Duplicate comparison ignores Unicode case, punctuation and whitespace within this bounded profile.

`builtInItems` contains the supported Boolean flags `play`, `rewind`, `forward_back` and `print`, initially true. They display actual Play/Stop, Rewind, Previous/Next frame and browser Print publication actions. `hideBuiltInItems()` sets these flags false and removes those rows. Browser printing concerns the containing publication page. `zoom`, `quality` and `loop` access diagnoses explicitly; no fake Boolean support or player plug-in settings are provided. These differences are why the API remains partial.

## Evidence and accounting

`examples/as2-stage-display-batch.nflash` combines 25 grouped checks, real decoded Video metadata and loaded PNGs, four sizing controls, fullscreen, focus/menu dragging, geometry and authored right-click menus. The coordinator observed **25/25**, actual fullscreen entry and Escape exit, exactFit/showAll sizing, A→B menu-press transfer (B=2 and drags=2), the context-menu action with disabled/hidden rows, decoded Video and loaded-image smoothing. The stage example helper-name collision and the existing Selection.getFocus path were repaired during delivery; the affected text-form workflow passed its focused rerun. These observations do not establish independent conformance for every alias or every browser permission path.

The one new grouped Rust test `as2_stage::tests::stage_layout_family` passed once locally, including menu row policy. Existing `as2_input` and `dynamic_display` functions were extended for menu transfer/focus and actual smoothing without adding test functions. Shared delivery checks belong to the coordinator; they are not repeated per method. Source review also guarded menu-transfer callbacks after removal/Restart and kept the preview's Y center correct after exactFit.

Exact inventory: **35 new declarations** (four methods, 24 properties, three events, two constructors, two classes), **four extensions/corrections**, **20 reviewed-existing corrections**. Records are in ignored `target/verification/as2-runtime-two-hour/display-inventory.json`; the coordinator applies the final catalogue review. The source review does not independently certify each declaration.

## Provenance

Contract references are Adobe's mirrored [Stage](https://open-flash.github.io/mirrors/as2-language-reference/Stage.html), [MovieClip](https://open-flash.github.io/mirrors/as2-language-reference/MovieClip.html), [ContextMenu](https://open-flash.github.io/mirrors/as2-language-reference/ContextMenu.html) and [ContextMenuItem](https://open-flash.github.io/mirrors/as2-language-reference/ContextMenuItem.html) reference pages. The new policy is project Rust code; no Adobe implementation was copied. Existing shared display and geometry adaptations retain their attribution and pinned provenance in `THIRD_PARTY_NOTICES.txt` and the earlier subsystem records. Downloaded references remain under ignored `target/`.
