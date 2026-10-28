# AS2 class accessors and explicit with scopes

This AS2-only package adds concrete class `get`/`set` accessors and a bounded `with` statement. Rust owns parsing, declaration analysis, completion analysis and reference lowering. It reuses the existing project class registry, accessor descriptors and property adapters. There is no new engine dependency, source interpreter, editor change or FLA conversion.

Public/private and static concrete class accessors use the existing class lookup and access rules. Getters have no arguments and return a value; setters have one argument and return Void. Compound writes read the getter, calculate the new value and invoke the setter through the shared property reference. Interface accessor signatures remain unsupported in this AS2 profile. Historical prototype mutation, exact AVM1 access-check behavior and native class subclassing are not newly implemented.

`with(object)` evaluates its target once, then selects existing properties from the innermost to outermost with targets, falling back to the existing function/class/timeline bindings. Reads, assignment, compound writes, increments, deletion, method receivers, nested blocks and ordinary control flow use those references. A name absent from all with objects falls back to its ordinary binding; it is not automatically created on the with target. Array and native calls use the existing adapters, including AS2 one-based timeline navigation. Targets must be non-null objects/functions; primitive boxing is outside this profile.

Function definitions and eval/set references inside the body diagnose explicitly because captured and dynamically evaluated scope chains require a further coherent extension. Define callbacks and resolve named paths outside the block. `this` and `super` retain their language binding. Statically linked type/constructor names keep the existing project linker rules. This is useful bounded source support, not AVM1 scope-chain conformance.

The Rust compiler still emits authored executable JavaScript for the current HTML5 publication architecture. Tiny generated reference helpers query live host objects and invoke existing adapters; they do not add a second parser or embed an external language engine. Rust runtime services compile to WASM. Authored execution is not a WASM VM.

`dynamic` declarations and global `Array(...)` / `Object(...)` calls use the ordinary-object profile. Complete dynamic-versus-sealed AS2 enforcement remains unfinished. Primitive boxes have a [separate supported profile](AS2-BOXING-BATCH.md).

Contracts were read in Adobe's mirrored [AS2 statements reference](https://open-flash.github.io/mirrors/as2-language-reference/statements.html) and [global functions reference](https://open-flash.github.io/mirrors/as2-language-reference/global_functions.html). This batch adapts the project's own compiler infrastructure; it copies no external source. Existing shared-module attribution remains in THIRD_PARTY_NOTICES.txt.

[as2-language-batch.nflash](../examples/as2-language-batch.nflash) combines accessors, independent instances, dynamic properties, nested with scopes and array/object function forms. Representative browser observations covered initial execution and its visible Run control; they do not establish complete AVM1 behavior.
