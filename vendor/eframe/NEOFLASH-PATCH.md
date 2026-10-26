# NeoFlash eframe patch

This is eframe **0.33.3**, published from upstream commit
[`44cdd653e2317d300fb8a6c9c36b03f23991e803`](https://github.com/emilk/egui/tree/44cdd653e2317d300fb8a6c9c36b03f23991e803/crates/eframe).
The original crates.io archive SHA-256 is
`457481173e6db5ca9fa2be93a58df8f4c7be639587aeb4853b526c6cf87db4e6`
(verified against the cached archive and prior Cargo.lock registry entry).
Source files, the default icon, README and normalized Cargo.toml come from that
archive. LICENSE-MIT and LICENSE-APACHE are retained from the same upstream commit;
upstream offers the crate under either license. NeoFlash modifications are dated
2026-10-27.

The source patch is limited to `src/native/wgpu_integration.rs`. The root wgpu
window already initializes its egui-winit AccessKit adapter, but dynamically
created child windows do not. Their widgets render and handle ordinary input,
yet native accessibility sees only the OS window controls. Carry a clone of the
existing event-loop proxy in `SharedState`, behind the `accesskit` feature, and
initialize the adapter immediately after creating each child window's
`egui_winit::State`. AccessKit requires initialization before the first show:
create the window from a temporary builder with `visible(false)`, install the
adapter, store the State/window, then restore the requested visibility (default
visible). The persistent builder keeps its original visibility, including an
explicit `false`, so later viewport patches retain their normal semantics.
This covers immediate and deferred viewports, and the same
window recreation path. An existing window returns before initialization, so
repainting it does not recreate its adapter.

The existing upstream event routing already selects the target viewport using
the AccessKit event's window ID. The existing per-window input processing and
platform-output handling deliver actions and tree updates to that window's
State. Those paths, root initialization, rendering, clipboard behavior and the
public eframe API remain unchanged. No unsafe code, native hook, extra thread or
global accessibility state is added. The glow backend is unchanged; NeoFlash
uses wgpu.

Cargo.toml only changes the local license/include paths and records this note.
Dependency versions and features are unchanged. The workspace patch selects this
crate alongside the separately documented egui-winit paste patch. Cargo.lock
remains the sole dependency lock; duplicate upstream lock/package metadata are
omitted.

Verification from the repository root:

```sh
cargo check --locked -p neoflash-editor
bash vendor/eframe/check-features.sh
cargo test --locked -p neoflash-editor detached_tests
```

The feature check builds a minimal wgpu consumer with and without `accesskit`.
Its source and temporary lock stay under ignored `target/verification`, with
dependency versions seeded from the root lock and resolved offline. A separate
consumer is necessary because Cargo rejects feature selection on a patched
dependency excluded from the workspace.

These compilation and headless editor checks do not prove native accessibility
delivery. Acceptance also requires a rebuilt macOS app: open Tools, Timeline and
Actions as native windows; inspect their accessible widget names; activate a
child control through accessibility; close and reopen a child; and verify root
controls still work. Record actual results in the project's verification notes.

The first native attempt exposed a missing lifecycle precondition: initializing
the adapter on a default-visible child caused the AccessKit constructor to panic.
A captured first-panic backtrace identified that exact check in
`accesskit_winit::Adapter::with_direct_handlers`; creating the child hidden before
initialization fixes the cause without suppressing the panic or disabling AX.

Known upstream-integration boundary: on macOS, showing a newly created child via
`set_visible(true)` also activates it, even if a generic caller supplied
`active(false)`. NeoFlash does not request inactive child windows. Preserve that
case before using this fork as a general eframe replacement.

Remove this fork once an upstream release initializes accessibility for every
wgpu viewport, retaining the native child-window accessibility acceptance check.
