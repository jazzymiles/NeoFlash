# AS2 runtime hour

28 October 2026. Baseline `dad0097`, delivery branch `codex/as2-runtime-hour`.
Authorized window: 07:56:47–08:56:47 UTC. The wider project, AS3 feature work,
native editor UI, FLA and monitor automations remain paused. Three subsystem
agents worked in the same source tree; brief progress reports were sent to the
user-authorized AS2 chat. Integration and publication stayed with the coordinator.

## Delivered behavior and counts

| Package | New methods | New globals | New properties | Total API paths |
| --- | ---: | ---: | ---: | ---: |
| MovieClip / TextField aliases | 8 | 0 | 7 | 15 |
| System / input / component events / audio stop | 8 | 3 | 30 | 41 |
| XML / LoadVars / namespaces | 5 | 0 | 3 | 8 |
| Legacy target globals | 0 | 7 | 0 | 7 |
| **Total in the declaration catalogue** | **21** | **10** | **40** | **71** |

Separately: **13 new language constructs** (11 deprecated operators plus
`interface` and `implements`), **8 constants**, **1 event name**, **6 static
namespace/class entries**, and **0 constructors**. One additional useful method,
`mx.events.EventDispatcher.initialize`, is absent from this catalogue and is
reported separately, not inserted into its denominator. There are **72 new
method/global/property entry points** including that method.

Thirteen existing declarations were extended: nine display methods, three XML
entries and interface `extends`. One existing algorithm, AS2 `parseInt`, moved
from JavaScript into Rust. **77 declarations were already implemented and received
an evidence-based catalogue correction**; they are not new functionality.

These are declaration paths, not independent algorithms. MovieClip/TextField
aliases, static namespaces and legacy globals share owned implementations. Counts
are deliberately separate from original Flash conformance and from test cases.

## Catalogue result

| AS2 catalogue status | Before | After | Change |
| --- | ---: | ---: | ---: |
| Implemented subset | 15 | 15 | 0 |
| Partial | 563 | 745 | +182 |
| Missing | 1,665 | 1,501 | −164 |
| Needs review | 64 | 46 | −18 |
| **Total declarations** | **2,307** | **2,307** | **0** |

The review covers 190 exact AS2 IDs: 99 newly available declarations, 14 existing
extensions/migrations and 77 baseline corrections. Eight were already partial;
182 change status. AS3 remains exactly **15 subset / 1,254 partial / 42,308 missing /
163 needs review**, total 43,740. The complete catalogue is used; supplementary
feature-group counts are never a language-coverage denominator.

## Real HTML5 acceptance

| Ordinary source project | Browser result |
| --- | --- |
| `as2-display-batch.nflash` | 20/20; depth/duplicate/bounds/text aliases; Restart 20/20 |
| `as2-platform-batch.nflash` | 8/8 grouped checks; real key A produced code 65; Restart 8/8 |
| `as2-xml-data-batch.nflash` | 18 checks, no failures; real XML POST with validated body/content type/custom headers and HTTP 200 response |
| `as2-legacy-batch.nflash` | 24/24; operators, parseInt, target globals, duplication and drag setup/stop; Restart 24/24 |
| `as2-interfaces-batch.nflash` | Typed counter initially 3; button invokes interface methods and updates to 4, clicks 1 |

These are normal exporter outputs using actual Rust WASM, observed in the Codex
in-app browser. The five exports are under ignored `dist/as2-*-batch/`; screenshots,
initial failures, command records and the local XML endpoint log are under ignored
`target/verification/as2-runtime-hour/`. The 70 assertions across the first four
projects are batched checks, not 70 new automated test cases. IME registration was
checked; an actual OS IME composition session and audible audio playback were not.
The audio-stop check used unloaded silent AS2 Sound objects.

Browser acceptance caught a noncanonical ObjectRef field order in duplicate
results; a typed Rust packet fixes it. It also exposed two example mistakes:
partial replacement with mismatched default text format, and expecting an AS2
catch to absorb a native unsupported-platform diagnostic. Examples now use the
existing supported profiles; text and exception engines were not broadened to
make the examples pass. A compiler global-call routing error was fixed. Only
affected browser examples were repeated.

## Controlled verification

Three new grouped Rust tests were added. Existing String and project-interface
groups were extended; test counts for those groups did not increase. The existing
**27 compiler tests** and **90 execution fixtures** passed. One shared project run
executed all **24 workflows** and the common prelude: 23 passed, one caught the
new AS2 `<>` token interfering with AS3 E4X XMLList syntax. The lexer now recognizes
that token only for AS2. Only the affected `xml-catalog` workflow was rerun and
passed; the whole 24-workflow suite was not repeated.

The recorded compiler/execution/project commands consumed **44.6 seconds**,
including the failed XML workflow and its focused rerun. Agent-local grouped
Rust checks, lint, export and browser interaction are separate. The final WASM is **3,004,039 bytes**, up **109,215 bytes (3.8%)** before
HTTP compression. Three WASM build
attempts were needed (one failed on a missing direct serde declaration); no
per-method browser jobs or new conformance harnesses were added.

Clippy was run for all targets in the four affected Rust crates and passed.
This also fixed two previous filter-test clone lints that had been missed by the
previous library/binary-only local check, plus test-module ordering under current
Clippy. The filter fixes only change test borrowing. Formatting and generated
progress consistency are checked for publication. Full remote CI remains a
separate GitHub Actions check.

## Reuse, ownership and efficiency

Pinned Ruffle MIT contracts/individual algorithms were adapted into the owned
Rust core. The new AS2 interfaces reuse NeoFlash's own linker and contract model;
legacy methods reuse owned display, text, network and broadcaster infrastructure.
Browser adapters transport references/data and connect browser facilities. No
complete third-party ActionScript engine, foreign-language implementation library
or copied editor was added. The only Cargo dependency-list change is making the
already-present workspace `serde` dependency explicit in the web crate for typed
serialization: **zero new crate packages or versions**.

This hour produced 71 catalogue API paths, 13 syntax constructs and a real set of
five HTML5 examples. The earlier single-agent experiment's five items in roughly
25 minutes used a different scope; a precise speed multiplier would be misleading.
The previous broader AS2/AS3 hour added 198 member paths, many generated event
fields. This AS2-only hour covered fewer paths but more integration-heavy behavior:
mutable display lifetimes, XML transport, legacy syntax and source contracts.
Reusable kernels and a shared acceptance pass remain effective; reports also
expose catalogue backfill rather than counting it as implementation speed.

AS2 is **not closed**. Remaining work includes old UI components, full dynamic
Object/prototype semantics, authored timeline duplication, SWF/AVM version quirks,
media metadata/matrix behavior and browser-incompatible platform facilities.
Specific package profiles and pinned sources are recorded in
[display](AS2-DISPLAY-BATCH.md), [platform](AS2-PLATFORM-BATCH.md),
[XML/data](AS2-XML-DATA-BATCH.md), [legacy syntax](AS2-LEGACY-BATCH.md) and
[interfaces](AS2-INTERFACES-BATCH.md). The bounded delivery does not resume the rest
of the project.
