#!/usr/bin/env bash
set -euo pipefail
project_root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$project_root"
export PATH="$HOME/.cargo/bin:$PATH"

# Cargo cannot select features on a patched dependency outside the workspace.
# Keep this feature-isolation harness and its pruned lock in ignored build output.
probe=target/verification/eframe-features
mkdir -p "$probe/src"
cat > "$probe/Cargo.toml" <<'TOML'
[package]
name = "neoflash-eframe-feature-probe"
version = "0.0.0"
edition = "2024"
[workspace]
[features]
accesskit = ["eframe/accesskit"]
[dependencies]
eframe = { path = "../../../vendor/eframe", default-features = false, features = ["wgpu"] }
[patch.crates-io]
egui-winit = { path = "../../../vendor/egui-winit" }
TOML
cat > "$probe/src/main.rs" <<'RUST'
fn main() { let _options = eframe::NativeOptions::default(); }
RUST
cp Cargo.lock "$probe/Cargo.lock"
cargo check --offline --manifest-path "$probe/Cargo.toml" --target-dir target --features accesskit
cargo check --offline --locked --manifest-path "$probe/Cargo.toml" --target-dir target
