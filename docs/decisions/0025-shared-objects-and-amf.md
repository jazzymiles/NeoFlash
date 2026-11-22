# Local SharedObject persistence and AMF values

Status: accepted for the bounded local persistence and native SOL exchange profile.

## One real binary codec

Rust encodes and decodes bounded AMF0/AMF3 values and Flash Local Shared Object (`.sol` / TCSO) containers. The browser crosses WASM through a tagged object graph that keeps undefined, non-finite numbers, negative zero and references explicit; JSON is only the bridge representation, not the persistence encoding. There is no JavaScript fallback codec. Missing native services request a runtime rebuild and project re-export.

Records, registered class aliases, dates, ByteArrays, XML/XMLDocument and shared/cyclic objects are supported. Arrays keep the dense prefix and actual numeric/named associative properties, so absent indices remain absent. AMF0 strict-array entries explicitly encoded as undefined remain present. ECMA-array header counts are retained by the Rust wire graph but do not impose a JavaScript array length; trailing holes may disappear. AMF3 has no separate total-length field. AMF0 Date/XMLDocument values are inline; AMF3 retains their reference identity. E4X XML and ByteArray use the standard AVMPlus bridge inside AMF0. Vector, Dictionary, XMLList and IExternalizable remain unsupported.

ByteArray readObject consumes exactly one value and advances by the consumed-byte count; writeObject uses the selected ObjectEncoding. Wire order is independent of ByteArray endianness. Codec/materialization failure preserves the original buffer/cursor, and releases newly allocated XML and ByteArray values. Source-class public fields/getters can flatten to records; registered aliases reconstruct classes. Full historical AS2 forged-constructor serialization is not claimed.

Bounds include 16 MiB encoded values, 100,000 graph items and 128 nested levels; publication XML and byte-buffer budgets also apply. Unsupported markers, invalid references, duplicate keys and malformed data produce diagnostics.

## Explicit native file exchange

The NeoFlash extensions `SharedObject.fromSolBytes`, `exportSolBytes` and `importSolBytes` exchange native AMF0/AMF3 SOL bytes in both dialects. Browser file inputs can supply bytes and Blob downloads can save them; these extensions do not scan Flash storage directories or add an editor file dialog. Import does not flush browser storage automatically.

A single codec transaction covers the complete header, object encoding, property stream and shared string/object/trait tables. Native AMF0 SOL numbers inline scalar slots as well as complex values; ordinary AMF0 ByteArray/remoting keeps its object-only reference table. The implicit SOL data envelope is not a wire object: a saved `data.self = data` can become a nested self-cycle rather than a reference to the newly created outer data record.

The 16 MiB file bound includes the complete header and name. Length mismatches, trailing bytes, invalid UTF-8, unsafe logical names and malformed records reject before replacing live data. Names have at most 1,024 UTF-16 units, without control/whitespace, reserved punctuation or empty/dot path components. Imported names remain logical storage identifiers, never filesystem paths. AS2 replacement preflights the existing data object and retains its identity; failed imports retain data and encoding. Constructor/setter reentry cannot commit into a changed or restarted session. Temporary decoded resources are released on rejection.

Five unmodified public Flash/Ruffle SOL fixtures cover native arrays, dates/XML, typed aliases and self-reference. Their pinned revisions, hashes and MIT license are in `crates/neoflash-core/tests/fixtures/sol/`. Additional authored wire cases check shared tables, the SOL reference profile and malformed/size boundaries. This is bounded interoperability evidence, not complete Flash Player conformance.

## Browser storage with Flash-shaped APIs

SharedObject persists an AMF payload inside a versioned localStorage envelope, separate from explicit SOL exchange. Keys include browser origin, publication pathname or a valid ancestor localPath, name and secure flag. HTTP/HTTPS origins remain separate; file/opaque origins diagnose. localPath rejects malformed escapes and dot traversal. AS3 flush returns the flushed status; AS2 returns true. Quota/access refusal is visible, without inventing an old permission dialog. Size counts AMF payload bytes, excluding the envelope and historical filesystem overhead.

getLocal caches publication-owned facades. AS3 clear replaces its data object; AS2 clear empties it in place. Clearing removes only the named record. Explicit flush is the reliable application boundary; pagehide is best effort. Restart invalidates facades and discards unflushed changes while preserving successful saves. Independent publications can use the [NeoFlash WebSocket relay](../WEB.md#share-live-state-between-published-movies) for ordered per-property AMF updates and server-side snapshots. Local persistence and native SOL exchange keep their existing representations. Adobe RTMP/FMS remains a separate transport.

The saved-game workflow covers both dialects, object records/cursors, SOL helper calls, save/reopen/Restart, refused storage and clear. The shared browser family additionally exercises real file input, Blob download/re-import, malformed-file recovery and reload/Restart persistence.

References: [SharedObject](https://airsdk.dev/reference/actionscript/3.0/flash/net/SharedObject.html), [AS2 SharedObject](https://open-flash.github.io/mirrors/as2-language-reference/SharedObject.html), [ByteArray](https://airsdk.dev/reference/actionscript/3.0/flash/utils/ByteArray.html) and [ObjectEncoding](https://airsdk.dev/reference/actionscript/3.0/flash/net/ObjectEncoding.html).

Wire specifications: Adobe [AMF0](https://veovera.org/docs/legacy/amf0-file-format-spec.pdf) and [AMF3](https://veovera.org/docs/legacy/amf3-file-format-spec.pdf), mirrored by Veovera. Native SOL reference facts also use the independently inspected, pinned rust-flash-lso writer listed in fixture provenance; no upstream codec algorithm is copied.
