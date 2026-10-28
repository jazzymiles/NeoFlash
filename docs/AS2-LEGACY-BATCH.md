# AS2 legacy syntax and target globals

AS2 supports the deprecated source operators: `add`, `eq`, `ne`, `lt`, `le`, `gt`, `ge`, `and`, `or`, `not`, and `<>`. The Rust frontend parses their AS2 precedence and rejects the infix forms in AS3. UTF-16 concatenation/comparison, Boolean operations and numeric inequality execute in the shared Rust string service. The browser bridge only evaluates/coerces operands and transports code units or numbers. Legacy `and`/`or` evaluate both operand expressions and return Boolean values; modern `&&`/`||` retain their existing behavior.

Seven global entry points connect to the owned display graph: `getProperty`, `setProperty`, `targetPath`, `duplicateMovieClip`, `removeMovieClip`, `startDrag`, and `stopDrag`. Rust maps numeric property indices and resolves display paths. The wrappers share existing display mutations and the AS2 method bridge. Object arguments and absolute `_root`/`_level0`/slash paths work. Relative string paths currently resolve from the publication root, not the caller MovieClip. Only runtime-created clips can be duplicated; authored/linked timeline duplication diagnoses. Unsupported numeric property slots diagnose, and read-only assignments keep the underlying display diagnostics.

`parseInt` uses Rust arithmetic with the existing conversion order, automatic/specified radix differences and signed-octal handling. Complete historical player-version coercion parity is not claimed. The transport keeps NaN/infinity/signed zero as strings. Inputs retain the shared one-million UTF-16-unit and 32 MiB transport limits.

## Reuse

Individual opcode contracts and the integer parser were adapted after reading Ruffle at pinned revision [`89a7049965f3090e706265670df309a5128233ac`](https://github.com/ruffle-rs/ruffle/tree/89a7049965f3090e706265670df309a5128233ac): `core/src/avm1/activation.rs` (`action_and`, `action_or`, `action_equals`, `action_string_add`, string comparison) and `core/src/avm1/globals.rs` (`parse_int_internal`). The AS2 reference operator descriptions are preserved in the [Open Flash documentation mirror](https://open-flash.github.io/mirrors/as2-language-reference/operators.html). We select Ruffle's MIT license; the existing exact notice in `THIRD_PARTY_NOTICES.txt` ships with exports. No VM, compiler, renderer or third-party runtime is embedded. Source downloads remain under ignored `target/`.

## Example and limits

[as2-legacy-batch.nflash](../examples/as2-legacy-batch.nflash) combines legacy operators, eager Boolean evaluation, precedence, UTF-16, integer parsing, target paths, duplicate/remove globals and bounded drag control. Representative browser execution and Restart were observed.

SWF4 numeric Boolean values, arbitrary AVM coercion, every parser combination and original-player equivalence remain outside this partial profile.
