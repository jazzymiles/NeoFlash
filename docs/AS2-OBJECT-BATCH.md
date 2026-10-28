# AS2 dynamic objects and named access

AS2 supports bounded `Object.watch`, `unwatch`, `addProperty`, `isPropertyEnumerable` and missing-property `__resolve`. Watchpoint identity, replacement/deletion lifetime, reentrant-write suppression and resolver recursion belong to `neoflash-core::as2_object`. Browser adapters retain authored references and invoke getters/setters without modifying the global prototype.

The Rust compiler lowers AS2 member reads/writes through that publication service.
Ordinary records and project instances support watchpoints; native objects,
arrays and existing native/getter/setter properties remain outside this watch
profile. A watch callback receives name/old/new/userData and its returned value is
stored. Deleting a watched property preserves its watchpoint. Computed properties
require a nonempty name, a getter and a function or null setter. Missing-property
resolution does not run for an existing property whose value is undefined.
Limits are 4,096 watchpoints/accessors, 4,096 UTF-16 units per name and 128 nested
resolutions. Restart releases all callback references and owned state.

AS2 `instanceof` supports statically resolved project/native types with an object
operand; arbitrary constructor expressions and prototype mutation conformance
remain unsupported. `on(press, release, releaseOutside, rollOver, rollOut,
dragOver, dragOut)` and `onClipEvent(enterFrame, mouseDown, mouseMove, mouseUp,
unload)` lower to the connected legacy event bridge. Multiple names share one
authored handler. Blocks belong in top-level object scripts; keyPress filters,
AVM action-tag ordering and clip-wide keyboard broadcasts remain unsupported.

`eval(path)` performs named reference lookup, never JavaScript source evaluation.
`set(path,value)` uses the same Rust dotted/slash/colon path parser and compiler
supplied lexical references, including locals and project fields. Direct object
arguments to eval preserve identity. Paths support `_root`, `_level0`, `_parent`,
`this` and the publication `_global`; arbitrary computed expressions, indexed
source syntax and eval/set function aliases diagnose or remain outside the path
profile. No implicit browser globals are exposed through `_global`; it is an
owned publication-wide user-property namespace, cleared on Restart. `newline`
is the LF constant.

## References and adaptation

- [Adobe AS2 Object contract](https://open-flash.github.io/mirrors/as2-language-reference/Object.html)
- [Adobe AS2 global functions](https://open-flash.github.io/mirrors/as2-language-reference/global_functions.html)
- [Adobe AS2 statements](https://open-flash.github.io/mirrors/as2-language-reference/statements.html)
- Ruffle, commit `89a7049965f3090e706265670df309a5128233ac`,
  `core/src/avm1/globals/object.rs` and the AVM1 watchpoint model. MIT terms and
  attribution retained in `THIRD_PARTY_NOTICES.txt`. The callback contracts were
  adapted to owned bounded IDs/maps rather than Ruffle GC/Activation/Value types.
  Compiler lowering and integration reuse NeoFlash's own frontend/class registry.

[as2-object-batch.nflash](../examples/as2-object-batch.nflash) combines object behavior, local/path lookup, project classes and a legacy event button. It demonstrates useful partial behavior, not complete AVM1 or original-player equivalence.
