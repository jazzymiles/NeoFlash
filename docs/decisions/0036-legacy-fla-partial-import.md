# 0036 — Open every readable legacy FLA, reporting what is approximated

Status: accepted, 2026-11-01. Amends the all-or-nothing rule of [0008](0008-real-fla-acceptance.md) for binary FLA.

The first native binary profile converted a document only when every record was decoded and every feature had an exact model equivalent. On a local survey of 106 real Flash 3–CS4 documents that opened 5. The refusals were mostly not about missing semantics: a strict container reader rejected stale allocation metadata that Flash itself never reads, Contents layouts were gated by save-target revision, and one unrepresentable detail (a filter, a justified paragraph, a bitmap fill, a graphic first-frame offset, a second scene) refused the whole file.

Decision:

- **Container.** Read CFB the way Flash does: follow only the sector chains that live directory entries name. Stale DIFAT/MiniFAT slots, reserved timestamps and directory colors are tolerated and reported; cycles, out-of-file sectors, short chains and budget overruns remain errors. Stream bytes are never repaired.
- **Partial import.** A document converts when its Contents descriptors and document settings are readable. Everything the model cannot express yet is converted to its nearest representable form or kept as an explicit placeholder, and each case produces a warning diagnostic with a location. A timeline stream that stops at an undecoded record keeps every fully read layer and frame before the stop.
- **Never silent, never invented.** Placeholders are visible (an empty "Unsupported source object" instance keeps the slot and its timing). Repeated warnings are collapsed per code with a count. Keep exact source bytes in parts of up to 16 MiB. If the raw source cannot fit, try one lossless gzip stream split into ordered resources. Provenance records the reconstruction order, original size and digest. When neither encoding fits, the report identifies the original file to keep.
- **Structure first.** Scenes, symbols, folders, layer order and relations (mask, folder, motion guide), keyframe timing, labels, graphic playback offsets, classic and shape tweens and scripts map to their model equivalents. Approximations are limited to presentation (filters, blend modes, bitmap fills, stroke styles, text styling) and to records not yet decoded.

Errors (`can_import: false`) remain for: unreadable or unsafe containers, missing Contents settings, invalid resulting project graphs and unknown formats.

Consequences: success no longer means lossless. The import report is the fidelity statement, and `scripts/survey-legacy-fla.py` measures both refusals and approximations across a corpus. Exact-equivalence work continues per feature by removing warnings, not by refusing files.
