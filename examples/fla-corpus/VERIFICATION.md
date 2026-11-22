# FLA corpus verification

## Per-file multi-frame visual and behavioral verification (21 / 21)

Every entry in [manifest.json](manifest.json) (8 binary `.fla` files, 7 upstream `.fla`/XFL archives,
and 3 authored examples in both `.fla` and `xfl/` directory form) imports into a validated native
`.nflash` project (`python3 scripts/check-fla-corpus.py`) and is verified against the checked-in
per-file visual and behavioral contract in [behavioral-oracles.json](behavioral-oracles.json) by
`cargo test -p neoflash-export --test t471_corpus_21_visual_behavioral_parity`:

- **Multi-frame authoring stage (`editable_stage`):** enumerates every scene (`21` scenes), keyframe
  start, tween midpoint, span terminal frame, frame-script frame, and synchronized `Graphic` cycle
  boundary (`99` scene frames) plus multi-frame library symbol timelines (`143` library frames across
  `Button` states and nested/Flump `MovieClip` animations). Evaluates
  `AuthoredScene::evaluate_editable_stage_with_filters` and renders stage RGBA bitmaps via
  `neoflash_export::render_project_frame_rgba`, verifying draw counts, world-space geometry bounding
  boxes, non-background RGBA coverage, pixel bounding boxes, perceptual hashes, exact RGBA checksums
  on pure vector/bitmap frames, and 0-pixel identity across all three `.fla` ↔ `xfl/` pairs.
- **Multi-frame runtime playback (`AuthoredPlayback` & `SwfBytecodePlayer`):** steps
  `AuthoredPlayback::with_live_scripts` across multi-frame trajectories on all 21 entries (`9`
  explicitly marked `editable_vs_playback_diverges` where independent `MovieClip` timelines or
  `stop()`/`gotoAndPlay` scripts diverge from static authoring stage, and `12` matching static
  stage), and executes all `8` companion `.swf` files in `SwfBytecodePlayer` (`5 / 5` upstream
  `output.txt` Flash Player trace suites matching `82 / 82` lines with `0` unsupported diagnostics,
  plus `DefineMorphShape-MX.swf` morph-ratio progression, `DefineButton-MX.swf`, and
  `DefineText2-MX.swf`).
- **Pinned Adobe Animate 27.0.1.5 (`MAC 27,0,1,5`) reference bounds:** records stage dimensions,
  frame rates, background colors (`21 / 21` exact matches), and `42` captured authoring-frame SHA-256
  digests and RGBA MAE ceilings. Live pixel comparison against `target/artifacts/t463/pinned-final`
  runs automatically whenever the local capture directory is present and skips only the disk-only PNG
  diff in clean CI while enforcing all checked-in visual, geometry, and behavioral contracts.

| Case ID | Format | Sampled frames (`scene` + `lib`) | `AuthoredPlayback` vs `editable_stage` | Companion `.swf` (`SwfBytecodePlayer`) | Pinned Adobe MAE ceiling |
| --- | --- | --- | --- | --- | ---: |
| `ruffle-mx-button` | `legacy_binary_fla` | `1` + `4` (`TestButton` states) | matches stage | `avm1` (`1`f, `1` display) | `≤ 0.0502` |
| `ruffle-mx-static-text` | `legacy_binary_fla` | `1` + `0` | matches stage | `avm1` (`1`f, `1` display) | `≤ 0.1632` |
| `ruffle-mx-shape-tween` | `legacy_binary_fla` | `4` + `0` (`30`f morph) | matches stage | `avm1` (`30`f, `28` morph ratios) | `≤ 0.9595` |
| `ruffle-avm1-frame-labels` | `xfl_archive` | `7` + `2` | diverges (`8` preview traces, `stop()` @ f3) | `avm1` (`17 / 17` `output.txt` lines) | `≤ 0.0567` |
| `ruffle-avm1-clip-events` | `xfl_archive` | `4` + `0` | diverges (`12` preview traces, `stop()` @ f3) | `avm1` (`19 / 19` `output.txt` lines) | `0.0000` |
| `ruffle-avm1-nested-button-text` | `legacy_binary_fla` | `1` + `0` | matches stage | `avm1` (`0 / 0` `output.txt` lines) | `≤ 0.2422` |
| `ruffle-avm2-button-frame-scripts` | `xfl_archive` | `2` + `4` (`MyButton` states) | matches stage | `avm2` (`28 / 28` `output.txt` lines) | `≤ 0.0837` |
| `ruffle-avm2-nested-construction` | `xfl_archive` | `1` + `2` (`FirstChild`) | diverges (independent `MovieClip`) | `avm2` (`18 / 18` `output.txt` lines) | `≤ 0.2042` |
| `flump-rotate-test2` | `xfl_archive` | `1` + `8` (`villager_01_dance` `25`f) | matches stage | — | `0.0000` |
| `flump-potion-simple` | `xfl_archive` | `1` + `41` (`potion` `110`f + `5` clips) | diverges (independent `MovieClip`) | — | `≤ 0.6580` |
| `flump-flump` | `xfl_archive` | `1` + `56` (`attack`, `defeat`, `explode`, …) | matches stage | — | `0.0000` |
| `flacomdoc-flash5-shapes` | `legacy_binary_fla` | `1` + `0` | matches stage | — | `≤ 0.1424` |
| `flacomdoc-mx-layers` | `legacy_binary_fla` | `1` + `0` (`37` layers) | matches stage | — | `0.0000` |
| `flacomdoc-flash8-scripts` | `legacy_binary_fla` | `1` + `0` | diverges (`2` AS2 traces) | — | `≤ 0.1424` |
| `flacomdoc-cs4-motion-tween` | `legacy_binary_fla` | `30` + `0` (`30`f motion arc) | matches stage | — | `≤ 0.1757` |
| `primitives-and-holds-fla` | `xfl_archive` | `9` + `0` (`72`f holds/blank) | matches stage | — | `≤ 2.6584` |
| `primitives-and-holds-xfl` | `xfl_directory` | `9` + `0` (`0`-px diff vs `.fla`) | matches stage | — | `≤ 2.6584` |
| `nested-playback-as2-fla` | `xfl_archive` | `5` + `13` (`Orb` `24`f, `Pair` `48`f) | diverges (`4` traces + child `MovieClip`) | — | `≤ 4.6129` |
| `nested-playback-as2-xfl` | `xfl_directory` | `5` + `13` (`0`-px diff vs `.fla`) | diverges (`4` traces + child `MovieClip`) | — | `≤ 4.6129` |
| `frame-scripts-as3-fla` | `xfl_archive` | `7` + `0` (`36`f labeled) | diverges (`3` traces, `stop()` @ f24) | — | `≤ 3.5991` |
| `frame-scripts-as3-xfl` | `xfl_directory` | `7` + `0` (`0`-px diff vs `.fla`) | diverges (`3` traces, `stop()` @ f24) | — | `≤ 3.5991` |

## Browser and WASM runtime checks

- **Ruffle `avm1-frame-labels` (`web/tests/real-fla-frame-labels.test.cjs`):** imports the unchanged
  Adobe Flash CS6 [source FLA](upstream/ruffle/avm1-frame-labels/test.fla) to
  [imported/ruffle-avm1-frame-labels.nflash](imported/ruffle-avm1-frame-labels.nflash), compiles all
  nine AS2 frame scripts, verifies all 17 trace lines against upstream `output.txt` (missing-label
  no-ops, ASCII-only case-insensitive labels, earliest duplicate label, and root-captured child
  callback), checks eight additional stable ticks, and repeats on Restart (`34` total lines, root
  parked on UI frame `3 / 7`).
- **Primitives and holds (`scripts/check-fla-examples.cjs`):** real-WASM checks cover boundary frames
  `0`, `23`, `24`, `47`, `48`, `59`, `60`, and `71`, verifying shape positions, blank spans,
  persistent captions, frame labels, and alpha `204`.
- **Nested playback AS2 (`scripts/check-fla-examples.cjs`):** verifies independent `MovieClip` vs
  synchronized `Graphic` (`Loop`, `PlayOnce`, `SingleFrame`) advancement at and beyond frame `24`,
  alpha `0.55`, and `AS2: root frame 1` / `AS2: nested Pair frame 1` traces.
- **Frame scripts AS3 (`scripts/check-fla-examples.cjs`):** verifies preserved AS3 typed locals,
  dispatch at frames `0` / `12` / `24`, traces `AS3: start`, `AS3: middle 5`, `AS3: done`, and
  `stop()` halting the root timeline on display frame `25 / 36` (zero-based frame `24`).

## Known boundaries and local evidence

- Non-zero MAE residuals on device-text and anti-aliased edge frames reflect rasterizer anti-aliasing,
  host font metrics when fonts are referenced by name rather than embedded, and bitmap resampling;
  pure vector/bitmap frames enforce exact pixel counts, bounding boxes, perceptual hashes, and FNV-1a
  RGBA checksums, while device-text frames enforce exact scene geometry plus bounded pixel/hash
  tolerances across hosts.
- Bounded native stage preview (`AuthoredPlayback`) reports explicit diagnostics when scripts use
  construct/unload clip events or dynamic method assignment (`ruffle-avm1-clip-events`,
  `ruffle-avm1-frame-labels`) and defers full bytecode/compiled execution to `SwfBytecodePlayer` and
  HTML5 `Test Movie`.
- Generated inspection reports (`target/fla-corpus/reports/`), HTML5 exports
  (`target/artifacts/fla-corpus/`), and optional local Adobe Animate captures
  (`target/artifacts/t463/pinned-final/`) stay under ignored `target/`. No private FLA files or
  bytes derived from private collections are stored or referenced in this corpus.
