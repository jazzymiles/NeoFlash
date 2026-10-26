# Script and runtime compatibility

Status: initial executable foundation, 26 October 2026. This is **not full Flash, AS2 or AS3 compatibility**. The product goal remains broad compatibility; this file describes only what the current implementation actually accepts.

## Languages

| Capability | Current state |
| --- | --- |
| JavaScript frame/object handlers | Parsed by Oxc as strict-mode function bodies, matching the HTML5 host, with syntax/early-error diagnostics; JavaScript is emitted without rewriting its expressions. |
| TypeScript frame/object handlers | Parsed, transformed and emitted by Oxc in Rust. Type annotations/interfaces are removed; supported runtime constructs such as enums are transformed. This does not run the TypeScript type checker. |
| JavaScript/TypeScript modules, imports, top-level await | Not supported inside a synchronous frame/object handler. Imports/exports must not be silently removed. A module/project build system is future work. |
| JSX/TSX, external transform helpers | Not part of this phase. Constructs requiring unbundled transformation helpers produce an error. |
| AS2/AS3 | Explicit small language frontend described below; independent lexer, recursive-descent statement parser, Pratt expression parser, AST and JavaScript emitter. No regex transpilation. |
| Full AVM1/AVM2 behavior | Not implemented. Browser execution currently uses JavaScript semantics, with selected AS3 primitive coercions. |

The compiler API is `neoflash_script::compile(&Script) -> Result<String, Vec<Diagnostic>>`. Every diagnostic includes the source language, one-based line/column and a message. A failed script must block publishing; exporting an empty replacement would lose project behavior. A single script is limited to 1 MiB; the AS frontend also bounds token count and expression nesting to avoid runaway parsing.

## Accepted ActionScript subset

| Area | Accepted now | Limits |
| --- | --- | --- |
| Literals | Decimal, exponent and hexadecimal numbers; single/double-quoted strings; booleans, `null`, `undefined`, `NaN`, `Infinity`; arrays without holes; object literals | RegExp literals, XML/E4X, legacy leading-zero numbers and specialized literal semantics are pending. |
| Variables | `var name = value`, comma-separated declarations, frame/object fields | Top-level variables are stored on `this`, so values can persist on that runtime instance. Full compiler scope, hoisting and class-slot semantics are pending. |
| Type annotations | AS2 `Number`, `String`, `Boolean`; AS3 also `int`, `uint`, `*` | AS2 annotations are erased without a type checker. AS3 primitive annotations affect defaults and bare-name writes **inside that one script**; they do not create globally enforced typed slots. |
| Arithmetic | `+ - * / %`, unary `+ -`, prefix/postfix `++ --` | Uses JavaScript number/string operations; complete Flash coercion conformance is not claimed. |
| Comparison/logical | `== != === !== < <= > >=`, `!`, `&&`, `||`, conditional `?:`, `typeof` | Legacy AVM1 coercion/version differences remain pending. |
| Bitwise | `& \| ^ ~ << >> >>>` | JavaScript 32-bit semantics. |
| Assignment | `= += -= *= /= %= &= \|= ^= <<= >>= >>>= &&= \|\|=` | Compound assignment to a declared AS3 primitive is coerced. Property writes and cross-script writes do not have AS3 slot-type enforcement. |
| Access | `this`, bare instance fields/names, `object.property`, `object[index]` | Unknown field reads follow the browser host's behavior. There is no full AS scope-chain/prototype implementation. |
| Statements | Blocks, `if/else`, `while`, classic three-part `for`, `break`, `continue`, expression statements | No function/class declarations, `switch`, `do/while`, `for each`, exception handling, `with`, namespaces, packages or imports. |

Rejected constructs include `is`, `as`, `instanceof`, `in`, `delete`, `new`, `void`, namespace qualifier `::`, E4X `@`/`..`, `const`, functions and classes. They produce diagnostics, not approximate replacements. Function calls are limited to the API below; unknown ActionScript calls are errors. Equivalent functionality can currently be authored in JavaScript or TypeScript handlers.

Selected AS3 defaults: `Number` starts as `NaN`, `int`/`uint` as `0`, `Boolean` as `false`, `String` as `null`, and `*`/untyped as `undefined`. Bare-name primitive assignments in that script use numeric/boolean/string coercion; integer casts wrap to 32 bits. This is a useful subset, **not** a complete AS3 type system. For example, writing `this.count` from another handler does not enforce a `count:int` declaration in the first handler.

Declarations without an initializer preserve an already defined instance field when the handler executes again; an explicit initializer runs on each invocation. Defaults are established when the declaration executes, not by a complete Flash class/slot initialization pass. JavaScript/TypeScript `var`/`let`/`const` keep ordinary function/block scope; use `this.counter` for state shared across their handlers.

## Host API contract

Frame handlers run with `this` bound to their owning timeline instance; object handlers run with `this` bound to the object. Named children and custom fields belong to that instance. The document identifies object events separately as `load`, `click` and `enter_frame`.

| API | Compilation/runtime contract |
| --- | --- |
| `play()`, `stop()` | Call the bound timeline object's method. |
| `gotoAndPlay(frame)`, `gotoAndStop(frame)` | Frame numbers are one-based at the author API; internal model frame indices are zero-based. Frame labels are resolved by the host where available. |
| `nextFrame()`, `prevFrame()` | Delegate playhead movement to the host. |
| `trace(...)` | Emits `console.log(...)`. |
| Named children | `child.play()` and `parent.child._x` become instance property access; each instance has its own runtime state. |
| Legacy properties | `_x`, `_y`, `_rotation`, `_alpha`, `_xscale`, `_yscale`, `_root`, `_parent`, `_currentframe`, `_totalframes` are preserved for the host aliases. `_alpha` and scale aliases use percentages; modern `alpha` and `scaleX/scaleY` use unit values. |
| Primitive globals | `Number`, `String`, `Boolean`, `parseInt`, `parseFloat`, `isNaN`, `isFinite` use their JavaScript counterparts. |
| `Math` | `abs`, `acos`, `asin`, `atan`, `atan2`, `ceil`, `cos`, `exp`, `floor`, `log`, `max`, `min`, `pow`, `random`, `round`, `sin`, `sqrt`, `tan`. |
| Array/string methods | `push`, `pop`, `shift`, `unshift`, `join`, `slice`, `indexOf`, `substring`, `charAt`, `charCodeAt`, `toString`, `toFixed`, `split`, `concat`, `reverse`; JavaScript behavior. |

Direct `on(...)`/`onClipEvent(...)` source wrappers are not parsed yet. Use the editor's object event selection with a handler body. Historically these wrappers belonged to AS1/AS2; AS3 did not permit direct object scripts in that form. NeoFlash's object-handler UI is a deliberate authoring abstraction for all four languages.

The browser host still needs independent conformance work for event ordering, first-frame creation, nested `goto` reentrancy, frame script ordering, buttons, loaders, destruction, sound and timeline lifetimes. Scrubbing/onion skin must evaluate authored state without executing user code. Passing a syntax test does not prove parity with Flash Player.

Checks: `cargo test -p neoflash-script` verifies parser/diagnostics and lowering invariants. `node crates/neoflash-script/tests/execute.mjs` compiles and executes emitted code in a real JavaScript engine, checking timeline calls, repeated-handler state, integer overflow, evaluation order, short-circuit writes, named children and TypeScript erasure/runtime output.

## Complete compatibility work remaining

1. Per-language conformance fixtures: parse, type resolution, output trace, runtime state, and expected diagnostics. Include edge cases for short-circuiting, coercions, integer overflow, string indexing and frame transitions.
2. AS2 source compiler integration evaluation (Rascal) and AVM1 compatibility strategy.
3. AS3 classes/packages/interfaces/namespaces/E4X, functions and exceptions; decide between a native Rust frontend and an isolated compatible compiler toolchain.
4. Flash API mapping by package, with explicit supported/adapted/unavailable decisions. Platform APIs are distinct from operators. No claim that all of `flash.*` or AIR works in a browser.
5. Document class/linkage and instance lifecycle; scripts on independent nested timelines; debugging and source maps.
6. Optional legacy playback/import lane using Ruffle, subject to integration tests and dependency licensing. A SWF hosted in a WASM player is not the native HTML5 export architecture.

## References and implementation evidence

- [Adobe AS3 operator reference](https://help.adobe.com/en_US/FlashPlatform/reference/actionscript/3/operators.html): includes AS-specific typing, namespaces and XML/E4X operators.
- [Adobe on organizing ActionScript](https://helpx.adobe.com/animate/desktop/platforms/best-practices-organizing-actionscript-application.html): historical distinction between AS2 object scripts and AS3 timeline/class code.
- [Ruffle compatibility](https://ruffle.rs/compatibility): distinguishes language support from platform API support. Percentages are upstream self-reported progress, not our compatibility promise.
- [Ruffle](https://github.com/ruffle-rs/ruffle) and [licensing](https://github.com/ruffle-rs/ruffle/blob/master/LICENSE.md): Rust/WASM player; primary code MIT OR Apache-2.0, with separately listed dependencies.
- [Rascal](https://github.com/Dinnerbone/Rascal): Rust AS2 compiler, MIT OR Apache-2.0; currently targets SWF.
- [Apache Royale compiler](https://apache.github.io/royale-docs/compiler): AS3/MXML to JavaScript; an existing compiler option, not a bundled NeoFlash dependency.
- [Oxc parser](https://docs.rs/oxc_parser/0.75.0/oxc_parser/) and [transformer](https://docs.rs/oxc_transformer/0.75.0/oxc_transformer/): current JavaScript/TypeScript implementation dependency. Versions are pinned; Oxc is MIT licensed.
- [Adobe HTML5 Canvas migration](https://helpx.adobe.com/ph_fil/animate/desktop/platforms/creating-publishing-html5-canvas-document.html): migration may comment out ActionScript and remove unsupported content; NeoFlash must report such gaps rather than silently lose behavior.
