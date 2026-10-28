# Rust Math adaptation

The shared numeric family adapts individual functions from Ruffle at `89a7049965f3090e706265670df309a5128233ac`: [AVM1 Math](https://github.com/ruffle-rs/ruffle/blob/89a7049965f3090e706265670df309a5128233ac/core/src/avm1/globals/math.rs) and [AVM2 Math](https://github.com/ruffle-rs/ruffle/blob/89a7049965f3090e706265670df309a5128233ac/core/src/avm2/globals/math.rs). The selected MIT notice is in THIRD_PARTY_NOTICES.txt and accompanies exports. No Ruffle VM or numeric dependency is embedded.

The Rust implementation retains the standard-library function mappings, Flash half-rounding direction, AVM2 extrema's NaN/signed-zero handling, selected non-finite power rules, and 31-bit random fractions. It replaces VM arguments/GC objects with an explicit dialect flag and a borrowed f64 slice. Constants come from Rust's standard library. JavaScript only converts arguments in the dialect's observable order and supplies host entropy. The old AS2 random-range calculation was removed from the emitted JavaScript helper.

This migrates 18 shared Math methods and the existing AS2 random global; no new method names are claimed. Eight constants are sourced from Rust and counted separately. The same 18 methods have AS2 and AS3 bindings, not 36 independent algorithms.

AS2 uses the SWF7+ profile: Math methods coerce the first two supplied/default arguments, min/max use two values, and zero-argument extrema return infinities. AS3 extrema accept many arguments and stop coercing after NaN. Native API arity, complete AVM number conversion, exact libm results across platforms and the historical seeded/locale RNG sequence remain partial. Host entropy is quantized in Rust; the host entropy generator itself is not a Rust implementation.

One grouped Rust test and the ordinary runtime-math.nflash scenario cover the family. Execution, browser acceptance, counts and timings belong to the hour's shared delivery report rather than this provenance record.
