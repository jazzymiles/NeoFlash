# NeoFlash

Open-source authoring environment inspired by classic Macromedia/Adobe Flash, built in Rust for macOS 15+. The goal is one comfortable workspace for interactive websites, applications, games and animation, publishing to HTML5.

**Status: development preview, 0.1.0. This is a working foundation, not a complete Flash replacement.** Full AS2/AS3 compatibility, general FLA/SWF import, a complete vector toolset and DOM-based web layouts remain substantial work. See the [compatibility matrix](docs/COMPATIBILITY.md) and [roadmap](docs/ROADMAP.md).

The editor supports nested timelines, keyframes and tweens, frame/object scripts, system fonts and standalone HTML5 export. The authoring workspace includes vector mask/guide layers, colored layer outlines, configurable panel docks and a searchable library with folders and previews. AS2 and AS3 support remains partial; the catalogue below shows the current audited coverage. Runtime profiles and known limitations are described in the [compatibility matrix](docs/COMPATIBILITY.md).

<!-- progress:start -->
## Where we are

The editor foundation works: nested timelines, vector tools, scripting and standalone HTML5. Full AS2/AS3, FLA/SWF import, portable media and DOM applications remain in development.

### ActionScript reference coverage

| Language | Documented entries | Implemented subset | Partial | Missing | Needs review |
| --- | ---: | ---: | ---: | ---: | ---: |
| AS2 | 2,307 | 15 | 1,518 | 737 | 37 |
| AS3 | 43,740 | 15 | 1,254 | 42,308 | 163 |

Counts use the complete selected documentation catalogue, including AIR, Flex and components. “Implemented subset” covers only the audited rule, not the whole API contract. Missing and unreviewed declarations remain separate. These are **not Flash compatibility percentages**. See the [dated audit and scope breakdown](docs/progress/ACTIONSCRIPT.md).

### Product capabilities

| Area | Implemented | Partial | Planned | Registry coverage |
| --- | ---: | ---: | ---: | ---: |
| Editor and graphics | 5/12 | 6 | 1 | 41.7% |
| Timelines and animation | 5/10 | 3 | 2 | 50.0% |
| HTML5 and interactivity | 6/12 | 4 | 2 | 50.0% |
| Import and file compatibility | 1/10 | 6 | 3 | 10.0% |
| Platform and quality | 4/10 | 5 | 1 | 40.0% |

Implemented means the item's stated scope is complete; partial items do not contribute to the percentage. This measures our capability registry, **not Flash compatibility or test line coverage**. Items differ in size, so there is no overall product completion percentage.

[Current coverage](https://jazzymiles.github.io/NeoFlash/progress.html) · [Daily history](https://jazzymiles.github.io/NeoFlash/history.html) · [Detailed registry](docs/progress/DETAILS.md) · [Counting rules](docs/progress/README.md)

This summary is generated from `docs/progress/*.json`; GitHub Pages rebuilds on every push to `main`.
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

Open [authoring-workspace.nflash](examples/authoring-workspace.nflash) for mask layers, guides and a linked library symbol. Drag [authoring-badge.svg](examples/authoring-badge.svg) onto the stage to import editable artwork. PNG/JPEG and more complex SVG artwork also import through **File → Import graphics to Stage**.

1. Open the example, scrub the top timeline and enable **Onion Skin**. Drag the blue range handles or change Before/After; choose outlines and opacity.
2. Double-click a symbol in the Library or an instance on the stage to edit a nested timeline. Use the breadcrumbs to return.
3. Draw a rectangle or oval, or add text. Change position, scale, rotation, fill/stroke color, alpha and font in Properties. Fonts are discovered through macOS CoreText.
4. Press **F6** to insert a keyframe, move the object, and set a tween/easing on the preceding keyframe. **F5** inserts a frame; **F7** adds a blank keyframe. Filled spans show held content; a blank key ends that layer's visible content. Drag a key to move it; hold Option to copy it.
5. Select artwork and press **F8**. Choose a name, Movie Clip / Graphic / Button, a library folder, a registration point, and optional ActionScript linkage. Confirm to create a reusable symbol without moving its artwork; Cancel leaves the document unchanged. Drag more instances from the Library onto the Stage.
6. Select a timeline frame and press **F9**, use its context menu, or click **Actions · F9** to edit that frame's script. The editor shows the current symbol/frame and focuses the code field. Object events and project source files remain available in Actions. JavaScript/TypeScript are the primary working languages; the AS2/AS3 subset is documented explicitly.
7. **⌘Enter / Test Movie** exports and opens a browser preview with scripts. **File → Publish HTML5** writes a self-contained folder. The editor's Enter playback is a side-effect-free authoring preview.
8. Save a `.nflash` project with **⌘S**. Source files are versioned JSON and round-trip through validation.

## Editing and publishing

Use **Pen (P)**, **Pencil (Y)** and **Subselection (A)** for vector paths. **Selection (V)** supports multiple objects and marquee selection; **Free Transform (Q)** resizes and rotates them. Copy/paste carries nested symbols and scripts between documents. See [clipboard workflows](docs/CLIPBOARD.md) and [workspace controls](docs/WORKSPACE.md).

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

The [public FLA corpus](examples/fla-corpus/README.md) includes licensed, pinned source files. One unchanged Adobe CS6 example imports and reproduces its reference trace in HTML5; other external cases retain explicit refusals. This is a bounded import profile, not general FLA compatibility.

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
