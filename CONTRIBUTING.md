# Contributing

Read [AGENTS.md](AGENTS.md), [architecture](docs/ARCHITECTURE.md) and [compatibility](docs/COMPATIBILITY.md) first. Keep changes small enough to explain and verify. Include a reproducible example for editor/runtime behavior changes and update relevant compatibility documentation.

Run `./scripts/check.sh` on macOS before proposing a change. This includes native Rust tests, lint, the WASM build and tests that execute the actual compiled WASM. UI changes also require an observed desktop check; renderer changes require an observed browser check.

Do not add external project assets, fonts, Flash binaries or third-party test corpora to the repository without permission. Add small original fixtures, and use the ignored `examples/old flash files/` directory for local corpus tests. Do not execute legacy projector EXEs during import tests.

Contributions are under the repository's MIT OR Apache-2.0 license. Report missing functionality honestly; a supported subset must not be presented as complete compatibility.
