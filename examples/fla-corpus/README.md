# FLA import corpus

Small public source projects for developing and checking NeoFlash's importer.
The initial collection contains **15 unchanged FLA files from three upstream
projects**, plus **3 original synthetic ZIP/XFL examples and their editable
`.nflash` imports**. It is separate from the private, ignored
`examples/old flash files/` collection. No upstream scripts or reference SWFs
were executed as part of collection.

## Open a working example

These are original NeoFlash fixtures, not conversions of the downloaded FLA:

| Project | What to check |
| --- | --- |
| [Primitives and holds](authored/primitives-and-holds/primitives-and-holds.nflash) | Rectangle/oval, opacity, held and blank frames, labels |
| [Nested playback AS2](authored/nested-playback-as2/nested-playback-as2.nflash) | Nested symbols, four playback modes, frame scripts |
| [Frame scripts AS3](authored/frame-scripts-as3/frame-scripts-as3.nflash) | Typed variables, arithmetic, labels and final `stop()` |

Each has readable `xfl/` sources, a deterministic synthetic `.fla` archive and
an untouched native importer result. These `.fla` packages were not saved by
Adobe Flash. See [authored examples](authored/README.md) for expected frame
behavior and regeneration. Frame numbers there are zero-based; the player UI
displays one-based frame numbers.

## Downloaded sources

| Collection | FLA files | Coverage | Upstream license |
| --- | ---: | --- | --- |
| [Ruffle](upstream/ruffle/README.md) | 8 | Flash MX/8 binary files; ZIP/XFL; buttons, text, shape tween, frame labels, clip events, AS3 construction | MIT OR Apache-2.0 |
| [Flump](upstream/flump/README.md) | 3 | Bitmap symbols, layers, rotation, easing and animation timelines | MIT |
| [flacomdoc](upstream/flacomdoc/README.md) | 4 | Flash 5/MX/8/CS4 binary geometry, layer hierarchy, AS2 and motion tween | LGPL-2.1 |

The external inputs currently **all block editable conversion**. That is a
tested compatibility boundary, not a successful import. Inspect reports identify
unsupported structures, media, ordinary shape geometry, tweens and script
configuration. Original Ruffle `.as` files, reference SWFs, test configuration
and expected traces are retained where present; their expected output describes
Ruffle's tests, not NeoFlash execution compatibility.

The original Flump archives have a central-directory size inconsistency:
Python's ZIP reader rejects them while the current Rust reader inspects them.
Their bytes remain intact to preserve this real container compatibility case.

Every upstream payload, companion file and license has a commit-pinned download
URL, byte length and SHA-256 in [manifest.json](manifest.json). Collection
metadata retains the upstream paths and purpose. The manifest contains 69
verified files totaling 941,558 bytes, excluding local explanatory metadata.
Upstream files retain their own licenses; NeoFlash's license applies to its own
authored examples and checking scripts.

## Reproduce the checks

From the repository root:

```sh
python3 scripts/check-fla-corpus.py
```

The script builds the current importer with the pinned Cargo lockfile, verifies
all source hashes, inspects 21 entry points (15 external FLA plus three original
examples in both XFL and FLA form), and tests actual conversion/refusal. Positive
imports must equal the checked-in native project; blocked inputs must produce a
normal diagnostic refusal without writing a partial project. An existing output
must remain unchanged. Source hashes are rechecked afterwards. Detailed reports
go to ignored `target/fla-corpus/reports/`.

For an offline integrity-only check, use `--integrity-only`. To use an existing
build, pass `--importer target/debug/neoflash-import`; the report then describes
that binary, which may be older than the working tree.

Build the browser runtime with `./scripts/build-web.sh`, then check the authored
runtime examples with `node --test scripts/check-fla-examples.cjs`. Export a native
example with `cargo run -p neoflash-export -- PATH.nflash --output dist/NAME`.
Use **File → Open** for `.nflash` in the editor and **Test Movie** for scripts.
See [verification](VERIFICATION.md) for checks actually performed and their limits.

## Extend the collection

Add small, purposeful inputs with explicit license evidence and pinned source
URLs. Keep original bytes and necessary companions. Add expected inspection and
conversion behavior to the manifest; do not automatically replace expectations
when an importer changes. New support needs semantic tests before changing a
blocked case to a successful conversion. Generated reports and HTML5 builds
belong under `target/` and `dist/`, never alongside source examples.

Regenerating original examples intentionally changes their own payloads and
subtree metadata. If content changes, review and update the root manifest's
hashes, counts and native snapshots together. A reproducibility run with the
same generator/importer should leave all of them byte-identical.
