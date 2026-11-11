# NeoFlash

Open-source vector animation, interactive media and application authoring studio inspired by classic Macromedia/Adobe Flash before Animate, built in Rust for macOS 15+. NeoFlash gives illustrators, animators, web designers and game makers one responsive workspace with HTML5 + WebAssembly as its primary publishing target—requiring no Flash Player, browser plug-in or external CDN.

**Status: development preview, 0.1.0.** Authoring tools, nested timelines, tweens, motion guides, scenes, synchronized audio/video, multi-language scripting (ActionScript 1/2/3, JavaScript and TypeScript), portable HTML5 publishing, and legacy FLA (Flash 3–CS6), XFL, SWF and SWC import work within the documented boundaries below. Open items and platform limits are tracked in the [compatibility matrix](docs/COMPATIBILITY.md) and [roadmap](docs/ROADMAP.md).

## Key capabilities

- **Vector drawing & illustration:** Object Drawing and planar Merge Drawing modes (**J**), Pen (**P**), Subselection (**A**), Pencil (**Y**), calligraphic Brush (**B**) with presets and tablet pressure, Free Transform (**Q**), Gradient Transform (**F**), Paint Bucket gap closing, Eraser stroke/fill modes, boolean Combine Objects operations, **Trace Bitmap** vectorization, live filters and blend modes.
- **Timelines, symbols & sound/video:** Nested MovieClip, Button (Up/Over/Down/Hit) and Graphic symbols (**F8**), in-place affine symbol editing with dimmed parent context, Classic, Motion (editable Bézier paths + property ◆ keyframes) and Shape tweens (with shape hints), motion guides, multiple scenes, vector/alpha/luminance mask layers, and decoded stereo timeline sound waveforms with envelopes and video trim sync.
- **Scripting & debugging:** Frame scripts, placed instance handlers and project modules in **ActionScript 1/2**, **ActionScript 3**, **JavaScript (ES2023+)** and **TypeScript** (with built-in static checking, Source Map v3 and connected Chromium breakpoint debugging).
- **Publishing & interchange:** Self-contained HTML5/Canvas/DOM bundles (`Full`, `Balanced`, `Compact`, `Shared`), texture atlases, raster/vector sequences, derived Adobe Animate `.fla` / XFL archives, standalone `.swf` movies, and opening legacy binary `.fla` (Flash 3–CS4), XFL/CS5+ `.fla`, `.swf`, `.swc`, `.svg`, `.ai`, `.pdf`, `.psd` and media assets.

<!-- progress:start -->
## Current coverage

### Product functions

| Area | Functions working | Partial | Planned | Capability groups (closed / partial / planned) |
| --- | ---: | ---: | ---: | ---: |
| Editor and graphics | 151 of 163 | 11 | 1 | 7 / 5 / 0 |
| Timelines and animation | 77 of 80 | 3 | 0 | 7 / 3 / 0 |
| HTML5 and interactivity | 85 of 95 | 10 | 0 | 8 / 4 / 0 |
| Import and file compatibility | 57 of 68 | 11 | 0 | 4 / 6 / 0 |
| Platform and quality | 41 of 59 | 5 | 13 | 2 / 7 / 1 |
| **All areas** | 411 of 465 | 40 | 14 | 28 / 25 / 1 |

Each capability group lists its user-visible functions; a group is closed only when all of them work. Counts are functions, not effort or overall product completion.

### ActionScript: Flash Player target

| Language | Now (areas 1–6 working) | All Flash Player areas working | Partial / missing | Outside target (never in %) |
| --- | ---: | ---: | ---: | --- |
| ActionScript 3 — Flash Player | 1,935 of 1,939 (99.8%) | 3,716 of 4,164 (89.2%) | 447 / 1 | AIR-only APIs 1,448 · Flex, components and libraries 37,990 · Platform not resolved 138 |
| ActionScript 2 — Flash Player | 924 of 925 (99.9%) | 1,203 of 1,238 (97.2%) | 35 / 0 | AS2 UI components 1,044 · Other libraries and extensions 25 |

Test-backed declaration statuses only (not an overall Flash compatibility score); last status review: 2026-11-05. Full eight-area breakdown and per-declaration provenance: [ActionScript declaration audit](docs/progress/ACTIONSCRIPT.md).

[Current coverage](https://jazzymiles.github.io/NeoFlash/progress.html) · [Detailed registry](docs/progress/DETAILS.md) · [ActionScript details](docs/progress/ACTIONSCRIPT.md) · [Counting rules](docs/progress/README.md)

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
2. Double-click a symbol in the Library or an instance on the stage to edit a nested timeline in place with its parent context dimmed. Double-click empty canvas to leave one parent hierarchy level without changing the active tool; repeat to climb further upward, or use the Stage breadcrumbs to jump directly to any level.
3. Draw a rectangle or oval, or add text. Change position, scale, rotation, fill/stroke color, alpha and font in Properties. Fonts are discovered through macOS CoreText.
4. Press **F6** to insert a keyframe, move the object, and create a classic, shape or motion tween from the Timeline controls or the frame menu; edit its curve in Properties or **Window → Panels → Ease**. **F5** inserts a frame; **F7** adds a blank keyframe. Filled spans show held content; a blank key ends that layer's visible content. Drag a key to move it; hold Option to copy it. [timeline-tweens.nflash](examples/timeline-tweens.nflash) shows shape, motion and guided tweens in two scenes.
5. Select artwork and press **F8**. Choose a name, Movie Clip / Graphic / Button, a library folder, a registration point, and optional ActionScript linkage. Confirm to create a reusable symbol without moving its artwork; Cancel leaves the document unchanged. Drag more instances from the Library onto the Stage.
6. Select a timeline frame and press **F9**, use its context menu, or click **Actions · F9** to edit that frame's script. The editor shows the current symbol/frame and focuses the code field. Object events and project source files remain available in Actions. JavaScript/TypeScript (with static type checking, ES module bundling and Source Map v3 debugging) and ActionScript 1/2/3 are supported; see the audited declaration catalogue above. **Actions → Debug options** starts the connected Chromium debugger for source breakpoints, pause/step and read-only locals; see [setup and limits](docs/NF2-ACTIONS-EDITOR.md#56-source-maps-and-test-movie).
7. **⌘Enter / Test Movie** exports and opens a browser preview with scripts. **File → Publish** (or the Publish button in the top bar) writes a self-contained HTML5 folder. The editor's Enter playback is a transient authoring preview with independent nested MovieClip clocks, including one-frame parent timelines. **Control → Live ActionScript** is on by default and runs the bounded frame/object-script profile, including `trace()`, timeline controls and supported property overrides; turn it off for script-free animation preview. Pause retains the current pose; Rewind or scrubbing resets the preview.
8. Save a `.nflash` project with **⌘S**. Source files are versioned JSON and round-trip through validation.

## Editing and publishing

Use **Pen (P)**, **Pencil (Y)** and **Subselection (A)** for vector paths in **Object Drawing** or planar **Merge Drawing** mode (**J**). **Selection (V)** supports multiple objects, marquee selection and connected Merge-shape selection; **Free Transform (Q)** resizes, rotates and skews around a draggable transformation point. Line, PolyStar, vector Brush, Paint Bucket, Ink Bottle, Eyedropper, Eraser, whole-object Lasso with visible selection hatching, Hand, Zoom and Gradient Transform complete the tool palette. Tool groups open from their corner mark, right-click, long-press or focused →; **Customize tools** adds, removes and reorders supported tools. **Modify** and the **Align / Transform / Color / Swatches / Filters** panels add grouping, Combine Objects boolean operations (Union, Intersect, Punch, Crop), arrangement, Break Apart, exact spacing, gradients, filters, blend modes and compact numeric editing.

**Brushes** offers Round, Flat and Chisel presets; Brush properties add eight tip shapes, document-unit size, smoothing, pressure size control and gradient Lock fill. Pencil provides Straighten, Smooth and Ink modes. Physical tablet acceptance and tilt remain pending. **Modify → Bitmap → Trace bitmap** converts selected images into editable vector artwork with preview, colour and curve controls, cancellation and one Undo. Paint Bucket closes small contour gaps; Eraser **Cut strokes** preserves fills and refuses unsupported gestures atomically, with a Warning notification and Output explanation. See [tool behavior and limits](docs/WORKSPACE.md#tools-and-modify).

New and Open add document tabs; reopening the same file or imported source activates its existing tab. Documents retain independent selection, views, Undo and recovery, and copy/paste carries nested symbols, groups, timelines, resources and scripts between them. Close all and Quit preserve earlier unsaved work if a later prompt is cancelled; failed Save keeps the decision open. The workspace selector saves, updates, renames, deletes and resets named panel layouts without changing documents. See [clipboard workflows](docs/CLIPBOARD.md) and [workspace controls](docs/WORKSPACE.md).

**Actions → Project files** stores reusable AS1/AS2/AS3, JavaScript and TypeScript source, linked classes and imported SWC libraries. **Library → Resource files** embeds data, images, fonts and media for portable HTML5 projects. Runtime support covers display hierarchies, events, rich and editable text, Stage3D and Pixel Bender shaders, filters, bitmap processing, local/remote SharedObject and SQLite storage, Web Audio/video and responsive DOM layout; environment boundaries are listed in the [compatibility matrix](docs/COMPATIBILITY.md).

HTML5 export runs the shared Rust timeline/runtime through WebAssembly and a Canvas browser host. Published content needs no Flash Player or CDN. `neoflash-export --size-level` trades portability for weight: Full embeds WebAssembly in the project bundle, Balanced and Compact keep a streaming WebAssembly file plus a direct-file fallback (Compact leaves out ActionScript families the project never names), and Shared loads a runtime folder published once; `--size-estimate` compares them ([decision](docs/decisions/0038-publish-size-levels.md)). Full remains the default for older editor preferences and Test Movie. Publish settings (⇧F12) selects Full, Balanced, Compact or Shared runtime with a live on-disk weight bar, exact gzip total and hosting trade-offs. Publish uses the draft, OK saves workspace defaults, and Cancel discards changes. Frame/object JavaScript runs with the normal privileges of its containing page. The native authoring preview remains side-effect free.

Timeline masks use classic geometric silhouettes by default and switch between **Geometry**, **Alpha** and **Luminance** mask modes directly from the Timeline layer badge or **Layer Properties** (supporting gradients, PNG transparency and nested/moving mask sources on both the native Stage and published HTML5). AS2/AS3 scripts can also combine `mask` with source-and-target `cacheAsBitmap` for runtime soft alpha masking ([mask behavior and limits](docs/decisions/0023-runtime-masks-and-scroll-rectangles.md)).

System fonts are discovered through CoreText and author-selected standalone font bytes can be embedded as portable `@font-face` assets on HTML5 export. NeoFlash validates the font container but cannot determine the user's separate licence rights; lawful embedding remains the author's responsibility. Remaining native editor items awaiting NF2 design decisions, legacy binary FLA edge cases and notarized release packaging are tracked in the [roadmap](docs/ROADMAP.md).

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

**File → Open…** opens binary FLA files saved by Flash 3 through CS4, XFL and ZIP-based FLA documents (CS5+/Animate), published SWF movies (with source reconstruction and a bounded AVM1/AVM2 interpreter) and SWC libraries. **File → Save as…** can create a preflighted modern Animate `.fla` as a derived copy without retargeting the open native document or clearing its unsaved state; the other versioned FLA/XFL profiles live under **File → Export**. **File → Export Flash Movie…** writes a bounded standalone `.swf` after script/depth checks and structural re-import verification. Supported scenes, nested symbols, layers, masks, motion guides, tweens, filters, blend modes, bitmap fills, text, sounds and scripts are preserved where the selected profile supports them; unsupported content is blocked or kept as an explicit approximation instead of being silently discarded. The SWF interpreter can project its bounded runtime display list onto the Stage and supports seek, step and restart without modifying the authored project; it is still not a complete Flash Player. Missing installed fonts use fallback and do not block import. See [legacy import support and limits](docs/IMPORTS.md) and [current export boundaries](docs/COMPATIBILITY.md).

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
