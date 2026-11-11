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
  read-only planned preview (`panels/planned*.rs`, Window ▸ Panels ▸ Planned panels); `absent` = in the NF2 reference, not in
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
  honest diagnostics (no silent drops), all four NF2 themes, keyboard access and NF2 tooltips with shortcuts.
- Enabling an element may only drop its "— not available yet" / "Not available yet" suffix or disabled hover text;
  any other label, tooltip or layout change is a design request.

## Summary

| ID | Element | Surface | Status | Engine | Priority |
| --- | --- | --- | --- | --- | --- |
| TL-01 | Add motion guide | Timeline controls | done | yes | P2 |
| TL-02 | Loop playback | Timeline controls | done | editor | P1 |
| TL-03 | Scene select ▸ New scene | Timeline controls | done | yes | P2 |
| TL-04 | Shape tween | Timeline controls | done | yes (small) | P2 |
| TL-05 | Motion tween | Timeline controls | done | yes | P2 |
| TL-06 | Ease panel button | Timeline controls | done | editor | P1 |
| TL-07 | Sound | Timeline controls | done | yes | P3 |
| TL-08 | Edit multiple frames | Timeline controls | done | editor | P2 |
| TL-09 | Frame view ▸ Short rows | Frame view menu | done | UI-only | P1 |
| TL-10 | Frame view ▸ Preview | Frame view menu | done | editor | P3 |
| TL-11 | Frame view ▸ Snap playhead to keyframes | Frame view menu | done | UI-only | P1 |
| TL-12 | Frame view ▸ Placement, Preview in context, Help, Close | Frame view menu | done | UI-only | P3 |
| TL-13 | Layer types Sound / Video / Armature / Motion guide rows | Timeline layers | partial | yes | P2–P3 |
| TL-14 | Button symbol state labels Up/Over/Down/Hit | Timeline | verify | editor | P2 |
| TO-01 | Rectangle / Oval primitive tools | Tools | done (options on Rectangle/Oval + Properties) | yes | P2 |
| TO-02 | PolyStar tool + settings | Tools | done | editor (+small) | P2 |
| TO-03 | Deco tool | Tools | partial (generator + settings draft/internal command; user apply gesture pending) | yes | P3 |
| TO-04 | Spray brush | Tools | partial (seeded generator + settings draft/internal command; user apply gesture pending) | editor | P3 |
| TO-05 | Bone / Bind (IK) | Tools | partial (target/length/rotation-limit/stiffness solver fields + transient binding command; direct Stage tool pending) | yes | P3 |
| TO-06 | Snap toggle (tools rail) | Tools | done | editor | P1 |
| TO-07 | Tool options: Object drawing, smooth / straighten | Tools | partial | editor | P2 |
| TO-08 | Fill/stroke chip → Swatches popover | Tools | partial | UI-only | P1 |
| ST-01 | Stage rulers + ruler corner | Stage chrome | working | UI-only | P1 |
| ST-02 | Guides | Stage chrome | working | yes (small) | P1 |
| ST-03 | Stage grid | Stage chrome | done | UI-only | P1 |
| ST-04 | Snapping (grid, guides, objects, pixels) + snap ring | Stage chrome | done (View ▸ Snapping R-003) | editor | P1 |
| ST-05 | Zoom control toggles rulers/guides/grid/snap | Stage chrome | working | UI-only | P1 |
| ST-06 | View settings popover | Stage chrome | working | UI-only | P1 |
| ST-07 | NF2 stage cursors | Stage chrome | done | UI-only | P1 |
| ST-08 | Breadcrumb ▾ menu + Symbols & scenes popover | Stage chrome | done | editor | P2 |
| ST-09 | Toasts / progress / spinner | Window chrome | partial | UI-only | P1 |
| ST-10 | Dock compass | Docking | absent | UI-only | P3 |
| ST-11 | Canvas editing chrome (Selection/Transform box variants, gradient transform focal handle) | Stage | partial (boxes, anchors, marquee, transformation point done) | editor | P2 |
| PR-01 | Properties — Text (full NF2 variant) | Properties | partial (`Embed font` / `Embedding…` working) | yes | P2 |
| PR-02 | Properties — Oval / Rectangle primitive options | Properties | done | yes | P2 |
| PR-03 | Stroke style (line style, cap, join, miter, hinting, scale) + Stroke style dialog | Properties / dialog | done (`panels/stroke_style.rs`) | yes | P2 |
| PR-04 | Properties — Movie clip: colour effect styles | Properties | partial (R-012) | editor | P1 |
| PR-05 | Properties — Movie clip: Display & Filters tabs | Properties | done (`panels/filters.rs`) | yes | P2 |
| PR-06 | Properties — Movie clip: Swap…, Parameters tab | Properties | partial (Library + Stage Swap done; R-012) | editor / yes | P2 |
| PR-07 | Properties — Graphic thumbnail strip | Properties | done (`panels/live_planned.rs` + `authoring_symbols.rs`) | editor | P3 |
| PR-08 | Properties — Sound (frame) | Properties | partial | yes | P3 |
| PR-09 | Properties — Video playback & cue points | Properties | partial (`panels/properties_video.rs`; resource/sync/loop/mute, interactive In/Out trim, Split at playhead, Link/Unlink, Extract audio and cue-point authoring/retiming work; native Progressive/skin/Swap remain) | yes | P3 |
| PR-10 | Properties — Motion tween | Properties | done | yes | P2 |
| PR-11 | Frame easing: overshoot values, Flash acceleration field | Properties | done | UI-only | P1 |
| CO-01 | Color panel fill type: Linear / Radial gradient | Color | done | yes (small) | P2 |
| CO-02 | Color panel fill type: Bitmap fill (+ Bitmap fill panel) | Color | done (`panels/live_planned.rs` + `bitmaps.rs`) | yes | P3 |
| CO-03 | Color panel: Swatches tab, panel menu Add swatch, picker area | Color | partial (Add swatch + Swatches panel done) | editor | P1 |
| LI-01 | Library: delete / duplicate item | Library | done | editor | P1 |
| LI-02 | Library: bitmap / sound / font / video items, document select, pin | Library | partial (bitmap/sound/video/font items done) | editor | P2 |
| AC-01 | Actions toolbar: auto-format | Actions | partial (re-indent only) | yes | P2 |
| AC-02 | Actions toolbar: show code hint (+ hint popover) | Actions | done | yes | P2 |
| AC-03 | Actions toolbar: debug options | Actions | done | yes | P3 |
| AC-04 | Actions toolbar: collapse between braces / expand all (folding) | Actions | done (`sources.rs`) | editor | P2 |
| AC-05 | Actions toolbar: block comment / line comment / uncomment | Actions | done | UI-only | P1 |
| AC-06 | Actions toolbar: toolbox sidebar + script navigator | Actions | done | yes | P2 |
| AC-07 | Script assist | Actions | done (`sources.rs` + `panels/actions.rs`) | yes | P3 |
| AC-08 | Gutter markers (warning, breakpoint, fold), inline error peek + Quick fix, overview ruler | Actions | done (`sources.rs`) | yes | P2 |
| OU-01 | Output: Show All / Errors / Warnings / Trace, timestamps | Output | done | editor | P1 |
| OU-02 | Output: trace() from native Play / Test Movie | Output | partial (native Live ActionScript done; browser Test Movie bridge pending) | yes | P2 |
| PP-01 | Ease panel | Planned panel | done | editor | P1 |
| PP-02 | Motion editor | Planned panel | partial | yes | P2 |
| PP-03 | Properties — Motion tween | Planned panel | done | yes | P2 (= PR-10) |
| PP-04 | Debugger | Planned panel | done (`debugger.rs` + `sources.rs` + `live_planned.rs`) | yes | P3 |
| PP-05 | Project | Planned panel | done | editor | P2 |
| PP-06 | Code snippets | Planned panel | done (`panels/code_snippets.rs`) | editor | P2 |
| PP-07 | Compiler errors | Planned panel | done (`panels/compiler_errors.rs`) | editor (+small) | P1 |
| PP-08 | Scene | Planned panel | done | yes | P2 |
| PP-09 | History | Planned panel | done | editor (+small) | P1 |
| PP-10 | Swatches | Planned panel | done | editor | P1 |
| PP-11 | Info | Planned panel | done | editor | P1 |
| PP-12 | Rulers, guides & grid (View settings) | Popover | working | UI-only | P1 (= ST-06) |
| PP-13 | Properties — Sound | Planned panel | partial | yes | P3 (= PR-08) |
| PP-14 | Properties — Display & filters | Planned panel | done (`panels/filters.rs`) | yes | P2 (= PR-05) |
| DL-01 | Preferences (9 pages) | Dialog | done (8 pages wired; Updates + design/subsystem items disabled) | editor | P1–P3 per page |
| DL-02 | Document settings | Planned dialog | done | editor | P1 |
| DL-03 | Publish settings | HTML5 size settings | done (`panels/publish_settings.rs`) | yes | P2 |
| DL-04 | Export image (PNG/SVG/PSD/AI/PDF) | Dialog | partial (`panels/export_image.rs` + `neoflash_export::image_export`; documented format fallbacks) | yes | P2 (PNG/SVG) / P3 |
| DL-05 | Export video | Dialog | done (`panels/export_video.rs`) | yes | P3 |
| DL-06 | Export audio | Dialog | done | yes | P3 |
| DL-07 | Import video | Dialog | partial (`panels/import_video.rs`; HTML5 plus native Embedded/local External playback work, Progressive/native cue editing and bounded on-demand long-video decode remain) | yes | P3 |
| DL-08 | Sound properties | Dialog | done | yes | P3 |
| DL-09 | Edit envelope | Dialog | done | yes | P3 |
| DL-10 | Import PSD to library | Importer | done for the bounded supported profile (`neoflash_import::psd`; editor and CLI routes) | yes | P3 |
| DL-11 | Font embedding | Dialog | done (`panels/font_embedding.rs`) | yes | P3 |
| DL-12 | Test Movie window | Window | done (`panels/live_planned.rs` + browser host) | yes | P2 |
| DL-13 | Find and replace | Dialog | done (`panels/find_replace.rs`) | editor | P2 |
| DL-14 | Movie Explorer | Panel | done (`panels/movie_explorer.rs`) | editor | P2 |
| DL-15 | Convert to Symbol ▸ Advanced: 9-slice, runtime / authortime sharing | Dialog | partial (Library fields, painted guides and open-document sharing work; direct guide drag/external-file chooser remain) | yes | P3 |
| DL-16 | Layer properties: Guided / Masked / Armature types, layer height | Dialog | done (Armature disabled) | editor / yes | P2 |
| DL-17 | Accessibility: child objects, auto label, shortcut | Panel | partial | yes (small) | P3 |
| MN-01 | Menu items absent from §8 menus | Menus | partial | mixed | P1–P3 |
| MN-02 | Frame / layer / stage context menu items absent | Menus | partial | mixed | P1–P2 |
| MN-03 | View ▸ Rulers / Guides / Grid / Rulers, guides & grid… | Menus | partial (Snapping ▸ disabled) | UI-only | P1 |

Counts: 93 entries — 14 timeline, 8 tools, 11 stage/window chrome, 11 properties, 3 colour, 2 library, 8 actions,
2 output, 14 planned panels (3 alias other entries), 17 dialogs/panels (DL-01 covers 9 preference pages), 3 menu
groups.

---

## 1. Timeline controls (`crates/neoflash-editor/src/panels/timeline_controls.rs`, `panels/timeline.rs`)

**TL-01 · Add motion guide** — `timeline_layer_actions` · done
- Done (2026-11-01): `LayerProperties.motion_guide` link to a Guide-role layer (format 6); Timeline button, Insert ▸ Motion guide and layer menu Add classic motion guide; the guided layer indents under the guide, which shows the Motion guide type. Classic tweens follow the guide's first stroke (`evaluate::sample_layer_in`). `Keyframe.orient_to_path` turns the object with the guide tangent (editor and HTML5; `Editor::set_orient_to_path`). Classic Tween Properties exposes Orient / Snap only on guided layers (R-007); drawing the path when the guide has no stroke is the author's step.
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
- Done: `WorkspacePrefs.timeline.loop_playback` (default on), toggle in the bar and Control ▸ Loop playback; off stops on the last frame of the last scene when Play all scenes is enabled, otherwise on the current timeline; Play restarts from frame 1.
- Purpose: when on, editor playback wraps from the last frame to frame 1; when off, it stops at the last frame
  (Control ▸ Loop playback, checked by default in §8).
- Behaviour: toggle (Selected style when on). Editor preference, not document data; no Undo.
- Data: editor state; `AuthoredPlayback::tick` currently advances the shared `Player` which wraps (verify exact
  wrap point). Off = detect wrap and stop.
- Depends on: editor (optionally a small `AuthoredPlayback` option).
- Acceptance: off → playback stops on the last frame and the Play button returns to Play; on → wraps; state survives
  panel re-dock (workspace prefs).

**TL-03 · Scene select ▸ New scene** — `timeline_scene_select` · done
- Done: `Project.scenes` (ordered MovieClip symbol IDs, first = `root`, format 6); Scene select lists and switches scenes, New scene and Insert ▸ Scene add one (one Undo); scene symbols are hidden from the Library; Control ▸ Play all scenes continues editor playback into the next scene; `Control ▸ Test scene` (`⌥⌘↵`) publishes and opens only the active scene; breadcrumb `▾` (ST-08) switches scenes and symbols. Runtime/HTML5 flatten scenes into one main timeline (`neoflash_core::flatten_scenes`); the Scene panel (PP-08) renames, duplicates, deletes and reorders.
- Purpose: Flash scenes — consecutive main timelines played in order; the Scene select switches the edited scene.
- Behaviour: menu lists scenes (checked = current) + New scene; selecting a scene leaves symbol editing and shows its
  timeline. New scene is one Undo step. Scene panel (PP-08) adds rename/duplicate/delete/reorder.
- Data: `Project.scenes: Vec<Id>` (scene MovieClip symbols in play order; empty = one scene, `root`).
- Depends on: engine — playback across scenes, `gotoAndPlay(frame, scene)`, `currentScene`, export (done).
- Acceptance: two scenes play in order in the editor preview and HTML5; AS3 `scenes`/`currentScene` report them
  (bounded); legacy one-scene documents load unchanged.

**TL-04 · Shape tween** — `timeline_frame_tools` (tween group, item 2) · done
- Done: `Keyframe.tween_kind = Shape`; general morph in `neoflash_core::shape_tween` (different contours, rect/oval/path, gradients); diagnostic when a keyframe lacks shapes. Shape hint markers are wired to the existing correspondence model (R-009).
- Purpose: morph raw shapes between two keyframes (green span, `timeline/shape-tween`).
- Behaviour: enabled on a keyframe whose layer holds shapes (paths, rectangles, ovals) in this and the next keyframe;
  click toggles a shape tween. Mixed content or symbols → diagnostic "Shape tweens need shapes on both keyframes".
  Shape hints (View ▸ Show shape hints ⌥⌘H, Modify ▸ Shape ▸ Add shape hint ⇧⌘H) are draggable on endpoint keys; offstage drop removes a hint.
- Data: `Keyframe.tween_kind: TweenKind { Classic, Shape, Motion }` qualifies `Keyframe.tween`; shape hints use existing format-7 `Keyframe.shape_hints: Vec<ShapeHint { label, start_uv, end_uv }>` (up to 26).
- Depends on: engine — contour correspondence for differing topology (done in `shape_tween.rs`); span colour per kind.
- Acceptance: shape tween marker renders green with arrow; paths with matching and differing topology morph; a
  keyframe without shapes produces an explanation instead of a jump.

**TL-05 · Motion tween** — `timeline_frame_tools` (tween group, item 3) · done
- Done: `tween_kind = Motion`; every object on a non-empty layer keeps independent stable-ID property keys, easing,
  roving state, path and Orient/Snap/Sync settings. Editing inside the span records only the changed properties for
  each changed object; F6 keys all properties for every object in the span. Motion Editor, Ease, Properties — Motion tween
  and the Stage path follow exactly one selected object; no or ambiguous selection uses an empty/mixed state and never
  silently targets the first object. Dragging a ◆ path point keys only that object's X/Y in one Undo. Copy/Paste,
  Paste motion special, Reverse, Convert to keyframes and range edits preserve sibling tracks; native, HTML5 and
  Lottie evaluation use the same stable identities. Range cuts/reverse and XFL may bake curved paths.
- Purpose: object-based tween with per-property keyframes ◆ (blue span) and an editable motion path on Stage for
  every selected object, including several objects sharing one Motion layer.
- Behaviour: creating Motion on a non-empty frame converts every object on the layer to one span. Moving or changing
  one member creates or updates only that member's keys and settings; switching Stage selection switches the displayed
  track and path. Empty content produces an actionable diagnostic.
- Data: `tween_kind = Motion` on the span start key; format-10 `Keyframe.motion_tracks` stores sparse settings per
  `element_id`, and `Keyframe.motion_keys` stores per-object property/roving keys. Legacy `keyed_properties`,
  `property_eases`, `motion_path`, `orient_to_path`, `sync_symbols`, `snap_to_guide` and `roving` remain exact fallbacks
  for older documents.
- Acceptance: create/remove with one Undo each; ◆ markers render; two- and three-object spans keep overlapping and
  non-overlapping keys, curves/easing/options and relative poses independent through save/reopen, Copy/Paste, range
  edits, nested Edit in Place, native playback and HTML5; Classic Tween behaviour is unchanged.

**TL-06 · Ease panel button** — `timeline_frame_tools` · done
- Done: enabled only while the playhead is inside a tween (classic, shape or motion); opens the Ease panel (PP-01). Frame
  menu ▸ Ease… and Window ▸ Panels ▸ Ease open it too.
- Purpose: open the Ease panel (PP-01) for the selected classic tween.
- Behaviour: enabled when the current frame is inside a tweened span; click opens/focuses the Ease panel bound to that
  keyframe.
- Data: `Easing::{Linear, EaseIn, EaseOut, EaseInOut, CubicBezier, Flash, Bounce, Elastic}` exists.
- Depends on: editor (PP-01).
- Acceptance: enabled only inside a tween; opens Ease showing the keyframe's current curve.

**TL-07 · Sound** — `timeline_frame_tools` · done
- Done: menu of library sounds + Import sound… (MP3/WAV) sets the current span's keyframe sound (default Event) as one Undo and selects that frame so Properties shows Sound; Sound layers with Sound spans (real decoded mono/stereo peaks, cached by resource and zoom off the UI thread); Event/Start/Stop/Stream in HTML5 and in editor playback (rodio). Scrubbing is silent (documented limit).
- Purpose: attach a library sound to the current keyframe (sound layer) and open Properties — Sound.
- Behaviour: menu of library sounds + "Import sound…"; choosing one sets the keyframe's sound (default Sync Event).
- Data: `Keyframe.sound: Option<FrameSound>` referencing an `audio/*` `ResourceUnit` (decision 0037).
- Depends on: engine (done: `Player` `SoundCommand`s, `web/audio.js`, editor rodio preview).
- Acceptance: waveform span on the layer; Event/Start/Stop/Stream behave in HTML5 preview; scrubbing with Stream
  sync plays audio (or a documented limitation).

- Layer merge preserves frame sound commands. Same-frame sound collisions and changes that would truncate a Stream
  span are rejected before mutation; compatible merges remain one Undo step (`layer_controls.rs` tests).

**TL-08 · Edit multiple frames** — `timeline_onion_group` · done
- Done: toggle; stage draws every keyframe in the onion range opaque; clicking, marquee-selecting or Select All hits objects across every keyframe in `onion_range` (including objects that exist only on other keyframes), and Move / Scale / Rotate / Skew gestures apply across all selected keyframes in a single Undo step (`selection.rs`, `stage_selection.rs`, `timeline_commands.rs`).
- Purpose: show and edit content of all keyframes inside the onion range at once (Flash "Edit multiple frames").
- Behaviour: toggle; while on, stage shows every keyframe in the range opaque; selection/move/transform apply to
  objects on several keyframes; one Undo for the whole gesture. Group requires every selected member on the current
  keyframe; a selection containing other-frame-only objects produces a diagnostic without mutation.
- Data: selection targets are per keyframe; no model change.
- Depends on: editor (selection across keyframes; `animation-frame-ranges` limit "simultaneous artwork editing").
- Acceptance: select-all + move shifts objects on all keyframes in range with one Undo; off restores normal view.

**TL-09 · Frame view ▸ Compact / Short rows** — `timeline_frame_view_menu` · done
- Done: `TimelineView.compact_rows` (`14 pt`, `TimelineView::LAYER_ROW_COMPACT`) and `TimelineView.short_rows` (`18 pt`, `metrics::LAYER_ROW_SHORT`), also selectable in `Preferences ▸ Timeline ▸ Row: Compact / Short / Normal` and `Layer properties ▸ Layer height: Compact / Short / 100 % / 200 %`; row height is passed to painters and hit tests.
- Purpose: 14 pt (`Compact`) and 18 pt (`Short`) layer rows.
- Behaviour: checked menu item; layer list, grid, hit testing and drag targets all use the compact/short height.
- Data: workspace pref. `timeline_paint::ROW_HEIGHT` is a constant today → must become per-view.
- Depends on: UI-only (editor).
- Acceptance: rows 14 pt / 18 pt; clicking/dragging frames and layers hits the right row; preference persists.

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

**TL-12 · Frame view ▸ Placement / Preview in context / Help / Close panel** — done
- Done: Placement Bottom/Top/Left/Right/Floating, Close panel, Preview in context (TL-10), and Help (F1), which
  opens Preferences ▸ Shortcuts as the searchable command/function catalogue.
- Purpose (§5.4): move the Timeline to Bottom / Top / Left / Right / Floating; Help (F1); Close panel.
- Behaviour: Placement items call the existing dock/detach commands (`set_panel_placement`); Close hides the panel.
- Data: workspace layout. Depends on: UI-only.
- Acceptance: each placement docks the timeline accordingly and is restorable; Help and global F1 open the
  searchable catalogue (`panel_tests::preferences_dialog_previews_saves_cancels_and_resets`).

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
The narrow rail opens a group from its corner mark, secondary click, long-press or focused →; a body click selects
its remembered tool. The R-038 shared flyout rows, Brushes panel, Brush/Pencil properties and Customize tools are
connected. Customize lists implemented tools only and edits a draft: Add appends to the selected slot, Remove
returns a tool to Available, drag reorders, Restore default resets the draft, Cancel/Escape discards and OK saves.
Empty slots drop on OK. The saved tool layout is global, outside named workspace presets; changing these controls
preserves the selected tool and document Undo. History can replay, copy and save supported structured authoring steps
as project Commands; arbitrary `.nfsl` source execution and a stable public extension contract remain unavailable.
The approved 36 pt Brush/Pencil type header still needs a compact shared API; the existing locked `TypeHeader`
remains 48 pt.

Modify ▸ Bitmap ▸ Trace bitmap (`CommandId::ModifyTraceBitmap`) converts one selected bitmap (either a direct
`ElementKind::Bitmap` placement or a static 1-frame `Graphic` symbol instance wrapping a bitmap, such as images placed
via `Import graphics to Stage…`, drag-and-drop or `Place on Stage`) into editable vector paths with cancellable background
preview, colour/palette presets, curve/noise controls, fill/stroke output and Ignore white. Success preserves
placement and the selected tool and creates one Undo; multicolour output is grouped, while a single-colour compound
path preserves holes and islands. Limits are 4096×4096 source pixels, 20,000 contours, 1,000,000 boundary edges and
100,000 commands per colour. Full palette retains at most 20,000 exact RGBA colours. Unsupported scripts, filters,
non-Normal blending, bitmap caching and guided automatic-key insertion produce actionable errors without changing
the document. Bitmap Properties is available from the Library menu; exact resource-row and placed-bitmap context
actions are tracked separately.

**TO-01 · Rectangle primitive / Oval primitive** — done (parametric options on `Rectangle` / `Oval` in Tool options and Properties PR-02)
- Done: `Transform.primitive: Option<PrimitiveOptions>` stores Rectangle `corner_radii: [f64; 4]` (linked by default)
  and Oval `start_angle`, `end_angle` (`0–360°`), `inner_radius` (`0–99 %`) and `close_path` in document format 7.
  Geometry builders in `neoflash_core::geometry` drive evaluation, shape morphs, hit testing, native painting and HTML5
  export. While dragging a `Rectangle` on Stage (`stage_selection.rs`), pressing `↑` / `↓` arrow keys adjusts
  `rectangle_corner_radius` live with immediate visual feedback. `Modify ▸ Break Apart` (`⌘B`) converts a parametric
  rectangle or oval into its `Path`.
- Purpose: drawing objects that keep parametric corner radii / start-end angle / inner radius after drawing.
- Behaviour: drag draws and keeps the tool active for subsequent gestures; Oval previews its actual contour (including arcs and inner radius) before release; ⇧ constrains; ⌥ from centre; Properties shows PR-02 options; Break Apart converts to paths.
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

**TO-03 · Deco** — partial (`ik_puppet_3d.rs`, `panels/live_planned.rs`)
- Available: eight deterministic procedural generators, a settings draft, and a tested internal command applying
  effect, scale, segments, branch angle, step-frame and Animate-pattern settings in one Undo step.
- Missing: an approved user Apply affordance or sticky `Tool::Deco` Stage gesture using the actual pointer location
  and selected Library symbols. The Deco draft currently has no author-visible seed field.
- Acceptance remaining: Grid fill through a recorded Stage gesture fills the intended area deterministically and
  remains selected for the next gesture.

**TO-04 · Spray brush** — partial (`ik_puppet_3d.rs`, `panels/live_planned.rs`)
- Available: a seeded dot/symbol burst engine, settings draft, and tested internal command honoring the requested
  Library symbol, ellipse, angle, scale and rotation settings.
- Missing: an approved user Apply affordance or sticky `Tool::Spray` gesture, pointer sampling and selected-symbol
  authoring on Stage.
- Acceptance remaining: a recorded stroke produces deterministic elements at its real coordinates with one Undo.

**TO-05 · Bone / Bind (IK)** — partial (`ik_puppet_3d.rs`, `panels/live_planned.rs`)
- Available: persisted format-9 armatures, bounded FABRIK/constrained-CCD solving, joint limits, spring helpers,
  and a Bone panel path that persists target, length, degree-based rotation limits and stiffness and runs the solver.
  Full-affine transient vertex binding and WarpMesh deformation have tested internal command paths.
- Missing: direct Bone/Bind Stage gestures, editable live skin weights and persisted spring/skin state. Binding is
  currently baked into path geometry; transient `IkShapeSkin`/joint specs do not survive reload. Bone name, direct
  angle, authortime/runtime type, display style and spring fields in this preview panel are draft-only.
- Acceptance remaining: Stage bone creation/dragging, editable weights and saved poses must solve within limits and
  evaluate consistently in HTML5.

**TO-06 · Snap toggle** — done (`panels/tools.rs`, `ViewPrefs.snap_to_objects`)
- Done: Snap to objects (magnet) toggle at the bottom of the Tools rail (`Selected` state when active, persisted in view
  preferences) enables object snapping across drawing, selection moves, transform handles and path point drags (ST-04).
- Purpose: global Snap to objects (magnet) toggle.
- Behaviour: Selected style when on; affects drawing and dragging (ST-04).
- Data: workspace/document view setting. Depends on: editor (ST-04).
- Acceptance: with snap on, dragging an object's registration point snaps to another object's edge/centre with the
  NF2 snap ring.

**TO-07 · Tool options: Object drawing, Smooth / Straighten, Snap to objects** — partial
- Done: Under `Options` in the `Tools` panel (`panels/tools.rs`), per-tool Classic Flash icon modifiers replace the old generic settings button (`Selection`: `Snap to objects`, `Smooth`, `Straighten`; `Free Transform`: `Snap to objects`, `Rotate and Skew`, `Scale`, `Distort`, `Envelope`; `Line`/`Pen`/`Rectangle`/`Oval`: `Object drawing (J)`, `Snap to objects`; `PolyStar`: `Object drawing (J)`, `Snap to objects`, `PolyStar tool settings…`; `Pencil`: `Object drawing (J)`, `Pencil mode`; `Brush`: `Object drawing (J)`, `Lock fill`, `Brush mode`, `Brush size`, `Brush shape`, `Use pressure`, `Use tilt`; `Paint Bucket`: `Gap size`, `Lock fill`; `Eraser`: `Eraser mode`, `Faucet`, `Eraser size`; `Lasso`: `Touch selects`, `Slice merge fills`, `Polygon mode`, `Magic wand`, `Magic wand settings…`; `Zoom`: `Enlarge`, `Reduce`). Stretching the `Tools` panel horizontally reflows tools into `N` left-aligned columns (`tool_columns_for_width`, `tool_grid_row`) and compacts the floating window height from the bottom up while keeping its top `outer_position[1]` fixed (`compact_floating_tools_window_bottom_up`). Object / Merge Drawing (`J`, existing tool options and Drawing preferences), pencil smoothing/straightening,
  brush size and whole-object/fill/stroke/selected-fills/inside eraser modes (`Round` and `Square` tip shapes). Merge separates fill/stroke, merges or subtracts overlapping
  fills and splits at actual fill/stroke intersections; internal tessellation edges are not cuts. Double-clicking a raw Merge stroke with Selection (`V`) selects only connected Merge stroke segments of the same stroke paint (`merge_connected_stroke_ids`). Grouped Merge
  elements remain isolated from raw drawing and drop-time remerging. Transformed gradients keep their appearance. On a multi-selection, `Modify ▸ Break Apart` (`⌘B`, `break_apart_selection`) continues working when some selected objects are already final shapes (`merge == true`), advancing each remaining group, symbol instance, text block, bitmap, primitive or Object Drawing shape by one hierarchy step per invocation until all selected objects are shapes.
- Subselection (`A`) supports multi-anchor dragging (`move_selected_anchors`), `1 px` / `Shift + 10 px` arrow-key anchor nudging (`nudge_selected_vector_points`), rectangular marquee selection of anchor points (`subselect_marquee`), and `⌥`-dragging a corner anchor to pull out symmetric cubic Bézier tangent handles (`pull_anchor_tangent`).
- Free Transform (`Q`) double-clicking the transformation point (`reset_transformation_point`) toggles a Symbol Instance between its default center (`None`) and its symbol registration point (`Some([0.0, 0.0])`), and resets custom pivots back to center.
- Line (`N`) supports `⌥` (`Alt`) symmetric center-out drawing (`line_gesture_points`).
- Lasso (`L`) includes `Polygon mode` (click-by-click straight-edge polygon selection closed by double-click, `Enter`, or clicking the start vertex) and `Magic wand` + `Magic wand settings…` (`Threshold 0..=200`, `Smoothing: Pixels | Rough | Normal | Smooth`) for selecting contiguous color regions of bitmaps/bitmap-filled Merge shapes and same-paint Merge fills.
- Bucket applies the active solid or gradient paint to an existing shape or to a bounded planar face assembled from
  several vector contours; Gap size closes eligible contour gaps in screen units. With `Lock fill` off, clicking with a Radial gradient centers the gradient at the click point, and clicking-and-dragging with a Linear or Radial gradient sets the gradient center, rotation angle and span along the drag vector (`apply_bucket_gradient_to_element`). Ink Bottle can add the active stroke
  to a transparent contour; Eyedropper replaces the selected well with the sampled solid color or gradient, and when sampling a gradient fill automatically enables `Lock fill` (`brush_lock_fill = true`). Properties RGB edits
  replace a gradient; alpha-only edits retain stops and update their alpha.
- Eraser tests the full swept cursor footprint (`Round` disk or `Square` box) against visible geometry, including holes, masks and layer locks. Its default
  mode (`Erase normal`) cuts through raw Merge fills and Bézier strokes; `Erase fills`, `Erase lines`, `Erase selected fills` and `Erase inside` remain explicit choices, plus `Faucet` one-click fill/stroke erasure. The
  cursor is the complete preview: crossing artwork never flashes whole-object bounding boxes.
- Pen closes only inside the displayed first-anchor Close zone, auto-removes an interior anchor when clicked (`nf::PEN_REMOVE`), and converts an anchor between sharp corner and smooth curve with `⌥`-click or `⌥`-drag (`nf::PEN_CONVERT`). Escape commits a valid open contour and keeps Pen
  active, so the next click begins a second path instead of losing the first one.
- Brush: Round, Flat and Chisel presets; eight tip shapes; Size 1–200 px in document units; smoothing; pressure size
  control; Lock fill for gradient placement. The shared cursor follows zoom and ⌥⌘-drag resizes the brush. Existing
  artwork is unchanged. Pressure-bearing touch and macOS tablet-point samples drive 10–100% width when enabled;
  mouse input stays constant. Preview and saved strokes share geometry; cancellation leaves no commit. Brush retains the chosen stroke RGB/alpha/gradient with zero width; Merge normalization keeps that dormant paint without creating a visible outline.
- Pencil: independent Straighten, Smooth and Ink modes, with per-tool smoothing strength based on Drawing
  preferences. Smooth produces editable cubic curves; Ink preserves the sampled path without recognition/smoothing.
  Stroke-width presets include 0.25 and 0.5 px; the field and preset list preserve fractional values.
- Remaining: artistic/pattern brushes, advanced Pencil stroke styles/caps/joins/hinting/scaling and predictive
  smoothing-ahead feedback.
- Available R-034 controls: Bucket Gap size (None/Small/Medium/Large: 0/6/14/28 screen px) and Eraser modes; per-contour
  gap closing in screen units; swept partial clipping with original identity on the retained piece. Default fill+stroke
  cut-through is limited to raw Merge geometry; Object Drawing retains conservative stroke-segment behavior. Touched
  curves are approximated at 0.2 screen points; masked and combined fill/stroke effects are explicitly refused.
  Guided/oriented cuts use the displayed pose, retain the destination key transform and refuse automatic guided-key
  insertion if it would move the artwork. A refused cut cancels the entire gesture without mutation or Undo; reasons
  appear as Warning notifications and in status/Output.
- Acceptance: `selection_tests.rs`, `vector_gesture_tests.rs` and core Merge tests cover drawing, paint application,
  actual intersections, group isolation and Undo. `tool_workflow_tests.rs` covers eight pressure footprints,
  document-unit size across zooms, Pencil modes, sticky tools, one Undo and global Customize draft/apply/cancel.

**TO-08 · Fill/stroke chip → Color & Fill Type popover** — `fill_stroke_chip` · done (`panels/tools.rs`, `authoring_modify.rs`)
- Done: Clicking Fill or Stroke in `NF2/Fill Stroke Chip` opens the unified NF2 Color & Fill Type popover (`color_fill_popover_contents`) with fill-type selector (`None`, `Solid colour`, `Linear gradient`, `Radial gradient`, `Bitmap fill`), hex + alpha, 2D HSB/RGB colour picker, gradient stops bar, eyedropper, no colour, black-and-white reset, swap, `Add to swatches`, and collapsible `Swatches`.
- Data: `Editor::contextual_paints`, `Editor.fill/stroke`, `WorkspacePrefs.recent_colors`, document colours (`Project`).

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

**ST-03 · Stage grid** — done: Dynamic (default) adapts page contrast and display density; Static keeps manual colour and spacing. Snapping retains the configured step. See [NF2-RULERS-GUIDES-GRID.md](NF2-RULERS-GUIDES-GRID.md) (behaviour, data model, invariants, tests).
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

**ST-07 · NF2 stage cursors** — done (`stage_cursor.rs`, `stage_selection.rs`, `vector.rs`)
- Done: every stage tool shows its NF2 cursor over the canvas only (rulers, floating stage chrome, popovers and
  dialogs keep system cursors; guides keep their system resize cursor). Hover targets: Selection — `arrow-reshape-corner`
  over an unselected shape corner/endpoint or with ⌥ over a segment, `arrow-reshape-curve` over an unselected shape/line
  segment or smooth curve vertex, `arrow-move` over a selected object or fill interior; Free Transform — `resize-h/v/d1/d2`
  on handles (by screen direction, or `skew-h` with ⌘ on a corner in Free Transform), `skew-h/v` on the edge between handles,
  `rotate` just outside a corner, `move` inside the box; Subselection — `subselect-point` over a path/shape point and `pen-add`
  with ⌥ over a path/shape segment; Pen — `pen-close` over the first anchor, `pen-remove` over an existing interior anchor, `pen-convert` with ⌥ over any anchor, and `pen-add` over a selected path segment; Hand —
  `hand-closed` while dragging; Zoom — `zoom-out` with ⌥; motion path ◆ — `hand-open/closed`. The mapping follows Flash
  conventions; the design has no per-handle table (R-011). Cursors are identical in all four themes. Not yet:
  `drag-copy`.
- Purpose: 39 `NF2 Cursor/*` glyphs (tool cursors, reshape corner/curve, move, rotate, skew, hand open/closed,
  zoom in/out, eyedropper, bucket, pen add/remove/close …) over the canvas.
- Behaviour: stage tools call `neoflash_ui::cursors::show(ctx, canvas, cursor)` (hides the system cursor); chrome
  keeps system cursors; cursor reflects hover target (e.g. reshape curve near a stroke). The Pen-family runtime
  hotspot is the visible nib (`[11, 3]`); R-065 tracks synchronization of the stale locked cursor metadata.
- Data: none. Depends on: UI-only. Acceptance: each tool shows its NF2 cursor only over the canvas; hotspot accurate
  (click lands at the glyph's hotspot).

**ST-08 · Breadcrumb ▾ menu + Symbols & scenes popover** — `stage_chrome` (`panels/symbols_popover.rs`, `shell.rs`) · done
- Done: breadcrumb `▾` and overflow `…` open `Popover / Symbols & scenes` with single-pass `< 1` frame search filtering, `Scenes` · `Matches` · `Recent` · `Browse` expandable library-folder sections, `↑`/`↓` row movement, `←`/`→` folder collapse/expand, `↵` edit, `⌥↵` edit in place, and `Esc` / click-outside dismissal (`⌘↵ new window` stays disabled with its hint).
- Purpose: ▾ lists sibling scenes/symbols with search; popover for large projects (sections Scenes · Matches ·
  Recent · Browse, keyboard ↑↓ ↵ ⌥↵ ⌘↵); Breadcrumb overflow "…" for long paths.
- Data: `Project.symbols`, library folders; Recent = editor history. Depends on: editor (scenes need TL-03).
- Acceptance: with 300 symbols, typing filters in < 1 frame; ↵ edits the symbol; ⌥↵ edits in place.

**ST-09 · Toasts / progress / spinner** — partial
- Available: one R-044 notification host, at most three toasts, newest at the bottom, centred 16 pt above the canvas
  bottom outside docks; with no canvas, above the status bar. Repeated messages coalesce and restart the timer.
  Info/Success expire after 4 s, Warning after 6 s, Error stays until dismissed or its action is used; hover pauses
  expiry. New notifications wait while a modal is open.
- Routing: partial-stroke refusals use Warning; failed Save and explicit external-change reload use Error with
  Show. Every notification also enters its originating document's Output; Show exposes its diagnostic in the active
  document's Output after a tab switch. History lasts for the editor session, not a persistent disk log.
- Remaining: ordinary Open/import/export and successful Save retain existing status/Output or dialog feedback.
  General export-success/failed-Open toast routing and VoiceOver speech acceptance remain open. Progress/Spinner
  are used by their connected operations, not a replacement for every long-running workflow.
- Data/API: editor-only `notifications.rs` queue and `Editor::notify`; shared `Toast`, `progress`, `spinner`.
- Acceptance: notification families cover queue limits, coalescing, modal delay, hover/expiry, canvas bounds,
  dismissal and Output actions. Broader acceptance still requires export Success and failed-Open Error routing.

**ST-10 · Dock compass** — absent (editor uses `dock_target` edge zones)
- Purpose: `NF2/Dock Compass` — 5 drop targets (edges + centre = add as tab) shown while dragging a panel.
- Depends on: UI-only (`components::dock_compass` exists). Acceptance: dropping on centre tabs the panel into the
  group; edges split.

**ST-11 · Canvas editing chrome variants** — partial
- Done: Raw Merge selections clip the existing NF2 hatch to the actual fill/stroke silhouette and authored masks,
  preserving holes and empty bounding-box areas. The two-tone hatch is theme-independent and remains visible with
  Lasso active, in nested symbol editing and over onion-skin previews; ghosts themselves are not selected. Lasso
  defaults to cutting and selecting only the enclosed raw Merge fill/stroke fragments. A rectangular marquee applies
  the same split, so Backspace removes only the selected segment; Shift preserves the prior selection. Object Drawing
  placements and authoring groups remain whole. Selection and Subselection draw `NF2/Selection Box` per other selected placement (Symbol for instances, Static text
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
  Shape Selection hatch, Gradient Transform boundaries and handles.
- Behaviour: `Tool::Gradient` (`F`) draws classic Flash Linear boundaries (two parallel perpendicular boundary lines at `x = ±819.2`, Center circle `(0, 0)`, Width square `[→]` at `(819.2, 0)`, and Rotation circle at the end of the right boundary line) and Radial boundaries (64-segment elliptical boundary ring + horizontal diameter axis line, Center circle `(0, 0)`, Focal point inverted triangle `▽` + circle, Width square `[→]`, proportional Radius/Size circle with inward arrow, and Rotation circle). Remaining: on-stage primitive corner/angle handles.
- Brush uses the shared zoom-following footprint/resize cursor; Pencil retains its existing draft overlay without
  the separately illustrated smoothing-ahead preview. Still `StageOverlay`: rectangle/oval/line/polystar drafts,
  pen/pencil draft stroke, lasso, eraser size circle and the gradient tool (`paint_gradient_handles`).
- Panel behavior: Tools uses the standard NF2 panel title/header and keeps the existing tool, paint, view and modifier
  controls separated by dividers without redundant body captions. A lone floating Tools window can attach to eight
  magnetic positions around the Stage workspace: side centres use the narrow vertical rail, while top/bottom centres
  and corners use the compact horizontal layout and open inward. Dragging outside the magnetic range restores free
  placement and ordinary docking. The Fill/Stroke block is centred with the surrounding grid. Fill/Stroke from
  detached Tools opens and focuses the real Color panel.

## 4. Properties variants (`crates/neoflash-editor/src/panels/properties.rs`, `text_boxes.rs`, `paint_controls.rs`)

Properties is contextual: object selection takes precedence, followed by Timeline range/frame, Stage when Select is
active with no selection, and active-tool settings otherwise. There is no separate `Selection / Tool` switch. Explicit
entry points such as Brushes ▸ Brush options activate the matching tool and request its settings without leaking the
override across documents or later context changes. Every semantically paired value row in Properties, Transform,
Color, Text, Filters, Motion, Info, Accessibility, live/planned tool settings and Sound Properties uses the same
two-column grid: equal cells, the NF2 S2 gap and an 80 pt minimum. Narrow surfaces stack left before right. A trailing
Link/lock/action stays inside its own cell and cannot move the second-column axis; colour rows keep swatch+hex in the
first cell and alpha in the second. Full-span rows and intentional action or three/four-column compositions retain
their own layout.

**PR-01 · Properties — Text** — done (`text_boxes.rs`, `ui_icons.rs`, `neoflash-core/src/text.rs`, `web/text.js`, `web/text-input.js`)
- Stage authoring: clicking with Text creates point text that grows with its content; dragging creates a fixed,
  multiline wrapping TextBox; double-clicking an existing TextBox from `Selection`, `Free Transform` or `Subselection`
  enters inline Stage text editing (`Esc` returns to `Selection`). When a TextBox is selected, Stage draws TLF-style
  In/Out flow port boxes at the top-left/bottom-right corners and dashed vertical column-gutter dividers when `Columns > 1`
  (`ui_icons::paint_container_guides`). Break Apart splits multi-character text into positioned characters and a subsequent
  Break Apart converts a character to editable contours from its installed font; single-character text converts
  directly. Conversion is atomic and preserves supported per-character formatting rather than synthesising boxes.
- Inline editing uses UTF-16 caret/range selection, keyboard and text insertion, clipboard operations, and exact
  mixed-run character formatting over the selected range. Paragraph changes expand to intersecting paragraphs; a
  collapsed caret carries its typing format. Each accepted edit is one Undo step, while rejected input preserves the
  prior text and selection. Complete platform IME/grapheme conformance remains outside this bounded path.
- Complete Flash CS5 `Properties — Text` inspector (`text_boxes.rs`):
  - Header: `Text type` (`TLF Text (CS5)` / `ClassicStatic` / `Dynamic Text` / `Input Text`), `Orientation`
    (`Horizontal` / `Vertical LTR` / `Vertical RTL`) + `Direction` (`LTR` / `RTL`), and `Read only` / `Selectable` / `Editable` mode select.
  - `Character`: `Family`, `Style` (`Regular` / `Bold` / `Italic` / `Bold Italic`) + `Embed…` (opening the NF2 Font
    Embedding dialog on the selected Family/Style and committing ranges/custom characters in one Undo), `Size` (`pt`) +
    `Leading` (`pt` or `%`), `Color` + `Tracking` (`px`), `Highlight` optional slash-swatch + `Auto kern`, `Anti-alias`
    (`Readability` / `Animation` / `Bitmap text (no anti-alias)` / `Use device fonts`), `Rotation` (`Auto` / `0°` / `90°` /
    `180°` / `270°`), and `Strikethrough` / `Underline` / `Superscript` / `Subscript` vector icon strip.
  - `Advanced character`: `Case` (`Default` / `Caps` / `Small Caps` / `Uppercase` / `Lowercase` / `Caps to Small Caps`),
    `Digit case` (`Default` / `Lining` / `Old-Style`), `Digit width` (`Default` / `Proportional` / `Tabular`),
    `Ligatures` (`Minimum` / `Common` / `Uncommon` / `Exotic`), `Break` (`All` / `Any` / `Auto` / `No Break`),
    `Baseline shift` (`px`), and `Locale`.
  - `Paragraph`: 5-mode `Format` icon strip (`Align Left` / `Align Center` / `Align Right` / `Justify` / `Justify All`),
    two-column `Left` + `Right` margins, `Indent` + `Justify` mode (`Inter-word` / `Distribute`), and two-column
    `Space before` + `Space after` (`pt`).
  - `Container and flow`: `Behavior` (`Single line` / `Multiline` / `Multiline no wrap` / `Password`), `Max chars` +
    4-mode `Vertical alignment` icon strip (`Top` / `Center` / `Bottom` / `Justify`), `Columns` (`1..=50`) + `Gutter`
    (`px`) with custom column/gutter icons, 4-sided `Padding` (`L` / `R` / `T` / `B`) with `Lock padding` contained in
    the right cell so it never shifts the shared second-column origin,
    `Border` optional slash-swatch + `Border width` (`pt`) + `Background` optional slash-swatch, and `1st line offset`
    (`Auto` / `Ascent` / `Line Height` / `Fixed pt`).
  - `Options`: `Link` (`URL`) + `Target` (`_blank` / `_self` / `_parent` / `_top`), `Variable` + `Restrict`, and `Render as HTML`.
- Data: `ElementKind::TextBox { text, width, height, format: TextFormat, word_wrap, multiline }`; `TextFormat.fieldOptions`
  carries all Flash CS5 character, advanced character, paragraph, container/flow (`column_count`, `column_gutter`,
  `vertical_align`, 4-sided padding, `border_width`, `first_line_offset_mode` + `first_line_offset`) and options fields.
- HTML5 runtime: only `Input` text fields mount a transparent, scroll-free DOM `<input>`/`<textarea>` overlay on click;
  `Static` and `Dynamic` text fields stay on Canvas 2D and inherit all authored `TextFormat` and `AuthoredTextFieldOptions`
  properties (including multi-column flow, 4-sided padding, vertical align, highlight/strikethrough/superscript/subscript,
  HTML tags, timeline variable binding, and URL navigation).
- Acceptance: Input text authored in the editor is editable in HTML5 without scrollbars; Dynamic text has an instance name
  and variable binding reachable from AS3/AS2; Static text stays non-selectable unless "selectable"; multi-column flow
  matches between Stage and HTML5.

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

**PR-04 · Movie clip — Colour effect styles** — done (`panels/properties.rs` `color_effect_section_ui`: `Style: None / Brightness / Tint / Advanced / Alpha` with value fields, sliders, tint colour picker and two-column Advanced RGBA multiplier/offset rows)
- Purpose: Style None / Brightness / Tint / Advanced / Alpha + value slider (Tint: colour + %, Advanced: 8 fields).
- Layout: Style/value and every Advanced multiplier/offset pair use the shared responsive grid; Tint keeps its
  swatch+hex value in the first cell and alpha/amount in the second.
- Data: `Transform.color_transform: ColorTransform` already exists and interpolates in tweens (format 5).
- Depends on: editor. Acceptance: Tint 50 % red shows in editor and HTML5; Undo restores; tween between two
  tints interpolates.

**PR-05 · Movie clip & Layer — Display & Filters** — partial (`panels/filters.rs::display_and_filters_sections_ui`, wired into `panels/properties.rs` for selected symbol instances and `Window ▸ Panels ▸ Filters`; complete Flash 8–CS6 / Animate filter controls, presets, layer/frame filters, full tween interpolation, and live Stage rendering tracked in `T-193`)
- Controls: Display: Visible, Cache as bitmap, Blending (Normal, Layer, Multiply, Screen, Lighten, Darken,
  Difference, Add, Subtract, Invert, Alpha, Erase, Overlay, Hardlight), Render Original / Export as bitmap. Filters
  table (`Drop shadow`, `Blur`, `Glow`, `Bevel`, `Gradient glow`, `Gradient bevel`, `Adjust color`) with add / duplicate / remove, reorder up/down, per-filter `Enable`/`Disable` toggle (`Enable All` / `Disable All`, `Reset Filter`, `Remove All`, `Copy` / `Paste` filters), named **Filter Presets** (`Save Preset…`, apply, delete), and full per-filter parameters:
  - `Drop shadow`: `Blur X` / `Blur Y` + `Lock X/Y`, `Strength %`, `Quality` (`Low` / `Medium` / `High`), `Angle °`, `Distance px`, `Color` + `Alpha %`, `Knockout`, `Inner shadow`, `Hide object`.
  - `Blur`: `Blur X` / `Blur Y` + `Lock X/Y`, `Quality` (`Low` / `Medium` / `High`).
  - `Glow`: `Blur X` / `Blur Y` + `Lock X/Y`, `Strength %`, `Quality`, `Color` + `Alpha %`, `Knockout`, `Inner glow`.
  - `Bevel`: `Blur X` / `Blur Y` + `Lock X/Y`, `Strength %`, `Quality`, `Shadow` & `Highlight` `Color` + `Alpha %`, `Angle °`, `Distance px`, `Knockout`, `Type` (`Inner` / `Outer` / `Full`).
  - `Gradient glow` & `Gradient bevel`: interactive multi-stop gradient bar (`colors` / `alphas` / `ratios`), `Blur X` / `Blur Y` + `Lock X/Y`, `Strength %`, `Quality`, `Angle °`, `Distance px`, `Knockout`, `Type` (`Inner` / `Outer` / `Full`).
  - `Adjust color`: `Brightness`, `Contrast`, `Saturation`, `Hue` (`-180°..+180°`), `Preset`, and `Reset`, persisted/recovered across save & reopen.
  - **Layer / Keyframe Filters** (Flash CS5.5 / CS6 / Animate): when a Timeline Layer or Keyframe is selected without a single Stage object, `Filters` and `Blending` apply directly to the layer/keyframe span.
  Every semantically paired value row uses the shared grid; asymmetric action rows remain intentional exceptions.
- Data: `Element.{filters, blend_mode, cache_as_bitmap, visible}` plus per-filter `enabled` and layer/keyframe filters reusing `FilterSpec` (max 8 per target), with full Classic & Motion Tween interpolation (including `GradientGlow`/`GradientBevel` stops and automatic zero-strength dummy filter matching when a filter exists on only one keyframe of a tween).
- Depends on: engine (authored → runtime display graph, native editor Stage raster preview). Acceptance: Drop Shadow, Blur, Glow, Bevel, Gradient Glow/Bevel, and Adjust Color render live on the native Editor Stage and in HTML5; filter order, enable state, presets, and tween interpolation are preserved; max-8 limit is reported as a diagnostic.

**PR-06 · Movie clip — Swap…, Parameters tab** — partial (`Properties ▸ Swap…` symbol-picker popover, Library and Stage context `Swap symbol…` plus AE-style `⌥`-drag replacement work; component `Parameters` tab remains absent)
- Done: selecting an instance on Stage (or selecting a layer with a symbol instance) and clicking `Swap…` in `Properties`,
  choosing `Swap symbol` on a Library symbol (row context menu or ⋯ menu) or Stage context `Swap symbol…`, or holding `⌥`
  (`Alt`) while dragging a symbol from Library onto the Stage (`NF2/Drag Ghost` `Replace “<old>” with “<new>”` + `Selection Box`
  (`Symbol`), or `Place “<new>”` without `⌥`) or Timeline layer row/frame span (`timeline_row_selected` fill + `1 pt`
  `accent_default` outline + `Replace on all keyframes of “<layer>”`), replaces `ElementKind::Instance.symbol_id` across the
  layer/instance while preserving transform, instance name, scripts, filters, blend mode, accessibility and compatible playback
  settings (`authoring_symbols.rs::swap_layer_or_selected_symbol`, cycle-safe, one Undo). Selected symbol-instance Properties
  also provides symbol behavior switching (`Movie Clip` / `Button` / `Graphic`), `Edit symbol`, `Duplicate…`, and
  two-column `Position and size` (`W`/`H` + `Constrain proportions` lock inside the H cell on Row 1, `X`/`Y` on Row 2,
  `Scale X`/`Scale Y` in `100 %` on Row 3, `Rotation`/`Skew` on Row 4), with one stable second-column axis.
- Parameters tab: component parameters — no component model. **NEW**, P3.
- Acceptance: Swap keeps position/filters/name, one Undo; Undo restores the original symbol.

**PR-07 · Graphic / Movie clip — First frame ("Start from") & thumbnail strip** — partial (MovieClip `Timeline ▸ Start at frame` Stepper `1..=count` + tertiary note and Graphic `Playback` + `First frame` `1..=count` work per `R-042`; layer-level `Sync layer to timeline` works in `Layer properties`, the layer context menu and Timeline `⋯`; clickable Graphic thumbnail strip remains open)
- Purpose: `Start at frame` / `First frame` (`1..=frame_count`) in Properties sets the starting child frame (`LoopFrom` / `PlayOnceFrom` /
  `SingleFrame`), and `Sync layer to timeline` (`LayerProperties.sync_timeline` in format 8) locks child timeline frames
  to the parent frame index when leading frames on the layer are empty; clickable thumbnail strip to pick the first frame.
  Depends on: editor (thumbnails as TL-10).

**PR-08 · Properties — Sound (frame)** — `panels/properties_sound.rs` · partial
- Done: Sound section of frame Properties when the keyframe has a sound: sound select (None + library sounds), Effect presets + Custom, `Envelope…` opening the modal `Edit envelope` dialog (`panels/edit_envelope.rs`, DL-09; format-7 `SoundEffect::Custom` + `FrameSound.envelope`), Sync, Repeat/Loop + count, file info, Play (native preview), and `Sound properties…` opening the modal `Sound properties` dialog (`panels/sound_properties.rs`, DL-08). Real decoded peaks are shared with the timeline; Edit envelope waveform/trim UI still awaits R-016.
- Layout: paired values in Sound Properties, including bit rate/quality, use the shared responsive grid; waveform and
  action rows keep their intentional full-width/asymmetric composition.
- Controls: Label name/type; sound file select; Effect (None, Left/Right channel, Fade left→right / right→left, Fade in,
  Fade out, Custom) + Envelope… (DL-09); Sync Event / Start / Stop / Stream; Repeat / Loop + count; waveform; file info;
  Play; Sound properties… (DL-08).
- Data: `FrameSound { resource, sync, loops, loop_forever, effect, envelope }` (TL-07, DL-09, format 7).
  Acceptance: Event sound plays once per keyframe hit; Stop stops it; Stream follows playhead.

**PR-09 · Properties — Video** — partial (`panels/properties_video.rs`)
- Working: contextual `Properties ▸ Video` section edits the active `TimelineVideoSpan`'s Library `video/*` resource,
  `Sync` (`Stream` synchronous frame-locked vs `Event` asynchronous / `Stop`), `Loop` and `Mute`, interactive `SpanType::Video`
  trim handles + numeric `In` / `Out` (`ms`) fields, `Split at playhead` (`TimelineEdit::SplitClip`, tightening the left
  segment's out-point, advancing the right segment's in-point, partitioning cue points, and rejecting `Event`/`Stop`
  mid-span splits atomically), `Linked track` pairing (`LayerProperties.linked_layer`, format 12) with `Link`/`Unlink`,
  `Extract audio` (demuxing embedded AVI audio or transcoding via `ffmpeg` onto a linked `audio/*` layer in one Undo
  step), and `VideoCuePoint` authoring (`Event` / `Navigation`), removal and proportional retiming.
- Missing: instance `Swap…`, player `Skin`, `Scale Fit / Fill / None`, per-clip `Volume` and native Progressive streaming.
- Data: `ElementKind::Video { width, height }` plus `TimelineVideoSpan`; the Rust `Player` schedules authored
  Stream/Event/Stop commands through WASM to `web/video.js`, while scripted NetStream remains available.
- Acceptance: embedded authored video plays in HTML5 and native Stage preview without script; `Stream` tracks follow the
  playhead synchronously while `Event` tracks continue asynchronously across halted timelines.

**PR-10 · Properties — Motion tween** — done (`panels/motion_tween.rs`)
- Done: replaces the frame Tween section while the playhead is inside a motion tween and exactly one member of that
  span is selected on Stage. Ease + Edit curve…, Rotate turns/direction, Orient to path, Path X/Y/W/H, Sync graphic
  symbols and Snap to motion guide read/write that object's format-10 track only; Show path/frame dots remain
  workspace display preferences. Switching Stage selection switches the displayed settings and path. No or multiple
  selection shows the existing empty/mixed state and never targets the first object. Format-7/8 shared settings remain
  the fallback for older documents.
- Controls: Ease −100 in … +100 out + Edit curve… (→ PP-01); Rotation count + direction CW/CCW + Orient to path; Path
  X/Y/W/H; Show path / frame dots; Sync graphic symbols; Snap to motion guide.
- Data: `MotionTrackOverride` on the span start plus `MotionPropertyKey` on ◆ keys (TL-05). Acceptance: changing
  Ease, Orient, Path, Sync or Snap for one of two objects leaves the sibling untouched in editor, HTML5 and Lottie.
- Reusable: `Editor::set_orient_to_path` (any tweened span); `TimelineEdit::SyncSymbols` / `Editor::sync_symbols`;
  `timeline_motion.rs::paint_motion_path`.

**PR-11 · Frame easing values** — done (`properties` Tween section)
- Done: Y1/Y2 accept −100…100 (X stays 0–1); "Flash ease" option with an Acceleration field −100…100.
- Behaviour: allow Y outside 0–1 (Ease panel ranges: −30…130 % in the overshoot graph); add Flash acceleration
  −100…100 as the existing field style (no new control type — if a field must be added, request).
- Depends on: UI-only. Acceptance: Y2 = 1.12 saves, reloads and overshoots in editor and HTML5.

## 5. Color panel (`crates/neoflash-editor/src/authoring_modify.rs` › `color_panel_*`, `fill_type_select`)

**CO-01 · Fill type Linear / Radial gradient & unified contextual paint** — `fill_type_select` · done (`authoring_modify.rs`, `paint_controls.rs`)
- Done: Color panel and Color popover place the `Fill` well (`ColorWellTarget::Fill`) on the left and the `Stroke` well (`ColorWellTarget::Stroke`) on the right, operate with a single unified contextual UI (`Editor::contextual_paints`, without a `Selection`/`Drawing` toggle or `Mixed` apply gates), and switch both fill and stroke paints between `None`, `Solid colour`, `Linear gradient`, `Radial gradient`, and `Bitmap fill`. `gradient_stops_bar_ui` renders a 64-slice gradient mesh (`GradientPaint::sample_at_unit`), accepts clicks on both the 20 px bar and the 20 px pointer rail below it to insert a stop, preserves `*selected_stop` across stop-ratio sorting so subsequent color picks edit the newly added or dragged stop, and removes stops when dragged vertically off (> 2 stops). The `Overflow` dropdown (`GradientSpread::{Extend, Reflect, Repeat}`) renders visual overflow preview swatches (`paint_overflow_preview`) next to `Extend`, `Reflect`, and `Repeat`, and the `Linear RGB` checkbox (`GradientPaint.linear_rgb`) toggles linear-light vs sRGB stop interpolation. Shared multi-stop gradient swatches (`paint_gradient_swatch`, `paint_fill_stroke_chip_gradients` in `paint_controls.rs`) render across `Tools` (`FillStrokeChip`), `Color` (`ColorWell`), and `Properties` (`color_swatch`). `No fill` / `No stroke` (`PaintFillType::None`, `[0, 0, 0, 0]` with no gradient or bitmap fill) is strictly separated from `Solid colour` with `α = 0 %` (`color != [0, 0, 0, 0]`, `color[3] == 0`, normalized via `ensure_solid_non_none`). On a multi-selection, changing Fill (solid colour, gradient, or bitmap fill) updates only selected objects that actually have a fill (`element_has_fill == true`) and never assigns a fill to unfilled stroke-only contours until filled with the Paint Bucket (`Tool::Bucket`). Solid rectangles, ovals and paths convert cleanly to `PaintedPath` via
  `Element::to_path_commands()` with one Undo step.
- Purpose: Color panel edits gradients: gradient stops bar with draggable stops (click on bar or rail adds, drag off removes),
  Overflow `Extend` / `Reflect` / `Repeat` dropdown with visual preview icons, `Linear RGB`, selected stop edited by the picker; on-stage via Gradient Transform (`Tool::Gradient`, `F`, with `Icon::Gradient` `GRADIENT_TRANSFORM_16` / `TOOL_GRADIENT_TRANSFORM_20` matching classic Flash's bordered horizontal gradient rectangle). Every color entry point (`Tools` `FillStrokeChip`, `Properties` `paint_row` / `color_row` / `Color…`, `Brushes` `ColorWell`, and `Swatches` `Open colour mixer`) opens `color_fill_popover_contents` / `show_active_color_popover`, while non-gradient RGBA color fields (`Document Settings`, `Layer Properties`, `Layer Outline Color`, filters, tint) open `show_rgba_color_popup` / `show_color32_field_popup`.
- Data: `ElementKind::PaintedPath { fill_gradient, stroke_gradient: Option<GradientPaint> }` (`GradientPaint { kind, stops, matrix, focal_point_ratio, spread, linear_rgb }`).
- Acceptance: switching a filled path to Radial creates a 2-stop gradient with one Undo; stops, Overflow and Linear RGB are editable; HTML5 and Stage rendering match.

**CO-02 · Fill type Bitmap fill (+ "Color — Bitmap fill" panel)** — done (`panels/live_planned.rs`, `bitmaps.rs`)
- The Color fill-type entry opens the working panel. Its tile grid imports a deterministic bitmap resource and applies
  an `AuthoredBitmapFill` (resource, UV matrix, repeat and smoothing) to an editable selected vector shape in one Undo
  step; applying without a shape imports the tile for later use. Imported library bitmaps can be bound through the same
  engine path. Invalid or locked targets leave the document unchanged.

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
  items (**NEW** with DL-11). Multiple documents are available through the document tabs below the main menu and Window document list;
  the Library currently follows the active document. Its independent document picker and pin remain unavailable.
- Depends on: editor. Acceptance: an imported PNG appears in the Library tree and can be dragged to the stage.
- Placement: an explicit Finder or Library drop keeps the exact pointer coordinate, including pasteboard positions;
  automatic placement without a pointer remains fitted to the page.

## 7. Actions and code editor (`panels/actions.rs` `CODE_TOOLS`, `sources.rs`)

Status: AC-02, AC-05 and AC-06 work and AC-01 re-indents only (details in [NF2-ACTIONS-EDITOR.md](NF2-ACTIONS-EDITOR.md));
AC-03 connects to the Test Movie debugger; AC-04 and AC-07 stay disabled; AC-08 is partial.

| ID | Element | Purpose / behaviour | Data / engine | Acceptance |
| --- | --- | --- | --- | --- |
| AC-01 | Auto-format | Reformat the current script (indent, braces, spacing) per language; one Undo; refuses on parse error with the error shown. | Needs a formatter per language: JS/TS via the existing oxc stack in `neoflash-script` (verify printer availability); AS2/AS3 needs a pretty-printer on the AS parser. | Formatting is idempotent; comments preserved. |
| AC-02 | Show code hint | Popover with members for the identifier before the caret (`Menu Item` + type on the right); ⌃Space; delay from Preferences ▸ Code editor. | Shared compiler language service: catalogue/declaration context plus advisory AST flow for assignments, scopes, branch joins and known signatures; incomplete/dynamic cases remain conservative. | `this.` lists MovieClip members supported by the runtime; unsupported ones are not offered as working. |
| AC-03 | Debug options | Starts debugging; requests Pause in a running session (→ PP-04). | Connected Chromium debugger (PP-04). | Debugger Toggle breakpoint uses the current Actions caret line; Remove all clears the list. Gutter breakpoint interaction remains pending. |
| AC-04 | Collapse between braces / expand all | Folding ranges in the code editor with ▸/▾ gutter markers. | Brace matching on the token stream (`components::code::tokens`). Editor only. | Folded code keeps text and Undo intact. |
| AC-05 | Block comment / line comment / uncomment | Toggle `/* */` or `//` over the selection/line; one Undo. | Editor only (language comment syntax is identical for AS/JS/TS). | Round-trip comment → uncomment restores the exact text. |
| AC-06 | Toolbox sidebar + Script navigator | Left sidebar: toolbox search + tree (Top level, Language elements, packages); navigator (current selection, scenes, symbol definitions with scripts). Double-click inserts. | Catalogue data (as AC-02); navigator from `Timeline.scripts`, `Element.scripts`, `Project.sources`. | Navigator jumps to every script in the document. |
| AC-07 | Script assist | Form-based editing of simple calls (Flash Script Assist). | Large; parameter metadata per API. | P3; disabled until specified. |
| AC-08 | Gutter markers, inline error peek + Quick fix ⌘., overview ruler, code hint popover | Structured errors/warnings, exact range underlines, hover/F8 peek, correct-file navigation and safe ⌘. fixes work. Multiple fixes use the approved mouse/keyboard menu and one Undo. F8 continues a paused debug session, otherwise navigates problems in focused Actions; ⇧F8 navigates backwards. Breakpoint/fold markers and overview ruler remain pending. | `neoflash_script::Diagnostic` preserves severity, code, byte/Unicode range, path and safe fix suggestions; Actions and Compiler errors consume it. | Warning shows amber triangle; peek shows code + message under the line. |

## 8. Output and Compiler errors (`panels/actions.rs` › `output_panel`)

**OU-01 · Output Show filter and timestamps** — absent (comment: messages carry no level or time stamp)
- Purpose: Show All / Errors / Warnings / Trace select; timestamped lines; info / warning (amber) / error (red).
- Data: `Editor.output: Vec<String>`. **NEW (editor)** structured entries `{time, level, source, text}`.
- Depends on: editor. Acceptance: filter "Errors" shows only error lines; copy keeps timestamps.

**OU-02 · trace() from native Play / Test Movie** — partial
- Available: native Play with `Control ▸ Live ActionScript` enabled drains bounded frame/object-script `trace()`
  output into Output.
- Missing: browser Test Movie still writes trace to the page `#trace` element; it has no browser → editor Output
  bridge. Acceptance remaining: `trace("x")` in a Test Movie frame script appears in editor Output within one second.

## 9. Planned panels (`panels/planned.rs` enum `PlannedPanel`, 22 remaining previews in `planned_panels.rs` / `planned_dialogs.rs` / `planned_documents.rs`; PP-01, PP-02, PP-03, PP-05, PP-08, PP-09, PP-10, PP-11, PP-12, ST-08, DL-02, DL-08, DL-09 and DL-16 are real panels/dialogs/popovers now)

**PP-01 · Ease** — done (`panels/ease.rs`)
- Done: bound to the tween span under the playhead (motion: its start key); presets Linear, Ease In/Out/In-Out, Back,
  Bounce, Elastic (`Easing::{Bounce, Elastic}` with `EaseDirection::{In, Out, InOut}` in format 7, Penner exact
  formulas) and Custom; `Property` select (`All`, `Position`, `Rotation`, `Scale`, `Colour`) + `One curve for all`
  editing the selected object's format-10 track on motion tweens (legacy `Keyframe.property_eases` remains the
  fallback); draggable handles; X1–Y2 fields; preview track with Play/Stop; Reset · Cancel · Apply (one Undo).
- Controls: context line ("Classic tween · <layer> · frames a–b"); Property select All / Position / Rotation / Scale /
  Colour / Filters + "One curve for all"; presets Linear, Ease In, Ease Out, Ease In-Out, Back, Bounce, Elastic, Custom;
  `EaseGraph` with draggable Bézier handles (range 0–100 / Overshoot / Undershoot switches automatically); X1 Y1 X2 Y2
  fields; preview track with play and "Frame n / N · p %"; Reset · Cancel · Apply.
- Behaviour: edits a draft; for Motion, Apply writes `MotionTrackOverride.easing` or `.property_eases` for the single
  selected element ID; classic/shape and legacy Motion retain their shared fields. Cancel discards; X fields 0–1,
  Y fields −0.3…1.3 in the graph (model allows |y| ≤ 100).
- Data: `Easing::{Linear, EaseIn, EaseOut, EaseInOut, CubicBezier{..}, Flash{..}, Bounce{..}, Elastic{..}}`, legacy
  `Keyframe.property_eases` and format-10 `MotionTrackOverride.property_eases`; evaluation solves Bézier x by
  bisection and evaluates Penner Bounce and Elastic exactly.
- Acceptance: Apply on "Back out" (Y2 > 1) or "Bounce" overshoots/bounces in editor preview and HTML5; Cancel leaves the
  document unchanged (History unchanged).

**PP-02 · Motion editor** — done (`panels/motion_editor.rs`)
- Done: follows exactly one Stage-selected object in the active Motion span. Rows X, Y, Rotation Z, Scale X/Y, Skew,
  Alpha and Tint/Colour show only that stable element ID's ◆ keys and format-10 easing; value edits, add/remove,
  previous/next navigation and vertical/horizontal key drags cannot key, retime or remove sibling tracks. Switching
  selection switches lanes; no or multiple selection shows the existing empty/mixed state. `Add property` menus,
  Graph size / Expanded sliders and the `Frames` span-duration stepper retain their existing composition.
- Controls: columns Property · Value · Ease · Keyframe + frame ruler; sections Basic motion (X, Y, Rotation Z),
  Transformation (Scale X/Y, Skew X/Y), Colour effect (Alpha, Tint …), Filters; per row value field, ease select (No
  ease, Simple (slow), Bounce …), ◀ ◆ ▶ keyframe navigation, curve lane; Add property; footer Graph size, Expanded
  height, visible frames; hint "Click a curve to edit · ⌥-drag a keyframe to copy · ⇧ snaps to frames".
- Data/Depends on: TL-05 format-10 object-scoped Motion tracks. Acceptance: adding/moving/removing Rotation for object
  A at frame 20 leaves object B's keys and value untouched; curve drag is one Undo and survives save/reopen/HTML5.

**PP-03 · Properties — Motion tween** = PR-10.

**PP-04 · Debugger** — partial (`panels/debugger.rs`, existing NF2 composition)
- Connected: Actions Debug options launches an owned Chromium Test Movie and pauses at an authored statement;
  clicking it while running requests Pause. Continue F8, Step over F10, Step in F11, Step out ⇧F11, Stop,
  caret-line Toggle breakpoint ⌘B, Remove all, call-frame selection and read-only Variables use the live VM.
  Test Movie ⌘↵ restarts the active session.
- Data: editor-session breakpoints and VM state, never serialized document data. Source edits/document switches
  invalidate the session; Restart preserves breakpoints while discarding old frame/object/request IDs.
- Transport: isolated temporary Chromium profile and private inherited CDP pipe; no network debugging listener
  or attachment to unrelated browser targets. Existing compiler maps provide authored positions.
- Limits: statement-start AS mapping, grouped identical handlers, generated-frame fallback, bounded stack/locals
  and helper stepping. Variables provides primitive values/object summaries without invoking getters. Watch
  expressions, editable/deep object inspection, change highlighting and a breakpoint gutter remain pending;
  missing visual states stay with R-033.
- Acceptance: real exported AS2/JS scenarios verify breakpoints, all three steps, locals, a timer that freezes during
  Pause and advances after Continue, Restart and owned-process cleanup. This does not imply full Flash VM semantics.

**PP-05 · Project** — done (`panels/project.rs`, `Window ▸ Panels ▸ Project`)
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

**PP-08 · Scene** — done (`panels/scene.rs`, Window ▸ Panels ▸ Scene ⇧F2): Find scene, list (current highlighted), Add / Duplicate / Delete; drag to
reorder; double-click rename. Depends on TL-03. Acceptance: reorder changes playback order; delete of the last scene
refused.

**PP-09 · History** — done (`panels/history.rs`, `Window ▸ Panels ▸ History` `⌘F10`)
- Done: chronological `Open` + named step rows (`History::undo_labels` / `redo_labels`) with the current step checked
  and undone steps dimmed yet clickable (`History::step_to`), `Edit ▸ Undo <step>` / `Redo <step>` menu labels, and
  footer Undo · Redo · Clear history controls.
- Controls: list of steps with the current position checked; Undo / Redo; Clear history.
- Data: `History` snapshots with step labels, capacity 150 in the editor.
- Acceptance: labels match commands ("Move", "Fill colour", "Delete"); jumping 5 steps back equals 5 × Undo; Clear
  disables Undo.

**PP-10 · Swatches** — done (`panels/swatches.rs`, `Window ▸ Panels ▸ Swatches` `⌘F9`)
- Done: `SwatchesPopover` panel with hex + alpha readout, eyedropper, no colour, wheel → Color panel, Recent colours,
  Document colours (derived from `Project` paints + user-added swatches in `WorkspacePrefs.swatches`, also populated by
  Color panel ⋯ `Add swatch`), switchable Palette (`Default`, `Web 216`, `Grayscale`, `Pastels`) and Gradients (default +
  document `GradientPaint`s, preserving primitive geometry when applied), plus the Figma-style **Project colours** section
  (`Editor::project_colors_section_ui`, `T-197`) listing every unique colour in the project with its live RGBA popover swatch,
  `#HEX`, `α %`, usage count & roles (`fill, stroke, gradient, text, swatch`), stage-frame selection button (`Select elements with this colour`),
  and batch JSFL `change_color.jsfl` replacement bar (`Replace colour across project (JSFL)`). Multi-selection in `Properties`
  also exposes `Selection colours` (`selection_only = true`) and `Project colours`. Persistent file-stored swatches remain W1.9.
- Controls: hex + alpha, eyedropper, no colour, wheel → Color panel; Recent; Document (+ add); Palette (switchable);
  Gradients.
- Acceptance: clicking a swatch applies to the active well (drawing colour or selection fill/stroke with one Undo);
  Recent updates.

**PP-11 · Info** — done (`panels/info.rs`, `Window ▸ Panels ▸ Info` `⌘I`)
- Done: interactive 9-point (3×3) `RegistrationPicker` (`info_anchor`) + editable 2×2 selection `W` / `H` / `X` / `Y` fields
  (displaying and editing the chosen 9-point anchor coordinate and scaling `W` / `H` relative to that anchor as one Undo step),
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
persists; Cancel discards; Reset restores defaults for the current page only.

Done (`panels/preferences.rs`, modal like Document settings; Edit ▸ Preferences… ⌘U, Edit ▸ Keyboard shortcuts…
opens the Shortcuts page): edits live on a draft; global F1 and Timeline Frame view ▸ Help also open Shortcuts as a
searchable command/function catalogue; Dark, Light, Mid Neutral Gray and Dark Graphite preview live, and Cancel /
Esc / ✕ restores the previously saved theme; Save / Enter applies and persists; Reset to defaults resets the active
page's group. Wired across all 8 working pages:
- **General:** Theme Dark / Light / Mid Neutral Gray / Dark Graphite, UI scale (80–150 %, egui zoom), On launch (`New document` / `Last document` /
  `Example`, plus `File ▸ Open recent` up to 10 existing files), Undo levels (10–1000, history capacity), Autosave
  (`Every` switch + `1–120 Min` stepper writing atomic snapshots to `<app_support>/Recovery` and offering `Recover
  unsaved work?` on launch via `recovery.rs`), Show tooltips with shortcuts, Shift-select.
- **Code editor:** Font family (`Roboto Mono` + installed system families), font size (`8–32 px`), line height
  (`1.0–3.0`), Line numbers (with marker-only gutter strip when off), Word wrap, Auto-close brackets/quotes, Code hints
  + delay (`0–1000 ms`; `⌃Space` always immediate), Tab size (`2` / `4` / `8` spaces for `Tab`/`⇧Tab`, Auto-format and
  line comments), Format on save, Highlight current line, plus live 5-line preview.
- **Timeline:** single-frame selection by default; opt-in Span-based selection (`⌘`-click single-cell override), Default frame view and row height, Show seconds,
  Snap playhead to keyframes, Onion range, linear Onion opacity falloff (`15–100 % → 15 %`, unified with the Timeline
  onion popover), Create keyframe on transform (when off, in-between transforms edit the span keyframe), Default ease,
  Tween type on drag (`None` / `Classic tween` / `Motion tween` in one Undo step).
- **Drawing:** Show pen preview, Precise cursors (`CROSSHAIR`), Connect lines, Smooth curves (Pencil RDP tolerance),
  Recognise lines, Recognise shapes (`Rectangle` / `Ellipse`), Click accuracy (`Off` / `1` / `4` / `8 px` slop with
  exact-hit priority), Snap tolerance (`1–32 px`), Snap to pixels (`StageView::prefs.pixels_snap` + `view_defaults`).
- **Text:** Default font and size of new text, Font substitution (`Show warning` / `Substitute silently`).
- **Clipboard:** Default paste (`In centre` / `In place`), Import vector paste as editable paths (SVG clipboard text
  imports as editable `Path` elements when on or a rasterized `Bitmap` when off, in one Undo step).
- **Shortcuts (`commands.rs`):** 98-command registry driving all editor shortcut dispatch, menu labels and tool/panel
  tooltips; shortcut set selector (`NeoFlash default` + user sets with `Duplicate` / `Delete`), live Search and
  category filter, click-to-record `"Press keys…"` (`Esc` cancels, `Backspace`/`Delete` clears), `Conflicts with …`
  warning badge, `Reset command`, and `Export as HTML…`.
- **Publish:** Default target (`HTML5 · Rust / WebAssembly`, `Desktop app (macOS / Windows)`, `Video (MP4)`, `Image sequence / sprite sheet`, or `SVG animation`), Output folder + `Browse…`, `HTML wrapper`, `Assets folder`, `Source maps`, `Preview & publish` (`Preview mode`: `Technical bar` vs `Clean stage`, `Clean publish output` checkbox, and individual `FPS & frames`, `Stage size (W×H)`, `Playback controls`, `Weight & size report` and `Trace & diagnostics` checkboxes), `Compression` (`Images` JPEG quality slider `1..=100`, `Audio` policy, `Acceleration`: `GPU (WebGL)`, `Canvas 2D`, `Software`), `Strict mode`, `Treat warnings as errors`, and `Publish on save` (`T-198`). Five target routes, wrapper/template/scale/playback, JPEG, trace, strict executable wrapping and rebased compiled-code Source Map v3 are functional. Font Subset performs real TrueType subsetting with a conservative full-face fallback when preservation requires it. The packed PNG/manifest atlas is a companion artifact and published playback still uses the original resources; Assets-folder relocation, Acceleration and runtime-module minification remain incomplete. Minify intentionally compacts bundle data without rewriting executable JavaScript.
- **Updates** and controls waiting on NF2 design tokens (`R-013`–`R-020`) or standalone subsystems (merge drawing, font
  embedding `E-26`, spell check, vertical text, native OS bitmap/SVG clipboard, asset re-encoding, WebGL) stay disabled.
Screenshot aids: `NEOFLASH_SCREENSHOT_PREFERENCES=<page index>` and
`NEOFLASH_SCREENSHOT_THEME=dark|light|mid-neutral-gray|dark-graphite`.

| Page | Controls (from the preview) | Effect when wired | Model / engine | Prio |
| --- | --- | --- | --- | --- |
| General | Theme Dark / Light / Mid Neutral Gray / Dark Graphite; UI scale; Accent; Language; On launch; Undo levels; Autosave every N min; tooltips with shortcuts; Shift-select; highlight colour | Theme → `set_theme` with live draft preview and Save/Cancel restoration; Undo levels → `History::new(n)`; Autosave → recovery (`platform-recovery`); UI scale → egui zoom; Follow system, Language and Accent → **request** (no localisation, single accent token) | editor; autosave editor | P1 (theme, undo levels), P2 (autosave), P3 (language, accent) |
| Code editor | Font, size, line height; line numbers, word wrap, auto-close brackets, code hints + delay, tab size, format on save, highlight current line; colour scheme + Duplicate / Import…; syntax colour table; preview | Code editor rendering options; scheme edits must keep "red = errors only" | editor (`components::code` reads tokens — custom colours need a request) | P2 |
| Timeline | span-based selection, default frame view, row height, show seconds, snap playhead, named anchor per scene; onion range + tints + opacity falloff; auto keyframe on transform, default ease, tween type on drag | Defaults for TL-09/TL-11, onion state (`onion_before/after/opacity` exist), default `Easing` for new tweens | editor | P1 |
| Drawing | Merge / Object drawing; pen preview, solid points, precise cursors; connect lines, smooth curves, recognise lines/shapes, click accuracy; snap tolerance, snap to pixels, snap align | Tool defaults (TO-07), snapping (ST-04) | editor | P2 |
| Text | default font/size/anti-alias; font substitution warning; embed used glyphs; spell check; vertical text; input method | Defaults for new text; warnings exist in `export_warnings` | editor; embedding engine (DL-11) | P3 |
| Clipboard | bitmap resolution, gradient quality, copy as SVG, keep layer names; default paste in centre / in place; vector paste as paths; ask before large bitmaps | `clipboard.rs` behaviour switches | editor | P2 |
| Shortcuts | set + Duplicate / Rename / Delete; search; command filter; command → keys (`Kbd`); recording "Press keys…"; conflict badge; Reset command; Export as HTML… | Needs a command registry (today shortcuts are hard-coded in handlers, e.g. `view_shortcuts`) | editor (**NEW** command table) | P3 |
| Publish | default target, output folder + Browse…, HTML wrapper, assets folder, source maps; image quality, audio, acceleration; strict mode, warnings as errors, publish on save | Defaults for DL-03 | export | P2 |
| Updates | version + Check now / Release notes; channel; automatic checks; background download; usage statistics; crash reports; extensions | No update service, telemetry or extensions exist; must stay disabled/honest until `platform-release` / `platform-extensions` define them; never send data | — | P3 |

### Other planned dialogs

**DEV-01 · UI Layout Studio** — partial (`panels/ui_studio.rs`, standalone launcher, `T-194`)
- Working: developer-only multi-artboard canvas, editable layout tree and SVG import/export using existing shared
  component primitives; the macOS package exposes a second launcher for the same editor binary.
- Pending: approved NF2 composition/state contract (`NF2-REQUESTS` R-069), complete artboard Undo/Redo, durable
  artboard/session persistence and drag-origin handling across successive frames. It is not yet a production editor
  for the locked design system and must not write design tokens or locked sources.

**DL-02 · Document settings (⌘J)** — done (`panels/document_settings.rs`, `Document ▸ Document settings…` `⌘J`)
- Done: modal dialog editing a draft of stage `W × H` (with optional aspect lock), `Match contents` (translates root
  scene content so union bounds start at `(0, 0)`), `Scale content with stage` + 9-point registration anchor, `Units`
  (updates `ViewPrefs.ruler_units`), `Background`, `Frame rate`, and `Make default` (`WorkspacePrefs.document_defaults`,
  applied by `File ▸ New`). `OK` / `Enter` commits document changes in one `"Document settings"` Undo step; `Cancel` /
  `Esc` discards. A blank unmodified Stage click clears Timeline frame/range focus, so Properties returns to the Stage
  context and exposes the same Background colour directly. `Script` version stays informational until W1.12.
- Controls: stage W × H linked; Match contents / Match printer; Scale content with stage + anchor (registration
  picker); Units; Background; Frame rate (→ "N frames · s"); Script version; Make default · Cancel · OK.
- Acceptance: OK applies everything as one Undo; Cancel changes nothing.

**DL-03 · Publish settings (⇧F12)** — partial (`panels/publish_settings.rs`, `T-198`)
- Controls: `Profile` preset (`Default`, `Web Compact`, `Web Balanced`, `High Quality`, `Preview / Debug`); targets `HTML5 · Rust / WebAssembly` (enabled by default), `Desktop app (macOS / Windows)`, `Video (MP4)`, `Image sequence / sprite sheet`, and `SVG animation`; output folder/file + `Browse…`; `Include HTML wrapper` + `Template` (`Responsive`, `Fixed stage`, `Centered card`, `Fullscreen`); Full/Balanced/Compact/Shared runtime size selector; `Scale` mode (`Fit, keep ratio`, `100% (No scale)`, `Exact fit (stretch)`, `Fill, crop`); `Loop`, `Autoplay`, `Preloader`, Source Map v3 for compiled project code; `JPEG quality` (`1..=100`), `Audio` (`AudioExportPolicy`), packed PNG plus manifest companion atlas (runtime binding to it pending), `Embed fonts (subset)` (real TrueType subsetting with conservative full-face fallback), executable `Strict mode`, `Include trace()`, `Minify` (bundle data only; executable JavaScript is preserved); estimated size; `Cancel` · `OK` · `Publish`.
- Data: `neoflash_export::export_project_with_config(project, output_dir, runtime_dir, &PublishOptions { fonts, size, audio }, &HtmlPublishConfig)`
  publishes at a `SizeLevel` (Full · Balanced · Compact · Shared runtime URL) with `HtmlPublishConfig::clean_publish_default()` (clean stage output without technical HUD by default), while `Test Movie` and `Test Scene` use `PublishPrefs::preview_html_config()` (`PlayerChromeConfig` from `Preferences ▸ Publish`). `Balanced`, `Compact`, and local `Shared` packages embed `globalThis.NeoFlashWasmBase64` in `neoflash_web.js` so they open directly from `file://` without browser `fetch` CORS failure while preserving HTTP(S) streaming. `estimate_publish_options_cancellable_with_config` returns exact on-disk files, bytes and gzip bytes across all `HtmlPublishConfig` options.
  Both `OK` and `Publish` persist `self.workspace.publish = draft.settings.clone()` so settings never reset after publishing; if all target checkboxes are unchecked when `Publish` is invoked from the toolbar/menu, the editor opens `Publish settings` instead of a no-op folder picker.
- Acceptance: draft/default persistence across both `OK` and `Publish`, clean publish vs configurable preview HUD, direct `file://` execution for `Compact`/`Balanced`/`Full`, exact background size estimates, strict executable output with rebased source maps, and all five target routes are verified. Completion still requires real Assets-folder packaging, runtime atlas binding, subsetting for unsupported font layouts, acceleration selection and runtime-module minification.

**DL-04 · Export image (`⌥⇧⌘S`)** — partial (`panels/export_image.rs`, `neoflash_export::image_export`, `File ▸ Export ▸ Export image…`):
- Working: 760 pt modal dialog and live floating panel with format tabs `PNG · SVG · PSD · AI · PDF`; preview `"Frame n of N · W × H"`; `Range` (`Current frame`, `Whole movie`, `Current scene`, `Frames A..=B`); linked `W × H` + `1x` / `2x` / `0.5x` scale presets; and `Background` (`Stage colour`, `Transparent`). PNG and SVG are direct exports. PSD writes a layered v1 document when supported. AI is an EPS-compatible PostScript subset and uses a raster fallback for unsupported scene features; PDF is currently raster-backed. Every destination file is written atomically.
- Pending: SVG image-link/CSS-class generation and PSD folder groups, smart objects, editable text and colour-space conversion. Their approved controls are present but do not yet alter exported content. Native Illustrator/PDF editability beyond the documented fallbacks also remains open.

**DL-05 · Export video** — done (`panels/export_video.rs`, `neoflash_export::video`, `File ▸ Export ▸ Export video…`, `Library ▸ Export symbol as video…`, CLI `--video`):
- Done: 840 pt modal dialog with format tabs `MP4 · WebM · MOV · GIF · PNG seq.`; live rendered stage preview thumbnail (`render_project_frame_rgba`) with cancellable background `VideoAsyncWorker` progress bar (`Exporting frames f / N`) and pause/resume (`Preview while rendering · ⏸ to pause`); `Range` (`Whole movie (all scenes)`, `Current scene`, `Frames From..To`); `Video` (`W × H` with aspect-lock toggle, `1x` / `2x` / `0.5x` scale, `FPS`, per-format `Codec: H.264 / H.265 / VP9 / AV1 / Apple ProRes / GIF 256-colour / PNG 8-bit`, `Quality` slider `0..=100` with live bitrate caption); `Content` (`Render nested movie clip animation and scripts (runtime capture)`, `Stop at last frame` vs `Stop after N s`, `Include audio` + `AAC 128/192/256 kbps` / `Opus 160 kbps` / `PCM 16-bit`, `Transparent bg (MOV/WebM/GIF/PNG seq.)`); and live footer estimate (`~ X.X MB · MM:SS · W×H`). Acceptance: 5 s MP4 at 24 fps has 120 frames matching the timeline.

**DL-06 · Export audio (`⌥⇧⌘A`)** — done (`panels/export_audio.rs`, `neoflash_export::audio`, `File ▸ Export ▸ Export audio…`, `Library ▸ Export audio…`, CLI `--audio`):
- Done: 640 pt modal dialog with format tabs `WAV · MP3 · OGG · AAC`; `Source` selector (`Timeline mix (all sound layers)`, individual sound layers `Layer: <name>`, or Library audio resources `Resource: <name>`) and `Range` selector (`Range: whole movie`, `Range: current scene`, `Range: frames A–B`); `Format` selectors (`8` / `11.025` / `22.05` / `44.1` / `48 kHz`, WAV bit depth `8-bit` / `16-bit` / `24-bit` / `32-bit float`, `Stereo` / `Mono`, compressed CBR bit rate `64`–`320 kbps CBR`); `Processing` controls (`Normalize to −1 dB`, `Fade out 0.0..=30.0 s` stepper, `Trim silence`); and live footer summary (`<source> · <duration> s · ~ <size>`).

**DL-07 · Import video** — partial (`panels/import_video.rs`, `File ▸ Import ▸ Import video…`):
- Working: the 720 pt modal dialog and live floating panel choose a real local MP4/WebM/Ogg/AVI resource, probe
  container duration (`mvhd` / RIFF AVI headers), provide interactive `SpanType::Video` In/Out trim handles synchronized
  with `MM:SS.FF` timecode fields, validate its container/codec diagnostics, set the timeline video span
  (`start_offset_ms` and format-12 `end_offset_ms`), optionally place a video element, extend the timeline and match
  the document FPS in one `"Import video"` Undo step. Identical media is reused and colliding resource paths are made
  unique. Read, format or project-validation failures leave both the draft and document unchanged. Embedded authored
  spans publish and play automatically in HTML5 (AVI transcodes to H.264 MP4 during publish). External spans stage a
  package-relative asset and Progressive spans retain a validated HTTP(S) URL; mixed deployment of one resource is
  deterministic. Native Stage playback and offline export decode Embedded and local External spans through the shared
  streaming pipe, with format-12 authored out-points and PR-09 montage/cue-point editing honored. Decoded CPU frames are
  currently retained, so bounded on-demand long-video decoding and native Progressive playback remain incomplete. Local
  External authoring keeps source bytes in the portable document so a later offline publish can stage them.

**DL-08 · Sound properties** — done (`panels/sound_properties.rs`, modal dialog): edits audio resource name, shows
source path and decoded format/duration/size stats, configures per-resource `Export` compression (`Compression: MP3 | Default | WAV (PCM) | Original`, `Bit rate: 64`–`320 kbps`, `Quality: Fast | Medium | Best`, `Convert stereo to mono`, and live `"Result: … — N % of original"` caption in `ResourceUnit.audio_export`, format 9), auditions the transcoded preview with `Test`/`Stop`, reloads from the recorded import path
via `Update`, or replaces bytes via `Import…` in one `"Sound properties"` Undo step (`Cancel`/`Esc` discards and stops
audition). P3.

| ID | Absent / promoted dialog / panel (reference §9) | Purpose | Depends on | Prio |
| --- | --- | --- | --- | --- |
| DL-09 | Edit envelope — **done** (`panels/edit_envelope.rs`) | Modal dialog with Left/right `EnvelopeLane`s (`1..=8` draggable stereo points in format-7 `FrameSound.envelope`, `SoundEffect::Custom`), presets, seconds/frames ruler, zoom, Play/Stop preview, one Undo | TL-07 audio model (done) | P3 |
| DL-10 | Import PSD to library — **done for the supported profile** | The bounded PSD/PSB reader and project conversion options are wired through editor and CLI routes; numbered sequences, PDF/AI, FXG and ACT/CLR/ASE palette imports use the same failure-atomic import workflow and report unsupported fidelity explicitly | broader format fidelity remains governed by importer diagnostics | P3 |
| DL-11 | Font embedding — **done for full resources; subsetting partial** (`panels/font_embedding.rs`) | Font list, character ranges, extra chars, export for AS + class, glyph count; stores the author-selected standalone TTF/OTF binary plus metadata after technical container validation. NeoFlash does not adjudicate OpenType licence flags because it cannot know the author's separate rights; lawful use remains the author's responsibility. HTML5 performs real TrueType glyph/table subsetting and conservatively keeps the complete face for dynamic/input or unsupported preservation cases; complete generated-class output remains partial. | `web-font-portability` export policy | P3 |
| DL-12 | Test Movie window | Native window: stage, transport, speed, fps/memory badge, Output drawer | Embedded runtime host (today Test Movie opens a browser) | P2 |
| DL-13 | Find and replace (⌘F) | Text / Font / Colour / Symbol / Sound / Video / Bitmap search, scope, regex, results table, Replace all | editor scan of `Project` (+ scripts); Replace all = one Undo | P2 |
| DL-14 | Movie Explorer | Tree document ▸ scene ▸ layer ▸ frame ▸ instance / actions, filters, path footer | editor traversal of `Project` | P2 |
| DL-15 | Convert to Symbol ▸ Advanced — **partial** | Library Properties edits the 9-slice rectangle and shared linkage; Stage paints guides; open documents can act as collision-safe donor libraries and synchronize shared symbols | direct guide dragging and a separate external-library file chooser remain | P3 |
| DL-16 | Layer properties — **done** (`panels/layer_properties.rs`) | Modal dialog editing Name, Visible, Locked, Type (`Normal`, `Guide`, `Guided` ↔ `motion_guide`, `Mask`, `Masked`, `Folder`; `Armature` stays disabled), `Mask mode` (`Geometry`, `Alpha`, `Luminance`) + `Invert mask` (`R-048`), Outline colour + outlines toggle, Layer height `Compact`/`Short`/`100 %`/`200 %`, `Blending: <mode>` (`Blending: Inherited (<mode>)` for folder-inherited modes, 6 groups of 14 modes, `R-040`), and `Sync layer to timeline` Switch (`R-042`) with one Undo | Guided = TL-01 (done); Armature = TO-05 | P2 |
| DL-17 | Accessibility panel (`panels/accessibility.rs`) | Missing: Make child objects accessible, Auto label, Shortcut | `Accessibility` lacks these fields (**NEW**) + HTML5 ARIA mapping | P3 |

**Layer and scripted mask coverage — done (DL-16, R-048, R-049).** Geometry remains the
legacy default. Format 9 adds explicit painted-alpha (`MaskMode::Alpha`) and BT.709 luminance (`MaskMode::Luminance`)
masks plus AE-style mask inversion (`LayerProperties.mask_invert`) with gradient transparency, decoded PNG alpha/luma,
compound holes, nested/mixed sources and moving/tweened transforms. Native stage, HTML5 Canvas 2D and Lottie export
(`"inv": true`) share the mask model; target layers composite before coverage so overlaps and nested masks retain the
correct alpha. AS2 `setMask` / AS3 `mask` preserve the existing source-plus-target `cacheAsBitmap` contract, including
authored gradient/PNG sources. The Mask command links the eligible sibling below in displayed order, with one Undo;
`Layer properties` (`R-048`), the layer context menu / `Folder and mask…` and the 2 inline AE-style 16×16 mask icons on
`LayerRole::Mask` timeline rows (`R-049`) switch `Geometry` / `Alpha` / `Luminance` and `Invert mask` in one Undo step.
See task card 22 in [the wiring plan](NF2-WIRING-PLAN.md#74-remaining-timeline-work-task-cards).

## 11. Menus (`panels/menu.rs`, `authoring_modify.rs::modify_menu_ui`, frame/layer context menus)

**MN-03 · View ▸ Rulers / Guides / Grid / Rulers, guides & grid…** — toggles, settings and snapping
are connected. Grid settings offer Dynamic (default) and Static modes: page-aware contrast and adaptive display
density, or fixed manual colour/spacing. Show and Snap are independent; hiding the grid does not disable snapping. The Snapping ▸ submenu awaits R-003.

**MN-01 · Main-menu coverage.** Disabled selection/history commands become available when their context exists;
that is distinct from an unimplemented command. Labelled previews do not execute their planned operation.

| Menu | Working behavior | Unavailable or partial |
| --- | --- | --- |
| File | New/Open tabs, Close/Close all, Save/Save as, existing recent files, Revert from a saved/imported source, supported PSD/PSB, sequence, PDF/AI, FXG and palette imports, validated Import video with Embedded/External/Progressive HTML5 deployment plus native Embedded/local External playback, HTML5/Video Publish, Export image…, Export video…, Export audio… (⌥⇧⌘A) and Export Lottie… (⌥⇧⌘L) | Print disabled; native Progressive video, cue editing and bounded on-demand long-video decoding remain partial; documented image/import fidelity limits remain; Publish settings has live HTML5 size and Video (MP4) controls |
| Edit | Undo/Redo, clipboard, Clear/Duplicate, Select all/Deselect, Find and Replace, timeline commands, symbol editing, Preferences, Keyboard shortcuts and Customize tools | Editing commands require the appropriate active document, selection and history |
| View | Zoom/magnification, Rulers, Show/Snap grid, Dynamic/Static grid, grid over objects, Show/Lock/Snap guides, settings popover, shape-hint visibility | Preview mode and Snapping submenus disabled; Pasteboard always shown |
| Insert | New symbol, Layer/Folder, Motion guide, frame/keyframe/blank keyframe, Classic/Shape/Motion tween, Scene | Frame/tween/guide commands depend on the current layer and selection |
| Modify | Convert to Symbol, Break Apart, shape hints, Combine objects, timeline operations, transforms/arrange/align, Group/Ungroup and Bitmap ▸ Trace bitmap | Other Bitmap operations and Symbol submenu remain unavailable; shape hints need a shape-tween endpoint |
| Control | Play/Stop, Rewind/End/step, browser Test movie/scene, Live ActionScript, Loop playback, Play all scenes, Mute sounds, Enable simple buttons | Native Test Movie host remains planned |
| Document | Document settings; open-document list and Next/Previous document | Requires an open document for settings and navigation |
| Window | `Panels` groups all real authoring panels including Brushes; built-in and named workspaces, Save/Update/Reset/Rename/Delete layouts, Hide panels, detach/dock, Appearance ▸ Dark / Light / Mid Neutral Gray / Dark Graphite | Duplicate window disabled; Debugger is connected; Follow system is not exposed; other planned panels retain their stated limits |
| Help | `Search functions…` (F1) opens an immediate focused fuzzy-search palette of registered menu commands, accepts the first keystroke after menu dismissal, supports ↑/↓/↵/Esc and runs the chosen command; NeoFlash Help & Reference keeps fixed bounds with independently scrolling chapters/article and can open a portable English text edition in the browser; that edition inherits the active theme and its single header control cycles all four NF2 themes; Open example; About NeoFlash | Keyboard shortcuts remains a separate Preferences page |

New/Open insert a tab after the active tab; opening an already open path activates it without replacing edits.
Each document keeps its selection, playhead, script state, Undo and recovery. Dirty Close/Close all/Quit prompts
in tab order; Cancel or failed Save stops on that document, preserving earlier deferred Discard decisions. Closing
the last tab shows New document/Open and disables document commands. Recovery copies open as independent tabs.
Named workspaces save layout only, not document-tab sessions; built-in layouts and up to 32 named presets preserve
panel visibility, active tabs, dock sizes and floating/native windows. Explicit Replace/Update, collision-safe
Rename, Reset and Delete preserve documents and unfinished edits. Deleting the active preset keeps its arrangement
as Unsaved layout. When an NF2 revision changes the canonical built-in composition, older live built-in layouts
refresh once to that composition; theme, Timeline/editor/tool/publish preferences and saved named layouts remain
intact. Tests: `document_sessions_tests`, document-strip `panel_tests`, `workspace_editor_tests`, recovery
and notification families.

Coverage: `panel_tests::menu_bar_toggles_live_state_without_changing_the_document` opens all nine menus,
checks explicitly unavailable rows, exercises View/Control/Window toggles in both directions, panel visibility,
Find opening and theme changes without editing the document or Undo history. `stage_view_tests` additionally
checks actual grid paint on the next idle frame and drawing/moving with snapping enabled and disabled.
These checks do not claim end-to-end acceptance of every context-dependent command.

**MN-02 · Context menus** (reference §8 Frame / Layer / Stage object)

| Menu | Still absent or disabled | Depends on |
| --- | --- | --- |
| Frame | Working: tweens, Ease…, Clear / Convert keyframes, Select all frames, Copy / Paste motion, Paste motion special, Reverse frames, Synchronise symbols | Appropriate frame/span selection |
| Layer | — (Add classic motion guide, Guide / Mask toggles, `Sync layer to timeline` and `Blending` submenu work; `Folder and mask…` also edits layer blend mode and timeline sync; `Window ▸ Panels ▸ Filters` edits layer `Blending` when no stage object is selected) | — |
| Stage object | Working under every active tool: Edit in place defaults to the exact selected instance with its affine parent context dimmed; Library/Explorer definition editing remains isolated. Nested sessions preserve each level's frame, selection, viewport, ruler zero and Timeline focus, and restore them on exit. Double-clicking genuinely empty canvas with no active draft/gesture consumes the complete gesture and leaves exactly one hierarchy level without changing the sticky tool or reopening the restored child; repeat the gesture to continue upward, while breadcrumbs provide direct deliberate navigation. Swap symbol…, Duplicate symbol…, Create motion tween, Show in library, Break apart (⌘B), Group (⌘G), Ungroup (⇧⌘G) and Distribute to layers ⇧⌘D work. Edit in new window stays disabled; Bitmap Trace awaits R-060. Non-affine 9-slice instances refuse in-place editing. Local guides/grid work for axis-preserving and quarter-turn mappings; signed/reflected ruler labels and arbitrary oblique local guide/grid/ruler chrome await R-063, with a safe screen-ruler fallback. | PR-06, LI-01, R-060, R-063 |
