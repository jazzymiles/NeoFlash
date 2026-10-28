# 0008 — Verify real FLA imports through ordinary HTML5 publication

Status: accepted, 2026-10-27; extended for native binary conversion, 2026-10-28.

An importer success flag alone cannot establish that an old project still works. Retain unchanged, licensed upstream source bytes and a pinned reference, convert only a bounded supported subset, and publish through the same exporter used by authors.

The first case is the Ruffle corpus Adobe CS6 `avm1-frame-labels/test.fla`. Its nine AS2 sources, solid contours, labels and nested instance remain editable. Its 17-line upstream trace is checked with the actual host/WASM and in a real browser, including Restart. We did not execute the original SWF in this acceptance run.

Unsupported topology, paints, metadata and language behavior produce explicit diagnostics. AS2 label lookup has a separate bridge so its bounded case folding, duplicate-label and missing-label rules do not change strict AS3/JavaScript behavior. Native goto function aliases remain explicitly unsupported. Source scripts are retained; XML text fragmentation must not drop content.

Native binary conversion reads bounded CFB, Contents, typed timeline and media records directly in Rust. The initial MX/CS4 profile requires complete supported records before constructing and validating a project. It does not require Java, Python or an intermediate XFL export. Unknown semantics retain explicit diagnostics; parser progress alone is never a conversion success.

Use document version 5 to preserve authored affine skew, gradient paths, inherited channel transforms and classic easing in the shared model. Keep nested symbol/layer structure and editable tween endpoints rather than baking every display frame. Reuse the runtime gradient representation and transform composition; native preview samples a bounded mesh and HTML5 uses Canvas brushes. Button state frames remain authored data, with separate invisible Hit geometry and stable runtime placement identity. The original archive is embedded as `legacy/source.fla`, alongside `legacy/provenance.json`; preserved bytes do not imply an editable adapter or lossless FLA re-export for every source property.

Retain source dialect identity. MX AS1 handlers use the existing bounded AVM1 frontend and capture a dynamic-name case profile backed by the shared Rust SWF table. Do not rewrite camelCase source identifiers or lowercase library instance names to make Clock execute. AS2/AS3 and ordinary JavaScript keep their own behavior. Complete AS1 lexical/prototype/coercion semantics remain outside this profile. Importing and authored evaluation do not execute scripts.

Keep source integrity checks, accepted/refused corpus expectations, focused regressions and browser evidence together. Add Clock and the 300×600 banner as native binary cases through the ordinary exporter, actual WASM host and browser; retain the [sample-specific acceptance limits](../COMPATIBILITY.md#legacy-file-conversion-and-source-fixtures). Missing fonts remain a nonblocking rendering difference. Extend acceptance case by case without claiming general XFL, binary FLA, original-player rasterization or ActionScript compatibility. The [import boundary](../IMPORTS.md) records the current hidden-layer and placement-lifetime rules.
