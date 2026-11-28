# NeoFlash

Open-source vector animation, interactive media, and application authoring studio in Rust, inspired by classic Macromedia/Adobe Flash before Animate. Primary target: self-contained HTML5 + WebAssembly (Canvas or semantic DOM) with no Flash Player, browser plug-in, or external CDN.

**Beta 1 (`0.1.0-beta.1`) supports macOS 15+ on Apple Silicon and macOS 12+ on Intel, with Windows x64 and Ubuntu 24.04 x64 Technical Previews.** Open items and format boundaries are tracked in the [compatibility matrix](docs/COMPATIBILITY.md) and [roadmap](docs/ROADMAP.md).

## Capabilities

| Area | Supported in `0.1.0-beta.1` |
| --- | --- |
| **Drawing & illustration** | Merge & Object Drawing (`J`), Pen (`P`), Subselection (`A`), Pencil (`Y`), calligraphic Brush (`B`) with custom presets & tablet pressure/tilt, Free Transform (`Q`), Gradient Transform (`F`), Paint Bucket gap closing, Eraser stroke/fill modes, boolean Combine Objects, **Trace Bitmap**, 7 live filter families & 14 blend modes |
| **Timelines, symbols & media** | Nested `MovieClip`, `Button` (`Up`/`Over`/`Down`/`Hit`) & `Graphic` symbols (`F8`), in-place affine editing with dimmed parent context, Classic, Motion (editable Bézier paths + `◆` property keys) & Shape tweens (with `a–z` shape hints), motion guides, multiple scenes, geometry/alpha/luminance masks, stereo sound waveforms/envelopes, linked video/audio Razor (`C`) trims |
| **Scripting & debugging** | Frame scripts, placed instance handlers, standalone `.as` Script Editor tabs & project modules in **ActionScript 1/2**, **ActionScript 3**, **JavaScript (ES2023+)** & **TypeScript** (built-in type checker, Source Map v3, connected Chromium breakpoint/Watch debugger, deterministic `Save As` `AS2 ↔ AS3` & `AS2/AS3 → JS` conversion) |
| **Publishing & export** | HTML5 + WASM (`Full`, `Balanced`, `Compact`, `Shared`; `Canvas` or `Web App · DOM`), zero-WASM single-file HTML5 ad banners (`Web Banner · Native`, `Criteo · HTML5 ZIP`), Flash Player 8/9/10 `.swf`, versioned `.fla` / XFL (Flash 8–Animate), Lottie / `.lottie`, 5-format Texture Atlases, After Effects `.aep`/`.jsx`, PNG/JPEG/WebP/GIF/SVG/AI/PDF/PSD, MP4/WebM/WAV/MP3, plus experimental Xcode/Swift 6 |
| **Import & clipboard** | Legacy binary `.fla` (Flash 3–CS4), XFL/CS5+ `.fla`, `.swf` (source reconstruction + bounded AVM1/AVM2 preview), `.swc`, Figma `.fig`/`.figma` + native `⌘C → ⌘V` Figma & bidirectional Illustrator PDF/AICB/SVG clipboard, layered `.psd`/`.psb`, `.ai`/`.pdf`/`.eps`/`.svg`/`.fxg`, palettes, fonts, audio & video |

<!-- progress:start -->
## Current coverage

### Product functions

| Area | Functions working | Partial | Planned | Capability groups (closed / partial / planned) |
| --- | ---: | ---: | ---: | ---: |
| Editor and graphics | 170 of 170 | 0 | 0 | 12 / 0 / 0 |
| Timelines and animation | 84 of 84 | 0 | 0 | 10 / 0 / 0 |
| HTML5 and interactivity | 111 of 111 | 0 | 0 | 12 / 0 / 0 |
| Import and file compatibility | 69 of 69 | 0 | 0 | 10 / 0 / 0 |
| Platform and quality | 68 of 69 | 0 | 1 | 9 / 1 / 0 |
| **All areas** | 502 of 503 | 0 | 1 | 53 / 1 / 0 |

Each capability group lists its user-visible functions; a group is closed only when all of them work. Counts are functions, not effort or overall product completion.

### ActionScript: Flash Player target

| Language | Now (areas 1–6 working) | All Flash Player areas working | Partial / missing | Outside target (never in %) |
| --- | ---: | ---: | ---: | --- |
| ActionScript 3 — Flash Player | 1,939 of 1,939 (100.0%) | 4,162 of 4,164 (<100%) | 2 / 0 | AIR-only APIs 1,448 · Flex, components and libraries 37,990 · Platform not resolved 138 |
| ActionScript 2 — Flash Player | 925 of 925 (100.0%) | 1,236 of 1,238 (99.8%) | 2 / 0 | AS2 UI components 1,044 · Other libraries and extensions 25 |

Test-backed declaration statuses only (not an overall Flash compatibility score); last status review: 2026-11-22. Full eight-area breakdown and per-declaration provenance: [ActionScript declaration audit](docs/progress/ACTIONSCRIPT.md).

[Current coverage](https://jazzymiles.github.io/NeoFlash/progress.html) · [Detailed registry](docs/progress/DETAILS.md) · [ActionScript details](docs/progress/ACTIONSCRIPT.md) · [Counting rules](docs/progress/README.md)

Generated from `docs/progress/`; GitHub Pages rebuilds on every push to `main`.
<!-- progress:end -->

## Download & Install (`0.1.0-beta.1`)

Download **[NeoFlash Beta 1 (build 605)](https://github.com/jazzymiles/NeoFlash/releases/tag/v0.1.0-beta.1)** and verify archives against `NeoFlash-0.1.0-beta.1-SHA256SUMS.txt` on the release page:

| Platform | Package | Install & update |
| --- | --- | --- |
| **macOS 15+ · Apple Silicon** (`arm64`) | `macos-arm64.dmg` / `.zip` (Developer ID signed & Apple notarized) | Drag `NeoFlash.app` to `/Applications`. Replacing `NeoFlash.app` preserves `~/Library/Application Support/NeoFlash`. |
| **macOS 12+ · Intel** (`x86_64`) | `macos-x86_64.dmg` / `.zip` (Developer ID signed & Apple notarized) | Drag the Intel `NeoFlash.app` to `/Applications`. Preserves `~/Library/Application Support/NeoFlash`. |
| **Windows 10/11 · x64** (Technical Preview) | `windows-x64-preview.zip` (unsigned portable) | Extract the folder and launch `neoflash-editor.exe` (SmartScreen may prompt on first run). User data stays in `%APPDATA%\NeoFlash`. |
| **Ubuntu 24.04 · x64** (Technical Preview) | `neoflash_*_amd64.deb` / `linux-x64-preview.tar.gz` | `sudo apt install ./neoflash_*.deb` (remove with `sudo apt remove neoflash`), or extract `.tar.gz` and run `neoflash-editor`. User data stays in `~/.local/share/NeoFlash`. |

- **Bundled media & licences:** Every package includes self-contained `ffmpeg` and `ffprobe` helpers (`Contents/Resources/media/bin/` on macOS, `media/bin/` in portable archives, `/usr/lib/neoflash/media/bin/` in `.deb` installs; override via `NEOFLASH_FFMPEG` / `NEOFLASH_FFPROBE`) plus exact Rust and media licence notices. Checksummed corresponding-source archives are published alongside the binaries on the release page.
- **Updates & bug reports:** `Preferences ▸ Updates` checks GitHub Releases and downloads SHA-256-verified archives for manual replacement (no silent self-update). Keep `.nflash` projects in Git or regular backups, and include a minimal reproduction, `Output` (`F2`) diagnostics, and the **About NeoFlash** build/commit when reporting bugs.

### Tested hardware & environments

| Platform | Tested machines | Verified scope |
| --- | --- | --- |
| **macOS · Apple Silicon** | Apple Silicon Mac (macOS 15 Sequoia, Metal) · GitHub Actions `macos-15` | Native GUI authoring, full Rust/WASM/browser check suite, Chrome/Firefox/Safari 27.2 HTML5 matrix, notarized DMG/ZIP Gatekeeper launch |
| **macOS · Intel** | **MacBook Pro 15" Retina Mid 2015** (`MacBookPro11,4`, Core i7-4770HQ, Intel Iris Pro `Metal GPUFamily macOS 1`, macOS 12.7.6 Monterey) · Rosetta 2 on macOS 15 | Native `x86_64` desktop GUI, Stage rendering, authoring, bundled `x86_64` `ffmpeg`/`ffprobe`, HTML5/SWF/MP4 export, notarized DMG/ZIP |
| **Linux · x64** | **Bare-metal Intel Retina laptop** (`MacBookPro11,4` `2880×1800` HiDPI, Ubuntu 24.04.5 LTS `amd64`, X11/Wayland, Vulkan/GLES) · Ubuntu 24.04 ARM64 + Rosetta VM · GitHub Actions `ubuntu-24.04` | `.deb` desktop install & portable `.tar.gz`, GUI authoring, HiDPI Chrome `Test Movie`, bundled static-codec `ffmpeg`/`ffprobe`, headless CLI |
| **Windows · x64 / ARM64** | **Bare-metal Windows 10 Pro x64** (`Miles-WIN-Test`) · **Windows 11 24H2 ARM64 VM** (UTM, DX12 native `arm64` & `x86_64` emulation) · GitHub Actions `windows-2022` (MSVC + DX12 WARP) | Portable launch, DX12 GUI rendering, headless CLI/JSFL authoring, bundled `ffmpeg`/`ffprobe` H.264 MP4, HTML5 & SWF export |

- **Known Beta 1 limits:** Spoken macOS VoiceOver output is unverified; graphics-tablet pressure, tilt, and eraser switching are experimental; Windows builds are unsigned and await interactive physical Windows 11 desktop sign-off (subsequent `main` builds link static MSVC CRT, suppress background GUI consoles, and connect the private Win32 pipe Chromium `Test Movie` debugger); Linux packages target Ubuntu 24.04 x64 (`glibc 2.39+`, with self-contained AppImage staging on `main`); legacy FLA, SWF, and ActionScript remain bounded by [docs/COMPATIBILITY.md](docs/COMPATIBILITY.md) and [docs/IMPORTS.md](docs/IMPORTS.md).

### Build from source

Requires [Rust (`rustup`)](https://rust-lang.org/tools/install/) and Xcode Command Line Tools on macOS 15+ (Apple Silicon); Node.js 22+ is used only by repository test suites.

```sh
./scripts/run.sh                    # build WASM runtime and launch editor
./scripts/package-macos.sh          # build local Apple Silicon dist/NeoFlash.app
open dist/NeoFlash.app
```

Ordinary `./scripts/package-macos.sh` builds only the ad-hoc-signed `arm64` `dist/NeoFlash.app` (use `./scripts/verify-macos-intel.sh` for standalone `x86_64` macOS 12.0+ staging; `./scripts/package-macos.sh release --beta-release` is reserved for full release archives).

## Examples

| Category | Projects |
| --- | --- |
| **Playable AS2 / AS3 / TS games ([`examples/games/`](examples/games/))** | [Doom: E1M1 Hangar (AS2)](examples/games/doom-as2.nflash), [Fallout: Vault 13 Cavern (AS2)](examples/games/fallout-as2.nflash), [X-COM: Sector 7 (AS3)](examples/games/xcom-as3.nflash), [Micropolis 2000 (TS/DOM)](examples/games/simcity-ts.nflash), [Tetris AS2](examples/games/tetris-as2.nflash) / [AS3](examples/games/tetris-as3.nflash), [Arkanoid AS2](examples/games/arkanoid-as2.nflash) / [AS3](examples/games/arkanoid-as3.nflash), [Snake AS2](examples/games/snake-as2.nflash) / [AS3](examples/games/snake-as3.nflash), [Saved game](examples/games/saved-game.nflash) |
| **Video & audio ([`examples/video-and-audio/`](examples/video-and-audio/))** | [Video playback](examples/video-and-audio/video-playback.nflash), [Timeline sound](examples/video-and-audio/timeline-sound.nflash), [Sound playback](examples/video-and-audio/sound-playback.nflash) |
| **Animation & cartoons ([`examples/animation/`](examples/animation/))** | [Midnight Express Ep. 1–6](examples/README.md#animated-series--midnight-express), [Timeline tweens](examples/animation/timeline-tweens.nflash), [Nested orbit](examples/animation/nested-orbit.nflash), [Vector paths](examples/animation/vector-paths.nflash), [Clipped scene](examples/animation/clipped-scene.nflash) |
| **ActionScript 2 & 3 suites ([`examples/as2/`](examples/as2/), [`examples/as3/`](examples/as3/))** | [AS2 00–08 & API batches](examples/README.md#actionscript-2-examplesas2), [AS2 input](examples/as2/as2-project-input.nflash), [AS3 00–08 & runtime suites](examples/README.md#actionscript-3-examplesas3), [Project program](examples/as3/project-program.nflash) |
| **Graphics, UI, text & web ([`graphics-and-filters/`](examples/graphics-and-filters/), [`ui-and-text/`](examples/ui-and-text/), [`web-and-ads/`](examples/web-and-ads/))** | [Loader gallery](examples/graphics-and-filters/loader-gallery.nflash), [Filter gallery](examples/graphics-and-filters/filter-gallery.nflash), [Text form](examples/ui-and-text/text-form.nflash), [Drag and drop](examples/ui-and-text/drag-drop.nflash), [Web integration](examples/web-and-ads/web-integration.nflash), [Moon Tide 300×250 (`< 8 KB` Criteo HTML5 ZIP)](examples/web-and-ads/moon-tide-300x250.nflash) |
| **Public FLA / XFL / SWF corpus** | [examples/fla-corpus](examples/fla-corpus/README.md) (`21/21` licensed Flash 5–CS6 & XFL imports, `0` refusals, [visual & SWF trace oracles](examples/fla-corpus/VERIFICATION.md)) |

## Headless CLI & Automation

`neoflash-editor --headless` (or `NEOFLASH_HEADLESS=1`) runs display-free JSON/text commands for CI, scripts, and AI agents without opening a GUI window:

```sh
neoflash-editor --headless capabilities
neoflash-editor --headless new --output app.nflash --name Demo --width 800 --height 600 --fps 24 --language as2
neoflash-editor --headless jsfl --project app.nflash --in-place --eval 'fl.getDocumentDOM().getTimeline().layers[0].frames[0].actionScript = "trace(\"hi\");";'
neoflash-editor --headless compile --project app.nflash --strict
neoflash-editor --headless eval --project app.nflash --frames 0..=1 --scripts
neoflash-editor --headless export --project app.nflash --target html5 --size-level compact --out dist-web
```

Supports `inspect`, `validate`, `compile`, `eval` (including `--vm swf`), deterministic `convert` (`--to as2|as3|js`), capability-gated `jsfl` (`FLfile` denied unless `--allow-read`/`--allow-write`/`--allow-delete` are set), `import`, `export`, and transactional 19-operation JSON `batch` pipelines (`--stdin` / `--file`). Details: [docs/WORKSPACE.md](docs/WORKSPACE.md#headless-cli-and-ai-automation) and [docs/JSFL-COMPATIBILITY.md](docs/JSFL-COMPATIBILITY.md).

## Project Layout

```text
crates/     shared Rust core, editor, script compiler, WASM bridge, importer and exporter
web/        HTML5 browser host, progress dashboard and runtime tests
examples/   authored .nflash projects, games and licensed FLA/XFL/SWF corpus
scripts/    build, verification, packaging, notarization and progress tooling
docs/       architecture, compatibility, import boundaries, verification and ADRs
vendor/     pinned egui/eframe patches, provenance and licenses
```

## Documentation

- [Product scope](docs/PRODUCT.md), [architecture](docs/ARCHITECTURE.md), [workspace & CLI](docs/WORKSPACE.md) and [decisions](docs/decisions/)
- [Compatibility](docs/COMPATIBILITY.md), [legacy imports](docs/IMPORTS.md), [JSFL matrix](docs/JSFL-COMPATIBILITY.md), [clipboard](docs/CLIPBOARD.md) and [interactive HTML5](docs/WEB.md)
- [Roadmap](docs/ROADMAP.md), [known issues](docs/BACKLOG.md) and [verification record](docs/VERIFICATION.md)
- [Changelog](CHANGELOG.md) and [contributing](CONTRIBUTING.md)

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option. Third-party components retain their licenses and pinned provenance in [THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt); packaged builds carry complete Rust and media licence evidence alongside corresponding-source archives. Independent project; no Adobe application code or branding assets are included.

## Thanks

I want to thank the entire community of creative people who once made the web — and the internet as a whole — such an interesting and interactive place. Your creativity played a huge role in my life — thank you!!!

Special thanks to:
ndglook, Смольный (Smolniy), Iv, juggy, Audax, Luke, iNils, etc, Gossip, ish, john, Xoxol, baxtep, УильямБрэдберри, C2Plus, лис, M@trix, Ales, Дениска, McMamus, silin, and everyone else!
