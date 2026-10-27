# NeoFlash

Open-source authoring environment inspired by classic Macromedia/Adobe Flash, built in Rust for macOS 15+. The goal is one comfortable workspace for interactive websites, applications, games and animation, publishing to HTML5.

**Status: development preview, 0.1.0. This is a working foundation, not a complete Flash replacement.** Full AS2/AS3 compatibility, general FLA/SWF import, a complete vector toolset and DOM-based web layouts remain substantial work. See the [compatibility matrix](docs/COMPATIBILITY.md) and [roadmap](docs/ROADMAP.md).

<!-- progress:start -->
## Where we are

The editor foundation works: nested timelines, vector tools, scripting and standalone HTML5. Full AS2/AS3, FLA/SWF import, portable media and DOM applications remain in development.

### ActionScript reference coverage

| Language | Documented entries | Implemented subset | Partial | Missing | Needs review |
| --- | ---: | ---: | ---: | ---: | ---: |
| AS2 | 2,307 | 15 | 218 | 1,957 | 117 |
| AS3 | 43,740 | 15 | 297 | 43,178 | 250 |

Counts use the complete selected documentation catalogue, including AIR, Flex and components. “Implemented subset” covers only the audited rule, not the whole API contract. Missing and unreviewed declarations remain separate. These are **not Flash compatibility percentages**. See the [dated audit and scope breakdown](docs/progress/ACTIONSCRIPT.md).

### Product capabilities

| Area | Implemented | Partial | Planned | Registry coverage |
| --- | ---: | ---: | ---: | ---: |
| Editor and graphics | 5/12 | 5 | 2 | 41.7% |
| Timelines and animation | 5/10 | 3 | 2 | 50.0% |
| HTML5 and interactivity | 6/12 | 3 | 3 | 50.0% |
| Import and file compatibility | 1/10 | 6 | 3 | 10.0% |
| Platform and quality | 4/10 | 5 | 1 | 40.0% |

Implemented means the item's stated scope is complete; partial items do not contribute to the percentage. This measures our capability registry, **not Flash compatibility or test line coverage**. Items differ in size, so there is no overall product completion percentage.

[Current coverage](https://jazzymiles.github.io/NeoFlash/progress.html) · [Daily history](https://jazzymiles.github.io/NeoFlash/history.html) · [Detailed registry](docs/progress/DETAILS.md) · [Counting rules](docs/progress/README.md)

This summary is generated from `docs/progress/*.json`; GitHub Pages rebuilds on every push to `main`.
<!-- progress:end -->

## Run

Install [Rust with rustup](https://rust-lang.org/tools/install/) and Xcode Command Line Tools on macOS. The repository pins its Rust toolchain. Node.js 22+ is used only by the test suite, not by the desktop application or published content.

```sh
./scripts/run.sh
```

The script builds the shared browser runtime and opens the editor with the included nested-animation example. The first build downloads the pinned Rust/WASM dependencies.

```sh
./scripts/package-macos.sh           # dist/NeoFlash.app, optimized build
open dist/NeoFlash.app
./scripts/check.sh                   # formatting, lint, Rust and real-WASM tests
```

The local bundle is ad-hoc signed, not a notarized distribution release. Apple Silicon is the locally tested architecture; Intel macOS support remains unverified.

## Try the workflow

1. Open the example, scrub the top timeline and enable **Onion Skin**. Drag the blue range handles or change Before/After; choose outlines and opacity.
2. Double-click a symbol in the Library or an instance on the stage to edit a nested timeline. Use the breadcrumbs to return.
3. Draw a rectangle or oval, or add text. Change position, scale, rotation, color and font in Properties. Fonts are discovered through macOS CoreText.
4. Press **F6** to insert a keyframe, move the object, and set a tween/easing on the preceding keyframe. **F5** extends a layer; **F7** adds a blank keyframe. Drag a key to move it; hold Option to copy it.
5. Select an object and press **F8** to turn it into a reusable symbol. Place more instances from the Library.
6. Open **Actions** with **F9**. Choose a frame or selected object and an event. JavaScript/TypeScript are the primary working languages; the AS2/AS3 subset is documented explicitly.
7. **⌘Enter / Test Movie** exports and opens a browser preview with scripts. **File → Publish HTML5** writes a self-contained folder. The editor's Enter playback is a side-effect-free authoring preview.
8. Save a `.nflash` project with **⌘S**. Source files are versioned JSON and round-trip through validation.

For vector drawing, choose **Pen (P)**: click for corners, drag for Bézier handles, click the first anchor to close, or press Enter to finish an open path. Escape cancels the draft. **Pencil (Y)** records a freehand line and simplifies redundant samples. **Subselection (A)** edits anchors and control handles; moving an anchor carries adjacent handles. Fill, stroke and nonzero/even–odd rules are editable in Properties. Compound contours, curve bounds, transformed strokes and paths inside nested symbols share the HTML5 geometry model. General shape morphing, brush pressure, gradients and masks remain pending.

For object editing, **Selection (V)** moves objects; Shift-click toggles objects and dragging an empty area selects enclosed objects across visible, unlocked layers. Shift adds a marquee to the selection, **⌘A** selects all editable objects in the current frame, and Escape cancels a gesture or clears the selection. Duplicate, Delete and Undo work on the whole selection. **Free Transform (Q)** exposes resize and rotation handles: Shift preserves proportions when resizing or snaps rotation to 15°; Option resizes around the center. Multiple objects can be uniformly scaled or rotated together. Custom pivots, skew and nonuniform multi-object scaling remain planned.

**⌘C / ⌘X / ⌘V** copy, cut and paste selected objects, including nested symbols, their timelines and scripts. Paste inserts into the active editable layer at the center of the visible stage; **Edit → Paste in Place (⇧⌘V)** retains the original coordinates. Pasting into another document carries the required library symbols and creates fresh identities. Text fields and Actions keep their normal text clipboard behavior. See [clipboard workflows and boundaries](docs/CLIPBOARD.md).

**Window** switches Light/Dark, hides or restores panels, and detaches Tools, Timeline or Actions into separate macOS windows. **Dock** returns a panel; **Restore Workspace** restores the default layout. Appearance and layout preferences do not change your project. See [workspace controls](docs/WORKSPACE.md).

**Actions → Project files** stores reusable source code inside the project. Add or import source files, set the document class or a nested symbol's class, and use **Check project** for compiler feedback. Project programs support bounded public AS3 classes with typed fields, bound methods, getters/setters and project interfaces, and AS2 classes with erased annotations and shared prototypes. Both support exact imports and inheritance; unsupported constructs report diagnostics. Set an **Export identifier** on a MovieClip symbol to enable AS3 class construction from its library timeline or AS2 `attachMovie`. Projects with source files use document version 2. Older projects remain readable, and linked-symbol clipboard transfers preserve source files.

Example frame script:

```js
this.stop();
```

Example click handler on an instance:

```js
this.gotoAndPlay(1);
this.rotation += 30;
```

HTML5 export uses the same Rust timeline/runtime through WebAssembly and a small Canvas browser host. Published content needs no Flash Player, SWF or CDN. The generated WASM bytes are embedded in a local JavaScript bundle. JavaScript executes with the normal privileges of its containing browser page.

**Current text limitation:** the editor uses installed fonts, while HTML5 export references fonts by name. Font embedding/subsetting and consistent cross-platform text layout are not implemented yet; export reports this rather than silently promising identical typography.

For interactive web content, use the **Accessibility** panel to give an object a name, description, Button/Image role and reading/focus order. Buttons use the existing Click handler for keyboard activation. See [interactive HTML5 projects](docs/WEB.md) for authoring and preview controls, and the verification record for tested boundaries.

The [public FLA corpus](examples/fla-corpus/README.md) contains 15 licensed, pinned legacy source files and three original XFL/FLA examples. One unchanged Adobe CS6 FLA now imports as an editable project and publishes to HTML5 with its original 17-line reference trace; 14 other external files retain explicit refusal expectations. Try the [imported frame-label example](examples/fla-corpus/imported/ruffle-avm1-frame-labels.nflash). This verifies one bounded case, not general FLA compatibility.

Open [keyboard-controls.nflash](examples/keyboard-controls.nflash) for two independent AS3 counter instances, [script-recovery.nflash](examples/script-recovery.nflash) for AS2/AS3 exceptions and cleanup on animated instances, or [vector-paths.nflash](examples/vector-paths.nflash) for curved strokes and compound fills. HTML5 previews provide Fit, 100% and fixed zoom levels with scrolling and a Pan tool.

[visibility.nflash](examples/visibility.nflash) exercises hidden AS2/AS3 clips that keep running, [symbol-replacement.nflash](examples/symbol-replacement.nflash) checks fresh nested state after replacement, and [as-intrinsics.nflash](examples/as-intrinsics.nflash) demonstrates the documented `parseInt`/`toFixed` profiles. These are bounded compatibility examples, not complete Flash API support.

[project-program.nflash](examples/project-program.nflash) combines a document class, two linked components, shared state and input events. [display-family.nflash](examples/display-family.nflash) covers child traversal, geometry and AS2 callbacks; [events-stage.nflash](examples/events-stage.nflash) demonstrates propagation and Stage input. Compiler/host checks and visual acceptance are recorded separately in [VERIFICATION.md](docs/VERIFICATION.md).

[as2-project-input.nflash](examples/as2-project-input.nflash) combines inherited AS2 MovieClips, independent instance counters, shared class state and Key/Mouse/Stage listeners. Left/Right activate the cards, Space resets them, and H/S hide or show the stage cursor. AS2 root document linkage is a NeoFlash extension.

[collections-text.nflash](examples/collections-text.nflash) processes the same leaderboard in AS2 and AS3: parse rows, update records, sort with Flash array flags and display the result in authored text fields. The shared Array/String profile also works in project classes. Plain text mutation, color, line queries and Canvas-based measurements are available; rich text, editable input, wrapping and embedded fonts remain separate work.

[geometry-timer.nflash](examples/geometry-timer.nflash) combines Point/Rectangle/Matrix with Timer events: four ticks move an authored marker, query typed bounds and update text. The timer service owns its callbacks for the publication session and clears them on Restart. Geometry uses a finite 2D profile; browser timer throttling still applies.

[dynamic-display.nflash](examples/dynamic-display.nflash) creates Sprite/Shape components from code, draws Graphics, moves a Card subclass between containers and detaches/reattaches it without losing fields or listeners. Click the card, then use Right / Space / Up; the purple triangle uses AS2 creation and drawing. These runtime changes do not alter the authored document. Editing authored display-list ownership remains separate work.

[linked-movieclips.nflash](examples/linked-movieclips.nflash) creates two AS3 instances from one exported library MovieClip and a third with AS2 `attachMovie`. Each keeps its own frame controls, nested animation, fields and script targets. Click the cards, use Right to step frames, Space to detach/reattach the middle instance and Up to use frame labels. Explicit export identifiers and required program/library snapshots survive cross-document clipboard transfer.

[interfaces-accessors.nflash](examples/interfaces-accessors.nflash) uses project interfaces, inherited getter/setter properties and two independent typed models. Click the stage to increase the bounded model; Restart restores instance and static state.

[binary-data.nflash](examples/binary-data.nflash) writes, reads and copies a typed binary/UTF packet with ByteArray/Endian and displays the decoded values. Buffers are bounded and session-owned; AMF, compression and other code pages remain unsupported.

Prepare these and representative existing examples as one browser batch using an already built exporter:

```sh
python3 scripts/prepare-browser-batch.py --exporter target/debug/neoflash-export --runtime target/web-runtime --manifest examples/browser-batch.json
python3 -m http.server 8000 --directory dist/acceptance
```

Open `http://localhost:8000` to review the batch. Export success, runtime readiness and observed behavior have separate statuses; an export failure does not prevent the remaining examples from being prepared.

## Project layout

```text
crates/
  neoflash-core/     document, timeline, playback, validation, undo
  neoflash-editor/   macOS desktop workspace
  neoflash-script/   JS/TS compiler integration, initial AS frontend
  neoflash-web/      WebAssembly wrapper of the shared runtime
  neoflash-export/   standalone HTML5 publisher and CLI
  neoflash-import/   bounded legacy inspection and initial XFL importer
web/                browser host and runtime tests
examples/           small source examples
scripts/            reproducible build, check and packaging commands
docs/               requirements, architecture, decisions, roadmap, test record
vendor/             pinned native integration patches, provenance and licenses
```

There is one source tree. Version history belongs in Git, build outputs in ignored `target/` and `dist/`. User-provided `examples/old flash files/` stays local and is excluded from Git.

## Documentation

- [Product requirements](docs/PRODUCT.md)
- [Architecture](docs/ARCHITECTURE.md) and [decisions](docs/decisions/)
- [Roadmap](docs/ROADMAP.md) and [backlog](docs/BACKLOG.md)
- [Language compatibility](docs/COMPATIBILITY.md) and [legacy import](docs/IMPORTS.md)
- [Interactive HTML5 projects](docs/WEB.md)
- [Workspace controls](docs/WORKSPACE.md) and [clipboard workflows](docs/CLIPBOARD.md)
- [Verification record](docs/VERIFICATION.md)
- [Changes](CHANGELOG.md) and [contributing](CONTRIBUTING.md)

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option. Contributions are accepted under the same terms. Third-party dependencies and public corpus fixtures retain their respective licenses; fixture provenance and notices are recorded alongside their source files. This independent project does not include Adobe application code or branding assets.
