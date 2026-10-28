# AS2 runtime two-hour batch

28 October 2026. Baseline `fea2333`, branch `codex/as2-runtime-two-hour`.
Authorized implementation window: **12:05:54–14:05:54 UTC** (14:05–16:05 in Cyprus). This batch prioritizes the 252 missing runtime declarations and 20 remaining non-component declarations. The 628 legacy component declarations are last priority; AS3, native editor UI, FLA, wider project work and monitor automations stay paused. The ten CustomActions/XMLUI authoring-extension declarations remain outside this runtime scope.

The session stopped making visible progress after the saved capture result at **13:02:28 UTC**. At **14:08:38 UTC**, the coordinator found no active agents and recovered the work from disk. The underlying interruption cause is unknown. That roughly 66-minute gap is not counted as productive implementation time. After recovery, work is limited to integration repairs, the requested delivery checks, reporting and publication; no new feature package is authorized by this recovery.

**Delivery status:** local delivery is complete: code, bounded audit, shared checks and five ordinary HTML5 families have recorded results. PrintJob has limited browser acceptance; its visual preview/system dialog remains unverified. Requested main/Pages publication is next. The result tables distinguish new behavior, audit corrections and observed execution. Remote results will be visible in [GitHub Actions](https://github.com/jazzymiles/NeoFlash/actions) and the [progress dashboard](https://jazzymiles.github.io/NeoFlash/progress.html).

## Exact change accounting

| Package | New methods | New properties | Other new declarations | Total new declarations |
| --- | ---: | ---: | ---: | ---: |
| Stage, display and content menus | 4 | 24 | 7 | 35 |
| Camera and Microphone | 9 | 21 | 6 | 36 |
| Progressive NetStream buffering/completion | 1 | 1 | 1 | 3 |
| Bitmap PrintJob | 3 | 5 | 2 | 10 |
| LocalConnection and HTTP remoting | 5 | 3 | 4 | 12 |
| Primitive object constructors | 0 | 0 | 3 | 3 |
| Project includes and Locale replacement | 1 | 1 | 1 | 3 |
| **Total** | **23** | **55** | **24** | **102** |

The batch contains **78 new method/property paths**. The other 24 declarations are ten events, seven constructors, six classes and one directive. These are declaration paths, not 102 independent algorithms. No stub, diagnostic-only API, browser convenience extension or legacy component catalogue row is counted as new support.

**Seven existing declarations are extended or corrected**: Stage width/height/onResize, Video.attachVideo, NetConnection.connect/onStatus and Selection.getFocus. The Selection correction routes its result through the shared display target-path implementation; it adds no API. **61 reviewed-existing declarations** correct stale missing decisions: 20 display/selection/geometry entries, NetConnection.close, and 40 class/ExternalInterface entries. Those 61 are not newly implemented functionality. The normalized inventory has **170 unique AS2 IDs**, no duplicates or unknown IDs, with 163 previously missing and seven already partial.

## Catalogue result

The bounded review retains explicit partial profiles and the acceptance limits recorded below:

| AS2 status | Baseline | After bounded review | Change |
| --- | ---: | ---: | ---: |
| Implemented subset | 15 | 15 | 0 |
| Partial | 1,355 | 1,518 | +163 |
| Missing | 900 | 737 | −163 |
| Needs review | 37 | 37 | 0 |
| **Total** | **2,307** | **2,307** | **0** |

The partial increase includes 61 corrections of existing support. It is neither a new-implementation count nor complete AS2 conformance. AS3 remains **15 implemented subset, 1,254 partial, 42,308 missing and 163 needs review**, total 43,740.

The normalized baseline-relative inventory, source evidence, command logs and reference downloads are kept under ignored `target/verification/as2-runtime-two-hour/`. The tracked per-entry review is `docs/progress/reviews/2026-10-28-as2-runtime-two-hour.json`. It retains explicit partial profiles; `currentSourceVerified=false` distinguishes representative family acceptance from independent per-declaration Flash conformance.

## Working profiles and boundaries

- [Stage/display](AS2-STAGE-DISPLAY-BATCH.md): Rust viewport sizing/placement, AS2 fullscreen requests, content context menus with real actions, menu press transfer, explicit focus rectangles, loaded-image smoothing and geometry aliases. Browser permissions still govern fullscreen; noScale currently requires top-left alignment. Content menus support a bounded item policy, not every Flash player menu option.
- [Capture](AS2-CAPTURE-BATCH.md): Camera/Microphone identity, settings, lifecycle, frame/activity measurements and adapters for actual browser capture. Rust owns policy and measurements; browser facilities own hardware access. **Real camera permission/images and audible microphone output remain unperformed acceptance.** The synthetic adapter result is not hardware proof. Encoding, server publishing and historical device preferences remain unsupported.
- [NetStream](AS2-CAPTURE-BATCH.md#related-progressive-netstream-extension): a real contiguous-data buffer target and completion callback over finite progressive media. FLV metadata, RTMP/FMS, server upload and invented QOS results remain unsupported.
- [PrintJob](AS2-PRINT-BATCH.md): shared Rust scene capture and PNG output feed an owned browser print preview. Page properties describe an explicit virtual CSS A4 page, **not physical printer capabilities**. Current-frame bitmap pages are supported; vector printing, arbitrary frame evaluation and printer status are not. Two captured PNG pages and the send call have browser evidence; the visual print preview/system dialog and cancellation remain unverified. No physical page is printed by development checks.
- [Connections](AS2-CONNECTIONS-BATCH.md): same-origin LocalConnection over BroadcastChannel/Web Locks and AMF0 HTTP NetConnection requests with actual response envelopes. The browser channel's asynchronous reservation status is an explicit adaptation. Raw TCP XMLSocket, RTMP, remote SharedObject, cross-origin local channels, AMF3 remoting and server callbacks remain unsupported.
- [Primitive boxes](AS2-BOXING-BATCH.md): distinct AS2 Boolean/Number/String objects with Rust conversion/formatting, valueOf/toString, instance fields and existing UTF-16 String methods. Arbitrary object coercion, full prototype behavior, box serialization and complete historical formatting are outside this profile. Unsupported box coercions produce host diagnostics that authored AS2 catch cannot recover in the current profile.
- [Includes/Locale](AS2-INCLUDES-LOCALE-BATCH.md): nested AS2 includes from serialized project resources with cycle/path/size bounds, plus delayed plain TextField replacement after a successful XLIFF load. Arbitrary filesystem includes, class-unit includes, initclip/intrinsic declarations and authoring Strings UI remain unsupported.

The new code extends shared Rust geometry, display, AMF, text, media, binding, string and image services. Individual compatible references are understood and adapted with pinned provenance; existing Ruffle MIT attribution is retained. Capture references that were upstream stubs were not copied or credited as working support. No complete third-party engine, foreign-language runtime implementation or new runtime dependency is embedded. The existing compiler still emits authored JavaScript using Rust/WASM services; the browser bridge retains object/callback identities and connects browser facilities. This delivery does not claim a complete Rust bytecode VM.

## Ordinary HTML5 delivery

Five coherent authored families have been exported using the normal compiler/exporter and shared WASM. Browser observations are recorded separately below. PrintJob is part of the capture family, primitive boxes are part of connections, and buffering extends the existing media family. It does not add a browser acceptance cycle for each API.

| Source example | Actual observed result |
| --- | --- |
| `as2-stage-display-batch.nflash` | 25/25; actual fullscreen entry/Escape exit, exactFit/showAll scaling, press transfer A→B (B=2, drags=2), context-menu action/disabled/hidden rows, decoded video and image smoothing |
| `as2-capture-batch.nflash` | Metadata/policy 8/8, zero cameras/one microphone reported, idle detach and Restart 8/8; no hardware permission. Print scene accepts two Rust-captured PNGs and invokes send without error; visual preview/system dialog/cancel unverified |
| `as2-connections-batch.nflash` | 9 passed / 0 failed initially, 13/0 after real AMF HTTP; two tabs exchange A↔B messages and close/reclaim channel A in the other instance |
| `as2-runtime-language-batch.nflash` | 6 passed / 0 failed initially; 11/0 after the physical French reload button, with nested includes and real XLIFF replacement |
| `as2-media-batch.nflash` | READY 12/12, physical playback 13/13 with measured 23.18 fps and freeze/resume; raw onPlayStatus completion yields 14/14, one raw and one controller completion, two cues |

The built shared WASM is **4,427,923 bytes**, up from 4,209,426 (**+218,497; +5.2%**). Exports belong under ignored `dist/`; the local AMF fixture is delivery tooling, not a deployed backend. Neither synthetic capture nor browser content acceptance establishes full Flash, cross-browser or individual-API conformance.

The actual Print scene click reports `AS2_PRINT_PAGES true true page523x770 paper595x842 portrait`: both Rust-captured PNG pages were accepted and send was invoked without an observed error. The owned popup was not exposed as a controllable in-app browser tab, and native Codex app inspection was unavailable. Therefore the visual preview, system dialog and cancellation were not inspected. The paper/page numbers are virtual output geometry; no physical printing or printer-capability check is claimed.

## Controlled verification

- Existing compiler checks passed: **27 unit + ten integration = 37**, with no new compiler test functions.
- Shared WASM build passed. The actual-WASM capture adapter fixture passed **8/8 grouped phases** on its first run in 0.157 seconds, with no hardware permission request.
- There are **four new grouped Rust test functions**: Stage, capture, connections and primitive boxing. Existing media/platform/Locale/input/display/compiler groups are extended in place. The shared affected native batch passed **19 AS2 library + two integration tests**. The Stage group also passed once before that shared delivery pass.
- The **90 emitted-code fixtures and 24 project workflows keep their existing counts and both passed one shared run**. The full suites were not repeated. After browser acceptance exposed the Selection.getFocus path defect, the affected text-form workflow passed its focused rerun (1/1 plus the common prelude). Export failures received focused repair/re-export; no per-method verification loop was introduced.
- Formatting, all-target Clippy and all five ordinary exports passed. Clippy required two lint repairs before its successful third invocation. The initial export batch had three successes and two authored-example failures; only the affected examples were repaired/re-exported. Five HTML5 families have the bounded observations above; publication CI is a separate result.

The lint repairs changed a slice-chunk iterator to the Rust 1.88 array-chunk API and removed an unnecessary closure borrow. Export repairs gave copied capture/connections symbols unique internal layer/element IDs. The capture example had also requested unsupported native `instanceof Camera/Microphone`; those predicates were removed while stable `get()` identity/index checks were retained. This preserves the explicit unsupported type-test boundary rather than claiming new native type-test support. The shared WASM was rebuilt after the Rust lint edit. Browser acceptance then exposed an existing Selection.getFocus path defect; the host now reuses the shared display target-path operation, and the stage example avoids a helper-name collision. Exported assets were refreshed after that integration repair. The connections example initially tried to recover an unsupported boxed-object coercion with authored catch; the host diagnostic is deliberately rethrown by the current execution profile. That out-of-profile negative probe was removed, retaining four positive boxing groups. No new catch or coercion behavior was claimed. These are delivery repairs, not new feature counts.

## Effectiveness and remaining work

| Metric | Previous AS2 two-hour batch | This runtime batch |
| --- | ---: | ---: |
| New method/property paths | 149 | 78 |
| All new declarations | 258 | 102 |
| Existing declarations extended or corrected | 9 | 7 |
| Corrections of existing support | 43 | 61 |
| New grouped Rust test functions | 3 | 4 |
| Shared execution/workflow counts | 90 / 24 | 90 / 24 |
| Ordinary HTML5 families | 7 observed | 5 observed within recorded limits |

This batch builds fewer catalogue rows than the preceding component-heavy package, while adding browser-device, cross-context transport, print and viewport capabilities. Counts differ in effort and are not a speed benchmark. The interruption also prevents a defensible direct throughput comparison between the two wall-clock windows. The useful result of the approach is coherent Rust services reused across several public declarations, with shared tests kept at 90/24 and only four new native groups. Corrections, aliases and constructors are kept visible rather than being presented as independent new algorithms.

The bounded review moves 163 of the initially missing 272 non-component declarations out of the missing category, leaving **109 non-component declarations**: 96 runtime, three language (`#initclip`, `#endinitclip`, `intrinsic`) and ten paused CustomActions/XMLUI authoring extensions. The **628 deferred component declarations** are unchanged. The residual includes unsupported server/platform behavior and language/conformance gaps. Full AS2 remains unfinished. Another implementation cycle requires a new user instruction.
