# AS2 rendered transitions

AS2 TransitionManager supports Fade, Fly, Rotate, Zoom and Squeeze effects, using the existing Transition base and IN/OUT directions.

`TransitionManager.start(clip, params)` reuses or creates a manager for a live MovieClip and returns the actual effect instance. An explicit manager provides `startTransition`, mutable content when idle, `contentAppearance`, `toString`, and the existing AS2 event dispatcher methods. The saved appearance includes position, scales, alpha, rotation, dimensions, inner/outer bounds and a copied ColorTransform. It refreshes from the current content before a new idle sequence starts. Returned snapshots can be edited without altering the stored appearance.

The five effects write real display properties: Fade changes alpha; Fly moves from/to a viewport edge; Rotate changes angle with degrees/ccw; Zoom scales both dimensions; Squeeze scales its selected dimension. Fly's nine startPoint positions use the stage viewport converted into the target parent's coordinates. This includes transformed parents through the existing display coordinate conversion. Entry/exit progression, eased time, transform values and bounds calculations live in the new Rust `ScriptAS2Transition`. The browser adapter supplies actual display snapshots, Timer ticks, easing identity and reference/property/event wiring.

A transition uses duration in seconds and an existing built-in Flash easing function. Independent channels can animate together on one manager. Replacing the same effect cancels its timer and Rust state. Concurrent different effects that write the same property fail explicitly; this pass does not invent a transform-composition stack. Completion events are emitted when the last active effect in the corresponding direction finishes. After all OUT effects finish, the clip is hidden and its saved transforms restored, so a subsequent IN can replay. A manager allows at most one target; a target has at most one manager in this bounded profile.

The profile supports 256 managers per publication, durations greater than zero through 3600 seconds, finite appearance/viewport values, startPoint 1–9, Squeeze dimension 0/1 and Rotate magnitude through 9999 degrees. The target registration point is used for scaling/rotation. Runtime removal, noninvertible parent transforms or invalid type parameters report errors; there is no simulated completion for an invalid target. Restart stops timers and frees the Rust state before the shared Timer/Event services reset.

Blinds, Iris, Photo, PixelDissolve and Wipe remain absent; no placeholder effect constructors are registered. Custom Transition subclasses, arbitrary easing callbacks, CustomEase, masks, historical private Transition methods, full component inheritance and exact legacy interaction between conflicting effects are outside this profile. There is no native-editor UI or AS3 expansion.

## Reuse and evidence

Transition contracts follow Adobe's [AS2 Components Language Reference](https://web.archive.org/web/20130325184910id_/http://livedocs.adobe.com/flash/9.0/main/flash_as2_components_reference.pdf), pages 1237–1256. The ignored reference PDF is pinned by SHA-256 `95a200ff72acb178048b92ee00a3fec7aba69c3c808b63a3df42ab32d48c7b01`.

The package reuses the existing Rust Penner easing adaptation, existing Rust Timer state, display transform engine and AS2 EventDispatcher. The easing implementation is based on the BSD-3-Clause [Robert Penner equations](https://github.com/danro/tweenman-as3/blob/f28c395213c021be9d64cc3c4e4c196c69b5f69e/Easing.as), pinned at `f28c395213c021be9d64cc3c4e4c196c69b5f69e`; its existing notice remains in THIRD_PARTY_NOTICES.txt and details in `docs/RUNTIME-EASING-PROVENANCE.md`. No third-party animation engine, new dependency or foreign-language algorithm implementation is introduced. Transition composition/state is new owned Rust code.

[as2-transitions-batch.nflash](../examples/as2-transitions-batch.nflash) displays five MovieClips and runs their IN/OUT sequence. Representative browser observations covered visible animation, saved appearance and completion boundaries; every easing/parameter combination remains unverified.
