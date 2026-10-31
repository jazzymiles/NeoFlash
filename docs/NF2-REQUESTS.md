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

### R-001 · Motion path on the stage
- **Status:** open
- **Requested by / for:** Claude (implementation) · TL-05 Motion tween, PR-10
- **Where:** `crates/neoflash-editor/src/stage.rs` overlay for the selected motion-tweened instance
- **Need:** an NF2 painter for a motion tween path: polyline through the per-frame positions with frame dots, ◆ markers at property keyframes, selected/hover states, and a drag handle style for editing path points.
- **Why:** motion tweens work (independent tracks, ◆ keys) but the path is invisible on the stage; drawing it with hand-styled egui strokes is not allowed.
- **Checked:** design system reference §5 (Timeline) and §9 (Stage overlays), `exports/interface/*/png/editor-main.png`; no motion-path component in `neoflash-ui`.
- **Proposal (optional):** reuse `color/guide` for the line and the `TL Marker` ◆ glyph at keyframes.
- **Answer:**

## Resolved requests

_None yet._
