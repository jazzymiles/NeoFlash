# FLA corpus verification — 2026-10-27

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

Those paths are ignored build/check outputs. Source fixtures are kept in this
folder; no private user FLA was copied into this public corpus. External FLA/SWF
scripts were not executed. The upstream reference outputs are not NeoFlash
results. Full Flash compatibility, visual comparison against Adobe playback,
desktop-editor interaction and the full workspace test suite were not checked
as part of this fixture-only change. The three authored examples exercise the
listed supported subset; they do not convert or substitute for the 15 external
projects that remain blocked.

The main development chat was given the corpus paths, commands, current blockers
and verification results for integration with the shared development records.
