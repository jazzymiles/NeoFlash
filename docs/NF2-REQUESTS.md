# NF2 design requests

Implementation agents (Codex) must not edit the NF2 design system (`interface/`, `crates/neoflash-ui/`,
`docs/DESIGN-SYSTEM.md`) or change how a panel looks — see AGENTS.md › "Design system (NF2) — locked".
When a task needs something the design does not provide, add a request here and continue with the parts
that do not depend on it. The design track answers each request, updates Figma and `neoflash-ui`, re-pins
the design lock and marks the request as resolved.

## How to write a request

Copy the template, give it the next number, keep it short and concrete.

```markdown
### R-000 · <short title>
- **Status:** open
- **Requested by / for:** <agent> · <work item from docs/NF2-WIRING-PLAN.md>
- **Where:** <panel file and function, or screen / dialog name>
- **Need:** <missing component, variant, state, icon, text, layout, value or behaviour clarification>
- **Why:** <what the feature cannot do without it>
- **Checked:** <reference section, export PNG and outline file you looked at>
- **Proposal (optional):** <closest existing NF2 component or Figma variant>
- **Answer:** <filled in by the design track>
```

Status values: `open`, `answered` (design decision given, waiting for implementation in `neoflash-ui`),
`resolved` (available in the design system), `declined` (with reason).

## Open requests

### R-001 · Ruler pointer-position marker
- **Status:** open
- **Requested by / for:** Claude · W0.4 rulers (ST-01 "pointer position marker")
- **Where:** `stage_view.rs › stage_rulers`, `NF2/Stage Ruler`
- **Need:** a marker state of `NF2/Stage Ruler` showing the pointer's position on both rulers (colour, length).
- **Why:** spec ST-01 lists it; the ruler shelf has no marker variant.
- **Checked:** `docs-rulers-guides-grid.txt`, `editor-main.png` (no marker shown).
- **Proposal (optional):** 1 pt `color/guide` tick across the ruler.

### R-002 · Exact guide position (Move guide)
- **Status:** open
- **Requested by / for:** Claude · W1.2 guides (ST-02 "double-click → exact position")
- **Where:** guide double-click on the stage
- **Need:** a small popover or dialog with an axis-labelled position field and Remove / Cancel / OK.
- **Why:** dragging is limited to whole pixels at the current zoom; Flash offers a Move Guide dialog.
- **Checked:** reference §3, §6 "View settings", panels exports — no such dialog.

### R-003 · View ▸ Snapping submenu and object / pixel snapping
- **Status:** open
- **Requested by / for:** Claude · W1.3 snapping (ST-04, TO-06, MN-03)
- **Where:** `panels/menu.rs › view_menu` (Snapping is a disabled submenu), tools rail snap toggle
- **Need:** the Snapping submenu items (order, labels, shortcuts) and whether Snap to grid / guides repeat there;
  the tools-rail snap toggle placement.
- **Why:** guide and grid snapping work through Grid ▸ / Guides ▸; object and pixel snapping have no entry point.
- **Checked:** `panels-menus.txt` (Snapping row only, no submenu frame), §8.

### R-004 · Lock guides shortcut and View settings colour swatch
- **Status:** open
- **Requested by / for:** Claude · W1.2 guides
- **Where:** View ▸ Guides ▸ Lock guides; `NF2/View Settings` colour fields
- **Need:** confirm ⌥⌘; (spec ST-02; implemented, not shown on the menu row) and which picker the colour-field
  swatch opens (hex and alpha are editable today; the swatch also shows the alpha checkerboard where the reference
  draws the colour opaque).
- **Why:** the menu row shows no shortcut; clicking the swatch does nothing yet.
- **Checked:** `panels-menus.txt`, `view-settings.png`.

### R-006 · Paste motion special dialog
- **Status:** open
- **Requested by / for:** Claude · MN-02 frame menu (Copy / Paste motion)
- **Where:** frame menu ▸ Paste motion special… (`panels/timeline.rs › frame_context_menu`, disabled)
- **Need:** a dialog choosing which properties to paste (X/Y, scale, rotation and skew, colour, filters, blend,
  override target scale / rotation, ease) with OK / Cancel.
- **Why:** Copy motion and Paste motion work; the "special" variant has no dialog in the design.
- **Checked:** reference §8 (Frame menu), `panels-menus.txt`, `planned_dialogs.rs` — no Paste motion special dialog.
- **Proposal (optional):** `NF2/Dialog` with a column of `NF2/Checkbox` rows like the other planned dialogs.

### R-007 · Orient / Snap rows in the classic Tween section
- **Status:** open
- **Requested by / for:** Claude · TL-01, PR-10 (WIRING-PLAN §7.4 row 5)
- **Where:** `panels/properties.rs` frame Tween section for classic tweens on a guided layer
- **Need:** rows for Orient to path and Snap (to motion guide), as Flash's classic tween properties show them.
- **Why:** `Keyframe.orient_to_path` and `Editor::set_orient_to_path` work, but the classic Tween section has no slot.
- **Checked:** `panel-properties-frame` reference and the Motion tween section, which has these rows.
- **Proposal (optional):** reuse the Motion tween section's Orient checkbox row.
- **Answer:**

### R-008 · Bézier handles for motion path editing
- **Status:** open
- **Requested by / for:** Claude · TL-05 (WIRING-PLAN §7.4 row 9)
- **Where:** stage motion path overlay (`NF2/Motion Path`)
- **Need:** a curve-handle style (tangent line + handle dot, hover/drag states) to bend the path between ◆ keys.
- **Why:** paths are edited only at their ◆ points today.
- **Checked:** R-005 answer (◆ handles only), `canvas.rs`.
- **Answer:**

### R-009 · Shape hint markers
- **Status:** open
- **Requested by / for:** Claude · TL-04 (WIRING-PLAN §7.4 row 10)
- **Where:** stage overlay on shape-tween keyframes; View ▸ Show shape hints, Modify ▸ Shape ▸ Add shape hint
- **Need:** the lettered shape-hint marker (a–z) with unplaced / start (yellow) / end (green) states.
- **Why:** shape hints pin the morph correspondence; there is no marker component.
- **Checked:** design system reference, `canvas.rs`.
- **Answer:**

### R-010 · Sound span painter with real waveform peaks
- **Status:** open
- **Requested by / for:** Claude · TL-07, TL-13 (WIRING-PLAN §7.4 row 13)
- **Where:** `neoflash_ui::components::timeline::paint_span(SpanType::Sound)`
- **Need:** a variant that takes decoded peak values instead of the decorative waveform.
- **Why:** Sound spans should show the actual sound.
- **Checked:** `timeline.rs` span painter, `timeline-states-legend.png`.
- **Answer:**

### R-011 · Canvas chrome on artwork: rotated boxes, contrast, Light transformation point, cursor map
- **Status:** open
- **Requested by / for:** Antigravity · ST-07, ST-11
- **Where:** `neoflash_ui::components::canvas::{paint_selection_box, paint_transform_box, transformation_point}`,
  `neoflash_ui::cursors`; used by `crates/neoflash-editor/src/stage_selection.rs` and `stage_cursor.rs`
- **Need:** (1) a rotated/skewed (quad) variant of `Selection Box` and `Transform Box`; today rotated objects get the
  axis-aligned box of their bounds. (2) A contrast rule over artwork: the 1 pt `accent/default` line has no halo and
  disappears on blue or busy artwork (the old editor overlay used a halo pair). (3) The transformation point is
  `#FFFFFF` with a `bg/canvas` outline, nearly invisible on light artwork in the Light theme. (4) A per-handle cursor
  table (rotate zone size outside corners, skew vs scale hit areas); the editor follows Flash: handles scale, the edge
  between handles skews, up to 18 pt outside a corner rotates.
- **Why:** the boxes and cursors are now wired; these are the remaining visual decisions the editor must not improvise.
- **Checked:** `canvas-editing-chrome.png` (Dark only), `cursors.png`, design doc §3.5 and the canvas chrome catalogue.
- **Answer:**

### R-012 · Colour effect styles (Brightness / Tint / Advanced) and Properties Swap… symbol picker
- **Status:** open
- **Requested by / for:** Antigravity · W0.6 (PR-04), W0.15 (PR-06)
- **Where:** `crates/neoflash-editor/src/panels/properties.rs` (symbol instance inspector) and `stage.rs` object context menu (`Swap symbol…`, `Duplicate symbol…`)
- **Need:** (1) NF2 row composition in the Properties instance inspector for Colour effect `Style` (`None` / `Brightness` / `Tint` / `Advanced` / `Alpha`) and its per-style value rows (Tint colour + %, Advanced 8 multiplier/offset fields). (2) NF2 row/button placement in Properties for `Swap…` plus the Swap symbol picker dialog or popover (Library row/⋯ `Swap symbol` already works when an instance is selected, and `Duplicate symbol` works in the Library).
- **Why:** `Transform.color_transform` and `Editor::swap_instance_symbol` / `duplicate_symbol` exist, but `panels/properties.rs` currently contains only the Opacity row and no Swap… button or symbol-picker dialog preview; adding new rows or a picker dialog without a locked NF2 composition would violate the design lock.
- **Checked:** `panel-properties-movie-clip.png`, `panels/properties.rs`, `panels/planned_*.rs` (no Swap symbol dialog preview exists).
- **Answer:**

## Resolved requests

### R-005 · Motion path on the stage
- **Status:** resolved
- **Requested by / for:** Claude (implementation) · TL-05 Motion tween, PR-10
- **Where:** `crates/neoflash-editor/src/stage.rs` overlay for the selected motion-tweened instance
- **Need:** an NF2 painter for a motion tween path: polyline through the per-frame positions with frame dots, ◆ markers at property keyframes, selected/hover states, and a drag handle style for editing path points.
- **Why:** motion tweens work (independent tracks, ◆ keys) but the path is invisible on the stage; drawing it with hand-styled egui strokes is not allowed.
- **Checked:** design system reference §5 (Timeline) and §9 (Stage overlays), `exports/interface/*/png/editor-main.png`; no motion-path component in `neoflash-ui`.
- **Proposal (optional):** reuse `color/guide` for the line and the `TL Marker` ◆ glyph at keyframes.
- **Answer:** `neoflash_ui::components::canvas` gains `NF2/Motion Path`: `paint_motion_path` (1 pt `color/guide`
  polyline through the per-frame registration points, 1.5 pt `color/guide` dot per frame), `paint_motion_key` (the
  `TL Marker` ◆ glyph, 4.5 pt half-diagonal, `color/guide` fill with a 1 pt `bg/canvas` outline; Hover adds a
  `color/guide` ring 3 pt outside; Selected = dragging, fill `color/guide-selected`) and `motion_key_hit` (7 pt radius).
  The ◆ itself is the drag handle (Grab / Grabbing cursor); no separate handle style.
