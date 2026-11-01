# 0037 — Timeline sound: Rust decides, hosts schedule

Status: accepted, 2026-11-01.

Flash frame sounds (Sync Event / Start / Stop / Stream) must behave the same in editor playback, Test Movie and published HTML5, while audio output differs per host (Web Audio in the browser, Core Audio in the editor).

Decision:

- **Model.** `Keyframe.sound: Option<FrameSound { resource, sync, loops, loop_forever, effect }>` in document format 6 with serde defaults. Validation requires an existing `audio/*` resource and a repeat count of 1–10,000. Effects are the Flash presets only; custom envelopes wait for the Edit envelope dialog (DL-09). Sound layers are derived (keys hold sounds and no artwork), not stored.
- **Runtime.** The `Player` emits `SoundCommand`s (`play`, `stop`, `stream_start`, `stream_stop`, `stop_all`) when frames are entered and when stream state changes, never from authored evaluation. A Stream is keyed by placement path, layer and keyframe; it is continuous while each synchronised frame equals or follows the previous one, otherwise it restarts at `(frame − key) / fps`. A stream runs only while its timeline's clock and the transport are playing. Effect envelopes travel as `(position, left, right)` points computed in Rust.
- **Hosts.** `web/audio.js` decodes each embedded resource once on the publication's shared, gesture-unlocked bus and maps commands and envelopes onto buffer sources and gain automation; it decides only the host fact "is this sound still playing" for Start. The editor drains the preview player's commands and plays them with rodio 0.21 (Core Audio output, Symphonia MP3/WAV decoding); tests and screenshot captures never open a device. A new editor playback session silences the previous one; scrubbing is silent.
- **Script.** AS2 `stopAllSounds()` and AS3 `SoundMixer.stopAll()` call `Player::stop_all_sounds()`; streams resume at their offset on the next frame, as Flash streams do.

Consequences: one tested state machine (`crates/neoflash-core/tests/timeline_sound.rs`) defines sync behaviour for all hosts. Streams are not re-synchronised if a host drops frames (Flash drops frames instead). The editor gains MPL-2.0 Symphonia decoder crates through rodio; they are unmodified dependencies listed in THIRD_PARTY_NOTICES.txt and are not part of HTML5 exports.
