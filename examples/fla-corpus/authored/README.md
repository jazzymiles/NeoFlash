# Original NeoFlash import examples

These small projects are authored for NeoFlash under the repository's
MIT-or-Apache-2.0 license. They contain no downloaded artwork. Their `.fla` files
are **synthetic ZIP/XFL packages**, not projects saved by Adobe Flash and not
historical binary FLA files. They exercise the current supported import subset;
they do not establish general FLA or AS2/AS3 compatibility.

Each folder contains the editable XFL source in `xfl/`, a deterministic `.fla`
package of that source and the unmodified `.nflash` result from the importer.
Open the `.nflash` in NeoFlash or import the XFL/FLA again. The importer currently
names imported documents `Imported XFL`; the scene name identifies each example.

| Example | Coverage | Expected behavior |
| --- | --- | --- |
| [Primitives and holds](primitives-and-holds/primitives-and-holds.nflash) | Rectangle, oval, fill alpha, static text, held/blank keyframes, labels | Two held positions, blank frames 48–59, return at frame 60. |
| [Nested playback AS2](nested-playback-as2/nested-playback-as2.nflash) | Three symbols, nested instances, independent/loop/play-once/single-frame playback, placement alpha, AS2 frame scripts | Four playback modes share the Orb symbol; root and nested timelines trace their start. |
| [Frame scripts AS3](frame-scripts-as3/frame-scripts-as3.nflash) | Typed local variables, arithmetic/string expressions, labels, AS3 frame scripts | Three poses and traces; `stop()` holds the final pose at frame 24. |

Frame indices above are zero-based. Editor scrubbing must not evaluate authored
scripts. Use **Test Movie** to exercise scripts in HTML5. The AS2/AS3 examples
exercise only the specific constructs listed here. System font metrics may vary.

Regenerate from the repository root:

```sh
cargo build -p neoflash-import
python3 scripts/generate-fla-examples.py
```

The generator checks both directory and archive inspection, then imports each
archive and checks symbol/frame/script counts and language. It writes inspection
reports to ignored `target/fla-corpus-research/authored/`; these reports are not
source fixtures. `metadata.json` records expected warning codes, manual checks
and SHA-256 digests for all source packages and imported projects. ZIP entries
use a fixed timestamp and stored compression for byte-for-byte reproducibility.
The generator deliberately overwrites only its own named example files.

Export an example after building the web runtime:

```sh
cargo run -p neoflash-export -- \
  examples/fla-corpus/authored/frame-scripts-as3/frame-scripts-as3.nflash \
  --output dist/fla-corpus/frame-scripts-as3
```

Successful import means the source model was accepted. Browser behavior and
visual fidelity require separate checks; see the corpus verification report for
the checks actually performed.
