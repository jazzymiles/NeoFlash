# AS2 local Camera and Microphone

This bounded runtime package connects AS2 Camera and Microphone to local browser devices. It introduces no recording, encoder, server transport or AS3 API. Rust owns device identities, constraint plans, gain conversion, activity measurement/hysteresis and presented-frame FPS. JavaScript retains browser tracks, media elements, authored callback identities and the shared audio bus.

## Supported profile

`Camera.get(index)` and `Microphone.get(index)` return stable publication-local objects from the latest device snapshot, or null for an absent index. `names`, `index` and `name` reflect browser enumeration; labels can be empty before permission. Discovery is asynchronous in browsers. The explicit **NeoFlash extension** `Camera.refresh(callback)` / `Microphone.refresh(callback)` reports `(success, errorText)` after enumeration; it is not counted as historical AS2 support. The service also refreshes at startup and device changes. Calls before discovery has completed see the current snapshot, not a promise that all devices are known.

- Camera supports `setMode`, `setMotionLevel`, requested `fps`, actual `width`/`height`, measured `currentFps`, `activityLevel`, `motionLevel`, `motionTimeOut`, `muted`, identity properties and `onActivity`/`onStatus`.
- Microphone supports `setGain`, `setRate`, `setSilenceLevel`, `setUseEchoSuppression`, their readable properties, `activityLevel`, `muted`, identity properties and `onActivity`/`onStatus`.
- Authored `Video.attachVideo(camera)` renders the browser camera through the existing Video scene path. Null detaches and retains the last frame. Multiple local consumers share the device; the final detach stops its tracks.
- `MovieClip.attachAudio(microphone)` connects a live microphone to the existing publication audio output/mute bus. Null or false detaches. Removing the MovieClip or restarting releases the capture. Per-MovieClip sound transforms are not newly implemented.

Capture begins only from an actual user gesture and is subject to the browser permission prompt. Discovery never requests capture permission. The delivered example starts microphone gain at zero. Requested camera dimensions are bounded to 1920×1080 and FPS to 120. Browser constraints may select a different actual capture size/rate. Requested `fps` remains distinct from measured `currentFps`; the latter requires presented-frame callbacks, with an explicit diagnostic when absent. A fallback redraw timer does not fabricate FPS.

Camera motion uses a bounded 32×24 RGB frame-difference measurement; microphone activity uses bounded PCM RMS, both with Rust threshold/time hysteresis. These are useful modern local measurements, not claims of the original Flash detector's exact output. Gain maps 0–100 to 0–2 in the shared audio graph. The sample-rate requests 5/8/11/22/44 use their conventional Hz values, while `rate` reports the actual browser track rate in kHz after acquisition. Echo suppression is a bounded mapping to browser echo cancellation; historical codec processing is not reproduced. Unsupported browser constraints or absent actual setting reports diagnose explicitly.

## Lifecycle and unsupported behavior

Denied permission, ended tracks and device failures produce status callbacks without invented streams. Before active sampling, `activityLevel` is -1 and active camera dimensions are zero. Cleanup cancels sampling/frame callbacks, disconnects audio nodes and stops every acquired track. Generation guards release late permission results after Restart and invalidate prior Camera/Microphone objects. Authored callback errors are reported separately from device acquisition failures; an `onStatus` callback may detach or restart without leaving capture running.

Camera encoder bandwidth/quality/keyframes/compressed loopback, Microphone codec/encodeQuality/framesPerPacket, native subclassing, NetStream publishing and capture attachment to a server stream remain unsupported and are not promoted in the catalogue. Device ordering and blank labels follow the browser; historical Flash global/default-device preferences, exact coercion and complete permission semantics are not claimed.

## References and example

Public contracts were consulted in Adobe's mirrored [Camera reference](https://open-flash.github.io/mirrors/as2-language-reference/Camera.html) and [Microphone reference](https://open-flash.github.io/mirrors/as2-language-reference/Microphone.html). The MIT option of [Ruffle at 07d3e430631333f80f754df01a968bdcd80d98c5](https://github.com/ruffle-rs/ruffle/tree/07d3e430631333f80f754df01a968bdcd80d98c5) supplied individual declaration/default/unsupported-contract references from `core/src/avm1/globals/camera.rs` and `microphone.rs`. Those upstream implementations are largely stubs; none was copied or counted as working support. Existing Ruffle attribution remains in THIRD_PARTY_NOTICES.txt. Downloads and exact SHA-256 provenance are ignored under `target/verification/as2-runtime-two-hour/capture-sources/`.

[as2-capture-batch.nflash](../examples/as2-capture-batch.nflash) demonstrates discovery, settings, attachment controls and cleanup. Recorded browser observations cover metadata, device enumeration, idle detach and Restart; the synthetic adapter scenario covers generated frames and PCM without hardware access. **Real hardware permission, camera images, audible microphone output and hardware quality remain unverified.**

## Related progressive NetStream extension

The existing shared Rust `ScriptAS2Stream` also gains `bufferTime`/`setBufferTime` policy, without a second media kernel. A finite nonnegative target up to 60 seconds gates AS2 progressive playback until the browser reports that much contiguous data, or the remaining end-of-file duration. The initial default is 0.1 seconds. Actual TimeRanges still determine `bufferLength`; setting a target does not fabricate buffered bytes. The gate is rearmed on seek and native waiting, and browser loaded-data/can-play/progress events resume playback when the threshold is reached. Changing the target while already playing applies at the next refill. AS3 behavior remains on its existing path.

Actual native completion now delivers AS2 `onPlayStatus({code:"NetStream.Play.Complete", level:"status"})`. Transition/switch statuses, FLV embedded cue/text metadata, server upload/publishing and unobservable server QOS statistics remain unsupported. No `getInfo` result is fabricated.

[as2-media-batch.nflash](../examples/as2-media-batch.nflash) demonstrates the progressive NetStream buffer target, measured FPS, frame freeze/resume and completion callbacks. Representative browser playback and Restart were observed with the silent fixture; audible quality and every control/error branch remain unverified.

Camera/Microphone `instanceof` type tests remain outside the compiler profile; device identity is checked through stable `get()` references and indices.
