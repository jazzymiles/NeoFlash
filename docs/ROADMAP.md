# Roadmap

Full Flash replacement is the product objective. The stages below are independently reviewable deliveries; completion of one does not imply feature parity.

| Stage | Scope | Acceptance |
|---|---|---|
| M0 · foundation | Versioned document, symbols, deterministic evaluation, runtime, script/compiler boundaries, tests and repository | Shared Rust behavior is exercised natively and as real WASM |
| M1 · authoring preview | Classic desktop workspace; basic shapes/text; nested symbols; keys/easing/onion; actions; save/open; HTML5 test movie | User can create/edit/save/reopen and run an interactive nested animation |
| M2 · daily editing | Complete selection/transform tools, bezier/pen/brush, snapping, groups, layer folders/masks/guides, span and multi-frame operations, autosave | Real Flash workflows are reproduced using keyboard/mouse without workarounds |
| M3 · portable text and media | Font embedding/subsetting/outlines, text layout, bitmap/SVG/audio/video imports, media synchronization | Export has predictable appearance and media playback on clean machines |
| M4 · modern web | DOM components, accessible text/forms, responsive layout and breakpoints, browser API bridges, module imports | One project can publish an accessible responsive site/application and Canvas game/animation |
| M5 · language compatibility | Complete AS2/AS3 syntax, semantics, classes, namespaces/E4X, events and supported Flash APIs with conformance corpus | Each claimed feature has native/web tests and a documented result against references |
| M6 · legacy projects | XFL breadth; binary FLA reading; SWF AVM1/AVM2 playback and structured import strategy | Provided legacy corpus has repeatable per-file results with no silent loss |
| M7 · release quality | Performance, accessibility, macOS integration, packaging/notarization, crash recovery, migration and extension APIs | Signed tested releases and documented compatibility/performance budgets |

The initial commit addresses M0 and part of M1, with an initial importer and compatibility harness. Subsequent increments add pen/pencil/point editing, compound vector paths, cross-layer multi-selection and Free Transform, object clipboard transfer with nested symbols, authored Button/Image metadata, ActionScript functions and structured exceptions, and bounded binary FLA shape, Contents and typed timeline preservation. A separate strict solid-contour adapter produces core paths; complete binary documents remain blocked. M2–M7 are not complete. Order can overlap: language and legacy work continue while the editor matures. No arbitrary completion date is promised.
