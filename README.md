# NeoFlash

Open-source authoring environment inspired by classic Macromedia/Adobe Flash, built in Rust for macOS 15+. The goal is one comfortable workspace for interactive websites, applications, games and animation, publishing to HTML5.

**Status: development preview, 0.1.0. This is a working foundation, not a complete Flash replacement.** Full AS2/AS3 compatibility, general FLA/SWF import, a complete vector toolset and DOM-based web layouts remain substantial work. See the [compatibility matrix](docs/COMPATIBILITY.md) and [roadmap](docs/ROADMAP.md).

<!-- progress:start -->
## Where we are

The editor foundation works: nested timelines, vector tools, scripting and standalone HTML5. Full AS2/AS3, FLA/SWF import, portable media and DOM applications remain in development.

### ActionScript reference coverage

| Language | Documented entries | Implemented subset | Partial | Missing | Needs review |
| --- | ---: | ---: | ---: | ---: | ---: |
| AS2 | 2,307 | 15 | 340 | 1,843 | 109 |
| AS3 | 43,740 | 15 | 544 | 42,963 | 218 |

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

**Text (T)** creates a text box with a separate width and height, word wrap and multiline content. Properties edits its installed font, bold/italic style, size, color, leading and alignment. Resizing the box reflows the text; scaling the object transforms it. Older unboxed text keeps its appearance and can be converted with **Use text box**. AS3 TextField/TextFormat and AS2 createTextField use the shared Rust layout in HTML5. Browser fonts still depend on the visitor’s installed fonts or fallback; embedding and mixed formatting remain planned. HTML5 input, focus and selection have the separate scripted text-form profile.

The independent `FORMS-HTML5-20261027T0811Z` check of [text-form.nflash](examples/text-form.nflash) confirms real typing/paste, bounded restriction and length limits, focus/selection, cancellable input, and saved values across reload/Restart. Restart also discards the observed unsaved edit and removes its editing control. Password/selectable behavior, IME, arbitrary transformed/clipped editing, full accessibility and native authoring still need acceptance; see [the verification record](docs/VERIFICATION.md#independent-forms-verification-of-c10e19e).

For vector drawing, choose **Pen (P)**: click for corners, drag for Bézier handles, click the first anchor to close, or press Enter to finish an open path. Escape cancels the draft. **Pencil (Y)** records a freehand line and simplifies redundant samples. **Subselection (A)** edits anchors and control handles; moving an anchor carries adjacent handles. Fill, stroke and nonzero/even–odd rules are editable in Properties. Compound contours, curve bounds, transformed strokes and paths inside nested symbols share the HTML5 geometry model. General shape morphing, brush pressure, native gradient/mask tools remain pending.

For object editing, **Selection (V)** moves objects; Shift-click toggles objects and dragging an empty area selects enclosed objects across visible, unlocked layers. Shift adds a marquee to the selection, **⌘A** selects all editable objects in the current frame, and Escape cancels a gesture or clears the selection. Duplicate, Delete and Undo work on the whole selection. **Free Transform (Q)** exposes resize and rotation handles: Shift preserves proportions when resizing or snaps rotation to 15°; Option resizes around the center. Multiple objects can be uniformly scaled or rotated together. Custom pivots, skew and nonuniform multi-object scaling remain planned.

**⌘C / ⌘X / ⌘V** copy, cut and paste selected objects, including nested symbols, their timelines and scripts. Paste inserts into the active editable layer at the center of the visible stage; **Edit → Paste in Place (⇧⌘V)** retains the original coordinates. Pasting into another document carries the required library symbols and creates fresh identities. Text fields and Actions keep their normal text clipboard behavior. See [clipboard workflows and boundaries](docs/CLIPBOARD.md).

**Window** switches Light/Dark, hides or restores panels, and detaches Tools, Timeline or Actions into separate macOS windows. **Dock** returns a panel; **Restore Workspace** restores the default layout. Appearance and layout preferences do not change your project. See [workspace controls](docs/WORKSPACE.md).

**Actions → Project files** stores reusable source code inside the project. Add or import source files, set the document class or a nested symbol's class, and use **Check project** for compiler feedback. Project programs support bounded AS3 classes with typed fields, bound methods, getters/setters, interfaces and public/private/protected/internal access. Final and dynamic declarations, exact/wildcard imports and same-package collaboration share the project linker. AS2 keeps erased annotations, shared prototypes and its distinct compile-time private profile; unsupported constructs report diagnostics. Set an **Export identifier** on a MovieClip symbol to enable AS3 class construction from its library timeline or AS2 `attachMovie`. Projects with source files use document version 2. Older projects remain readable, and linked-symbol clipboard transfers preserve source files.

**Library → Resource files** imports text, data and media bytes into the project. Use the displayed project path in scripts, edit its content type/path, or remove it with Undo. Resource documents use version 3 so older editors refuse them safely; versions 1 and 2 remain readable. Embedded files travel with HTML5 publication and work offline. AS3 URLLoader/URLVariables and AS2 LoadVars also support bounded browser HTTP(S) data requests. Select a PNG/JPEG resource and choose **Place image** to add it to the stage; Properties controls smoothing. AS3 Bitmap/BitmapData and AS2 BitmapData/attachBitmap share bounded Rust pixel buffers in HTML5. AS3 Sound/SoundChannel/SoundTransform and AS2 Sound provide complete-file MP3/WAV loading, playback, repeats, volume/pan and completion callbacks in HTML5. A browser gesture enables playback; timeline audio, sound streaming and font embedding remain planned. Video has the separate file-playback profile below.

**Display transforms:** AS2/AS3 Transform and ColorTransform connect exact 2D matrices and nested RGBA adjustments to the shared Rust scene. Linked objects retain skew through scalar edits/reparenting; BitmapData views can have independent tints. Native authoring skew and 3D remain planned.

**Video:** Insert → Video placeholder adds an authored viewport; AS3 can also create Video at runtime. NetConnection(null)/NetStream connect embedded media and browser HTTP(S) playback to the same display graph, with pause, seek, resume and publication cleanup. AS2 uses an authored Video with attachVideo. Native authoring shows an explicit placeholder; actual decoding happens in HTML5. RTMP, camera capture, Flash FLV codecs and native timeline synchronization remain separate work.

**Data in scripts:** Date works in AS2 and AS3; AS3 adds JSON and constructor-based RegExp with String matching/replacement. The data-processing example loads embedded JSON, computes UTC dates and serializes a project class including its public getter. These are bounded modern browser profiles; historical Flash parser/locale/regex parity remains unfinished.

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

[collections-text.nflash](examples/collections-text.nflash) processes the same leaderboard in AS2 and AS3: parse rows, update records, sort with Flash array flags and display the result in authored text fields. The shared Array/String profile also works in project classes. Plain text mutation, color, line queries and Canvas-based measurements are available; dynamic wrapping and uniform formatting have their own text-layout profile; rich text and embedded fonts remain separate work; editable input has the text-form profile.

[geometry-timer.nflash](examples/geometry-timer.nflash) combines Point/Rectangle/Matrix with Timer events: four ticks move an authored marker, query typed bounds and update text. The timer service owns its callbacks for the publication session and clears them on Restart. Geometry uses a finite 2D profile; browser timer throttling still applies.

[dynamic-display.nflash](examples/dynamic-display.nflash) creates Sprite/Shape components from code with linear/radial gradient fills and strokes, rounded/cubic paths and independently copied Graphics. The Card changes palette on click, moves between containers and detaches/reattaches without losing fields or listeners. Click the card, then use Right / Space / Up; the purple triangle uses AS2 gradient drawing and percentage alpha. These runtime changes do not alter the authored document. Editing authored display-list ownership remains separate work.

[clipped-scene.nflash](examples/clipped-scene.nflash) combines scrolling linked cards, compound vector masks, clipped pointer targets and AS2 setMask/scrollRect. Advance moves the view and masks, reveals the full panels, then restores them. Runtime state preserves the same card instances; native mask and scroll-rectangle tools remain planned.

[linked-movieclips.nflash](examples/linked-movieclips.nflash) creates two AS3 instances from one exported library MovieClip and a third with AS2 `attachMovie`. Each keeps its own frame controls, nested animation, fields and script targets. Click the cards, use Right to step frames, Space to detach/reattach the middle instance and Up to use frame labels. Explicit export identifiers and required program/library snapshots survive cross-document clipboard transfer.

[interfaces-accessors.nflash](examples/interfaces-accessors.nflash) uses project interfaces, inherited getter/setter properties and two independent typed models. Click the stage to increase the bounded model; Restart restores instance and static state.

[loader-gallery.nflash](examples/loader-gallery.nflash) combines AS3 Loader/LoaderInfo, ByteArray image loading and AS2 MovieClipLoader replacement. Cycle swaps, unloads and restores the gallery. PNG/JPEG use the shared Rust decoder; SWF/GIF loading and complete legacy loading semantics remain planned.

[resource-loading.nflash](examples/resource-loading.nflash) loads embedded text, repeated form fields and a 50-byte binary packet through AS3 URLLoader and AS2 LoadVars, handles an absent file and cancels a load. When served over HTTP, **Load HTTP notes** reads the published README and displays the response status/size. Restart repeats the portable pipeline.

[image-pipeline.nflash](examples/image-pipeline.nflash) combines independent color/filter edits, nested source capture, bulk ARGB bytes and PNG encoding loaded back into the scene. Palette changes repeat the pipeline while the original pixels stay unchanged; Restart creates fresh owners. Independent `IMAGE-PIPELINE-HTML5-20261027T0943Z` verifies complete initial source/tint pixels and captured/roundtrip/loaded RGBA, plus the observed partial-write EOF contract. Later palette/Restart phases have screenshot/AX evidence; complete later console/PNG traces were unavailable. See [the verification record](docs/VERIFICATION.md).

[class-packages.nflash](examples/class-packages.nflash) connects separate private state in base/derived models, protected virtual dispatch, internal package collaborators and public JSON. A dynamic metadata model accepts extra keys while its sealed subclass refuses them; the AS2 component retains inherited private access. Independent `CLASSES-HTML5-20261027T1007Z` verifies real updates with an untouched control, private-state isolation, public JSON/dynamic keys, bounded ReferenceError refusals and Restart on frozen 6687ced. Final and wildcard declarations have positive-use evidence only; full reflection and initialization semantics remain open. See [the independent record](docs/VERIFICATION.md#independent-classpackage-verification-of-6687ced).

[function-workflow.nflash](examples/function-workflow.nflash) connects reusable AS3 commands with optional/rest parameters and bound callbacks, alongside AS2 dynamic receivers and arguments. The two dialects update independent visible models; Restart recreates their state. Independent `FUNCTIONS-HTML5-20261027T0914Z` confirms the corrected command workflow, omitted/undefined argument distinction, tested call/apply receivers, arity refusals and fresh Restart state. Its original instrumentation-altered probe remains separate; see [the verification record](docs/VERIFICATION.md).

[filter-gallery.nflash](examples/filter-gallery.nflash) applies Blur, Glow, DropShadow and ColorMatrix to complete vector/text/bitmap subtrees, including nested filters and an AS2 strip. Cycle changes styles, clears effects and restores them while retaining the same objects and shared source pixels. Independent `FILTERS-HTML5-20261027T0844Z` confirms the representative visible styles, copied settings, unchanged source pixels, observed source hits/halo exclusion and Restart. Filtered editing, mask/scroll combinations, native effect authoring and exact Flash raster fidelity remain unverified; see [the verification record](docs/VERIFICATION.md).

[text-form.nflash](examples/text-form.nflash) combines native browser text entry, UTF-16 selection, user-only input filters/length limits, AS3 events, AS2 Selection and a saved profile. Input controls follow stage transforms and vector clips; browser text shaping and the active DOM control’s foreground layering remain explicit limitations.

[saved-game.nflash](examples/saved-game.nflash) saves scores through AS3/AS2 SharedObject and round-trips cyclic object graphs through ByteArray AMF0/AMF3. Save, reload or Restart, then clear only the example’s named saves. Browser persistence requires an HTTP(S) origin with storage enabled.

[binary-data.nflash](examples/binary-data.nflash) writes, reads and copies a typed binary/UTF packet with ByteArray/Endian and displays the decoded values. Buffers are bounded and session-owned; AMF0/AMF3 object records are also supported, while compression and other code pages remain separate work.

[typed-collections.nflash](examples/typed-collections.nflash) connects Vector models with Dictionary keys that retain linked-card identity. Process a batch to update stock, reuse a card and regenerate JSON through public model getters. An AS2 record loop runs alongside the AS3 workflow. Typed collections and enumeration have bounded partial support; XML/E4X and full historical coercion remain pending.

[xml-catalog.nflash](examples/xml-catalog.nflash) loads a shared portable catalog through AS2 XML.load and AS3 URLLoader. It compares legacy DOM moves with AS3 E4X selectors and editable XMLList targets, constructs XML/XMLList literals with escaped interpolation and serializes namespace-aware trees. Shared Rust parsing and bounded target/literal semantics are implemented; filters, scoped defaults and full E4X equality remain unfinished.

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

[video-playback.nflash](examples/video-playback.nflash) combines AS3-created and AS2-authored Video views with an original silent four-second clip. Play/Pause/Seek/Resume/Close operate both views; the optional HTTP control expects `/video-fixture.mp4` on the hosting server. Both default sources are embedded and portable.
