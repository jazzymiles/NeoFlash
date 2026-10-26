# FLA corpus verification — 2026-10-27

## Real downloaded FLA: import through HTML5

The unchanged Adobe Flash CS6
[Ruffle frame-labels source](upstream/ruffle/avm1-frame-labels/test.fla) now imports
to [this native project](imported/ruffle-avm1-frame-labels.nflash). The exact FLA
bytes remain pinned by the manifest. This is a real upstream source file, not a
synthetic or stripped replacement.

| Current check | Result |
| --- | --- |
| Importer suite | 80 Rust tests passed, including the unchanged public FLA, metadata rejection, bounded geometry and eight semantic-preservation tests; importer Clippy with all targets passed. |
| Imported content | 550 × 400, 24 fps; 2 symbols, 3 layers, 11 keys, 3 authored elements, all 9 AS2 frame scripts retained verbatim. |
| Source geometry | Local square bounds ±21.25 px, opaque red fill, 1 px opaque black round stroke; dimensions/styles independently checked against the original SWF data. |
| Corpus | 21/21 cases passed: 7 successful imports and 14 explicit refusals. 70 hashed files, 951,014 bytes, including the new native project. |
| Real WASM and host | `web/tests/real-fla-frame-labels.test.cjs` imports the original FLA, compiles all nine scripts, compares all 17 trace lines with upstream `output.txt`, checks eight more stable ticks and repeats after reset. Passed. |
| Actual HTML5 | Current exporter generated `dist/fla-corpus/ruffle-avm1-frame-labels/`; opened in the in-app browser. The original red square and black stroke rendered. |
| Browser trace | Exactly 17 upstream trace lines on initial playback; Restart produced the same 17 again (34 total). No console errors. Root remained on UI frame 3 / 7. |

The original source uses missing-label no-ops, ASCII-only case-insensitive labels,
the earliest duplicate label and a root-captured function called by the child.
Those behaviors are tested separately from the strict JavaScript/AS3 controls.
Native goto function aliases and unsupported numeric frame ranges remain explicit
diagnostic boundaries; this is not a general AS2 compatibility claim.

Only the bounded ordinary-shape subset is added: a single strictly convex closed
straight polygon with integer twip coordinates, one solid fill and an optional
uniform normal round stroke. Curves, holes, concave/self-intersecting paths,
multiple paints and ambiguous stroke coverage still block conversion. Inactive
AS3 publish fields are warned about only for unambiguous AS2; active, malformed,
nested and namespaced script settings remain blocked. Unknown semantic XML is
not silently discarded. Script/static text includes every immediate XML text or
CDATA fragment, even after comments. Namespaced attributes cannot shadow plain
XFL attributes. Folder/swatch containers accept only audited empty markers;
nonempty contents produce blocking diagnostics.

New evidence is in ignored `target/real-fla/browser/`: first/restart console JSON,
exact comparison result and screenshots. Original SWF bytecode was not executed;
its retained trace is the independent upstream oracle. The actual browser runs
the imported AS2 source after NeoFlash compilation. The native editor bundle was
left to the main chat's separate UI verification.

## Initial authored-example verification

The following records the earlier collection pass, before the real FLA support
above. Its original 15-refusal boundary and 69-file count are historical; the
current corpus counts and compatibility are given above.

Checked locally in the NeoFlash working tree. Importer/exporter and the shared
browser WASM runtime were rebuilt from the working tree; this is a development
verification record, not an Adobe interoperability certification.

| Check | Result |
| --- | --- |
| Source integrity | 69 payload/companion/license files, 941,558 bytes; every SHA-256 and length matched. |
| Current importer | `python3 scripts/check-fla-corpus.py`: 21/21 cases passed after a locked importer build. |
| External FLA | 15/15 recognized: 8 binary FLA and 7 ZIP/XFL; all 15 explicitly refused editable conversion. |
| Original examples | 3 synthetic FLA plus 3 XFL-directory inputs successfully imported and exactly matched the stored native projects. |
| Refusal behavior | Blocked inputs returned exit status 1 with `Import blocked:` and created no partial output. Crashes do not count as expected refusals. |
| Overwrite/input preservation | Successful-import destinations could not be overwritten; every source hash matched again after all checks. |
| Reproducible fixtures | The authored generator was run twice; ZIP, XFL, native project and metadata bytes matched. |
| Actual WASM runtime | `node --test scripts/check-fla-examples.cjs`: 3/3 passed after `./scripts/build-web.sh`. |
| HTML5 exporter | Current exporter successfully produced all three original examples in `dist/fla-corpus/`. |
| Browser | All three exports opened and rendered in the in-app browser; no console errors were observed. |

## Runtime evidence

- **Primitives and holds:** real-WASM checks cover frames 0, 23, 24, 47, 48, 59,
  60 and 71; shape positions, blank spans, persistent captions, labels and alpha
  204 are checked. The browser showed the rectangle, translucent circle and
  captions. Precise boundary frames were verified in WASM rather than inferred
  from the timing of a browser screenshot.
- **Nested playback AS2:** checks cover live, looping, play-once and frozen
  instances at and beyond frame 24, including alpha 0.55. After stopping the
  parent, the independent child advances while the Graphic instance follows its
  stopped parent. Browser logs included `AS2: root frame 1` and
  `AS2: nested Pair frame 1`; all five visible Orb placements rendered.
- **Frame scripts AS3:** exact source strings and dispatch frames 0/12/24 are
  checked, including typed locals and `stop()`. WASM only queues the script:
  the isolated runtime test continues to frame 25 without executing it. The
  actual browser host printed `AS3: start`, `AS3: middle 5`, `AS3: done` and
  stayed on display frame **25 / 36** (zero-based frame 24), showing that the
  host executed the final `stop()`.

The browser correctly warns that Arial is referenced by name and not embedded.
The toolbar's global playback button can still say Pause when a frame script
has stopped the root MovieClip; the stable frame counter and script trace were
used to verify the timeline stop.

## Local evidence and boundaries

- Full inspection reports and summary: `target/fla-corpus/reports/`.
- Collection research and import-refusal logs: `target/fla-corpus-research/`.
- Browser screenshots and console evidence: `target/fla-corpus/screenshots/`.
- HTML5 output: `dist/fla-corpus/<example>/index.html`.

At that initial collection pass, those paths were ignored build/check outputs.
Source fixtures are kept in this
folder; no private user FLA was copied into this public corpus. External FLA/SWF
scripts were not executed during that initial pass. The upstream reference outputs are not NeoFlash
results. Full Flash compatibility, visual comparison against Adobe playback,
desktop-editor interaction and the full workspace test suite were not checked
as part of this fixture-only change. The three authored examples exercise the
listed supported subset; they do not convert or substitute for the 15 external
projects that were blocked at that time. The real FLA verification above
supersedes that historical boundary for one of those projects.

The main development chat was given the corpus paths, commands, current blockers
and verification results for integration with the shared development records.
