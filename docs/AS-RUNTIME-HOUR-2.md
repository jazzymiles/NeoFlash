# AS2/AS3 runtime hour 2

28 October 2026. Baseline `69b42f3`, working branch `codex/as-runtime-hour-2`.
Authorized window: 07:04:01–08:04:01 UTC. Three subsystem agents implement coherent
families; the coordinator integrates, checks ordinary HTML5 exports and prepares
the main-branch publication. Editor UI, FLA and all other development remain paused.

## Delivered behavior

| Family | New methods | New properties | New member paths | Existing members moved to Rust |
| --- | ---: | ---: | ---: | ---: |
| Event families | 47 | 69 | 116 | 0 |
| Convolution, Bevel, GradientGlow, GradientBevel | 4 | 43 | 47 | 0 |
| DateTimeFormatter and Date | 11 | 3 | 14 | 54 |
| NumberFormatter / NumberParseResult | 5 | 16 | 21 | 0 |
| **Total** | **67** | **131** | **198** | **54** |

Separately: **29 new constructors, 97 new constants and one migrated Date
constructor**. The 54 migrations comprise 37 methods and 17 properties. There are
**zero new language constructs**. Four shared filter implementations expose the
same 47 members in AS2 and AS3; they are counted once in the delivery total, not
twice. There are 245 dialect-qualified new method/property exposures: 47 AS2 and
198 AS3. Catalogue declarations have their own denominator and include classes,
constructors and constants; their totals must not be compared directly to 198.

The event package supplies 22 concrete classes and three constant holders.
Forty-four clone/toString overrides share schema-driven implementations; they are
not 44 independent algorithms. Fields retain the appropriate value/reference
behavior. The existing EventDispatcher delivers these manually created events;
automatic browser gestures, sensors, camera, native IME and AIR event producers
are not implemented by adding their payload classes.

Date arithmetic, normalization and UTC string formatting now run in Rust.
DateTimeFormatter uses Rust pattern/state/formatting logic and the browser's locale
names/templates; browser timezone resolution, legacy parsing and locale services
remain explicit host facilities. NumberFormatter owns formatting, parsing,
grouping, digit substitution and state in Rust; Intl supplies only fixed-probe
locale metadata. NumberFormatter locale enumeration explicitly diagnoses.

The four filters render real vector/text/bitmap subtrees and support BitmapData
filtering through the existing bounded Rust pixel pipeline. Their blur and
composition profiles remain approximations, not original Flash pixel parity.

## Actual HTML5 acceptance

| Ordinary authored project | Observed result | Interaction |
| --- | --- | --- |
| `runtime-date.nflash` | AS3 36/36; AS2 group true | Run 1 → 2 → Restart → 1 |
| `runtime-event-families.nflash` | 37/37; 22 classes and 22 dispatched clones | Run 1 → 2 → Restart → 1 |
| `runtime-filter-extensions.nflash` | AS3 20/20; AS2 four-class clone group true | Original → alternate → clear → Restart |
| `runtime-number-format.nflash` | 19/19; English, French and Arabic output | Run 1 → 2 → Restart → 1 |

All four projects use the normal compiler/exporter and real Rust WASM in the
in-app browser. Screenshots, console observations and initial failures remain in
ignored `target/verification/as-runtime-hour-2/`; generated exports are in
ignored `dist/runtime-*`. Font fallback warnings remain. No native editor or FLA
acceptance is included.

The browser found a derived event constructor reading fields before its own
payload existed, and a Rust filter diagnostic escaping as a string rather than
a catchable native Error. Both were corrected in their shared adapters and only
the affected examples were repeated. The filter's intentional invalid assignment
still verifies atomic refusal. Initial failures are preserved as failures.

## Controlled verification

Four grouped Rust family tests were added and passed. The existing 27 compiler
tests and 90 execution fixtures passed. One completed shared run covered all
24 existing project workflows plus their common prelude. Its first attempt
stopped in the prelude because the test harness did not pass the new Date service
to a directly evaluated script; the harness now uses the same service as exports.
No 24-workflow rerun followed the two unrelated browser fixes.

The progress tooling has one additional grouped validation test; all 25 existing
and new progress tests passed. Clippy passed after one mechanical match-guard
cleanup. Formatting and report consistency are checked for delivery. No per-API
browser jobs, original-Flash differential run or browser-engine matrix was added.

Recorded automated test commands took about **42.6 seconds**, including the failed
prelude attempt and compilation; progress tests contribute another **3.4 seconds**.
Three incremental WASM builds took **24.3 seconds**. Exports, lint, browser
interaction, review and publication are separate costs. The final WASM is
**2,894,824 bytes**, up **237,358 bytes (8.9%)** from the previous hour, before
HTTP compression. No Cargo dependencies or lockfile versions changed.

## Reuse and efficiency

New runtime algorithms are Rust, with thin browser bridges for references and
browser facilities. Whole third-party engines and foreign-language implementation
libraries were not introduced. Ruffle's compatible individual event contracts and
bevel algebra were adapted; calendar arithmetic adapts Howard Hinnant's MIT date
algorithms. Exact license notices accompany exports. The older ActionScript
frontend still emits JavaScript for authored program execution; this is not a
claim that the whole existing project has become Rust/WASM.

Compared with the previous hour: **198 versus 192 new member paths**, and
**54 versus 117 existing-member migrations**. The new total is similar, while
this batch includes actual rendered filters and common event payload families.
The migration total is lower because more effort went into new behavior. Counts
measure bounded API breadth, not independent algorithms, implementation quality
or a universal speed multiplier. The useful pattern remains shared Rust kernels,
schema-driven families, one common regression pass and affected-only fixes.

## Catalogue and publication

The complete declaration catalogue is updated by explicit reviewed IDs, including
retrospective mapping of the previously completed reuse/runtime packages. Prior
browser evidence remains attached to its original source snapshots. Current
family acceptance does not imply that every property/constant was individually
executed, and no entry is promoted to full Flash compatibility.

The authored review record is
`docs/progress/reviews/2026-10-28-runtime-batches.json`. The review importer
preserves baseline hashes, previous per-entry decisions and superseded evidence.
It distinguishes coordinator reviews from earlier independent acceptance.
Untracked original research snapshots are not re-imported or overwritten.

The completed language, reuse and first runtime-hour commits are ancestors of
this batch and can fast-forward into `main`. The separate unfinished
`checkpoint/paused-text-viewport` remains excluded because editor work is paused.
The standard Progress Pages workflow publishes the updated catalogue statistics
after the requested main-branch push. The entire project returns to paused status
when this bounded delivery is complete; monitor automations stay paused.

Provenance and limits: [dates](RUNTIME-DATE-PROVENANCE.md),
[events](RUNTIME-EVENTS-PROVENANCE.md), [filters](RUNTIME-FILTERS-PROVENANCE.md),
[numbers](RUNTIME-NUMBER-FORMAT-PROVENANCE.md).


## Published catalogue delta

The explicit review covers 1,019 declarations: 518 from previous accepted packages
and 501 from this batch, including constructors, constants, class bindings and
Date migrations. It moves 795 missing and 102 needs-review entries to partial;
122 already-partial entries receive refreshed scoped evidence. No implemented
subset entry is changed. This is not 1,019 newly implemented APIs in this hour.

| Catalogue | Total | Implemented subset | Partial before → after | Missing | Needs review |
| --- | ---: | ---: | ---: | ---: | ---: |
| AS2 | 2,307 | 15 | 352 → 563 | 1,665 | 64 |
| AS3 | 43,740 | 15 | 568 → 1,254 | 42,308 | 163 |

All runtime feature work and browser acceptance completed before 07:40 UTC;
review, packaging and requested GitHub publication follow within the authorized
window. Initial browser failures and the failed test-harness prelude remain in
the local evidence. A later mechanical Rust cleanup was rebuilt, and all four
exports were republished with the final shared WASM; unchanged examples did not
receive another full browser cycle.
