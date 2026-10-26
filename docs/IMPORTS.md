# Import compatibility

The product goal includes FLA, XFL and SWF import. They are **not interchangeable formats**, and a file extension alone cannot establish support. The importer implements a deliberately limited XFL subset and a real, bounded binary FLA container/structure reader. Binary FLA and SWF cannot yet become editable NeoFlash projects. Inspection and successful conversion are separate operations.

| Format | Current state | Planned treatment |
| --- | --- | --- |
| NeoFlash source document | Implemented by the core project serialization/validation layer | Versioned source model, stable IDs, complete scripts, symbols, instances and timelines. |
| Uncompressed XFL | Limited conversion implemented | One scene, library symbols, layers, held keyframes, labels, nested instances, TRS matrices/alpha, basic rectangle/oval primitives, plain static text and preserved frame ActionScript source. |
| FLA containing XFL | Same limited conversion implemented | Stored/Deflate ZIP entries are read in memory; the archive is never extracted. Path, symlink, duplicate-entry and size checks precede conversion. |
| Historical binary FLA | CFB validation, stream inventory, Contents save-target revision and bounded MFC timeline structure inspection | Editable conversion is blocked: Contents descriptors/settings, geometry, linkage and many object payloads remain unsupported. |
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
- `CPicShape` 0–6 with shape-data revisions 0–5 only when fill/stroke/edge/auxiliary tables are empty. Nonempty geometry stops explicitly.
- `CPicFrame` 0–3 and 19–26 when its inherited shape and referenced payloads are supported. Durations, optional sound metadata, labels and empty/default easing records are consumed structurally. Historical action records, custom easing and nonempty compiled commands stop explicitly. Selected source-action encodings are bounded and consumed without executing or publishing their strings.
- `CPicBitmap` instance revisions 1–2 without extension records. Referenced media payloads are still unsupported.

`structure_complete: true` means the supported reader consumed that stream's entire record structure. It does **not** mean the document can be imported, that the artwork was converted, or that its semantics were validated. Completed record counts are separate from the ordinary converted-project statistics. Contents currently yields only its first revision byte; all its remaining descriptors, settings and linkage remain blocking. Consequently every binary FLA still has `can_import: false`, including a file whose individual scene stream is fully understood. No partial project is returned.

Save-target mapping from the independently researched Contents revision table:

| Contents byte | Save target |
| --- | --- |
| `10`, `11`, `14`, `1B`, `20` (hex) | Flash 1, 2, 3, 4, 5 respectively |
| `29`, `38`, `3F` | Flash MX, MX 2004, Flash 8 respectively |
| `43`, `47` | Flash CS3, CS4 respectively |

Unknown values stay unknown. This identifies the format saved, **not** the application/version that last edited the file. CFB's major version is a separate container property.

## Verification and migration work

Sixteen importer tests cover the existing eight XML/ZIP/SWF cases plus eight binary cases: synthetic CFB v3/v4, mini/regular streams, exact save-target mapping, combined MFC references, complete simple timelines, every truncated prefix of a timeline fixture, trailing bytes, unknown classes/schemas, excessive nesting, file/stream/table budgets, directory cycles and legal extra FAT allocation. Fixtures contain only original test data. Existing strict XFL behavior remains unchanged.

Initial signature inspection found 20 compound-binary FLA and 22 SWF (20 FWS, 2 CWS; SWF versions 3–8) in a local private corpus. The subsequent real binary reader accepted 19 CFB containers and rejected one under strict red/black directory-tree validation (adjacent red nodes; writer compatibility requires investigation). Among the 19 accepted containers, save targets were Flash 3 × 6, Flash 5 × 10, Flash MX × 2 and Flash 8 × 1. It indexed 20 scene streams, 246 symbol streams and 23 media streams. Seven timeline streams were completely consumed; completed records across all streams included 7 pages, 13 layers, 30 frames and 2 empty shapes. Remaining timeline stops were 132 shape-style records, 118 unsupported MFC payload classes and 9 historical action records. Contents and other unsupported data still block editable conversion for all accepted files.

No corpus filenames, content, code, media or derived artwork are included in this repository. The optional `inspect_binary_corpus` example writes only aggregate JSON, refuses to overwrite its output and never emits source/stream names. Local reports belong in ignored `target/`; they are not fixtures or proof of general compatibility.

Next steps: lossless edge-path representation and import, Flash easing/motion curves, font/text-layout fidelity, media references, object handlers/linkage, versioned binary FLA readers, and a distinct SWF playback/reconstruction pipeline.

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
- [Ed Moore's MIT-licensed format notes](https://github.com/eddiemoore/fla-decoder/blob/1d368cbb74e2a7b2d17ca4d18941e187b202b6aa/docs/FORMAT.md) describe MFC object structure and field layouts. These are reverse-engineering notes, not an Adobe specification; some decoder paths use recovery heuristics. NeoFlash does not adopt those heuristics or infer support from another decoder's byte-consumption claims.
