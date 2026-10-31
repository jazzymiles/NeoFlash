# NeoFlash

Open-source authoring environment inspired by classic Macromedia/Adobe Flash, built in Rust for macOS 15+. The goal is one comfortable workspace for interactive websites, applications, games and animation, publishing to HTML5.

**Status: development preview, 0.1.0. This is a working foundation, not a complete Flash replacement.** Full AS2/AS3 compatibility, general FLA/SWF import, a complete vector toolset and DOM-based web layouts remain substantial work. See the [compatibility matrix](docs/COMPATIBILITY.md) and [roadmap](docs/ROADMAP.md).

The editor supports nested timelines, keyframes and tweens, frame/object scripts, system fonts and standalone HTML5 export. The authoring workspace includes vector mask/guide layers, colored layer outlines, configurable panel docks and a searchable library with folders and previews. AS2 and AS3 support remains partial; the catalogue below shows the current audited coverage. Runtime profiles and known limitations are described in the [compatibility matrix](docs/COMPATIBILITY.md).

<!-- progress:start -->
## Where we are

The editor foundation works: nested timelines, vector tools, scripting and standalone HTML5. Full AS2/AS3, FLA/SWF import, portable media and DOM applications remain in development.

### ActionScript: Flash Player areas

**Working**: the main behavior from the Adobe reference runs and a linked test proves it. **Partial**: it runs, with the missing behavior named. **Missing**: no runtime path yet. **Needs review**: not yet classified. Statuses change only through test-backed reviews; this is not a Flash compatibility score. Last status review: 2026-11-01.

#### ActionScript 3 — Flash Player

| Flash Player area | Entries | Working | Partial | Missing | Needs review | Working | Working or partial |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1. Mathematics and 2D geometry | 155 | 0 | 152 | 1 | 2 | 0.0% | 98.1% |
| 2. Objects, language and collections | 582 | 16 | 361 | 112 | 93 | 2.7% | 64.8% |
| 3. MovieClip and timeline | 37 | 10 | 22 | 5 | 0 | 27.0% | 86.5% |
| 4. Display, events and basic input | 678 | 24 | 183 | 471 | 0 | 3.5% | 30.5% |
| 5. Graphics, bitmaps and filters | 313 | 3 | 151 | 159 | 0 | 1.0% | 49.2% |
| 6. Text and fonts | 174 | 3 | 58 | 113 | 0 | 1.7% | 35.1% |
| 7. File audio and video playback | 91 | 0 | 35 | 54 | 2 | 0.0% | 38.5% |
| 8. Services and advanced Player APIs | 2,134 | 0 | 359 | 1,775 | 0 | 0.0% | 16.8% |
| **Now: areas 1–6** | 1,939 | 56 | 927 | 861 | 95 | 2.9% | 50.7% |
| **All Flash Player areas** | 4,164 | 56 | 1,321 | 2,690 | 97 | 1.3% | 33.1% |

Outside the target and never in a percentage: AIR-only APIs 1,448 · Flex, components and libraries 37,990 · Platform not resolved 138.

#### ActionScript 2 — Flash Player

| Flash Player area | Entries | Working | Partial | Missing | Needs review | Working | Working or partial |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1. Mathematics and 2D geometry | 128 | 0 | 128 | 0 | 0 | 0.0% | 100.0% |
| 2. Objects, language and collections | 281 | 14 | 255 | 3 | 9 | 5.0% | 95.7% |
| 3. MovieClip and timeline | 123 | 1 | 111 | 7 | 4 | 0.8% | 91.1% |
| 4. Display, events and basic input | 130 | 0 | 122 | 6 | 2 | 0.0% | 93.8% |
| 5. Graphics, bitmaps and filters | 145 | 0 | 141 | 0 | 4 | 0.0% | 97.2% |
| 6. Text and fonts | 118 | 0 | 103 | 13 | 2 | 0.0% | 87.3% |
| 7. File audio and video playback | 83 | 0 | 66 | 16 | 1 | 0.0% | 79.5% |
| 8. Services and advanced Player APIs | 230 | 0 | 163 | 54 | 13 | 0.0% | 70.9% |
| **Now: areas 1–6** | 925 | 15 | 860 | 29 | 21 | 1.6% | 94.6% |
| **All Flash Player areas** | 1,238 | 15 | 1,089 | 99 | 35 | 1.2% | 89.2% |

Outside the target and never in a percentage: AS2 UI components 1,044 · Other libraries and extensions 25.

### Product functions

| Area | Functions working | Partial | Planned | Capability groups (closed / partial / planned) |
| --- | ---: | ---: | ---: | ---: |
| Editor and graphics | 65 of 95 | 7 | 23 | 4 / 8 / 0 |
| Timelines and animation | 47 of 65 | 4 | 14 | 4 / 5 / 1 |
| HTML5 and interactivity | 46 of 73 | 4 | 23 | 5 / 5 / 2 |
| Import and file compatibility | 32 of 61 | 2 | 27 | 1 / 7 / 2 |
| Platform and quality | 28 of 46 | 0 | 18 | 4 / 5 / 1 |
| **All areas** | 218 of 340 | 17 | 105 | 18 / 30 / 6 |

Each capability group lists its user-visible functions; a group is closed only when all of them work. Counts are functions, not effort or overall product completion.

[Current coverage](https://jazzymiles.github.io/NeoFlash/progress.html) · [Daily history](https://jazzymiles.github.io/NeoFlash/history.html) · [Detailed registry](docs/progress/DETAILS.md) · [ActionScript details](docs/progress/ACTIONSCRIPT.md) · [Counting rules](docs/progress/README.md)

Generated from `docs/progress/`; GitHub Pages rebuilds on every push to `main`.
<!-- progress:end -->

## Run

Install [Rust with rustup](https://rust-lang.org/tools/install/) and Xcode Command Line Tools on macOS. The repository pins its Rust toolchain.

```sh
./scripts/run.sh                    # build the browser runtime and open the editor
./scripts/package-macos.sh          # optimized dist/NeoFlash.app
open dist/NeoFlash.app
```

The first build downloads pinned Rust/WASM dependencies. Apple Silicon is locally tested; Intel support remains unverified. The local bundle is ad-hoc signed, not a notarized release. Node.js 22+ is needed for development checks, not for the application or published content. See [contributing](CONTRIBUTING.md) for verification commands.

## Try the workflow

Open [authoring-workspace.nflash](examples/authoring-workspace.nflash) for masks, guides, a linked library symbol, four-state buttons and Graphic playback. Drag [authoring-badge.svg](examples/authoring-badge.svg) onto the stage to import editable artwork. PNG/JPEG and more complex SVG artwork also import through **File → Import graphics to Stage**.

1. Open the example, scrub the top timeline and enable **Onion Skin**. Drag the blue range handles or change Before/After; choose outlines and opacity.
2. Double-click a symbol in the Library or an instance on the stage to edit a nested timeline. Use the breadcrumbs to return.
3. Draw a rectangle or oval, or add text. Change position, scale, rotation, fill/stroke color, alpha and font in Properties. Fonts are discovered through macOS CoreText.
4. Press **F6** to insert a keyframe, move the object, and set a tween/easing on the preceding keyframe. **F5** inserts a frame; **F7** adds a blank keyframe. Filled spans show held content; a blank key ends that layer's visible content. Drag a key to move it; hold Option to copy it.
5. Select artwork and press **F8**. Choose a name, Movie Clip / Graphic / Button, a library folder, a registration point, and optional ActionScript linkage. Confirm to create a reusable symbol without moving its artwork; Cancel leaves the document unchanged. Drag more instances from the Library onto the Stage.
6. Select a timeline frame and press **F9**, use its context menu, or click **Actions · F9** to edit that frame's script. The editor shows the current symbol/frame and focuses the code field. Object events and project source files remain available in Actions. JavaScript/TypeScript are the primary working languages; the AS2/AS3 subset is documented explicitly.
7. **⌘Enter / Test Movie** exports and opens a browser preview with scripts. **File → Publish** (or the Publish button in the top bar) writes a self-contained HTML5 folder. The editor's Enter playback is a side-effect-free authoring preview with independent nested MovieClip clocks, including one-frame parent timelines. Pause retains the current pose; Rewind or scrubbing resets the preview.
8. Save a `.nflash` project with **⌘S**. Source files are versioned JSON and round-trip through validation.

## Editing and publishing

Use **Pen (P)**, **Pencil (Y)** and **Subselection (A)** for vector paths. **Selection (V)** supports multiple objects and marquee selection; **Free Transform (Q)** resizes and rotates them. Line, Brush, Bucket, Ink Bottle, Eyedropper, Eraser, Lasso, Hand, Zoom and Gradient Transform extend the compact tool palette. **Modify** and the **Align / Transform / Color** panels add grouping, arrangement, exact spacing and compact numeric editing. Copy/paste carries nested symbols, groups and scripts between documents. See [clipboard workflows](docs/CLIPBOARD.md) and [workspace controls](docs/WORKSPACE.md).

**Actions → Project files** stores reusable AS2/AS3 source and linked classes. **Library → Resource files** embeds data, images and media for portable HTML5 projects. Runtime support includes partial display, events, text input, storage, audio/video and browser integration; each has explicit limits in the [compatibility matrix](docs/COMPATIBILITY.md).

HTML5 export runs the shared Rust timeline/runtime through WebAssembly and a Canvas browser host. Published content needs no Flash Player or CDN. Frame/object JavaScript runs with the normal privileges of its containing page. The native authoring preview remains side-effect free.

System fonts are referenced by name; font embedding and identical cross-platform typography remain unfinished. General FLA/SWF import, full AS2/AS3 semantics, responsive DOM layouts and production packaging are also unfinished. The [roadmap](docs/ROADMAP.md) describes the remaining work.

## Examples

| Workflow | Source project |
| --- | --- |
| Nested animation and timelines | [Nested animation](examples/nested-orbit.nflash) |
| Vector paths and compound fills | [Vector drawing](examples/vector-paths.nflash) |
| Linked classes and input events | [Project program](examples/project-program.nflash), [AS2 input](examples/as2-project-input.nflash) |
| Text entry and saved values | [Text form](examples/text-form.nflash), [Saved game](examples/saved-game.nflash) |
| Images, filters and resource loading | [Loader gallery](examples/loader-gallery.nflash), [Filter gallery](examples/filter-gallery.nflash) |
| Audio and video | [Sound playback](examples/sound-playback.nflash), [Video playback](examples/video-playback.nflash) |
| Browser integration and dragging | [Web integration](examples/web-integration.nflash), [Drag and drop](examples/drag-drop.nflash) |

Native binary FLA import now covers the supported Flash MX/CS4 profiles exercised by `Clock.fla` and `300x600.fla`, preserving nested structure, classic animation and original scripts. The imported project embeds its exact source archive. Clock uses a partial AS1 timeline profile; the banner retains AS2. Missing installed fonts use fallback and do not block import. See [legacy import support and limits](docs/IMPORTS.md).

The [public FLA corpus](examples/fla-corpus/README.md) includes licensed, pinned source files. Accepted cases include MX button/static-text files through the native profile and an Adobe CS6 example through XFL. The CS6 example reproduces its reference trace in HTML5. Unsupported cases retain explicit diagnostics; these examples do not establish general FLA compatibility.

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

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option. Contributions use the same terms. Third-party code and fixtures retain their licenses and pinned provenance; see [third-party notices](THIRD_PARTY_NOTICES.txt). This independent project does not include Adobe application code or branding assets.
