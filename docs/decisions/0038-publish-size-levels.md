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
- **Estimates.** `estimate_publish_sizes` assembles every level in memory and reports files, bytes on disk, gzip transfer size and a breakdown (WebAssembly, runtime JS, scripts, content, resources, fonts, page) for the publish dialog's size scale.

Consequences: Full stays the default for `export_project`, the editor and Test Movie, so existing behaviour does not change. Compact is conservative: a false match keeps a family and costs only size. The core player still references most `WebPlayer` methods, so Compact mostly saves on the optional families. Smaller cores need `runtime.js` itself split by feature, and that is future work. A size-optimised WebAssembly build (`opt-level`, fat LTO) shrinks every level but needs a runtime performance check first. The Publish settings scale needs an NF2 design (request R-032).
