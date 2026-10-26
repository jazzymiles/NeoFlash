# Flump FLA samples

Three small original Flash CS6 projects from [tconkling/flump](https://github.com/tconkling/flump/tree/4933c6c13c3e846cb1f7cf9f03986326daf8cce3/exporter/src/test/resources), pinned to commit `4933c6c13c3e846cb1f7cf9f03986326daf8cce3`. The upstream [MIT license](LICENSE) and [authors](AUTHORS) are retained. These third-party files keep their own license. Exact download URLs, sizes and SHA-256 hashes are in [metadata.json](metadata.json).

| Source project | Bytes | Coverage |
| --- | ---: | --- |
| `rotate_test2/rotate_test2.fla` | 11,583 | Nested bitmap symbols, clockwise rotation, full-turn tween and easing |
| `potion_simple/potion_simple.fla` | 52,327 | Potion animation, multiple layers, nested bitmap symbols and tweens |
| `flump/flump.fla` | 373,675 | 27 inspected symbols, 78 layers, 364 keyframes, skew and many tweens |

All three are ZIP/XFL FLA archives. The original bytes are preserved, including the internal bitmap payloads. The samples use ActionScript 3 publish configuration and symbol linkage; they contain no observed embedded `.as` source or document class. No SWF playback or Adobe software is needed to run NeoFlash inspection.

The original archives contain central-directory size inconsistencies: Python `zipfile` rejects them and macOS `unzip` warns. NeoFlash's current ZIP reader inspects all three. Do not silently rewrite or repack these originals; this characteristic is part of their real-world container coverage.

Current inspection returns `can_import: false` for every sample. Bitmap assets, tweens, linkage/publish settings and additional authoring attributes are outside the current import boundary. The larger `flump.fla` also reports skew and layer-type blockers. These are negative compatibility examples, not editable NeoFlash conversions. `import` was verified to refuse each file without creating an output.

```sh
cargo run -p neoflash-import -- inspect examples/fla-corpus/upstream/flump/rotate_test2/rotate_test2.fla
```

Observed diagnostic codes in metadata are a baseline, not a promise that importer coverage will remain unchanged. No source script or Flash movie was executed during inspection.
