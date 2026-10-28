# AS2 TextField.variable

`TextField.variable` provides bounded two-way binding between a plain TextField and a primitive MovieClip variable. It uses existing AS2 Object identity/path/watch state and primitive binding conversion.

Setting `field.variable = "score"` resolves score relative to the field's parent MovieClip. Root/slash/colon paths use the existing named-variable parser; `_root.score` can bind multiple fields to the same value. An existing variable initializes the text. A missing variable is seeded from nonempty current field text; empty text does not fabricate a variable. A temporarily unresolved MovieClip path is retried at synchronization boundaries. String, finite Number, Boolean, null and undefined values use the shared Rust binding conversion. Arbitrary object/Date/function coercion and HTML fields are explicitly outside this profile.

Programmatic AS2 `.text` writes and native input changes write the bound variable and refresh other bound fields. Native input propagation runs before the authored AS2 onChanged callback. Variable-to-text synchronization occurs after intercepted object writes, on AS2 text reads, and at the end of frame/object script dispatch. Bare timeline variable assignments are therefore observed on a subsequent AS2 text read or script-dispatch boundary; this does not claim an eager setter hook for every JavaScript assignment.

Reassigning variable detaches the old path and initializes from the new one. Null, undefined and the empty string detach, and the unbound getter returns null. Removing a field releases its binding at the next synchronization; Restart clears all bindings and identities. The Rust state limits a publication to 256 bindings, 128 path components, 4096 UTF-16 path units and 1 MiB field/converted text. A Rust reentrancy guard prevents feedback while the browser applies a binding action. Host code retains only live object references and invokes the existing read/write paths, preserving TextField layout updates.

This partial profile uses the current field text when a new missing variable is seeded and retains that text on detach. It does not reproduce the historical reset to the SWF-authored initial text on every variable assignment. HTML serialization bindings, custom object coercion, native display-property binding, complete prototype/accessor coercion and all historical lazy-resolution timing remain outside the supported contract. Bound targets must be MovieClips; ordinary-object data fields should use the existing explicit Binding API.

## Provenance

The public contract is the [Adobe AS2 TextField.variable reference](https://open-flash.github.io/mirrors/as2-language-reference/TextField.html#variable). Initialization, unset/null behavior and propagation direction were examined in MIT-licensed Ruffle, pinned commit `89a7049965f3090e706265670df309a5128233ac`: [AVM1 text-field properties](https://github.com/ruffle-rs/ruffle/blob/89a7049965f3090e706265670df309a5128233ac/core/src/avm1/globals/text_field.rs) and [EditText binding lifecycle](https://github.com/ruffle-rs/ruffle/blob/89a7049965f3090e706265670df309a5128233ac/core/src/display_object/edit_text.rs). The bounded lifecycle policy is adapted into the existing owned Rust Object state, without its VM, garbage collector or display engine. Existing Ruffle copyright and full MIT notice in THIRD_PARTY_NOTICES.txt apply.

Reference downloads stay under ignored `target/verification/as2-two-hour-2/text-variable-reference/`. Their SHA-256 values are `deccf5fbaa597606d546b95d3ba7ca961c20e5a2c470956f28c8962badda1bdf` (text_field.rs) and `abe09eb0318fc3a19ddda3a8fc289f6d0ff56a2cb85ccf61090e9b315b041abb` (edit_text.rs). Shared primitive conversion retains its existing module provenance.

## Example and verification boundary

[as2-text-variable-batch.nflash](../examples/as2-text-variable-batch.nflash) includes an editable field, a mirror and a button that changes the MovieClip variable. Representative browser observations covered actual typing, mirror synchronization, the button update and Restart removing the old input control. Complete historical TextField binding conformance remains unverified.
