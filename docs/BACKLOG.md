# Known Issues & Open Follow-Ups

This page tracks concrete open limitations and follow-ups without duplicating generated registries:

- **Product functions (`working` / `partial` / `planned`):** [progress/DETAILS.md](progress/DETAILS.md) (`docs/progress/product.json`)
- **ActionScript per-declaration status & notes:** [progress/ACTIONSCRIPT.md](progress/ACTIONSCRIPT.md)
- **Historical verification findings:** [VERIFICATION.md › Open findings](VERIFICATION.md#open-findings-and-scoped-fixes)
- **Future work:** [ROADMAP.md](ROADMAP.md) · **Import approximations:** [IMPORTS.md](IMPORTS.md)

## `0.1.0-beta.1` Known Issues & Platform Limits

- **Supported platforms & separate macOS bundles:** `0.1.0-beta.1` ships Developer ID signed, Apple-notarized DMG and ZIP packages for **macOS 15+ (Apple Silicon, `arm64`)** and **macOS 12+ (Intel, `x86_64`)** as separate architecture-specific bundles (no fat universal binary), alongside **Windows x64** (`.zip`) and **Ubuntu 24.04 x64** (`.deb` and `.tar.gz`) Technical Previews.
- **Manual update installation:** `Preferences ▸ Updates` checks GitHub Releases (`Stable` / `Beta`) and downloads SHA-256-verified packages (`T-347` verified live against `v0.1.0-beta.1` and across sandboxed notarized build-605 upgrade and rollback), while applying an update uses manual replacement of `NeoFlash.app` in `/Applications` (or extracting the new archive / running `sudo apt install ./neoflash_*.deb`) rather than unattended background self-replacement. Preferences, workspaces, recent files, and crash-recovery snapshots in `~/Library/Application Support/NeoFlash`, `%APPDATA%\NeoFlash`, or `~/.local/share/NeoFlash` are preserved.
- **Windows Technical Preview limits:** The portable `windows-x64-preview.zip` (`neoflash-editor.exe`) is unsigned and may trigger a Windows SmartScreen prompt on first launch (`main` statically links the MSVC CRT, suppresses background console windows on GUI launch, and connects the private Win32 pipe Chromium `Test Movie` debugger). Bare-metal Windows 10 Pro x64 CLI/headless/media export and Windows 11 ARM64 VM DX12 GUI launch are verified; interactive physical Windows 11 x64 desktop sign-off remains open (`T-339`).
- **Linux Technical Preview limits:** Tested on bare-metal Ubuntu 24.04.5 LTS `amd64` (X11/Wayland, Vulkan/GLES, `.deb` and `.tar.gz`), Ubuntu 24.04 ARM64 + Rosetta VM, and CI (`libc6 >= 2.39`); `scripts/package-linux-appimage.sh` stages self-contained AppDirs/AppImages on `main`, while non-Ubuntu distribution testing remains open.
- **Bundled media helpers:** Every package includes self-contained `ffmpeg` and `ffprobe` (`Contents/Resources/media/bin/` on macOS, `media/bin/` in portable archives, `/usr/lib/neoflash/media/bin/` in `.deb` installs) plus exact Rust/media licence notices and side-by-side corresponding-source archives. Set `NEOFLASH_FFMPEG` / `NEOFLASH_FFPROBE` only to override the bundled binaries. Note: Intel macOS HEVC encoding uses hardware VideoToolbox, while Apple Silicon additionally bundles software `libx265` and `SVT-AV1`.
- **Accessibility & graphics tablets:** The macOS editor exposes `NSAccessibility` (`AccessKit`) trees and keyboard focus, and HTML5 exports expose `#stage-a11y-overlay` ARIA roles/labels/tab stops, but spoken macOS VoiceOver output has not been verified. Graphics-tablet pressure, tilt, and pen/eraser switching are **experimental** pending broader physical Wacom and Apple Sidecar hardware certification (`T-235`).
- **In-app Beta feedback:** The Stage `Bug?` chip and **Help ▸ Send Beta Feedback…** dialog are hidden in `0.1.0-beta.1` pending endpoint activation (`T-335`). Report issues on GitHub with a minimal `.nflash` reproduction, `Output` (`F2`) diagnostics, and the build/commit from **About NeoFlash**.
- **Bounded Flash / ActionScript compatibility:** Binary FLA (Flash 3–CS4), XFL/CS5+ FLA, SWF decompilation/preview/publish, and ActionScript 1/2/3 execution operate within the documented [compatibility matrix](COMPATIBILITY.md) and [import rules](IMPORTS.md).

## Standing Correctness & Runtime Follow-Ups

- **Built-in API contracts:** Known Flash properties must either implement their runtime contract or emit an explicit diagnostic; visibility, dimensions, text, and callbacks must never silently degrade into inert custom fields.
- **AS2 vs AS3 dialect isolation:** Keep separate dialect semantics (`parseInt` octal radix, `Number.toFixed` bounds, `_alpha`/`_xscale` percentage vs `0..1` unit floats, rotation normalization, `this` receiver binding on Button `on(release)` vs MovieClip `onClipEvent`).
- **Open runtime conformance areas:**
  - *Compiler & VM edge cases:* Rare AVM1/AVM2 prototype-mutation and reentrant frame-construction ordering edge cases where compiled JS differs from historical Flash Player bytecode VMs.
  - *Text, graphics & media:* Full OpenType complex-script shaping, exact Flash font-metric/raster parity, and live camera/microphone hardware capture verification.
  - *Networking & storage:* Native Adobe Media Server / RTMP protocol gateways (`SharedObject.getRemote` / `NetStream` currently use `neoflash-sync` WebSockets and HTTP(S) / loopback profiles).
