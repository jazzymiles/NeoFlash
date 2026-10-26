# Import compatibility

The product goal includes FLA, XFL and SWF import. They are **not interchangeable formats**, and a file extension alone cannot establish support. The importer implements a deliberately limited XFL subset and a real, bounded binary FLA container/structure reader. Binary FLA and SWF cannot yet become editable NeoFlash projects. Inspection and successful conversion are separate operations.

| Format | Current state | Planned treatment |
| --- | --- | --- |
| NeoFlash source document | Implemented by the core project serialization/validation layer | Versioned source model, stable IDs, complete scripts, symbols, instances and timelines. |
| Uncompressed XFL | Limited conversion implemented | One scene, library symbols, layers, held keyframes, labels, nested instances, TRS matrices/alpha, basic rectangle/oval primitives, plain static text and preserved frame ActionScript source. |
| FLA containing XFL | Same limited conversion implemented | Stored/Deflate ZIP entries are read in memory; the archive is never extracted. Path, symlink, duplicate-entry and size checks precede conversion. |
| Historical binary FLA | CFB/MFC inspection, Contents prefix/settings/descriptors and nonempty binary shape preservation | Editable conversion is blocked: Contents extensions/suffix, contour assembly, scene semantics and many object/media payloads remain unsupported. |
| SWF | FWS/CWS/ZWS header/version inspection only | No decompression, tag parsing, playback, bytecode import or source reconstruction yet. Original files remain untouched. |
| SWC / external AS libraries | Planned | Linkage, dependencies, bytecode/source boundary and library versioning need their own design. |


## Using the importer

```sh
cargo run -p neoflash-import -- inspect /path/to/project.fla
cargo run -p neoflash-import -- inspect /path/to/uncompressed-xfl
cargo run -p neoflash-import -- import /path/to/project.xfl /path/to/new.neoflash.json
```

`inspect` prints a JSON report with detected format/version, `can_import`, statistics and source-located diagnostics. A recognized unsupported format is a successful inspection with `can_import: false`. Read/container failures return an error. `import` requires no blocking diagnostics and creates a new output; it refuses to overwrite an existing file.

Library API: `inspect(&Path) -> Result<ImportReport, ImportError>` and `import_project(&Path) -> Result<Project, ImportError>`. Accepted uncompressed entry points are the folder, `DOMDocument.xml`, or its `PROXY-CS5` `.xfl` stub.

## Exact initial conversion boundary

- XFL topmost-first layers become NeoFlash painter order. Frame duration/blank gaps, keyframe labels and nested library references are retained.
- Translation, rotation, scale, reflection and alpha are supported. Skew/singular matrices block conversion. The stored placement matrix is retained; custom authoring pivots are reported as a warning.
- Solid-fill rectangle/oval **primitive objects** are supported. Ordinary `DOMShape` edge geometry, all imported strokes, rounded corners, partial ovals, gradients and bitmap fills block conversion.
- A single-run, single-line, left-aligned static text object is supported with its font name, size and color. Text metrics/antialiasing can differ and always produce a warning. Rich/dynamic/input/TLF text and nonzero margins/tracking are blocked.
- MovieClip instances and Graphic loop/play-once/single-frame modes are supported. Nonzero loop/play-once offsets are blocked. Buttons, masks, guide/folder layers, audio/video/bitmap/font-library assets and effects are blocked.
- Frame ActionScript is retained verbatim, using unambiguous `ActionScriptVersion` 2 or 3 from `PublishSettings.xml`. Language ambiguity, document classes and external script configuration block import. Source retention does **not** certify execution compatibility; the separate compiler/exporter checks it.
- XFL motion/shape/object tweens are blocked in this version. Import baked keyframes instead; curve conversion remains planned. Object-attached legacy handlers also remain blocked.
- Unknown semantic elements/attributes, missing resources, invalid values and invalid resulting project graphs produce blocking diagnostics. Library-folder/swatch organization and Flash publish settings are not retained and are reported.

XML is parsed with DTD disabled and a 100,000-node limit. Sources are bounded to 10,000 files, 8 MiB per file and 64 MiB total; directories have a depth limit of 16. ZIP paths and symlinks are rejected without filesystem extraction. Scripts are never executed by the importer.

## Binary FLA reader boundary

`ImportReport.binary_fla` contains the CFB major version, Contents revision, save-target name, storage/stream counts, logical byte lengths and per-stream structural progress. Stream identifiers are numeric; library names, source strings, media and filenames inside streams are not emitted. Ordinary compound documents are not classified as FLA merely because their extension is `.fla`: a root Contents stream and a recognized scene/symbol stream are required. An eight-byte signature with no valid container now returns a container error.

The reader uses the MIT-licensed Rust `cfb` 0.15 crate in **strict** mode. CFB v3/v4, regular streams and mini-streams are read from a bounded immutable memory snapshot; nothing is extracted or executed. Budgets are 64 MiB for the file and total logical streams, 8 MiB per stream, 10,000 directory entries, storage depth 16, FAT allocation 4 MiB and DIFAT allocation 1 MiB. Extra allocated FAT sectors are allowed within those budgets. Allocation/directory failures return an error, not an empty project. Strict rejection can represent a compatibility issue with an old writer and is not proof that the original application cannot open the file.

The MFC reader follows the combined class/object reference table, short/extended tags, null list terminators, schema checks and bounded CString lengths. It stops sequentially at the first unsupported or invalid record in each stream; it never searches for a later byte pattern to guess a boundary. Limits are 50,000 references/collection entries, object nesting 32 and strings 1 MiB. UTF-16 strings are validated; legacy ANSI strings are bounded but not decoded using a guessed code page.

Currently decoded **structures**, without editable conversion:

- `CPicObj` revisions 0–5, `CPicPage` 0–7 and `CPicLayer` 0–11: bounded child lists, registration words, layer metadata, guide tables and related-object references.
- `CPicShape` 0–6 with shape-data revisions 0–5: matrix words, fill/stroke tables, straight/quadratic delta edges and auxiliary curves are decoded into preservation IR. This includes inherited geometry inside `CPicFrame` records.
- `CPicFrame` 0–3 and 19–26 when its inherited shape and referenced payloads are supported. Durations, optional sound metadata, labels and empty/default easing records are consumed structurally. Historical action records, custom easing and nonempty compiled commands stop explicitly. Selected source-action encodings are bounded and consumed without executing or publishing their strings.
- `CPicBitmap` instance revisions 1–2 without extension records. Referenced media payloads are still unsupported.

`structure_complete: true` means the supported reader consumed that stream's entire record structure. It does **not** mean the document can be imported, that the artwork was converted, or that its semantics were validated. Completed record counts are separate from the ordinary converted-project statistics. Contents now reads the supported scene/library/media descriptor lists and raw settings prefix, but stops before extensions/suffix. Consequently every binary FLA still has `can_import: false`, including a file whose individual scene stream is fully understood. No partial project is returned.

### Contents prefix and document settings

`read_binary_contents(&Path) -> Result<BinaryContentsDocument, ImportError>` is a separate, explicit opt-in preservation API. It returns the ordinary inspection plus `BinaryContentsPrefix`: the entire exact `original_bytes` stream (including unread suffix), completed descriptor spans, encoded name spans, a combined MFC identity table, scene/library/media object-ID lists, selected-scene identity, raw counters and optional `BinaryDocumentSettings`. It contains private names, metadata and potentially embedded code/media in its raw bytes. It is not emitted by ordinary `inspect`, and scripts are never executed. Ordinary reports expose only `ContentsCounts`, byte progress and static stop diagnostics.

The supported Contents revisions are `0x14` (Flash 3), `0x20` (Flash 5), `0x29` (MX) and `0x3F` (Flash 8). The reader consumes the versioned header, counted null OLE pointers, reserved fields, null-terminated descriptor lists and selected-scene pointer in their actual order. Short and extended MFC tags share one class/object table throughout the Contents stream. A selected-scene pointer must identify an existing scene-list object. Descriptor targets resolve case-insensitively to the immutable root CFB inventory; missing streams, wrong scene/symbol/media roles, repeated identities and duplicate target ownership stop explicitly. No filesystem path is opened from an embedded name.

Schema-1 `CDocumentPage` revisions 0–25 are traversed except revision 9's historical action lists. Supported media descriptors are `CMediaElem`/`CMediaVideoStream` base revisions 0–7, `CMediaBits` subrevisions 0–4 and `CMediaSound` 0–10. Nested library metadata 0–7, linkage 0–7/auxiliary 0–3, property lists 0–2/properties 0–3/values 0–8 (including tables), smart-clip movie 0–3 and component 0–8 have bounded versioned layouts. Known persistent-data scalar/array/string/blob layouts are traversed. These nested fields remain exactly preserved inside descriptor byte spans; their semantic interpretation is not claimed. Non-null OLE objects, historical descriptor actions, nonempty embedded component bitmaps, unknown classes/schemas/revisions/value layouts stop at their first unproven boundary. Length-delimited smart-clip data stays opaque.

Settings are exposed only after the whole supported prefix is read. They retain the original four bounds words in `xmin,xmax,ymin,ymax` order, grid/print flags and words, printer-data spans, background/grid RGBA words and FPS fixed-point word. `size_twips()` computes positive extents with 64-bit subtraction and returns `None` for undefined (`xmin == i32::MIN`), inverted or zero-sized bounds. It never sorts or clamps the stored bounds. Pixel dimensions are twips/20; FPS is the unsigned word/65536, preserving values such as 29.5. Color packing is low-byte R, then G, B, A; historical alpha interpretation remains unresolved. No integer rounding or normalization into a NeoFlash Project occurs.

A successful prefix always ends with `fla_contents_suffix_unsupported` and `structure_complete: false`, even if the supplied bytes end there: publish properties, folders, fonts, guides, linkage extensions and compatibility tails have not been decoded. Earlier stops retain completed descriptors and original bytes, with settings absent. Known field boundaries are distinct from full document completeness or editable scene ownership.

Contents has an 8 MiB stream limit, 1 MiB per encoded string, and a shared 50,000-item work budget for references, list members and counted collections. Counts and property-table products are checked before iteration/allocation; remaining bytes bound every read and opaque blob. Descriptor metadata nesting has a fixed, nonrecursive structure. ANSI strings are preserved without choosing a code page; UTF-16 strings are validated. Compact stream references use their separate bounded encoding, not CString parsing.

### Preserved binary geometry

`decode_binary_shape(&[u8]) -> Result<BinaryShapeRecord, BinaryGeometryError>` decodes exactly one shape body, beginning at the `CPicShape` revision byte, after its inherited `CPicObj` fields. It rejects trailing bytes. `read_binary_geometry(&Path) -> Result<BinaryGeometryDocument, ImportError>` explicitly retains complete shape bodies encountered before the first unsupported/invalid record in each stream. Each retained body has its stream index, byte offset, parsed fields and exact `original_bytes`. These APIs contain artwork and are separate from ordinary `inspect`, which only emits `GeometryCounts`.

The preservation representation retains:

- Legacy fill words; modern packed values/flags; gradient matrices, stops and extensions; bitmap media IDs; and the known-layout but semantically unresolved `0x20` paint extension.
- Stroke color/width/style words, optional cap/join/scale/hinting fields, miter word and stroke paint. Procedural stroke appearance is not synthesized.
- Edge encoding flags, all three deltas, inherited contour style words, absolute integer start/control/end coordinates and optional line flags. Style bit 15 is retained as selection; the remaining index is checked against its shape-local table. Out-of-range references stop before coordinate data is read.
- Auxiliary point words, optional approximation entries, boundary flags and independently optional start/end boundary points. Unresolved flags remain in the IR and original bytes.

The shape edge terminator determines the boundary; the metadata hint is never trusted as a count. Signed coordinate addition is explicitly wrapping, matching the archive representation. No pixel-unit conversion, alpha normalization, contour joining, fill direction, document placement or unsupported paint approximation is inferred here. A retained shape can belong to an enclosing frame/object that later stops, so inspect the stream's completeness and stop diagnostics before using it. An exact shape body does not establish an editable scene graph.

Shape decoding is bounded to 8 MiB input and 100,000 geometry/style items, with at most 15 gradient stops. Every fixed-width read checks the remaining bytes; counted collections are checked before allocation. Opt-in document retention has a separate 250,000-item budget across streams and stops explicitly when exhausted. Unknown revisions and malformed records are errors, with no resynchronization scan.

Save-target mapping from the independently researched Contents revision table:

| Contents byte | Save target |
| --- | --- |
| `10`, `11`, `14`, `1B`, `20` (hex) | Flash 1, 2, 3, 4, 5 respectively |
| `29`, `38`, `3F` | Flash MX, MX 2004, Flash 8 respectively |
| `43`, `47` | Flash CS3, CS4 respectively |

Unknown values stay unknown. This identifies the format saved, **not** the application/version that last edited the file. CFB's major version is a separate container property.

## Verification and migration work

Thirty-seven importer tests cover eight XML/ZIP/SWF cases, eight CFB/MFC cases, eleven Contents cases and ten binary geometry cases. Coverage includes container budgets/corruption, combined MFC references, complete simple timelines, unknown classes/schemas, quadratic/straight edges, contour style inheritance and selection, missing style indices, gradients/bitmap/unresolved paint boundaries, auxiliary boundary bits, signed coordinate wrapping, revision-dependent endings, every truncated prefix of original shape fixtures, trailing bytes, per-shape and shared document retention budgets, and aggregate-only inspection versus opt-in preservation. Contents coverage adds four save-target prefixes, every truncated prefix, selected-scene/class/object identity, exact suffix retention, UTF-16/CString and compact-name boundaries, counted OLE nulls, missing/duplicate/wrong-role stream references, all supported bitmap/sound subrevision gates, nonempty persistent metadata and property tables, fractional FPS/raw color, sentinel/inverted/wide bounds and shared allocation/work limits. Fixtures contain only original test data. Existing strict XFL behavior remains unchanged.

Initial signature inspection found 20 compound-binary FLA and 22 SWF (20 FWS, 2 CWS; SWF versions 3–8) in a local private corpus. The binary reader accepts 19 CFB containers and rejects one under strict red/black directory-tree validation (adjacent red nodes; writer compatibility requires investigation). Among the 19 accepted containers, save targets are Flash 3 × 6, Flash 5 × 10, Flash MX × 2 and Flash 8 × 1. It indexes 20 scene streams, 246 symbol streams and 23 media streams.

With nonempty geometry decoding, complete timeline streams increased from 7 to 75. Completed enclosing records now include 75 pages, 132 layers, 176 frames and 168 shapes. Separately, 394 shape blocks were decoded (including inherited geometry in frames that may later stop): 301 contain edges, with 501 fills, 208 strokes, 2,916 straight edges, 4,180 quadratic edges and 706 auxiliary curves. All 132 previous shape-style stops were passed. The next timeline stops are 142 unsupported MFC classes, 46 historical action records, two unsupported object references and one nonempty compiled action record. Contents and other unsupported data still block editable conversion for every accepted file. These counts measure structural progress, not visual fidelity or migration completeness.

The Contents increment reaches raw document settings in all 19 accepted containers: 289 complete descriptors resolve to 20 scene, 246 library and 23 media streams. Descriptor revisions are 4 × 60, 12 × 170, 17 × 31 and 23 × 5 for `CDocumentPage`; bitmap descriptors are base 3/subrevision 2 × 1, base 5/subrevision 3 × 21 and base 6/subrevision 3 × 1. All 19 stop explicitly at the still-unread Contents suffix. Geometry and enclosing timeline counts above remain unchanged. These aggregates do not publish dimensions, names, strings, code or artwork and are not a general compatibility guarantee.

No corpus filenames, content, code, media or derived artwork are included in this repository. The optional `inspect_binary_corpus` example writes only aggregate JSON, refuses to overwrite its output and never emits source/stream names. It also checks opt-in geometry retention and Contents preservation in memory against inspection counts; all 394 decoded shape blocks and 289 completed descriptors were retained on this corpus. Local reports belong in ignored `target/`; they are not fixtures or proof of general compatibility.

Next steps: decode Contents extensions/suffix and preserve instance/text/action records, assemble verified fill/stroke contours from the retained edges, resolve coordinate/alpha conventions and media references, then build a validated editable graph. Flash easing/motion curves, font/text-layout fidelity, linkage and the distinct SWF playback/reconstruction pipeline remain separate work.

An import result should include:

- The detected format/version and original project provenance.
- A list of imported symbols, instances, layers, frames, assets, fonts and scripts.
- Each unsupported feature with its source location, severity and suggested action.
- Whether a visual/behavioral difference is known, and whether original data was retained.
- Explicit failure for unreadable source, instead of an apparently successful empty project.

Keep the original document unchanged. Never execute imported scripts while inspecting, opening, scrubbing or rendering onion skin. User code runs only in the preview/execution path.

For SWF, compiled content may not retain all original authoring structure. Successful playback does not imply recovered layers, source text, comments, tween intent or original instance naming. An optional Ruffle preview can help validate behavior but does not itself provide FLA/XFL editing or native HTML5 output.

Fixtures should cover multiple original Flash generations. For each fixture, record the producing application/version, available source, expected symbol/layer/frame structure, fonts, script language and whether redistribution is permitted. Use small self-authored fixtures before large third-party projects.

References:

- [Adobe: working with document types](https://helpx.adobe.com/animate/desktop/introduction-to-animate/documents.html) describes XFL's XML document, per-symbol files and external assets, and distinguishes authored FLA from compiled SWF.
- [XFL schema project](https://github.com/jindrapetrik/xfl-schema) is useful research material for CS5-and-later XFL. Its author warns that the schema may be incomplete/inaccurate; it is CC BY 4.0, not an official compatibility guarantee.
- [Ruffle SWF implementation](https://github.com/ruffle-rs/ruffle/tree/master/swf) is a candidate Rust component for compiled content; evaluate a pinned revision against import fixtures before adoption.
- [Microsoft MS-CFB specification](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-cfb/53989ce4-7b05-4f8d-829b-d08d6148375b) defines the container, not the application-specific FLA payloads. [Rust CFB](https://github.com/mdsteele/rust-cfb) supplies the MIT-licensed container reader.
- [JPEXS FLA Convert revision table](https://github.com/jindrapetrik/jpexs-fla-convert/blob/0923d70870d11b236755af71337da8d0110a57aa/src/com/jpexs/flash/fla/converter/FlaFormatVersion.java) and its reader document independently researched binary layouts and distinguish binary Flash 1–CS4 from CS5+ XFL targets. Its LGPL-2.1 implementation was consulted for format facts; no source code was copied or linked into NeoFlash.
- Its pinned [shape-data reader](https://github.com/jindrapetrik/jpexs-fla-convert/blob/0923d70870d11b236755af71337da8d0110a57aa/src/com/jpexs/flash/fla/reader/ShapeDataDecoder.java) and [shape exporter](https://github.com/jindrapetrik/jpexs-fla-convert/blob/0923d70870d11b236755af71337da8d0110a57aa/src/com/jpexs/flash/fla/exporter/XflShapeWriter.java) provide primary implementation evidence for versioned field widths, edge deltas, style-index selection and optional boundary points. These are reverse-engineered layouts, not an Adobe guarantee. NeoFlash uses its own bounded parser and original synthetic fixtures; uncertain visual semantics remain preserved data.
- The pinned [Contents prefix reader](https://github.com/jindrapetrik/jpexs-fla-convert/blob/0923d70870d11b236755af71337da8d0110a57aa/src/com/jpexs/flash/fla/reader/BinaryFlaReader.java), [descriptor layouts](https://github.com/jindrapetrik/jpexs-fla-convert/blob/0923d70870d11b236755af71337da8d0110a57aa/src/com/jpexs/flash/fla/reader/DefaultFlaDecoders.java), [property layouts](https://github.com/jindrapetrik/jpexs-fla-convert/blob/0923d70870d11b236755af71337da8d0110a57aa/src/com/jpexs/flash/fla/reader/PropertyRecordDecoder.java) and [MFC string/reference layout](https://github.com/jindrapetrik/jpexs-fla-convert/blob/0923d70870d11b236755af71337da8d0110a57aa/src/com/jpexs/flash/fla/reader/MfcArchive.java) establish the independently implemented prefix boundaries. [Document settings accessors](https://github.com/jindrapetrik/jpexs-fla-convert/blob/0923d70870d11b236755af71337da8d0110a57aa/src/com/jpexs/flash/fla/reader/BinaryFlaDocument.java) and [XFL color conversion](https://github.com/jindrapetrik/jpexs-fla-convert/blob/0923d70870d11b236755af71337da8d0110a57aa/src/com/jpexs/flash/fla/converter/XflModelXml.java) corroborate fixed16 FPS, twips bounds and raw RGBA packing.
- [Ed Moore's MIT-licensed format notes](https://github.com/eddiemoore/fla-decoder/blob/1d368cbb74e2a7b2d17ca4d18941e187b202b6aa/docs/FORMAT.md) describe MFC object structure and field layouts. These are reverse-engineering notes, not an Adobe specification; some decoder paths use recovery heuristics. NeoFlash does not adopt those heuristics or infer support from another decoder's byte-consumption claims.
