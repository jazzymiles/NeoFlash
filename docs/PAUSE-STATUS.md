# Paused checkpoint — 27 October 2026

The project owner requested a pause until an explicit resume command and publication of the current progress. Development agents have stopped. This Git branch preserves unfinished work; it is not a completed delivery.

## Last completed delivery

`main` at `51e0f98b09f3e3f5d3a8a8d462ef61ddc55ef164` contains AS2/AS3 web-page integration. GitHub Check run 40496417366 and Progress Pages run 40496417290 both succeeded. Its shared suite, ordinary publication, real browser callbacks, native navigation and HTTP receipts are recorded in VERIFICATION.md. The existing packaged application and WASM artifacts belong to that delivery.

The checkpoint also preserves subsequent independent pointer/web-integration audit records and exact declaration-catalogue updates. Those records identify their own frozen revisions; they do not validate the unfinished TextField changes.

## Saved unfinished work

- Rust text state/layout: automatic sizing, affine anchor updates, scroll metrics, decorations, compact glyph/paragraph geometry and measured mutation APIs.
- Browser host: matching TextField properties/queries, AS2 listener notifications, decoration painting, input-overlay sizing and scrolling.
- Compiler: enum/metrics registration, native member handling, AS2 autoSize conversion, and extensions to existing grouped tests.
- One original `examples/text-viewport.nflash` reader workflow, its actual-WASM helper and registration in the family runner.
- Native text-cache accounting and documentation/registry descriptions, explicitly marked unverified.

No further feature work or test runs were started after the pause request. Publication housekeeping preserves the source and this handoff.

## Verification state and first resume actions

Core/WASM `cargo check -p neoflash-web --locked` passed before the last small source/test changes. The focused existing text_field group did not pass: an initial fixture mixed current and default font sizes, which was corrected; the latest run stops at an old whole-TextLayout equality assertion whose new viewport heights differ (80 versus 100). Inspect that assertion and preserve its intended behavior checks when adapting it. This is a recorded failing checkpoint, not a passing test result.

The core owner also identified unfinished background/border behavior for legacy unboxed Text: a virtual decorated viewport was agreed but not implemented. Review this alongside the distinction between unboxed text and automatic-size conversion.

The five changed browser modules passed syntax checks and diff checks. The new helper passed its JavaScript syntax check only. Compiler tests/Clippy, ordinary export, the new actual-WASM workflow, the shared delivery pass and browser acceptance have not run for this package. No new accepted application bundle was produced.

After an explicit resume command: finish the known integration points, rebuild WASM, run the single new coherent helper and only the affected preparation groups, then one shared delivery pass. The planned browser batch contains text-viewport, text-layout, text-form, web-integration and drag-drop. Keep test count controlled; do not start a separate browser cycle for each method.

ADR 0034 records the intended profile, including approximate character geometry and a known horizontal-scroll query difference from historical Flash 32. Native viewport authoring UI, full typography and original-player parity remain outside this unfinished increment.

The private old Flash corpus and independently maintained raw reference directories remain local and are not part of this publication. Resume in this checkpoint branch, preserving the one-source-tree structure and existing unsaved native document.
