# NeoFlash egui-winit patch

This is egui-winit **0.33.3**, published from upstream commit
[`44cdd653e2317d300fb8a6c9c36b03f23991e803`](https://github.com/emilk/egui/tree/44cdd653e2317d300fb8a6c9c36b03f23991e803/crates/egui-winit).
The original crates.io archive SHA-256 is
`ec6687e5bb551702f4ad10ac428bab12acf9d53047ebb1082d4a0ed8c6251a29`
(verified against the cached archive and prior Cargo.lock registry entry).
The source files, README and normalized published Cargo.toml come from that archive.
LICENSE-MIT and LICENSE-APACHE are retained from the same upstream commit; upstream
offers the crate under either license. NeoFlash modifications are dated 2026-10-27.

The patch is deliberately local to `src/lib.rs`: immediately before the existing
successful, nonempty `Event::Paste`, append its initiating `Event::Key` with the
same logical key, physical key and modifiers observed by `on_keyboard_input`.
Previously the paste branch returned without preserving that key. A later release
or the frame's final modifiers cannot distinguish Cmd+Shift+V from Cmd+V reliably.
The existing clipboard read, CRLF normalization, empty/error behavior, shortcut
recognition, key-repeat policy and all other event translation remain unchanged.
Four focused unit tests exercise this helper without accessing the OS clipboard.

NeoFlash's editor consumes the adjacent key as transport metadata before its
normal shortcut/TextEdit routing. Every payload is delivered once. A payload with
no adjacent initiating key means plain Paste; explicit menu Paste in Place keeps
its own mode. No new egui event variant, unsafe code, native hook, global monitor,
clipboard cache or delayed modifier state is introduced. Physical macOS delivery
still requires live desktop verification; these are adapter and headless tests.

Cargo.toml only changes the local license/include paths and records this note.
Dependency versions/features are unchanged. The workspace `[patch.crates-io]`
selects this single vendored crate; Cargo.lock remains the sole dependency lock.
Upstream package metadata and lock duplicates are omitted from the vendored tree.

Verification from the repository root:

```sh
cargo test --locked -p egui-winit --lib neoflash_paste_tests
cargo test --locked -p neoflash-editor clipboard_tests
```

Remove this fork once upstream preserves event-local paste intent and the editor
can consume that public API. Keep the modifier-release, multiple/repeated-paste,
empty-read and text-recipient regressions when upgrading. Do not replace the
captured press modifiers with final input state or a later key release.
