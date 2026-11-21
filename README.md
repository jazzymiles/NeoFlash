# NeoFlash

Open-source vector animation, interactive media and application authoring studio inspired by classic Macromedia/Adobe Flash before Animate, built in Rust. NeoFlash gives illustrators, animators, web designers and game makers one responsive workspace with HTML5 + WebAssembly as its primary publishing target—requiring no Flash Player, browser plug-in or external CDN.

**Status: `0.1.0-beta.1` for macOS 15+ (Apple Silicon), with Windows x64 and Linux x64 Technical Previews.** Authoring tools, nested timelines, tweens, motion guides, scenes, synchronized audio/video, multi-language scripting (ActionScript 1/2/3, JavaScript and TypeScript), portable HTML5 publishing, and legacy FLA (Flash 3–CS6), XFL, SWF and SWC import work within the documented boundaries below. Open items and platform limits are tracked in the [compatibility matrix](docs/COMPATIBILITY.md) and [roadmap](docs/ROADMAP.md).

## Key capabilities

- **Vector drawing & illustration:** Object Drawing and planar Merge Drawing modes (**J**), Pen (**P**), Subselection (**A**), Pencil (**Y**), calligraphic Brush (**B**) with presets and tablet pressure, Free Transform (**Q**), Gradient Transform (**F**), Paint Bucket gap closing, Eraser stroke/fill modes, boolean Combine Objects operations, **Trace Bitmap** vectorization, live filters and blend modes.
- **Timelines, symbols & sound/video:** Nested MovieClip, Button (Up/Over/Down/Hit) and Graphic symbols (**F8**), in-place affine symbol editing with dimmed parent context, Classic, Motion (editable Bézier paths + property ◆ keyframes) and Shape tweens (with shape hints), motion guides, multiple scenes, vector/alpha/luminance mask layers, and decoded stereo timeline sound waveforms with envelopes and video trim sync.
- **Scripting & debugging:** Frame scripts, placed instance handlers and project modules in **ActionScript 1/2**, **ActionScript 3**, **JavaScript (ES2023+)** and **TypeScript** (with built-in static checking, Source Map v3 and connected Chromium breakpoint debugging).
- **Publishing & interchange:** Self-contained HTML5/Canvas/DOM bundles (`Full`, `Balanced`, `Compact`, `Shared`), texture atlases, raster/vector sequences, derived Adobe Animate `.fla` / XFL archives, standalone `.swf` movies, editable After Effects compositions, and opening legacy binary `.fla` (Flash 3–CS4), XFL/CS5+ `.fla`, `.swf`, `.swc`, `.fig`/`.figma`, `.svg`, PDF-compatible `.ai`, `.pdf`, EPS previews, Photoshop layers or composites, audio and video assets.

<!-- progress:start -->
## Current coverage

### Product functions

| Area | Functions working | Partial | Planned | Capability groups (closed / partial / planned) |
| --- | ---: | ---: | ---: | ---: |
| Editor and graphics | 170 of 170 | 0 | 0 | 12 / 0 / 0 |
| Timelines and animation | 84 of 84 | 0 | 0 | 10 / 0 / 0 |
| HTML5 and interactivity | 109 of 110 | 1 | 0 | 11 / 1 / 0 |
| Import and file compatibility | 63 of 68 | 5 | 0 | 5 / 5 / 0 |
| Platform and quality | 62 of 68 | 4 | 2 | 7 / 3 / 0 |
| **All areas** | 488 of 500 | 10 | 2 | 45 / 9 / 0 |

Each capability group lists its user-visible functions; a group is closed only when all of them work. Counts are functions, not effort or overall product completion.

### ActionScript: Flash Player target

| Language | Now (areas 1–6 working) | All Flash Player areas working | Partial / missing | Outside target (never in %) |
| --- | ---: | ---: | ---: | --- |
| ActionScript 3 — Flash Player | 1,928 of 1,939 (99.4%) | 3,756 of 4,164 (90.2%) | 408 / 0 | AIR-only APIs 1,448 · Flex, components and libraries 37,990 · Platform not resolved 138 |
| ActionScript 2 — Flash Player | 925 of 925 (100.0%) | 1,222 of 1,238 (98.7%) | 16 / 0 | AS2 UI components 1,044 · Other libraries and extensions 25 |

Test-backed declaration statuses only (not an overall Flash compatibility score); last status review: 2026-11-21. Full eight-area breakdown and per-declaration provenance: [ActionScript declaration audit](docs/progress/ACTIONSCRIPT.md).

[Current coverage](https://jazzymiles.github.io/NeoFlash/progress.html) · [Detailed registry](docs/progress/DETAILS.md) · [ActionScript details](docs/progress/ACTIONSCRIPT.md) · [Counting rules](docs/progress/README.md)

Generated from `docs/progress/`; GitHub Pages rebuilds on every push to `main`.
<!-- progress:end -->

## Install, Update & Run (`0.1.0-beta.1`)

Install [Rust with rustup](https://rust-lang.org/tools/install/) and Xcode Command Line Tools on macOS 15+ (Apple Silicon). The repository pins its Rust toolchain.

```sh
./scripts/run.sh                    # build the browser runtime and open the editor
./scripts/package-macos.sh          # local Apple Silicon dist/NeoFlash.app only
open dist/NeoFlash.app
```

Ordinary local packaging intentionally creates only the self-contained, ad-hoc-signed Apple Silicon app and removes
stale macOS release archives for the same version. A complete macOS Beta release set is an explicit release operation:
`./scripts/package-macos.sh release --beta-release` adds the ZIP, DMG, corresponding-source archive and checksums.
Use that mode only when preparing an authorized Beta release.

Ready-made Technical Preview archives use these names in `dist/`:

- Windows x64: `NeoFlash-0.1.0-beta.1-windows-x64-preview.zip`; extract it and run `neoflash-editor.exe`.
- Linux x64: `NeoFlash-0.1.0-beta.1-linux-x64-preview.tar.gz`; extract it and run `./neoflash-editor` on Ubuntu 24.04-compatible systems.
- Verify each platform download and its media corresponding-source archive against its matching `SHA256SUMS.txt` (`NeoFlash-0.1.0-beta.1-SHA256SUMS.txt` covers the macOS `.zip`, `.dmg` and `NeoFlash-0.1.0-beta.1-media-corresponding-source.tar.xz`; `NeoFlash-0.1.0-beta.1-windows-x64-SHA256SUMS.txt` and `NeoFlash-0.1.0-beta.1-linux-x64-SHA256SUMS.txt` cover the portable previews and `NeoFlash-0.1.0-beta.1-portable-media-corresponding-source.tar.xz`). All application, Rust-dependency and bundled-media licence texts are already included; users do not need to find licences themselves.

- **Install & manual update:** Drag `NeoFlash.app` into `/Applications` (or run `dist/NeoFlash.app` directly). Local and unnotarized Beta 1 macOS builds are ad-hoc signed: if macOS Gatekeeper blocks first launch, open **System Settings ▸ Privacy & Security** and click **Open Anyway** (`scripts/notarize-macos-beta.sh` signs and notarizes release builds when Apple Developer ID credentials are configured). To update to a newer Beta build, quit NeoFlash and replace `NeoFlash.app` in `/Applications`; user preferences, custom workspace layouts, recent files, and crash-recovery snapshots in `~/Library/Application Support/NeoFlash` are preserved automatically.
- **Bundled media helpers:** Every packaged build includes `ffmpeg` and `ffprobe` helpers (`NeoFlash.app/Contents/Resources/media/bin/` on macOS and `media/bin/` in portable archives), so video import, audio/video transcoding, and MP4 export require no separate media-tool installation. Windows/Linux helpers include their codec dependencies but still use normal operating-system runtime libraries such as glibc on Linux. Exact Rust and media notices are inside each package; the explicit full-release gate also emits a checksummed corresponding-source archive beside it. Advanced users can override the bundled binaries by setting `NEOFLASH_FFMPEG` (and `NEOFLASH_FFPROBE`).
- **Project backups & bug reporting:** `.nflash` projects are human-readable versioned JSON; keep projects in Git or maintain regular file backups before major migrations. In beta builds, click the bottom-left Stage **`Bug?`** pill or choose **Help ▸ Send Beta Feedback…** to submit a note and up to five screenshots/images (`PNG`/`JPEG`/`GIF`/`WebP`) to `developer@redinger.cc` over HTTPS via the `formsubmit.co` relay (sending only the visible note and attached files—no hidden telemetry or document bytes). For complex issues, keep a minimal `.nflash` reproduction project ready, paste relevant **Output** diagnostics and the version/commit from **NeoFlash → About NeoFlash** into the note, and mention that the project can be shared separately if requested; the feedback form itself does not attach project files.
- **Beta 1 known issues & limitations:**
  - Native macOS VoiceOver accessibility tree (`platform-accessibility`) for the custom-painted NF2 editor UI is deferred to post-Beta; keyboard-only focus navigation (`Tab` / `Shift+Tab`, `keyboardFocusActive` focus rings) and published HTML5 `#stage-a11y-overlay` ARIA DOM (`role`, `aria-label`, `tabindex`) are supported now.
  - Physical graphics-tablet pressure, tilt-to-angle mapping, and pen-tip/eraser switching are **experimental** in `0.1.0-beta.1`: the deterministic event-level pipeline (pressure `0.05..=1.0`, tilt-to-nib angle, consecutive strokes, pen/eraser tool switching, single-step Undo, and `.nflash` round-trip) is verified, while physical Wacom and Apple Sidecar hardware certification remains an external hardware check (`T-235`).
  - macOS Intel (`x86_64`) builds and a built-in automatic updater are not part of `0.1.0-beta.1`. Ad-hoc signed macOS builds require **System Settings ▸ Privacy & Security ▸ Open Anyway** on first launch when downloaded outside a notarized Developer ID build; the unsigned Windows archive may trigger SmartScreen, and its private connected Test Movie debugger is not yet available (ordinary Test Movie works). The Linux archive is tested on Ubuntu 24.04 x64 with X11/Wayland and Vulkan/GLES, and is not an AppImage or Debian package. Interactive acceptance on a physical Windows 11 desktop and broader Linux distributions remains pending.
  - Legacy binary FLA, SWF, and ActionScript compatibility remains bounded by the documented [compatibility matrix](docs/COMPATIBILITY.md) and [legacy import rules](docs/IMPORTS.md).

The first source build downloads pinned Rust/WASM dependencies. Node.js 22+ is needed for repository verification checks, not for running `NeoFlash.app` or published HTML5 content. See [contributing](CONTRIBUTING.md) for verification commands.

## Examples

| Workflow | Source project |
| --- | --- |
| Nested animation and timelines | [Nested animation](examples/nested-orbit.nflash) |
| Vector paths and compound fills | [Vector drawing](examples/vector-paths.nflash) |
| Linked classes and input events | [Project program](examples/project-program.nflash), [AS2 input](examples/as2-project-input.nflash) |
| Text entry and saved values | [Text form](examples/text-form.nflash), [Saved game](examples/saved-game.nflash) |
| Images, filters and resource loading | [Loader gallery](examples/loader-gallery.nflash), [Filter gallery](examples/filter-gallery.nflash) |
| Self-contained 300×250 HTML5 ad | [Moon Tide](examples/moon-tide-300x250.nflash) · Publish settings → Profile → **Criteo · HTML5 ZIP** |
| Tweens, motion guides and scenes | [Timeline tweens](examples/timeline-tweens.nflash) |
| Timeline sound with Flash sync | [Timeline sound](examples/timeline-sound.nflash) |
| Audio and video | [Sound playback](examples/sound-playback.nflash), [Video playback](examples/video-playback.nflash) |
| Browser integration and dragging | [Web integration](examples/web-integration.nflash), [Drag and drop](examples/drag-drop.nflash) |

**File → Open…** opens binary FLA files saved by Flash 3 through CS4, XFL and ZIP-based FLA documents (CS5+/Animate), published SWF movies (with source reconstruction and a bounded AVM1/AVM2 interpreter) and SWC libraries. **File → Save as…** can create a preflighted modern Animate `.fla` as a derived copy without retargeting the open native document or clearing its unsaved state; the other versioned FLA/XFL profiles live under **File → Export**. **File → Export Flash Movie…** writes a bounded standalone `.swf` after script/depth checks and structural re-import verification. Supported scenes, nested symbols, layers, masks, motion guides, tweens, filters, blend modes, bitmap fills, text, sounds and scripts are preserved where the selected profile supports them; unsupported content is blocked or kept as an explicit approximation instead of being silently discarded. The SWF interpreter can project its bounded runtime display list onto the Stage and supports seek, step and restart without modifying the authored project; it is still not a complete Flash Player. Missing installed fonts use fallback and do not block import. See [legacy import support and limits](docs/IMPORTS.md) and [current export boundaries](docs/COMPATIBILITY.md).

The [public FLA corpus](examples/fla-corpus/README.md) includes licensed, pinned source files. All eight binary FLA sources (Flash 5, MX, 8 and CS4) and seven external XFL/FLA sources import through the native converter with 21 imports and 0 refusals, and an Adobe CS6 example reproduces its reference trace in HTML5. Unsupported cases retain explicit diagnostics; see [legacy import support and limits](docs/IMPORTS.md).

## Headless CLI & AI automation

`neoflash-editor --headless` (or `NEOFLASH_HEADLESS=1`, `--help`, `--version`) runs a display-free, machine-readable JSON CLI before initializing a native window while keeping macOS Finder `-psn_*` launches untouched:

```sh
neoflash-editor --headless capabilities
neoflash-editor --headless new --output app.nflash --name Demo --width 800 --height 600 --fps 24 --language as2
neoflash-editor --headless jsfl --project app.nflash --in-place --eval 'fl.getDocumentDOM().getTimeline().layers[0].frames[0].actionScript = "trace(\"hi\");";'
neoflash-editor --headless compile --project app.nflash
neoflash-editor --headless eval --project app.nflash --frames 0..=1 --scripts
neoflash-editor --headless export --project app.nflash --target html5 --size-level compact --out dist-web
```

`inspect`, `validate`, strict 18-operation JSON `batch` pipelines (`--stdin` / `--file`, including in-memory `import`), and capability-gated JSFL `FLfile` access (`--allow-read` / `--allow-write` / `--allow-delete`, default deny-all) return structured JSON on stdout (`{ "ok": true, ... }`) and errors on stderr with non-zero exit codes. Batch `save`, `export`, permitted `FLfile` mutations and the final `.nflash` output retain step-order read-your-writes behavior but share one filesystem rollback boundary; standalone `jsfl` applies the same rule to its permitted writes and project save. Returned errors, cooperative cancellation and response-write failure restore the pre-command state; process termination or power loss cannot run this in-memory rollback. See [workspace and headless CLI notes](docs/WORKSPACE.md#headless-cli-and-ai-automation).

## Project layout

```text
crates/     shared Rust model, editor, compiler, runtime, importer and exporter
web/        browser host and runtime tests
examples/   small authored projects and licensed import fixtures
scripts/    build, check, packaging and coverage tasks
docs/       product behavior, architecture, compatibility and roadmap
vendor/     pinned integration patches, provenance and licenses
```

## Documentation

- [Product requirements](docs/PRODUCT.md), [architecture](docs/ARCHITECTURE.md) and [decisions](docs/decisions/)
- [Compatibility](docs/COMPATIBILITY.md), [legacy imports](docs/IMPORTS.md) and [interactive HTML5](docs/WEB.md)
- [Roadmap](docs/ROADMAP.md), [known issues](docs/BACKLOG.md) and [verification scope](docs/VERIFICATION.md)
- [Changelog](CHANGELOG.md) and [contributing](CONTRIBUTING.md)

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option. Contributions use the same terms. Third-party code and fixtures retain their licenses and pinned provenance; see [third-party notices](THIRD_PARTY_NOTICES.txt). Packaged builds carry their exact dependency notices and media corresponding source, so end users do not need to assemble a licence bundle. The editor plays timeline sounds through unmodified MPL-2.0 Symphonia decoder crates; they are not part of published HTML5 content. This independent project does not include Adobe application code or branding assets.
