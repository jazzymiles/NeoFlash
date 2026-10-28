# AS2 two-hour subsystem batch

28 October 2026. Baseline `b3edf75`, branch `codex/as2-two-hour`.
Authorized window: **08:53:22–10:53:22 UTC**. Three subsystem agents and the
coordinator worked in one source tree. The native editor, FLA, AS3 feature work,
the wider project and monitor automations remain paused.

**Implementation and local acceptance complete.** Eight ordinary HTML5 families
passed their recorded browser scopes, and the shared regression failure passed
its focused rerun. The exact catalogue review is applied. Implementation commit
`ffa3bad` was fast-forwarded into `main`; further feature work remains paused.
Publication and remote validation are independently visible in
[GitHub Actions](https://github.com/jazzymiles/NeoFlash/actions) and the
[progress dashboard](https://jazzymiles.github.io/NeoFlash/progress.html).
No new family is authorized by this document.

Delivery publication was observed at `5b6b865`: both the
[macOS Rust/WASM/HTML5 check](https://github.com/jazzymiles/NeoFlash/actions/runs/40671064518)
and [Progress Pages deployment](https://github.com/jazzymiles/NeoFlash/actions/runs/40671064414)
completed successfully. The public dashboard was inspected in the browser and
showed that revision and the exact AS2/AS3 counts below. Later reference-book
ignore rules and this publication note do not change runtime source.

## New behavior and exact counts

| Package | New methods | New properties | New global functions | New API paths |
| --- | ---: | ---: | ---: | ---: |
| Collections, DataSet, local deltas and binding | 82 | 23 | 0 | 105 |
| Text styles, snapshots and display/input | 20 | 28 | 0 | 48 |
| Bitmap operations and displacement | 13 | 9 | 0 | 22 |
| FileReference / FileReferenceList | 9 | 6 | 0 | 15 |
| Locale tables and XLIFF loading | 10 | 2 | 0 | 12 |
| Dynamic Object and named references | 4 | 2 | 1 | 7 |
| Sound resources, metadata and matrices | 3 | 1 | 0 | 4 |
| Timeline variable loading | 1 | 0 | 2 | 3 |
| **Total** | **142** | **71** | **3** | **216** |

The 216 API paths are catalogue declarations, not 216 independent algorithms.
Common Rust kernels serve related methods, interfaces and returned objects.
The batch also adds seven documented constructors, 37 event declarations, eight
class entries, five interfaces, one actual constant (`newline`) and two syntax
entries (`instanceof`, `set`). The catalogue separately labels
`EndPoint.constant` and `DeltaItem.kind` as constants although they are instance
fields: these **two fields are reported separately**, not as new static constants
or additional method/property rows. All categories total **278 new declarations**.

Another **12 existing declarations are extended**: 11 methods and one event.
**30 declarations already had working support** and receive a scoped catalogue
correction: seven methods, ten properties and thirteen events. These are not
credited as new implementation. There are **320 unique reviewed IDs**, with no
duplicate IDs, unknown IDs, baseline-status mismatches or protected implemented
subset downgrades in the normalized inventory. Noncatalogue convenience entry
points remain outside the denominator.

## What the packages do

- Collections and DataSet preserve original object identity while Rust owns row
  IDs, sorted/filtered views, independent cursors, ranges, cancelable edits,
  nested event batches and coalesced local change packets. Primitive bindings
  connect object paths and explicit events, including guarded two-way updates.
- AS2 text supports uniform styled HTML, a bounded StyleSheet, additional
  rendered TextFormat fields, real static-text snapshots and their selections,
  text surfaces/scrolling and legacy input preferences/listeners. These are
  runtime facilities; the native authoring interface is unchanged.
- BitmapData gains clipped channel/palette/merge/difference operations, fill,
  threshold, alpha hit tests, deterministic noise/dissolve, bounded Perlin noise
  and bundled-image lookup. Displacement works through BitmapData filtering.
- FileReference families connect user-selected browser files, real metadata,
  owned Rust multipart uploads and supported native save handles. Browser
  activation, secure-context and transport restrictions remain explicit.
- Locale provides translation tables, configured language paths, environment
  language selection, callbacks and atomic Flash XLIFF plain-text ingestion.
- Dynamic Object adds accessors, watchpoints and missing-property resolution;
  named eval/set paths and source event blocks connect to existing publication
  objects. Timeline variable loading uses Rust form decoding and shared IO.
- Sound adds bundled resource attachment, bounded ID3 text metadata and actual
  channel matrix routing; existing volume/pan methods use that matrix path.

Detailed contracts and limits are in [data/binding](AS2-DATA-BATCH.md),
[text/display](AS2-TEXT-DISPLAY-TWO-HOUR.md), [bitmap](AS2-BITMAP-BATCH.md),
[files](AS2-FILES-BATCH.md), [Locale](AS2-LOCALE-BATCH.md),
[Object](AS2-OBJECT-BATCH.md), [variables](AS2-VARIABLES-BATCH.md) and
[Sound](AS2-SOUND-BATCH.md).

## Catalogue result

The exact reviewed declarations are applied locally. This does not assert that
GitHub Pages or the remote main branch has already been updated.

| AS2 status | Baseline | After review | Change |
| --- | ---: | ---: | ---: |
| Implemented subset | 15 | 15 | 0 |
| Partial | 745 | 1,054 | +309 |
| Missing | 1,501 | 1,201 | −300 |
| Needs review | 46 | 37 | −9 |
| **Total** | **2,307** | **2,307** | **0** |

Of 320 reviewed declarations, 300 move from missing and nine from needs review
to partial; eleven were already partial. The increase of 309 includes scoped
corrections of earlier implementation and is not a count of new API paths.
AS3 is unchanged: **15 implemented subset, 1,254 partial, 42,308 missing,
163 needs review**, total 43,740. No feature-group percentage substitutes for
the declaration catalogue.

The reproducible normalized input is
`target/verification/as2-two-hour/normalized-inventory.json`, with per-package
inventories and source hashes alongside it. Downloaded material remains ignored.
The tracked declaration review and generated reports preserve the per-entry
provenance. The approach is recorded in ADR 0035; publication remains with the
coordinator.

## Ordinary HTML5 evidence

These statuses describe the integration snapshot, not blanket acceptance of
every declared member. Exports use the normal compiler/exporter and real Rust
WASM; no separate acceptance job is created for each API.

| Ordinary source project | Observed result at this snapshot |
| --- | --- |
| `as2-object-batch.nflash` | 31/31 and Restart observed in a real browser |
| `as2-variables-batch.nflash` | 4/4, including actual HTTP POST |
| `as2-data-batch.nflash` | 14/14 observed in a real browser |
| `as2-bitmap-batch.nflash` | 14/14 and a visible clamp-mode change |
| `as2-display-batch.nflash` | 41/41 plus physical mouse, A and Tab input; trace press 1, mouse 1/1, key 2/1, focus 2/1 |
| `as2-sound-batch.nflash` | Two ID3 notifications; silent Play; one onSoundComplete; reattachment produced two ID3 notifications again |
| `as2-files-batch.nflash` | Actual 35-byte fixture selection and multipart POST; server validation true; multiple-picker capability with one selected file; clean Restart |
| `as2-locale-batch.nflash` | 6/6, including malformed XML retention and callback paths |

Generated output is under ignored `dist/as2-*-batch/`; screenshots, command logs
and the local verification server's HTTP records are under ignored
`target/verification/as2-two-hour/`. Export success alone is never a browser pass.
Native file-save download was not executed. The multiple-picker observation
selected one file and does not establish multi-file transfer. Sound used silent
playback; audible channel balance and quality were not assessed. No native
editor or FLA acceptance is included.

The bitmap example initially assumed two particular noise pixels must differ.
Both quantized to the same value for that seed. A focused check of the shipped
Rust WASM found a deterministic nonuniform field, so the example now checks the
whole field's nonmatching bounds. The 14 groups and native test count did not
grow. This is recorded as a corrected example predicate, not a runtime feature.


Browser integration also found missing timeline `stop()` calls in the Sound and
files examples. Adding them prevents the authored examples from restarting their
asynchronous setup each frame. The display example used an unsupported mixed
format inside an AS2 try/catch; it now exercises supported uniform markup while
the native rejection remains intact. These were example corrections, not a
reason to broaden the exception or text engines. Fresh delivery directories
avoided stale browser-cache results. No unresolved runtime integration failure
remained in the coordinator's final local pass.

## Controlled verification

The existing **27 compiler tests** and **90 execution fixtures** passed. Their
counts did not increase. One shared run collected **23 passes out of 24 project
workflows**. The legacy input semantic-delivery failure was corrected and its
single affected workflow passed the focused rerun. All 24 families therefore
have delivery results; a second complete 24-workflow run was not performed.
The initial failure remains in the evidence.

The batch adds **12 grouped Rust tests**, counted from the source test functions.
Data/binding and Locale use three groups, bitmap two, Sound/files one each, and
text/display, Object and variable decoding provide the remaining groups. One
existing text_field group also passed as an impact check, including the input
workflow. Tests cover coherent families rather than one test per declaration.
No original-Flash differential campaign or browser-engine matrix was added.

The workspace all-target check passed in **13.5 seconds**. Clippy across all
targets in the four affected crates passed in **7.854 seconds**. These durations
are command timings, not the entire verification budget. The final WASM grows
from **3,004,039 to 3,856,955 bytes**, an increase of **852,916 bytes (28.4%)** before
HTTP compression. Cargo manifests and lockfile versions are unchanged. Formatting, source fingerprints, progress consistency and all 25 progress
checks passed. Remote CI is not inferred from successful local checks.

## Reuse, ownership and efficiency

New runtime algorithms reside in the owned Rust core. Browser adapters preserve
object references and connect browser input, networking, fonts, audio and file
facilities. The existing Rust compiler still emits JavaScript for authored
ActionScript programs; this work does not claim that the entire prior execution
pipeline has become WASM bytecode.

Individual Ruffle MIT contracts and algorithms were adapted at pinned revisions
documented by each package. Bitmap turbulence also retains the applicable W3C
notice and modification attribution. While adding that notice, the previously
truncated final word of the existing Howard Hinnant MIT notice was restored;
this is an attribution repair, not new date implementation. Existing XML, URI, broadcaster, pixel,
display and transport kernels are reused. No complete external ActionScript
engine, foreign-language runtime library or copied editor was added. Adapted
material is attributed rather than relabeled as wholly original. At the source
freeze, Cargo manifests and lockfile have no dependency/version changes.

| Bounded cycle | Authorized time | New catalogue API paths | Other accounting |
| --- | ---: | ---: | --- |
| [Previous AS2 hour](AS2-RUNTIME-HOUR.md) | 1 hour | 71 | 13 new syntax constructs; 77 existing-support corrections |
| This AS2 cycle | 2 hours | 216 | 2 syntax entries; 30 existing-support corrections |

This is 3.04 times the previous total in twice the authorized time, or roughly
108 versus 71 API paths per authorized hour (1.52 times). It is a breadth metric,
not an independently controlled productivity benchmark: packages differ in
complexity, reuse, aliasing and validation cost. The earlier mixed AS2/AS3 hour's
198 member paths included generated event payload families and used a different
counting scope; it is not a like-for-like speed comparison.

The useful improvement is that one collection/view kernel supports many actual
data operations, one binding policy supports related field paths, and existing
IO/XML/pixel infrastructure supports new complete workflows. Fewer repeated
acceptance cycles leave more time for breadth. The remaining cost is integration
between shared runtime facilities; keeping those failures visible is part of
the approach, not evidence that conformance is already complete.

## Remaining limits and pause boundary

**AS2 is not complete.** Data components do not provide remote resolvers, schemas,
UI controls or persistence. Text is uniform-format, not arbitrary rich runs or
exact Flash shaping. Displacement on live DisplayObject filters remains refused.
Locale automatic TextField replacement and delayed instances remain unsupported.
Sound does not provide streaming/timeline mixing or every ID3 variant. File
operations depend on real user gestures and available browser facilities; exact
historical Flash security and metadata semantics are not fabricated. Full
prototype/AVM1 version behavior, older UI components and browser-incompatible
platform APIs remain separate work.

Requested merge to `main` and GitHub Pages statistics publication belong to the
coordinator's final delivery. They remain pending until their actual remote results are recorded. The unfinished
`checkpoint/paused-text-viewport` is not merged merely because this batch adds
separate runtime text features. After this bounded delivery, further feature
work, agents or verification cycles require a new user instruction; the wider
project and monitor automations stay paused.
