# NeoFlash

Open-source authoring environment inspired by classic Macromedia/Adobe Flash, built in Rust for macOS 15+. The goal is one comfortable workspace for interactive websites, applications, games and animation, publishing to HTML5.

**Status: development preview, 0.1.0. This is a working foundation, not a complete Flash replacement.** Full AS2/AS3 compatibility, general FLA/SWF import, a complete vector toolset and DOM-based web layouts remain substantial work. See the [compatibility matrix](docs/COMPATIBILITY.md) and [roadmap](docs/ROADMAP.md).

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

Open [keyboard-controls.nflash](examples/keyboard-controls.nflash) for two independent AS3 counter instances, or [vector-paths.nflash](examples/vector-paths.nflash) for curved strokes and compound fills. HTML5 previews provide Fit, 100% and fixed zoom levels with scrolling and a Pan tool.

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
```

There is one source tree. Version history belongs in Git, build outputs in ignored `target/` and `dist/`. User-provided `examples/old flash files/` stays local and is excluded from Git.

## Documentation

- [Product requirements](docs/PRODUCT.md)
- [Architecture](docs/ARCHITECTURE.md) and [decisions](docs/decisions/0001-foundation.md)
- [Roadmap](docs/ROADMAP.md) and [backlog](docs/BACKLOG.md)
- [Language compatibility](docs/COMPATIBILITY.md) and [legacy import](docs/IMPORTS.md)
- [Interactive HTML5 projects](docs/WEB.md)
- [Verification record](docs/VERIFICATION.md)
- [Changes](CHANGELOG.md) and [contributing](CONTRIBUTING.md)

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option. Contributions are accepted under the same terms. Third-party dependencies retain their respective licenses. This independent project does not include Adobe application code or branding assets.
