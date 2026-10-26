# Import compatibility

The product goal includes FLA, XFL and SWF import. They are **not interchangeable formats**, and a file extension alone cannot establish support. The initial importer implements a deliberately limited XFL subset. Legacy binary FLA and SWF are detected and reported, but cannot yet become editable NeoFlash projects. Inspection and successful conversion are separate operations.

| Format | Current state | Planned treatment |
| --- | --- | --- |
| NeoFlash source document | Implemented by the core project serialization/validation layer | Versioned source model, stable IDs, complete scripts, symbols, instances and timelines. |
| Uncompressed XFL | Limited conversion implemented | One scene, library symbols, layers, held keyframes, labels, nested instances, TRS matrices/alpha, basic rectangle/oval primitives, plain static text and preserved frame ActionScript source. |
| FLA containing XFL | Same limited conversion implemented | Stored/Deflate ZIP entries are read in memory; the archive is never extracted. Path, symlink, duplicate-entry and size checks precede conversion. |
| Historical binary FLA | Compound-container signature inspection only | A dedicated reader is still required. The signature does not establish the exact Flash authoring version. |
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

## Verification and migration work

Eight self-authored fixture tests cover XML/ZIP conversion, symbol nesting, frame spans/layer order, source-script preservation, explicit unsupported content, missing text/skew, traversal/size rejection, malformed XML/DTD, and truthful binary FLA/SWF reports. Fixtures in `crates/neoflash-import/tests/fixtures/simple` contain only original test data.

A local private legacy corpus was inspected read-only: 20 compound-binary FLA and 22 SWF (20 FWS, 2 CWS; SWF versions 3–8). All 42 were recognized; all correctly report that editable import is unavailable. No corpus content or derived artwork is included in this repository. Binary FLA reading is therefore a priority separate from widening XFL support.

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
