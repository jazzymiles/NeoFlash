# Product Scope & Requirements

NeoFlash is an open-source authoring studio in Rust inspired by classic Macromedia/Adobe Flash before Animate, designed for interactive websites, web applications, 2D games, and vector animation. Its primary publishing target is self-contained **HTML5 + WebAssembly** (`Canvas` and semantic `Web App · DOM`), requiring no Flash Player, browser plug-in, or external CDN.

## Supported Platforms (`0.1.0-beta.1`)

- **macOS 15+ (Apple Silicon, `arm64`)** — Developer ID signed and Apple-notarized `.dmg` and `.zip`.
- **macOS 12+ (Intel, `x86_64`)** — Developer ID signed and Apple-notarized `.dmg` and `.zip`.
- **Windows 10/11 x64** — Portable `.zip` Technical Preview (`neoflash-editor.exe`).
- **Ubuntu 24.04+ x64 (`amd64`)** — Installable `.deb` package and portable `.tar.gz` Technical Preview.

Every platform archive bundles self-contained `ffmpeg` and `ffprobe` media helpers, offline HTML5 publishing assets, and exact Rust/media licence notices, accompanied by checksummed corresponding-source archives.

## Core Capabilities

- **Dense classic workspace:** Stage, Tools, Timeline, Library, Properties, Color/Swatches, Actions, Debugger, Output, History, Scene, Motion Editor, and Help/Scripting Reference built on the locked NF2/NF3 design system across four themes (`Dark`, `Light`, `Mid Neutral Gray`, `Dark Graphite`) and three densities (`Comfortable`, `Compact`, `Classic Flash`), with multi-document tabs, standalone `.as` Script Editor tabs, and dockable/floating panels.
- **Vector & bitmap authoring:** Planar Merge Drawing and Object Drawing (`J`), Pen, Subselection, Pencil, calligraphic Brush with presets and experimental tablet pressure/tilt, Free Transform, Gradient Transform, Paint Bucket gap closing, Eraser modes, boolean operations, **Trace Bitmap**, 9-slice guides, 7 live filter families, and 14 blend modes.
- **Symbols, timelines & media:** Reusable `MovieClip` (independent clock), `Graphic` (`Loop` / `Play once` / `Single frame` synchronized to parent), and 4-state `Button` (`Up` / `Over` / `Down` / `Hit`) symbols, affine **Edit in Place**, Classic, Motion (Bézier paths + `◆` property keys), and Shape tweens (`a–z` hints), motion guides, multiple scenes, geometry/alpha/luminance masks, stereo sound envelopes, and linked video/audio Razor (`C`) montage trims.
- **Scripting, conversion & automation:** Frame scripts, instance handlers, and project classes/modules in **ActionScript 1/2**, **ActionScript 3**, **JavaScript (ES2023+)**, and **TypeScript** (built-in static checker, Source Map v3, connected Chromium debugger), deterministic `Save As` language conversion (`AS1/AS2 ↔ AS3`, `AS1/AS2 → JS`, `AS3 → JS`), sandboxed **JSFL** automation, and a display-free `--headless` JSON CLI.
- **Publishing & interchange:** Self-contained HTML5 (`Full`, `Balanced`, `Compact`, `Shared`, `Web App · DOM`, zero-WASM `Web Banner · Native`, `Criteo · HTML5 ZIP`), Flash Player 8/9/10 `.swf`, versioned `.fla` / XFL (Flash 8–Animate), Lottie / `.lottie`, Texture Atlases, After Effects `.aep`/`.jsx`, raster/vector sequences, MP4/WebM/WAV/MP3, and experimental Xcode/Swift 6 export; imports binary `.fla` (Flash 3–CS4), XFL/CS5+ `.fla`, `.swf`, `.swc`, Figma (`.fig`/clipboard), Illustrator (`.ai`/PDF/AICB clipboard), Photoshop (`.psd`/`.psb`), `.svg`, `.fxg`, fonts, palettes, audio, and video.

## Quality Invariants

1. **No silent drops:** Unsupported script features, media codecs, or import/export constructs emit actionable, source-located diagnostics rather than disappearing silently.
2. **Predictable Undo:** Every completed user gesture or batch command commits as a single reversible Undo transaction.
3. **Pure authored evaluation:** Timeline scrubbing and Onion Skin evaluate deterministically without executing user scripts or mutating document state.
4. **Portable documents:** Versioned `.nflash` projects (`formats 1..=15` and opt-in `NFLASHB1` binary bundles) store no UI widget state, GPU handles, or machine-specific filesystem paths.
5. **Measured compatibility:** Product and ActionScript coverage in [`docs/progress/`](progress/README.md) reflect test-backed capabilities, never an unverified "100% Flash parity" claim.

## Repository Layout

One source tree (`main`), with versions tracked via Git tags rather than duplicated `v1/v2` directories. Platform packages, installers, and checksums live at the top level of ignored `dist/`; build caches, verification logs, and generated exports stay under ignored `target/` (`target/artifacts/`). Public documentation, ADRs, and progress manifests are versioned alongside the code in `docs/`.
