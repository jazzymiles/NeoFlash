# Script and runtime compatibility

Status: initial executable foundation, 26 October 2026. This is **not full Flash, AS2 or AS3 compatibility**. The product goal remains broad compatibility; this file describes only what the current implementation actually accepts.

## Languages

| Capability | Current state |
| --- | --- |
| JavaScript frame/object handlers | Parsed by Oxc as strict-mode function bodies, matching the HTML5 host, with syntax/early-error diagnostics; JavaScript is emitted without rewriting its expressions. |
| TypeScript frame/object handlers | Parsed, transformed and emitted by Oxc in Rust. Type annotations/interfaces are removed; supported runtime constructs such as enums are transformed. This does not run the TypeScript type checker. |
| JavaScript/TypeScript modules, imports, top-level await | Not supported inside a synchronous frame/object handler. Imports/exports must not be silently removed. A module/project build system is future work. |
| JSX/TSX, external transform helpers | Not part of this phase. Constructs requiring unbundled transformation helpers produce an error. |
| AS2/AS3 | Explicit bounded language frontend described below; independent lexer, recursive-descent statement parser, Pratt expression parser, AST, lexical scope analysis and JavaScript emitter. No regex transpilation. |
| Full AVM1/AVM2 behavior | Not implemented. Browser execution currently uses JavaScript semantics, with selected AS3 primitive coercions. |

The compiler API is `neoflash_script::compile(&Script) -> Result<String, Vec<Diagnostic>>`. Every diagnostic includes the source language, one-based line/column and a message. A failed script must block publishing; exporting an empty replacement would lose project behavior. A single script is limited to 1 MiB; the AS frontend also bounds token count and expression nesting to avoid runaway parsing.

## Accepted ActionScript subset

| Area | Accepted now | Limits |
| --- | --- | --- |
| Literals | Decimal, exponent and hexadecimal numbers; single/double-quoted strings; booleans, `null`, `undefined`, `NaN`, `Infinity`; arrays without holes; object literals | RegExp literals, XML/E4X, legacy leading-zero numbers and specialized literal semantics are pending. |
| Variables | `var name = value`, comma-separated declarations, frame/object fields; hoisted function-local variables and parameters | Top-level variables persist on `this`. Function locals use lexical scope, including variables declared inside blocks; class-slot semantics remain pending. |
| Type annotations | AS2 `Number`, `String`, `Boolean`, `Function`; AS3 also `int`, `uint`, `*`; return-only AS2 `Void` / AS3 `void` | AS2 annotations are erased. AS3 annotations affect defaults, bare-name writes, arguments and return values; they do not create globally enforced instance slots or a complete type checker. |
| Arithmetic | `+ - * / %`, unary `+ -`, prefix/postfix `++ --` | Uses JavaScript number/string operations; complete Flash coercion conformance is not claimed. |
| Comparison/logical | `== != === !== < <= > >=`, `!`, `&&`, `||`, conditional `?:`, `typeof` | Legacy AVM1 coercion/version differences remain pending. |
| Bitwise | `& \| ^ ~ << >> >>>` | JavaScript 32-bit semantics. |
| Assignment | `= += -= *= /= %= &= \|= ^= <<= >>= >>>= &&= \|\|=` | Compound assignment to a declared AS3 primitive is coerced. Property writes and cross-script writes do not have AS3 slot-type enforcement. |
| Access | `this`, bare instance fields/names, `object.property`, `object[index]` | Unknown field reads follow the browser host's behavior. There is no full AS scope-chain/prototype implementation. |
| Statements | Blocks, `if/else`, `while`, classic three-part `for`, `break`, `continue`, function declarations and `return`, expression statements | No classes, `switch`, `do/while`, `for each`, exception handling, `with`, namespaces, packages or imports. |
| Functions | Declarations, anonymous/named function expressions, positional parameters, recursion, nested closures, callbacks and immediate expression invocation | Declarations must be directly in a handler/function body. Optional/default/rest parameters, `arguments`, generators/async and Function.call/apply are pending. |

Rejected constructs include `is`, `as`, `instanceof`, `in`, `delete`, `new`, the unary `void` operator, namespace qualifier `::`, E4X `@`/`..`, `const` and classes. They produce diagnostics, not approximate replacements. Calls accept the APIs below plus declared user values and explicitly defined function members; unknown ActionScript calls remain errors.

Selected AS3 defaults: `Number` starts as `NaN`, `int`/`uint` as `0`, `Boolean` as `false`, `String`/`Function` as `null`, and `*`/untyped as `undefined`. Bare-name primitive assignments use the annotation of the nearest lexical binding; integer casts wrap to 32 bits. Function-typed values accept functions or null/undefined (converted to null), otherwise they throw TypeError. This is a useful subset, **not** a complete AS3 type system. For example, writing `this.count` from another handler does not enforce a `count:int` declaration in the first handler.

At handler level, declarations without an initializer preserve an already defined instance field on subsequent invocations; an explicit initializer runs each time. Those instance defaults are established when the declaration executes, not by a complete Flash class/slot initialization pass. Function-local defaults are established at function entry; their initializers run at their source position, and a later uninitialized `var` does not reset an earlier assignment. JavaScript/TypeScript `var`/`let`/`const` retain ordinary JavaScript scope; use `this.counter` for shared handler state.

## Function and closure semantics

Scope analysis collects declarations before checking calls. Direct function declarations are available before their source position, enabling recursion and mutual recursion. Handler-level declarations are installed as fields on the owning timeline/object before the handler statements execute; nested declarations remain local. Their definitions are recreated when that handler executes again. Local parameters and `var` bindings shadow enclosing locals and instance fields, and closures retain their lexical bindings after the outer function returns. Unqualified instance names and timeline calls inside a function resolve against the owner captured when the handler ran, even if the closure is later invoked by another object.

Local bindings may shadow built-ins such as `trace`, `Number`, `play` or `Math`; compiler coercions use separately captured intrinsics. A shadowed `trace()` invokes the user's binding. Unknown methods on a shadowed Math value still require an explicit user function definition. At handler level, built-in names remain reserved. All declaration/parameter names beginning with `__nf` are reserved for compiler-generated bindings and produce a diagnostic. Named function-expression self bindings support recursion but cannot be reassigned.

AS3 handler-level declared functions model bound instance methods: their explicit `this` remains the owning object when detached. Explicit `this` inside AS2 functions, nested declarations or function expressions is **rejected** because the required receiver/global-object differences are not implemented. Bare owner fields, lexical variables and explicit object parameters work in those functions. This boundary avoids claiming that JavaScript's strict-function receiver behavior matches every AVM1/AVM2 closure.

AS3 function entry enforces the exact positional argument count and coerces annotated arguments. Annotated returns are coerced once. A void/Void function cannot return a value. An explicitly typed AS3 non-void/non-`*` function must return a value on every path the frontend can prove: sequential returns and both branches of `if/else` are supported; unproven fallthrough is a compilation diagnostic. Untyped/`*` functions may return undefined. AS2 parameter/return annotations are erased and missing/extra arguments follow JavaScript behavior. Default/rest parameters and the `arguments` object require further compatibility work.

Declared variables/parameters can be called as dynamic user values (known AS3 primitive bindings cannot); non-callable dynamic values fail at runtime. Member calls outside the built-in API list require an explicit function declaration/assignment or a function-valued object-literal property at the same resolved receiver path. For example, defining `tools.run` does not permit `other.run` or an unrelated Flash API. Computed calls, general alias/type inference and cross-handler function declarations are not yet resolved by this per-script compiler.

## Host API contract

Frame handlers run with `this` bound to their owning timeline instance; object handlers run with `this` bound to the object. Named children and custom fields belong to that instance. The document identifies object events separately as `load`, `click` and `enter_frame`.

| API | Compilation/runtime contract |
| --- | --- |
| `play()`, `stop()` | Call the bound timeline object's method. |
| `gotoAndPlay(frame)`, `gotoAndStop(frame)` | Frame numbers are one-based at the author API; internal model frame indices are zero-based. Frame labels are resolved by the host where available. |
| `nextFrame()`, `prevFrame()` | Delegate playhead movement to the host. |
| `trace(...)` | Calls the host-provided trace function, updating console output and player status. Captured outside nested function scopes; a function-local user binding named trace still shadows it. |
| Named children | `child.play()` and `parent.child._x` become instance property access; each instance has its own runtime state. |
| Legacy properties | `_x`, `_y`, `_rotation`, `_alpha`, `_xscale`, `_yscale`, `_root`, `_parent`, `_currentframe`, `_totalframes` are preserved for the host aliases. `_alpha` and scale aliases use percentages; modern `alpha` and `scaleX/scaleY` use unit values. |
| Primitive globals | `Number`, `String`, `Boolean`, `parseInt`, `parseFloat`, `isNaN`, `isFinite` use their JavaScript counterparts. |
| `Math` | `abs`, `acos`, `asin`, `atan`, `atan2`, `ceil`, `cos`, `exp`, `floor`, `log`, `max`, `min`, `pow`, `random`, `round`, `sin`, `sqrt`, `tan`. |
| Array/string methods | `push`, `pop`, `shift`, `unshift`, `join`, `slice`, `indexOf`, `substring`, `charAt`, `charCodeAt`, `toString`, `toFixed`, `split`, `concat`, `reverse`; JavaScript behavior. |

Direct `on(...)`/`onClipEvent(...)` source wrappers are not parsed yet. Use the editor's object event selection with a handler body. Historically these wrappers belonged to AS1/AS2; AS3 did not permit direct object scripts in that form. NeoFlash's object-handler UI is a deliberate authoring abstraction for all four languages.

The browser host still needs independent conformance work for event ordering, first-frame creation, nested `goto` reentrancy, frame script ordering, buttons, loaders, destruction, sound and timeline lifetimes. Scrubbing/onion skin must evaluate authored state without executing user code. Passing a syntax test does not prove parity with Flash Player.

Checks: 20 Rust tests verify parser/diagnostics and lowering invariants. `node crates/neoflash-script/tests/execute.mjs` runs 30 executable behavior fixtures: timeline calls, repeated-handler state, integer overflow, evaluation order (including one numeric conversion per typed postfix update), short-circuit writes, named children, TypeScript output, function hoisting, local defaults, shadowing, recursion, closures, callbacks, early returns, typed arguments/returns, argument-count errors, supported receiver binding, escaped object-property names, postfix line breaks and assignment in conditional branches. An additional integration regression executes the actual `proxyFor`/`dispatchScripts` host code with a lifecycle stub, verifying independent instance state, retained closure ownership, replacement lifetimes and host trace/status updates with separate trace and console observations. These test NeoFlash's implemented subset; they are not a Flash Player conformance suite.

## Complete compatibility work remaining

1. Per-language conformance fixtures: parse, type resolution, output trace, runtime state, and expected diagnostics. Include edge cases for short-circuiting, coercions, integer overflow, string indexing and frame transitions.
2. AS2 source compiler integration evaluation (Rascal) and AVM1 compatibility strategy.
3. AS3 classes/packages/interfaces/namespaces/E4X and exceptions; complete function defaults/rest/receiver/type semantics and cross-script symbol resolution; evaluate a compatible compiler toolchain alongside the bounded Rust frontend.
4. Flash API mapping by package, with explicit supported/adapted/unavailable decisions. Platform APIs are distinct from operators. No claim that all of `flash.*` or AIR works in a browser.
5. Document class/linkage and instance lifecycle; scripts on independent nested timelines; debugging and source maps.
6. Optional legacy playback/import lane using Ruffle, subject to integration tests and dependency licensing. A SWF hosted in a WASM player is not the native HTML5 export architecture.

## References and implementation evidence

- [Adobe AS3 operator reference](https://help.adobe.com/en_US/FlashPlatform/reference/actionscript/3/operators.html): includes AS-specific typing, namespaces and XML/E4X operators.
- [Adobe Learning ActionScript 3.0](https://help.adobe.com/en_US/as3/learn/as3_learning.pdf), functions section: lexical function scope/closures, methods, argument/return annotations, default/rest parameters and the arguments object. This describes the target language, not blanket support in the current compiler.
- [Adobe on organizing ActionScript](https://helpx.adobe.com/animate/desktop/platforms/best-practices-organizing-actionscript-application.html): historical distinction between AS2 object scripts and AS3 timeline/class code.
- [Ruffle compatibility](https://ruffle.rs/compatibility): distinguishes language support from platform API support. Percentages are upstream self-reported progress, not our compatibility promise.
- [Ruffle](https://github.com/ruffle-rs/ruffle) and [licensing](https://github.com/ruffle-rs/ruffle/blob/master/LICENSE.md): Rust/WASM player; primary code MIT OR Apache-2.0, with separately listed dependencies.
- [Rascal](https://github.com/Dinnerbone/Rascal): Rust AS2 compiler, MIT OR Apache-2.0; currently targets SWF.
- [Apache Royale compiler](https://apache.github.io/royale-docs/compiler): AS3/MXML to JavaScript; an existing compiler option, not a bundled NeoFlash dependency.
- [Oxc parser](https://docs.rs/oxc_parser/0.75.0/oxc_parser/) and [transformer](https://docs.rs/oxc_transformer/0.75.0/oxc_transformer/): current JavaScript/TypeScript implementation dependency. Versions are pinned; Oxc is MIT licensed.
- [Adobe HTML5 Canvas migration](https://helpx.adobe.com/ph_fil/animate/desktop/platforms/creating-publishing-html5-canvas-document.html): migration may comment out ActionScript and remove unsupported content; NeoFlash must report such gaps rather than silently lose behavior.
