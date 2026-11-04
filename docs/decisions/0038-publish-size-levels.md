# 0038 — HTML5 publish size levels

Status: accepted, 2026-11-04.

Every HTML5 export carried the whole player: about 7 MB of WebAssembly, embedded as base64 in `bundle.js`, plus about 2.7 MB of browser host modules for every ActionScript family. A banner without scripts published as 12 MB, where a SWF of the same content was about 100 KB because Flash Player was already installed.

Decision: the exporter offers four size levels (`neoflash_export::SizeLevel`). Each level plays the same project and gives up something specific.

| Level | WebAssembly | Host modules | Gives up |
| --- | --- | --- | --- |
| Full | base64 in `bundle.js` | all | nothing; `index.html` opens from disk |
| Balanced | `neoflash_web_bg.wasm` file, streamed | all | must be served over HTTP(S) |
| Compact | file, stripped | without families the project never names | as Balanced; a class name built at run time cannot reach a left-out family |
| Shared | loaded from a runtime URL | loaded from a runtime URL | needs the shared runtime folder, published from the same build |

- **Families.** Compact leaves out the AIR, AS3 networking/system, text engine/Stage3D and AS3 media/input families when no script, class linkage or text resource names any class they register (`'flash.pkg.Name'` keys in their `builtins`), as qualified or short name, or one of the properties they install on core objects (`stage3Ds`, `nativeWindow`, …). A project without any script or class linkage also leaves out the script-only modules (AS2 families, XML, Loader, SharedObject, timers, …) and the compiled prelude. `net.js` and `byte-array.js` always stay, because authored sound and video play through them.
- **WebAssembly stripping.** Compact removes WebAssembly exports that only unreachable bindings call: a wasm-bindgen glue member (class method, accessor, constructor or exported function) is unreachable when no shipped JavaScript (host modules, `runtime.js`, project program, compiled scripts) names it, or names it without the `object_` prefix that the display host adds. `walrus` (MIT/Apache-2.0, an unmodified dependency) then removes the code nothing else reaches. Instantiation is unchanged, because the glue looks up exports only when they are called.
- **Shared runtime.** `export_shared_runtime` writes every runtime file plus `runtime-id.js`, an FNV-1a identity of the runtime files. A Shared project records the same identity, and `runtime.js` refuses to start against a different build with an actionable message.
- **Estimates.** `estimate_publish_sizes` assembles every level in memory; `estimate_publish_options` measures one selected font/size combination. Totals count every generated file, including the package manifest and shared-runtime README. Gzip totals compress the actual files; per-part gzip compresses fragments independently and is approximate attribution, not an additive transfer breakdown. `estimate_publish_levels_cancellable` shares preparation and cooperatively cancels between compilation, assembly and compression phases.

The editor persists `PublishOptions` in the existing workspace `PublishPrefs` contract. Publish and publish-on-save use those options; a draft can be estimated and published without changing saved defaults. Estimates run on one background worker with one replaceable pending snapshot; project, selected options, warning policy and runtime-file changes invalidate stale results. Closing demand cancels it without blocking the UI. Compilation and WebAssembly stripping already in progress finish before observing cancellation. The size selector and weight/computing states remain unavailable pending R-032; document-stored publication profiles remain future work.

Shared runtime directory URLs are validated before compilation or output writes. HTTP(S), relative and root-relative paths are accepted; credentials, query strings, fragments and backslashes are rejected. Validation does not fetch a remote runtime. Browser startup checks the build identity; cross-origin WASM hosting must permit CORS.

Consequences: Full/Embed stays the default for `export_project` and older editor preferences. Test Movie continues to use Full independently of publication settings. Compact is conservative: a false match keeps a family and costs only size. The core player still references most `WebPlayer` methods, so Compact mostly saves on the optional families. Smaller cores need `runtime.js` itself split by feature, and that is future work. A size-optimised WebAssembly build (`opt-level`, fat LTO) shrinks every level but needs a runtime performance check first. The Publish settings scale needs an NF2 design (request R-032).
