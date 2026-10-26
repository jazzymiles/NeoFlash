# flacomdoc source fixtures

Four unmodified binary FLA documents from
[jindrapetrik/flacomdoc](https://github.com/jindrapetrik/flacomdoc/tree/0923d70870d11b236755af71337da8d0110a57aa/testdata/fla),
pinned to commit `0923d70870d11b236755af71337da8d0110a57aa` on 2026-10-27.
The upstream [README](UPSTREAM-README.md) identifies Jindra Petřík (JPEXS) as
author and licenses the work under [LGPL v2.1](LICENSE.md). That upstream license
applies to these fixtures; they are not relicensed as NeoFlash source code.
No converter implementation or application icons are included.

| File | Purpose | Observed format |
| --- | --- | --- |
| [0002_shapes.fla](flash5-shapes/0002_shapes.fla) | Small vector geometry document | Flash 5 binary FLA |
| [0008_nested_layers.fla](mx-layers/0008_nested_layers.fla) | Layer hierarchy and object references | Flash MX binary FLA |
| [0018_scripts_as2.fla](flash8-scripts/0018_scripts_as2.fla) | Legacy AS2 source and object/media boundaries | Flash 8 binary FLA |
| [0022_motiontween-cs4.fla](cs4-motion-tween/0022_motiontween-cs4.fla) | Later record revisions and motion tween | Flash CS4 binary FLA |

NeoFlash currently recognizes all four and refuses editable conversion with
`legacy_fla_conversion_unsupported` plus source-located structural diagnostics.
The original files have not been rewritten or executed. Filename/version labels
describe upstream fixtures and detected container targets, not a claim that
NeoFlash can reproduce their behavior. `metadata.json` retains exact source URLs,
SHA-256 hashes and sizes, including the upstream license and README.
