#!/usr/bin/env bash
set -euo pipefail
project_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$project_root"
export PATH="$HOME/.cargo/bin:$PATH"
# Full delivery checks do not need to retain incremental compilation sessions.
# Developers can still opt in; ordinary direct cargo commands are unchanged.
export CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}"
python3 scripts/design-lock.py
python3 scripts/editor-api-index.py --check
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
./scripts/build-web.sh
cargo test --locked --workspace
cargo test --locked -p egui-winit --lib neoflash_paste_tests
python3 scripts/check-fla-corpus.py
node --test web/tests/*.test.cjs scripts/check-fla-examples.cjs scripts/tests/*.test.cjs
node crates/neoflash-script/tests/execute.mjs
node crates/neoflash-script/tests/project-execute.mjs
python3 -B -m unittest discover -s scripts/tests -p 'test_browser_batch.py'
cargo run --locked -p neoflash-export -- --demo --output target/artifacts/demo
