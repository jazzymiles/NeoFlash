#!/usr/bin/env bash
set -euo pipefail
project_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$project_root"
export PATH="$HOME/.cargo/bin:$PATH"
case "$#:${1:-}" in
  0:) export NEOFLASH_CHECK_TIER=standard ;;
  1:--release) export NEOFLASH_CHECK_TIER=release ;;
  *) printf 'Usage: %s [--release]\n' "$0" >&2; exit 2 ;;
esac
# Full delivery checks do not need to retain incremental compilation sessions.
# Developers can still opt in; ordinary direct cargo commands are unchanged.
export CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}"
python3 scripts/design-lock.py
python3 interface/design-system/tools/icon-masters/build.py --check
python3 scripts/editor-api-index.py --check
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
./scripts/build-web.sh
# Each check owns its publish fixtures. Chrome consumes the exact output the Rust
# suite verified, even when another check runs concurrently or an earlier one failed.
mkdir -p target/artifacts
publish_gate_dir="$(mktemp -d "$project_root/target/artifacts/publish-gate-$NEOFLASH_CHECK_TIER.XXXXXX")"
NEOFLASH_PUBLISH_GATE_DIR="$publish_gate_dir" cargo test --locked --workspace
cargo test --locked -p egui-winit --lib neoflash_paste_tests
python3 scripts/check-fla-corpus.py
cargo build --locked -p neoflash-export --bin neoflash-export
cargo build --locked -p neoflash-import --bin neoflash-import
cargo build --locked -p neoflash-sync --bin neoflash-sync
cargo build --locked -p neoflash-script --example compile_batch --example compile_project
node --test --test-concurrency=2 --test-timeout=180000 web/tests/*.test.cjs scripts/check-fla-examples.cjs scripts/tests/*.test.cjs
node crates/neoflash-script/tests/execute.mjs
node crates/neoflash-script/tests/project-execute.mjs
python3 -B -m unittest discover -s scripts/tests -p 'test_browser_batch.py'
cargo run --locked -p neoflash-export -- --demo --output target/artifacts/demo
node scripts/verify-as-publish-gate.mjs --input-dir="$publish_gate_dir"
