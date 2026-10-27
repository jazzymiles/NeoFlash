# Source function arguments and invocation

Status: accepted for the bounded source-function profile, 2026-10-27.

## One source-level family

Functions are shared infrastructure for reusable components, callbacks and event dispatch. This delivery extends the existing AS2/AS3 frontend and per-publication class runtime together, rather than adding unrelated native API shims. AS3 optional constant defaults and a trailing rest parameter join the existing typed parameter and return handling. AS2 keeps its own source syntax and unbound receiver behavior. Arguments, indirect calls and project signature checks must use the same function contract.

Omission and an explicit undefined argument are distinct. Optional values are selected from the supplied argument count before the existing typed conversions. A rest value is a fresh Array. The activation retains the supplied count. AS3 arguments expose the converted supplied normal parameters and raw extra values; omitted defaults do not add entries. AS2 arguments retain the supplied values. Compiler-generated parameters never leak into the authored argument list. Reusable handlers must keep their own invocation state during recursion and nested calls.

## Receiver and ownership

AS3 project methods retain their original instance and stable extracted identity. AS2 project methods remain shared, writable, unbound functions. Ordinary function invocation can provide a receiver through call/apply, without changing the lexical owner used for unqualified timeline names. Null/undefined receivers of unbound functions use the captured source frame when available, as an explicit NeoFlash convention. A function without such a frame requires an explicit object receiver. This is not complete AVM global-scope behavior; JavaScript globalThis is never substituted. Explicit user overrides and receiver/method/argument evaluation order remain part of the call path.

Project constructors, overrides, interfaces and accessors share parameter metadata. Override/interface checks compare the supported parameter types, count and optionality; literal default values belong to the implementation. Full VM type/coercion behavior, prototype/reflection and historical global scope remain separate work.

## Evidence and bounds

Reuse the existing grouped compiler/project fixtures and one ordinary command-bus publication with AS2/AS3 callbacks, visible state, real input and Restart. Run one shared regression pass for the delivery. Calls are bounded to 16,384 supplied values. Function.length includes declared normal parameters, including optional positions, and excludes rest. Arguments are a fresh Array with a nonenumerable callee; AS3 rest suppresses the arguments binding. arguments.caller, primitive receiver boxing, arbitrary array-like apply inputs, extracted native call/apply aliases and dynamic Function construction remain outside the profile. Array/null/undefined apply inputs are accepted, with indexed values read in order after call-site expressions. The shared delivery pass and one command workflow passed; its fixture-only label correction and browser observations are recorded separately in VERIFICATION.md.

Independent `FUNCTIONS-HTML5-20261027T0914Z` on 11a59df confirms the representative optional/rest/supplied-count, bound/unbound receiver and corrected arity-refusal workflow with real input and Restart. Instrumentation is part of the authored contract: inserting arguments.length into a method enables this profile's extra-argument path. The initial altered probe is retained separately; only that trace was removed and the family re-exported, without repeating the shared batch. This is bounded browser evidence, not complete AVM/global-scope or original-player equivalence. See [the verification record](../VERIFICATION.md#independent-function-verification-of-11a59df).

References: Adobe/AIR [Function](https://airsdk.dev/reference/actionscript/3.0/Function.html) and the Adobe AS2 reference mirror [Function](https://open-flash.github.io/mirrors/as2-language-reference/Function.html). Original Flash execution is not part of this decision's current evidence.

Pinned implementation references: Ruffle `fe6cfc545405b888ebb478ce1c211d63147ca49b`, [declared normal signature length](https://github.com/ruffle-rs/ruffle/blob/fe6cfc545405b888ebb478ce1c211d63147ca49b/core/src/avm2/globals/function.rs#L99) and [argument construction](https://github.com/ruffle-rs/ruffle/blob/fe6cfc545405b888ebb478ce1c211d63147ca49b/core/src/avm2/activation.rs#L240). These are implementation references, not original-player measurements.
