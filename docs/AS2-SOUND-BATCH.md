# AS2 sound resources, metadata and channel matrices

This package adds five useful AS2 Sound declarations: three methods (`attachSound`, `getTransform`, `setTransform`), the `id3` property and the `onID3` event. Existing `start`, `setPan`, `getPan` and `setVolume` gain a matrix-aware path. These are partial profiles, not complete Flash audio compatibility. AS3 Sound and SoundTransform support is unchanged.

## Behavior

- `attachSound(id)` resolves an exact MP3/WAV **project resource ID** in Rust, then uses the existing embedded-resource loader and browser decoder. Missing/non-audio IDs fail explicitly. Attaching a replacement stops this Sound object's owned channels, cancels its old load and releases its old PCM allocation. Loading is asynchronous: use `onLoad` before playback. This is not imported FLA/SWF linkage and does not reconstruct MovieClip sound ownership.
- `getTransform()` returns a detached `{ll, lr, rl, rr}` value. `setTransform(object)` patches supplied own coefficient properties. Rust normalizes integer percentages, derives pan and combines volume with the four gains. Web Audio splitter/gain/merger nodes route the channels; changes also reach already playing channels without restarting their source.
- `lr` is right input to left output; `rl` is left input to right output. `setPan` replaces the matrix with ordinary left/right balance. Volume remains independent of the matrix. Supported coefficient/volume percentages are 0..100, pan -100..100; fractional values truncate on the matrix path. Historical overflow, negative/out-of-range amplification and full mixer hierarchy are excluded. Mono browser audio is duplicated into the two logical inputs before routing.
- AS2 complete-file loads parse ID3 in Rust. `id3` is undefined before metadata arrives. Each accepted tag produces a snapshot and `onID3`; a file with a v2 header and v1 trailer produces two callbacks, in that order, before `onLoad`. Loading and callbacks retain the existing publication-generation/lifecycle guards. No callbacks survive Restart or a replacement load.
- Text metadata supports ID3v1/v1.1 and v2.3/v2.4 common `T*` frames (except user-defined `TXXX`), comments and ordinary URL frames. Text decoding supports Latin-1, UTF-8 and BOM-tagged UTF-16/UTF-16BE. Standard title/artist/album/year/track/genre/comment aliases are supplied; comments aggregate under `COMM`. ID3v1 genre stays a numeric string. Non-text, user-defined, compressed/encrypted/grouped frames emit an explicit diagnostic and are excluded. Malformed tags, v2.2, unsynchronization and extended headers fail with actionable errors.
- Bounds: existing 16 MiB audio input, 1 MiB metadata tag, 256 frames, 16 KiB per text field, existing 128 MiB PCM / 32 simultaneous channels / 256 Sound objects. Audio remains complete-file playback through browser-supported MP3/WAV decoding.

`checkPolicyFile` remains missing in the compatibility catalogue: its default false can be inspected, but requesting Flash `crossdomain.xml` behavior throws an explicit browser-CORS diagnostic. False-only plumbing is not counted as a supported feature. Streaming, timeline audio, global/MovieClip sound ownership, linkage-selective stop and AS3 ID3/matrix APIs remain excluded.

## Source and reuse

The channel orientation, independent volume and matrix-to-pan semantics were reviewed and adapted from Ruffle's MIT-licensed `core/src/avm1/globals/sound.rs` and `core/src/display_object.rs`, pinned at `07d3e430631333f80f754df01a968bdcd80d98c5`. The existing Ruffle notice in `THIRD_PARTY_NOTICES.txt` applies. No Ruffle runtime, GC, audio backend or third-party parser is embedded. The ID3 parser is an owned bounded implementation based on [ID3v2.3](https://id3.org/id3v2.3.0), [ID3v2.4](https://id3.org/id3v2.4.0-structure) and the [AS2 Sound contract](https://open-flash.github.io/mirrors/as2-language-reference/Sound.html). Downloaded references stay under ignored `target/references/as2-two-hour/`.

## Example and verification boundary

[as2-sound-batch.nflash](../examples/as2-sound-batch.nflash) uses the authored two-tone MP3 with metadata and visible controls for silent playback, matrix changes and reattachment.

Recorded browser observations covered ID3 callbacks, ready state, silent playback, natural completion and fresh callbacks after reattachment. **Audible balance and quality remain unverified.**
