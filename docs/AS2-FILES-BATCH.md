# AS2 FileReference and FileReferenceList

This package brings 28 catalogue declarations to a bounded browser profile: 21 FileReference declarations and seven FileReferenceList declarations. They comprise two constructors, nine methods, six properties and eleven events. File creation time and Macintosh creator codes remain unsupported because browsers do not expose them. These counts describe implementation breadth, not 28 independent browser tests or full Flash conformance.

## Selection and metadata

`FileReference.browse(types)` and `FileReferenceList.browse(types)` open a native browser file chooser from an actual user gesture. Rust translates AS2 `*.extension;*.extension` filters to the browser's accept list, deduplicates extensions and rejects unsupported patterns. Browser filters are user hints, not file-content validation. Description-based filter grouping and Macintosh `macType` codes are not emulated.

A selected file exposes its actual basename, size, extension and last-modified timestamp as `name`, `size`, `type` and `modificationDate`. No disk path is available. Date values are detached and use the existing Date adapter. A list selection returns FileReference objects with the same transfer behavior; `fileList` returns a detached array. The user may pick at most 64 files / 64 MiB in one list; each file is at most 8 MiB. A publication may own at most 256 file/list objects.

Listener ordering and object identity reuse the existing Rust AsBroadcaster service. Both classes implement `addListener`/`removeListener`, and selection/cancellation dispatch `onSelect`/`onCancel` with the originating object. Programmatic `cancel()` cancels an in-flight transfer or invalidates a pending picker; it does not synthesize a user-cancel event. Browser cancel-event support governs native chooser cancellation.

## Transfer

`FileReference.upload(url, field, false)` reads only the user-selected File, creates its multipart request in Rust, and sends it through the existing URLLoader/ByteArray transport. It supports the default `Filedata` field or a custom field, `Filename`, `Upload=Submit Query` and URL-encoded `postData`. POST fields are decoded with the existing owned Rust URI kernel. A boundary collision fails explicitly. Reserved fields cannot be overridden. Windows test-upload preflights are unsupported.

Uploads report `onOpen`, bounded progress, HTTP/I/O failures, `onComplete` and the server's text response through `onUploadCompleteData`. The fetch transport does not expose incremental upload byte counts: upload progress is zero at initiation and the selected file size after successful completion. It never labels response-download bytes as upload progress. Download progress uses the existing actual response-byte counters. Native `SecurityError` failures dispatch `onSecurityError`; ordinary opaque CORS/network failures remain I/O errors because browsers do not reliably distinguish them.

`download(url, defaultFileName)` uses `showSaveFilePicker`, writes response bytes to the selected handle and awaits the writable stream's close before `onComplete`. It requires a user gesture, secure context and a browser supporting File System Access (the intended target is Chromium). Unsupported browsers receive an actionable error rather than a simulated saved-file callback. `onSelect` identifies the chosen basename; size and modification date are populated from the actual saved File after completion. A provided `postData` changes the request to POST; otherwise it uses GET. Downloads are capped at 8 MiB before any writable is opened. Cancelling aborts the network request and any open writable; it cannot undo a file already committed by the browser.

One active operation per FileReference and one active chooser per service are enforced. Shared transport URL/CORS, cancellation and memory limits apply. Reset/disposal invalidates callbacks and selected-file references. No authentication dialog, arbitrary filesystem access, old Flash policy-file semantics, native subclasses, direct file-byte exposure, streaming upload, creationDate or creator metadata is claimed.

## Ownership and evidence

The authored implementation is split between `crates/neoflash-core/src/as2_files.rs` (validation, filter conversion and multipart/form-data assembly) and `web/as2-files.js` (browser chooser/File handles, browser save facilities, reference wiring and existing transport). No third-party file-transfer engine or foreign-language algorithm was embedded. It follows the [AS2 FileReference contract](https://open-flash.github.io/mirrors/as2-language-reference/flash/net/FileReference.html), [FileReferenceList contract](https://open-flash.github.io/mirrors/as2-language-reference/flash/net/FileReferenceList.html) and [RFC 7578](https://www.rfc-editor.org/rfc/rfc7578.html). Existing Rust URI and broadcaster provenance remains in their respective source modules and notices.

One native family scenario, `as2_file_transfer_family`, checks grouped filter/filename validation, binary multipart content, UTF-8/plus form decoding, reserved fields and boundary collisions. It passed once after module registration. An earlier pre-registration invocation selected zero tests and is not counted as a passing verification. JavaScript syntax and an ordinary HTML5 export passed.

The ordinary project `examples/as2-files-batch.nflash` has visible Choose one, Choose multiple, Upload, Download and Cancel controls. Its tiny authored input is `examples/as2-files-fixture.txt`. The optional local verification server accepts POST `/__as2_file_echo` and GET `/__as2_file_fixture`. Browser acceptance, including any unperformed native-save-dialog branch, is reported by the coordinating two-hour batch. The source review does not substitute for browser observations.

Coordinator browser evidence: actual single-file selection, a valid 35-byte multipart upload with the expected form fields and server response, the multiple-file chooser configured for multiple selection (one file selected), and clean Restart. The native save/download branch and cancellation during an active transfer were not exercised in this delivery.
