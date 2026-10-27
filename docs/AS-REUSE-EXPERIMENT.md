# Rust algorithm reuse experiment

28 October 2026, local time (27 October UTC). Isolated branch: `experiment/as-reuse-30m`, baseline `694c37b14822a619b732872385b41fa8dafb909c`. The wider project and its monitors remain paused. This experiment changes scripting support and HTML5 publication; it does not resume editor or FLA work.

## Delivered and executed

| Measure | AS2 String/global family | AS3 geometry family | Total |
| --- | ---: | ---: | ---: |
| Newly supported functions/methods | 10 | 43 | 53 |
| Newly supported properties | 0 | 9 | 9 |
| Existing members moved from JavaScript to Rust | 14 | 0 | 14 |
| Constructors, counted separately | 0 | 2 | 2 |
| Constants, counted separately | 0 | 3 | 3 |
| New language constructs | 0 | 0 | 0 |
| Checks in the ordinary HTML5 scenario | 27/27 | 39/39 | 66/66 |

The result is **62 new partial method/function/property entries and 14 existing entries converted to Rust**. Constructors and constants are not folded into that headline. These are exercised API names, not complete specification contracts, independent algorithms, or a percentage of Flash compatibility. Eight legacy AS2 names share four kernels; getters/setters count as one property. The full catalogue retains its separately dated audit.

AS2 adds `escape`, `unescape`, `chr`, `ord`, `length`, `substring`, `mbchr`, `mbord`, `mblength`, `mbsubstring`. Existing `charAt`, `charCodeAt`, `concat`, `fromCharCode`, `indexOf`, `lastIndexOf`, `slice`, `split`, `substr`, `substring`, `toLowerCase`, `toUpperCase`, `toString`, `valueOf` move into Rust. Historical Flash case tables improve existing casing without creating extra API counts.

AS3 adds partial `Vector3D`, `Matrix3D` and `Utils3D.projectVector/projectVectors`: vector arithmetic and comparisons, matrix composition/inversion/rotation, typed raw data, scalar/bulk transforms and projection. The [member inventory](../scripts/reuse-as3-inventory.json) lists all 52 method/property entries.

## Actual HTML5 output

Both examples use the normal Rust frontend, project format, exporter and shared Rust WASM runtime:

```sh
bash scripts/build-web.sh
cargo run --locked -p neoflash-export -- examples/reuse-as2.nflash --output dist/reuse-as2
cargo run --locked -p neoflash-export -- examples/reuse-as3.nflash --output dist/reuse-as3
```

Browser acceptance observed AS2 run 1 → run 2 → Restart → run 1, always 27/27 checks and 24 exercised APIs. AS3 produced 39/39 checks and 52 exercised members, moved its marker from (220.0, 390.0) to approximately (214.6, 420.8), and restored the initial state with Restart. No browser errors were captured; both examples warn that Arial is referenced rather than embedded. The AS3 text label can display the existing text-focus overlay when clicked; its action executes. No editor input changes were made for this experiment.

Recorded local evidence lives under ignored `target/verification/as-reuse-30m/`: browser logs/screenshots, source hashes, AS2 inventory and timed commands. Generated exports are under ignored `dist/reuse-as2/` and `dist/reuse-as3/`.

## Reuse and ownership

Individual Ruffle Rust algorithms and historical case tables were adapted to bounded owned Rust values. Vector algorithms from Ruffle's AS source were translated into Rust. Matrix rotation/projection preserves the upstream OpenFL attribution. No complete engine, compiler, UI or renderer was imported; no Cargo dependencies were added.

[AS2 provenance](AS2-REUSE-PROVENANCE.md) and [AS3 provenance](AS3-REUSE-PROVENANCE.md) pin revisions, source paths and modifications. The selected MIT notices are retained in [THIRD_PARTY_NOTICES.txt](../THIRD_PARTY_NOTICES.txt) and included in HTML5 exports. Adapted upstream portions are attributed, not presented as wholly original work. Downloaded checkouts, archives and abandoned toolchains remain ignored.

All **new String and geometry algorithms/state** run in Rust/WASM. JavaScript adapters marshal values and preserve conversion order; previous AS2 calculation branches were removed. Existing authored ActionScript still uses the existing Rust compiler's JavaScript output and existing runtime facilities. This experiment does not replace the whole execution backend or claim that every pre-existing API already runs in Rust.

## Verification cost and comparison

Final recording: 23:06 UTC. The agreed Rust phase took 19.3 minutes through this record; total elapsed including the abandoned route was 28.57 minutes.

The previous experiment used one implementation agent and took 25.33 minutes through saving. It added eight named functions/methods plus several language constructs. Its 24.69 seconds of recorded automated commands did not include an actual browser acceptance run.

This experiment used two implementation agents in parallel plus coordinator integration. The initial external-toolchain route began at 22:37:44 UTC and was abandoned on the user's Rust-only clarification; none of its unexecuted API inventories count. The agreed individual-Rust-adaptation phase began at 22:47 UTC. Both feature packages were frozen around 22:58 UTC; agent review finished by 23:01:44 UTC. Shared builds, browser verification and saving followed within the original experiment window. AS3 reported approximately 10.5 minutes of implementation; AS2's Rust phase including review lasted 14m44s.

Verification added **two grouped Rust tests and two ordinary HTML5 example scenarios**. The existing execution batch stayed at 90 fixtures; three existing project workflows and their common prelude passed. Initial checks were repeated only after the adjacent API extension; a final small transport-error change received one focused check. Recorded automatic test commands took approximately 27 seconds, including their compilation, plus that subsecond check. Two WASM builds took approximately 14 seconds combined. Browser interaction and reporting are separate from these command durations. There was no full workspace, native editor, browser-matrix or original-Flash differential run.

The new-function/method output is **53 versus 8** in the previous experiment, with nine new properties and 14 conversions reported separately. The raw name count is 6.6 times larger; it is **not a measured 6.6× whole-project speedup**. Two agents worked this time, the families differ, eight names share four kernels, and the previous experiment also tackled syntax. Even dividing the raw count by two gives 3.3× more names per implementation agent, but task difficulty and active time still prevent treating that as a general productivity coefficient.

## Efficiency conclusions

The approach is effective for coherent standard-library families: proven algorithms and tables avoid rediscovering behavior, one Rust service carries many members, and one ordinary export exposes integration failures across the family. Most new geometry members share the same matrix/vector representation; the added projection methods reused that representation without another runtime layer. The two document-schema mistakes were caught by the normal exporter and fixed together with their examples.

The next authorized batches should favor similarly separable families: AS3 String and the existing Point/Rectangle/Matrix value APIs are candidates for individual algorithm adaptation and shared Rust implementations. Generate repetitive bindings/inventories from a reviewed API description where it removes duplication; keep semantics and ownership explicit. Display-list lifecycle, event ordering, classes, GC and full VM behavior require separate architectural work and cannot inherit the arithmetic family's throughput.

Keep the successful constraints: one owner per family, an early shared ABI, implementation across a family before polishing edge cases, one combined browser scenario, and a fixed feature cutoff. Count new capabilities separately from migrations and aliases. Do not resume the wider project or extrapolate a completion date from this single experiment.

Remaining partial boundaries include SWF5 behavior, full AVM coercion/prototypes, AS2 boxed strings, lone-surrogate URL codecs, malformed decoded UTF-8, finite-only geometry, bounded arrays, decomposition/interpolation/pointTowards, native subclasses and 3D display rendering.
