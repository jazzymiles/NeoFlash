# AS2/AS3 Rust runtime hour

28 October 2026 local time. Branch `experiment/as-runtime-hour`, baseline `ef3b8a5`. Authorized window: 27 October 23:11:15 UTC through 28 October 00:11:15 UTC. Three subsystem agents worked in parallel; the coordinator implemented Math and integrated the shared compiler, runtime and exporter. The wider project, editor UI, FLA work and existing monitors remain paused.

This record uses the normal HTML5 exports and the member inventories under ignored `target/verification/as-runtime-hour/`. It counts newly useful partial API paths separately from existing behavior moved into Rust. A path is not an independent algorithm or a complete specification contract; the full declaration catalogue retains its separately dated audit.

## Delivery

| Family | New partial API paths | Existing entries moved to Rust |
| --- | ---: | ---: |
| AS3 String / URI | 8 | 19 |
| Geometry, AS2 Color, PerspectiveProjection, Motion Color, MatrixTransformer | 46 | 74 |
| Easing, Tween, CustomEase, callback timers, AsBroadcaster | 126 | 0 |
| ByteArray compression, text, Base64 / hex | 12 | 5 |
| Shared AS2/AS3 Math and AS2 random | 0 | 19 |
| **Total** | **192** | **117** |

New entries comprise **164 methods/functions and 28 properties**: **47 AS2 paths and 145 AS3 paths**. Migrations comprise 88 methods/functions and 29 properties. Separately: seven new constructors, four migrated constructors, six new constants, eight migrated Math constants and eleven event names. **No new language constructs** are claimed.

The same 18 Math methods and shared geometry classes are not counted twice merely because both dialects use them. Easing's distinct namespaces are counted as API paths, with their shared kernels stated separately. MatrixTransformer's nineteen methods include degree/radian forms sharing the same axis arithmetic; they are not nineteen independent algorithms. ByteArray's twelve new names include six entries present in the historical catalogue, including deflate/inflate, and six AIR 51 extensions outside it. The later exact-ID review corrected the initial inventory's mistaken claim that deflate/inflate were omitted; implementation totals are unchanged. These figures do not silently promote absent catalogue rows.

Binary text supports **38 distinct codec profiles** through Rust encoding_rs: 34 beyond the initial four profiles built earlier in this hour. This expands readMultiByte/writeMultiByte, not the API-name total. Labels/aliases are not extra codecs; representative UTF-8, UTF-16, Windows-1251, Shift_JIS, GBK, Big5 and EUC-KR paths are exercised together. Full round-trip conformance for every code point of all profiles is not implied.

## Ordinary HTML5 evidence

| Source project | Final observed result | Interaction |
| --- | --- | --- |
| `runtime-strings.nflash` | 31/31 checks; 27 API names | Run again and Restart |
| `runtime-geometry.nflash` | AS3 54/54; AS2 6/6; 120 names combined | Advance transform/tint and Restart |
| `runtime-easing.nflash` | AS3 81/81; AS2 30/30; AsBroadcaster 9/9; both callback-timer batches pass | AS2/AS3 animations finish at x=730; CustomEase animation finishes; Restart |
| `runtime-binary.nflash` | 16/16 checks; twelve new codec names | 1600-byte payload compresses to 72 bytes and restores; charset loop; Restart |
| `runtime-math.nflash` | AS3 21/21; grouped AS2 result true | Run again and Restart |

These are standard `.nflash` projects compiled and exported by `neoflash-export`, running the actual Rust WASM services in the in-app browser. Final family observations contain no runtime errors; Arial fallback warnings remain. The initial failed geometry and timer runs are retained in the evidence, not relabeled as successes. Numeric and string families were not rerun after unrelated adjacent additions; geometry/easing/binary were re-exported and checked after their own changes. Mechanical Clippy cleanup was rebuilt without another browser cycle. MatrixTransformer then received only its affected geometry browser run; the other exports were republished with the final shared runtime without repeating their checks.

Reproduce an export with `cargo run --locked -p neoflash-export -- examples/runtime-easing.nflash --output dist/runtime-easing` after `bash scripts/build-web.sh`. Serve `dist/` over localhost. Screenshots, console logs, source hashes and command timings are in ignored `target/verification/as-runtime-hour/`.

The new profiles remain bounded: finite geometry, object-only AS2 Color targets, standalone projection without a 3D renderer, literal String replacement with the old RegExp backend, programmatic monotone CustomEase curves without XML construction, browser scheduling, read-only AsBroadcaster listener snapshots, LZMA refusal and explicit text-codec limits. Full AVM conversion/prototype/native error fidelity is not claimed.

## Runtime and reuse

Individual algorithms from pinned compatible open-source revisions are adapted into owned Rust modules. No whole Flash engine, compiler, UI implementation or foreign-language library is embedded. JavaScript adapters marshal values, maintain browser references and connect browser clocks/events; the existing ActionScript frontend still emits JavaScript for authored program execution. This hour does not claim that the entire older runtime has become Rust.

Migrated String, geometry and UTF calculation branches are removed from the JavaScript adapters. Math resolves through Rust, with browser entropy only for random values. Codec libraries are ordinary Rust dependencies, not ActionScript engines: exact flate2 1.1.10 uses its Rust backend (already in the lockfile transitively); encoding_rs 0.8.35 with alloc adds one package and reuses cfg-if. No unrelated dependency versions change. All downloaded sources and investigative files stay under ignored `target/`; generated exports stay under ignored `dist/`. Necessary attribution travels in `THIRD_PARTY_NOTICES.txt` with every export.

Sources and deliberate partial boundaries: [strings](RUNTIME-STRINGS-PROVENANCE.md), [geometry](RUNTIME-GEOMETRY-PROVENANCE.md), [easing/Tween/timers](RUNTIME-EASING-PROVENANCE.md), [binary codecs](RUNTIME-BINARY-PROVENANCE.md), [Math](RUNTIME-MATH-PROVENANCE.md).

## Verification policy

The existing execution batch remains 90 fixtures. One shared pass covers those fixtures and seven affected project workflows with their common prelude: AS2 project/input, geometry/Timer, binary data, bitmap pixels, data processing, display transforms and typed collections. Six grouped Rust family tests are added; the pre-existing 3D test is extended. Five ordinary source projects are exported through the standard frontend and publisher. No per-method browser jobs, native editor pass, full workspace suite, original-player differential run or browser matrix is added.

Initial integration found a source-schema mistake, a stale geometry module export and AS2 import/direct-call registration gaps. They were corrected by family. Shared regressions passed once; later adjacent packages receive only their affected grouped checks and example exports; the last nineteen-method MatrixTransformer packet uses one existing grouped geometry test and that same geometry export. Browser observations and exact command timings are recorded separately from test-case counts. Recorded automatic test commands total approximately **34.3 seconds**, including compilation and the charset dependency build. Existing Rust compiler tests pass **27/27**; new core checks run as grouped selections (4, then 4 after adjacent work, then 3 after extensions, then one affected geometry test for MatrixTransformer). The unchanged execution batch and seven-workflow regression run once. Clippy passes for the four touched packages after five mechanical style fixes; formatting and diff checks pass. Five incremental WASM builds total approximately 37.6 seconds. Builds, exports, lint and browser interaction are separate costs, not hidden in that test duration.

## Efficiency comparison

The previous reuse trial delivered 62 new method/function/property paths and 14 migrations in 28.57 minutes, using two implementation agents plus a coordinator (19.3 minutes for its agreed Rust-only phase). The older language experiment delivered eight functions/methods plus syntax in 25.33 minutes with one implementation agent. This hour delivers **192 versus 62 new API paths**, and **117 versus 14 migrations**: 3.1× and 8.4× the raw totals respectively. It uses three subsystem agents instead of two, the coordinator also implements Math, and it has a longer authorized window. These are different tasks and team sizes, **not measured 3.1× or 8.4× whole-project speedups**.

The useful gain is breadth per shared implementation: 72 easing paths share 31 kernels, both dialects reuse one Tween state, callback timers reuse the existing scheduler, and geometry/binary extensions reuse the same Rust values/codecs. Repetitive adapters are simple table-driven bindings; copied engines and duplicated fallback algorithms are avoided. Charset breadth is reported as codec coverage, never inflated into new API names.

A short, representative browser scenario catches integration gaps that arithmetic tests cannot. Running a shared regression only once leaves time for adjacent capabilities. This approach is suitable for standard-library and value-object families; it does not establish the same throughput for display construction order, garbage collection, the full language or complete Flash compatibility. No project completion date is extrapolated.

The shared WASM payload is 2,657,466 bytes versus 2,285,042 bytes in the saved prior reuse publication: +372,424 bytes (about 16.3%, before HTTP compression). This compares the local runtime artifacts, not the differently authored example bundles.

Feature work ended around 23:57 UTC (46 minutes after the start); final geometry browser acceptance completed around 23:59 UTC. Packaging and report saving followed within the authorized hour. No further development or monitor automation was resumed.
