# Contributing

Read [AGENTS.md](AGENTS.md), [architecture](docs/ARCHITECTURE.md) and [compatibility](docs/COMPATIBILITY.md) first. Keep changes small enough to explain and verify. Include a reproducible example for editor/runtime behavior changes and update relevant compatibility documentation.

Run `./scripts/check.sh` on macOS before proposing a change. This includes native Rust tests, lint, the WASM build and tests that execute the actual compiled WASM. UI changes also require an observed desktop check; renderer changes require an observed browser check.

Do not add external project assets, fonts, Flash binaries or third-party test corpora to the repository without permission. Add small original fixtures, and use the ignored `examples/old flash files/` directory for local corpus tests. Do not execute legacy projector EXEs during import tests.

Contributions are under the repository's MIT OR Apache-2.0 license. Report missing functionality honestly; a supported subset must not be presented as complete compatibility.

Keep public documentation focused on setup, behavior, architecture and known limits. Update the existing topic rather than adding delivery reports or repeated statistics. README introduces the product, the roadmap describes future work, and the changelog records user-visible changes. Internal work diaries, agent coordination, timings and raw command/browser logs belong in ignored `target/`. Preserve licenses, pinned provenance and concise verification boundaries; maintain coverage through the existing `docs/progress/` registry and generated summary.

Project workflow checks collect family failures together. After a concrete failure, use `NEOFLASH_WORKFLOWS=as2-project-input,drag-drop node crates/neoflash-script/tests/project-execute.mjs` to rerun affected families plus the common prelude; omit the variable for the full workflow check. A focused rerun is not a complete delivery check.
