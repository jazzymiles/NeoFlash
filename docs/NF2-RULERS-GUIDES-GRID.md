# Rulers, guides and grid (NF2 ST-01…ST-06)

Stage rulers, ruler guides, the stage grid, snapping and the **Rulers, guides & grid** popover, as the NF2
design shows them (`editor-main.png`, `editor-view-settings-open.png`, `panels/*/view-settings.png`; outlines
`docs-rulers-guides-grid.txt`, `panels-view-settings.txt`, `interface-editor-view-settings-open.txt`).

## What the user sees

| Element | Behaviour |
| --- | --- |
| **Rulers** (`NF2/Stage Ruler` 20 pt, `NF2/Ruler Corner`) | Top and left edges of the canvas, over the stage. Units: Pixels, Points (= px), Inches (72 px), Centimetres, Millimetres. Labels **Auto** pick the smallest step of 10 / 50 / 100 / 500 (×10ⁿ; 1 / 5 × 10ⁿ for other units) at least 50 pt apart, so labels never overlap; **Every N px** fixes the step. Zero is the scene's top-left corner or the edited symbol's registration point. Dragging the corner moves the zero point; double-clicking it resets. Vertical labels are rotated like the Figma ruler. |
| **Guides** (`NF2/Guide`) | Drag out of the top ruler (horizontal guide) or the left ruler (vertical guide). With Selection, Free Transform or Subselection: click a guide to select it (handle), drag to move, drop on a ruler or outside the canvas to delete, **Delete** removes the selected guide, **Esc** deselects / cancels a drag. Locked guides are dashed and do not react. Hidden guides do not react or snap. Positions are whole pixels (grid lines when snapping to grid). |
| **Grid** (`NF2/Stage Grid`) | Over the stage page, aligned to the timeline origin; every 5th line uses the grid colour (default `color/stage-grid` 14 %), lines between use 42 % of it. Below artwork, or above it with **Over objects**. Spacing ↔ / ↕ (1–1000 px, linkable). |
| **Snapping** (`NF2/Snap Ring`) | Moving objects snaps the selection's left / centre / right and top / centre / bottom; scale handles snap the dragged handle; rectangle and oval corners, line ends, pen anchors and Subselection point drags snap the point. Guides win over the grid; each axis snaps on its own; accuracy **Must be close / Normal / Can be distant** = 3 / 8 / 16 screen points at any zoom. The snap ring follows the snapping point while dragging and is larger when snapped. Rotation and freehand pencil/brush strokes do not snap. |
| **View settings** (`NF2/View Settings`, 320 × 542) | Popover from ⚙ in the zoom control (toggle), View ▸ Rulers, guides & grid…, View ▸ Grid ▸ Edit grid… (⌥⌘G) and View ▸ Guides ▸ Edit guides…. Every control applies immediately. **Clear guides** removes the current timeline's guides (one Undo). **Save as default** stores the settings in the editor preferences. Esc, ✕ or a click outside closes it. |

Entry points and shortcuts (all change the same state):

| Setting | View menu | Zoom control | Shortcut |
| --- | --- | --- | --- |
| Show rulers | View ▸ Rulers | Rulers | ⌥⇧⌘R |
| Show guides | View ▸ Guides ▸ Show guides | Guides | ⌘; |
| Lock guides | View ▸ Guides ▸ Lock guides | — | ⌥⌘; |
| Snap to guides | View ▸ Guides ▸ Snap to guides | Snap to guides | ⇧⌘; |
| Show grid | View ▸ Grid ▸ Show grid | Grid | ⌘' |
| Snap to grid | View ▸ Grid ▸ Snap to grid | — | ⇧⌘' |
| Grid over objects | View ▸ Grid ▸ Show over objects | — | — |
| View settings | View ▸ Rulers, guides & grid…, Grid ▸ Edit grid…, Guides ▸ Edit guides… | ⚙ | ⌥⌘G |

Shortcuts also match the physical key, because Option and Shift change the typed character on macOS.

## Data model

- **Guides are document data.** `neoflash_core::Timeline::guides: Vec<Guide { axis: GuideAxis, position: f64 }>`
  per timeline (scene or symbol), in timeline coordinates (stage pixels from the scene's top-left or the symbol's
  registration point). Serialized only when non-empty (`skip_serializing_if`), so older documents load with no
  guides and are not rewritten. A document that contains guides is format version **6**
  (`Guide::FORMAT_VERSION`); validation rejects guides in older versions, non-finite positions, |position| >
  1 000 000 px (`Guide::MAX_POSITION`) and more than 1000 guides per timeline (`Guide::LIMIT`). Clipboard payloads
  with symbol guides infer version 6.
- **Guides never reach HTML5.** Evaluation ignores them; `neoflash_export::export_project` serializes a copy with
  every timeline's guides cleared (`publishable`).
- **View settings are editor state**, not document data: `stage_view::ViewPrefs` (rulers, units, labels, guide
  show / snap / lock / colour / accuracy, grid show / snap / over objects / spacing / link / colour / accuracy).
  The session starts from `WorkspacePrefs::view_defaults` (written by **Save as default**, persisted with the
  workspace preferences) or the NF2 defaults: rulers, guides and grid shown, snap to guides on, snap to grid off,
  20 px linked grid, Auto labels, Normal accuracy. Colours are `None` while they equal the theme token, so default
  guide and grid colours follow Light / Dark. The ruler zero point and the selected guide are session state.
- **Undo:** adding a guide, a whole guide move (from press to release), deleting a guide and Clear guides are one
  Undo step each. Toggles and popover edits are not document changes and are not undoable.

## File map

| File | Responsibility |
| --- | --- |
| `crates/neoflash-core/src/model.rs` | `Guide`, `GuideAxis`, `Timeline::guides`, limits, format version. |
| `crates/neoflash-core/src/validation.rs`, `clipboard.rs` | Version 6 validation; clipboard version inference. |
| `crates/neoflash-export/src/lib.rs` | `publishable`: strip guides before bundling. |
| `crates/neoflash-ui/src/components/canvas.rs` | Painters (design owner): `paint_stage_ruler(_units)`, `ruler_step`, `ruler_step_units`, `ruler_label`, `RulerLayout`, `paint_ruler_corner`, `paint_guide`, `guide_hit`, `guide_extent`, `GUIDE_HIT`, `paint_stage_grid`, `paint_snap_ring`. |
| `crates/neoflash-ui/src/components/view_settings.rs` | `ViewSettings` body and `popover` (placement `POPOVER_OFFSET` from the ⚙ button, `POPOVER_SIZE`), `UNITS`, `SNAP_ACCURACY`, `default_grid_color`. |
| `crates/neoflash-editor/src/stage_view.rs` | `ViewPrefs`, `RulerUnits`, `SnapAccuracy`, `StageView` session state; guide hit-testing and drags, ruler / corner interaction, grid / guide / snap-ring painting, snapping helpers (`snapper`, `snap_drag`, `snap_screen`, `snap_move`), commands and shortcuts. |
| `crates/neoflash-editor/src/snapping.rs` | Pure snapping math (`Snapper`): per-axis, guides before grid, tolerance in stage units. |
| `crates/neoflash-editor/src/panels/view_settings.rs` | The popover panel: binds `ViewPrefs` ↔ `ViewSettingsState`, Clear guides, Save as default. |
| `crates/neoflash-editor/src/stage.rs` | Order per frame: guide input → grid (below) → content → grid (over) → guides → tools → snap ring → rulers → chrome. |
| `crates/neoflash-editor/src/stage_selection.rs`, `authoring_tools.rs`, `vector.rs` | Snapping call sites: move / scale / resize, rectangle / oval, line, pen anchors, path point drags. |
| `crates/neoflash-editor/src/shell.rs` | Zoom control toggles (`Some(state)`), ⚙ anchor, breadcrumb and size badge offset by the ruler. |
| `crates/neoflash-editor/src/panels/menu.rs` | View ▸ Rulers, Grid ▸ …, Guides ▸ …, Rulers, guides & grid…. |

## Invariants

1. Guides are stored in timeline coordinates; screen = timeline origin + position × zoom. The ruler zero point
   only changes ruler labels, never stored positions.
2. Guide hit-testing and snap tolerances are in **screen points** (`GUIDE_HIT` 4.5, accuracy 3 / 8 / 16), converted
   by zoom, so behaviour is identical at every zoom.
3. Rulers are an interactable egui layer over the canvas; every stage tool already ignores presses on other layers.
   A press on a guide is removed from the frame's events before the tools read them, and
   `StageView::guide_pointer` suppresses the stage's click / double-click handlers for that press.
4. One Undo step per guide add / move / delete / clear; a guide drag that did not move commits nothing.
5. Guides never appear in evaluation or HTML5 output; a document without guides serializes exactly as before.
6. Toggles in the View menu, the zoom control, shortcuts and the popover read and write the same `ViewPrefs`
   fields; there is no second copy.
7. Snapping only adjusts the dragged / drawn points of the current gesture; untouched objects never change.
8. The look comes only from `neoflash-ui` painters and components; the editor passes positions, states and
   colours, never sizes or hand-styled widgets.

## Tests that protect it

| Test | Covers |
| --- | --- |
| `neoflash-core/tests/core.rs` `ruler_guides_round_trip_and_migrate_older_documents`, `ruler_guides_are_validated` | Serialization, version-5 migration, validation limits. |
| `neoflash-export` `export_strips_authoring_guides` | No `guides` in the HTML5 bundle; source project untouched. |
| `neoflash-ui` `canvas::tests` | Auto label steps at every zoom, unit steps and labels, ruler layout, guide hit band. |
| `neoflash-editor` `snapping::tests` | Grid / guide snapping, guide priority, zoom-independent tolerance, moving bounds, degenerate grids. |
| `neoflash-editor` `stage_view_tests` | Ruler drag-out (both axes), move and drop-back delete as single Undo steps; locked / hidden guides; per-timeline guides and save/open; every shortcut; Delete on a selected guide; Clear guides Undo; popover state round trip; Save as default persistence; snapping while drawing and moving; zoom-control toggles, ⚙ / Close and View ▸ Rulers through real input. |

Visual check: `NEOFLASH_WINDOW=1920x1080 NEOFLASH_SCREENSHOT=target/ui-shots/x.png NEOFLASH_SCREENSHOT_THEME=dark|light
[NEOFLASH_SCREENSHOT_VIEW_SETTINGS=1] target/debug/neoflash-editor <document with guides>` against
`editor-main.png` / `editor-view-settings-open.png` (build first: `cargo build -p neoflash-editor`).

## Do not change (Codex and other implementation agents)

- Do not move guides, grid or view settings into another structure, rename `Timeline::guides`, change its
  serialization (`skip_serializing_if`, snake_case axis) or the version-6 rule. A new field in `ViewPrefs` needs
  `#[serde(default)]`-compatible defaults.
- Do not remove the `publishable` strip from export or let evaluation read guides.
- Do not paint rulers, guides, grid, snap ring or the popover in the editor by hand, change their sizes, colours,
  offsets or the popover layout; these are `neoflash-ui` (locked) — file a request in `docs/NF2-REQUESTS.md`.
- Do not make stage tools read pointer presses without the existing layer check, and do not bypass
  `StageView::guide_pointer` in stage click handlers; guides and rulers rely on both.
- Do not add a second state for any toggle; menus, zoom control, shortcuts and popover must stay in sync.
- Do not change snapping tolerances to stage units or snap untouched objects.
- Keep `stage_view_tests.rs`, `snapping.rs` tests and the core / export / ui tests above passing; extend them for
  new behaviour instead of loosening them.

## Known limits

- The ruler pointer-position marker, a double-click **Move guide** field for exact positions, the View ▸ Snapping
  submenu contents, snapping to objects / pixels, the colour picker behind the colour-field swatch and the ⌥⌘;
  label on View ▸ Guides ▸ Lock guides are not designed yet (`docs/NF2-REQUESTS.md` R-001…R-004).
- Guide lock and visibility are editor settings for all timelines, not stored per document as in Flash.
