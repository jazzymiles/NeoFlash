# Local SharedObject persistence and AMF values

Status: accepted for the bounded local persistence/AMF profile, 2026-10-27.

## One real binary codec

Rust encodes and decodes bounded AMF0/AMF3 values. The browser crosses WASM through a tagged object graph that keeps undefined, non-finite numbers, negative zero and object references explicit; JSON is only the bridge representation. It is not the persistence encoding. Arrays carry dense and named entries, with holes represented by undefined values. Common records, dates and ByteArrays can be read and written with shared references and cycles. AMF0 Date identity follows its inline wire representation. Source-class public fields/getters can flatten to anonymous records; aliases and class reconstruction, externalizable objects, XML/Vector/Dictionary values and native/display objects remain separate work.

ByteArray readObject consumes exactly one encoded value and advances by the decoder's consumed-byte count. writeObject uses the instance's selected ObjectEncoding. AMF wire order is independent of the ByteArray endian setting. Codec/materialization failure preserves the original buffer/cursor; nested ByteArray creation is transactional. Formats 0 and 3 are explicit, and static defaults apply to newly created instances. Bounds include 16 MiB encoded values, 100,000 graph items and 128 nested levels; the existing publication byte-buffer budgets still apply.

## Browser storage with Flash-shaped APIs

SharedObject stores an AMF payload inside a versioned localStorage envelope. Keys include the browser origin and publication pathname, or a valid explicit ancestor localPath, plus name and secure flag. Browser HTTP/HTTPS origins remain separate. File/opaque origins diagnose; this does not emulate desktop Flash storage or .sol files. AS3 flush returns the flushed status; AS2 returns true. Quota/access refusal is visible, without inventing the old permission dialog or claiming pending completion. Encoded AMF size excludes envelope and historical filesystem overhead.

getLocal returns a cached publication-owned facade. AS3 clear replaces its data object; AS2 clear empties its existing data object, reflecting the separately identified implementation profile. Clearing removes only the named record. Explicit flush is the reliable application boundary. Pagehide attempts a best-effort flush; abrupt termination cannot guarantee persistence. Restart invalidates facades and discards unflushed changes while preserving successful saves. Concurrent tabs use the last successful write; remote SharedObject, NetConnection and cross-tab synchronization remain out of scope for this batch.

One saved-game workflow combines both dialects, AMF object records with trailing data, Save, reopen/Restart, a refused storage write and Clear. Existing compiler/binary scenarios are extended, with one grouped Rust codec boundary test rather than per-member acceptance cycles.

References: [SharedObject](https://airsdk.dev/reference/actionscript/3.0/flash/net/SharedObject.html), [AS2 SharedObject](https://open-flash.github.io/mirrors/as2-language-reference/SharedObject.html), [ByteArray](https://airsdk.dev/reference/actionscript/3.0/flash/utils/ByteArray.html) and [ObjectEncoding](https://airsdk.dev/reference/actionscript/3.0/flash/net/ObjectEncoding.html). Original-player differential and broad AMF interoperability remain separate evidence.

Wire specifications: Adobe [AMF0](https://veovera.org/docs/legacy/amf0-file-format-spec.pdf) and [AMF3](https://veovera.org/docs/legacy/amf3-file-format-spec.pdf), mirrored by Veovera.
