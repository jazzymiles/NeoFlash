# TextField viewport, automatic sizing and geometry

Status: paused in implementation at the owner’s request, 2026-10-27. Integration and browser acceptance are pending; see ../PAUSE-STATUS.md.

## One layout serves text applications

The existing uniform text engine becomes a document viewport: content can determine its box size, scroll within a fixed box, expose line/character geometry and paint its own background and border. These operations belong to one family because a reader, chat panel or property inspector needs them together. AS2 aliases and callbacks share the same model as AS3 while retaining their own source contracts.

Rust owns text, UTF-16 positions, line layout, viewport state and validated mutations. Frontends supply installed-font measurements through the existing batched measurement boundary. Browser Canvas painting, display bounds/picking, geometry queries and native input overlays must consume compatible coordinates. Automatic sizing must preserve its chosen horizontal anchor through shared display state; it must not merely move painted glyphs while leaving script transforms or hit areas behind.

Scrolling changes the viewport, not the underlying text or its UTF-16 indexing. Keep requested and effective values coherent after content, formatting and box changes. Decorations remain part of the ordinary display painter and color/clip pipeline. A copied geometry result must not become a mutable handle into layout state. Native textarea scroll events must not create a feedback loop with script scrolling or publication reset.

The initial point/character query profile consistently uses viewport-local coordinates with both scroll offsets. Pinned [Ruffle edit_text.rs](https://github.com/ruffle-rs/ruffle/blob/fe6cfc545405b888ebb478ce1c211d63147ca49b/core/src/display_object/edit_text.rs#L2416) records a Flash 32 asymmetry that reverses horizontal scrolling for character bounds and ignores it in character picking. That historical behavior is an explicit compatibility gap; passing the NeoFlash viewport workflow does not close it.

## Bounded scope and validation

This increment extends uniform system-font text and existing authored/runtime identities. HTML/CSS text, mixed-format runs, embedded fonts, full shaping and original Flash font-engine raster equivalence remain separate work. New runtime state defaults must preserve existing document behavior; do not silently reinterpret saved projects or add an unnecessary document-format revision. Authored viewport controls are a separate UI contract unless explicitly implemented and checked.

Reuse existing grouped Rust/compiler tests, add one combined ordinary-export/actual-WASM reader example and run one shared delivery pass. Real browser actions cover scrolling, resizing/anchor behavior, geometry-driven feedback and Restart. Synthetic text measurements establish ownership and arithmetic, not exact native glyph metrics. Report unverified browser/native states directly and move on to the next family after useful partial support.

Compatibility targets: Adobe/AIR [TextField](https://airsdk.dev/reference/actionscript/3.0/flash/text/TextField.html), [TextFieldAutoSize](https://airsdk.dev/reference/actionscript/3.0/flash/text/TextFieldAutoSize.html), [TextLineMetrics](https://airsdk.dev/reference/actionscript/3.0/flash/text/TextLineMetrics.html), and Adobe's [AS2 TextField reference mirror](https://open-flash.github.io/mirrors/as2-language-reference/TextField.html). Implementation choices and observed behavior retain separate evidence.
