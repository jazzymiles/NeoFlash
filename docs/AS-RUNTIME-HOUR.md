# Shared Rust runtime profiles

The shared Rust core supplies bounded standard-library services to AS2 and AS3:

- [Strings and URI codecs](RUNTIME-STRINGS-PROVENANCE.md): UTF-16 operations and literal replacement with explicit malformed-input boundaries.
- [Geometry and color](RUNTIME-GEOMETRY-PROVENANCE.md): finite matrices/vectors, AS2 Color, Motion Color and MatrixTransformer; standalone projection does not add a 3D renderer.
- [Easing, Tween and timers](RUNTIME-EASING-PROVENANCE.md): shared easing kernels, programmatic monotone CustomEase curves, Tween state, callback timers and AsBroadcaster.
- [Binary codecs](RUNTIME-BINARY-PROVENANCE.md): ByteArray compression, multibyte text, Base64 and hex. Text codecs use Rust encoding_rs; compression uses the Rust flate2 backend. LZMA remains unsupported.
- [Math](RUNTIME-MATH-PROVENANCE.md): shared numeric operations, with entropy supplied by the browser for random values.

Browser adapters marshal values and connect clocks, references and events. Authored ActionScript still executes through the existing JavaScript-emitting frontend. Full AVM conversion/prototype/error fidelity and every historical version remain unfinished.

Examples: [strings](../examples/runtime-strings.nflash), [geometry](../examples/runtime-geometry.nflash), [easing](../examples/runtime-easing.nflash), [binary](../examples/runtime-binary.nflash) and [Math](../examples/runtime-math.nflash). Each is an ordinary project for the standard HTML5 exporter.

Individual adapted algorithms retain their pinned sources and licenses in the linked provenance records. [THIRD_PARTY_NOTICES.txt](../THIRD_PARTY_NOTICES.txt) accompanies exports.

Coverage decisions and declaration counts are maintained in the [complete ActionScript audit](progress/ACTIONSCRIPT.md). These profiles describe partial support, not full Flash compatibility.
