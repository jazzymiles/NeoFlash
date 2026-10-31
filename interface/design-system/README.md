# NeoFlash design system

Design system, UI specification and exports of the NeoFlash animation editor (v2 redesign).
Source of truth: Figma file `BUz3pseopiuOg2Uiy3Fog7`.

| Start here | For |
|---|---|
| `docs/pdf/NeoFlash-Design-System-EN.pdf`, `docs/pdf/NeoFlash-Design-System-RU.pdf` | People: overview and full description with illustrations |
| `docs/NeoFlash-Design-System.md` | Claude: exhaustive reference, rules for editing the Figma file |
| `AGENTS.md` → `docs/NeoFlash-Design-System.codex.md` | Codex and other coding agents: implementation brief + reference |
| `tokens/` | `tokens.css`, `typography.css`, `tokens.json` for code |
| `exports/` | PNG @2x and SVG of every screen and panel, Dark and Light; icons and cursors |
| `site/` | Russian gallery pages `dark.html` and `light.html` (self-contained, relative links; `python3 tools/build-gallery.py`) |
| `tools/` | Export plugin, local receiver, doc and PDF build scripts (see `tools/README.md`) |

Rebuild after a design change:

```bash
python3 tools/receiver.py            # keep running, then run "NeoFlash Export" in Figma Desktop
python3 tools/build-docs.py && tools/build-pdf.sh
```
