# Tools

## Export from Figma (PNG, SVG, JSON dump, outlines)

The Figma MCP server is rate-limited (Organization / Full seat: 200 read calls a day, 15 a minute),
so exports run as a local development plugin instead. It has no limits and produces clean frame
exports (no screenshot shadow padding).

1. Start the receiver (writes into `design-system/`):
   ```bash
   python3 tools/receiver.py
   ```
2. In Figma Desktop open the NeoFlash file, then
   `Plugins → Development → Import plugin from manifest…` → `tools/export-plugin/manifest.json`.
3. `Plugins → Development → NeoFlash Export` → **Export**. Takes about 1–3 minutes.

What it writes:

| Path | Content |
|---|---|
| `exports/interface/{dark,light}/{png,svg}/` | Editor screens, PNG @2x and SVG (text outlined) |
| `exports/panels/{dark,light}/{png,svg}/` | Floating panels, dialogs, menus, preferences, timeline views |
| `docs/illustrations/` | Foundations and component shelves, audit, icons, used by the docs |
| `docs/data/figma-dump.json` | Every variable collection (all modes), text style, component (properties, variants, description), page and top-level frame |
| `docs/data/outline/*.txt` | Text outline of every exported frame: containers, auto layout, texts, instances with property values |
| `exports/export-report.json` | Count, failures, duration |

The list of nodes is `exports/manifest.json`. After editing it run `python3 tools/export-plugin/build.py`
to regenerate `code.js` (the plugin cannot read local files, so the list is baked in).

`receiver.py` only accepts paths inside `design-system/` and only listens on 127.0.0.1.

## Build the PDFs

```bash
tools/build-pdf.sh
```
Renders `docs/source/*.html` to `docs/pdf/*.pdf` with headless Google Chrome.
