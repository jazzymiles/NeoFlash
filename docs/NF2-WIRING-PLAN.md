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
| W2.1 motion path | Resolved (R-005): `NF2/Motion Path` painter in `components/canvas.rs`. Open: R-006 Paste motion special dialog. |
| W0.8 toasts | Toast placement/stacking and auto-dismiss timing (not specified in §4). |
| W0.10 swatches chip | Approval to replace the egui RGB picker in the Tools fill/stroke popover with `SwatchesPopover` (changes the component used). |
| W0.2 frame view | Resolved: Placement, Preview / Preview in context, Help (disabled) and Close panel rows are in the frame-view ⋯ menu. |
| W0.15 menus | Confirm order/grouping for absent §8 menu items and View ▸ Grid/Guides/Snapping submenus. |
| W2.x canvas chrome | NF2 painters for Selection Box variants, Transform Box, transformation point, gradient focal handle (ST-11) before restyling stage overlays. |
| W1.1 History | Visual state for "steps after current" (dimmed?) in the History list. |
| W3.6 Preferences | Accent colour / language / custom syntax colours imply new token behaviour — confirm or keep disabled. |

## 5. Work items

Format: **UI** → panel file/function · **Engine** → crate/module · **API** (NEW = proposal) · **Tests** · **Registry**
(id in `docs/progress/product.json`) · **Acceptance** (short; full criteria in the spec).

### Phase 0 — UI/editor-only (no engine change)

**W0.1 · Loop playback (TL-02, Control ▸ Loop playback)** — **done** (§7). `WorkspacePrefs.timeline.loop_playback`;
end-of-timeline handling in `Editor::after_playback_tick` (`timeline_commands.rs`). A shared `AuthoredPlayback` option
for Test Movie is W1.12.

**W0.2 · Frame view: Short rows + Snap playhead (TL-09, TL-11)** — **done** (§7). `TimelineView { short_rows,
snap_playhead }`; row height from `TimelineView::row_height()`; `Editor::snap_scrub_frame`.

**W0.3 · Ease panel, single curve (PP-01, TL-06, PR-11)** — **done** (§7). `panels/ease.rs` edits the ease of the
span under the playhead as a draft (egui temp memory) and applies it with one Undo; the Properties Tween section
accepts Y1/Y2 in −100…100 and a Flash acceleration field. Bounce/Elastic presets and per-property curves are W2.2
(§7.4 rows 1–2).

**W0.4 · Rulers and grid view state (ST-01, ST-03, ST-05, ST-06, PP-12, MN-03)** — **done**; see
[NF2-RULERS-GUIDES-GRID.md](NF2-RULERS-GUIDES-GRID.md). View settings live in `stage_view.rs` (`ViewPrefs`), defaults in
`WorkspacePrefs::view_defaults`; the popover is `panels/view_settings.rs`. Original plan:
- UI: `shell.rs › stage_chrome` (pass `Some(..)` to `ZoomControl`, handle `ZoomAction::{Rulers, Grid, ViewSettings}`),
  `panels/menu.rs` View items, new `panels/view_settings.rs` from `planned_panels::view_settings`
  (`ViewSettings` + `ViewSettingsState` + `ViewSettingsResponse`), stage painting in `stage.rs`.
- Engine: none (view state). Guides are W1.2.
- API: `ViewSettings` state in `WorkspacePrefs` (NEW, serde default from `ViewSettingsState::defaults`).
- Tests: stage↔ruler coordinate mapping at zoom 0.25/1/4 and pan; label step Auto selection.
- Registry: `editor-workspace`. Acceptance: spec ST-01/ST-03.

**W0.5 · NF2 stage cursors (ST-07)**
- UI: `stage.rs` / `stage_selection.rs` / `authoring_tools.rs` tool hover code: replace `ctx.set_cursor_icon` over the
  canvas with `neoflash_ui::cursors::show(ctx, canvas, Cursor::by_name(..))`; chrome keeps system cursors.
- Tests: mapping test tool/hover-state → cursor name exists in `cursors::ALL`.
- Registry: `editor-workspace`. Acceptance: spec ST-07.

**W0.6 · Colour effect styles (PR-04)**
- UI: `panels/properties.rs` "Colour effect" section for `ElementKind::Instance` (Style select + value rows using the
  existing `select_row` / `numeric_row` / `paint_row`). If the Style select or Advanced fields are not in the current
  composition, request first (layout reference `panel-properties-movie-clip.png`).
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

**W0.8 · Toasts, progress, spinner (ST-09)** — placement request §4
- UI: `shell.rs` overlay area; `components::{Toast, progress, spinner}`; triggers from save/open/export/import results.
- API: `Editor::notify(level, text, action)` queue (NEW, editor).
- Tests: queue/expiry unit test. Registry: `editor-workspace`. Acceptance: spec ST-09.

**W0.9 · Swatches panel + Color panel Swatches tab (PP-10, CO-03 tab part)**
- UI: new `panels/swatches.rs` from `planned_panels::swatches` (`SwatchesPopover`); Color panel ⋯ Add swatch.
- Engine: none if Document swatches are derived from used colours; persistent document swatches are W1.9.
- API: `Editor::recent_colors` (prefs), `document_colors(&Project) -> Vec<Color>` (editor scan of `ElementKind` paints).
- Tests: scan returns unique colours in stable order. Registry: `editor-paint-effects`. Acceptance: spec PP-10.

**W0.10 · Tools fill/stroke chip → Swatches popover (TO-08)** — approval request §4
- UI: `panels/tools.rs › fill_stroke_chip`. Tests: picking writes `Editor.fill/stroke`. Registry: `editor-workspace`.

**W0.11 · Info panel (PP-11)**
- UI: new `panels/info.rs` from `planned_panels::info`; W/H/X/Y via the Transform panel's selection code
  (`authoring_modify.rs` layout units), cursor from the stage pointer transform, colour from topmost hit element paint.
- Tests: values for a known rectangle at zoom 2. Registry: `editor-workspace`. Acceptance: spec PP-11.

**W0.12 · Project panel (PP-05)**
- UI: new `panels/project.rs` from `planned_panels::project`, re-using `sources.rs` operations (new class, import,
  remove) and selection (`selected_source`); double-click opens Actions ▸ Project files.
- Tests: create/delete source with Undo; linkage footer text from `Symbol.class_name`.
- Registry: `web-project-toolchain`. Acceptance: spec PP-05.

**W0.13 · Code editor comments and folding (AC-04, AC-05)** — **partial**: Apply block/line comment and Remove comment
work; folding (Collapse between braces / Expand all) remains disabled.
- UI: `panels/actions.rs` `PLANNED_CODE_TOOLS` groups 2–3 enabled; logic in `sources.rs › code_editor`.
- Tests: comment toggle round-trip per language; fold ranges from `components::code::tokens`.
- Registry: `web-project-toolchain`. Acceptance: spec AC-04/AC-05.

**W0.14 · Document settings dialog (DL-02)**
- UI: dialog from `planned_dialogs::document_settings` (3.3); "Scale content with stage" transforms root content in
  one transaction; Match contents uses root bounds; Script version row stays informational until W1.12.
- Tests: OK = single Undo; Cancel no change. Registry: `platform-document-model`. Acceptance: spec DL-02.

**W0.15 · Menu items backed by existing commands (MN-01, MN-02 subset, LI-01, PR-06)** — **partial**: Insert ▸ Layer /
Layer folder / Motion guide / tweens / Scene, Edit ▸ Timeline ▸, Control ▸ Go to end / step, Window ▸ Ease / Scene /
Motion editor, Show in library, Clear keyframe, Convert to (blank) keyframes and Select all frames work. Open: Delete /
Duplicate / Swap symbol, Distribute to layers, Modify ▸ Document… (W0.14).
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

**W1.1 · History panel with step names (PP-09)** — E-12
- UI: new `panels/history.rs` from `planned_panels::history`; Edit menu Undo label "Undo <step>".
- Engine: `neoflash-core/src/history.rs`. API (NEW): `History::begin_named(&Project, &str)` or `commit_named`,
  `History::entries() -> impl Iterator<Item=&str>` (undo labels + redo labels + current index), `undo_to(index)`.
  Editor: `Editor::transact_named(label, f)`; existing `transact` call sites get labels gradually ("Edit" fallback).
- Tests: core — labels follow undo/redo/capacity eviction; editor — click step 3 back == 3 × undo.
- Registry: `platform-undo`. Acceptance: spec PP-09.

**W1.2 · Guides (ST-02)** — **done** (E-04): `Timeline::guides`, format version 6, export strips guides; see
[NF2-RULERS-GUIDES-GRID.md](NF2-RULERS-GUIDES-GRID.md). Original plan:
- UI: `stage.rs` guide painting/drag, ruler drag-out, View ▸ Guides items, View settings guide section.
- Engine: `model.rs` NEW `Timeline.guides: Vec<Guide { id, axis, position }>` (authoring-only, serde default,
  format version bump), validation (finite, bounded count), export ignores.
- Tests: save/open round-trip; export JSON contains no guides; Undo per move.
- Registry: `editor-advanced-drawing` (snapping/guides). Acceptance: spec ST-02.

**W1.3 · Snapping (ST-04, TO-06, Preferences ▸ Drawing snapping)** — E-05; **partial**: grid and guide snapping
with the snap ring are done (`snapping.rs`, NF2-RULERS-GUIDES-GRID.md); object/pixel snapping, the tools-rail toggle,
View ▸ Snapping and ⌘-suspend remain (R-003). Original plan:
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

**W1.6 · Gradient fills in the Color panel (CO-01)** — E-07; picker/gradient-bar request §4
- UI: `authoring_modify.rs › fill_type_select` enable Linear/Radial; gradient editing rows.
- Engine: `model.rs` — convert solid-paint shapes to `PaintedPath` or add `fill_gradient` to Rectangle/Ellipse (NEW,
  decide in E-07); validation of stops; export already paints `GradientPaint` (verify for authored primitives).
- Tests: conversion keeps geometry; HTML5 export renders gradient; Undo.
- Registry: `editor-paint-effects`. Acceptance: spec CO-01.

**W1.7 · Stroke styles (PR-03 Solid/Dashed/Dotted, caps, joins, miter, scale, hinting)** — E-06
- UI: Properties fill & stroke rows (`panels/properties.rs`), Color panel stroke section, Stroke style dialog
  (absent → request for dialog composition if no preview exists).
- Engine: NEW `StrokeStyle { cap, join, miter_limit, dash: Option<Vec<f64>>, scale_mode, hinting }` on stroked
  element kinds; native painter (`vector.rs`/`stage_primitives.rs`), HTML5 canvas (`setLineDash`, `lineCap`,
  `lineJoin`), hit testing.
- Tests: core serialization + validation; export snapshot; native/HTML5 visual check.
- Registry: `editor-advanced-drawing`. Acceptance: spec PR-03.

**W1.8 · Primitive tools and PolyStar (TO-01, TO-02, PR-02)** — E-08
- UI: `panels/tools.rs` (add tools per NF2 group order), `authoring_tools.rs`, Properties primitive sections.
- Engine: NEW parametric fields on `Rectangle` (corner radii) and `Ellipse` (start/end angle, inner radius, closed)
  with geometry generation shared by evaluation/export; PolyStar produces `Path` (editor-only).
- Tests: geometry generation unit tests; Break Apart → equivalent path.
- Registry: `editor-primitives`, `editor-advanced-drawing`.

**W1.9 · Document swatches persistence (PP-10, CO-03)** — E-13
- Engine: NEW `Project.swatches: Vec<Color>` (+ gradient presets), serde default. Tests: round-trip, Undo.
- Registry: `editor-paint-effects`.

**W1.10 · Publish settings (DL-03, Preferences ▸ Publish)** — E-15
- UI: dialog from `planned_dialogs::publish_settings`; non-HTML5 targets stay disabled with hint; Properties ▸
  Publish button may then open settings (label stays "Publish HTML5…" unless the design changes it).
- Engine: NEW `Project.publish: PublishSettings { loop, autoplay, scale_mode, include_trace, minify, strict,
  jpeg_quality, html_template }`; `neoflash-export::export_project_with(project, settings, …)` (NEW); web runtime honours
  loop/autoplay/scale.
- Tests: export with loop off stops at end (browser scenario); trace stripped.
- Registry: `web-html5-export`. Acceptance: spec DL-03.

**W1.11 · Authored text options (PR-01 part: type Static/Dynamic/Input, selectable, border, password, max chars)** — E-09
- UI: `text_boxes.rs › text_box_properties` rows existing in the NF2 Text variant composition (request for any row not
  present).
- Engine: NEW `TextBox` fields; export maps to runtime `TextField` (already implemented for scripts).
- Tests: authored Input text editable in HTML5 (browser scenario); AS3 instance name access.
- Registry: `editor-text`.

**W1.12 · Loop/scene-agnostic playback option + document script default** — E-01 — **partial**: editor Loop playback
and Play all scenes are done in the editor (W0.1); the shared `AuthoredPlayback` option and the script default are open.
- Engine: `AuthoredPlayback::set_looping(bool)` (NEW) so preview and Test Movie share semantics; optional
  `Project.default_script_language` for Document settings "Script" row.
- Registry: `animation-lifecycle`.

### Phase 2 — Major features

**W2.1 · Motion tweens + Motion editor + Motion tween properties (TL-05, PP-02, PR-10/PP-03, MN-02)** — E-10 —
**partial** (§7). Done: ◆ property keyframes with independent tracks, motion path on the stage, Motion editor
(`panels/motion_editor.rs`), Properties — Motion tween (`panels/motion_tween.rs`), Copy / Paste motion. Open (§7.4):
per-property eases, moving keys in time, Orient to path for motion tweens, Show path / frame dots, Sync graphic
symbols, Snap, Bézier path editing, Paste motion special. Registry: `animation-motion-tween`, `animation-easing`.

**W2.2 · Ease presets Bounce/Elastic + per-property curves (PP-01 rest)** — E-02 — open; task cards in §7.4 rows 1–2.
Registry: `animation-easing`.

**W2.3 · Motion guides (TL-01, TL-13, DL-16 Guided)** — E-11 — **done** (§7) except the Orient / Snap controls for
guided classic tweens and the Guided type in the Layer properties dialog (§7.4). Registry: `editor-layer-organization`.

**W2.4 · Scenes (TL-03, PP-08, ST-08, Insert ▸ Scene, Control ▸ Test scene / Play all scenes)** — E-16 — **partial**
(§7). Done: `Project.scenes`, Scene select, Insert ▸ Scene, Scene panel, Play all scenes, runtime/HTML5 flattening and
scene-aware AS2/AS3 navigation. Open (§7.4): Control ▸ Test scene, breadcrumb ▾ / Symbols & scenes popover (ST-08).
Registry: `animation-lifecycle`.

**W2.5 · Display & filters (PR-05/PP-14, blend, cache as bitmap, visible)** — E-17
- UI: Properties movie-clip tabs from `planned_panels::properties_filters`.
- Engine: authored `Element.filters: Vec<FilterSpec>`, `blend`, `cache_as_bitmap`, `visible`; native preview
  rasterisation; export into runtime display graph (runtime already applies filters/blend for scripts).
- Registry: `editor-paint-effects`.

**W2.6 · Timeline sound (TL-07, PR-08/PP-13, TL-13 Sound layer, Control ▸ Mute sounds)** — E-18 — **partial** (§7,
decision 0037). Done: `Keyframe.sound`, Flash sync in HTML5 and editor playback, Sound button, Properties ▸ Sound, Sound
layers, Mute sounds. Open (§7.4): Edit envelope (DL-09), Sound properties (DL-08), decoded waveform peaks, legacy FLA
frame sounds. Registry: `animation-media-sync`, `web-media-assets`.

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
**partial**. Done: Edit multiple frames shows and moves (TL-08), Frame view ▸ Preview thumbnails (TL-10,
`timeline_preview.rs`). Open: selecting and scaling/rotating across frames (§7.4), Graphic thumbnail strip (PR-07, can
reuse `timeline_preview.rs`), stage transformation point (ST-11). Registry: `animation-frame-ranges`, `editor-transform`.

### Phase 3 — Import/export, media pipelines, preferences, specialised tools

| Item | Spec | UI source | Engine block | Registry |
| --- | --- | --- | --- | --- |
| W3.1 Export image PNG + SVG | DL-04 | `planned_dialogs::export_image` | E-22 | none (propose under `web-*`/`platform-*` with owner) |
| W3.2 Export image PSD / AI / PDF | DL-04 | same | E-22 | — |
| W3.3 Export video | DL-05 | `planned_dialogs::export_video` | E-23 | — |
| W3.4 Export audio, Sound properties, Edit envelope | DL-06, DL-08, DL-09 | `planned_dialogs::{export_audio, sound_properties}`, `planned_documents::edit_envelope` (task cards §7.4) | E-18, E-23 | `web-media-assets`, `animation-media-sync` |
| W3.5 Import video, Import PSD | DL-07, DL-10 | `planned_dialogs::import_video` | E-19, E-24 | `web-media-assets`, `imports-*` |
| W3.6 Preferences persistence (all pages) | DL-01 | `planned_dialogs::preferences` | E-25 | `editor-workspace`, `platform-recovery` (autosave) |
| W3.7 Shortcut editor | DL-01 Shortcuts | `prefs_shortcuts` | E-25 (command registry) | `editor-workspace` |
| W3.8 Font embedding + text embed rows | DL-11, PR-01 | absent → request | E-26 | `web-font-portability` |
| W3.9 Deco, Spray, Bone/IK | TO-03…05 | tools rail → request for panels | E-27, E-28 | `editor-advanced-drawing` |
| W3.10 9-slice, sharing, accessibility extras, Script assist, Bitmap fill | DL-15, DL-17, AC-07, CO-02 | various | E-29 | `editor-symbols`, `platform-accessibility`, `editor-paint-effects` |

Preferences ▸ Updates must stay disabled (no update service, no telemetry) until `platform-release` /
`platform-extensions` define them; never send usage data.

## 6. Dependency order

```text
Done: W0.1, W0.2, W0.3, W0.4, W1.2, W1.4, W2.3 (rest in §7.4)
Partial: W0.13, W0.15, W1.3, W1.12, W2.1, W2.4, W2.6, W2.8, W2.10
W1.3 snapping (objects/pixels) ◄── R-003
W2.2 presets/per-property ──► W2.1 rest (Motion editor Ease column, Property select in Ease)
W0.7 output levels ──► W1.5 compiler errors ──► W2.8 code intelligence ──► W2.9 debugger + trace bridge
W1.1 history names ──► (Edit menu labels, Preferences undo levels in W3.6)
W0.9 swatches (derived) ──► W1.9 document swatches ──► W1.6 gradients (Gradients section)
W1.7 stroke styles, W1.8 primitives ──► W3.2 vector exports (SVG/AI/PDF need them)
W1.10 publish settings ──► W3.1–W3.3 exports (shared range/size options)
W2.4 scenes ──► breadcrumb sections (ST-08), Test scene, Export "whole movie (all scenes)"
W2.6 timeline sound ──► W3.4 Edit envelope / Sound properties / audio export ──► W3.3 video export audio track
W2.7 authored video ──► W3.5 import video
```

Recommended batch order for the open items (breadth first): Batch A = W0.5–W0.12, W0.14–W0.16 and the rest of W0.13 /
W0.15; Batch B = W1.1, W1.3 rest, W1.5 (after requests are answered); Batch C = W1.6–W1.12; the timeline task cards of
§7.4 as one or two coherent batches; then the remaining Phase 2 items each as one batch with its UI, engine, export and
tests; Phase 3 last.

## 7. Timeline: current state and remaining work

Read this before touching the timeline. AGENTS.md › "Timeline engine" is the short version of §7.1–§7.3.

### 7.1 Document model (format 6, `crates/neoflash-core/src/model.rs`)

| Field | Meaning |
| --- | --- |
| `Keyframe.tween: Option<Easing>` | The span starting here is tweened with this ease. `Easing` = `Linear`, `EaseIn`, `EaseOut`, `EaseInOut`, `CubicBezier { x1, y1, x2, y2 }` (x ∈ 0…1, \|y\| ≤ 100), `Flash { acceleration }` (−100…100). |
| `Keyframe.tween_kind: TweenKind` | `Classic` (default, omitted in JSON) interpolates poses of elements with the same ID; `Shape` morphs the layer's shapes; `Motion` = motion tween with property tracks. |
| `Keyframe.keyed_properties: Option<Vec<TweenProperty>>` | `Some` marks a ◆ property keyframe inside a motion span. It stores a full pose, but only the listed properties (`X`, `Y`, `ScaleX`, `ScaleY`, `Rotation`, `Skew`, `Alpha`, `Color`) are keys. `None` = an ordinary keyframe that starts a new span. |
| `Keyframe.orient_to_path: bool` | Turn the tweened object with the path tangent. Evaluated for guided classic tweens (`sample_layer_in`, guide tangent) and motion tweens (`sample_motion`, X/Y track direction). |
| `Keyframe.sound: Option<FrameSound>` | Frame sound: `resource` (an `audio/*` `ResourceUnit`), `sync` (`Event`/`Start`/`Stop`/`Stream`), `loops` (1–10 000), `loop_forever`, `effect` (Flash preset envelope). A layer whose keys hold sounds and no artwork is shown as a Sound layer (derived, not stored). |
| `LayerProperties.motion_guide: Option<Id>` | A `Normal` layer guided by a `LayerRole::Guide` layer of the same timeline. The guide's first stroke is the path. |
| `Project.scenes: Vec<Id>` | Scene timelines in play order (MovieClip symbol IDs). Empty = one scene, `root`; otherwise `scenes[0] == root`. Scene symbols are not library items and are never instantiated. |
| `Timeline.guides: Vec<Guide>` | Ruler guides (authoring only, stripped from HTML5). See [NF2-RULERS-GUIDES-GRID.md](NF2-RULERS-GUIDES-GRID.md). |

`Project::uses_format_6()` detects these features; `upgrade_format_for_features()` raises `format_version` to 6;
validation (`validation.rs`) accepts versions 1–6, rejects format-6 features in older versions, ◆ keys outside a
motion span, motion guides that do not point to a guide layer, scene lists whose first entry is not `root`, duplicate
or non-MovieClip scenes, instances of scene symbols and frame sounds without an audio resource. Every new field is
`#[serde(default, skip_serializing_if = …)]`, so documents that do not use it serialize as before.

### 7.2 Where each behaviour lives

| File | Responsibility |
| --- | --- |
| `neoflash-core/src/evaluate.rs` | `sample_layer` dispatches by tween kind: Motion → `sample_motion` (each property interpolates through the ◆ keys that key it; the span ease remaps time over the whole span; after its last key a track holds), Shape → `shape_tween::shape_tween_elements`, Classic → identity interpolation. `motion_span(layer, frame)` returns the start key, its ◆ keys and the span end. `sample_layer_in(timeline, layer, frame)` adds the motion guide (`guide_polyline`, `GuidePath`) and Orient to path; the evaluation walker uses it. |
| `neoflash-core/src/shape_tween.rs` | Shape morph: shapes pair by ID, then by order; equal command structure interpolates exactly; other contours are resampled by arc length to a common cubic count with start-point/direction alignment; unpaired shapes and contours grow/shrink from their centre; fills, strokes and compatible gradients blend. |
| `neoflash-core/src/timeline_edit.rs` | Range edits (`TimelineEdit`), including `Reverse`, `ConvertToKeyframes` and `SyncSymbols`. Every edit ends with `Layer::normalize_motion_keys`, so a cut through a motion tween never leaves orphan ◆ keys. |
| `neoflash-core/src/scenes.rs` | `scene_ranges`, `flatten_scenes` / `flatten_scenes_with_ranges` (concatenate scenes into one main timeline with fresh layer IDs), `resolve_goto` (AS3/JS frames are scene-relative, AS2 frames absolute unless a scene is named), `scene_step` (`nextScene` / `prevScene`). |
| `neoflash-core/src/timeline_sound.rs` | `SoundState` inside `Player` turns keyframe sounds into `SoundCommand`s (`Play`, `Stop`, `StreamStart`, `StreamStop`, `StopAll`) when frames are entered; `envelope_gains`; `sound_span_end`. Never called from pure evaluation. |
| `neoflash-core/src/runtime.rs` | `Player::set_scenes`, `resolve_goto`, `resolve_scene_step`, `drain_sound_commands`, `stop_all_sounds`. |
| `neoflash-export/src/lib.rs`, `neoflash-web/src/lib.rs` | The **scene boundary**: scripts compile against the authored scenes (diagnostics name the scene), then the published bundle holds the flattened project plus `scenes` ranges; `WebPlayer::new` flattens an authored project or accepts the bundle's ranges. Nothing else flattens. |
| `web/audio.js`, `web/runtime.js` | Decode each timeline sound resource once and schedule `sound_commands_json()` on Web Audio (buffer sources + gain automation); `stopAllSounds` / `SoundMixer.stopAll` call `stop_all_sounds`. |
| `neoflash-editor/src/timeline_commands.rs` | `Editor::commit_history` and `record_motion_property_keys`; tween create/remove/toggle, `add_motion_guide`, `clear_keyframe`, `convert_to_keyframes`, `reverse_frames`, `sync_symbols`, scenes (`scene_list`, `switch_scene`, `new_scene`, `delete_scene`), playback (`after_playback_tick`, `rewind_if_finished`), `snap_scrub_frame`, Edit multiple frames (`edit_multiple_key_frames`, `move_on_other_keyframes`). |
| `neoflash-editor/src/timeline_motion.rs` | Motion path on the stage (`motion_path`, `paint_motion_path`, ◆ key drag with one Undo), Copy / Paste motion. |
| `neoflash-editor/src/timeline_preview.rs` | Frame view ▸ Preview thumbnails, cached per keyframe and `History::revision`. |
| `neoflash-editor/src/timeline_sound.rs` | Attach/import/edit frame sounds; native preview: decodes with rodio/Symphonia (MP3, WAV) and plays the preview `Player`'s `SoundCommand`s; Mute sounds. Scrubbing is silent. |
| `neoflash-editor/src/panels/` | `timeline_controls.rs` (transport, tween/ease/sound buttons, scene select, frame view menu), `timeline.rs` (frame menu), `layer_controls.rs` (layer menu, Add classic motion guide), `ease.rs`, `scene.rs`, `motion_editor.rs`, `motion_tween.rs` (also the motion-span edits shared with the Motion editor), `properties_sound.rs`, `menu.rs` (Insert / Edit ▸ Timeline / Control / Window). |
| `neoflash-editor/src/workspace.rs` | `TimelineView { tinted, show_seconds, short_rows, snap_playhead, loop_playback, preview }` (saved with the workspace; `row_height()` feeds painters and `timeline_cell_at`); `PanelId::{Ease, Scene, MotionEditor}` start hidden. Session-only state (`edit_multiple`, `play_all_scenes`) lives in `timeline_edit.rs::TimelineSelection`. |

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

### 7.4 Remaining timeline work (task cards)

Each row is one delivery unless grouped. Do not change visuals (§1); file a request when the design is missing. Every
model change follows §1.7: serde default, one format bump per batch (the next is **7**: add `uses_format_7` beside
`uses_format_6` and extend validation's accepted range), validation, an old-document load test.

| # | Work (spec) | Files to touch | Model change | Tests to add | Acceptance |
| --- | --- | --- | --- | --- | --- |
| 1 | Bounce / Elastic ease presets (PP-01, E-02, W2.2) | `model.rs` (`Easing`), `evaluate.rs::easing`, reuse `as_easing.rs::ease` (Penner, provenance in `RUNTIME-EASING-PROVENANCE.md`), `validation.rs`, `panels/ease.rs` (enable presets), Properties Tween select | NEW `Easing::Preset { family, mode }` (or `Back/Bounce/Elastic` variants); format 7 | core: `easing(0)=0`, `easing(1)=1`, values equal `as_easing::ease`; editor: Apply/Cancel Undo in `timeline_panels_tests.rs`; JSON round-trip | Bounce out on a classic and a motion tween bounces in editor playback and HTML5; Cancel leaves `History` unchanged |
| 2 | Per-property eases (PP-01 Property select, PP-02 Ease column, E-02/E-10) | `model.rs`, `evaluate.rs::sample_motion`, `validation.rs`, `panels/ease.rs`, `panels/motion_editor.rs` (Ease column, now disabled "Per-property ease") | NEW `Keyframe.property_eases: Vec<(TweenProperty, Easing)>` on the motion span start key; a property without an entry uses `tween`; format 7 | core: an X ease differs from the Y track over one span; validation rejects it on non-motion keys; editor: Motion editor ease edit is one Undo | Motion editor and Ease (One curve for all off) edit one property's curve; HTML5 matches editor |
| 3 | Move keys in time in the Motion editor (PP-02) | `panels/motion_editor.rs`, `panels/motion_tween.rs` (NEW `move_motion_key(span, property, from, to)`) | none (moves the property between ◆ keys; creates/removes ◆ keyframes) | `timeline_panels_tests.rs`: drag moves one property's key, other tracks unchanged, one Undo | Dragging a ◆ horizontally on a lane retimes only that property |
| 4 | ~~Orient to path for motion tweens~~ | Done: `sample_motion` turns the object with its X/Y track direction (test `motion_tween_orient_to_path_turns_with_the_direction_of_travel`). | | | |
| 5 | Orient / Snap controls for guided classic tweens (TL-01, PR-10) | `panels/properties.rs` Tween section | none (`orient_to_path` exists; `Editor::set_orient_to_path` accepts any tweened span) | editor test toggling the control | **Needs a design request first**: the classic Tween section has no slot for these rows |
| 6 | Show path / Show frame dots (PR-10) | `panels/motion_tween.rs` (checkboxes, disabled), `timeline_motion.rs::paint_motion_path`, `workspace.rs` | none (editor view state; the path is always painted today) | editor test: hidden path is not hit-tested | Unchecking hides the path or its dots; the setting persists |
| 7 | Sync graphic symbols and Snap to motion guide options (PR-10) | `panels/motion_tween.rs`, `evaluate.rs` (graphic first frame in a motion span), `timeline_edit.rs::SyncSymbols` | decide: flag on the span start key (format 7) or a one-shot edit; check `evaluate::placement_start` first | core + editor tests per option | Graphic instances in a motion span play continuously; Snap keeps path ends on a guide |
| 8 | Paste motion special (MN-02, R-006) | after the design answer: new dialog, `timeline_motion.rs` (`paste_motion` with options) | none | `timeline_motion_tests.rs`: only the chosen properties change, one Undo | Blocked on R-006 |
| 9 | Bézier motion path editing (TL-05) | `timeline_motion.rs`, `evaluate.rs::sample_motion` (position along the path by arc length) | NEW `Keyframe.motion_path: Option<Vec<PathCommand>>` on the span start key; format 7 | core: position follows a curved path; editor: bending is one Undo | Dragging the path between keys bends it; **request** a curve-handle style (R-005 answer covers ◆ handles only) |
| 10 | Shape hints (TL-04, View ▸ Show shape hints ⌥⌘H, Modify ▸ Shape ▸ Add shape hint) | `model.rs`, `shape_tween.rs` (pin correspondence), `menu.rs`, `authoring_modify.rs`, stage overlay | NEW `Keyframe.shape_hints: Vec<ShapeHint { id, start: [f64; 2], end: [f64; 2] }>` on the start key; format 7 | core: hints pin the morph; editor: add/move/remove hint, one Undo | **Request** the NF2 hint marker first; hints change the in-between frames as in Flash |
| 11 | Edit envelope (DL-09) | `model.rs` (`FrameSound`), `timeline_sound.rs` (core `envelope()` already takes points), `validation.rs`, `panels/properties_sound.rs` (Envelope…), dialog from `planned_documents::edit_envelope` using `neoflash_ui::components::EnvelopeLane` | NEW `FrameSound.envelope: Option<Vec<[f64; 3]>>` (`position`, `left`, `right`), `SoundEffect::Custom`; format 7 | core `timeline_sound.rs`: custom gains reach `SoundCommand::Play`; `web/tests/wasm.test.cjs`: envelope scheduled; editor: OK is one Undo, Cancel none | A custom fade plays the same in editor playback and HTML5 |
| 12 | Sound properties (DL-08) | dialog from `planned_dialogs::sound_properties`, `resources.rs` | none for name/info/Update/Import/Test; export compression stays disabled (no encoder) | editor test: Update replaces the resource bytes with one Undo | Opens from Properties ▸ Sound; compression rows stay disabled with their hint |
| 13 | Decoded waveform peaks in Sound spans (TL-07, TL-13) | editor `timeline_sound.rs` (decode peaks once per resource), `panels/timeline.rs` | none | editor test: peaks cached per resource, not per repaint | **Request** a peaks-taking span painter (today `paint_span(SpanType::Sound)` draws a decorative waveform) |
| 14 | Legacy FLA frame sounds (`fla_frame_sound`, `fla_media_not_imported`) | `neoflash-import/src/legacy/convert.rs`, `legacy/convert/assets.rs` | none (map to `Keyframe.sound` + audio `ResourceUnit`) | `crates/neoflash-import/tests/binary_timeline.rs`; corpus check | Imported frame sounds play with their sync; undecodable media keeps the warning |
| 15 | Edit multiple frames, rest (TL-08) | `timeline_commands.rs` (pattern: `move_on_other_keyframes`), `stage_selection.rs` | none | `timeline_commands_tests.rs`: scale/rotate on every keyframe in range, one Undo | Objects existing only on other keyframes can be selected; scale/rotate apply across the range |
| 16 | Control ▸ Test scene ⌥⌘↵ (MN-01, E-16) | `panels/menu.rs` (planned item), export call | none (export a clone whose `scenes` is empty and `root` is the current scene) | export test: only that scene's frames are published | Test scene plays only the edited scene in the browser |
| 17 | Breadcrumb ▾ and Symbols & scenes popover (ST-08) | `shell.rs` stage chrome, existing `BreadcrumbItem { menu }` | none | editor test: switching scene from the popover | Lists scenes and symbols with search; keyboard ↑↓ ↵ |
| 18 | Range edits inside tweens keep editable curves (`animation-frame-ranges`) | `timeline_edit.rs` | none or `property_eases` (row 2) | core: cutting inside a classic/motion tween keeps an equivalent ease instead of baking | No integer-frame baking for supported cases |
| 19 | XFL import of tween kinds, guides and scenes | `neoflash-import/src/xfl.rs` | none (map `tweenType="shape"/"motion"`, guide layers + `parentLayerIndex`, several `DOMTimeline`s) | `crates/neoflash-import/tests/` fixtures | XFL documents keep tweens, guides and scenes instead of blocking |
| 20 | Layer properties dialog: Guided type (DL-16) | dialog from `PlannedPanel::LayerProperties`, `layer_controls.rs` | none (`motion_guide`) | editor test: setting Guided links the layer, one Undo | Type Guided ↔ `motion_guide` stay consistent |
| 21 | Frame view ▸ Help (F1) (TL-12) | `panels/timeline_controls.rs` | none | — | Needs a product decision on the help target; stays disabled until then |

Related stage work outside the timeline: object and pixel snapping and View ▸ Snapping (ST-04, R-003) extend
`snapping.rs` with object candidates (bounds corners and centres on visible, unlocked layers) and pixel rounding; the
grid/guide rules in [NF2-RULERS-GUIDES-GRID.md](NF2-RULERS-GUIDES-GRID.md) must keep passing.
