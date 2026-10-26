# Script and runtime compatibility

Status: initial executable foundation, 27 October 2026. This is **not full Flash, AS2 or AS3 compatibility**. The product goal remains broad compatibility; this file describes only what the current implementation actually accepts.

## Languages

| Capability | Current state |
| --- | --- |
| JavaScript frame/object handlers | Parsed by Oxc as strict-mode function bodies, matching the HTML5 host, with syntax/early-error diagnostics; JavaScript is emitted without rewriting its expressions. |
| TypeScript frame/object handlers | Parsed, transformed and emitted by Oxc in Rust. Type annotations/interfaces are removed; supported runtime constructs such as enums are transformed. This does not run the TypeScript type checker. |
| JavaScript/TypeScript modules, imports, top-level await | Not supported inside a synchronous frame/object handler. Imports/exports must not be silently removed. A module/project build system is future work. |
| JSX/TSX, external transform helpers | Not part of this phase. Constructs requiring unbundled transformation helpers produce an error. |
| AS2/AS3 | Explicit bounded language frontend described below; independent lexer, recursive-descent statement parser, Pratt expression parser, AST, lexical scope analysis and JavaScript emitter. No regex transpilation. |
| Full AVM1/AVM2 behavior | Not implemented. Browser execution uses JavaScript semantics with selected AS3 primitive coercions and the bounded AS2 timeline behavior below. |

The compiler API is `neoflash_script::compile(&Script) -> Result<String, Vec<Diagnostic>>`. Every diagnostic includes the source language, one-based line/column and a message. A failed script must block publishing; exporting an empty replacement would lose project behavior. A single script is limited to 1 MiB; the AS frontend also bounds token count, parsing nesting (48 levels) and expression-tree depth to avoid runaway parsing.

## Accepted ActionScript subset

| Area | Accepted now | Limits |
| --- | --- | --- |
| Literals | Decimal, exponent and hexadecimal numbers; single/double-quoted strings; booleans, `null`, `undefined`, `NaN`, `Infinity`; arrays without holes; object literals | RegExp literals, XML/E4X, legacy leading-zero numbers and specialized literal semantics are pending. |
| Variables | `var name = value`, comma-separated declarations, frame/object fields; hoisted function-local variables and parameters | Top-level variables persist on `this`. Function locals use lexical scope, including variables declared inside blocks; class-slot semantics remain pending. |
| Type annotations | AS2 `Number`, `String`, `Boolean`, `Function`, `Error`; AS3 also `int`, `uint`, `*` and the selected core Error family; return-only AS2 `Void` / AS3 `void` | AS2 annotations are erased. AS3 annotations affect defaults, bare-name writes, arguments and return values; they do not create globally enforced instance slots or a complete type checker. |
| Arithmetic | `+ - * / %`, unary `+ -`, prefix/postfix `++ --` | Uses JavaScript number/string operations; complete Flash coercion conformance is not claimed. |
| Comparison/logical | `== != === !== < <= > >=`, `!`, `&&`, `||`, conditional `?:`, `typeof` | Legacy AVM1 coercion/version differences remain pending. |
| Bitwise | `& \| ^ ~ << >> >>>` | JavaScript 32-bit semantics. |
| Assignment | `= += -= *= /= %= &= \|= ^= <<= >>= >>>= &&= \|\|=` | Compound assignment to a declared AS3 primitive is coerced. Property writes and cross-script writes do not have AS3 slot-type enforcement. |
| Access | `this`, bare instance fields/names, `object.property`, `object[index]` | Unknown field reads follow the browser host's behavior. There is no full AS scope-chain/prototype implementation. |
| Statements | Blocks, `if/else`, `while`, `do/while`, classic three-part `for`, `switch` with strict comparison and fallthrough, `break`, `continue`, AS3 labels, function declarations/`return`, `throw`, `try/catch/finally`, expression statements | Exception and AS2 catch boundaries are described below. No classes, `for each`, `with`, namespaces, packages or imports. |
| Functions | Declarations, anonymous/named function expressions, positional parameters, recursion, nested closures, callbacks and immediate expression invocation | Declarations must be directly in a handler/function body. Optional/default/rest parameters, `arguments`, generators/async and Function.call/apply are pending. |

Rejected constructs include `is`, `as`, `instanceof`, `in`, `delete`, arbitrary `new` (selected Error constructors are supported), the unary `void` operator, namespace qualifier `::`, E4X `@`/`..`, `const` and classes. They produce diagnostics, not approximate replacements. Calls accept the APIs below plus declared user values and explicitly defined function members. AS2 additionally resolves unqualified dynamic timeline calls at execution time; a missing or non-callable value produces an actionable runtime error. Unknown member APIs and unresolved AS3 calls still produce compilation errors.

Selected AS3 defaults: `Number` starts as `NaN`, `int`/`uint` as `0`, `Boolean` as `false`, `String`/`Function` as `null`, and `*`/untyped as `undefined`. Bare-name primitive assignments use the annotation of the nearest lexical binding; integer casts wrap to 32 bits. Function-typed values accept functions or null/undefined (converted to null), otherwise they throw TypeError. This is a useful subset, **not** a complete AS3 type system. For example, writing `this.count` from another handler does not enforce a `count:int` declaration in the first handler.

At handler level, declarations without an initializer preserve an already defined instance field on subsequent invocations; an explicit initializer runs each time. Those instance defaults are established when the declaration executes, not by a complete Flash class/slot initialization pass. Function-local defaults are established at function entry; their initializers run at their source position, and a later uninitialized `var` does not reset an earlier assignment. JavaScript/TypeScript `var`/`let`/`const` retain ordinary JavaScript scope; use `this.counter` for shared handler state.

## Function and closure semantics

Scope analysis collects declarations before checking calls. Direct function declarations are available before their source position, enabling recursion and mutual recursion. Handler-level declarations are installed as fields on the owning timeline/object before the handler statements execute; nested declarations remain local. Their definitions are recreated when that handler executes again. Local parameters and `var` bindings shadow enclosing locals and instance fields, and closures retain their lexical bindings after the outer function returns. Unqualified instance names and timeline calls inside a function resolve against the owner captured when the handler ran, even if the closure is later invoked by another object.

Local bindings may shadow built-ins such as `trace`, `Number`, `play` or `Math`; compiler coercions use separately captured intrinsics. A shadowed `trace()` invokes the user's binding. Unknown methods on a shadowed Math value still require an explicit user function definition. At handler level, built-in names remain reserved. All declaration/parameter names beginning with `__nf` are reserved for compiler-generated bindings and produce a diagnostic. Named function-expression self bindings support recursion but cannot be reassigned.

AS3 handler-level declared functions model bound instance methods: their explicit `this` remains the owning object when detached. Explicit `this` inside AS2 functions, nested declarations or function expressions is **rejected** because the required receiver/global-object differences are not implemented. Bare owner fields, lexical variables and explicit object parameters work in those functions. This boundary avoids claiming that JavaScript's strict-function receiver behavior matches every AVM1/AVM2 closure.

AS3 function entry enforces the exact positional argument count and coerces annotated arguments. Annotated returns are coerced once. A void/Void function cannot return a value. An explicitly typed AS3 non-void/non-`*` function must return a value or throw on every path the frontend can prove. A conservative completion analysis handles branches, switch fallthrough, loops, labels, catches and finally overrides; unproven normal fallthrough is a compilation diagnostic. It does not prove arbitrary infinite loops. Untyped/`*` functions may return undefined. AS2 parameter/return annotations are erased and missing/extra arguments follow JavaScript behavior. Default/rest parameters and the `arguments` object require further compatibility work.

Declared variables/parameters can be called as dynamic user values (known AS3 primitive bindings cannot); non-callable dynamic values fail at runtime. Member calls outside the built-in API list require an explicit function declaration/assignment or a function-valued object-literal property at the same resolved receiver path. For example, defining `tools.run` does not permit `other.run` or an unrelated Flash API. Direct computed calls and general alias/type inference remain unsupported.

AS2 permits an unqualified call such as `run()` to find a function installed on its owning timeline by an earlier frame or parent script. It resolves against the captured timeline owner, checks that the value is callable, and preserves that owner for calls inside retained closures. A missing value reports the function name and originating script location. This does not add AVM1 prototype/scope-chain lookup or implement an otherwise absent Flash API. Method lookup precedes argument evaluation; callability checks follow argument evaluation. JavaScript call/receiver semantics remain the underlying subset, not a full AVM1 call implementation.

## Control flow and exceptions

`switch` evaluates its subject once and tests cases in source order until a match; case bodies can fall through without re-evaluating later tests. `do/while` runs its body before testing. Break and continue targets are validated separately, including switch inside loops. AS3 permits labels on statements and labelled continue to enclosing iteration statements; targets do not cross function boundaries. AS2 labels remain diagnosed pending source-compiler compatibility evidence.

`throw` requires a same-line expression. `try` accepts catch clauses, finally, or both. Finally runs for normal completion, returns, exceptions and loop/label jumps, and its own abrupt completion replaces the previous one. Return coercion occurs at the return site before finally. Lowering uses native structured statements, preserving control flow rather than placing user blocks in helper functions.

AS3 catch parameters have their own lexical environment. They can shadow outer locals, owner fields and built-ins; retained closures capture the particular catch binding. Function/frame `var` declarations retain their existing declaration owner. Catch matching is ordered and does not coerce values. Supported AS3 catch types are `*`/untyped, `Object`, `Function`, `Number`, `String`, `Boolean`, `int`, `uint`, and `Error`, `TypeError`, `RangeError`, `ReferenceError`, `SyntaxError`, `EvalError`, `URIError`, `ArgumentError`. Null and undefined match only catch-all. The current primitive predicates cover JavaScript primitive representations; boxed primitive objects, custom classes/interfaces and namespaces are outside this boundary. Unsupported catch types produce diagnostics. Subsequent bare-name writes use implemented primitive/Error type coercions; Object catch writes normalize undefined to null while preserving other values.

AS2 supports untyped catches and `catch(e:Error)`. Catch parameter names must be distinct from enclosing declarations and built-ins; outside-catch references to otherwise unresolved catch names and function definitions/expressions inside catches are diagnosed. These restrictions avoid inventing AVM1 variable/register collision and capture behavior. Other AS2 typed catches, including String, are not approximated with AS3 predicates: upstream AVM1 evidence distinguishes primitive strings from boxed String values.

Explicit AS2 throws travel in `{ [Symbol.for('neoflash.as2.thrown.v1')]: true, value: originalPayload }` envelopes, recognized across separately compiled handlers. A catch sees the original payload; rethrows preserve payload identity, while an unmatched catch propagates its envelope. AS2 catches do not swallow native JavaScript/host failures; finally still executes. AS3 throws raw values. JS/TS interoperability with these envelopes is an explicit boundary, not a sandbox or a claim of complete AVM1 runtime error behavior. General mixed-language catch conversion is not implemented.

`new Error(...)` is available in both languages; AS3 also accepts the listed core Error-family constructors. AS2 accepts at most one argument, AS3 at most two. These are a closed facade, not arbitrary constructor/class support. AS3 selected Error types also work in variable, argument and return annotations: null/undefined become null, matching Error-family objects preserve identity, and incompatible bare-name writes throw TypeError. AS2 Error annotations are erased. Constructor aliases/class values, getStackTrace, platform-specific subclasses and Flash runtime error numbers remain unsupported.

AS3 Error construction preserves the message value without immediate String coercion, converts the id to int once, and exposes read-only `errorID`; string conversion happens when the error is formatted, and toString applies the implemented nullable String return coercion even if name was reassigned. This follows [Adobe avmplus Error.as at the inspected revision](https://github.com/adobe/avmplus/blob/858d034a3bd3a54d9b70909386435cf4aec81d21/core/Error.as), whose untyped constructor differs from the published String signature. AS2 missing/undefined message defaults to `"Error"`; its toString returns the message, following [the inspected AVM1 implementation](https://github.com/ruffle-rs/ruffle/blob/fe6cfc545405b888ebb478ce1c211d63147ca49b/core/src/avm1/globals/Error.as). This facade does not reproduce prototype enumeration, all error properties or Flash stack traces.

Runtime error families generated by the browser/earlier compiler subset remain browser approximations. For example, current exact-argument-count checks throw TypeError, not a fully mapped AVM2 error/errorID. Catch support does not make every underlying JavaScript operation Flash-compatible. The host reports uncaught arbitrary values with guarded formatting and preserves their original cause.

## Host API contract

Frame handlers run with `this` bound to their owning timeline instance; object handlers run with `this` bound to the object. Named children and custom fields belong to that instance. The document identifies object events separately as `load`, `click` and `enter_frame`.

| API | Compilation/runtime contract |
| --- | --- |
| `play()`, `stop()` | Call the bound timeline object's method. |
| `gotoAndPlay(frame)`, `gotoAndStop(frame)` | Frame numbers are one-based at the author API; internal model indices are zero-based. AS2 direct calls use the explicit label/numeric boundary below. JS/AS3 retain the existing strict host behavior. |
| `nextFrame()`, `prevFrame()` | Delegate playhead movement to the host. |
| `trace(...)` | Calls the host-provided trace function, updating console output and player status. Captured outside nested function scopes; a function-local user binding named trace still shadows it. |
| Named children | `child.play()` and `parent.child._x` become instance property access; each instance has its own runtime state. |
| Legacy properties | `_x`, `_y`, `_rotation`, `_alpha`, `_xscale`, `_yscale`, `_root`, `_parent`, `_currentframe`, `_totalframes` are preserved for the host aliases. `_alpha` and scale aliases use percentages; modern `alpha` and `scaleX/scaleY` use unit values. |
| Primitive globals | `Number`, `String`, `Boolean`, `parseInt`, `parseFloat`, `isNaN`, `isFinite` use their JavaScript counterparts. |
| `Math` | `abs`, `acos`, `asin`, `atan`, `atan2`, `ceil`, `cos`, `exp`, `floor`, `log`, `max`, `min`, `pow`, `random`, `round`, `sin`, `sqrt`, `tan`. |
| Array/string methods | `push`, `pop`, `shift`, `unshift`, `join`, `slice`, `indexOf`, `substring`, `charAt`, `charCodeAt`, `toString`, `toFixed`, `split`, `concat`, `reverse`; JavaScript behavior. |

### AS2 timeline navigation boundary

Direct AS2 `gotoAndPlay`/`gotoAndStop` calls compare labels with ASCII-only case folding and select the earliest matching frame across layers. `END` can match `end`; `BADÀ` does not match `BADà`. An unknown label is a no-op that preserves both the frame and its play/stop state. A successful jump changes `_currentframe` immediately while destination-frame scripts remain queued for the host's normal dispatch. The separate bridge is captured by AS2 closures, so invoking one from JS or AS3 does not change its navigation behavior.

Decimal numeric strings are parsed before label lookup: `"3"` targets author frame 3 even when a label named `3` exists elsewhere. The implemented numeric range is integer frames 1 through the timeline length. Zero/negative values, values above the last frame, fractions, non-finite numbers and wrapping/clamping cases produce explicit unsupported-behavior errors; other argument coercions are not implemented. This intentionally bounded numeric behavior is not the complete [AVM1 goto behavior in the pinned Ruffle implementation](https://github.com/ruffle-rs/ruffle/blob/fe6cfc545405b888ebb478ce1c211d63147ca49b/core/src/avm1/globals/movie_clip.rs#L1113).

Native AS2 goto function values/aliases remain unsupported because detached receiver semantics have not been established. Known direct forms such as `var jump = gotoAndStop` or `var jump = clip.gotoAndStop` produce compilation diagnostics. Calls through computed/copied aliases are also checked by native method identity and fail explicitly without changing the timeline. Ordinary user functions and local variables/parameters shadowing `gotoAndStop` retain their own behavior. Arbitrary computed member calls remain compilation errors. JS/AS3 navigation stays strict: exact label matching, the existing label-first numeric-string handling, and errors for unknown labels or invalid frame numbers. None of these rules claims complete Flash navigation compatibility.

Direct `on(...)`/`onClipEvent(...)` source wrappers are not parsed yet. Use the editor's object event selection with a handler body. Historically these wrappers belonged to AS1/AS2; AS3 did not permit direct object scripts in that form. NeoFlash's object-handler UI is a deliberate authoring abstraction for all four languages.

The browser host still needs independent conformance work for event ordering, first-frame creation, nested `goto` reentrancy, frame script ordering, buttons, loaders, destruction, sound and timeline lifetimes. Scrubbing/onion skin must evaluate authored state without executing user code. Passing a syntax test does not prove parity with Flash Player.

Checks: 28 script-crate Rust tests verify parser/diagnostics, lowering invariants and the AS2 dynamic-call/alias boundary; four core tests cover AS2 label resolution. `node crates/neoflash-script/tests/execute.mjs` runs 62 executable behavior fixtures: timeline calls, repeated-handler state, integer overflow, evaluation order (including one numeric conversion per typed postfix update), short-circuit writes, named children, TypeScript output, function hoisting, local defaults, shadowing, recursion, closures, callbacks, early returns, typed arguments/returns, argument-count errors, supported receiver binding, escaped object-property names, postfix line breaks, assignment in conditional branches, switch test order/fallthrough, do/continue, labelled jumps, finally completion precedence, catch closures and shadowing, per-language throw identity, typed catch/slot checks and Error message/id evaluation order. Integration checks execute the actual `proxyFor`/`dispatchScripts` host code for lifecycle, trace/status, language isolation, numeric precedence, unknown-label no-ops, dynamic callable failures and alias rejection. The public FLA comparison below additionally runs the actual WASM timeline. These test NeoFlash's implemented subset; they are not a Flash Player conformance suite.

## Complete compatibility work remaining

1. Per-language conformance fixtures: parse, type resolution, output trace, runtime state, and expected diagnostics. Include edge cases for short-circuiting, coercions, integer overflow, string indexing and frame transitions.
2. AS2 source compiler integration evaluation (Rascal) and AVM1 compatibility strategy.
3. AS3 classes/packages/interfaces/namespaces/E4X, remaining exception/type conformance; complete function defaults/rest/receiver/type semantics and cross-script symbol resolution; evaluate a compatible compiler toolchain alongside the bounded Rust frontend.
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

## Public source fixtures

The [FLA corpus](../examples/fla-corpus/README.md) retains 15 unchanged upstream FLA sources and three original synthetic examples. Its 21 input forms now produce seven successful imports and 14 explicit refusals. One real upstream document, Ruffle's [frame-labels FLA](../examples/fla-corpus/upstream/ruffle/avm1-frame-labels/test.fla), imports directly to [an editable project](../examples/fla-corpus/imported/ruffle-avm1-frame-labels.nflash). Its geometry uses only the bounded convex solid/stroked XFL subset described in [import compatibility](IMPORTS.md).

`node --test web/tests/real-fla-frame-labels.test.cjs` imports the unchanged FLA, compiles all nine original AS2 frame scripts, and executes them through the actual WASM timeline and browser-host proxy/dispatch code. It matches the upstream 17-line trace exactly, verifies the stopped root at author frame 3 while the child loops to its own stop frame, and repeats the exact trace after reset with a fresh instance lifetime. The three original examples retain their separate import/runtime checks; see [the verification record](../examples/fla-corpus/VERIFICATION.md) for browser observations. The other 14 upstream FLAs remain blocked, and their expected outputs are reference material. Import retention alone never establishes ActionScript compatibility, and this single successful source does not establish general AS2/AS3 or binary FLA support.
