# 0008 — Correct ActionScript coverage to the complete declaration catalogue

Status: accepted, 2026-10-27. Supersedes the AS2/AS3 denominator in decision 0007.

The first dashboard used 35 AS2 and 36 AS3 capability groups. Although labelled as a curated registry, this did not answer the requested coverage against the existing reference catalogue and produced misleading language percentages. Remove those percentages and do not reinterpret the old counts as declaration measurements.

Import all 46,047 unique declarations from the four existing Adobe-reference inventories and join them exactly to the independent per-entry implementation audit. Preserve original IDs, scopes, URLs, evidence kinds, rule IDs and all four status states. Verify source catalogue/evidence hashes before import and preserve compact, sanitized provenance and source fingerprints. Keep the original detailed catalogues in the existing local research workspace; the normalized in-repository catalogue retains every row needed for reproducible counts and lookup.

The compact public page and README show 2,307 AS2 and 43,740 AS3 declarations with implemented-subset, partial, missing and needs-review counts. They do not display a global AS compatibility percentage. Full reference totals include AIR/Flex/component surfaces; show the core/language/runtime subtotals separately. Implemented-subset is not full API conformance, and architectural absence is not thousands of executed negative tests.

Methodology version 2 starts the declaration series. Exclude old AS family points from that series, retaining product history. Product areas still use bounded capability groups. Public JSON contains aggregates and audit freshness, keeping the full declaration list available locally without a multi-megabyte page download. All dashboard/report content is English.

Compute source freshness against the audited implementation fingerprints at the published revision. A changed implementation makes the counts a clearly dated snapshot; it never silently promotes or discards a status. Documentation-only differences remain separate. Later unshipped working-tree reviews do not become current implementation or fresh test evidence.

## AS3 priority classification

Keep product priority separate from implementation status. The existing reports additionally partition every AS3 declaration into Player foundations, subsequent local media, deferred Player services/advanced subsystems, AIR-only APIs, external libraries or unresolved availability. Ordered Player stages follow mathematics, objects/language, MovieClip, display/events, graphics, text, media and finally external/advanced facilities. The roadmap records remaining adaptation deltas; a dated missing decision does not automatically schedule an already existing method again.

Preserve the original catalogue/audit and all four status counts. A separate compact manifest pins source availability and priority assignments without importing the large local reference workspace into public builds. Validate its exact catalogue identity, provenance and partition totals. Resolve availability per declaration, retaining mixed Player/AIR classes and unresolved metadata; library runtime labels are not native-runtime evidence. Each immutable publication reads the manifest from its own revision. This adds planning dimensions, not a new coverage denominator, historical completion series or Flash compatibility percentage.
