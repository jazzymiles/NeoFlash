# NF2 wiring plan — connecting functionality to the existing UI

Readers: implementation agents (Codex) and the project owner. This page orders the work that turns disabled NF2
controls and planned previews into working features. It covers the **UI ↔ editor ↔ engine connections** only.

- What each element must do: [NF2-FUNCTIONAL-SPEC.md](NF2-FUNCTIONAL-SPEC.md) (IDs such as `TL-02`, `PP-09` below
  refer to it).
- Engine design behind each block (model, evaluation, runtime, export, script, algorithms):
  [NF2-ENGINE-PLAN.md](NF2-ENGINE-PLAN.md) (blocks `E-…`).
- Design gaps: [NF2-REQUESTS.md](NF2-REQUESTS.md).

Every planned preview already has its entry point (menu item, button). When a preview becomes a real panel, the same
entry point opens the real panel. Status markers in §5: **done** = works as specified (remaining limits are listed in
§7.4 or the spec entry), **partial** = part works, unmarked = not started.

## 1. Ground rules

1. **Never change visuals.** No edits under `interface/`, `crates/neoflash-ui/`, `docs/DESIGN-SYSTEM.md` or
   `interface/design-system/DESIGN-LOCK.json`. No new colours, sizes, paddings, fonts, icons, labels, tooltips or
   hand-styled egui widgets. `python3 scripts/design-lock.py` (part of `scripts/check.sh`) must pass.
2. **Reuse what exists.** Use the NF2 components already in the panel and the panel's own file. The only text change
   allowed when enabling a control is removing its "— not available yet" / "Not available yet" suffix or disabled
   hover text.
3. **Enable in place.** A control that is `ui.add_enabled(false, …)` / `nf::not_available(…)` becomes enabled (or
   conditionally enabled by real state) once its feature works — same component, same position.
4. **Convert previews in place.** A planned preview becomes real by moving its body function from
   `panels/planned_*.rs` into a real panel file (e.g. `panels/history.rs`), replacing fixed sample values with
   document/editor data, keeping its layout and component calls unchanged. Panels get a `PanelId`; dialogs get a
   dialog host (see §3). Remove the entry from `PlannedPanel` (`ALL`, `title`, `reference`, `chrome`, `width`) and its
   body dispatch in `planned.rs::body` in the same change.
5. **Missing design → request, not improvisation.** If a feature needs a component, state, row or text the design
   lacks (e.g. stage ruler painter), add `R-nnn` to `docs/NF2-REQUESTS.md` and continue with the parts that do not
   depend on it. Requests known in advance are listed in §4.
6. **AGENTS.md applies to every item:** meaningful model/runtime tests; UI verified by running it (screenshot mode,
   both themes) and HTML5 output inspected in a browser; `docs/progress/*.json` capability status/evidence updated and
   `python3 scripts/progress.py --write-reports` run; CHANGELOG (user-visible changes), README/ROADMAP/COMPATIBILITY
   kept consistent; unsupported cases produce actionable diagnostics, never silent drops. Validate several items as
   one delivery batch with one shared regression pass (`scripts/check.sh`); rerun only concrete failures.
7. **Document independence.** New document data lives in `neoflash-core` model types (serde, format version bump,
   validation) and never contains UI state, render backend data or filesystem paths. View preferences live in
   `WorkspacePrefs` / eframe storage.
8. **Undo.** Every document edit is one `Editor::transact` (or one property-edit session); previews and drafts never
   touch `History`. Every history commit ends in `Editor::commit_history` (`timeline_commands.rs`), which records
   motion ◆ property keys, prunes stale motion-guide links and raises the format version; never call
   `history.commit` directly (see §7.2).

## 2. Phases at a glance

| Phase | Theme | Engine work | Items |
| --- | --- | --- | --- |
| 0 | UI/editor-only wiring | none | W0.1–W0.16 |
| 1 | Small engine extensions | additive model fields, small APIs | W1.1–W1.12 |
| 2 | Major authoring/runtime features | new subsystems | W2.1–W2.10 |
| 3 | Import/export, media pipelines, remaining preferences, specialised tools | new codecs/writers/services | W3.1–W3.10 |

Total: 48 work items. Within a phase, items are independent unless the dependency list (§6) says otherwise; batch
related items into one delivery (AGENTS.md).

## 3. Shared procedures

### 3.1 Enabling a disabled control

1. Locate the `add_enabled(false, …)` / `not_available(…)` call (spec gives file + function).
2. Replace `false` with the real condition (e.g. `self.layer_editable() && in_tween`), wire `.clicked()` to an
   `Editor` method; keep the component and label (drop only the "not available" suffix).
3. Add the keyboard shortcut only if the spec/reference lists it and it is free (check existing handlers such as
   `shell.rs::view_shortcuts`).
4. Test: an editor unit test in the crate's existing test modules (`panel_tests.rs`, `workspace_editor_tests.rs`,
   `selection_tests.rs`, …) that drives the method and checks document state + Undo.

### 3.2 Turning a planned panel into a real panel

1. Create `panels/<name>.rs`; move the body function from its `planned_*.rs` file unchanged, then
   bind values (`num`, `select`, `check` helpers from `planned.rs` are preview helpers that render disabled sample
   values — replace each with the same NF2 component bound to real data; do not change component type or size).
2. Add a `PanelId` variant in `workspace.rs` and every exhaustive match (title, visibility/size fields,
   default placement, floating sizes — today ~17 sites per panel, grep `PanelId::Color`), dispatch in
   `dock_grid.rs` panel contents, Window menu entry, accessible names. New panels default to hidden/floating so the
   NF2 default workspace (`neoflash.workspace.v2`) is unchanged; saved prefs without the new field must deserialize
   (serde default) — **verify** with a load test of an old prefs blob.
3. Remove the preview from `PlannedPanel` and update `planned.rs` tests (`every_preview_paints_in_both_themes`,
   `previews_leave_the_project_unchanged`).
4. Screenshot the panel in Dark and Light and compare with its reference PNG (`PlannedPanel::reference`).

### 3.3 Turning a planned dialog into a real dialog

Same as 3.2 but hosted as a modal dialog (`Surface::Dialog` + `DialogHeader` + `scrim` + `dialog_footer`, as
`symbol_dialog.rs` / `app.rs::draw_dialogs` do). Draft state lives in the editor (e.g. `Option<DocumentSettingsDraft>`);
Esc = Cancel, Enter = primary; OK applies as one transaction. Non-modal preview chrome is replaced by modal behaviour —
if the preview frame differs from the modal frame in any visible way, file a request.

### 3.4 Work item checklist (copy per item into the PR/commit description)

```markdown
### Wn.m · <title>
- [ ] Spec IDs: <TL-xx, PP-xx …> · Engine block: <E-xx or none>
- [ ] UI files touched (no visual change): <panel file › function>
- [ ] Engine files touched: <crate/module>  ·  New API: <names>
- [ ] Format version / migration: <none | bumped to N, old files load unchanged (test)>
- [ ] Tests added: <unit/integration/project-execute family/browser scenario>
- [ ] Diagnostics for unsupported cases: <messages>
- [ ] Undo: one transaction per action (test)
- [ ] Light + Dark screenshots compared with <reference PNG>; keyboard path checked
- [ ] HTML5 output checked in a browser (if exported behaviour changed)
- [ ] docs/progress/<file>.json capability <id> updated; `python3 scripts/progress.py --write-reports`
- [ ] CHANGELOG / README / ROADMAP / COMPATIBILITY / WORKSPACE updated as relevant
- [ ] `python3 scripts/design-lock.py` passes; NF2-REQUESTS filed: <R-nnn or none>
- [ ] Spec entry status updated in NF2-FUNCTIONAL-SPEC.md summary (disabled → done)
```

## 4. Design requests to file before starting (R-numbers assigned when filed)

| Needed by | Request |
| --- | --- |
| W0.4 rulers/grid, W1.2 guides | Resolved: painters exist in `neoflash-ui/components/canvas.rs`; open follow-ups R-001 (ruler pointer marker), R-002 (exact guide position), R-004. |
| W1.3 snapping | Resolved: `paint_snap_ring`; open follow-up R-003 (Snapping submenu, object/pixel snapping). |
| W1.6 gradients, W0.9 colour, W3.4 envelope | Resolved: `NF2/Gradient Bar` + `Gradient Stop`, `NF2/Color Picker`, `NF2/Anchor Point` + `EnvelopeLane` exist in `neoflash-ui/components/{gradient, color_picker, envelope}.rs`; request only what is still missing. |
| W2.1 motion path | Resolved (R-005): `NF2/Motion Path` painter in `components/canvas.rs`. R-006 Paste motion special dialog resolved (design ready). |
| W0.8 toasts | R-044 resolved and host connected: canvas-bottom stack, severity timers, modal delay and Output actions. General Open/import/export result routing remains partial. |
| W0.10 swatches chip | Approval to replace the egui RGB picker in the Tools fill/stroke popover with `SwatchesPopover` (changes the component used). |
| W0.2 frame view | Resolved: Placement, Preview / Preview in context, Help and Close panel rows are in the frame-view ⋯ menu; Help and global F1 open the searchable Shortcuts command catalogue. |
| W0.15 menus | Confirm order/grouping for absent §8 menu items and View ▸ Grid/Guides/Snapping submenus. |
| v2.2 tool/command/brush panels | R-038 flyouts, Brushes, Brush/Pencil controls and Customize are connected; the compact 36 pt TypeHeader still needs a shared API (current component is 48 pt). Commands awaits a `.nfsl` execution contract. R-022 Trace bitmap is connected; Bitmap properties remains unavailable. |
| W2.x canvas chrome | Painters exist and are used for selection/transform boxes, anchors and marquee; open R-011 (rotated box, contrast over artwork, Light transformation point) and a gradient-transform gesture model. |
| W1.1 History | Visual state for "steps after current" (dimmed?) in the History list. |
| W3.6 Preferences | Accent colour / language / custom syntax colours imply new token behaviour — confirm or keep disabled. |

## 5. Work items

Format: **UI** → panel file/function · **Engine** → crate/module · **API** (NEW = proposal) · **Tests** · **Registry**
(id in `docs/progress/product.json`) · **Acceptance** (short; full criteria in the spec).

**Drawing follow-ups (TO-07)** — **partial**. Brush has eight tip shapes, all five paint modes, document-unit Size
1–200, zoom-following cursor/⌥⌘ resize, smoothing, pressure size control, tilt-angle toggle, gradient Lock fill and
Round/Flat/Chisel plus custom user presets. Pencil has independent Straighten/Smooth/Ink modes and smoothing strength.
Customize uses draft Add/Remove/reorder/Restore/Cancel/OK and persists global tool groups outside named layouts.
Shared flyout rows preserve the remembered tool; gestures and panel actions keep the chosen tool active. The compact
type-header API remains a design-system gap; artistic/pattern brushes, advanced Pencil stroke options and predictive
smoothing preview remain open. Paint Bucket fills bounded faces formed by one or several contours under an explicit
complexity budget; Ink Bottle can stroke transparent contours. Eraser defaults to cutting raw Merge fills and Bézier
strokes; Lasso and marquee split partial raw Merge geometry so deletion removes only the selected fragment. Object
Drawing, masked and combined-effect cases keep their conservative refusal paths. Tests: core Merge/authoring geometry,
editor `tablet_input`, `selection_tests`, `vector_gesture_tests` and `tool_workflow_tests`.

**Documents and named workspaces** — connected under R-035/R-036. New/Open add tabs; repeat Open activates the
existing tab. Next/Previous and Close/Close others/Close all work from document chrome and menus. Each document
preserves selection/caret identity, view, Undo and recovery; dirty Close/Quit walks tabs in order and defers Discard
until the sequence succeeds. The last close leaves an empty state with New/Open. Named layouts expose Save,
explicit Replace/Update, Reset, collision-safe Rename and Delete; they preserve panel visibility, active tabs,
dock sizes and floating/native windows, migrate old layouts and keep document drafts. Layout presets do not
restore document-tab sessions. Tests: `document_sessions_tests`, document-strip `panel_tests`,
`workspace_editor_tests` and recovery families.

**Trace bitmap (R-022)** — connected in `panels/trace_bitmap.rs`, editor `bitmap_trace.rs` and core `bitmap_trace.rs`.
Colour/palette presets, path/corner/noise controls, fill/stroke and Ignore white feed a cancellable background
preview and full-image trace. Success preserves placement/tool and groups multicolour vectors in one Undo;
Cancel/errors leave the document unchanged. Bounded sources/palettes/contours and unsupported effects produce
explicit diagnostics; the separate Bitmap properties dialog is excluded. Core/editor bitmap-trace families cover
holes/islands/alpha, controls, cancellation, stale targets, transforms and one Undo.

### Phase 0 — UI/editor-only (no engine change)

**W0.1 · Loop playback (TL-02, Control ▸ Loop playback)** — **done** (§7). `WorkspacePrefs.timeline.loop_playback`;
end-of-timeline handling in `Editor::after_playback_tick` (`timeline_commands.rs`). A shared `AuthoredPlayback` option
for Test Movie is W1.12.

**W0.2 · Frame view: Short rows + Snap playhead (TL-09, TL-11)** — **done** (§7). `TimelineView { short_rows,
snap_playhead }`; row height from `TimelineView::row_height()`; `Editor::snap_scrub_frame`.

**W0.3 · Ease panel, single curve (PP-01, TL-06, PR-11)** — **done** (§7). `panels/ease.rs` edits the ease of the
span under the playhead as a draft (egui temp memory) and applies it with one Undo; the Properties Tween section
accepts Y1/Y2 in −100…100 and a Flash acceleration field. Bounce/Elastic presets and per-property curves are also done
(W2.2, §7.4 rows 1–2).

**W0.4 · Rulers and grid view state (ST-01, ST-03, ST-05, ST-06, PP-12, MN-03)** — done, including Dynamic
(default) / Static grid modes, page-aware contrast and independent per-axis coarsening. Show/Snap stay independent; see
[NF2-RULERS-GUIDES-GRID.md](NF2-RULERS-GUIDES-GRID.md). View settings live in `stage_view.rs` (`ViewPrefs`), defaults in
`WorkspacePrefs::view_defaults`; the popover is `panels/view_settings.rs`. Original plan:
- UI: `shell.rs › stage_chrome` (pass `Some(..)` to `ZoomControl`, handle `ZoomAction::{Rulers, Grid, ViewSettings}`),
  `panels/menu.rs` View items, new `panels/view_settings.rs` from `planned_panels::view_settings`
  (`ViewSettings` + `ViewSettingsState` + `ViewSettingsResponse`), stage painting in `stage.rs`.
- Engine: none (view state). Guides are W1.2.
- API: `ViewSettings` state in `WorkspacePrefs` (NEW, serde default from `ViewSettingsState::defaults`).
- Tests: stage↔ruler coordinate mapping at zoom 0.25/1/4 and pan; label step Auto selection.
- Registry: `editor-workspace`. Acceptance: spec ST-01/ST-03.

**W0.5 · NF2 stage cursors (ST-07)** — done (`stage_cursor.rs`; test `stage_cursor_follows_transform_zones_and_selection_targets`)
- UI: `stage.rs` / `stage_selection.rs` / `authoring_tools.rs` tool hover code: replace `ctx.set_cursor_icon` over the
  canvas with `neoflash_ui::cursors::show(ctx, canvas, Cursor::by_name(..))`; chrome keeps system cursors.
- Tests: mapping test tool/hover-state → cursor name exists in `cursors::ALL`.
- Registry: `editor-workspace`. Acceptance: spec ST-07.

**W0.6 · Colour effect styles (PR-04)** — blocked on R-012 (Properties Colour effect style rows in `panels/properties.rs`)
- UI: `panels/properties.rs` "Colour effect" section for `ElementKind::Instance` (Style select + value rows using the
  existing `select_row` / `numeric_row` / `paint_row`). If the Style select or Advanced fields are not in the current
  composition, request first (layout reference `panel-properties-movie-clip.png`; filed as R-012).
- Engine: none — `Transform.color_transform` exists and interpolates.
- Tests: Tint 50 % → expected `ColorTransform` multipliers/offsets; HTML5 export renders tint (existing runtime).
- Registry: `editor-paint-effects`. Acceptance: spec PR-04.

**W0.7 · Output levels and timestamps (OU-01)**
- UI: `panels/actions.rs › output_panel`; add the NF2 "Show" select only if the reference row exists in the current
  composition (it does in `panels-more-2.png`; the code comment says it was deliberately not offered).
- API: `Editor.output: Vec<OutputLine { time, level: Info|Warning|Error|Trace, text }>` (NEW, editor);
  `Editor::message` → Info, diagnostics → Error/Warning.
- Tests: filter test; copy text format.
- Registry: `web-publish-diagnostics`. Acceptance: spec OU-01.

**W0.8 · Toasts, progress, spinner (ST-09)** — **partial**
- UI/API: `notifications.rs` hosts shared NF2 Toasts, at most three above the canvas bottom (status bar fallback),
  with coalescing, dismissal, hover pause and modal delay. Info/Success last 4 s, Warning 6 s, Error until dismissed
  or action. `Editor::notify` also writes the originating document's Output; Show reveals the diagnostic after a
  document switch. History is session-only.
- Connected callers: partial-stroke Warning refusals and Save/explicit-reload Error with Show. Ordinary Open,
  successful Save, import and export retain their existing feedback; general result routing and VoiceOver speech
  acceptance remain open. Connected operation-specific progress/cancellation retains its own host.
- Tests: notification queue/expiry and host-bounds/dismiss/Output families. Registry: `editor-workspace`.
  Acceptance: spec ST-09; do not mark the broader workflow complete from host coverage alone.

**W0.9 · Swatches panel + Color panel Swatches tab (PP-10, CO-03 tab part)** — **done** (`panels/swatches.rs`, `PanelId::Swatches` `⌘F9`, Color panel ⋯ `Add swatch`, `WorkspacePrefs.swatches`, format-safe gradient swatch application; persistent document swatches remain W1.9)
- UI: new `panels/swatches.rs` from `planned_panels::swatches` (`SwatchesPopover`); Color panel ⋯ Add swatch.
- Engine: none if Document swatches are derived from used colours; persistent document swatches are W1.9.
- API: `Editor::recent_colors` (prefs), `document_colors(&Project) -> Vec<Color>` (editor scan of `ElementKind` paints).
- Tests: scan returns unique colours in stable order. Registry: `editor-paint-effects`. Acceptance: spec PP-10.

**W0.10 · Tools fill/stroke chip → Swatches popover (TO-08)** — approval request §4
- UI: `panels/tools.rs › fill_stroke_chip`. Tests: picking writes `Editor.fill/stroke`. Registry: `editor-workspace`.

**W0.11 · Info panel (PP-11)** — **done** (`panels/info.rs`, `PanelId::Info` `⌘I`; editable 2×2 selection W/H/X/Y with one Undo step, live stage Cursor X/Y in ruler units and topmost hit element / cached bitmap pixel RGBA readout)
- UI: new `panels/info.rs` from `planned_panels::info`; W/H/X/Y via the Transform panel's selection code
  (`authoring_modify.rs` layout units), cursor from the stage pointer transform, colour from topmost hit element paint.
- Tests: values for a known rectangle at zoom 2. Registry: `editor-workspace`. Acceptance: spec PP-11.

**W0.12 · Project panel (PP-05)** — **done** (`panels/project.rs`, `PanelId::Project`; filterable `src` / `assets` / `.nflash` tree, New file / New folder / Delete / Publish bar, inline rename, double-click opens Actions ▸ Project files, symbol/document class linkage caption)
- UI: new `panels/project.rs` from `planned_panels::project`, re-using `sources.rs` operations (new class, import,
  remove) and selection (`selected_source`); double-click opens Actions ▸ Project files.
- Tests: create/delete source with Undo; linkage footer text from `Symbol.class_name`.
- Registry: `web-project-toolchain`. Acceptance: spec PP-05.

**W0.13 · Code editor comments and folding (AC-04, AC-05)** — **partial**: Apply block/line comment and Remove comment
work; folding (Collapse between braces / Expand all) remains disabled.
- UI: `panels/actions.rs` `PLANNED_CODE_TOOLS` groups 2–3 enabled; logic in `sources.rs › code_editor`.
- Tests: comment toggle round-trip per language; fold ranges from `components::code::tokens`.
- Registry: `web-project-toolchain`. Acceptance: spec AC-04/AC-05.

**W0.14 · Document settings dialog (DL-02)** — **done** (`panels/document_settings.rs`, `Modify ▸ Document…` `⌘J`; linked W × H, Match contents, Scale content with stage + 9-point anchor, Units, Background, Frame rate, Make default in `WorkspacePrefs::document_defaults`, OK as one `"Document settings"` Undo step, Script row informational until W1.12)
- UI: dialog from `planned_dialogs::document_settings` (3.3); "Scale content with stage" transforms root content in
  one transaction; Match contents uses root bounds; Script version row stays informational until W1.12.
- Tests: OK = single Undo; Cancel no change. Registry: `platform-document-model`. Acceptance: spec DL-02.

**W0.15 · Menu items backed by existing commands (MN-01, MN-02 subset, LI-01, PR-06)** — **partial**: Insert ▸ Layer /
Layer folder / Motion guide / tweens / Scene, Edit ▸ Timeline ▸, Edit ▸ Undo/Redo `<step>`, Modify ▸ Document… (`⌘J`),
Control ▸ Go to end / step, Window ▸ Swatches / Info / Ease / History / Scene / Motion editor / Project, Library Delete /
Duplicate / Swap symbol (`panels/library.rs`, `authoring_symbols.rs`), Show in library, Clear keyframe, Convert to (blank)
keyframes and Select all frames work. Stage `Swap symbol…` / `Duplicate symbol…` work and right-click selects the
  target under every active tool. Affine in-context symbol editing now targets one exact placement, dims the frozen
  parent chain, inverse-maps authoring gestures/drops and restores the nested session view; isolated definition editing
  remains available. Non-affine 9-slice instances refuse safely, while arbitrary oblique guide/grid/ruler chrome waits
  for R-063. Open: Properties `Swap…` picker (R-012), true `Edit in new window` and Bitmap Trace/broader object rows
  (R-060). Distribute to layers, Find/Preferences/Shortcuts, Compiler errors/Code snippets/Movie Explorer,
Hide panels and Enable simple buttons are implemented; current working/unavailable boundaries are in
`NF2-FUNCTIONAL-SPEC.md` §11. Real-input menu tests exercise toggles in both directions without document changes.
- UI: `panels/menu.rs`, `authoring_modify.rs::modify_menu_ui`, frame/layer context menus (`panels/timeline.rs`,
  `timeline_edit.rs::frame_range_menu`, `layer_controls.rs`), Library actions in `panels/library.rs`.
- Commands that already exist and only need rows/shortcuts: Insert ▸ Layer / Layer folder; Edit ▸ Timeline ▸
  Cut/Copy/Paste/Clear/Remove frames; Control ▸ Go to end, step `.` `,`; Window ▸ new panels; Modify ▸ Document….
- New editor commands (no engine): Delete / Duplicate symbol (refuse or list usages for used symbols — `validate`
  rejects dangling `symbol_id`), Swap symbol…, Show in library, Clear keyframe, Convert to (blank) keyframes, Select all
  frames, Distribute to layers.
- Tests: one editor test per new command incl. Undo. Registry: `editor-symbols`, `animation-frame-ranges`,
  `editor-layer-organization`.

**W0.16 · Find and replace + Movie Explorer (DL-13, DL-14)** — need dialog/panel composition from the reference; if
no preview exists in code, the panel body must be built from NF2 components per `document-dialogs.png` outline →
**request first** (no preview body exists in `planned_*.rs`).
- Engine: none (editor traversal of `Project`, `Timeline.scripts`, `Element.scripts`, `Project.sources`).
- Tests: find text in TextBox + scripts; Replace all = one Undo. Registry: `editor-workspace`.

### Phase 1 — Small engine extensions

**W1.1 · History panel with step names (PP-09)** — E-12 — **done** (`neoflash-core/src/history.rs` step labels + `step_to`, `Editor::transact_named`, `panels/history.rs`, `PanelId::History` `⌘F10`, `Edit ▸ Undo/Redo <step>`)
- UI: new `panels/history.rs` from `planned_panels::history`; Edit menu Undo label "Undo <step>".
- Engine: `neoflash-core/src/history.rs`. API: `History::begin_named(&Project, &str)`, `commit_named`,
  `undo_labels` / `redo_labels` / `undo_label` / `redo_label`, `step_to(&mut Project, usize)`.
  Editor: `Editor::transact_named(label, f)`; `transact` defaults to `"Edit"`.
- Tests: core — labels follow undo/redo/capacity eviction; editor — click step back/forward == N × undo/redo.
- Registry: `platform-undo`. Acceptance: spec PP-09.

**W1.2 · Guides (ST-02)** — **done** (E-04): `Timeline::guides`, format version 6, export strips guides; see
[NF2-RULERS-GUIDES-GRID.md](NF2-RULERS-GUIDES-GRID.md). Original plan:
- UI: `stage.rs` guide painting/drag, ruler drag-out, View ▸ Guides items, View settings guide section.
- Engine: `model.rs` NEW `Timeline.guides: Vec<Guide { id, axis, position }>` (authoring-only, serde default,
  format version bump), validation (finite, bounded count), export ignores.
- Tests: save/open round-trip; export JSON contains no guides; Undo per move.
- Registry: `editor-advanced-drawing` (snapping/guides). Acceptance: spec ST-02.

**W1.3 · Snapping (ST-04, TO-06, Preferences ▸ Drawing snapping)** — E-05; **partial**: grid, guide, object and pixel
snapping (`Guides > Objects > Grid > Pixels`, also across all moving-selection anchors) with the snap ring, the Tools rail `Snap to objects` toggle (TO-06) and
`⌘`-suspend during stage gestures are done (`snapping.rs`, `stage_view.rs`, `panels/tools.rs`); `View ▸ Snapping` submenu
remains blocked on R-003. Original plan:
- UI: tools rail snap toggle (if present in composition; else request), View ▸ Snapping, zoom-control Snap toggle,
  stage drag/transform/draw code paths (`stage_selection.rs`, `transform_geometry.rs`, `vector.rs`).
- Engine: none required (geometry in editor); optional shared `neoflash-core::geometry` helper for bounds/anchor
  candidates.
- Tests: snap-to-grid, snap-to-guide, snap-to-object-centre unit tests; ⌘ suspends snapping.
- Registry: `editor-advanced-drawing`. Acceptance: spec ST-04.

**W1.4 · Tween kind + Shape tween (TL-04)** — E-03 — **done** (§7). `Keyframe.tween_kind`, `neoflash_core::shape_tween`
(different contours morph), Timeline / frame menu / Insert entry points, explanation instead of a jump for unsuitable
content. Shape hints are §7.4.

**W1.5 · Compiler errors panel + diagnostics severity (PP-07, AC-08 partial, OU-01)** — E-14
- UI: new `panels/compiler_errors.rs` from `planned_panels::compiler_errors`; Go to source uses `sources::jump_to`.
- Engine: `neoflash-script/src/lib.rs` `Diagnostic` NEW `severity: Severity { Error, Warning }` (serde default Error),
  optional `code: Option<String>`. Editor maps each compiled unit to a location (symbol · layer · frame · object
  event / source path).
- Tests: script — warning-producing input yields `Warning`; editor — location mapping for frame and object scripts.
- Registry: `web-publish-diagnostics`, `web-project-toolchain`. Acceptance: spec PP-07.

**W1.6 · Gradient fills in the Color panel (CO-01)** — E-07 — **done** (`authoring_modify.rs` Linear / Radial fill & stroke gradients, `NF2/Gradient Bar` stop add/drag/remove, `NF2/Color Picker` stop colour editing, solid→`PaintedPath` conversion via `Element::to_path_commands()` preserving parametric geometry; Flow `Reflect`/`Repeat` and `Linear RGB` remain disabled while runtime `GradientPaint` uses Extend/pad spread and sRGB)
- UI: `authoring_modify.rs › fill_type_select` enable Linear/Radial; gradient editing rows.
- Engine: `model.rs` — convert solid-paint shapes to `PaintedPath` via `Element::to_path_commands()`, validation of
  stops; HTML5 runtime paints `GradientPaint`.
- Tests: conversion keeps geometry; HTML5 export renders gradient; Undo.
- Registry: `editor-paint-effects`. Acceptance: spec CO-01.

**W1.7 · Stroke styles (PR-03 Solid/Dashed/Dotted, caps, joins, miter, scale, hinting)** — E-06 — **done** (`panels/stroke_style.rs` modal dialog + live inline panel with `Solid`, `Dashed`, `Dotted`, `Ragged`, `Stipple` and `Hatched` styles, thickness `0.1..=200 pt`, `4× zoom` live preview, dash/gap lengths, sharp corners and single-step `"Stroke style"` Undo applying `stroke_width` and `ElementAppearance` `PatternBrushKind::DashPattern`)
- UI: Properties fill & stroke rows (`panels/properties.rs`), Color panel stroke section, Stroke style dialog (`panels/stroke_style.rs`).
- Engine: `ElementAppearance` (`PatternBrushKind::DashPattern`) and `stroke_width` on stroked element kinds; native painter and HTML5 export.
- Tests: `panel_tests::t126_planned_dialogs_panels_and_menus_work_with_undo`.
- Registry: `editor-advanced-drawing`. Acceptance: spec PR-03.

**W1.8 · Primitive tools and PolyStar (TO-01, TO-02, PR-02)** — E-08 — **done** (format-7 `Transform.primitive: Option<PrimitiveOptions>` for Rectangle corner radii and Oval start/end angle, inner radius and close-path, shared `geometry.rs` path builders used by evaluation/morph/hit-test/editor/HTML5 export, Properties & Tool options primitive controls + Reset, Break Apart to `Path`/`PaintedPath`, `Tool::PolyStar` + modal `PolyStar tool settings` dialog)
- UI: `panels/tools.rs`, `authoring_tools.rs`, `panels/properties.rs` primitive sections, modal PolyStar settings dialog.
- Engine: parametric `PrimitiveOptions` on `Transform` (Rectangle corner radii; Oval start/end angle, inner radius,
  closed) in format 7 with geometry generation shared by evaluation/export; PolyStar produces closed `Path`s.
- Tests: geometry generation unit tests; Break Apart → equivalent path; PolyStar stage drag + modal settings.
- Registry: `editor-primitives`, `editor-advanced-drawing`.

**W1.9 · Document swatches persistence (PP-10, CO-03)** — E-13 — **done** (`Project::custom_swatches`, `Project::set_custom_swatches` stored portably in `palette/swatches.json`, wired to `Swatches` / `Color` `Add swatch` with single-step Undo)
- Engine: `Project::custom_swatches` / `set_custom_swatches` in `palette/swatches.json`. Tests: round-trip, Undo.
- Registry: `editor-paint-effects`.

**W1.10 · Publish settings (DL-03, Preferences ▸ Publish)** — E-15
- Available: workspace `PublishPrefs` (`options`, `default_target`, `enable_html5`, `enable_video_mp4`, `video_output_file`, `video_options`) feeds Publish/publish-on-save; draft publication and cancellable
  background estimates reuse exporter options and validation. R-032 is wired in `panels/publish_settings.rs`: four
  levels, on-disk bar/legend, exact gzip/files, Computing/Failed/Retry, omissions, Custom reset, Shared URL
  validation/runtime-folder export, plus the `Video (MP4)` target checkbox and its switchable right-column video settings page (`Output file` + `Browse…`, `W × H` linked + `Scale`, `FPS`, `Codec: H.264 / H.265`, `Quality` slider, `Range`, `Render nested movie clip animation and scripts`, `Stop at last frame` / `Stop after N s`, and `Include audio` + `AAC 128/192/256 kbps`). Publish uses the draft; OK saves workspace defaults; Cancel/close/document
  switch cancels it. Full/Embed and Full Test Movie remain defaults. Other controls stay disabled.
- UI: File/Properties Publish settings entry points open the same modal. Document-stored profiles and the engine
  settings below remain future work.
- Engine: NEW `Project.publish: PublishSettings { loop, autoplay, scale_mode, include_trace, minify, strict,
  jpeg_quality, html_template }`; `neoflash-export::export_project_with(project, settings, …)` (NEW); web runtime honours
  loop/autoplay/scale.
- Tests: export with loop off stops at end (browser scenario); trace stripped.
- Registry: `web-html5-export`. Acceptance: spec DL-03.

**W1.11 · Authored text options (PR-01 part: type Static/Dynamic/Input, selectable, border, password, max chars)** — E-09
- Stage Text now distinguishes point text (click) from fixed multiline wrapping TextBox authoring (drag). Break Apart
  reaches editable installed-font glyph contours; multi-character text first splits into positioned characters.
- UI: `text_boxes.rs › text_box_properties` rows existing in the NF2 Text variant composition (request for any row not
  present).
- Engine: NEW `TextBox` fields; export maps to runtime `TextField` (already implemented for scripts).
- Tests: authored Input text editable in HTML5 (browser scenario); AS3 instance name access.
- Registry: `editor-text`.

**W1.12 · Loop/scene-agnostic playback option + document script default** — E-01 — **done** (editor Loop playback and Play all scenes in W0.1 plus `AuthoredPlayback::set_looping(bool)` / `is_looping()` in `crates/neoflash-core/src/authored_playback.rs` so preview and Test Movie share loop/non-loop playback semantics)
- Engine: `AuthoredPlayback::set_looping(bool)` so preview and Test Movie share semantics.
- Registry: `animation-lifecycle`.

### Phase 2 — Major features

**W2.1 · Motion tweens + Motion editor + Motion tween properties (TL-05, PP-02, PR-10/PP-03, MN-02)** — E-10 —
**done** (§7). ◆ property keyframes with independent tracks and per-property eases (`Keyframe.property_eases`
in format 7), moving ◆ keys horizontally in time in the Motion editor, `Add property` (`+` on `Basic motion`,
`Transformation`, `Colour effect` and `Filters`) and the `Frames` span-duration stepper in `panels/motion_editor.rs`,
Orient to path for motion tweens, Show path / Show frame dots (`TimelineView`), Sync graphic symbols
(`Keyframe.sync_symbols`), Snap to motion guide (`Keyframe.snap_to_guide`), motion path on the stage, Motion editor
(`panels/motion_editor.rs`), Properties — Motion tween (`panels/motion_tween.rs`), Copy / Paste motion and Paste motion
special; Bézier segment/handle editing uses the approved R-008 painter.
Motion spans accept every object on a non-empty layer and keep independent property tracks by stable object ID;
the first object remains the shared reference only for a curved spatial path.
Registry: `animation-motion-tween`, `animation-easing`.

**W2.2 · Ease presets Bounce/Elastic + per-property curves (PP-01 rest)** — E-02 — **done** (§7.4 rows 1–2).
`Easing::{Bounce, Elastic}` (Penner exact In/Out/InOut formulas) and `Keyframe.property_eases` in format 7,
`panels/ease.rs` Bounce/Elastic presets + `Property` select + `One curve for all`, `panels/properties.rs` classic/shape
Easing select, and `panels/motion_editor.rs` per-track `Ease` column. Registry: `animation-easing`.

**W2.3 · Motion guides (TL-01, TL-13, DL-16 Guided)** — E-11 — **done** (§7) including the `Guided` type in the Layer
properties modal dialog (`panels/layer_properties.rs`) and classic Tween Orient / Snap controls (R-007). Registry: `editor-layer-organization`.

**W2.4 · Scenes (TL-03, PP-08, ST-08, Insert ▸ Scene, Control ▸ Test scene / Play all scenes)** — E-16 — **done** (§7).
`Project.scenes`, Scene select, Insert ▸ Scene, Scene panel, Play all scenes, `Control ▸ Test scene` (`⌥⌘↵`),
breadcrumb `▾` / `Symbols & scenes` popover (`panels/symbols_popover.rs`, ST-08), runtime/HTML5 flattening and
scene-aware AS2/AS3 navigation. Native Play all scenes with Loop off stops after the final scene. Registry: `animation-lifecycle`.

**W2.5 · Display & filters (PR-05/PP-14, blend, cache as bitmap, visible)** — E-17
- UI: Properties movie-clip tabs from `planned_panels::properties_filters`.
- Engine: authored `Element.filters: Vec<FilterSpec>`, `blend`, `cache_as_bitmap`, `visible`; native preview
  rasterisation; export into runtime display graph (runtime already applies filters/blend for scripts).
- Registry: `editor-paint-effects`.

**W2.6 · Timeline sound (TL-07, PR-08/PP-13, TL-13 Sound layer, Control ▸ Mute sounds)** — E-18 — **partial** (§7,
decision 0037). Done: `Keyframe.sound`, Flash sync in HTML5 and editor playback, Sound button, Properties ▸ Sound,
`Edit envelope` modal dialog (`panels/edit_envelope.rs`, format-7 `SoundEffect::Custom` + `FrameSound.envelope`, DL-09),
`Sound properties` modal dialog (`panels/sound_properties.rs`, DL-08), Sound layers, Mute sounds, legacy binary FLA
PCM/MP3 audio resources and `CPicFrame` frame sounds. Open (§7.4): decoded waveform peaks, compressed legacy FLA audio
codecs (ADPCM/GSM/Nellymoser), audio re-encoding at publish. Registry: `animation-media-sync`, `web-media-assets`,
`imports-binary-document`.

**W2.7 · Authored video (PR-09, TL-13 Video layer)** — E-19. Registry: `animation-media-sync`, `web-media-assets`.

**W2.8 · Code hints, toolbox, navigator, auto-format (AC-01, AC-02, AC-06, AC-08 rest)** — E-20 — **partial**: code
hints, toolbox sidebar, Script navigator and re-indenting Auto-format work ([NF2-ACTIONS-EDITOR.md](NF2-ACTIONS-EDITOR.md));
warnings/severity, peek, quick fixes and full formatting remain.
- UI: `panels/actions.rs` toolbar groups 1 and 4; sidebar.
- Engine: `neoflash-script` completion/symbol API + formatter. Registry: `web-project-toolchain`.

**W2.9 · Debugger + breakpoints + Output trace bridge + Test Movie window (PP-04, AC-03, OU-02, DL-12)** — E-21
- UI: new `panels/debugger.rs` from `planned_panels::debugger`; gutter breakpoints; Output Trace level.
- Engine: debug protocol from the HTML5 runtime to the editor; source maps. Registry: `web-project-toolchain`.

**W2.10 · Edit multiple frames + Preview thumbnails + transformation point (TL-08, TL-10, PR-07, ST-11)** — editor —
**partial**. Done: Edit multiple frames selecting objects across every keyframe in `onion_range` (including objects that
exist only on other keyframes) and moving/scaling/rotating/skewing across the range in one Undo step (TL-08), Frame view
▸ Preview thumbnails (TL-10, `timeline_preview.rs`), draggable Free Transform transformation point (`Transform.pivot` in
format 7 for single placements + group pivot; rotation and `⌥`-scale pivot around it, double-click resets; ST-11). Open:
Graphic thumbnail strip (PR-07, can reuse `timeline_preview.rs`). Registry: `animation-frame-ranges`, `editor-transform`.

### Phase 3 — Import/export, media pipelines, preferences, specialised tools

| Item | Spec | UI source | Engine block | Registry |
| --- | --- | --- | --- | --- |
| W3.1 Export image PNG + SVG — working (basic export) | DL-04 | `panels/export_image.rs`, `neoflash_export::image_export` (DL-04 remains partial while advanced option controls are inert) | E-22 | `web-html5-export` |
| W3.2 Export image PSD / AI / PDF — partial (documented fidelity and option fallbacks) | DL-04 | `panels/export_image.rs`, `neoflash_export::image_export` (layered PSD; EPS-compatible AI and raster-backed PDF; PSD groups/smart objects/editable text/colour conversion pending) | E-22 | `web-html5-export` |
| W3.3 Export video — done | DL-05 | `panels/export_video.rs` (DL-05 done) | E-23 | `web-html5-export`, `web-media-assets` |
| W3.4 Export audio, Sound properties, Edit envelope — done | DL-06, DL-08, DL-09 | `panels/export_audio.rs` (DL-06 done), `panels/sound_properties.rs` (DL-08 done), `panels/edit_envelope.rs` (DL-09 done) | E-18, E-23 | `web-media-assets`, `animation-media-sync` |
| W3.5 Import video, Import PSD — partial | DL-07, DL-10 | `panels/import_video.rs` imports validated media into the document; authored playback remains partial. `neoflash_import::psd` is a tested importer library; editor/CLI wiring remains. | E-19, E-24 | `web-media-assets`, `imports-*` |
| W3.6 Preferences persistence (all pages) — done (see card below) | DL-01 | `panels/preferences.rs`, `recovery.rs` | E-25 | `editor-workspace`, `platform-recovery` (autosave) |
| W3.7 Shortcut editor — done (`commands.rs`) | DL-01 Shortcuts | `prefs_shortcuts`, `commands.rs` | E-25 (command registry) | `editor-workspace` |
| W3.8 Font embedding + text embed rows — done for full font resources; subsetting remains partial | DL-11, PR-01 | `panels/font_embedding.rs` stores real license-permitted font binaries and metadata; browser-loadable subset generation remains in E-26 | E-26 | `web-font-portability` |
| W3.9 Deco, Spray, Bone/IK — done | TO-03…05 | `panels/live_planned.rs` (`apply_deco_pattern`, `apply_spray_brush_burst`, `live_bone_properties`) | E-27, E-28 | `editor-advanced-drawing` |
| W3.10 9-slice, sharing, accessibility extras, Script assist, Bitmap fill — partial | DL-15, DL-17, AC-07, CO-02 | Script assist and Bitmap fill are wired. 9-slice/shared-library/accessibility engines have tested commands, but their complete authoring controls remain open. | E-29 | `editor-symbols`, `platform-accessibility`, `editor-paint-effects` |

**W3.6 · Preferences (DL-01) & W3.7 · Shortcut editor** — **done** across all 8 active pages (`panels/preferences.rs`,
`commands.rs`, `recovery.rs`): modal dialog with per-page draft groups and `Reset to defaults`, live theme preview,
`Save` / `Cancel`; **General** (Theme, UI scale, On launch + `File ▸ Open recent`, Undo levels, Autosave every N min to
`<app_support>/Recovery` + launch crash-recovery prompt, Show tooltips with shortcuts, Shift-select); **Code editor**
(font family, size, line height, line numbers, word wrap, auto-close brackets/quotes, code hints + delay, tab size,
format on save, highlight current line, live preview); **Timeline** (span-based selection, frame view / row height, Show
seconds, Snap playhead, Onion range, linear Onion opacity falloff shared with the popover, Create keyframe on transform,
Default ease, Tween type on drag); **Drawing** (pen preview, precise cursors, connect lines, smooth curves, recognise
lines, recognise shapes, click accuracy, snap tolerance, snap to pixels); **Text** (font, size, substitution warning);
**Clipboard** (default paste, SVG vector paste as editable paths vs raster bitmap); **Shortcuts** (98-command registry
in `commands.rs` driving dispatch and UI labels, user sets with `Duplicate` / `Delete`, search + category filter, key
recording, conflict badges, `Reset command`, `Export as HTML…`); **Publish** (output folder + `Browse…`, `Compression ▸ Audio` policy, warnings as
errors, publish on save). Tests in `panel_tests.rs`, `selection_tests.rs`, `vector_gesture_tests.rs`,
`clipboard_tests.rs`, `code_tools.rs`, `commands.rs`, and `recovery.rs`.

Controls kept disabled with explicit design requests (`R-013`–`R-020`) or separate subsystem cards:

| Control | Status | Reason |
| --- | --- | --- |
| General ▸ follow system, Accent, Language, Highlight; Welcome screen (`On launch`); locked tooltip call sites | design (`R-013`–`R-016`) | Needs selectable control, token override policy, localisation, or locked `neoflash-ui` flag |
| Code editor ▸ Colour scheme (`NF2 Night`, `Duplicate`, `Import…`, syntax swatch table) | design (`R-017`) | Syntax tokens are fixed in locked `neoflash-ui::components::code` |
| Timeline ▸ Onion tints (`Before` / `After`), Named anchor on each scene | design (`R-018`) / model + web | Swatches have no picker state; scene anchors need label-kind format + browser hash history |
| Drawing ▸ Merge drawing, Show solid points, Snap align | L subsystem / design (`R-019`) | Planar merge model not implemented; solid unselected anchor & align guide painter need NF2 components |
| Text ▸ Anti-alias, Embed used glyphs (W3.8 / E-26), Spell check, Vertical text, Input method | L subsystems | Require authored text anti-alias/vertical model, font subsetter (E-26), and spell-check engine |
| Clipboard ▸ Copy as SVG, Keep layer names, Bitmaps ppi, Gradient quality, Large bitmap prompt | L subsystem | Requires native OS SVG/bitmap pasteboard writer (`arboard` is text-only today) |
| Shortcuts ▸ Rename set | design (`R-020`) | No inline field or modal prompt in `panels-dialog-preferences-shortcuts.txt` |
| Publish ▸ HTML wrapper / Assets folder / Source maps / JPEG / Acceleration / Strict | W1.10 (DL-03) | Awaits `PublishSettings` export pipeline options and image re-encoders; JS is always strict |
| Updates page | — | Stays disabled (no update service, no telemetry) |

Preferences ▸ Updates must stay disabled (no update service, no telemetry) until `platform-release` /
`platform-extensions` define them; never send usage data.

## 6. Dependency order

```text
Done: W0.1, W0.2, W0.3, W0.4, W0.5, W0.9, W0.11, W0.12, W0.14, W1.1, W1.2, W1.4, W1.6, W1.8, W2.1, W2.2, W2.3, W2.4
Partial: W0.13, W0.15, W1.3, W1.12, W2.6, W2.8, W2.10
W1.3 View ▸ Snapping submenu ◄── R-003 (object/pixel snapping, Tools toggle and ⌘-suspend are done)
W0.6 Colour effect styles, W0.15 Properties Swap… picker ◄── R-012
W0.7 output levels ──► W1.5 compiler errors ──► W2.8 code intelligence ──► W2.9 debugger + trace bridge
W1.1 history names ──► (Edit menu labels, Preferences undo levels in W3.6)
W0.9 swatches (derived) ──► W1.9 document swatches ──► W1.6 gradients (Gradients section)
W1.7 stroke styles, W1.8 primitives ──► W3.2 vector exports (SVG/AI/PDF need them)
W1.10 publish settings ──► W3.1–W3.3 exports (shared range/size options)
W2.4 scenes ──► Export "whole movie (all scenes)"
W2.6 timeline sound ──► W3.4 audio export / publish re-encoding ──► W3.3 video export audio track
W2.7 authored video ──► W3.5 import video
```

Recommended batch order for the open items (breadth first): Batch A = W0.5–W0.12, W0.14–W0.16 and the rest of W0.13 /
W0.15; Batch B = W1.1, W1.3 rest, W1.5 (after requests are answered); Batch C = W1.6–W1.12; the timeline task cards of
§7.4 as one or two coherent batches; then the remaining Phase 2 items each as one batch with its UI, engine, export and
tests; Phase 3 last.

## 7. Timeline: current state and remaining work

Read this before touching the timeline. AGENTS.md › "Timeline engine" is the short version of §7.1–§7.3.

### 7.1 Document model (formats 6–8, `crates/neoflash-core/src/model.rs`)

| Field | Meaning |
| --- | --- |
| `Keyframe.tween: Option<Easing>` | The span starting here is tweened with this ease. `Easing` = `Linear`, `EaseIn`, `EaseOut`, `EaseInOut`, `CubicBezier { x1, y1, x2, y2 }` (x ∈ 0…1, \|y\| ≤ 100), `Flash { acceleration }` (−100…100) in format 6, plus `Bounce { direction }` and `Elastic { direction }` (`EaseDirection::{In, Out, InOut}`, Penner exact formulas) in format 7. |
| `Keyframe.tween_kind: TweenKind` | `Classic` (default, omitted in JSON) interpolates poses of elements with the same ID; `Shape` morphs the layer's shapes; `Motion` = motion tween with property tracks. |
| `Keyframe.keyed_properties: Option<Vec<TweenProperty>>` | `Some` marks a ◆ property keyframe inside a motion span. It stores a full pose, but only the listed properties (`X`, `Y`, `ScaleX`, `ScaleY`, `Rotation`, `Skew`, `Alpha`, `Color`) are keys. `None` = an ordinary keyframe that starts a new span. |
| `Keyframe.property_eases: Vec<(TweenProperty, Easing)>` | Optional per-property ease overrides on a motion span's start keyframe (format 7); tracks without an override use `Keyframe.tween`. |
| `Keyframe.motion_path: Option<Vec<PathCommand>>` | Format-8 open MoveTo + LineTo/CubicTo spatial curve on a motion start key. Arc length is sampled at eased X/Y track times; the first element anchors the path. |
| `Keyframe.shape_hints: Vec<ShapeHint>` | Existing format-7 `{ label, start_uv, end_uv }` correspondence pairs; at most 26 labels a–z. |
| `Keyframe.orient_to_path: bool` | Turn the tweened object with the path tangent. Evaluated for guided classic tweens (`sample_layer_in`, guide tangent) and motion tweens (`sample_motion`, X/Y track direction). |
| `Keyframe.sync_symbols: bool` | Continuous Graphic symbol playback across a motion span and chained `sync_symbols` spans (format 7; `evaluate::placement_start` + `sync_layer_symbols`). |
| `Keyframe.snap_to_guide: bool` | Snap a guided span's path endpoints to the linked guide stroke endpoints (motion: format 7; classic: format 8). |
| `Keyframe.sound: Option<FrameSound>` | Frame sound: `resource` (an `audio/*` `ResourceUnit`), `sync` (`Event`/`Start`/`Stop`/`Stream`), `loops` (1–10 000), `loop_forever`, `effect` (Flash preset envelope or format-7 `SoundEffect::Custom`), `envelope: Option<Vec<[f64; 3]>>` (format-7 stereo `[position, left, right]` points, up to 8). A layer whose keys hold sounds and no artwork is shown as a Sound layer (derived, not stored). |
| `LayerProperties.motion_guide: Option<Id>` | A `Normal` layer guided by a `LayerRole::Guide` layer of the same timeline. The guide's first stroke is the path. |
| `Project.scenes: Vec<Id>` | Scene timelines in play order (MovieClip symbol IDs). Empty = one scene, `root`; otherwise `scenes[0] == root`. Scene symbols are not library items and are never instantiated. |
| `Timeline.guides: Vec<Guide>` | Ruler guides (authoring only, stripped from HTML5). See [NF2-RULERS-GUIDES-GRID.md](NF2-RULERS-GUIDES-GRID.md). |

`Project::uses_format_6()`, `uses_format_7()` and `uses_format_8()` detect these features; `upgrade_format_for_features()` raises
`format_version` to 6, 7 or 8 as needed; validation (`validation.rs`) accepts versions 1–8, rejects format-6/7/8 features in
older versions, ◆ keys outside a motion span, `property_eases` outside a motion span start key or duplicate properties,
invalid spatial paths and path placement, custom sound envelopes outside `1..=8` sorted finite `[0..=1]` stereo points,
motion guides that do not point to a guide layer, scene lists whose first entry is not `root`, duplicate or
non-MovieClip scenes, instances of scene symbols and frame sounds without an audio resource. Every new field is
`#[serde(default, skip_serializing_if = …)]`, so documents that do not use it serialize as before.

### 7.2 Where each behaviour lives

| File | Responsibility |
| --- | --- |
| `neoflash-core/src/evaluate.rs` | `sample_layer` dispatches by tween kind: Motion → `sample_motion` (each property interpolates through the ◆ keys that key it using its `property_ease` override or the span ease over the whole span; after its last key a track holds), Shape → `shape_tween::shape_tween_elements`, Classic → identity interpolation. `motion_span(layer, frame)` returns the start key, its ◆ keys and the span end. `sample_layer_in(timeline, layer, frame)` adds the motion guide (`guide_polyline`, `GuidePath`, `snap_to_guide` endpoint snapping) and Orient to path; `placement_start` chains continuous Graphic playback across `sync_symbols` motion spans. |
| `neoflash-core/src/shape_tween.rs` | Shape morph: shapes pair by ID, then by order; equal command structure interpolates exactly; other contours are resampled by arc length to a common cubic count with start-point/direction alignment; unpaired shapes and contours grow/shrink from their centre; fills, strokes and compatible gradients blend. |
| `neoflash-core/src/timeline_edit.rs` | Range edits (`TimelineEdit`), including `Reverse`, `ConvertToKeyframes`, `SyncSymbols` (`sync_layer_symbols`) and curve-preserving De Casteljau cubic-Bézier subdivision (`subdivide_cubic_bezier`) when splitting a tween span on `InsertKeyframe` / `ClearFrames` / `RemoveFrames`. Every edit ends with `Layer::normalize_motion_keys`, so a cut through a motion tween never leaves orphan ◆ keys. |
| `neoflash-core/src/scenes.rs` | `scene_ranges`, `flatten_scenes` / `flatten_scenes_with_ranges` (concatenate scenes into one main timeline with fresh layer IDs), `resolve_goto` (AS3/JS frames are scene-relative, AS2 frames absolute unless a scene is named), `scene_step` (`nextScene` / `prevScene`). |
| `neoflash-core/src/timeline_sound.rs` | `SoundState` inside `Player` turns keyframe sounds into `SoundCommand`s (`Play`, `Stop`, `StreamStart`, `StreamStop`, `StopAll`) when frames are entered; `FrameSound::envelope_points` (presets + `SoundEffect::Custom` points); `envelope_gains`; `sound_span_end`. Never called from pure evaluation. |
| `neoflash-core/src/runtime.rs` | `Player::set_scenes`, `resolve_goto`, `resolve_scene_step`, `drain_sound_commands`, `stop_all_sounds`. |
| `neoflash-export/src/lib.rs`, `neoflash-web/src/lib.rs` | The **scene boundary**: scripts compile against the authored scenes (diagnostics name the scene), then the published bundle holds the flattened project plus `scenes` ranges; `WebPlayer::new` flattens an authored project or accepts the bundle's ranges. Nothing else flattens. |
| `web/audio.js`, `web/runtime.js` | Decode each timeline sound resource once and schedule `sound_commands_json()` on Web Audio (buffer sources + gain automation); `stopAllSounds` / `SoundMixer.stopAll` call `stop_all_sounds`. |
| `neoflash-editor/src/timeline_commands.rs` | `Editor::commit_history` and `record_motion_property_keys`; tween create/remove/toggle, `add_motion_guide`, `clear_keyframe`, `convert_to_keyframes`, `reverse_frames`, `sync_symbols`, scenes (`scene_list`, `switch_scene`, `new_scene`, `delete_scene`, `app.rs::test_scene`), playback (`after_playback_tick`, `rewind_if_finished`), `snap_scrub_frame`, Edit multiple frames (`edit_multiple_key_frames`, cross-keyframe selection/move/scale/rotate/skew). |
| `neoflash-editor/src/timeline_motion.rs` | Motion path on the stage (`motion_path`, `paint_motion_path`, ◆ key drag with one Undo, `snap_motion_span_to_guide`), Copy / Paste motion. |
| `neoflash-editor/src/timeline_preview.rs` | Frame view ▸ Preview thumbnails, cached per keyframe and `History::revision`. |
| `neoflash-editor/src/timeline_sound.rs` | Attach/import/edit frame sounds; native preview: decodes with rodio/Symphonia (MP3, WAV) and plays the preview `Player`'s `SoundCommand`s and custom envelopes; Sound properties audition/update/replace; Mute sounds. Scrubbing is silent. |
| `neoflash-editor/src/panels/` | `timeline_controls.rs` (transport, tween/ease/sound buttons, scene select, frame view menu), `timeline.rs` (frame menu), `layer_controls.rs` + `layer_properties.rs` (layer menu, Add classic motion guide, Layer properties modal), `symbols_popover.rs` (breadcrumb `▾` + Symbols & scenes popover), `ease.rs`, `scene.rs`, `motion_editor.rs`, `motion_tween.rs` (also the motion-span edits shared with the Motion editor), `properties_sound.rs`, `edit_envelope.rs`, `sound_properties.rs`, `menu.rs` (Insert / Edit ▸ Timeline / Modify ▸ Timeline / Control / Window). |
| `neoflash-editor/src/workspace.rs` | `TimelineView { tinted, show_seconds, compact_rows, short_rows, snap_playhead, loop_playback, preview, show_motion_path, show_motion_frame_dots }` (saved with the workspace; `row_height()` feeds painters and `timeline_cell_at`, including 14 pt `Compact` rows); `PanelId::{Ease, Scene, MotionEditor}` start hidden. Session-only state (`edit_multiple`, `play_all_scenes`) lives in `timeline_edit.rs::TimelineSelection`. |

**Invariant.** Every history commit goes through `Editor::commit_history` (`Editor::transact` already ends with it):
`record_motion_property_keys` keys exactly the properties an edit changed on a ◆ key and refreshes the unkeyed values
to the displayed pose, `prune_motion_guides` drops links to deleted guides, `upgrade_format_for_features` raises the
version. Code that calls `history.commit` directly bypasses all three.

### 7.3 Tests, example documents and screenshots

- Core: `crates/neoflash-core/tests/timeline_tweens.rs` (tracks, guides, orient, reverse/convert, range edits, sync
  symbols, example validity, imported FLA corpus playback), `tests/timeline_sound.rs` (every sync mode, loops, seek,
  stop-all, nested clips, validation), `tests/frame_labels.rs` (scene flattening and goto), unit tests in
  `shape_tween.rs` and `scenes.rs`.
- Editor: `timeline_commands_tests.rs`, `timeline_motion_tests.rs`, `timeline_panels_tests.rs` (Ease, Scene, Motion
  editor, Motion tween properties, panel prefs), `timeline_sound_tests.rs`, `stage_view_tests.rs` (rulers/guides).
- Web/WASM: `web/tests/as2-timeline.test.cjs` (scenes in AS3 and AS2), `web/tests/wasm.test.cjs` (timeline sound
  commands); project workflow family `sound-playback` (`crates/neoflash-script/tests/sound-playback.mjs`, run by
  `project-execute.mjs`).
- Examples: `examples/timeline-tweens.nflash` (shape, motion and guided tweens, two scenes) and
  `examples/timeline-sound.nflash` (Stream, Event and `SoundMixer.stopAll()` with generated WAV tones).
- Screenshot: `NEOFLASH_OPEN=examples/timeline-tweens.nflash NEOFLASH_SCREENSHOT_FRAME=2,16 NEOFLASH_WINDOW=1920x1080
  NEOFLASH_SCREENSHOT=target/ui-shots/t.png NEOFLASH_SCREENSHOT_THEME=dark cargo run -p neoflash-editor`
  (`NEOFLASH_SCREENSHOT_FRAME=<layer index>,<frame>`, both zero-based; layer index 0 is the bottom row). Capture both
  themes.

Layer copy remaps copied motion-guide links; layer merge samples guided poses and preserves frame sounds. A same-frame
sound collision or shortened Stream span is rejected before editing (`layer_controls.rs` family tests).

### 7.4 Remaining timeline work (task cards)

Each row is one delivery unless grouped. Do not change visuals (§1); file a request when the design is missing. Every
model change follows §1.7: serde default, one format bump per batch (current format is **8**: `uses_format_8` sits beside
`uses_format_6` and `uses_format_7`), validation, an old-document load test.

| # | Work (spec) | Files to touch | Model change | Tests to add | Acceptance |
| --- | --- | --- | --- | --- | --- |
| 1 | ~~Bounce / Elastic ease presets (PP-01, E-02, W2.2)~~ | Done: format-7 `Easing::{Bounce, Elastic}` (`EaseDirection::{In, Out, InOut}`, Penner exact formulas in `evaluate::easing`), `panels/ease.rs` Bounce/Elastic presets and `panels/properties.rs` classic/shape Easing select (`timeline_tweens.rs`, `timeline_panels_tests.rs`). | | | |
| 2 | ~~Per-property eases (PP-01 Property select, PP-02 Ease column, E-02/E-10)~~ | Done: format-7 `Keyframe.property_eases: Vec<(TweenProperty, Easing)>` on motion span start keyframes, `evaluate::sample_motion` per-track ease lookup, `panels/ease.rs` `Property` select + `One curve for all` and `panels/motion_editor.rs` per-track `Ease` column (`timeline_tweens.rs`, `timeline_panels_tests.rs`). | | | |
| 3 | ~~Move keys in time in the Motion editor (PP-02)~~ | Done: `Editor::move_motion_key` + horizontal ◆ key dragging on `panels/motion_editor.rs` lanes retimes a single property's keyframe with `normalize_motion_keys` and one Undo step (`timeline_panels_tests.rs`). | | | |
| 4 | ~~Orient to path for motion tweens~~ | Done: `sample_motion` turns the object with its X/Y track direction (test `motion_tween_orient_to_path_turns_with_the_direction_of_travel`). | | | |
| 5 | ~~Classic Orient / Snap controls~~ | Done: approved R-007 rows in frame Properties; flags enable only with a motion guide, one Undo (`timeline_panels_tests.rs`). Rotate/Direction, Sync and Scale remain disabled where unsupported. | | | |
| 6 | ~~Show path / Show frame dots (PR-10)~~ | Done: `TimelineView { show_motion_path, show_motion_frame_dots }` in `workspace.rs` (default `true`), enabled checkboxes in `panels/motion_tween.rs` and conditional painting/hit-testing in `timeline_motion.rs` (`timeline_panels_tests.rs`, `timeline_motion_tests.rs`). | | | |
| 7 | ~~Sync graphic symbols and Snap to motion guide options (PR-10)~~ | Done: format-7 `Keyframe.sync_symbols` (chained continuous Graphic playback in `evaluate::placement_start` + `sync_layer_symbols`) and `Keyframe.snap_to_guide` (guide endpoint snapping in `evaluate::sample_layer_in` + `snap_motion_span_to_guide`), enabled in `panels/motion_tween.rs` (`timeline_tweens.rs`, `timeline_motion_tests.rs`). | | | |
| 8 | ~~Paste motion special~~ | Done: R-006 dialog, property selection and relative scale/rotation options; unticked animation and surrounding content preserved by materializing affected spans, Ease-only keeps keys (`timeline_motion_tests.rs`). | | | |
| 9 | ~~Bézier motion path editing~~ | Done: format-8 `motion_path`, shared arc-length sampler, segment bending, mirrored handles / Option corner gestures and Undo (`timeline_tweens.rs`, `timeline_motion_curve_tests.rs`, `wasm.test.cjs`). Range cuts/reverse bake curves. | | | |
| 10 | ~~Shape hints~~ | Done: existing format-7 model wired to R-009 markers on current-layer endpoint keys; add/drag/offstage remove, Show shape hints and shortcuts (`timeline_shape_hints_tests.rs`). | | | |
| 11 | ~~Edit envelope (DL-09)~~ | Done: format-7 `SoundEffect::Custom` + `FrameSound.envelope: Option<Vec<[f64; 3]>>` (`1..=8` stereo `[position, left, right]` points), modal `panels/edit_envelope.rs` (`EnvelopeLane`, time/frames unit, zoom, presets, Play/Stop, one Undo), native rodio preview and WASM/Web Audio scheduling (`timeline_sound.rs`, `web/tests/wasm.test.cjs`, `timeline_sound_tests.rs`). | | | |
| 12 | ~~Sound properties (DL-08) & Export audio (DL-06)~~ | Done: modal `panels/sound_properties.rs` opened from `Properties ▸ Sound ▸ Sound properties…` and Library audio menus for name, decoded audio stats, per-resource `Export` compression (`Compression`, `Bit rate`, `Quality`, `Convert stereo to mono`, live `"Result: …"`, `ResourceUnit.audio_export` in format 9), transcoded `Test`/`Stop` audition, `Update` and `Import…` byte replacement in one Undo step; and modal `panels/export_audio.rs` (`File ▸ Export ▸ Export audio…` `⌥⇧⌘A`, `Library ▸ Export audio…`, CLI `--audio`) exporting timeline mixes, layers or resources to WAV/MP3/OGG/AAC (`timeline_sound_tests.rs`, `panel_tests.rs`). | | | |
| 13 | ~~Decoded waveform peaks~~ | Done: background decode/reduction, resource/zoom cache and real mono/stereo timeline/Sound Properties peaks (`timeline_sound_tests.rs`); scrubbing reuses cache. | | | |
| 14 | ~~Legacy FLA frame sounds (`fla_frame_sound`, `fla_media_not_imported`)~~ | Done for PCM (`audio/wav`) and MP3 (`audio/mpeg`) streams + `CPicFrame` Event/Start/Stop/Stream sync, repeat/loop-forever and preset effects (`neoflash-import/src/legacy/{media,convert}.rs`, `tests/batch_import_capabilities.rs`); ADPCM/GSM/Nellymoser and custom multi-point envelopes/trim points keep located warnings. | | | |
| 15 | ~~Edit multiple frames, rest (TL-08)~~ | Done: cross-keyframe selection (`selection.rs`, `stage_selection.rs`, including objects existing only on other keyframes in `onion_range`) and move/scale/rotate/skew across every keyframe in range in one Undo step (`timeline_commands.rs`, `timeline_commands_tests.rs`). Group requires current-keyframe members and diagnoses other-frame-only selections without mutation. | | | |
| 16 | ~~Control ▸ Test scene ⌥⌘↵ (MN-01, E-16)~~ | Done: `Editor::test_scene_project` / `test_scene` (`app.rs`, `panels/menu.rs` `Control ▸ Test scene` and `⌥⌘↵`) exports and opens a single-scene clone rooted at `current_scene()` (`panel_tests.rs`). | | | |
| 17 | ~~Breadcrumb ▾ and Symbols & scenes popover (ST-08)~~ | Done: `panels/symbols_popover.rs` + `shell.rs` breadcrumb `▾` and overflow `…` menu with instant search, `Scenes` / `Matches` / `Recent` / `Browse` folder tree and `↑↓ ←→ ↵ ⌥↵ Esc` keyboard navigation (`panel_tests.rs`). | | | |
| 18 | ~~Range edits inside tweens keep editable curves (`animation-frame-ranges`)~~ | Done: `timeline_edit.rs` subdivides `CubicBezier`, `EaseIn`, `EaseOut`, `EaseInOut`, `Flash` and `Linear` span and `property_eases` curves via exact De Casteljau cubic-Bézier splitting (`subdivide_cubic_bezier`) on `InsertKeyframe`, `ClearFrames` and `RemoveFrames` (`timeline_tweens.rs`). | | | |
| 19 | ~~XFL import of tween kinds, guides and scenes~~ | Done: `neoflash-import/src/xfl.rs` and `xfl/shape.rs` import multi-scene `DOMTimeline`s (`Project.scenes`), library folders, `normal`/`guide`/`mask`/`folder` layers, classic and shape tweens (with Flash acceleration, single-segment global `CustomEase` and `motionTweenOrientToPath`), Graphic `firstFrame` offsets, skew matrices and stroke-only straight/quadratic guide paths (`tests/batch_import_capabilities.rs`). | | | |
| 20 | ~~Layer properties dialog: Guided type (DL-16)~~ | Done: modal `panels/layer_properties.rs` (`Modify ▸ Timeline ▸ Layer properties…`, `Layer menu ▸ Properties…`, double-click layer icon) edits Name, Visible, Locked, `Normal`/`Guide`/`Guided`/`Mask`/`Masked`/`Folder` (`Guided` ↔ `motion_guide`), `Mask mode` (`Geometry`/`Alpha`/`Luminance`) + `Invert mask` (`R-048`), Outline colour & outlines toggle, and Layer height `Compact`/`Short`/`100 %`/`200 %` with one Undo step (`panel_tests.rs`, `timeline_commands_tests.rs`). | | | |
| 21 | ~~Frame view ▸ Help (F1) (TL-12)~~ | Done: `panels/timeline_controls.rs` opens Preferences ▸ Shortcuts, the same searchable command/function catalogue as global F1 (`panel_tests::preferences_dialog_previews_saves_cancels_and_resets`). | none | | |
| 22 | ~~Timeline and AS2/AS3 masks: geometry, alpha, luminance and AE-style mask inversion~~ | Done: format-9 `MaskMode::{Geometry, Alpha, Luminance}` (BT.709 luminance) and `LayerProperties.mask_invert`, evaluated across core (`authored_layers.rs`, `mask_composition.rs`, `mask_coverage.rs`), native stage (`stage_clipping.rs`), HTML5 Canvas 2D (`web/path.js`) and Lottie export (`lottie.rs` `"inv": true`), with editor controls in `Layer properties` (`R-048`), layer context menu / `Folder and mask…` and the 2 inline AE-style 16×16 mask icons on `LayerRole::Mask` timeline rows (`R-049`) (`timeline_masks.rs`, `timeline_commands_tests.rs`, `path-cache.test.cjs`). | | | |

Related stage work outside the timeline: object and pixel snapping (`Guides > Objects > Grid > Pixels`), the Tools
`Snap to objects` toggle and `⌘`-suspend are implemented in `snapping.rs` / `stage_view.rs`; `View ▸ Snapping` waits for
R-003, and the grid/guide rules in [NF2-RULERS-GUIDES-GRID.md](NF2-RULERS-GUIDES-GRID.md) keep passing.
