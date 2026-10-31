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

_None yet._

## Resolved requests

_None yet._
