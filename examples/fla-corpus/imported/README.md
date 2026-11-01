# Imported public projects

Every binary FLA input of the corpus has a pinned direct import here, named after
its manifest case: `ruffle-mx-shape-tween`, `ruffle-avm1-nested-button-text`,
`flacomdoc-flash5-shapes`, `flacomdoc-mx-layers`, `flacomdoc-flash8-scripts` and
`flacomdoc-cs4-motion-tween` join the two MX fixtures below. Each embeds its
unchanged original; the source collection's license notices
([Ruffle](../upstream/ruffle/LICENSE.md), [flacomdoc](../upstream/flacomdoc/LICENSE.md))
apply to the derived project. The manifest records the warnings each import must
report, for example approximated dynamic text or motion-tween XML kept as poses.

[ruffle-mx-button.nflash](ruffle-mx-button.nflash) and
[ruffle-mx-static-text.nflash](ruffle-mx-static-text.nflash) are direct imports of
the unchanged [MX button](../upstream/ruffle/mx-button/DefineButton-MX.fla) and
[MX static-text](../upstream/ruffle/mx-static-text/DefineText2-MX.fla) sources.
Their embedded original bytes and native snapshots are pinned in the corpus
manifest. Ruffle's [MIT OR Apache-2.0 license notices](../upstream/ruffle/LICENSE.md)
also apply to these derived projects. Both are 550 × 400 at 12 fps and contain
no scripts. Rust regressions check button states/Hit picking and archived text
style; static font metrics and padding can differ from Flash.

[ruffle-avm1-frame-labels.nflash](ruffle-avm1-frame-labels.nflash) is the direct,
unmodified result of importing the real Adobe Flash CS6
[source FLA](../upstream/ruffle/avm1-frame-labels/test.fla). It is not a synthetic
replacement. The source and its companion SWF/expected trace come from Ruffle's
[`avm1/goto_label` regression](https://github.com/ruffle-rs/ruffle/tree/fe6cfc545405b888ebb478ce1c211d63147ca49b/tests/tests/swfs/avm1/goto_label).
Ruffle's [MIT OR Apache-2.0 license notices](../upstream/ruffle/LICENSE.md) apply
to this derived project too.

The movie is 550 × 400 at 24 fps, with a seven-frame root timeline and a two-frame
child clip. It contains a red square with a 1 px black stroke, two symbols and
nine original AS2 frame scripts. The square occupies 42.5 × 42.5 pixels at the
top left of the stage. Its size is deliberately unchanged.

The scripts test an absent label, ASCII-only case-insensitive lookup, duplicate
labels, a function installed on the child by the root, and capture of the root
timeline inside that function. Expected output is retained verbatim in
[output.txt](../upstream/ruffle/avm1-frame-labels/output.txt). After the initial
sequence, the root should remain on UI frame **3 / 7**. The child subsequently
loops back to its own first frame and stops.

Reproduce from the repository root:

```sh
cargo run --locked -p neoflash-import -- import \
  examples/fla-corpus/upstream/ruffle/avm1-frame-labels/test.fla \
  target/ruffle-frame-labels.nflash
./scripts/build-web.sh
cargo run --locked -p neoflash-export -- \
  examples/fla-corpus/imported/ruffle-avm1-frame-labels.nflash \
  --output dist/fla-corpus/ruffle-avm1-frame-labels
node --test web/tests/real-fla-frame-labels.test.cjs
```

Use a fresh import destination: the importer refuses to overwrite it. Open the
native file in NeoFlash, or serve the generated HTML5 directory and open its
`index.html`. See the corpus [verification record](../VERIFICATION.md) for the
checks actually completed. Success for this document does not imply general
binary FLA, DOMShape, external class or AS2 compatibility.
