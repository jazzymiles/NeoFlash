# NF2 functional specification — unfinished controls and planned panels

Readers: implementation agents (Codex) and the project owner. This page says **what every NF2 element that does not
work yet is supposed to do**, using classic Flash semantics. It does not change visuals: the NF2 interface is complete
and locked (AGENTS.md › "Design system (NF2) — locked", [DESIGN-SYSTEM.md](DESIGN-SYSTEM.md)).

Related pages:

- [NF2-WIRING-PLAN.md](NF2-WIRING-PLAN.md) — phased order for connecting these elements to editor/engine code.
- [NF2-ENGINE-PLAN.md](NF2-ENGINE-PLAN.md) — engine work (model, evaluation, runtime, export, script) behind them.
- [NF2-REQUESTS.md](NF2-REQUESTS.md) — where design gaps are reported (never fixed in code).
- Reference: `interface/design-system/docs/NeoFlash-Design-System.codex.md` §4–§10 and `docs/data/outline/*.txt`.

Statuses and "Done / Not yet" notes reflect `main` on 2026-11-01 (after the timeline, scene, sound and rulers
batches). Every planned preview has its entry point (menu item, button); this page names "its menu item / button"
instead of pinning code. Statements marked **verify** were not confirmed in code. The timeline's current architecture
and remaining task cards are in [NF2-WIRING-PLAN.md](NF2-WIRING-PLAN.md) §7.

## Conventions

- **Status**: `done` = works as specified (remaining limits are named in its entry); `disabled` = control exists, rendered disabled with "Not available yet"; `preview` = body exists as a
  read-only planned preview (`panels/planned*.rs`, Window ▸ Planned panels); `absent` = in the NF2 reference, not in
  the UI; `partial` = control works but part of its NF2 behaviour is missing.
- **Engine**: `yes` = needs new `neoflash-core` / `-script` / `-export` / `-import` / `-web` behaviour; `editor` =
  needs editor-crate logic only (no engine change); `UI-only` = state and wiring inside the panel file.
- **Data — model has it?**: checked against `crates/neoflash-core/src/model.rs` (`Project`, `Symbol`, `Timeline`,
  `Layer`, `LayerProperties`, `Keyframe`, `Easing`, `Element`, `ElementKind`, `Transform`, `Accessibility`),
  `history.rs`, `text.rs`, `filters.rs` and `runtime_graph/*`. "Runtime only" = exists for scripts at runtime but is
  not authored document data.
- **Priority** (suggestion, breadth first per AGENTS.md): **P1** high value and UI/editor-only or a small engine
  extension; **P2** core Flash authoring capability needing real engine work; **P3** large, specialised or low value.
- Every element must keep: Undo as one transaction per user action (`Editor::transact` / `History::begin…commit`),
  honest diagnostics (no silent drops), Light/Dark, keyboard access and NF2 tooltips with shortcuts.
- Enabling an element may only drop its "— not available yet" / "Not available yet" suffix or disabled hover text;
  any other label, tooltip or layout change is a design request.

## Summary

| ID | Element | Surface | Status | Engine | Priority |
| --- | --- | --- | --- | --- | --- |
| TL-01 | Add motion guide | Timeline controls | done | yes | P2 |
| TL-02 | Loop playback | Timeline controls | done | editor | P1 |
| TL-03 | Scene select ▸ New scene | Timeline controls | done | yes | P2 |
| TL-04 | Shape tween | Timeline controls | done | yes (small) | P2 |
| TL-05 | Motion tween | Timeline controls | partial | yes | P2 |
| TL-06 | Ease panel button | Timeline controls | done | editor | P1 |
| TL-07 | Sound | Timeline controls | done | yes | P3 |
| TL-08 | Edit multiple frames | Timeline controls | partial | editor | P2 |
| TL-09 | Frame view ▸ Short rows | Frame view menu | done | UI-only | P1 |
| TL-10 | Frame view ▸ Preview | Frame view menu | done | editor | P3 |
| TL-11 | Frame view ▸ Snap playhead to keyframes | Frame view menu | done | UI-only | P1 |
| TL-12 | Frame view ▸ Placement, Preview in context, Help, Close | Frame view menu | partial | UI-only | P3 |
| TL-13 | Layer types Sound / Video / Armature / Motion guide rows | Timeline layers | partial | yes | P2–P3 |
| TL-14 | Button symbol state labels Up/Over/Down/Hit | Timeline | verify | editor | P2 |
| TO-01 | Rectangle / Oval primitive tools | Tools | done (options on Rectangle/Oval + Properties) | yes | P2 |
| TO-02 | PolyStar tool + settings | Tools | done | editor (+small) | P2 |
| TO-03 | Deco tool | Tools | absent | yes | P3 |
| TO-04 | Spray brush | Tools | absent | editor | P3 |
| TO-05 | Bone / Bind (IK) | Tools | absent | yes | P3 |
| TO-06 | Snap toggle (tools rail) | Tools | done | editor | P1 |
| TO-07 | Tool options: Object drawing, smooth / straighten | Tools | absent | editor | P2 |
| TO-08 | Fill/stroke chip → Swatches popover | Tools | partial | UI-only | P1 |
| ST-01 | Stage rulers + ruler corner | Stage chrome | working | UI-only | P1 |
| ST-02 | Guides | Stage chrome | working | yes (small) | P1 |
| ST-03 | Stage grid | Stage chrome | working | UI-only | P1 |
| ST-04 | Snapping (grid, guides, objects, pixels) + snap ring | Stage chrome | done (View ▸ Snapping R-003) | editor | P1 |
| ST-05 | Zoom control toggles rulers/guides/grid/snap | Stage chrome | working | UI-only | P1 |
| ST-06 | View settings popover | Stage chrome | working | UI-only | P1 |
| ST-07 | NF2 stage cursors | Stage chrome | done | UI-only | P1 |
| ST-08 | Breadcrumb ▾ menu + Symbols & scenes popover | Stage chrome | absent | editor | P2 |
| ST-09 | Toasts / progress / spinner | Window chrome | absent | UI-only | P1 |
| ST-10 | Dock compass | Docking | absent | UI-only | P3 |
| ST-11 | Canvas editing chrome (Selection/Transform box variants, gradient transform focal handle) | Stage | partial (boxes, anchors, marquee, transformation point done) | editor | P2 |
| PR-01 | Properties — Text (full NF2 variant) | Properties | partial | yes | P2 |
| PR-02 | Properties — Oval / Rectangle primitive options | Properties | done | yes | P2 |
| PR-03 | Stroke style (line style, cap, join, miter, hinting, scale) + Stroke style dialog | Properties / dialog | absent | yes | P2 |
| PR-04 | Properties — Movie clip: colour effect styles | Properties | partial (R-012) | editor | P1 |
| PR-05 | Properties — Movie clip: Display & Filters tabs | Properties | preview | yes | P2 |
| PR-06 | Properties — Movie clip: Swap…, Parameters tab | Properties | partial (Library Swap done; R-012) | editor / yes | P2 |
| PR-07 | Properties — Graphic thumbnail strip | Properties | partial | editor | P3 |
| PR-08 | Properties — Sound (frame) | Properties | partial | yes | P3 |
| PR-09 | Properties — Video playback & cue points | Properties | partial | yes | P3 |
| PR-10 | Properties — Motion tween | Properties | partial | yes | P2 |
| PR-11 | Frame easing: overshoot values, Flash acceleration field | Properties | done | UI-only | P1 |
| CO-01 | Color panel fill type: Linear / Radial gradient | Color | done | yes (small) | P2 |
| CO-02 | Color panel fill type: Bitmap fill (+ Bitmap fill panel) | Color | disabled | yes | P3 |
| CO-03 | Color panel: Swatches tab, panel menu Add swatch, picker area | Color | partial (Add swatch + Swatches panel done) | editor | P1 |
| LI-01 | Library: delete / duplicate item | Library | done | editor | P1 |
| LI-02 | Library: bitmap / sound / font / video items, document select, pin | Library | absent | editor | P2 |
| AC-01 | Actions toolbar: auto-format | Actions | partial (re-indent only) | yes | P2 |
| AC-02 | Actions toolbar: show code hint (+ hint popover) | Actions | done | yes | P2 |
| AC-03 | Actions toolbar: debug options | Actions | disabled | yes | P3 |
| AC-04 | Actions toolbar: collapse between braces / expand all (folding) | Actions | disabled | editor | P2 |
| AC-05 | Actions toolbar: block comment / line comment / uncomment | Actions | done | UI-only | P1 |
| AC-06 | Actions toolbar: toolbox sidebar + script navigator | Actions | done | yes | P2 |
| AC-07 | Script assist | Actions | disabled | yes | P3 |
| AC-08 | Gutter markers (warning, breakpoint, fold), inline error peek + Quick fix, overview ruler | Actions | partial | yes | P2 |
| OU-01 | Output: Show All / Errors / Warnings / Trace, timestamps | Output | absent | editor | P1 |
| OU-02 | Output: trace() from Test Movie | Output | absent | yes | P2 |
| PP-01 | Ease panel | Planned panel | done | editor | P1 |
| PP-02 | Motion editor | Planned panel | partial | yes | P2 |
| PP-03 | Properties — Motion tween | Planned panel | partial | yes | P2 (= PR-10) |
| PP-04 | Debugger | Planned panel | preview | yes | P3 |
| PP-05 | Project | Planned panel | done | editor | P2 |
| PP-06 | Code snippets | Planned panel | preview | editor | P2 |
| PP-07 | Compiler errors | Planned panel | preview | editor (+small) | P1 |
| PP-08 | Scene | Planned panel | done | yes | P2 |
| PP-09 | History | Planned panel | done | editor (+small) | P1 |
| PP-10 | Swatches | Planned panel | done | editor | P1 |
| PP-11 | Info | Planned panel | done | editor | P1 |
| PP-12 | Rulers, guides & grid (View settings) | Popover | working | UI-only | P1 (= ST-06) |
| PP-13 | Properties — Sound | Planned panel | partial | yes | P3 (= PR-08) |
| PP-14 | Properties — Display & filters | Planned panel | preview | yes | P2 (= PR-05) |
| DL-01 | Preferences (9 pages) | Planned dialog | preview; General ▸ Theme done | editor | P1–P3 per page |
| DL-02 | Document settings | Planned dialog | done | editor | P1 |
| DL-03 | Publish settings | Planned dialog | preview | yes | P2 |
| DL-04 | Export image (PNG/SVG/PSD/AI/PDF) | Planned dialog | preview | yes | P2 (PNG/SVG) / P3 |
| DL-05 | Export video | Planned dialog | preview | yes | P3 |
| DL-06 | Export audio | Planned dialog | preview | yes | P3 |
| DL-07 | Import video | Planned dialog | preview | yes | P3 |
| DL-08 | Sound properties | Planned dialog | preview | yes | P3 |
| DL-09 | Edit envelope | Dialog | absent | yes | P3 |
| DL-10 | Import PSD to library | Dialog | absent | yes | P3 |
| DL-11 | Font embedding | Dialog | absent | yes | P3 |
| DL-12 | Test Movie window | Window | absent | yes | P2 |
| DL-13 | Find and replace | Dialog | absent | editor | P2 |
| DL-14 | Movie Explorer | Panel | absent | editor | P2 |
| DL-15 | Convert to Symbol ▸ Advanced: 9-slice, runtime / authortime sharing | Dialog | absent | yes | P3 |
| DL-16 | Layer properties: Guided / Masked / Armature types, layer height | Dialog | partial | editor / yes | P2 |
| DL-17 | Accessibility: child objects, auto label, shortcut | Panel | partial | yes (small) | P3 |
| MN-01 | Menu items absent from §8 menus | Menus | partial | mixed | P1–P3 |
| MN-02 | Frame / layer / stage context menu items absent | Menus | partial | mixed | P1–P2 |
| MN-03 | View ▸ Rulers / Guides / Grid / Rulers, guides & grid… | Menus | working (Snapping ▸ disabled) | UI-only | P1 |

Counts: 93 entries — 14 timeline, 8 tools, 11 stage/window chrome, 11 properties, 3 colour, 2 library, 8 actions,
2 output, 14 planned panels (3 alias other entries), 17 dialogs/panels (DL-01 covers 9 preference pages), 3 menu
groups.

---

## 1. Timeline controls (`crates/neoflash-editor/src/panels/timeline_controls.rs`, `panels/timeline.rs`)

**TL-01 · Add motion guide** — `timeline_layer_actions` · done
- Done (2026-11-01): `LayerProperties.motion_guide` link to a Guide-role layer (format 6); Timeline button, Insert ▸ Motion guide and layer menu Add classic motion guide; the guided layer indents under the guide, which shows the Motion guide type. Classic tweens follow the guide's first stroke (`evaluate::sample_layer_in`). `Keyframe.orient_to_path` turns the object with the guide tangent (editor and HTML5; `Editor::set_orient_to_path`). Not yet: Orient / Snap controls for guided classic tweens (the classic Tween section has no slot — design request needed; wiring plan §7.4 row 5); drawing the path when the guide has no stroke is the author's step.
- Purpose: insert a classic motion guide layer above the current layer and make the current layer guided by it.
- Behaviour: click → new layer `Guide: <name>` of type Motion Guide above the current layer; current layer indents
  under it. A stroke drawn on the guide is the path; classic tweens on guided layers follow it ("Snap to motion
  guide" in PR-10). Locked/hidden current layer → command disabled. One Undo step.
- Data: `LayerRole::Guide` layer + `LayerProperties.motion_guide: Option<Id>` on the guided Normal layer (no separate
  motion-guide role); `Keyframe.orient_to_path`.
- Depends on: engine — path sampling along a guide stroke in evaluation and export (done).
- Acceptance: button creates a guide layer + guided relation in one Undo; a classic tween on the guided layer moves
  along a drawn curve in the editor and in HTML5; the guide is not exported as artwork.

**TL-02 · Loop playback** — `timeline_transport` · done
- Done: `WorkspacePrefs.timeline.loop_playback` (default on), toggle in the bar and Control ▸ Loop playback; off stops on the last frame and Play restarts from frame 1.
- Purpose: when on, editor playback wraps from the last frame to frame 1; when off, it stops at the last frame
  (Control ▸ Loop playback, checked by default in §8).
- Behaviour: toggle (Selected style when on). Editor preference, not document data; no Undo.
- Data: editor state; `AuthoredPlayback::tick` currently advances the shared `Player` which wraps (verify exact
  wrap point). Off = detect wrap and stop.
- Depends on: editor (optionally a small `AuthoredPlayback` option).
- Acceptance: off → playback stops on the last frame and the Play button returns to Play; on → wraps; state survives
  panel re-dock (workspace prefs).

**TL-03 · Scene select ▸ New scene** — `timeline_scene_select` · done
- Done: `Project.scenes` (ordered MovieClip symbol IDs, first = `root`, format 6); Scene select lists and switches scenes, New scene and Insert ▸ Scene add one (one Undo); scene symbols are hidden from the Library; Control ▸ Play all scenes continues editor playback into the next scene. Runtime/HTML5 flatten scenes into one main timeline (`neoflash_core::flatten_scenes`); the Scene panel (PP-08) renames, duplicates, deletes and reorders. Not yet: Control ▸ Test scene, breadcrumb ▾ (ST-08).
- Purpose: Flash scenes — consecutive main timelines played in order; the Scene select switches the edited scene.
- Behaviour: menu lists scenes (checked = current) + New scene; selecting a scene leaves symbol editing and shows its
  timeline. New scene is one Undo step. Scene panel (PP-08) adds rename/duplicate/delete/reorder.
- Data: `Project.scenes: Vec<Id>` (scene MovieClip symbols in play order; empty = one scene, `root`).
- Depends on: engine — playback across scenes, `gotoAndPlay(frame, scene)`, `currentScene`, export (done).
- Acceptance: two scenes play in order in the editor preview and HTML5; AS3 `scenes`/`currentScene` report them
  (bounded); legacy one-scene documents load unchanged.

**TL-04 · Shape tween** — `timeline_frame_tools` (tween group, item 2) · done
- Done: `Keyframe.tween_kind = Shape`; general morph in `neoflash_core::shape_tween` (different contours, rect/oval/path, gradients); diagnostic when a keyframe lacks shapes. Not yet: shape hints.
- Purpose: morph raw shapes between two keyframes (green span, `timeline/shape-tween`).
- Behaviour: enabled on a keyframe whose layer holds shapes (paths, rectangles, ovals) in this and the next keyframe;
  click toggles a shape tween. Mixed content or symbols → diagnostic "Shape tweens need shapes on both keyframes".
  Shape hints (View ▸ Show shape hints, Modify ▸ Shape ▸ Add shape hint) later.
- Data: `Keyframe.tween_kind: TweenKind { Classic, Shape, Motion }` qualifies `Keyframe.tween`; shape hints would be
  **NEW** (`Keyframe.shape_hints`, wiring plan §7.4 row 10).
- Depends on: engine — contour correspondence for differing topology (done in `shape_tween.rs`); span colour per kind.
- Acceptance: shape tween marker renders green with arrow; paths with matching and differing topology morph; a
  keyframe without shapes produces an explanation instead of a jump.

**TL-05 · Motion tween** — `timeline_frame_tools` (tween group, item 3) · partial
- Done: `tween_kind = Motion`; ◆ property keyframes are keyframes with `keyed_properties`; editing inside the span keys only changed properties (`timeline_commands::record_motion_property_keys`, run on every history commit); F6 keys all; independent tracks with the span ease (`evaluate::sample_motion`). The selected instance's path is drawn on the stage (`NF2/Motion Path`, R-005) while the playhead is in its span; dragging a key point ◆ moves that key and keys X/Y, one Undo (`timeline_motion.rs`). Motion editor (PP-02) and Properties — Motion tween (PR-10) are partial. Not yet: Bézier path editing between keys, per-property eases, Orient to path evaluation for motion tweens (wiring plan §7.4).
- Purpose: object-based tween with per-property keyframes ◆ (blue span) and an editable motion path on stage.
- Behaviour: on a frame with exactly one symbol instance → convert span to a motion tween; dragging the instance on a
  later frame creates property keyframes; path visible on stage; edited in Motion editor (PP-02) and Properties —
  Motion tween (PR-10). Non-symbol content → offer Convert to Symbol (F8) or a diagnostic.
- Data: `tween_kind = Motion` on the span start key; ◆ keys = `Keyframe.keyed_properties` (tracks are derived from
  them, not stored separately).
- Depends on: engine (done for tracks; path curves and per-property eases open).
- Acceptance: create/remove with one Undo each; ◆ markers render; X/Y/rotation/scale/alpha tracks interpolate
  independently in editor and HTML5; classic tweens unchanged.

**TL-06 · Ease panel button** — `timeline_frame_tools` · done
- Done: enabled only while the playhead is inside a tween (classic, shape or motion); opens the Ease panel (PP-01). Frame
  menu ▸ Ease… and Window ▸ Ease open it too.
- Purpose: open the Ease panel (PP-01) for the selected classic tween.
- Behaviour: enabled when the current frame is inside a tweened span; click opens/focuses the Ease panel bound to that
  keyframe.
- Data: `Easing::{Linear, EaseIn, EaseOut, EaseInOut, CubicBezier, Flash}` exists.
- Depends on: editor (PP-01).
- Acceptance: enabled only inside a tween; opens Ease showing the keyframe's current curve.

**TL-07 · Sound** — `timeline_frame_tools` · done
- Done: menu of library sounds + Import sound… (MP3/WAV) sets the current span's keyframe sound (default Event) as one Undo and selects that frame so Properties shows Sound; Sound layers with Sound spans (the span painter's waveform is decorative, not decoded peaks); Event/Start/Stop/Stream in HTML5 and in editor playback (rodio). Scrubbing is silent (documented limit).
- Purpose: attach a library sound to the current keyframe (sound layer) and open Properties — Sound.
- Behaviour: menu of library sounds + "Import sound…"; choosing one sets the keyframe's sound (default Sync Event).
- Data: `Keyframe.sound: Option<FrameSound>` referencing an `audio/*` `ResourceUnit` (decision 0037).
- Depends on: engine (done: `Player` `SoundCommand`s, `web/audio.js`, editor rodio preview).
- Acceptance: waveform span on the layer; Event/Start/Stop/Stream behave in HTML5 preview; scrubbing with Stream
  sync plays audio (or a documented limitation).

**TL-08 · Edit multiple frames** — `timeline_onion_group` · partial
- Done: toggle; stage draws every keyframe in the onion range opaque; a Move gesture applies the same offset to the selected objects on all those keyframes (one Undo). Not yet: selecting objects that exist only on other keyframes; scale/rotate across frames.
- Purpose: show and edit content of all keyframes inside the onion range at once (Flash "Edit multiple frames").
- Behaviour: toggle; while on, stage shows every keyframe in the range opaque; selection/move/transform apply to
  objects on several keyframes; one Undo for the whole gesture.
- Data: selection targets are per keyframe; no model change.
- Depends on: editor (selection across keyframes; `animation-frame-ranges` limit "simultaneous artwork editing").
- Acceptance: select-all + move shifts objects on all keyframes in range with one Undo; off restores normal view.

**TL-09 · Frame view ▸ Short rows** — `timeline_frame_view_menu` · done
- Done: `TimelineView.short_rows`; row height is passed to painters and hit tests.
- Purpose: 18 pt layer rows (`metrics::LAYER_ROW_SHORT`).
- Behaviour: checked menu item; layer list, grid, hit testing and drag targets all use the short height.
- Data: workspace pref. `timeline_paint::ROW_HEIGHT` is a constant today → must become per-view.
- Depends on: UI-only (editor).
- Acceptance: rows 18 pt; clicking/dragging frames and layers hits the right row; preference persists.

**TL-10 · Frame view ▸ Preview** — `timeline_frame_view_menu` · done
- Done (2026-11-01): Preview and Preview in context (checked rows, saved in `TimelineView.preview`) switch rows to `metrics::LAYER_ROW_PREVIEW` (44) and paint a ≤ 62 × 38 thumbnail after each span's keyframe cell with the stage painter: the layer's own content, or the whole stage on the document background. Evaluated scenes are cached per (symbol, layer, keyframe, `History::revision`), only for visible spans, at most 12 new per repaint (`timeline_preview.rs`).
- Purpose: thumbnails of keyframe content inside spans (Preview) and in stage context (Preview in context).
- Behaviour: when on, row height grows (Large) and each span shows a thumbnail of its first frame; cached per keyframe
  revision.
- Data: none; render from `evaluate` / `AuthoredScene`.
- Depends on: editor (offscreen thumbnail rendering + cache).
- Acceptance: thumbnails update after edits; 1000-frame timelines stay responsive (virtualised).

**TL-11 · Frame view ▸ Snap playhead to keyframes** — `timeline_frame_view_menu` · done (⇧ suspends snapping)
- Purpose: dragging the playhead snaps to keyframes of the current layer (Preferences ▸ Timeline has the default).
- Behaviour: checked item; scrub snaps within ~4 pt of a keyframe; holding ⇧ (verify convention) suspends snapping.
- Data: workspace pref. Depends on: UI-only.
- Acceptance: scrub lands exactly on keyframes when on; free scrubbing when off.

**TL-12 · Frame view ▸ Placement / Preview in context / Help / Close panel** — partial
- Done: Placement Bottom/Top/Left/Right/Floating, Close panel, Preview in context (TL-10). Not yet: Help (F1).
- Purpose (§5.4): move the Timeline to Bottom / Top / Left / Right / Floating; Help (F1); Close panel.
- Behaviour: Placement items call the existing dock/detach commands (`set_panel_placement`); Close hides the panel.
- Data: workspace layout. Depends on: UI-only. Missing menu rows require a design check that the existing menu
  composition may grow (**request** if the rows must be added).
- Acceptance: each placement docks the timeline accordingly and is restorable.

**TL-13 · Layer types Sound / Video / Armature / Motion guide** — `panels/timeline.rs` `layer_kind` · partial
- Done: Motion guide; Sound (a layer whose keys hold frame sounds and no artwork). Not yet: Video, Armature.
- Purpose: NF2 `TL Layer` types with their span renderings (waveform, filmstrip, ◆ poses, guided indent).
- Data: `LayerKind` in `neoflash-ui` has the variants; `LayerRole` lacks them. **NEW** roles/derivation.
- Depends on: engine (TL-01, TL-07, PR-09, TO-05).
- Acceptance: legend states from `timeline-states-legend.png` appear for real documents.

**TL-14 · Button symbol timeline labels** — verify
- Purpose: 4-frame Up / Over / Down / Hit timeline with labelled columns.
- Note: `AuthoredPlayback` already treats button roots specially; whether the ruler shows state labels was not
  verified. Acceptance: ruler shows Up/Over/Down/Hit on button timelines; Hit content is not drawn at runtime.

## 2. Tools (`crates/neoflash-editor/src/panels/tools.rs`, `authoring_tools.rs`, `app.rs::Tool`)

Implemented tools: Selection, Subselection, Free Transform, Gradient Transform, Lasso, Pen, Line, Text, Rectangle,
Oval, PolyStar, Pencil, Brush, Paint Bucket, Ink Bottle, Eyedropper, Eraser, Hand, Zoom. The module comment says the
others are "omitted rather than faked". Adding a tool button is allowed only because the NF2 Tools panel contains it;
its icon, group and order follow `panel-tools-floating` / `NF2/Tools Panel` exactly (otherwise **request**).

**TO-01 · Rectangle primitive / Oval primitive** — done (parametric options on `Rectangle` / `Oval` in Tool options and Properties PR-02)
- Done: `Transform.primitive: Option<PrimitiveOptions>` stores Rectangle `corner_radii: [f64; 4]` (linked by default)
  and Oval `start_angle`, `end_angle` (`0–360°`), `inner_radius` (`0–99 %`) and `close_path` in document format 7.
  Geometry builders in `neoflash_core::geometry` drive evaluation, shape morphs, hit testing, native painting and HTML5
  export. `Modify ▸ Break Apart` (`⌘B`) converts a parametric rectangle or oval into its `Path`.
- Purpose: drawing objects that keep parametric corner radii / start-end angle / inner radius after drawing.
- Behaviour: drag draws; ⇧ constrains; ⌥ from centre; Properties shows PR-02 options; Break Apart converts to paths.
- Data: format-7 `Transform.primitive: Option<PrimitiveOptions>` on `ElementKind::Rectangle/Ellipse`.
- Depends on: engine (geometry generation in evaluation, export, runtime renderer).
- Acceptance: radii/angles editable after drawing with Undo; HTML5 renders identically; Break Apart yields paths.

**TO-02 · PolyStar** — done (`Tool::PolyStar` in `panels/tools.rs`, stage drag in `vector.rs`, modal settings dialog in `authoring_tools.rs`)
- Done: PolyStar tool in the Tools shape group + `PolyStar settings…` modal dialog (`Esc` cancels, `OK`/`Enter` applies
  Style `Polygon` / `Star`, Sides `3–32`, Star point size `0.05–1.0`); dragging on Stage authors a closed `ElementKind::Path`
  from the centre with one Undo step.
- Purpose: draw regular polygons/stars; settings: Style Polygon/Star, Sides 3–32, Star point size 0–1.
- Behaviour: Tool options → settings dialog (Esc/Enter); drag draws from centre; result is a `Path`.
- Data: produces existing `ElementKind::Path`.
- Acceptance: 5-point star with 0.5 point size matches expected vertices; one Undo.

**TO-03 · Deco** — absent
- Purpose: procedural fills (Vine fill, Grid fill, Symmetry brush …) using leaf/flower symbols (§6 Deco panel).
- Data/Depends on: engine (procedural generators producing instances/paths). P3.
- Acceptance: Grid fill fills a closed area with instances of the chosen symbol; deterministic with a seed.

**TO-04 · Spray brush** — absent
- Purpose: spray dots or symbol instances with random scale/rotation (§6 Spray brush panel).
- Data: produces existing `Path` dots or `Instance` elements. Depends on: editor (seeded random placement). P3.
- Acceptance: spraying produces N elements per second with settings honoured; one Undo per stroke.

**TO-05 · Bone / Bind (IK)** — absent
- Purpose: armatures linking instances or shape points; poses as ◆ on an Armature layer; joint limits, springs.
- Data: none. **NEW** armature, bones, joint constraints, pose keyframes. Depends on: engine (IK solver, runtime
  armatures). P3.
- Acceptance: dragging the end bone solves the chain within joint limits; poses interpolate in HTML5.

**TO-06 · Snap toggle** — done (`panels/tools.rs`, `ViewPrefs.snap_to_objects`)
- Done: Snap to objects (magnet) toggle at the bottom of the Tools rail (`Selected` state when active, persisted in view
  preferences) enables object snapping across drawing, selection moves, transform handles and path point drags (ST-04).
- Purpose: global Snap to objects (magnet) toggle.
- Behaviour: Selected style when on; affects drawing and dragging (ST-04).
- Data: workspace/document view setting. Depends on: editor (ST-04).
- Acceptance: with snap on, dragging an object's registration point snaps to another object's edge/centre with the
  NF2 snap ring.

**TO-07 · Tool options: Object drawing, Smooth / Straighten, Snap to objects** — absent in options area
- Purpose: Flash Options section — Object drawing mode (J), pencil Straighten/Smooth/Ink, brush shape.
- Behaviour: options live in the existing tool-options popover / Properties ▸ Tool; current options are brush size
  and eraser mode only.
- Data: Merge vs Object drawing needs NEW merge semantics for raw shapes (today every draw creates a separate
  element). Smoothing: editor algorithm on pencil input.
- Depends on: editor (smoothing), engine (merge drawing).
- Acceptance: Straighten turns a rough rectangle stroke into lines; Object drawing toggles a stored default.

**TO-08 · Fill/stroke chip → Swatches popover** — `fill_stroke_chip` · partial
- Purpose: §6 says the chip opens the NF2 Swatches popover; today it opens an egui RGB colour picker in a rail popover.
- Behaviour: hex + alpha, eyedropper, no colour, wheel → Color panel, Recent / Document / Palette / Gradients.
- Data: `Editor.fill/stroke`; Recent list (editor), Document colours (scan `Project`).
- Depends on: UI-only, but it **changes the component used** → confirm via NF2-REQUESTS.md before switching.
- Acceptance: choosing a swatch sets the drawing colour; alpha preserved; Recent updates.

## 3. Stage chrome (`crates/neoflash-editor/src/shell.rs`, `stage.rs`, `panels/menu.rs`)

**ST-01 · Stage rulers + ruler corner** — working. Implemented: see [NF2-RULERS-GUIDES-GRID.md](NF2-RULERS-GUIDES-GRID.md) (behaviour, data model, invariants, tests).
- Purpose: 20 pt horizontal/vertical rulers in stage units; corner drag moves the zero point, double-click resets.
- Behaviour: Show via View ▸ Rulers (⌥⇧⌘R) and the zoom-control toggle; labels Auto (10/50/100/500 so labels never
  overlap) or Every N px; pointer position marker; dragging out of a ruler creates a guide (ST-02).
- Data: view state (show, units, labels, origin). Not document content (Flash stores ruler state per document —
  decide: workspace pref vs document view settings, see ST-02).
- Depends on: UI-only, but `neoflash-ui` has no `NF2/Stage Ruler` / `NF2/Ruler Corner` painter and the editor may not
  hand-style widgets → **request** the components first (tokens `color/guide`, `color/stage-grid` exist).
- Acceptance: tick 0 meets the stage origin at every zoom/pan; labels never overlap; toggle persists.

**ST-02 · Guides** — working; per-timeline `Timeline::guides`, document version 6. Implemented: see [NF2-RULERS-GUIDES-GRID.md](NF2-RULERS-GUIDES-GRID.md) (behaviour, data model, invariants, tests).
- Purpose: horizontal/vertical reference lines (`color/guide`), Default / Selected / Locked states.
- Behaviour: create by dragging from a ruler; move by dragging (unless locked); drag back onto a ruler or Delete to
  remove; double-click → exact position (verify NF2 has a field; else request); View ▸ Guides ▸ Show / Lock / Snap /
  Clear (⌘;, ⌥⌘;, ⇧⌘;); one Undo per add/move/remove if guides are document data.
- Data: none. **NEW** per-timeline guide list (Flash saves guides per scene/symbol in the document).
- Depends on: engine (small: serialized authoring-only data, never exported).
- Acceptance: guides survive save/open; never appear in HTML5; locked guides cannot be dragged.

**ST-03 · Stage grid** — working. Implemented: see [NF2-RULERS-GUIDES-GRID.md](NF2-RULERS-GUIDES-GRID.md) (behaviour, data model, invariants, tests).
- Purpose: `NF2/Stage Grid` overlay (20 px, every 5th line stronger), below content unless "Show over objects".
- Behaviour: View ▸ Grid ▸ Show grid (⌘'), Snap to grid (⇧⌘'), Show over objects, Edit grid… (⌥⌘G → View settings).
- Data: view settings (spacing W/H linked, colour, snap accuracy). Depends on: UI-only + component **request**.
- Acceptance: lines align to stage origin at all zooms; over-objects toggles stacking.

**ST-04 · Snapping + snap ring** — done (View ▸ Snapping submenu waits for R-003): grid, guide, object (bounds corners, centres and instance registration points on visible, unlocked layers) and pixel snapping (`Guides > Objects > Grid > Pixels`) with `NF2/Snap Ring` and `⌘`-suspend work across moves, scale/resize handles, rectangle/oval, line, pen anchors and path points (`snapping.rs`, `stage_view.rs`). Implemented: see [NF2-RULERS-GUIDES-GRID.md](NF2-RULERS-GUIDES-GRID.md) (behaviour, data model, invariants, tests).
- Purpose: snap dragged/drawn points to grid, guides, objects (edges, centres, registration points), pixels; show
  `NF2/Snap Ring` while dragging (larger ring when snapped).
- Behaviour: snap accuracy Must be close / Normal / Can be distant (View settings); tolerance from Preferences ▸
  Drawing; ⌘ suspends snapping while dragging (verify Flash convention); works for move, transform handles, shape and
  pen points.
- Data: settings only. Depends on: editor (snap candidate index); `neoflash-ui` has no Snap Ring painter → request.
- Acceptance: with grid snap on, rectangle corners land on grid intersections; snapping never changes untouched
  objects; Undo covers the drag only.

**ST-05 · Zoom control toggles** — working (`ZoomControl` gets `Some(state)`). Implemented: see [NF2-RULERS-GUIDES-GRID.md](NF2-RULERS-GUIDES-GRID.md) (behaviour, data model, invariants, tests).
- Purpose: quick toggles for rulers, guides, grid, snap to guides in the floating zoom control.
- Behaviour: pass `Some(state)` once ST-01…ST-04 work; actions `ZoomAction::{Rulers, Guides, Grid, Snap}` toggle the
  same state as the View menu.
- Depends on: UI-only. Acceptance: toggles and menu items stay in sync.

**ST-06 · View settings popover** — working (`panels/view_settings.rs`; the planned preview was removed). Implemented: see [NF2-RULERS-GUIDES-GRID.md](NF2-RULERS-GUIDES-GRID.md) (behaviour, data model, invariants, tests).
- Purpose: `NF2/View Settings` (rulers, guides, grid) — see PP-12 for controls.
- Behaviour: opens anchored to the ⚙ button and from View ▸ Rulers, guides & grid…; edits apply live; Clear guides
  (one Undo); Save as default writes preferences.
- Data: `neoflash_ui::components::ViewSettingsState` + `ViewSettingsResponse { close, clear_guides, save_default }`.
- Depends on: UI-only (+ ST-01…ST-04). Acceptance: every control changes the stage immediately; Save as default
  survives restart.

**ST-07 · NF2 stage cursors** — done (`stage_cursor.rs`)
- Done: every stage tool shows its NF2 cursor over the canvas only (rulers, floating stage chrome, popovers and
  dialogs keep system cursors; guides keep their system resize cursor). Hover targets: Selection — `arrow-move` over an
  object; Free Transform — `resize-h/v/d1/d2` on handles (by screen direction), `skew-h/v` on the edge between handles,
  `rotate` just outside a corner, `move` inside the box; Subselection — `subselect-point` over a path point; Pen —
  `pen-close` over the first anchor and `pen-add` over a selected path segment; Hand — `hand-closed` while dragging;
  Zoom — `zoom-out` with ⌥; motion path ◆ — `hand-open/closed`. The mapping follows Flash conventions; the design has no
  per-handle table (R-011). Cursors are identical in both themes. Not yet: `arrow-reshape-curve/corner`,
  `pen-remove/convert`, `drag-copy`.
- Purpose: 39 `NF2 Cursor/*` glyphs (tool cursors, reshape corner/curve, move, rotate, skew, hand open/closed,
  zoom in/out, eyedropper, bucket, pen add/remove/close …) over the canvas.
- Behaviour: stage tools call `neoflash_ui::cursors::show(ctx, canvas, cursor)` (hides the system cursor); chrome
  keeps system cursors; cursor reflects hover target (e.g. reshape curve near a stroke).
- Data: none. Depends on: UI-only. Acceptance: each tool shows its NF2 cursor only over the canvas; hotspot accurate
  (click lands at the glyph's hotspot).

**ST-08 · Breadcrumb ▾ menu + Symbols & scenes popover** — `stage_chrome` (`BreadcrumbItem { menu: false }`) · absent
- Purpose: ▾ lists sibling scenes/symbols with search; popover for large projects (sections Scenes · Matches ·
  Recent · Browse, keyboard ↑↓ ↵ ⌥↵ ⌘↵); Breadcrumb overflow "…" for long paths.
- Data: `Project.symbols`, library folders; Recent = editor history. Depends on: editor (scenes need TL-03).
- Acceptance: with 300 symbols, typing filters in < 1 frame; ↵ edits the symbol; ⌥↵ edits in place.

**ST-09 · Toasts / progress / spinner** — absent
- Purpose: `NF2/Toast` Info/Success/Warning/Error for transient results (saved, exported, import warnings);
  `NF2/Progress` and `NF2/Spinner` for long work (publish, export, import).
- Behaviour: toasts stack bottom-centre/right (verify placement in reference), auto-dismiss Info/Success, Warning
  and Error stay until closed, optional action ("Show in Output"). Messages also go to Output.
- Data: editor status/messages (`Editor::message`, `status`). Depends on: UI-only (components exist:
  `Toast`, `progress`, `spinner`). Placement not specified in §4 → **request** if unclear.
- Acceptance: export success shows a Success toast; a failed open shows an Error toast with the message; screen
  readers get the text.

**ST-10 · Dock compass** — absent (editor uses `dock_target` edge zones)
- Purpose: `NF2/Dock Compass` — 5 drop targets (edges + centre = add as tab) shown while dragging a panel.
- Depends on: UI-only (`components::dock_compass` exists). Acceptance: dropping on centre tabs the panel into the
  group; edges split.

**ST-11 · Canvas editing chrome variants** — partial
- Done: Selection and Subselection draw `NF2/Selection Box` per selected placement (Symbol for instances, Static text
  for point text, Dynamic text for text boxes, Group otherwise); Free Transform draws `NF2/Transform Box` (Scale mode)
  on the selection's axis-aligned stage bounds, also for rotated objects, with Flash zones and a draggable
  `NF2/Transformation Point` (`Transform.pivot` in format 7 for single placements + transient group pivot; rotation and
  `⌥`-scale pivot around it, double-click resets to centre, one Undo step): handles scale (a single placement resizes in
  its own axes), edges skew (`transform_geometry::skew_many`), outside a corner rotates about the transformation point,
  inside moves. Marquee uses `NF2/Marquee`; path points and pen anchors use `NF2/Anchor Point` and `NF2/Bezier Handle`
  (Pen click on a selected path segment subdivides to insert an anchor; Subselection `Delete`/`Backspace` or `⌥`-click
  deletes selected anchors). Open request R-011 (rotated box, contrast over artwork, Light transformation point).
  Screenshot aid: `NEOFLASH_SCREENSHOT_SELECT=select|transform|subselect` selects everything on the frame with that tool.
- Purpose: `NF2/Selection Box` kinds (Group, Symbol + registration point, Static text round handle, Dynamic text
  square handle, Primitive control points), `Transform Box` scale vs rotate/skew, Anchor / Bezier handle, Marquee,
  Shape Selection hatch, Gradient Transform focal triangle.
- Behaviour: missing parts — radial focal handle (`editor-paint-effects` limit), on-stage primitive corner/angle handles.
- Still `StageOverlay`: rectangle/oval/line/polystar drafts, pen/pencil draft stroke, lasso, brush/eraser size circle and the
  gradient tool (2 handles; `NF2/Gradient Transform` has 5 that need focal/width gestures).

## 4. Properties variants (`crates/neoflash-editor/src/panels/properties.rs`, `text_boxes.rs`, `paint_controls.rs`)

**PR-01 · Properties — Text** — partial (`TextBox` font/size/colour/leading/alignment/wrap work)
- Missing NF2 controls: text type Static / Dynamic / Input; orientation; Embed…; tracking (letter spacing); Auto
  kern; anti-alias (readability / animation / device); selectable; render as HTML; border; superscript / subscript;
  paragraph indent / margins (fields exist in `TextFormat` — verify editor exposure); behaviour single line /
  multiline / no wrap / password; Options; Filters.
- Data: `ElementKind::TextBox { text, width, height, format: TextFormat, word_wrap, multiline }`; `TextFormat` has
  letter_spacing, kerning, indent, margins, url/target. Runtime `TextField` has type/selectable/password for scripts
  (runtime only). **NEW** authored text type, selectable, html, border, anti-alias, max chars, variable name.
- Depends on: engine (authored dynamic/input text in export; HTML subset); Embed… needs DL-11.
- Acceptance: Input text authored in the editor is editable in HTML5; Dynamic text has an instance name reachable
  from AS3/AS2; Static text stays non-selectable unless "selectable".

**PR-02 · Properties — Oval / Rectangle primitive options** — done (`authoring_tools.rs::primitive_options_properties_ui`, wired into `panels/properties.rs` and Tool options; see TO-01)
- Controls: Oval: Start angle, End angle, Inner radius %, Close path, Reset. Rectangle: four corner radii (linked by
  default), radius slider, Reset.
- Acceptance: values clamp (angles 0–360, inner radius 0–99 %); Reset restores defaults with one Undo.

**PR-03 · Stroke style** — absent (Properties fill & stroke section, Stroke style dialog)
- Controls: stroke size `Combo Field`; `Line Style Select` Solid / Dashed / Dotted / Ragged / Stipple / Hatched +
  Custom…; Stroke hinting; Scale Normal / Horizontal / Vertical / None; Miter; Cap none / round / square; Join miter /
  round / bevel. Dialog: type, thickness, 4× zoom preview, dash/gap, sharp corners.
- Data: elements have `stroke`, `stroke_width` only; path strokes are round caps/joins. **NEW** `StrokeStyle`.
- Depends on: engine (renderer, export, hit testing). Ragged/Stipple/Hatched are P3; Solid/Dashed/Dotted + caps/joins
  P2. Acceptance: dashed 2 px square-cap line renders the same in editor and HTML5.

**PR-04 · Movie clip — Colour effect styles** — partial (only Opacity row; Style / Tint / Brightness / Advanced rows requested as R-012)
- Purpose: Style None / Brightness / Tint / Advanced / Alpha + value slider (Tint: colour + %, Advanced: 8 fields).
- Data: `Transform.color_transform: ColorTransform` already exists and interpolates in tweens (format 5).
- Depends on: editor + R-012. Acceptance: Tint 50 % red shows in editor and HTML5; Undo restores; tween between two
  tints interpolates.

**PR-05 · Movie clip — Display & Filters** — preview `PropertiesFilters`
- Controls: Display: Visible, Cache as bitmap, Blending (Normal, Layer, Multiply, Screen, Lighten, Darken,
  Difference, Add, Subtract, Invert, Alpha, Erase, Overlay, Hardlight), Render Original / Export as bitmap. Filters
  table (Drop shadow, Blur, Glow, Bevel, Gradient glow/bevel, Adjust color) with add / duplicate / remove, per-filter
  fields (Blur X/Y, Strength, Quality …), enable checkbox.
- Data: authored `Element` has no filters/blend/cache/visible. Runtime has `FilterSpec` (Blur, Glow, DropShadow,
  Bevel, ColorMatrix …, max 8 per object) and AS2 blend modes (runtime only). **NEW** authored fields reusing
  `FilterSpec`.
- Depends on: engine (authored → runtime display graph, native editor preview raster). Acceptance: drop shadow on
  an instance appears in editor and HTML5; filters order preserved; max-8 limit reported as a diagnostic.

**PR-06 · Movie clip — Swap…, Parameters tab** — partial (Library `Swap symbol` works; Properties `Swap…` picker and Stage context menu `Swap symbol…` requested as R-012)
- Done: selecting an instance on Stage and choosing `Swap symbol` on a Library symbol (row context menu or ⋯ menu)
  replaces `ElementKind::Instance.symbol_id` while preserving transform, instance name, scripts, accessibility and
  compatible playback settings (`authoring_symbols.rs::swap_instance_symbol`, cycle-safe, one Undo).
- Swap…: Properties `Swap…` button + picker dialog/popover and stage context menu `Swap symbol…` wait for R-012.
- Parameters tab: component parameters — no component model. **NEW**, P3.
- Acceptance: Swap keeps position/filters/name, one Undo; Undo restores the original symbol.

**PR-07 · Graphic — thumbnail strip** — partial (Looping and first frame work via `instance_playback_ui`)
- Purpose: clickable thumbnail strip to pick the first frame. Depends on: editor (thumbnails as TL-10).

**PR-08 · Properties — Sound (frame)** — `panels/properties_sound.rs` · partial
- Done: Sound section of frame Properties when the keyframe has a sound: sound select (None + library sounds), Effect presets, Sync, Repeat/Loop + count, file info, Play (native preview). Not yet: Envelope… and Sound properties… open their planned dialogs (DL-09, DL-08); the waveform is the decorative span painter.
- Controls: Label name/type; sound file select; Effect (None, Left/Right channel, Fade left→right / right→left, Fade in,
  Fade out, Custom) + Envelope… (DL-09); Sync Event / Start / Stop / Stream; Repeat / Loop + count; waveform; file info;
  Play; Sound properties… (DL-08).
- Data: `FrameSound { resource, sync, loops, loop_forever, effect }` (TL-07); Custom envelope is **NEW** (DL-09).
  Acceptance: Event sound plays once per keyframe hit; Stop stops it; Stream follows playhead.

**PR-09 · Properties — Video** — partial (W/H + NetStream hint)
- Missing: tabs Properties / Cue points; preview with transport; instance name + Swap…; file info; Deployment
  Embedded / External / Progressive; Autoplay; Loop; Muted; Skin; Scale Fit / Fill / None; Volume; cue points table
  (name, time, Event / Navigation) add/remove; Edit in timeline.
- Data: `ElementKind::Video { width, height }`; runtime `video.rs` / `web/video.js` play scripted NetStream.
  **NEW** source resource, deployment, playback flags, cue points.
- Acceptance: an authored video with Autoplay plays in HTML5 without script; cue point fires an AS3 `cuePoint` event
  (bounded).

**PR-10 · Properties — Motion tween** — partial (`panels/motion_tween.rs`)
- Done: replaces the frame Tween section while the playhead is inside a motion tween. Ease (Flash acceleration of the
  span start key) + Edit curve… (opens PP-01); Rotate turns + CW/CCW (written as the last Rotation ◆ key); Orient to
  path (`Keyframe.orient_to_path` on the span start key); Path X/Y/W/H moves and scales the X/Y keys. The path itself
  is drawn on the stage (R-005 resolved). Orient to path turns the object along its X/Y track. Not yet: Show path / Show frame dots toggles, Sync graphic symbols and Snap to motion guide stay disabled ("Not
  available yet"). Task cards: wiring plan §7.4 rows 4, 6, 7.
- Controls: Ease −100 in … +100 out + Edit curve… (→ PP-01); Rotation count + direction CW/CCW + Orient to path; Path
  X/Y/W/H; Show path / frame dots; Sync graphic symbols; Snap to motion guide.
- Data: values live in the span keys (TL-05). Acceptance: Orient to path rotates the object along the path tangent
  in editor and HTML5.
- Reusable: `Editor::set_orient_to_path` (any tweened span); `TimelineEdit::SyncSymbols` / `Editor::sync_symbols`;
  `timeline_motion.rs::paint_motion_path`.

**PR-11 · Frame easing values** — done (`properties` Tween section)
- Done: Y1/Y2 accept −100…100 (X stays 0–1); "Flash ease" option with an Acceleration field −100…100.
- Behaviour: allow Y outside 0–1 (Ease panel ranges: −30…130 % in the overshoot graph); add Flash acceleration
  −100…100 as the existing field style (no new control type — if a field must be added, request).
- Depends on: UI-only. Acceptance: Y2 = 1.12 saves, reloads and overshoots in editor and HTML5.

## 5. Color panel (`crates/neoflash-editor/src/authoring_modify.rs` › `color_panel_*`, `fill_type_select`)

**CO-01 · Fill type Linear / Radial gradient** — `fill_type_select` · done (`authoring_modify.rs`)
- Done: Color panel switches both fill and stroke paints between None, Solid colour, Linear gradient and Radial gradient
  with `NF2/Gradient Bar` (click to add stops, drag to reposition, drag vertically off to remove when > 2 stops) and
  `NF2/Color Picker` editing of the selected stop. Solid rectangles, ovals and paths convert cleanly to `PaintedPath` via
  `Element::to_path_commands()` with one Undo step. Not yet: Flow `Reflect` / `Repeat` and `Linear RGB` remain disabled
  with an explanatory tooltip while runtime `GradientPaint` uses Extend (pad) spread and sRGB interpolation.
- Purpose: Color panel edits gradients: `NF2/Gradient Bar` with draggable stops (click adds, drag off removes),
  Flow extend / reflect / repeat, Linear RGB, selected stop edited by the picker; on-stage via Gradient Transform.
- Data: `ElementKind::PaintedPath { fill_gradient, stroke_gradient: Option<GradientPaint> }`.
- Acceptance: switching a filled path to Radial creates a 2-stop gradient with one Undo; stops editable; HTML5 matches.

**CO-02 · Fill type Bitmap fill (+ "Color — Bitmap fill" panel)** — disabled
- Purpose: fill with a library bitmap (grid + Import…, Lock fill). Data: none (`editor-paint-effects` limit).
  **NEW** bitmap paint. Depends on: engine. P3.

**CO-03 · Swatches tab, panel ⋯ Add swatch, colour picker area** — partial (`NF2/Color Picker` SV/HSB/RGB area, Color panel ⋯ `Add swatch` and dockable `Swatches` panel `PP-10` work; inline Color/Swatches tab switch inside Color panel remains open)
- Purpose: Color panel tabs Color / Swatches; ⋯ menu Add swatch · Help · Close · Close group; `NF2/Color Picker`
  (SV area, hue, alpha strips), RGB / HSB segmented.
- Data: editor swatch prefs (`WorkspacePrefs.swatches`) + document paint scan (`panels/swatches.rs`); persistent
  project-stored swatches are W1.9.

## 6. Library (`crates/neoflash-editor/src/panels/library.rs`, `resources.rs`)

**LI-01 · Delete / duplicate item** — done (`panels/library.rs`, `authoring_symbols.rs`)
- Done: `Duplicate symbol` (row context menu and Library ⋯ menu) clones any non-scene symbol as `<name> copy` with fresh
  IDs and cleared class/export linkage in one Undo step. `Delete symbol` (row context menu, ⋯ menu and footer Trash icon)
  deletes the symbol and removes any placed instances across timelines (exiting nested editing if inside it) in one Undo
  step, reporting the removed placement count so no dangling `symbol_id` can ever remain.
- Data: `Project.symbols`; no model change.
- Acceptance: deleting an unused or used symbol + Undo restores the symbol and all placements cleanly.

**LI-02 · Bitmap / sound / font / video items, document select, pin** — absent
- Purpose: Library tree shows all item types with icons; document select switches between open documents; pin keeps
  the library on one document.
- Data: images/audio/video are `ResourceUnit`s managed in `resources.rs` (separate panel). Fonts are not library
  items (**NEW** with DL-11). Multiple open documents are not supported (`editor-workspace` limit) → document select
  shows only the current document.
- Depends on: editor. Acceptance: an imported PNG appears in the Library tree and can be dragged to the stage.

## 7. Actions and code editor (`panels/actions.rs` `CODE_TOOLS`, `sources.rs`)

Status: AC-02, AC-05 and AC-06 work and AC-01 re-indents only (details in [NF2-ACTIONS-EDITOR.md](NF2-ACTIONS-EDITOR.md));
AC-03, AC-04 and AC-07 stay disabled; AC-08 is partial.

| ID | Element | Purpose / behaviour | Data / engine | Acceptance |
| --- | --- | --- | --- | --- |
| AC-01 | Auto-format | Reformat the current script (indent, braces, spacing) per language; one Undo; refuses on parse error with the error shown. | Needs a formatter per language: JS/TS via the existing oxc stack in `neoflash-script` (verify printer availability); AS2/AS3 needs a pretty-printer on the AS parser. | Formatting is idempotent; comments preserved. |
| AC-02 | Show code hint | Popover with members for the identifier before the caret (`Menu Item` + type on the right); ⌃Space; delay from Preferences ▸ Code editor. | Symbol table from `neoflash-script` project compilation + the declaration catalogue (`docs/progress/actionscript-catalog.json`) — **NEW** completion API. | `this.` lists MovieClip members supported by the runtime; unsupported ones are not offered as working. |
| AC-03 | Debug options | Toggle breakpoint on line, remove all, start debugging (→ PP-04). | Debugger (PP-04). | Breakpoint set from the gutter appears in Debugger ▸ Breakpoints. |
| AC-04 | Collapse between braces / expand all | Folding ranges in the code editor with ▸/▾ gutter markers. | Brace matching on the token stream (`components::code::tokens`). Editor only. | Folded code keeps text and Undo intact. |
| AC-05 | Block comment / line comment / uncomment | Toggle `/* */` or `//` over the selection/line; one Undo. | Editor only (language comment syntax is identical for AS/JS/TS). | Round-trip comment → uncomment restores the exact text. |
| AC-06 | Toolbox sidebar + Script navigator | Left sidebar: toolbox search + tree (Top level, Language elements, packages); navigator (current selection, scenes, symbol definitions with scripts). Double-click inserts. | Catalogue data (as AC-02); navigator from `Timeline.scripts`, `Element.scripts`, `Project.sources`. | Navigator jumps to every script in the document. |
| AC-07 | Script assist | Form-based editing of simple calls (Flash Script Assist). | Large; parameter metadata per API. | P3; disabled until specified. |
| AC-08 | Gutter markers, inline error peek + Quick fix ⌘., overview ruler, code hint popover | Errors exist (`CodeFeedback` diagnostics, `Severity`); missing warnings (diagnostics have no severity/code), breakpoint and fold markers, peek, quick fixes, overview ruler. | `neoflash_script::Diagnostic { language, line, column, message }` — **NEW** `severity`, `code`, optional fix suggestions. | Warning shows amber triangle; peek shows code + message under the line. |

## 8. Output and Compiler errors (`panels/actions.rs` › `output_panel`)

**OU-01 · Output Show filter and timestamps** — absent (comment: messages carry no level or time stamp)
- Purpose: Show All / Errors / Warnings / Trace select; timestamped lines; info / warning (amber) / error (red).
- Data: `Editor.output: Vec<String>`. **NEW (editor)** structured entries `{time, level, source, text}`.
- Depends on: editor. Acceptance: filter "Errors" shows only error lines; copy keeps timestamps.

**OU-02 · trace() from Test Movie** — absent (trace goes to the browser page `#trace` element in `web/runtime.js`)
- Purpose: Test Movie traces appear in Output (and in the Test Movie window drawer, DL-12).
- Depends on: engine/host bridge (browser → editor channel, or an embedded preview). Acceptance: `trace("x")` in a
  frame script appears in Output within one second of Test Movie.

## 9. Planned panels (`panels/planned.rs` enum `PlannedPanel`, bodies in `planned_panels.rs`; PP-01, PP-02, PP-03, PP-05, PP-08, PP-09, PP-10 and PP-11 are real panels now)

**PP-01 · Ease** — done (`panels/ease.rs`)
- Done: bound to the tween span under the playhead (motion: its start key); presets Linear, Ease In/Out/In-Out, Back,
  Custom; draggable handles; X1–Y2 fields; preview track with Play/Stop; Reset · Cancel · Apply (one Undo). Not yet:
  Bounce, Elastic (disabled; not single cubic curves) and per-property curves (Property select disabled).
- Controls: context line ("Classic tween · <layer> · frames a–b"); Property select All / Position / Rotation / Scale /
  Colour / Filters + "One curve for all"; presets Linear, Ease In, Ease Out, Ease In-Out, Back, Bounce, Elastic, Custom;
  `EaseGraph` with draggable Bézier handles (range 0–100 / Overshoot / Undershoot switches automatically); X1 Y1 X2 Y2
  fields; preview track with play and "Frame n / N · p %"; Reset · Cancel · Apply.
- Behaviour: edits a draft; Apply writes `Keyframe.tween` in one Undo; Cancel discards; X fields 0–1, Y fields
  −0.3…1.3 in the graph (model allows |y| ≤ 100); Bounce and Elastic are not single cubic curves → disabled preset with
  hint or **NEW** easing variants. Per-property curves ("One curve for all" off) need motion tracks (TL-05).
- Data: `Easing::{Linear, EaseIn, EaseOut, EaseInOut, CubicBezier{..}, Flash{..}}`; evaluation `evaluate::easing`
  solves Bézier x by bisection. Back is a cubic with y outside 0–1. Depends on: engine for Bounce/Elastic presets and
  per-property curves (wiring plan §7.4 rows 1–2).
- Acceptance: Apply on "Back out" (Y2 > 1) overshoots in editor preview and HTML5; Cancel leaves the document
  unchanged (History unchanged).

**PP-02 · Motion editor** — partial (`panels/motion_editor.rs`)
- Done: rows X, Y, Rotation Z, Scale X/Y, Alpha over the motion span with ◆ keys; value field keys the property at the
  playhead; ◆ adds/removes that property's key; ◀ ▶ jump between its keys; click a lane to move the playhead, drag a
  key on the selected lane to change its value (one Undo per drag); Graph size / Expanded sliders. Not yet:
  per-property eases (Ease column shows the span ease, disabled), Add property, moving keys in time.
- Controls: columns Property · Value · Ease · Keyframe + frame ruler; sections Basic motion (X, Y, Rotation Z),
  Transformation (Scale X/Y, Skew X/Y), Colour effect (Alpha, Tint …), Filters; per row value field, ease select (No
  ease, Simple (slow), Bounce …), ◀ ◆ ▶ keyframe navigation, curve lane; Add property; footer Graph size, Expanded
  height, visible frames; hint "Click a curve to edit · ⌥-drag a keyframe to copy · ⇧ snaps to frames".
- Data/Depends on: TL-05 motion tween tracks. Acceptance: adding a ◆ on Rotation at frame 20 changes only rotation;
  curve drag edits value with one Undo per drag.

**PP-03 · Properties — Motion tween** = PR-10.

**PP-04 · Debugger** — `planned_panels::debugger`
- Controls: Continue, Step over F10, Step in F11, Step out ⇧F11, Stop, Toggle breakpoint ⌘B, Remove all breakpoints;
  state badge; Call stack; Variables tree (changed values marked); Watch expressions (errors as values); Breakpoints
  list with checkboxes.
- Data: breakpoints are editor/project prefs (**NEW**); runtime state from an instrumented script runtime.
- Depends on: engine (debug protocol between HTML5 runtime and editor; source maps; `web-project-toolchain` limit
  "debugger/source maps"). Acceptance: a breakpoint in a frame script pauses Test Movie; Variables shows `this`
  properties; Continue resumes.

**PP-05 · Project** — done (`panels/project.rs`, `Window ▸ Project`)
- Done: project selector, Find file filter, hierarchical `src` (`Project.sources` + virtual folders), `assets`
  (`Project.resources`) and `.nflash` document tree, inline source rename, `New file` / `New folder` / `Delete` /
  `Publish` bar with one Undo step per source edit, double-click to open in `Actions ▸ Project files`, and linkage
  footer naming linked symbols or the document class.
- Controls: project select; Find file; file tree of `Project.sources` (unsaved ● marker); New file / New folder /
  Delete; Publish; footer explaining class linkage ("X.as is linked to symbol y").
- Data: `Project.sources: Vec<SourceUnit { id, path, language, source }>`, `Symbol.class_name/linkage_id`,
  `Project.document_class`.
- Acceptance: create/rename/delete source with Undo; linkage footer correct; double-click opens it in Actions.

**PP-06 · Code snippets** — `planned_panels::code_snippets`
- Controls: search; categories Actions, Timeline navigation, Animation, Load and unload, Audio and video, Event
  handlers; preview of the code; Copy; Insert at frame N.
- Data: bundled snippet library (**NEW** data file in the editor crate, per language, licensed as project-authored).
  Insert writes to `Timeline.scripts` at the current frame (one Undo); snippets that need an instance name ask for /
  use the selected instance.
- Depends on: editor. Only snippets that compile with `neoflash-script` may ship (test). Acceptance: every bundled
  snippet compiles; Insert appends to the frame script without overwriting.

**PP-07 · Compiler errors** — `planned_panels::compiler_errors`
- Controls: badges "N warnings" / "N errors"; rows icon + "code: message" + "Scene · layer · frame · Line n" + Go to
  source; double-click jumps to the code.
- Data: `neoflash_script::compile_project` / `compile_with_program` return `Vec<Diagnostic>` (line/column/message,
  no severity/code/location). Locations need mapping from script → symbol/layer/frame/object (editor knows which
  script it compiled). **NEW (small)**: severity + code on `Diagnostic`, or the editor classifies.
- Depends on: editor (+ small script change). Acceptance: Check project / Test Movie fill the panel; Go to source opens
  Actions at the right script, line and column.

**PP-08 · Scene** — done (`panels/scene.rs`, Window ▸ Scene ⇧F2): Find scene, list (current highlighted), Add / Duplicate / Delete; drag to
reorder; double-click rename. Depends on TL-03. Acceptance: reorder changes playback order; delete of the last scene
refused.

**PP-09 · History** — done (`panels/history.rs`, `Window ▸ History` `⌘F10`)
- Done: chronological `Open` + named step rows (`History::undo_labels` / `redo_labels`) with the current step checked
  and undone steps dimmed yet clickable (`History::step_to`), `Edit ▸ Undo <step>` / `Redo <step>` menu labels, and
  footer Undo · Redo · Clear history controls.
- Controls: list of steps with the current position checked; Undo / Redo; Clear history.
- Data: `History` snapshots with step labels, capacity 150 in the editor.
- Acceptance: labels match commands ("Move", "Fill colour", "Delete"); jumping 5 steps back equals 5 × Undo; Clear
  disables Undo.

**PP-10 · Swatches** — done (`panels/swatches.rs`, `Window ▸ Swatches` `⌘F9`)
- Done: `SwatchesPopover` panel with hex + alpha readout, eyedropper, no colour, wheel → Color panel, Recent colours,
  Document colours (derived from `Project` paints + user-added swatches in `WorkspacePrefs.swatches`, also populated by
  Color panel ⋯ `Add swatch`), switchable Palette (`Default`, `Web 216`, `Grayscale`, `Pastels`) and Gradients (default +
  document `GradientPaint`s, preserving primitive geometry when applied). Persistent file-stored swatches remain W1.9.
- Controls: hex + alpha, eyedropper, no colour, wheel → Color panel; Recent; Document (+ add); Palette (switchable);
  Gradients.
- Acceptance: clicking a swatch applies to the active well (drawing colour or selection fill/stroke with one Undo);
  Recent updates.

**PP-11 · Info** — done (`panels/info.rs`, `Window ▸ Info` `⌘I`)
- Done: editable 2×2 selection `W` / `H` / `X` / `Y` fields (committed as one Undo step via Transform layout units),
  live `Cursor` `X` / `Y` in active ruler units, and `Colour` `R` `G` `B` `A` + swatch under the stage pointer (sampling
  topmost painted elements, gradients and cached bitmap pixels, falling back to stage background).
- Controls: W H X Y of the selection, Cursor X Y in stage coordinates, Colour under cursor R G B A.
- Acceptance: cursor values match ruler; editing W scales the selection with one Undo.

**PP-12 · Rulers, guides & grid** — working popover (`panels/view_settings.rs`, ST-06): Rulers (Show, Units Pixels /
Inches / Points / Centimetres / Millimetres, Labels Auto / Every N); Guides (Show, Snap, Lock, colour, snap accuracy);
Grid (Show, Snap, Over objects, spacing ↔ ↕ linked, colour, snap accuracy); footer Clear guides · Save as default.
See ST-01…ST-06.

**PP-13 · Properties — Sound** = PR-08. **PP-14 · Properties — Display & filters** = PR-05.

## 10. Planned dialogs (`panels/planned_dialogs.rs`) and absent dialogs

### DL-01 · Preferences (`preferences`, 920 pt; footer Reset to defaults · Cancel · Save)

All values are editor preferences (eframe storage, like `WorkspacePrefs`), never document data. Save applies and
persists; Cancel discards; Reset restores defaults for the current page (verify: page vs all).

Done: General ▸ Theme Dark / Light (`planned_dialogs::prefs_general`) is a draft that Save applies through
`Editor::set_theme` and persists with the workspace prefs; Cancel and the header close discard it; Reset to defaults
(enabled on General only) restores Dark. The dialog is still the non-modal planned preview; "follow system" and every
other setting stay disabled.

| Page | Controls (from the preview) | Effect when wired | Model / engine | Prio |
| --- | --- | --- | --- | --- |
| General | Theme Dark / Light / follow system; UI scale; Accent; Language; On launch; Undo levels; Autosave every N min; tooltips with shortcuts; Shift-select; highlight colour | Theme → existing `set_theme`; Undo levels → `History::new(n)`; Autosave → recovery (`platform-recovery`); UI scale → egui zoom; Language/Accent → **request** (no localisation, single accent token) | editor; autosave editor | P1 (theme, undo levels), P2 (autosave), P3 (language, accent) |
| Code editor | Font, size, line height; line numbers, word wrap, auto-close brackets, code hints + delay, tab size, format on save, highlight current line; colour scheme + Duplicate / Import…; syntax colour table; preview | Code editor rendering options; scheme edits must keep "red = errors only" | editor (`components::code` reads tokens — custom colours need a request) | P2 |
| Timeline | span-based selection, default frame view, row height, show seconds, snap playhead, named anchor per scene; onion range + tints + opacity falloff; auto keyframe on transform, default ease, tween type on drag | Defaults for TL-09/TL-11, onion state (`onion_before/after/opacity` exist), default `Easing` for new tweens | editor | P1 |
| Drawing | Merge / Object drawing; pen preview, solid points, precise cursors; connect lines, smooth curves, recognise lines/shapes, click accuracy; snap tolerance, snap to pixels, snap align | Tool defaults (TO-07), snapping (ST-04) | editor | P2 |
| Text | default font/size/anti-alias; font substitution warning; embed used glyphs; spell check; vertical text; input method | Defaults for new text; warnings exist in `export_warnings` | editor; embedding engine (DL-11) | P3 |
| Clipboard | bitmap resolution, gradient quality, copy as SVG, keep layer names; default paste in centre / in place; vector paste as paths; ask before large bitmaps | `clipboard.rs` behaviour switches | editor | P2 |
| Shortcuts | set + Duplicate / Rename / Delete; search; command filter; command → keys (`Kbd`); recording "Press keys…"; conflict badge; Reset command; Export as HTML… | Needs a command registry (today shortcuts are hard-coded in handlers, e.g. `view_shortcuts`) | editor (**NEW** command table) | P3 |
| Publish | default target, output folder + Browse…, HTML wrapper, assets folder, source maps; image quality, audio, acceleration; strict mode, warnings as errors, publish on save | Defaults for DL-03 | export | P2 |
| Updates | version + Check now / Release notes; channel; automatic checks; background download; usage statistics; crash reports; extensions | No update service, telemetry or extensions exist; must stay disabled/honest until `platform-release` / `platform-extensions` define them; never send data | — | P3 |

### Other planned dialogs

**DL-02 · Document settings (⌘J)** — done (`panels/document_settings.rs`, `Modify ▸ Document…` `⌘J`)
- Done: modal dialog editing a draft of stage `W × H` (with optional aspect lock), `Match contents` (translates root
  scene content so union bounds start at `(0, 0)`), `Scale content with stage` + 9-point registration anchor, `Units`
  (updates `ViewPrefs.ruler_units`), `Background`, `Frame rate`, and `Make default` (`WorkspacePrefs.document_defaults`,
  applied by `File ▸ New`). `OK` / `Enter` commits document changes in one `"Document settings"` Undo step; `Cancel` /
  `Esc` discards. `Script` version stays informational until W1.12.
- Controls: stage W × H linked; Match contents / Match printer; Scale content with stage + anchor (registration
  picker); Units; Background; Frame rate (→ "N frames · s"); Script version; Make default · Cancel · OK.
- Acceptance: OK applies everything as one Undo; Cancel changes nothing.

**DL-03 · Publish settings (⇧F12)** — `publish_settings`
- Controls: profile; targets HTML5 · Rust / WebAssembly, Desktop app, Video MP4, Image sequence / sprite sheet, SVG
  animation; output file; HTML wrapper + template; runtime optimisation; scale mode; loop; autoplay; preloader; source
  maps; JPEG quality; audio; sprite sheets; embed fonts; strict / include trace() / minify; estimated size; Cancel ·
  OK · Publish.
- Data: `neoflash_export::export_project(project, output_dir, runtime_dir)` has no options. **NEW** `PublishSettings`
  stored in the document (Flash stores publish profiles in the document). Only HTML5 is a real target; others must
  stay disabled.
- Acceptance: settings persist in the file; Loop off makes the exported movie stop at the end; trace stripped when
  "Include trace()" is off.

**DL-04 · Export image** — `export_image` (format tabs PNG · SVG · PSD · AI · PDF)
- Controls: preview "Frame n of N · W × H"; range current frame / frames; size linked + scale; background stage /
  transparent; per-format options (SVG: images embed/link, text outlines, minify, CSS classes, hidden & guide layers,
  precision, ID prefix; PSD: layers / flatten, folders → groups, symbols → smart objects, editable text, colour,
  resolution; AI: version, layers / keyframes → artboards, strokes, text outlines).
- Depends on: engine — **NEW** rasteriser for PNG (shared with native preview?) and SVG writer from `evaluate`
  draw items. PSD/AI/PDF P3. Acceptance: PNG of frame 1 equals the HTML5 canvas frame within tolerance; SVG opens in
  a browser with the same geometry.

**DL-05 · Export video** — `export_video`: MP4 · WebM · MOV · GIF · PNG sequence; range; size; fps; codec; quality;
runtime capture of nested clips/scripts; stop at last frame / after N s; audio; transparent bg; progress. Depends on:
engine (frame rendering + encoder; runtime capture in browser via `MediaRecorder` or native encoder). P3.
Acceptance: 5 s MP4 at 24 fps has 120 frames matching the timeline.

**DL-06 · Export audio** — `export_audio`: WAV · MP3 · OGG · AAC; source timeline mix / layer; range; sample rate;
bit depth; channels; bit rate; normalize; fade out; trim silence. Depends on TL-07 (timeline audio) + encoders. P3.

**DL-07 · Import video** — `import_video`: preview + trim In/Out; File + Browse…; deployment Embed in timeline / Load
external / Progressive; symbol type, codec, place on stage, expand timeline, include audio, match frame rate; summary.
Depends on: engine (PR-09 model; embedding = frame extraction is large). P3. Acceptance: "Load external video" creates
a Video element bound to the imported resource with Autoplay.

**DL-08 · Sound properties** — `sound_properties`: name, path, source info; export compression MP3 / ADPCM / Raw /
Speech; bit rate; quality; stereo → mono; result size; Update / Import… / Test. Depends on: engine (audio resource
metadata + encoding at publish). P3.

| ID | Absent dialog / panel (reference §9) | Purpose | Depends on | Prio |
| --- | --- | --- | --- | --- |
| DL-09 | Edit envelope | Left/right volume lanes with draggable points, seconds/frames, zoom | TL-07 audio model | P3 |
| DL-10 | Import PSD to library | Layer list + per-layer options, placement, keep positions, stage size | **NEW** PSD reader in `neoflash-import` | P3 |
| DL-11 | Font embedding | Font list, character ranges, extra chars, export for AS + class, glyph count | `web-font-portability` (planned) | P3 |
| DL-12 | Test Movie window | Native window: stage, transport, speed, fps/memory badge, Output drawer | Embedded runtime host (today Test Movie opens a browser) | P2 |
| DL-13 | Find and replace (⌘F) | Text / Font / Colour / Symbol / Sound / Video / Bitmap search, scope, regex, results table, Replace all | editor scan of `Project` (+ scripts); Replace all = one Undo | P2 |
| DL-14 | Movie Explorer | Tree document ▸ scene ▸ layer ▸ frame ▸ instance / actions, filters, path footer | editor traversal of `Project` | P2 |
| DL-15 | Convert to Symbol ▸ Advanced | 9-slice guides; runtime shared library export/import URL; authortime sharing source | **NEW** symbol fields; runtime 9-slice scaling | P3 |
| DL-16 | Layer properties (exists as dialog in `layer_controls.rs`) | Missing: types Guided / Masked / Armature, Layer height 100/200/300 % | Guided = TL-01; height = timeline row model | P2 |
| DL-17 | Accessibility panel (`panels/accessibility.rs`) | Missing: Make child objects accessible, Auto label, Shortcut | `Accessibility` lacks these fields (**NEW**) + HTML5 ARIA mapping | P3 |

## 11. Menus (`panels/menu.rs`, `authoring_modify.rs::modify_menu_ui`, frame/layer context menus)

**MN-03 · View ▸ Rulers / Guides / Grid / Rulers, guides & grid…** — working except the Snapping ▸ submenu
(R-003). Grid ▸ and Guides ▸ are submenus as in the reference (§8 View ▸ Grid); the Snapping ▸ submenu content waits
for R-003.

**MN-01 · Absent main-menu items** (reference §8 vs. current menus). Each item uses the existing menu row style; adding
rows to an existing menu is wiring of a reference element, but confirm with NF2-REQUESTS.md if the order/grouping is
unclear.

| Menu | Still absent or disabled (everything else in §8 works) | Depends on |
| --- | --- | --- |
| File | Open recent, Close, Revert, Print… (disabled); Export ▸ image / video / audio and Publish settings… ⇧F12 open planned previews | editor (recent/revert); DL-03/04–06 |
| Edit | Find and replace ⌘F, Preferences… ⌘U, Keyboard shortcuts… (planned previews). Done: Timeline ▸ (Cut/Copy/Paste/Clear/Remove/Select all/Reverse frames, Synchronise symbols), Edit symbols ⌘E, Edit selected, Edit in place | DL-13; DL-01 |
| View | Preview mode ▸, Snapping ▸, Show shape hints ⌥⌘H (disabled); Pasteboard ⇧⌘W is shown checked but disabled (hiding the pasteboard is not available) | editor; ST-04 / R-003; TL-04 shape hints |
| Insert | — (Layer, Layer folder, Motion guide, Classic / Shape / Motion tween and Scene work) | — |
| Modify | Bitmap ▸, Symbol ▸, Shape ▸, Combine objects ▸ (disabled), Timeline ▸ Distribute to layers (disabled), Layer properties… (planned). Done: Document… ⌘J, Timeline ▸ Reverse frames, Synchronise symbols, Convert to (blank) keyframes | PR-06; LI-01; engine for shape ops |
| Control | Test scene ⌥⌘↵ (planned preview), Enable simple buttons ⌥⌘B (disabled). Done: Go to end, step `.` `,`, Loop playback, Play all scenes, Mute sounds ⌥⌘M | E-16; editor button preview |
| Window | Duplicate window, Hide panels F4 (disabled); Compiler errors, Debugger, Code snippets, Movie Explorer and other planned panels open previews. Done: Swatches ⌘F9, Info ⌘I, Ease, History ⌘F10, Scene ⇧F2, Motion editor, Project | PP-*; workspace |

**MN-02 · Context menus** (reference §8 Frame / Layer / Stage object)

| Menu | Still absent or disabled | Depends on |
| --- | --- | --- |
| Frame | Paste motion special… (disabled, R-006). Done: tweens, Ease…, Clear / Convert keyframes, Select all frames, Copy / Paste motion, Reverse frames, Synchronise symbols | R-006 |
| Layer | — (Add classic motion guide, Guide / Mask toggles work) | — |
| Stage object | Edit in new window, Swap symbol…, Duplicate symbol…, Distribute to layers ⇧⌘D (disabled). Done: Edit in place, Create motion tween, Show in library | PR-06, LI-01; distribute = editor |
