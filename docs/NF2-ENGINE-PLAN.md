# NF2 engine plan — engine work behind the designed UI

Readers: implementation agents (Codex) and the project owner. The NF2 interface is complete and leads; the engine
follows. This page lists, for every functional block the design shows and the engine lacks, the model, evaluation,
runtime, export, script and editor work, with implementation recommendations.

- Element behaviour: [NF2-FUNCTIONAL-SPEC.md](NF2-FUNCTIONAL-SPEC.md) (IDs `TL-…`, `PR-…`, `PP-…`, `DL-…`).
- UI connection order and per-item checklist: [NF2-WIRING-PLAN.md](NF2-WIRING-PLAN.md) (items `W…`).
- Design gaps: [NF2-REQUESTS.md](NF2-REQUESTS.md).

Written against branch `codex/native-design-system` (2026-11-01); the **Status** line of each block reflects `main` after
the timeline, scene, sound and rulers batches. Claims about existing code cite the file; anything not confirmed is
marked **verify**. Blocks marked done keep their original plan below the status line for context; where the
implementation chose a different shape, the status line names the real types. Current timeline architecture and open
task cards: [NF2-WIRING-PLAN.md](NF2-WIRING-PLAN.md) §7.

## 0. Hard rules (repeat of AGENTS.md › "Design system (NF2) — locked")

1. **No redesign.** Never edit `interface/`, `crates/neoflash-ui/`, `docs/DESIGN-SYSTEM.md` or
   `interface/design-system/DESIGN-LOCK.json`. No new colours, sizes, fonts, icons, labels, tooltips, components or
   hand-styled egui widgets in the editor.
2. **Connect in place.** Enable the existing disabled control (drop only its "not available yet" suffix) and convert
   planned previews (`crates/neoflash-editor/src/panels/planned_*.rs`) into real panels/dialogs by moving their bodies
   unchanged and binding data.
3. **Design gaps → `docs/NF2-REQUESTS.md`**, then continue with the parts that do not depend on the answer.
4. `python3 scripts/design-lock.py` must pass (it runs in `scripts/check.sh`).
5. AGENTS.md engineering rules: breadth first; honest diagnostics (never silently drop content or code); serialized
   document independent of UI, renderer and filesystem paths; authored evaluation side-effect free (scrubbing, onion
   skin); browser adapters marshal data and call browser facilities, Rust owns the algorithms; adapt compatible
   open-source algorithms with licences and pinned provenance, never embed complete third-party ActionScript engines
   or add foreign-language runtime implementations; update `docs/progress/*.json` + `python3 scripts/progress.py
   --write-reports`, CHANGELOG, README/ROADMAP/COMPATIBILITY; percentages describe the registry, not Flash parity.

## 1. Engine architecture facts that shape every block

| Fact | Where | Consequence |
| --- | --- | --- |
| One document model, serde JSON, `.nflash`; `format_version` 1..=6 accepted; features gate on version | `neoflash-core/src/model.rs` (`Project::uses_format_6`, `upgrade_format_for_features`), `validation.rs` (`matches!(format_version, 1..=6)`) | New fields: `#[serde(default, skip_serializing_if = …)]`, raise the accepted range to 7 with the next batch, validation "field X requires format N", writer bumps only when the feature is used (`upgrade_format_for_features`, called by `Editor::commit_history`). Old files must load byte-for-byte equivalent in meaning. Bump **once per delivery batch**, not per field. |
| Authored evaluation is pure | `evaluate.rs::evaluate`, `sample_layer`, `authored_layers.rs::evaluate_scene` | All timeline semantics (tweens, guides, scenes) are functions of `(Project, symbol, frame)`; never cache mutable state in the document. |
| One `Player` runs both the editor preview and HTML5 | `runtime.rs` (`Player`), `authored_playback.rs` (editor), `neoflash-web/src/lib.rs::WebPlayer` (WASM) | Evaluation changes in core reach HTML5 automatically through WASM; drawing changes need the canvas painter in `web/path.js`, `web/runtime.js`, `web/text.js` (strokes hard-code round caps/joins today) and the native painter (`neoflash-editor/src/stage_primitives.rs`, `vector.rs`). |
| Export packages project JSON + compiled scripts + runtime | `neoflash-export/src/lib.rs::export_project`, `export_warnings` | Export options need a settings struct; unsupported features add `export_warnings`. |
| Scripts compile per unit; diagnostics are `{language, line, column, message}` | `neoflash-script/src/lib.rs::Diagnostic`, `actionscript/project.rs::compile_project` | Severity/code/location need additive fields. |
| Snapshot undo, capacity 150 in the editor | `history.rs::History` | Named steps are a small additive change; command-log undo is a larger optional redesign (E-12). |
| Runtime already has filters, blend, ColorTransform, gradients, Sound, Video, Penner easing | `filters.rs`, `runtime_graph/{filters,as2_render,transform,video}.rs`, `as_easing.rs`, `web/{audio,video,filters}.js` | Reuse them for authored features instead of writing second implementations. |

## 2. Order (dependencies × product value, breadth first)

| Order | Block | Unblocks (spec / wiring) | Size |
| --- | --- | --- | --- |
| 1 | E-12 History names | PP-09 / W1.1 | S |
| 2 | E-14 Diagnostics severity + locations | PP-07, OU-01 / W1.5 | S |
| 3 | E-01 Playback options | TL-02 / W0.1, W1.12 | S |
| 4 | E-02 Easing (presets, overshoot, per-property later) | PP-01, PR-11 / W0.3, W2.2 | S→M |
| 5 | E-04 Guides + E-05 Snapping | ST-02, ST-04 / W1.2, W1.3 | S + M |
| 6 | E-03 Tween kinds + shape tweens | TL-04 / W1.4 | M (L for general morphing) |
| 7 | E-07 Gradient authoring | CO-01 / W1.6 | S |
| 8 | E-06 Stroke styles | PR-03 / W1.7 | M |
| 9 | E-08 Primitives + PolyStar | TO-01/02, PR-02 / W1.8 | M |
| 10 | E-13 Document swatches | PP-10 / W1.9 | S |
| 11 | E-15 Publish settings | DL-03 / W1.10 | M |
| 12 | E-09 Authored text options | PR-01 / W1.11 | M |
| 13 | E-10 Motion tweens (property tracks) | TL-05, PP-02, PR-10 / W2.1 | L |
| 14 | E-11 Motion guides | TL-01 / W2.3 | M |
| 15 | E-16 Scenes | TL-03, PP-08 / W2.4 | L |
| 16 | E-17 Authored filters / blend / cache / visible | PR-05 / W2.5 | M |
| 17 | E-20 Code intelligence | AC-01/02/06/08 / W2.8 | L |
| 18 | E-21 Debugger + trace bridge + Test Movie host | PP-04, OU-02, DL-12 / W2.9 | L |
| 19 | E-18 Timeline sound | TL-07, PR-08 / W2.6 | L |
| 20 | E-19 Authored video | PR-09 / W2.7 | M |
| 21 | E-22 Image export | DL-04 / W3.1–3.2 | M (PNG/SVG) / L |
| 22 | E-25 Preferences, command registry, autosave | DL-01 / W3.6–3.7 | M |
| 23 | E-23 Video/audio export | DL-05/06 / W3.3–3.4 | L |
| 24 | E-24 Import video / PSD | DL-07/10 / W3.5 | L |
| 25 | E-26 Font embedding | DL-11 / W3.8 | L |
| 26 | E-27 Deco / Spray, E-28 IK | TO-03…05 / W3.9 | M / XL |
| 27 | E-29 9-slice, sharing, accessibility extras, bitmap fill, components | DL-15, DL-17, CO-02, PR-06 / W3.10 | M each |

Status: done — E-03, E-04, E-11, E-16 (except Test scene), E-18 (except custom envelopes, Sound properties and decoded
waveforms); partial — E-01, E-02, E-05, E-10, E-20; the rest is open.

No engine block (editor only): rulers, grid, cursors, toasts, Info, Project panel, Output levels, colour-effect UI,
comments/folding, Find & replace, Movie Explorer, edit multiple frames, preview thumbnails (see wiring Phase 0).

## 3. Blocks

Each block: **Model** (neoflash-core types; serialization/migration) · **Evaluation/runtime** · **Export** (HTML5 /
Rust-WASM) · **Script API** (AS2/AS3/JS) · **Editor** (existing UI element it plugs into) · **Tests** · **Registry** ·
**Recommendations** (algorithms, pitfalls, Flash notes, reuse).

### E-01 · Playback options (loop, script language default)

**Status: partial.** Editor Loop playback and Play all scenes work (`Editor::after_playback_tick`, `WorkspacePrefs.timeline.loop_playback`). Open: `AuthoredPlayback::set_looping` shared with Test Movie, `Project.default_script_language`.

- Model: none for loop (editor pref). Optional `Project.default_script_language: Option<ScriptLanguage>` for Document
  settings (format bump).
- Evaluation/runtime: `AuthoredPlayback::set_looping(bool)`; when false, stop at `frame_count − 1` of the root without
  calling `Player::tick` further (nested clips keep their own clocks — Flash: stopping the main timeline does not stop
  independent movie clips).
- Export: none (publish loop is E-15).
- Editor: Loop toggle (TL-02), Control ▸ Loop playback.
- Tests: core — playback stops at the last frame and `frame()` stays there; nested independent clip still advances.
- Registry: `animation-lifecycle` (playback), `editor-workspace`.
- Recommendations: detect "would wrap" before ticking (compare frame before/after) rather than post-hoc rewind, so
  frame scripts on frame 1 are not re-entered in the preview.

### E-02 · Easing: overshoot, Flash presets, per-property curves

**Status: partial.** Overshooting `CubicBezier` curves (Back) and `Flash { acceleration }` are authorable in the Ease panel and Properties; evaluation still bisects Bézier x. Open: Bounce/Elastic presets, per-property curves (`Keyframe.property_eases`, wiring plan §7.4 rows 1–2), multi-anchor custom curves.

- Model: `Easing` already has `CubicBezier { x1, y1, x2, y2 }` (validation: x ∈ [0,1], |y| ≤ 100) and
  `Flash { acceleration }`. Add `Easing::Preset { family: EaseFamily, mode: EaseMode, params: [f64; 2] }` (NEW) for
  Back / Bounce / Elastic (and Quad…Expo if desired), and later `Easing::Curve(Vec<CurvePoint>)` for multi-segment
  custom curves (Flash "Custom ease" with several anchors). Per-property eases belong to E-10 tracks.
- Evaluation: `evaluate::easing` — bisection for Bézier x (40 iterations). Recommended: Newton–Raphson (4–8
  iterations, derivative check `|dx/du| > 1e-6`) with bisection fallback (the approach of the MIT-licensed
  `bezier-easing` / WebKit `UnitBezier`; reimplement, cite as reference). **Do not clamp the eased output**: values
  outside 0–1 are the overshoot/anticipation; clamp only the input `t`. Clamp interpolated alpha and colour channels
  after interpolation (colour already clamps in `color_lerp`; check alpha and scale sign flips).
- Presets: evaluate with the existing Penner adaptation `as_easing.rs::ease` (BSD-3-Clause, provenance in
  `docs/RUNTIME-EASING-PROVENANCE.md`) so authored and scripted Bounce/Elastic are identical.
- Export: automatic (core in WASM). Validation: finite params, bounded amplitude/period.
- Script API: none (runtime `fl.transitions.easing` already exists separately).
- Editor: Ease panel (PP-01) graph samples the same `easing()` function for its curve (never a second formula);
  Properties Tween select lists presets.
- Tests: `easing(0)=0`, `easing(1)=1` for every variant; Back y2 = 1.12 exceeds 1 mid-curve; Newton vs bisection agree
  to 1e-9; JSON round-trip.
- Registry: `animation-easing`.
- Flash notes: classic tween "Ease −100…100" is `Flash { acceleration }` (quadratic); CS4+ custom ease is a
  multi-anchor Bézier per property; imported XFL eases should map to `Curve` without loss (coordinate with
  `neoflash-import`).

### E-03 · Tween kinds and shape tweens

**Status: done** except shape hints. `Keyframe.tween_kind` (format 6); `neoflash-core/src/shape_tween.rs` pairs shapes by ID then by order (no Hungarian assignment), keeps exact interpolation for equal command structure and otherwise resamples contours by arc length to a common cubic count with start-point/direction alignment; unpaired shapes and contours grow/shrink from their centre; compatible gradients blend, others switch at the midpoint. The editor explains unsuitable content (`timeline_commands.rs::create_tween`). Open: shape hints (§7.4 row 10).

- Model: `Keyframe.tween_kind: TweenKind { Classic, Shape, Motion }` (serde default Classic; format bump). Later
  `Keyframe.shape_hints: Vec<ShapeHint { id: char, start: [f64;2], end: [f64;2] }>` on the start key.
- Evaluation: Classic = current pose interpolation. Shape = interpolate path geometry; today paths interpolate when
  command topology matches (`animation-shape-tween`). General case:
  1. Flatten each shape into closed contours (fill regions) + open strokes; normalise orientation (CCW outer, CW holes).
  2. Contour correspondence: match contours by area-weighted centroid distance (Hungarian assignment for small N;
     greedy for large); unmatched contours grow from / shrink to a point at their centroid.
  3. Point correspondence per contour pair: resample both to the same number of points by arc length, then choose
     the cyclic shift minimising summed squared distance (O(n²), or O(n log n) with FFT cross-correlation); shape
     hints pin chosen points and split contours into matched segments.
  4. Interpolate points (and fill/stroke colour, stroke width) with the eased `t`; rebuild cubic segments from
     resampled points (or interpolate control points when segment counts match).
  Cache correspondence per (start key revision, end key revision) **outside** the document (evaluation stays pure;
  a memo keyed by content hash is acceptable).
- Export: evaluation in WASM → automatic; cost: precompute correspondence at export time into the published JSON if
  runtime cost is too high (measure first).
- Script API: none.
- Editor: Shape tween button (TL-04), frame context menu, green span; diagnostics when a key holds symbols/groups
  ("Shape tweens need raw shapes on both keyframes").
- Tests: square→circle morph has no self-intersection at t=0.5 for a fixture; hint pins correspondence; old docs
  unchanged.
- Registry: `animation-shape-tween`.
- Pitfalls: Flash shape tweens operate on raw (merged) shapes; NeoFlash draws separate path elements — define that a
  shape tween morphs **all** path elements on the layer key as one shape set. Different fill types (solid ↔ gradient)
  need paint interpolation rules (lerp stops after aligning stop counts). Flash's own algorithm is unpublished: do not
  claim identical in-between frames.

### E-04 · Guides (authoring-only)

Done: `Timeline::guides` (no per-document lock; lock is an editor view setting), format version 6, validation,
export strip, tests — see [NF2-RULERS-GUIDES-GRID.md](NF2-RULERS-GUIDES-GRID.md). Original plan:

- Model: `Timeline.guides: Vec<Guide { id: Id, axis: Axis, position: f64 }>` (stage units), `Timeline.guides_locked:
  bool` (Flash locks guides per document; per-timeline is acceptable). Serde default, format bump, validation (finite,
  ≤ 1000 guides).
- Evaluation/export: ignored; export must strip guides (assert in a test).
- Editor: ruler drag-out, guide drag/delete, View ▸ Guides, View settings Clear guides (one Undo).
- Tests: round-trip; export JSON contains no `guides`.
- Registry: `editor-advanced-drawing`.
- Recommendations: guide hit-tolerance in screen pixels (not stage units) so it is zoom-independent.

### E-05 · Snapping (grid, guides, objects, pixels)

**Status: partial.** Grid and guide snapping with the snap ring work (`neoflash-editor/src/snapping.rs`, [NF2-RULERS-GUIDES-GRID.md](NF2-RULERS-GUIDES-GRID.md)). Open: object and pixel snapping, View ▸ Snapping (R-003), ⌘-suspend.

- Model: none (settings in view prefs; snapping tolerance in Preferences ▸ Drawing).
- Engine helper (optional, `neoflash-core::geometry`): `snap_candidates(&AuthoredScene, exclude: &[Id]) ->
  Vec<SnapTarget { point, kind }>` from element bounds (corners, edge midpoints, centre, registration point) and path
  anchors.
- Algorithm: build a uniform grid hash (cell = 2 × tolerance in stage units) of candidate points per drag gesture;
  query the 3 × 3 neighbourhood of the moving point(s); snap the best of: object point < guide < grid (Flash priority:
  objects over grid — **verify** against Flash), per axis independently for guides/grid lines; for a moving selection
  snap the registration point or the nearest bounding-box corner. Tolerance in screen pixels converted by zoom.
- Editor: move/transform/draw code paths, Snap Ring painter (design request), ⌘ to suspend (verify convention).
- Tests: grid/guide/object snapping, zoom-independence, no snapping to the dragged object itself.
- Registry: `editor-advanced-drawing` (limit "snapping").
- Pitfalls: rebuild the index once per gesture, not per frame; exclude locked/hidden layers as targets only if Flash
  does (it snaps to visible content — verify).

### E-06 · Stroke styles

- Model: `StrokeStyle { cap: Cap {None, Round, Square}, join: Join {Miter, Round, Bevel}, miter_limit: f64, dash:
  Option<Vec<f64>>, kind: LineKind {Solid, Dashed, Dotted, Ragged, Stipple, Hatched}, scale_mode: {Normal, Horizontal,
  Vertical, None}, hinting: bool }` on `Rectangle`, `Ellipse`, `Path`, `PaintedPath` (serde default = round caps/joins,
  solid — today's behaviour). Format bump.
- Evaluation: pass through `DrawItem`. Hit testing and bounds must include miter/cap extents.
- Export: `web/path.js` replace hard-coded `lineCap/lineJoin='round'` with style values, `setLineDash`; native painter
  (`stage_primitives.rs`, `lyon_tessellation` stroke options support caps/joins/miter; dashes need pre-splitting the
  path by arc length).
- Script API: AS3 `lineStyle(thickness, color, alpha, pixelHinting, scaleMode, caps, joints, miterLimit)` already
  exists for scripted Graphics? (**verify** in runtime_graphics) — share the enum.
- Editor: Properties stroke rows, Color panel stroke section, Stroke style dialog.
- Tests: JSON round-trip, bounds with square caps, export snapshot.
- Registry: `editor-advanced-drawing`.
- Recommendations: implement Ragged/Stipple/Hatched as deterministic geometry generators (seeded) at evaluation time,
  P3; scale mode "None" = hairline-like constant width — needs inverse-scale in the painter, same in both renderers.

### E-07 · Gradient and bitmap paint authoring

- Model: `PaintedPath { fill_gradient, stroke_gradient: Option<GradientPaint> }` exists. Decide: either (a) shapes
  converting to gradient become `PaintedPath` (simple, no format change, loses primitive parameters), or (b) add
  `fill: Paint { Solid(Color) | Gradient(GradientPaint) | Bitmap(BitmapPaint) }` to all shape kinds (cleaner; format
  bump; migration maps `Color` → `Paint::Solid`). Recommended (b) together with E-08, behind one format bump.
  `BitmapPaint { resource_id, matrix, smoothing, repeat }` for CO-02 (P3).
- Evaluation: interpolate gradients in tweens (align stop counts by inserting stops at matching ratios).
- Export: HTML5 paints `GradientPaint` already (`web/path.js` radial/linear); add focal point (`createRadialGradient`
  with focal circle), spread (pad/reflect/repeat — canvas only pads: implement reflect/repeat by generating extra stops
  across the extended range), linear RGB (approximate by inserting interpolated stops in linear space).
- Editor: Color panel fill type select (CO-01), gradient bar, Gradient Transform tool (exists, add focal handle).
- Tests: conversion keeps geometry; stops validation (0–1 sorted, ≤ 15 stops like Flash); export renders.
- Registry: `editor-paint-effects`.

### E-08 · Primitives and PolyStar

- Model: `Rectangle.corner_radii: [f64; 4]` (+ linked flag in editor), `Ellipse { start_angle, end_angle,
  inner_radius, close_path }` (serde defaults = today's shapes). PolyStar produces `Path` (no model change).
- Evaluation: a single `primitive_path(&ElementKind) -> Vec<PathCommand>` in core used by native painter, hit test,
  bounds, HTML5 painter and Break Apart (one geometry source of truth). Rounded rectangle corners: cubic arcs with
  κ = 0.5523; oval sectors: split arcs ≤ 90° per cubic.
- Export/runtime: canvas draws the generated path; tweens interpolate parameters (Flash interpolates primitive
  parameters in classic tweens — verify).
- Editor: tools rail (TO-01/02), Properties primitive sections (PR-02), primitive control points on stage (ST-11).
- Tests: geometry fixtures; Break Apart equality; old documents render identically.
- Registry: `editor-primitives`, `editor-advanced-drawing`.

### E-09 · Authored text options

- Model: `TextBox` + `kind: TextKind { Static, Dynamic, Input }`, `selectable`, `html: bool`, `border: bool`,
  `password`, `max_chars`, `variable: Option<String>` (AS2), `anti_alias: AntiAlias`, `orientation`; `TextFormat`
  already has letter spacing, kerning, indent, margins, url/target.
- Runtime: map authored TextBox to the existing runtime `TextField` (`runtime_graph/text.rs`, `web/text.js`,
  `web/text-input.js`) so Dynamic/Input behave like script-created fields; Static stays non-interactive.
- Script API: authored Dynamic/Input text with an instance name becomes reachable as `TextField` (AS3) / `TextField`
  object (AS2) — reuse runtime classes; AS2 `variable` binding exists for scripted fields (`as2_variables.rs` —
  verify reuse).
- Export: fonts referenced by name (`export_warnings`); embedding is E-26.
- Tests: browser scenario — authored Input text accepts typing; AS3 reads `tf.text`.
- Registry: `editor-text`.
- Pitfalls: superscript/subscript and mixed runs need character runs (`editor-text` limit) — keep disabled until runs
  exist; HTML rendering supports the Flash HTML subset only — diagnostic for unsupported tags.

### E-10 · Motion tweens (property tracks)

**Status: partial, with a simpler model than proposed below.** There is no `MotionTween` struct: the span start key has `tween_kind = Motion`, and ◆ property keyframes are ordinary `Keyframe`s with `keyed_properties: Some(vec![TweenProperty…])` (X, Y, ScaleX, ScaleY, Rotation, Skew, Alpha, Color). `evaluate.rs::sample_motion` interpolates each property through the keys that key it, with the span ease remapping time; `Editor::commit_history` records changed properties as keys. Rotation turns are the difference between the start rotation and the last Rotation key; the path box is the bounds of the X/Y keys; `Keyframe.orient_to_path` is stored on the start key. Open: per-property eases, motion path curves (`Keyframe.motion_path`), Orient to path evaluation for motion tweens, Sync graphic symbols, Snap, moving keys in time, XFL motion-tween import (§7.4).

- Model: on a motion-tween span keyframe: `MotionTween { target: Id (element id), duration: u32, tracks:
  Vec<PropertyTrack { property: MotionProperty, keys: Vec<TrackKey { frame: u32, value: f64, ease:
  Option<Easing> }> }>, path: Option<MotionPath { commands: Vec<PathCommand> }>, orient_to_path: bool, rotate:
  RotateSpec { count: i32, direction: Cw|Ccw|Auto }, sync_graphics: bool, snap: bool }`. Properties: X, Y, RotationZ,
  ScaleX, ScaleY, SkewX, SkewY, Alpha, ColorTransform channels, filter params (with E-17). Keyframe gets
  `tween_kind = Motion` (E-03). Format bump.
- Evaluation: value(frame) per track: find surrounding keys (binary search), local `t`, apply key ease (E-02), lerp;
  position from `path` when present: arc-length parameterise the path (precomputed lookup table of cumulative
  lengths, 64–256 samples per segment), position = path(s(t)); orient to path = tangent angle + base rotation.
  Spatial vs temporal: X/Y keys define path anchors; dragging the path edits anchors (Flash spatial path).
- Export: automatic via core; precompute arc-length tables at load, not per frame.
- Script API: AS3 `fl.motion.*` is a library (not built-in) — keep out of scope; ensure `MovieClip` properties set by
  script override tweened values the Flash way (script ownership after `x=` — **verify** existing runtime ownership
  rules for classic tweens and reuse).
- Editor: Motion tween button (TL-05), Motion editor (PP-02), Properties — Motion tween (PR-10), ◆ markers, path on
  stage, Copy/Paste motion (MN-02).
- Tests: track evaluation unit tests; orient-to-path tangent; HTML5 parity scenario; classic tweens unchanged; XFL
  import of motion tweens (coordinate with `neoflash-import`).
- Registry: `animation-motion-tween` (scope "independent property-level motion curves" is its stated limit),
  `animation-easing`.
- Pitfalls: keep classic tween semantics separate (identity matching by element id); converting classic → motion must
  not drop eases (bake to tracks); frame-range edits (`timeline_edit.rs`) must split/merge tracks instead of baking
  (today edits bake tween boundaries to integer poses).

### E-11 · Motion guides

**Status: done.** Implemented as `LayerRole::Guide` + `LayerProperties.motion_guide: Option<Id>` on the guided Normal layer (no separate MotionGuide role; `Project::prune_motion_guides` drops stale links). Classic tweens move between the guide-stroke points nearest their start and end poses (`evaluate.rs::sample_layer_in`, `guide_polyline`); `orient_to_path` adds the tangent change. Guides are never published. Open: Orient / Snap controls for classic tweens (needs a design slot).

- Model: `LayerRole::MotionGuide` (NEW) and `LayerProperties.guided_by: Option<Id>` (stable ids like `mask`);
  validation: guide layer exists, no cycles, only Normal layers guided.
- Evaluation: for a classic tween on a guided layer, take the first open stroke on the guide layer at the tween start
  frame; snap start/end registration points to nearest path points; position = arc-length interpolation between those
  parameters with the tween ease; orient to path from Properties. Guide layers are never drawn in export (like
  `LayerRole::Guide`).
- Export: automatic + guide stroke excluded from drawing.
- Editor: Add motion guide (TL-01), layer context menu, guided indentation (`LayerKind`), Snap to motion guide.
- Tests: object follows a quarter circle; guide not exported; validation errors for cycles.
- Registry: `editor-layer-organization` ("Motion-path guides" limit).
- Flash notes: Flash chooses the guide path segment closest to the start/end positions; closed paths need a gap — emit
  a diagnostic for closed guide paths.

### E-12 · History names (and optional command log)

- Model: none (history is not document data).
- Core: `History` stores `undo: Vec<(Project, String)>`, `redo` likewise; `begin_named`, `entries()`,
  `undo_to(n)`/`redo_to(n)`; capacity eviction drops labels with snapshots.
- Editor: `transact_named(label, f)`; History panel (PP-09); Edit ▸ "Undo <label>"; Preferences undo levels →
  `History::new(levels)` (reset with confirmation).
- Tests: label order through undo/redo/eviction; `undo_to` equals repeated undo.
- Registry: `platform-undo`.
- Recommendations: keep snapshot undo (clones share `Arc` resource bytes; already cheap). A command log (inverse
  operations) is only worth it if memory profiling of large projects shows snapshot cost (`platform-performance`);
  if adopted, keep snapshots as checkpoints every N commands.

### E-13 · Document swatches

- Model: `Project.swatches: Vec<Swatch { color: Color } | Gradient(GradientPaint)>` serde default; format bump.
- Editor: Swatches panel/popover Document section, Color panel ⋯ Add swatch; Recent colours stay in prefs.
- Tests: round-trip; Undo add/remove. Registry: `editor-paint-effects`.

### E-14 · Diagnostics: severity, codes, locations

- Script: `Diagnostic { severity: Severity { Error, Warning }, code: Option<String>, … }` with serde defaults;
  warnings for currently silent accepted-with-limitation cases (e.g. migration notes) — never downgrade an error.
- Editor: map each compiled script to `ScriptLocation { symbol, layer, frame, element, event } | Source(path)`; the
  editor already knows which script it compiled (`sources.rs::record_code_check`); `compile_project` diagnostics carry
  source-unit paths (**verify** they identify frame/object scripts; if not, add an origin id to the compile input).
- Export: `ExportError::Scripts` keeps structured diagnostics instead of a joined string.
- Tests: warning severity; location mapping; existing diagnostics unchanged.
- Registry: `web-publish-diagnostics`, `web-project-toolchain`.
- Flash note: Flash error codes (e.g. 1120) are Adobe compiler codes; only emit a code when NeoFlash's check matches
  that rule exactly, otherwise use NeoFlash codes (`NF####`).

### E-15 · Publish settings

- Model: `Project.publish: PublishSettings { html: HtmlOptions { template, scale_mode, loop_playback, autoplay,
  preloader }, scripts: { strict, include_trace, minify }, assets: { jpeg_quality, audio }, source_maps }` (serde
  default = current behaviour). Paths (output folder) are **not** document data — keep in prefs (AGENTS.md: no
  filesystem paths in the document).
- Export: `export_project_with(project, &PublishSettings, output_dir, runtime_dir)`; `export_project` calls it with
  the document settings. Minify via `oxc_codegen` minified output (already a dependency of `neoflash-script`);
  strip `trace` calls at compile time when disabled; JPEG re-encode via `image` crate.
- Runtime: `web/runtime.js` honours loop/autoplay/scale mode (`StageScaleMode` semantics exist for scripts —
  reuse).
- Editor: Publish settings dialog (DL-03), Preferences ▸ Publish defaults.
- Tests: export with loop off stops; trace stripped; settings round-trip.
- Registry: `web-html5-export`.
- Non-HTML5 targets (desktop, video, sprite sheet, SVG animation) remain disabled until E-22/E-23.

### E-16 · Scenes

**Status: done** except Control ▸ Test scene and the breadcrumb ▾ (ST-08). `Project.scenes: Vec<Id>` (scene MovieClip symbols; `scenes[0] == root`). Scenes are flattened only at the export / `WebPlayer::new` boundary (`scenes.rs::flatten_scenes_with_ranges`); `Player::set_scenes` keeps the ranges for `currentScene`, `scenes`, scene-aware `gotoAndPlay`/`gotoAndStop` (`resolve_goto`) and `nextScene`/`prevScene` (`scene_step`). The editor edits each scene symbol; Scene select, Insert ▸ Scene, the Scene panel and Play all scenes are in `timeline_commands.rs` and `panels/scene.rs`.

- Model: `Project.scenes: Vec<Scene { id, name, timeline_symbol: Id }>`; migration: absent → one scene from
  `Project.root` (no file change until a second scene is added; then format bump). Scene timelines are MovieClip-like
  symbols hidden from the Library.
- Evaluation/runtime: main timeline = concatenation; `Player` keeps `(scene index, frame)`; end of scene → next scene;
  `gotoAndPlay(frame, "Scene 2")`; AS3 `MovieClip.scenes`, `currentScene`, `nextScene()`, `prevScene()`; AS2
  `nextScene()`/`prevScene()`. Frame labels resolve within the scene first (Flash rule).
- Export: all scenes in order; "Test scene" exports only the current scene.
- Editor: Scene select, Scene panel, breadcrumb root item, Insert ▸ Scene, Control ▸ Play all scenes.
- Tests: two-scene playback in core and browser; label resolution; one-scene files unchanged.
- Registry: `animation-lifecycle` (limit "scenes"); ROADMAP AS3 stage 3 ("Authored multiple scenes and navigation").
- Pitfalls: Flash flattens scenes into one timeline at publish (frame numbers continue); AS3 `currentFrame` is
  scene-relative — implement both views explicitly.

### E-17 · Authored filters, blend modes, cache as bitmap, visibility

- Model: `Element.display: DisplayOptions { visible: bool, blend: BlendMode, cache_as_bitmap: bool, filters:
  Vec<FilterSpec> }` reusing `filters.rs::FilterSpec` (max 8 objects' filters, `MAX_OBJECT_FILTERS`); format bump.
- Evaluation: tween filter parameters (classic: lerp matching filter lists; mismatched lists → diagnostic like Flash's
  "filters must match").
- Runtime/export: feed authored filters into the existing runtime filter pipeline (`runtime_graph/filters.rs`,
  `web/filters.js`); blend via canvas `globalCompositeOperation` (`runtime_graph/as2_render.rs` already selects blend
  modes for AS2 — extract a shared mapping). Native editor preview: CPU filters from `filters.rs` on an offscreen
  raster of the instance (bounded by `MAX_FILTER_PIXELS`), cached per element revision.
- Script API: authored filters visible through `DisplayObject.filters` (AS3) / `MovieClip.filters` (AS2) —
  reuse runtime ownership rules.
- Editor: Properties — Display & filters (PR-05), Colour effect (PR-04 already uses `ColorTransform`).
- Tests: drop shadow fixture export snapshot; blend mode mapping table; filter tween.
- Registry: `editor-paint-effects`.
- Pitfalls: Flash blend modes Layer/Alpha/Erase need an isolated group (offscreen canvas) — the AS2 render path has
  "raster isolation"; reuse it rather than approximating with composite ops on the shared canvas.

### E-18 · Timeline sound

**Status: done** except custom envelopes (DL-09), Sound properties (DL-08), decoded waveform peaks and legacy FLA frame sounds — see [decision 0037](decisions/0037-timeline-sound.md). `Keyframe.sound: Option<FrameSound { resource, sync, loops, loop_forever, effect }>` (no envelope field yet); `timeline_sound.rs` `SoundCommand`s drained from `Player`; `web/audio.js` schedules them; the editor previews them with rodio 0.21 + Symphonia (MP3/WAV; MPL-2.0 decoder crates, editor only, never in HTML5 exports). Streams are not re-synchronised when a host drops frames; scrubbing is silent.

- Model: `Keyframe.sound: Option<FrameSound { resource_id: Id, sync: Event|Start|Stop|Stream, effect: SoundEffect,
  envelope: Vec<EnvelopePoint { position_ms, left, right }>, loops: LoopSpec { Repeat(u32) | Loop } }>`; layer
  derives "Sound layer" when its keys only hold sounds; `ResourceUnit.media_type` audio/*; format bump.
- Runtime: `Player` emits `SoundCommand { start | stop | stream_seek }` events when frames are entered (never inside
  pure `evaluate`); browser adapter (`web/audio.js`, existing AudioContext service) schedules buffers:
  Event = fire-and-forget overlapping; Start = start unless already playing; Stop = stop that sound; Stream = tie audio
  to the timeline: start at `frame/fps` offset on play, skip/drop frames to stay in sync (Flash drops frames to keep
  stream audio in sync). Envelopes = `GainNode` automation per channel (`ChannelSplitter`/`Merger`).
- Editor preview: native audio is not present (**verify**; eframe has none) — options: (1) play through a small Rust
  audio backend (e.g. `cpal` + `symphonia` decoding, both MIT/Apache — new dependencies need licence review), or (2)
  preview sound only in Test Movie with an explicit status note. Recommend (2) first (breadth), (1) later.
- Waveform: decode once (symphonia or browser) to min/max peaks per frame width, cache by resource hash.
- Script API: `SoundMixer.stopAll()`; Stream sounds are not scriptable objects (Flash).
- Tests: core frame-entry events for each sync mode incl. loops and goto; browser scenario with mocked AudioContext
  (pattern exists in web tests).
- Registry: `animation-media-sync`, `web-media-assets`.
- Pitfalls: browsers block audio until a user gesture — Test Movie needs a click-to-start overlay; scrubbing with
  Stream sync must not leak overlapping sources.

### E-19 · Authored video

- Model: `ElementKind::Video { width, height }` + NEW `source: Option<Id>` (resource), `deployment:
  Embedded|External|Progressive`, `autoplay`, `loop`, `muted`, `scale: Fit|Fill|None`, `volume`, `cue_points:
  Vec<CuePoint { name, time_ms, kind: Event|Navigation }>`; video layer derived from content.
- Runtime: reuse `runtime_graph/video.rs` + `web/video.js` (HTMLVideoElement drawn to canvas via `drawImage`);
  Embedded = timeline-synchronised: seek video to `frame/fps` when the timeline moves (seek cost: use
  `requestVideoFrameCallback` when available, tolerate ±1 frame); External/Progressive = free-running from URL.
  Cue points → AS3 `NetStream` `onCuePoint`/`MetaDataEvent` style dispatch (bounded).
- Editor: native preview shows the first frame poster (decode via browser at import, store a poster bitmap resource)
  — native video decoding is out of scope initially; state it in the UI hint.
- Tests: core model/validation; browser scenario autoplay + cue point.
- Registry: `animation-media-sync`, `web-media-assets`.

### E-20 · Code intelligence (completion, formatter, navigator, toolbox)

**Status: partial.** Code hints, the toolbox, the Script navigator and re-indenting Auto-format work ([NF2-ACTIONS-EDITOR.md](NF2-ACTIONS-EDITOR.md)); full formatting and folding are open.

- Script crate: `complete(project, script, offset) -> Vec<Completion { label, kind, detail }>` using the project
  program's class/member tables plus the declaration catalogue (`docs/progress/actionscript-catalog.json`) filtered to
  **implemented/partial** entries (never offer missing APIs as working); `format(script) -> Result<String,
  Diagnostic>`: JS/TS via `oxc_codegen` (verify comment preservation in 0.75; if comments drop, do not ship formatting
  for JS/TS), AS2/AS3 via a printer over the existing AS parser AST with comments attached as trivia.
- Navigator: editor traversal (no engine).
- Tests: completion fixtures; formatting idempotence; no comment loss.
- Registry: `web-project-toolchain`.

### E-21 · Debugger, breakpoints, trace bridge, Test Movie window

- Protocol: Test Movie serves the export from a local loopback HTTP server owned by the editor (or a WebSocket on
  127.0.0.1 with a random token); the runtime posts `trace`, errors and debug events; the editor sends
  continue/step/breakpoint commands. Keep the transport in a small adapter; no third-party debugger engine.
- Breakpoints: compile scripts with instrumentation — insert `__nfDebug.hit(scriptId, line)` probes at statement
  boundaries only for debug builds (the compile step already transforms code); pausing a browser JS thread
  synchronously is impossible without blocking — use either (a) `debugger;`-style pause via Chrome DevTools Protocol
  (external, heavy), or (b) compile debug builds to a resumable form (generator-based: each frame/handler becomes a
  generator, probes `yield` on breakpoints; the runtime scheduler resumes on Continue). Recommend (b) for
  frame/object scripts first; class methods called synchronously from native code cannot pause — show a diagnostic.
- Variables/watch: probes capture locals via a compiler-generated scope getter; watch expressions evaluated in that
  scope.
- Source maps: map generated JS lines back to source lines (`oxc_codegen` supports source maps — verify for
  transformed AS output).
- Test Movie window (DL-12): native window hosting the export requires a webview (e.g. `wry`, MIT/Apache) — licence
  and dependency review; alternative is keeping the system browser and showing Output in the editor.
- Tests: probe insertion does not change program semantics (run project-execute families with debug build); trace
  bridge end-to-end.
- Registry: `web-project-toolchain` (limit "debugger/source maps").

### E-22 · Image export (PNG, SVG; PSD/AI/PDF later)

- PNG: a CPU rasteriser shared by export and native thumbnails — `tiny-skia` (BSD-3, already pulled by `resvg` in the
  editor) can fill/stroke paths with gradients; text needs glyph outlines (font-kit/ab_glyph in the editor). Put the
  painter in a crate the export side can use (e.g. `neoflash-export` feature or a new internal render module) — do
  not duplicate the canvas semantics: fixture-compare against HTML5 canvas output (tolerance-based).
- SVG: write `DrawItem`s directly (paths, gradients with `gradientTransform`, `clipPath`/`mask` for masks, `<image>`
  for bitmaps embedded as data URIs or linked, text as `<text>` or outlines). Precision/ID prefix options from the
  dialog.
- PSD (layers as raster layers, no smart objects initially), AI (PDF-compatible), PDF: P3; consider `printpdf`-like
  writers only after licence review; AI = PDF with layers.
- Editor: Export image dialog (DL-04), File ▸ Export ▸.
- Tests: PNG of fixture frames vs reference; SVG validity (parse with `roxmltree`) and geometry checks.

### E-23 · Video and audio export

- Video: render frames with E-22 rasteriser (authored timeline) or capture the HTML5 runtime (`canvas.captureStream()`
  + `MediaRecorder` for WebM; "runtime capture" option for scripts and nested clips); encode MP4/MOV via an external
  `ffmpeg` only if present on the system (detect, never bundle silently; licence: LGPL/GPL builds) — otherwise offer
  WebM (browser) and PNG sequence (native) only, with an honest disabled state. GIF: palette quantisation (NeuQuant /
  median cut) via the `image`/`gif` crates.
- Audio: mix timeline sounds (E-18) offline (`OfflineAudioContext` in the browser, or Rust mixing after decode); WAV
  writer is trivial; MP3/OGG/AAC encoders need licence review (LAME LGPL, vorbis BSD) — WAV first.
- Tests: frame count/duration; WAV header and length.

### E-24 · Import video / PSD

- Video: store the file as a resource (size limits: `MAX_RESOURCE_BYTES` 16 MiB today — videos exceed it; external
  deployment keeps a relative URL instead of bytes; embedded deployment needs a larger budget decision by the owner).
  Trim In/Out stored as playback range metadata, not re-encoding.
- PSD: parse layers (RLE/raw channels) into bitmaps — implement a bounded reader in `neoflash-import` (format spec is
  public); vector/text layers → flattened bitmaps with a diagnostic; layer effects ignored with a diagnostic.
- Tests: fixture PSD with 3 layers; oversize file diagnostic.
- Registry: `web-media-assets`, `imports-*` (no PSD id exists — ask the owner before adding one; AGENTS.md forbids
  inflating the registry).

### E-25 · Preferences, command registry, autosave

- Storage: one `EditorPreferences` struct persisted like `WorkspacePrefs` (eframe storage key, versioned, serde
  defaults); never in the document.
- Command registry: `Command { id, title, default_shortcut, run: fn(&mut Editor) }` table used by menus, toolbar
  buttons and the shortcut editor; shortcut sets as maps `id → KeyChord`; conflict detection = map inversion.
- Autosave/recovery: periodic write of the document JSON to an app-support recovery folder (not next to the user's
  file) after N minutes of dirty state; restore prompt on launch (`platform-recovery`).
- Tests: prefs round-trip and defaults for missing fields; shortcut conflicts; recovery file lifecycle.
- Registry: `editor-workspace`, `platform-recovery`, `platform-native-macos`.
- Updates/telemetry pages stay disabled until defined (`platform-release`, `platform-extensions`).

### E-26 · Font embedding

- Model: `Project.fonts: Vec<EmbeddedFont { id, family, style, ranges: Vec<RangeInclusive<u32>>, extra: String,
  class_name: Option<String> }>`; resource = subset font file.
- Export: subset with a Rust subsetter (e.g. `subsetter`, MIT/Apache — licence review) and emit `@font-face` with the
  bytes; layout must use the embedded font metrics (`web-font-portability`).
- Tests: subset contains requested glyphs; text renders with the embedded font in a clean browser profile.

### E-27 · Deco and Spray brush

- Spray: seeded RNG (store seed in the gesture, not the document), Poisson-disc or uniform jitter within the brush
  ellipse; outputs `Instance`/`Path` elements — editor-side; no runtime change.
- Deco: generators (Grid fill, Vine fill = L-system-like branching with angle/length/step, Symmetry brush =
  rotational copies) producing ordinary elements; "Animate pattern" = generate one keyframe per step. Keep generators in
  core so they are testable and deterministic.
- Tests: deterministic output for a seed.

### E-28 · Bone tool / IK

- Model: `Armature { id, bones: Vec<Bone { id, parent, length, angle, joint: JointLimits { rotate: Option<(min,max)>,
  translate_x/y: Option<(min,max)>, spring: Option<(strength, damping)> } }>, bindings: Vec<(bone, element)> }`;
  poses = property keyframes ◆ on an Armature layer (reuse E-10 tracks: one angle track per bone).
- Solver: FABRIK for chains (fast, stable) with per-joint angle clamping after each backward/forward pass; CCD as
  fallback for strict limits; runtime armatures (Flash "Runtime" type) need the solver in WASM and an AS3
  `fl.ik` facade (library, far-future per ROADMAP) — authortime poses first.
- Shape binding (Bind tool) deforms path points by bone weights (linear blend skinning) — P3+.
- Tests: reach target within tolerance; limits respected; poses interpolate.

### E-29 · Remaining smaller blocks

| Block | Model / engine | Notes |
| --- | --- | --- |
| 9-slice (DL-15) | `Symbol.scale9: Option<Rect>`; runtime draws 9 regions with separate transforms | Flash applies 9-slice to shapes, not nested symbols — same limit. |
| Runtime / authortime sharing (DL-15) | `Symbol.shared: Option<SharedLink { url | source_file }>` | Needs loader semantics; keep disabled with hint. |
| Accessibility extras (DL-17) | `Accessibility.shortcut`, `children_accessible`, `auto_label` | Map to ARIA (`aria-keyshortcuts`) in `web/accessibility.js`. Registry `web-semantic-targets`, `platform-accessibility`. |
| Bitmap fill (CO-02) | `Paint::Bitmap` (E-07) | Canvas `createPattern` + `setTransform`; native via tiny-skia/lyon pattern. |
| Component parameters (PR-06) | none (no component model) | Far future (ROADMAP: `fl.*` components are far-future). |
| Layer height (DL-16) | `LayerProperties.row_height: Option<u8>` (authoring-only) | Timeline rows must stop assuming one constant height (also TL-09). |
| Reverse frames (MN-02) | `TimelineEdit::Reverse { range }` | Reverse keyframe order within range; tweens invert ease (Flash flips). |
| Copy/Paste motion (MN-02) | clipboard payload of E-10 tracks | Version the clipboard format (`clipboard.rs` `VERSION`). |

## 4. Cross-cutting test strategy

- Core unit/integration tests in `crates/neoflash-core/tests/*.rs` (extend existing families such as `core.rs`,
  `paths.rs`, `filters.rs` before adding files).
- Script/runtime behaviour through existing project workflow families (`node crates/neoflash-script/tests/
  project-execute.mjs`; focused reruns with `NEOFLASH_WORKFLOWS=id1,id2` only after a concrete failure).
- Web adapter tests in `web/tests/*.test.cjs` (mock AudioContext/video as existing tests do).
- Editor behaviour in existing editor test modules; screenshots (both themes) for every converted panel.
- One shared regression pass per delivery batch: `scripts/check.sh` (includes the design lock).

## 5. Format versions

| Version | Contents |
| --- | --- |
| 6 (done) | Ruler guides, tween kinds, ◆ property keys, `orient_to_path`, motion guides, scenes, frame sounds |
| 7 (next batch) | Whatever the next delivery batch adds, e.g. easing presets, `property_eases`, shape hints, motion path curves, custom sound envelopes, stroke style, paint/primitive parameters, document swatches, publish settings, text options, authored display/filters, video sources |

Each version: validation rule per field ("requires format N"), loader accepts ≤ N, writer bumps only when the new
data is present (so untouched documents stay readable by older builds), import (`neoflash-import`) maps Flash data to
the new fields where it currently drops or bakes them, with diagnostics for the rest.
