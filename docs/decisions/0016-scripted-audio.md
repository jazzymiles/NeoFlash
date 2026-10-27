# Complete-file scripted audio

Status: accepted for the first Sound family, 2026-10-27.

## Ownership and browser backend

Portable ResourceUnit bytes remain the document representation. Rust continues to own document validation, script compilation, export and authored timeline evaluation. A publication-owned Web Audio service handles actual browser audio; a synthetic Rust audio scheduler would not reproduce browser decoding, device clocks or gesture restrictions. Editor scrubbing stays side-effect free. Native audio tracks and timeline synchronization require a separate authoring design.

AS3 Sound/SoundChannel/SoundTransform and a separate AS2 Sound adapter reuse URLLoader, ByteArray and shared event dispatch. Complete-file loading supports standard MPEG-1/2/2.5 Layer III MP3 and mono/stereo PCM/float WAV. Playback begins after decoding and a real browser gesture. Channels own position, stop, bounded repeats and copied volume/pan values. Natural completion dispatches once; explicit stop suppresses it. AS2 start offsets are seconds and loops are total plays, while AS3 offsets are milliseconds and loops count repeats. Web Audio stereo panning does not claim exact Flash mixer equivalence.

A lazy context and zero-initialized publication gain prevent pending channels from starting before activation. The exported preview offers Enable audio and Mute/Unmute controls; trusted authored playback can resume the context. Restart stops/disconnects channels and closes the context before resetting resource and binary services. Old owners are invalidated by a session generation. Non-abortable native decoding keeps its reservation until settlement, even across Restart.

## Bounded decoding

The existing loader permits at most 16 MiB per file. Metadata inspection counts complete MP3 frames or WAV sample blocks before decodeAudioData, reserving float PCM at the actual output sample rate with conservative padding. It does not trust VBR metadata to reduce the reservation. Decoded buffers must fit their reservation and the shared 128 MiB budget. Active/pending channels are limited to 32; Sound owners to 256 until Restart. Browser decoder scratch memory and implementation details are outside the retained-buffer accounting.

Unknown/compressed WAV profiles, free-format MP3, changing rates/channel layouts, malformed frames and unsupported formats fail explicitly. This inspection is not a decoder and cannot establish historical Flash codec fidelity. Streaming, ID3 properties, native subclassing, sound linkage, sampleData/extract, four-channel mixing matrices and a MovieClip hierarchy mixer remain unsupported.

## Verification cost

One original MP3 publication extends the existing project-family runner and browser batch. The host integration uses an injected silent scheduler to observe ownership and callbacks, clearly separated from a single real-browser native decoder/offline PCM/lifecycle check. All live probe output passes through a permanent zero-gain bus set before any source connection; nonzero PCM is rendered offline only. No operating-system volume settings are changed. No per-method test suite is added.

Primary contracts: [AS3 Sound](https://airsdk.dev/reference/actionscript/3.0/flash/media/Sound.html), [AS2 Sound](https://open-flash.github.io/mirrors/as2-language-reference/Sound.html), [Web Audio](https://webaudio.github.io/web-audio-api/), [ID3 layout](https://id3.org/id3v2.4.0-structure), [WAVEFORMATEX](https://learn.microsoft.com/en-us/windows/win32/api/mmeapi/ns-mmeapi-waveformatex). MP3 header field formulas were checked against the public-domain [minimp3 implementation](https://github.com/lieff/minimp3); no decoder code is embedded.
