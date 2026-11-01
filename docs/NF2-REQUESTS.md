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
