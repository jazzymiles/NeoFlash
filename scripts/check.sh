#!/usr/bin/env bash
set -euo pipefail
project_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$project_root"
export PATH="$HOME/.cargo/bin:$PATH"
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo test --locked -p egui-winit --lib neoflash_paste_tests
python3 scripts/check-fla-corpus.py
./scripts/build-web.sh
node --test web/tests/*.test.cjs scripts/check-fla-examples.cjs
node crates/neoflash-script/tests/execute.mjs
node crates/neoflash-script/tests/project-execute.mjs
python3 -B -m unittest discover -s scripts/tests -p 'test_browser_batch.py'
cargo run --locked -p neoflash-export -- --demo --output dist/demo
