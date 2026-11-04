# NeoFlash

Open-source authoring environment inspired by classic Macromedia/Adobe Flash, built in Rust for macOS 15+. The goal is one comfortable workspace for interactive websites, applications, games and animation, publishing to HTML5.

**Status: development preview, 0.1.0.** The authoring environment, timeline engine, vector toolset (including Object and Merge Drawing modes and boolean Combine Objects operations), ActionScript 1/2/3 and TypeScript/JavaScript compiler and runtime, portable HTML5/DOM export, and FLA (Flash 3–CS6), XFL, SWF and SWC import are working across the audited scope below. Remaining native editor items awaiting NF2 design decisions, legacy binary FLA edge cases and notarized release packaging are tracked in the [compatibility matrix](docs/COMPATIBILITY.md) and [roadmap](docs/ROADMAP.md).

The editor supports nested timelines, keyframes, classic, shape (with shape hints) and motion tweens with editable Bézier paths and selective motion paste, motion guides, scenes, timeline sounds with decoded waveforms, envelopes and trim windows, Object and Merge Drawing modes, boolean shape operations, filters and blend modes, frame/object scripts, embedded and system fonts, and standalone HTML5 or XFL export. The authoring workspace includes vector and alpha mask layers, guide and folder layers, colored layer outlines, stage rulers, guides and grid with snapping, configurable panel docks with saved layouts, Movie Explorer, Find and Replace, Code Snippets, and a searchable Library with folders and previews. ActionScript 1/2/3 and JavaScript/TypeScript coverage is audited below; runtime profiles and known limitations are described in the [compatibility matrix](docs/COMPATIBILITY.md).

<!-- progress:start -->
## Where we are

The authoring workspace, nested timelines, vector tools (Object and Merge Drawing, boolean operations), ActionScript 1/2/3 and TypeScript/JavaScript runtime, portable HTML5/DOM export, and FLA/XFL/SWF/SWC import work across the audited scope below.

### ActionScript: Flash Player areas

**Working**: the main behavior from the Adobe reference runs and a linked test proves it. **Partial**: it runs, with the missing behavior named. **Missing**: no runtime path yet. **Needs review**: not yet classified. Statuses change only through test-backed reviews; this is not a Flash compatibility score. Last status review: 2026-11-02.

#### ActionScript 3 — Flash Player

| Flash Player area | Entries | Working | Partial | Missing | Needs review | Working | Working or partial |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1. Mathematics and 2D geometry | 155 | 155 | 0 | 0 | 0 | 100.0% | 100.0% |
| 2. Objects, language and collections | 582 | 582 | 0 | 0 | 0 | 100.0% | 100.0% |
| 3. MovieClip and timeline | 37 | 37 | 0 | 0 | 0 | 100.0% | 100.0% |
| 4. Display, events and basic input | 678 | 678 | 0 | 0 | 0 | 100.0% | 100.0% |
| 5. Graphics, bitmaps and filters | 313 | 313 | 0 | 0 | 0 | 100.0% | 100.0% |
| 6. Text and fonts | 174 | 174 | 0 | 0 | 0 | 100.0% | 100.0% |
| 7. File audio and video playback | 91 | 91 | 0 | 0 | 0 | 100.0% | 100.0% |
| 8. Services and advanced Player APIs | 2,134 | 1,692 | 442 | 0 | 0 | 79.3% | 100.0% |
| **Now: areas 1–6** | 1,939 | 1,939 | 0 | 0 | 0 | 100.0% | 100.0% |
| **All Flash Player areas** | 4,164 | 3,722 | 442 | 0 | 0 | 89.4% | 100.0% |

Outside the target and never in a percentage: AIR-only APIs 1,448 · Flex, components and libraries 37,990 · Platform not resolved 138.

#### ActionScript 2 — Flash Player

| Flash Player area | Entries | Working | Partial | Missing | Needs review | Working | Working or partial |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1. Mathematics and 2D geometry | 128 | 128 | 0 | 0 | 0 | 100.0% | 100.0% |
| 2. Objects, language and collections | 281 | 281 | 0 | 0 | 0 | 100.0% | 100.0% |
| 3. MovieClip and timeline | 123 | 123 | 0 | 0 | 0 | 100.0% | 100.0% |
| 4. Display, events and basic input | 130 | 130 | 0 | 0 | 0 | 100.0% | 100.0% |
| 5. Graphics, bitmaps and filters | 145 | 145 | 0 | 0 | 0 | 100.0% | 100.0% |
| 6. Text and fonts | 118 | 118 | 0 | 0 | 0 | 100.0% | 100.0% |
| 7. File audio and video playback | 83 | 83 | 0 | 0 | 0 | 100.0% | 100.0% |
| 8. Services and advanced Player APIs | 230 | 196 | 34 | 0 | 0 | 85.2% | 100.0% |
| **Now: areas 1–6** | 925 | 925 | 0 | 0 | 0 | 100.0% | 100.0% |
| **All Flash Player areas** | 1,238 | 1,204 | 34 | 0 | 0 | 97.3% | 100.0% |

Outside the target and never in a percentage: AS2 UI components 1,044 · Other libraries and extensions 25.

### Product functions

| Area | Functions working | Partial | Planned | Capability groups (closed / partial / planned) |
| --- | ---: | ---: | ---: | ---: |
| Editor and graphics | 109 of 125 | 3 | 13 | 6 / 6 / 0 |
| Timelines and animation | 73 of 76 | 1 | 2 | 7 / 3 / 0 |
| HTML5 and interactivity | 79 of 82 | 2 | 1 | 10 / 2 / 0 |
| Import and file compatibility | 61 of 67 | 6 | 0 | 7 / 3 / 0 |
| Platform and quality | 39 of 51 | 0 | 12 | 5 / 4 / 1 |
| **All areas** | 361 of 401 | 12 | 28 | 35 / 18 / 1 |

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
6. Select a timeline frame and press **F9**, use its context menu, or click **Actions · F9** to edit that frame's script. The editor shows the current symbol/frame and focuses the code field. Object events and project source files remain available in Actions. JavaScript/TypeScript (with static type checking, ES module bundling and Source Map v3 debugging) and ActionScript 1/2/3 are supported; see the audited declaration catalogue above. **Actions → Debug options** starts the connected Chromium debugger for source breakpoints, pause/step and read-only locals; see [setup and limits](docs/NF2-ACTIONS-EDITOR.md#56-source-maps-and-test-movie).
7. **⌘Enter / Test Movie** exports and opens a browser preview with scripts. **File → Publish** (or the Publish button in the top bar) writes a self-contained HTML5 folder. The editor's Enter playback is a side-effect-free authoring preview with independent nested MovieClip clocks, including one-frame parent timelines. Pause retains the current pose; Rewind or scrubbing resets the preview.
8. Save a `.nflash` project with **⌘S**. Source files are versioned JSON and round-trip through validation.

## Editing and publishing

Use **Pen (P)**, **Pencil (Y)** and **Subselection (A)** for vector paths in **Object Drawing** or planar **Merge Drawing** mode (**J**). **Selection (V)** supports multiple objects, marquee selection and connected Merge-shape selection; **Free Transform (Q)** resizes, rotates and skews around a draggable transformation point. Line, PolyStar, round vector Brush, closed-shape Paint Bucket, Ink Bottle, Eyedropper, whole-object/fill/stroke Eraser, whole-object Lasso with visible selection hatching, Hand, Zoom and Gradient Transform complete the tool palette. Narrow Tools groups open from their corner mark, right-click, long-press or focused →. **Modify** and the **Align / Transform / Color / Swatches / Filters** panels add grouping, Combine Objects boolean operations (Union, Intersect, Punch, Crop), arrangement, Break Apart, exact spacing, gradients, filters, blend modes and compact numeric editing. Copy/paste carries nested symbols, groups, timelines, resources and scripts between documents. Brush accepts pressure input; physical tablet acceptance and pressure/tilt controls remain pending. Bucket gap controls, stroke-segment erasing, named workspace management and multiple-document tabs remain unavailable in the interface. Failed Open or Save keeps unsaved work and recovery copies. See [clipboard workflows](docs/CLIPBOARD.md) and [workspace controls](docs/WORKSPACE.md).

**Actions → Project files** stores reusable AS1/AS2/AS3, JavaScript and TypeScript source, linked classes and imported SWC libraries. **Library → Resource files** embeds data, images, fonts and media for portable HTML5 projects. Runtime support covers display hierarchies, events, rich and editable text, Stage3D and Pixel Bender shaders, filters, bitmap processing, local/remote SharedObject and SQLite storage, Web Audio/video and responsive DOM layout; environment boundaries are listed in the [compatibility matrix](docs/COMPATIBILITY.md).

HTML5 export runs the shared Rust timeline/runtime through WebAssembly and a Canvas browser host. Published content needs no Flash Player or CDN. `neoflash-export --size-level` trades portability for weight: Full opens from disk, Balanced and Compact need HTTP(S) hosting (Compact leaves out ActionScript families the project never names), and Shared loads a runtime folder published once; `--size-estimate` compares them ([decision](docs/decisions/0038-publish-size-levels.md)). Full remains the default for older editor preferences and Test Movie. Publish uses the saved size/font options, while the Publish settings selector and live weight display still await NF2 design. Frame/object JavaScript runs with the normal privileges of its containing page. The native authoring preview remains side-effect free.

System fonts are discovered through CoreText and can be subsetted and embedded as portable `@font-face` assets on HTML5 export. Remaining native editor items awaiting NF2 design decisions, legacy binary FLA edge cases and notarized release packaging are tracked in the [roadmap](docs/ROADMAP.md).

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

**File → Import → Import legacy Flash…** opens binary FLA files saved by Flash 3 through CS4, XFL and ZIP-based FLA documents (CS5+/Animate), published SWF movies (with AVM1/AVM2 bytecode playback and source reconstruction) and SWC libraries, and **File → Save as…** exports round-trip `.fla` (XFL 2.2) archives. Scenes, nested symbols, layers, masks, motion guides, classic/shape/motion tweens, filters, blend modes, bitmap fills, rich text, sounds and scripts are preserved; any unrepresented legacy records are approximated or kept as visible placeholders with a located diagnostic in the import report. Missing installed fonts use fallback and do not block import. See [legacy import support and limits](docs/IMPORTS.md).

The [public FLA corpus](examples/fla-corpus/README.md) includes licensed, pinned source files. All eight binary FLA sources (Flash 5, MX, 8 and CS4) and seven external XFL/FLA sources import through the native converter with 21 imports and 0 refusals, and an Adobe CS6 example reproduces its reference trace in HTML5. Unsupported cases retain explicit diagnostics; see [legacy import support and limits](docs/IMPORTS.md).

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
