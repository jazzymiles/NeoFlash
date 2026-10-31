# NeoFlash — working agreement

- Work only within the latest human-authorized scope. Keep unrelated subsystems and existing monitor automations paused.
- Public documentation should describe user-visible behavior, current coverage, setup, architecture and known limits. Keep agent coordination, timeboxes, command logs and internal delivery chronology under ignored `target/`. Preserve necessary licenses and provenance. Do not create additional statistics pages; maintain the existing generated registry and summary.
- Before adding public documentation, update the existing topic instead of appending another delivery report. Do not commit hour/batch/experiment diaries, agent productivity comparisons, duplicate verification narratives or copies of coverage tables. Preserve concise reproducible validation boundaries and pinned audit/provenance evidence; keep raw observations in ignored `target/`. README is the product entry point, roadmap is future work, and changelog records user-visible changes.
- All authoring UI follows the NF2 design system. It is **locked**: see "Design system (NF2) — locked" below. Implementation work connects behaviour to the existing UI and never changes how it looks.
- Validate several related changes as one delivery batch. Recheck only a concrete affected failure; do not add per-method or per-small-edit verification cycles.

- Product: open-source Rust authoring environment for macOS 15+, inspired by classic Flash before Animate. HTML5 is the primary publishing target.
- User requirements include websites/apps, games and animation; AS2/AS3 plus JavaScript/TypeScript; nested timelines, instances, frame/object scripts; system fonts; eventual FLA/XFL/SWF import.
- Do not reduce the product goal to a video animator or silently redefine partial scripting support as full AS2/AS3 compatibility.
- Current user priority: breadth first. Bring the main subsystems to useful working partial support in large coherent batches, then deepen conformance. Do not let one method or rare edge case monopolize an iteration.
- Validate feature batches with representative integration/browser scenarios and one shared regression pass. Do not create a separate lengthy HTML5 acceptance cycle for each API. Record non-blocking failures for a later batch; only shared breakage, crashes or content/data loss block the whole delivery.
- Keep verification cost controlled: extend coherent family scenarios before adding new test cases, avoid duplicating the same behavior across many harnesses, and rerun the shared suite only for a delivery batch or a concrete regression. While the user is away or the screen is locked, continue compiler/runtime work in the background and leave UI-only acceptance explicitly pending.
- Project workflow checks collect all family failures in one pass. After a concrete failure, use `NEOFLASH_WORKFLOWS=id1,id2 node crates/neoflash-script/tests/project-execute.mjs` to rerun only affected families; the default still runs every family and its common prelude. Do not use a focused selection to claim a complete delivery check.
- Keep one source tree. Versions belong in Git tags/history, never in nested v1/v2 copies. No asset-folder chains. Build outputs go under ignored `target/` and `dist/`.
- Reuse compatible open-source algorithms and references after understanding and adapting them to the shared Rust core. Preserve licenses, attribution and pinned provenance; do not label adapted code as wholly original. Do not embed complete third-party ActionScript engines or introduce foreign-language implementations for new runtime behavior. Browser adapters should marshal data and connect browser facilities, without duplicating Rust algorithms.
- Downloaded source checkouts, archives, external tools and exploratory artifacts belong under ignored `target/`; generated exports belong under ignored `dist/`. Commit only deliberately adapted source, necessary license notices and authored project examples.
- `crates/` contains Rust modules, `web/` the browser host, `examples/` small source projects, `scripts/` build/check tasks, `docs/` product and engineering records.
- Keep the serialized document independent of UI, rendering backend and filesystem paths. Keep authored evaluation side-effect free for scrubbing/onion skin.
- Unsupported imports and script features must produce actionable diagnostics. Never silently drop content or code.
- Maintain README, roadmap, compatibility matrix, changelog and decisions as implementation changes.
- Verify model/runtime behavior with meaningful tests; verify UI by running it and inspect HTML5 output in a browser. Do not claim unperformed checks.
- Do not publish a repository, deployment or release unless requested. Local development, builds and tests are authorized.
- With implementation changes, update affected capability IDs/status/evidence in `docs/progress/*.json`, run `python3 scripts/progress.py --write-reports`, and keep the README summary and detailed ledger consistent. Percentages describe that registry, never complete Flash compatibility.
- Keep the progress dashboard, its capability registry and generated repository reports in English.
- AS2/AS3 coverage must use the complete declaration catalogue and per-entry audit in `docs/progress/actionscript-*.json`. Never use the supplementary 35/36 capability groups as the language coverage denominator. Preserve separate subset/partial/missing/needs-review counts and audit provenance.

## Design system (NF2) — locked

The NF2 design system is owned by the design track (the project owner with the Figma file `BUz3pseopiuOg2Uiy3Fog7`). Implementation agents, including Codex, use it and never change it.

**Forbidden (no exceptions, also not "small fixes"):**
- Editing anything under `interface/` (Figma export, tokens, icons, cursors, outlines, design docs, tools) or `crates/neoflash-ui/` (tokens, metrics, typography, icons, components, gallery), or `docs/DESIGN-SYSTEM.md`, or `interface/design-system/DESIGN-LOCK.json`.
- Changing the visual composition of an editor panel: which components a panel uses, their order and grouping, spacing, sizes, colours, fonts, icons, labels and tooltips, panel/dock layout and the workspace templates.
- Adding hex colours, font sizes, paddings or hand-styled egui widgets anywhere in the editor; adding a new visual role or component.
- Re-running the Figma export or doc builders, or editing the Figma file.

**Allowed:**
- Connecting behaviour to existing UI: enabling a control that is disabled as "Not available yet" once its feature works, wiring callbacks, binding document data, adding keyboard handling the spec already lists.
- Turning a planned preview (`crates/neoflash-editor/src/panels/planned_*.rs`) into a real panel by moving its body unchanged into a panel file and registering it.
- Fixing behaviour bugs inside panel files without altering their look.

**When the design is missing, unclear or blocks the work,** do not improvise: add an entry to `docs/NF2-REQUESTS.md` (what is needed, where, why, which reference/outline you checked, proposed component or state) and continue with the parts that do not need it. The design track answers the request and updates the design system.

**Rulers, guides, grid and snapping are finished features** — read `docs/NF2-RULERS-GUIDES-GRID.md` (data model, invariants, tests, "Do not change") before touching `stage_view.rs`, `snapping.rs`, `panels/view_settings.rs`, `Timeline::guides` or the stage event order in `stage.rs`. Keep `stage_view_tests.rs` passing.

**Checks:** `python3 scripts/design-lock.py` must pass (it is part of `scripts/check.sh`); it fails on any change to locked files. Functional scope and order of work: `docs/NF2-FUNCTIONAL-SPEC.md` and `docs/NF2-WIRING-PLAN.md`. Visual verification: `NEOFLASH_WINDOW=1920x1080 NEOFLASH_SCREENSHOT=target/ui-shots/x.png NEOFLASH_SCREENSHOT_THEME=dark|light [NEOFLASH_SCREENSHOT_WORKSPACE=developer] cargo run -p neoflash-editor`, compared with `interface/design-system/exports/` (local, not versioned).
