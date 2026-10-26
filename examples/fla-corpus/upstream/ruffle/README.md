# Ruffle source fixtures

Eight small, unmodified FLA projects from [ruffle-rs/ruffle](https://github.com/ruffle-rs/ruffle/tree/fe6cfc545405b888ebb478ce1c211d63147ca49b), pinned to commit `fe6cfc545405b888ebb478ce1c211d63147ca49b` and retrieved on 2026-10-27. The selection includes four legacy compound-document FLAs and four ZIP/XFL FLAs. Filenames, ActionScript companions, reference SWFs, and upstream test configuration/output are retained.

| Folder | What it exercises | Container |
| --- | --- | --- |
| `mx-button` | Flash MX button states and records | Binary FLA, Contents `0x29` |
| `mx-static-text` | Flash MX static text and fonts | Binary FLA, Contents `0x29` |
| `mx-shape-tween` | Flash MX shape tween and vector records | Binary FLA, Contents `0x29` |
| `avm1-frame-labels` | Frame labels, frame navigation and trace order | ZIP/XFL |
| `avm1-clip-events` | MovieClip event handlers and object scripts | ZIP/XFL |
| `avm1-nested-button-text` | Text fields nested inside button symbols | Binary FLA, Contents `0x3f` |
| `avm2-button-frame-scripts` | AS3 buttons, nested frame scripts and frame jumps | ZIP/XFL |
| `avm2-nested-construction` | AS3 instance construction, siblings and timeline navigation | ZIP/XFL |

Each case's `metadata.json` records the exact upstream source URL, SHA-256, byte length, entrypoint and coverage purpose. Downloads were also checked against the Git blob hashes returned by the pinned upstream tree. The top-level `metadata.json` identifies the shared licenses and upstream documentation. `LICENSE.md`, `SWF-LICENSE-MIT`, and `SWF-LICENSE-APACHE` are unmodified upstream copies; this collection uses the upstream `MIT OR Apache-2.0` terms. The two `UPSTREAM-*.md` files retain the licensing/contribution and regression-test context. Imported third-party `from_*` suites were excluded from this selection.

## Current NeoFlash result

The initial collection pass on 2026-10-27 recognized all eight sources but refused conversion. Subsequent importer/runtime work now converts **avm1-frame-labels** without changing its source: [native project](../../imported/ruffle-avm1-frame-labels.nflash), [verification](../../VERIFICATION.md). Its HTML5 output reproduces the exact 17-line upstream trace, including after Restart. The other seven Ruffle inputs remain explicit conversion refusals; this does not establish general FLA or AS2/AS3 support.

The binary files stop at unsupported Contents suffixes or MFC classes. The remaining ZIP/XFL files encounter unsupported elements, geometry or external script configuration; the AS3 cases additionally use button symbols and linked classes. Original files have not been simplified to hide those boundaries. Per-case observed diagnostics are informational, and should be rechecked when the importer changes.

For example, from the repository root:

```sh
cargo run -p neoflash-import -- inspect examples/fla-corpus/upstream/ruffle/mx-button/DefineButton-MX.fla
cargo run -p neoflash-import -- inspect examples/fla-corpus/upstream/ruffle/avm2-button-frame-scripts/test.fla
```

The reference SWFs and `output.txt` files describe upstream Ruffle tests. SWF bytecode has not been executed here. Only the imported **avm1-frame-labels** source has been compiled and run through NeoFlash HTML5 with an exact upstream trace comparison. The other references do not imply execution compatibility. AVM1 labels describe the legacy execution model; they do not establish that every fixture was authored in AS2 rather than AS1.
