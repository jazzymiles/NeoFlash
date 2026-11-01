# NeoFlash

Open-source authoring environment inspired by classic Macromedia/Adobe Flash, built in Rust for macOS 15+. The goal is one comfortable workspace for interactive websites, applications, games and animation, publishing to HTML5.

**Status: development preview, 0.1.0. This is a working foundation, not a complete Flash replacement.** Full AS2/AS3 compatibility, general FLA/SWF import, a complete vector toolset and DOM-based web layouts remain substantial work. See the [compatibility matrix](docs/COMPATIBILITY.md) and [roadmap](docs/ROADMAP.md).

The editor supports nested timelines, keyframes, classic, shape and motion tweens, motion guides, scenes, timeline sounds, frame/object scripts, system fonts and standalone HTML5 export. The authoring workspace includes vector mask/guide layers, colored layer outlines, stage rulers, guides and grid with snapping, configurable panel docks and a searchable library with folders and previews. AS2 and AS3 support remains partial; the catalogue below shows the current audited coverage. Runtime profiles and known limitations are described in the [compatibility matrix](docs/COMPATIBILITY.md).

<!-- progress:start -->
## Where we are

The editor foundation works: nested timelines, vector tools, scripting and standalone HTML5. Full AS2/AS3, FLA/SWF import, portable media and DOM applications remain in development.

### ActionScript: Flash Player areas

**Working**: the main behavior from the Adobe reference runs and a linked test proves it. **Partial**: it runs, with the missing behavior named. **Missing**: no runtime path yet. **Needs review**: not yet classified. Statuses change only through test-backed reviews; this is not a Flash compatibility score. Last status review: 2026-11-01.

#### ActionScript 3 — Flash Player

| Flash Player area | Entries | Working | Partial | Missing | Needs review | Working | Working or partial |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1. Mathematics and 2D geometry | 155 | 2 | 152 | 1 | 0 | 1.3% | 99.4% |
| 2. Objects, language and collections | 582 | 224 | 285 | 73 | 0 | 38.5% | 87.5% |
| 3. MovieClip and timeline | 37 | 12 | 20 | 5 | 0 | 32.4% | 86.5% |
| 4. Display, events and basic input | 678 | 477 | 135 | 66 | 0 | 70.4% | 90.3% |
| 5. Graphics, bitmaps and filters | 313 | 222 | 21 | 70 | 0 | 70.9% | 77.6% |
| 6. Text and fonts | 174 | 128 | 26 | 20 | 0 | 73.6% | 88.5% |
| 7. File audio and video playback | 91 | 5 | 34 | 52 | 0 | 5.5% | 42.9% |
| 8. Services and advanced Player APIs | 2,134 | 7 | 355 | 1,772 | 0 | 0.3% | 17.0% |
| **Now: areas 1–6** | 1,939 | 1,065 | 639 | 235 | 0 | 54.9% | 87.9% |
| **All Flash Player areas** | 4,164 | 1,077 | 1,028 | 2,059 | 0 | 25.9% | 50.6% |

Outside the target and never in a percentage: AIR-only APIs 1,448 · Flex, components and libraries 37,990 · Platform not resolved 138.

#### ActionScript 2 — Flash Player

| Flash Player area | Entries | Working | Partial | Missing | Needs review | Working | Working or partial |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1. Mathematics and 2D geometry | 128 | 124 | 4 | 0 | 0 | 96.9% | 100.0% |
| 2. Objects, language and collections | 281 | 26 | 255 | 0 | 0 | 9.3% | 100.0% |
| 3. MovieClip and timeline | 123 | 7 | 116 | 0 | 0 | 5.7% | 100.0% |
| 4. Display, events and basic input | 130 | 5 | 125 | 0 | 0 | 3.8% | 100.0% |
| 5. Graphics, bitmaps and filters | 145 | 3 | 142 | 0 | 0 | 2.1% | 100.0% |
| 6. Text and fonts | 118 | 4 | 114 | 0 | 0 | 3.4% | 100.0% |
| 7. File audio and video playback | 83 | 6 | 77 | 0 | 0 | 7.2% | 100.0% |
| 8. Services and advanced Player APIs | 230 | 14 | 216 | 0 | 0 | 6.1% | 100.0% |
| **Now: areas 1–6** | 925 | 169 | 756 | 0 | 0 | 18.3% | 100.0% |
| **All Flash Player areas** | 1,238 | 189 | 1,049 | 0 | 0 | 15.3% | 100.0% |

Outside the target and never in a percentage: AS2 UI components 1,044 · Other libraries and extensions 25.

### Product functions

| Area | Functions working | Partial | Planned | Capability groups (closed / partial / planned) |
| --- | ---: | ---: | ---: | ---: |
| Editor and graphics | 82 of 111 | 11 | 18 | 4 / 8 / 0 |
| Timelines and animation | 61 of 74 | 8 | 5 | 5 / 5 / 0 |
| HTML5 and interactivity | 47 of 74 | 4 | 23 | 5 / 5 / 2 |
| Import and file compatibility | 41 of 66 | 14 | 11 | 1 / 7 / 2 |
| Platform and quality | 32 of 50 | 0 | 18 | 4 / 5 / 1 |
| **All areas** | 263 of 375 | 37 | 75 | 19 / 30 / 5 |

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
4. Press **F6** to insert a keyframe, move the object, and create a classic, shape or motion tween from the Timeline controls or the frame menu; edit its curve in Properties or **Window → Ease**. **F5** inserts a frame; **F7** adds a blank keyframe. Filled spans show held content; a blank key ends that layer's visible content. Drag a key to move it; hold Option to copy it. [timeline-tweens.nflash](examples/timeline-tweens.nflash) shows shape, motion and guided tweens in two scenes.
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
| Tweens, motion guides and scenes | [Timeline tweens](examples/timeline-tweens.nflash) |
| Timeline sound with Flash sync | [Timeline sound](examples/timeline-sound.nflash) |
| Audio and video | [Sound playback](examples/sound-playback.nflash), [Video playback](examples/video-playback.nflash) |
| Browser integration and dragging | [Web integration](examples/web-integration.nflash), [Drag and drop](examples/drag-drop.nflash) |

**File → Import → Import legacy Flash…** opens binary FLA files saved by Flash 3 through CS4. Scenes, nested symbols, layers, masks, motion guides, classic and shape tweens and the original scripts are kept; Flash 3–MX Actions-panel commands become ActionScript 1. Filters, blend modes, bitmap fills, rich text, sounds and other details the model cannot express yet are approximated or kept as visible placeholders, and the import report lists each case. The imported project embeds its exact source file when it fits. Missing installed fonts use fallback and do not block import. See [legacy import support and limits](docs/IMPORTS.md).

The [public FLA corpus](examples/fla-corpus/README.md) includes licensed, pinned source files. All eight binary FLA sources (Flash 5, MX, 8 and CS4) import through the native converter, and an Adobe CS6 example imports through XFL and reproduces its reference trace in HTML5. Unsupported cases retain explicit diagnostics; these examples do not establish general FLA compatibility.

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

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option. Contributions use the same terms. Third-party code and fixtures retain their licenses and pinned provenance; see [third-party notices](THIRD_PARTY_NOTICES.txt). The macOS editor plays timeline sounds through unmodified MPL-2.0 Symphonia decoder crates; they are not part of published HTML5 content. This independent project does not include Adobe application code or branding assets.
