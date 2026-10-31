# NF2 wiring plan — connecting functionality to the existing UI

Readers: implementation agents (Codex) and the project owner. This page orders the work that turns disabled NF2
controls and planned previews into working features. It covers the **UI ↔ editor ↔ engine connections** only.

- What each element must do: [NF2-FUNCTIONAL-SPEC.md](NF2-FUNCTIONAL-SPEC.md) (IDs such as `TL-02`, `PP-09` below
  refer to it).
- Engine design behind each block (model, evaluation, runtime, export, script, algorithms):
  [NF2-ENGINE-PLAN.md](NF2-ENGINE-PLAN.md) (blocks `E-…`).
- Design gaps: [NF2-REQUESTS.md](NF2-REQUESTS.md).

Planned previews are being given real entry points (menu items, buttons) in parallel. This page refers to "its menu
item / button" generically; when a preview becomes a real panel, the same entry point opens the real panel.

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
   touch `History`.

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

1. Create `panels/<name>.rs`; move the body function from `planned_panels.rs` / `planned_motion.rs` unchanged, then
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
| W0.4 rulers/grid, W1.2 guides | `NF2/Stage Ruler`, `NF2/Ruler Corner`, `NF2/Guide`, `NF2/Stage Grid` painters in `neoflash-ui` (tokens `color/guide`, `color/stage-grid` exist; components do not). |
| W1.3 snapping | `NF2/Snap Ring` painter. |
| W1.6 gradients, W0.9 colour, W3.4 envelope | `NF2/Gradient Bar` + `Gradient Stop`, `NF2/Color Picker`, `NF2/Anchor Point` + envelope lane: being added by the design track (`components/{gradient, color_picker, envelope}.rs`, uncommitted on 2026-11-01). Use them once the design lock is re-pinned; request only what is still missing. |
| W0.8 toasts | Toast placement/stacking and auto-dismiss timing (not specified in §4). |
| W0.10 swatches chip | Approval to replace the egui RGB picker in the Tools fill/stroke popover with `SwatchesPopover` (changes the component used). |
| W0.2 frame view | Whether Placement / Preview in context / Help / Close rows may be added to the frame-view ⋯ menu (TL-12). |
| W0.15 menus | Confirm order/grouping for absent §8 menu items and View ▸ Grid/Guides/Snapping submenus. |
| W2.x canvas chrome | NF2 painters for Selection Box variants, Transform Box, transformation point, gradient focal handle (ST-11) before restyling stage overlays. |
| W1.1 History | Visual state for "steps after current" (dimmed?) in the History list. |
| W3.6 Preferences | Accent colour / language / custom syntax colours imply new token behaviour — confirm or keep disabled. |

## 5. Work items

Format: **UI** → panel file/function · **Engine** → crate/module · **API** (NEW = proposal) · **Tests** · **Registry**
(id in `docs/progress/product.json`) · **Acceptance** (short; full criteria in the spec).

### Phase 0 — UI/editor-only (no engine change)

**W0.1 · Loop playback (TL-02, Control ▸ Loop playback)**
- UI: `timeline_controls.rs › timeline_transport`; Control menu item (MN-01).
- Engine: none (editor stops on wrap in `preview.rs::tick_authored_preview`); optional later `AuthoredPlayback`
  option (E-01).
- API: `Editor::loop_playback: bool` persisted in `WorkspacePrefs` (NEW field, serde default `true`).
- Tests: editor test — loop off, play from last-1, tick → `playing == false`, frame == last.
- Registry: `animation-onion` (scrubbing/playback) or `editor-workspace`. Acceptance: stops at end when off.

**W0.2 · Frame view: Short rows + Snap playhead (TL-09, TL-11)**
- UI: `timeline_controls.rs › timeline_frame_view_menu`; `timeline_paint.rs` `ROW_HEIGHT` const → per-view value
  passed to painters/hit tests (`timeline_cell_at`, layer drag, `LayerRow::show`).
- API: `FrameViewOptions { short_rows, snap_playhead }` (extend the existing struct; persist in workspace prefs).
- Tests: hit-test unit tests at 18 pt rows; scrub snap test.
- Registry: `editor-workspace`. Acceptance: spec TL-09/TL-11.

**W0.3 · Ease panel, single curve (PP-01, TL-06, PR-11)**
- UI: new `panels/ease.rs` from `planned_motion::ease`; enable the Ease button in `timeline_frame_tools` when the
  current frame is in a tweened span; Properties Tween section: widen Y1/Y2 range to the model's limits (|y| ≤ 100,
  graph −0.3…1.3) and expose `Easing::Flash { acceleration }` in the existing select/field pattern.
- Engine: none for Linear/EaseIn/Out/InOut/Back/Custom (Back = cubic with y outside 0–1). Bounce/Elastic presets stay
  disabled with a hint until E-02.
- API: `Editor::ease_draft: Option<EaseDraft { symbol, layer_id, key_frame, easing }>`; Apply → `transact`.
- Tests: Apply/Cancel Undo test; overshoot round-trip through `Project::to_json`/`from_json`; `evaluate::easing` value
  > 1 for Back preset.
- Registry: `animation-easing`. Acceptance: spec PP-01.

**W0.4 · Rulers and grid view state (ST-01, ST-03, ST-05, ST-06, PP-12, MN-03)** — blocked on design request §4
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

**W0.13 · Code editor comments and folding (AC-04, AC-05)**
- UI: `panels/actions.rs` `PLANNED_CODE_TOOLS` groups 2–3 enabled; logic in `sources.rs › code_editor`.
- Tests: comment toggle round-trip per language; fold ranges from `components::code::tokens`.
- Registry: `web-project-toolchain`. Acceptance: spec AC-04/AC-05.

**W0.14 · Document settings dialog (DL-02)**
- UI: dialog from `planned_dialogs::document_settings` (3.3); "Scale content with stage" transforms root content in
  one transaction; Match contents uses root bounds; Script version row stays informational until W1.12.
- Tests: OK = single Undo; Cancel no change. Registry: `platform-document-model`. Acceptance: spec DL-02.

**W0.15 · Menu items backed by existing commands (MN-01, MN-02 subset, LI-01, PR-06)** — confirmation request §4
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

**W1.2 · Guides (ST-02)** — E-04; blocked on design request §4
- UI: `stage.rs` guide painting/drag, ruler drag-out, View ▸ Guides items, View settings guide section.
- Engine: `model.rs` NEW `Timeline.guides: Vec<Guide { id, axis, position }>` (authoring-only, serde default,
  format version bump), validation (finite, bounded count), export ignores.
- Tests: save/open round-trip; export JSON contains no guides; Undo per move.
- Registry: `editor-advanced-drawing` (snapping/guides). Acceptance: spec ST-02.

**W1.3 · Snapping (ST-04, TO-06, Preferences ▸ Drawing snapping)** — E-05
- UI: tools rail snap toggle (if present in composition; else request), View ▸ Snapping, zoom-control Snap toggle,
  stage drag/transform/draw code paths (`stage_selection.rs`, `transform_geometry.rs`, `vector.rs`).
- Engine: none required (geometry in editor); optional shared `neoflash-core::geometry` helper for bounds/anchor
  candidates.
- Tests: snap-to-grid, snap-to-guide, snap-to-object-centre unit tests; ⌘ suspends snapping.
- Registry: `editor-advanced-drawing`. Acceptance: spec ST-04.

**W1.4 · Tween kind + Shape tween (TL-04)** — E-03
- UI: `timeline_controls.rs › timeline_frame_tools` (enable item 2), frame context menu "Create shape tween",
  span painter uses `SpanType` per kind (`neoflash-ui` painters exist).
- Engine: `model.rs` NEW `Keyframe.tween_kind: TweenKind { Classic, Shape, Motion }` (serde default Classic, format
  bump); `evaluate.rs` uses it to pick interpolation; diagnostic for incompatible content.
- Tests: core interpolation of matching topology; diagnostic for mismatch; old documents default to Classic.
- Registry: `animation-shape-tween`. Acceptance: spec TL-04.

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

**W1.12 · Loop/scene-agnostic playback option + document script default** — E-01
- Engine: `AuthoredPlayback::set_looping(bool)` (NEW) so preview and Test Movie share semantics; optional
  `Project.default_script_language` for Document settings "Script" row.
- Registry: `animation-lifecycle`.

### Phase 2 — Major features

**W2.1 · Motion tweens + Motion editor + Motion tween properties (TL-05, PP-02, PR-10/PP-03, MN-02)** — E-10
- UI: `timeline_controls.rs` tween item 3; new `panels/motion_editor.rs` and Properties motion-tween section from
  `planned_motion::{motion_editor, motion_tween}`; ◆ markers via `paint_marker(MarkerKind::PropertyKeyframe)`;
  on-stage path editing (`stage.rs`).
- Engine: per-property tracks, spatial path, orient to path, rotation count/direction, per-property eases.
- Tests: core track evaluation; export parity (browser); Undo for key edits.
- Registry: `animation-motion-tween`, `animation-easing`.

**W2.2 · Ease presets Bounce/Elastic + per-property curves (PP-01 rest)** — E-02. Registry: `animation-easing`.

**W2.3 · Motion guides (TL-01, TL-13, DL-16 Guided)** — E-11
- UI: `timeline_layer_actions` (enable Add motion guide), layer context menu, layer properties type.
- Engine: `LayerRole::MotionGuide` + guided link; path sampling; orient to path. Registry: `editor-layer-organization`.

**W2.4 · Scenes (TL-03, PP-08, ST-08, Insert ▸ Scene, Control ▸ Test scene / Play all scenes)** — E-16
- UI: `timeline_scene_select`, new `panels/scene.rs` from `planned_panels::scene`, breadcrumb ▾.
- Engine: scene list, playback across scenes, AS2/AS3 scene APIs, export. Registry: `animation-lifecycle`.

**W2.5 · Display & filters (PR-05/PP-14, blend, cache as bitmap, visible)** — E-17
- UI: Properties movie-clip tabs from `planned_panels::properties_filters`.
- Engine: authored `Element.filters: Vec<FilterSpec>`, `blend`, `cache_as_bitmap`, `visible`; native preview
  rasterisation; export into runtime display graph (runtime already applies filters/blend for scripts).
- Registry: `editor-paint-effects`.

**W2.6 · Timeline sound (TL-07, PR-08/PP-13, TL-13 Sound layer, Control ▸ Mute sounds)** — E-18
- UI: Sound button, Properties — Sound from `planned_panels::properties_sound`, waveform spans.
- Engine: keyframe sound reference + sync/effect/loop; editor audio preview; Web Audio scheduling in export.
- Registry: `animation-media-sync`, `web-media-assets`.

**W2.7 · Authored video (PR-09, TL-13 Video layer)** — E-19. Registry: `animation-media-sync`, `web-media-assets`.

**W2.8 · Code hints, toolbox, navigator, auto-format (AC-01, AC-02, AC-06, AC-08 rest)** — E-20
- UI: `panels/actions.rs` toolbar groups 1 and 4; sidebar.
- Engine: `neoflash-script` completion/symbol API + formatter. Registry: `web-project-toolchain`.

**W2.9 · Debugger + breakpoints + Output trace bridge + Test Movie window (PP-04, AC-03, OU-02, DL-12)** — E-21
- UI: new `panels/debugger.rs` from `planned_panels::debugger`; gutter breakpoints; Output Trace level.
- Engine: debug protocol from the HTML5 runtime to the editor; source maps. Registry: `web-project-toolchain`.

**W2.10 · Edit multiple frames + Preview thumbnails + transformation point (TL-08, TL-10, PR-07, ST-11)** — editor
- UI: onion group button; frame view Preview; Graphic thumbnail strip; stage transform point.
- Engine: none (thumbnails from `evaluate`); custom pivot may need `Transform` pivot field (E-10 note).
- Registry: `animation-frame-ranges`, `editor-transform`.

### Phase 3 — Import/export, media pipelines, preferences, specialised tools

| Item | Spec | UI source | Engine block | Registry |
| --- | --- | --- | --- | --- |
| W3.1 Export image PNG + SVG | DL-04 | `planned_dialogs::export_image` | E-22 | none (propose under `web-*`/`platform-*` with owner) |
| W3.2 Export image PSD / AI / PDF | DL-04 | same | E-22 | — |
| W3.3 Export video | DL-05 | `planned_dialogs::export_video` | E-23 | — |
| W3.4 Export audio, Sound properties, Edit envelope | DL-06, DL-08, DL-09 | `planned_dialogs::{export_audio, sound_properties}` | E-18, E-23 | `web-media-assets` |
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
Design requests (§4) ──► W0.4 rulers/grid ──► W1.2 guides ──► W1.3 snapping
W0.3 ease (single curve) ──► W2.2 presets/per-property ──► W2.1 motion tweens ──► W2.3 motion guides
W1.4 tween kind ──► W2.1 motion tweens (TweenKind::Motion)
W0.7 output levels ──► W1.5 compiler errors ──► W2.8 code intelligence ──► W2.9 debugger + trace bridge
W1.1 history names ──► (Edit menu labels, Preferences undo levels in W3.6)
W0.9 swatches (derived) ──► W1.9 document swatches ──► W1.6 gradients (Gradients section)
W1.7 stroke styles, W1.8 primitives ──► W3.2 vector exports (SVG/AI/PDF need them)
W1.10 publish settings ──► W3.1–W3.3 exports (shared range/size options)
W2.4 scenes ──► Scene panel, breadcrumb sections, Test scene, Export "whole movie (all scenes)"
W2.6 timeline sound ──► W3.4 audio export/envelope ──► W3.3 video export audio track
W2.7 authored video ──► W3.5 import video
```

Recommended batch order (breadth first): Batch A = W0.1–W0.3, W0.5–W0.7, W0.11–W0.15; Batch B = W0.4, W0.8–W0.10,
W1.1–W1.5 (after requests are answered); Batch C = W1.6–W1.12; then Phase 2 items each as one coherent batch with its
UI, engine, export and tests; Phase 3 last.
