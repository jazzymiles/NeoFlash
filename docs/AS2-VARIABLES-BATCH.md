# AS2 timeline variable loading

`loadVariables`, level-zero `loadVariablesNum`, `MovieClip.loadVariables` and
`MovieClip.onData` now use the existing bounded browser transport and a new owned
Rust form decoder. A GET without an explicit method loads data only; explicit
GET/POST sends the calling timeline's serializable variables. Method calls send
the receiver's variables. Completion writes decoded fields to the target before
calling onData with the target as this; later requests replace pending requests
for the same target, and Restart cancels pending publication work.

The profile supports bundled immutable GET resources and browser HTTP(S), UTF-8,
plus-to-space and percent decoding, repeated fields as arrays, at most 1,024
pairs and 16 MiB per response. Malformed input diagnoses before applying fields.
There are no additional SWF levels, server-side interpreters, policy-file bypasses
or fabricated stream progress. Native-property assignment still follows the
owned target's constraints. Network failures call the legacy onData notification
without replacing the previous variable values. Global loads support live
MovieClip targets or paths resolved relative to the calling timeline.

The decoder is authored Rust; the browser adapter marshals references and connects
the existing URLLoader/ByteArray transport. Primary contracts are the Adobe
[global functions](https://open-flash.github.io/mirrors/as2-language-reference/global_functions.html)
and [MovieClip](https://open-flash.github.io/mirrors/as2-language-reference/MovieClip.html)
references. No third-party engine or implementation dependency was introduced.

[as2-variables-batch.nflash](../examples/as2-variables-batch.nflash) combines bundled-resource loading with an explicit HTTP POST button. The optional same-origin `/__as2_vars_echo` endpoint is an example backend contract, not a service included in the exported runtime.
