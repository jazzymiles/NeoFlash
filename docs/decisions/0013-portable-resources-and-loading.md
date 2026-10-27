# Portable project resources and data loading

Status: accepted for the first resource/loading increment, 2026-10-27.

## Document ownership

A project stores resource files as `ResourceUnit { id, path, media_type, data_base64 }`. Paths are canonical relative logical names, independent of the author's filesystem. The serialized document contains bytes rather than machine paths or implicit adjacent folders. Names are unique without case distinctions; identifiers share the document entity namespace. MIME metadata is a bounded type/subtype token. Standard base64 is checked strictly, including padding and trailing bits.

Documents containing resources use format version 3. Versions 1 and 2 remain readable, and projects without resources need no migration. The version boundary makes older editors refuse the new document instead of silently discarding its files. Limits are 1,024 resources, 16 MiB decoded per resource, 64 MiB decoded in total and 128 MiB serialized document size, checked on save and load.

Resource data uses immutable shared `Arc<str>` backing in native snapshots. Ordinary Undo and saved-state comparison share the bytes. The editor compares its current model to a saved model snapshot instead of serializing every resource on every dirty-state query. Resource imports validate a complete candidate before applying one history transaction. Metadata edits and removal use the same document history; original files are never modified.

## Library and clipboard

The Library contains Resource files alongside Symbols. Authors import one or several local files, edit the logical project path/content type, or remove a file. File reads are bounded before model insertion. Import does not place an image on Stage or add a second graphics renderer; resources are available to scripts and form the storage foundation for later image/audio/video authoring.

Clipboard capture conservatively includes all resources because a script can compute a URL at runtime. Exact matching path spelling/type/bytes reuse a destination resource; case-only path collisions and conflicting content is rejected before changing the destination. Resource identities remap with other document identities. The existing 8 MiB clipboard transfer cap remains explicit: oversized payloads fail rather than dropping bytes. No local file handles or filesystem reads occur while parsing clipboard data.

## Browser service

The shared publication owns one data-loading service. An exact relative resource path resolves embedded bytes first and works when opening the exported HTML directly. Other requests use ordinary browser HTTP(S), CORS and same-origin credential rules. The service supplies the bounded AS3 URLRequest/URLLoader/URLVariables family and AS2 LoadVars profile; supported formats and transport restrictions are stated in COMPATIBILITY.md. Authored URLs never provide access to the author's desktop files.

Callbacks run asynchronously, including embedded resource completion. Each request belongs to a publication generation and operation token. Replacement, cancellation, failure, Restart and disposal prevent late results from updating a new session. Event listeners reuse the existing EventDispatcher rather than a second event system. Binary results use the session ByteArray service with bounded host bulk conversion; their data remains independent of the immutable document resource.

Published HTML carries embedded resources inside the existing bundle and remains self-contained for its embedded content. External requests depend on the requested server and browser access rules. Returning text or bytes does not imply JSON/E4X APIs, BitmapData, media playback, font embedding or a filesystem API.

## Acceptance

Extend the existing project-family runner with one ordinary publication combining embedded text, a binary packet, URL-encoded fields, AS2 callbacks, visible data output, cancellation, an adjacent error and Restart. Exercise an actual local HTTP transport in that same scenario, including supported request bodies; deterministic pending operations verify abandoned-session suppression. Reuse existing core/editor scenarios for resource preservation, atomic conflicts and Undo. One shared regression pass and one aggregate browser pass cover the delivery; no per-method acceptance loops.
