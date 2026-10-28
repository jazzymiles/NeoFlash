# AS2 second two-hour subsystem batch

28 October 2026. Baseline `385908d`, branch `codex/as2-two-hour-2`.
Authorized window: **10:39:46–12:39:46 UTC**. Three subsystem agents and the coordinator worked in one source tree, with short milestones in the AS2 chat. AS3 feature work, native editor UI, FLA, the wider project and monitor automations remain paused.

Implementation and local HTML5 acceptance are complete. This delivery is prepared for the requested merge into main and GitHub Pages publication. Remote validation is separately visible in [GitHub Actions](https://github.com/jazzymiles/NeoFlash/actions) and the [progress dashboard](https://jazzymiles.github.io/NeoFlash/progress.html). This record does not authorize another implementation cycle.

## Exact change accounting

| Package | New methods | New properties | Other new declarations | Total new declarations |
| --- | ---: | ---: | ---: | ---: |
| Video controllers and NetStream | 29 | 61 | 61 | 151 |
| SOAP/WSDL and XML connectors | 12 | 20 | 26 | 58 |
| Raster groups, text and MovieClip images | 4 | 13 | 0 | 17 |
| Five rendered transitions | 3 | 2 | 12 | 17 |
| Relative DepthManager | 4 | 0 | 7 | 11 |
| AS2 class accessors and with | 0 | 0 | 3 | 3 |
| TextField.variable | 0 | 1 | 0 | 1 |
| **Total** | **52** | **97** | **109** | **258** |

There are **149 new method/property paths**. The remaining 109 declarations are **47 constants, 34 events, five callbacks, 17 classes, three constructors and three statements**. These are catalogue paths, not 258 separate algorithms. Shared Rust kernels serve related properties, owner aliases and callbacks.

Eight VideoError entries are labeled properties by the source catalogue but are static constants; the table correctly counts them as constants. The explicit NeoFlash `VideoPlayer.attach`/`FLVPlayback.attach` convenience methods are excluded from the historical declaration count. Unsupported constructors, skin methods and diagnostic-only methods are not promoted.

**Nine existing declarations are extended**: six methods, one property, one event and one constructor. **43 reviewed-existing declarations** receive corrections of stale missing decisions and are not credited as new code. The combined inventory contains **310 unique AS2 IDs** with no unknown IDs, duplicates or baseline-status mismatches. Of these, 301 were missing and nine were already partial.

## Catalogue result

| AS2 status | Baseline | After bounded review | Change |
| --- | ---: | ---: | ---: |
| Implemented subset | 15 | 15 | 0 |
| Partial | 1,054 | 1,355 | +301 |
| Missing | 1,201 | 900 | −301 |
| Needs review | 37 | 37 | 0 |
| **Total** | **2,307** | **2,307** | **0** |

The increase of 301 includes corrections of existing support. It is not a count of new implementations or a claim that all AS2 is complete. AS3 remains **15 implemented subset, 1,254 partial, 42,308 missing, 163 needs review**, total 43,740; its exact catalogue entries are checked unchanged.

The tracked review is `docs/progress/reviews/2026-10-28-as2-two-hour-2.json`. The normalized baseline-relative inventory, commands, browser observations and downloaded references are under ignored `target/verification/as2-two-hour-2/`. Source hashes record the bounded review; `currentSourceVerified=false` deliberately distinguishes representative family acceptance from independent per-declaration Flash conformance.

## Working behavior and limits

- [Media](AS2-MEDIA-BATCH.md): real progressive video playback, state/cue/seek policy, dimensions, aspect sizing, byte facts for bundled sources, actual FPS/buffer measurements and audio/video receive gates. AS2 component controllers use explicit attachment to an authored Video. FLV/RTMP, skins, reserved component construction, live video and full Flash buffering remain unsupported.
- [Services](AS2-SERVICES-BATCH.md): bounded WSDL 1.1 and SOAP 1.1 literal scalar messages, typed XML results, real HTTP 500 Faults, queued and concurrent calls, nonvisual XML/WebService connectors and logging. External schemas, encoded arrays, attachments, headers and arbitrary nested/custom values remain unsupported.
- [Display/text](AS2-DISPLAY-FILTERS-BATCH.md): real blend/cache/opaque groups, autoSize anchors, paragraph bullets/tabs/spacing/kerning, wheel scrolling, uniform hyperlinks and actual MovieClip PNG/JPEG loading/unloading. Native text overlays inside rasterized groups or bullet/custom-tab paragraphs diagnose or decline editing; rich text and exact Flash rasterization remain separate work.
- [Transitions](AS2-TRANSITIONS-BATCH.md): Fade, Fly, Rotate, Zoom and Squeeze use existing Rust easing and display transforms, saved appearance and actual completion events. Mask effects, custom Transition subclasses and conflicting concurrent channels remain unsupported.
- [DepthManager](AS2-DEPTH-MANAGER-BATCH.md): existing linked MovieClip creation plus stable relative ordering in bounded normal/topmost tiers. Authored negative placements, UIObject creation and cursor/tooltip system layers remain unsupported. Failed attachment may normalize numeric depths while preserving sibling order and objects; atomic historical creation parity is not claimed.
- [Language](AS2-LANGUAGE-BATCH.md): concrete instance/static class accessors and nested with-object scopes with method receivers and lexical fallback. Primitive with targets, functions/eval/set inside with and full captured AVM1 scope chains remain outside this profile.
- [Text binding](AS2-TEXT-VARIABLE-BATCH.md): plain TextField.variable with primitive MovieClip values, rebind/detach, shared mirrors, actual user input and Restart. HTML binding, arbitrary object coercion and exact legacy propagation are not claimed.

The implementation reuses the existing Rust XML, binding, easing, display, image and media infrastructure and individually reviewed compatible references. Pinned provenance and attribution remain in the package records and THIRD_PARTY_NOTICES.txt. No complete third-party engine, foreign-language parser or new runtime dependency was introduced. The existing architecture still compiles authored AS2 into JavaScript and uses Rust/WASM services; host references, class descriptors and browser adapters remain JavaScript. This is not a claim that all script execution has moved into a Rust bytecode VM.

## Ordinary HTML5 observations

Each row is one coherent authored family, not a separate browser acceptance cycle for every API. Exports use the normal compiler/exporter and shared Rust WASM.

| Source example | Observed browser result |
| --- | --- |
| `as2-language-batch.nflash` | 14/14 on load and physical Run; independent accessor instances included |
| `as2-services-batch.nflash` | 12 passed / 0 failed; real WSDL, XML/SOAP requests, HTTP 500 Fault, queued calls, SINGLE/LAST and raw XML |
| `as2-media-batch.nflash` | READY 12/12; physical Play gives 13/13 with measured 20 fps, two cues, completion 1, automatic rewind and Restart READY 12/12 |
| `as2-transitions-batch.nflash` | Three grouped checks pass; five actual clips visibly animate IN and OUT and restore appearance |
| `as2-display-filters-batch.nflash` | 20/20 including 411-byte PNG load/unload/reload; physical cached-child movement, wheel scroll 7/callback 1 and real local hyperlink navigation |
| `as2-text-variable-batch.nflash` | Nine initial groups; real typing updates variable/mirror before onChanged, button updates both fields, Restart resets state |
| `as2-depth-manager-batch.nflash` | 8/8 with visible linked clips in expected order; physical Restart repeats 8/8 |

The shared WASM grows from 3,856,955 to 4,209,426 bytes (+352,471; +9.1%). Exports remain under ignored `dist/`. Local SOAP fixture routes are delivery tooling, not a deployed backend. The media sample is silent, so audible quality is unverified. These observations do not establish cross-browser, original Flash, every event/error path or individual API conformance.

## Verification cost and fixes

- Existing compiler checks: **27 unit + 10 integration tests passed**; no new compiler test functions.
- Existing emitted-code checks: **90/90**, count unchanged, one shared run.
- Existing project workflows: **24/24**, count unchanged, one shared run. This includes the extended MovieClip image loading scenario.
- **Three new grouped Rust tests** cover media, services and transitions. Existing Object, text and display/depth tests were extended in place. There is no test per method/property.
- Formatting, all-target Clippy, shared WASM and ordinary exports are checked. Full repository CI runs on the requested publication; its result is recorded separately.

Concrete integration fixes covered AS2 dynamic operation lookup, real accessor installation and with-call receivers, raster composition through clips, native-overlay boundaries and a large Rust enum variant. The services sample was corrected to sequence its intentional SINGLE/LAST checks after a queued call. Example schema/name mistakes were fixed in the affected exports. Only affected examples/probes were repeated; the shared 90/24 suites were not rerun for each fix.

A short cross-review additionally guarded service callbacks after Restart, kept XML parse failures separate from authored callback failures to avoid double decrement, and removed duplicate transition render notifications. Those narrow post-acceptance changes are recorded as source-reviewed, not as new individually tested APIs.

## Comparison with the previous approach

| Same two-hour budget | Previous AS2 batch | This batch |
| --- | ---: | ---: |
| New method/property/global paths | 216 | 149 |
| All new declaration entries | 278 | 258 |
| Existing declarations extended | 12 | 9 |
| Corrections of earlier support | 30 | 43 |
| New grouped Rust test functions | 12 | 3 |
| Shared emitted-code fixtures | 90 | 90 |
| Shared project workflows | 24 | 24 |
| Ordinary HTML5 families | 8 | 7 |

This batch adds fewer API rows than the preceding collection-heavy batch, while delivering substantial media and service behavior. Counts are not equal units of engineering effort and do not prove a speedup on their own. The useful result of the approach is reusable subsystem implementations, actual HTML5 behavior and controlled verification growth: three new native groups, unchanged shared 90/24 sets, and targeted repairs. We do not inflate throughput with constants, aliases, stale audit corrections or diagnostic-only methods.

The remaining 900 missing AS2 declarations comprise 628 component, 252 runtime, four core, four language, two framework and ten platform-extension entries. They include substantial legacy UI/component, platform, media-server and language behavior. They remain explicit backlog, not automatically authorized work after this sprint.
