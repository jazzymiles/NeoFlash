# 0008 — Verify real FLA imports through ordinary HTML5 publication

Status: accepted, 2026-10-27.

An importer success flag alone cannot establish that an old project still works. Retain unchanged, licensed upstream source bytes and a pinned reference, convert only a bounded supported subset, and publish through the same exporter used by authors.

The first case is the Ruffle corpus Adobe CS6 `avm1-frame-labels/test.fla`. Its nine AS2 sources, solid contours, labels and nested instance remain editable. Its 17-line upstream trace is checked with the actual host/WASM and in a real browser, including Restart. We did not execute the original SWF in this acceptance run.

Unsupported topology, paints, metadata and language behavior produce explicit diagnostics. AS2 label lookup has a separate bridge so its bounded case folding, duplicate-label and missing-label rules do not change strict AS3/JavaScript behavior. Native goto function aliases remain explicitly unsupported. Source scripts are retained; XML text fragmentation must not drop content.

Keep source integrity checks, accepted/refused corpus expectations, minimal regressions and actual browser evidence together. Extend acceptance case by case; this decision does not approve general XFL, binary FLA or ActionScript compatibility. Hidden-layer publication policy and placement lifetime preservation remain separate P0 work.
