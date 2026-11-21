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
   This is the plugin manifest. `exports/manifest.json` is the v2.25 export list, not a plugin manifest: importing
   it gives `Manifest error: Expected "manifest.name" to have type string`.
3. `Plugins → Development → NeoFlash Export` → **Export**.

The window has two parts. **v3 reference pages** (ticked by default) exports every board of the pages
`v3 · <Density> · <Theme>` that are ticked, found by page and board name, so rebuilding the Figma pages needs no
manifest edit. **v2.25 (archive)** (unticked by default) exports the archived pages listed in `exports/manifest.json`.

Leave **Icons & cursors** unticked (the default). `exports/icons/{12,16,20}` are built from the construction
sources in `tools/icon-masters/` (see "Icon masters" below) and checked by
`crates/neoflash-ui/tests/icon_masters.rs`; Figma's SVG export of the same components drops explicit stroke widths
and round caps and outlines some strokes. If the masters were overwritten, rebuild them with
`python3 tools/icon-masters/build.py`.

What it writes:

| Path | Content |
|---|---|
| `exports/v3/{normal,compact,classic-flash}/{light,dark,mid-neutral-gray,dark-graphite}/{png,svg}/` | v3: one file per board (`Screen / …`, `Panel / …`, `Dialog / …`, `Menu / …`, `Popover / …`), named after the board. Git-ignored. |
| `exports/v3/system/{png,svg}/` | v3: the concept boards and the `NF3 / …` atom sheets |
| `docs/data/outline-v3/*.txt` | v3: text outline of every board of `v3 · Normal · Light` (sizes are the Normal ones) |
| `docs/data/figma-dump-v3.json` | v3: the same dump as below, taken from the current file (includes `NF3 · Density` and the `NF3/…` atoms) |
| `exports/interface/{dark,light,mid-neutral-gray,dark-graphite}/{png,svg}/` | Editor screens, PNG @2x and SVG (text outlined) |
| `exports/panels/{dark,light,mid-neutral-gray,dark-graphite}/{png,svg}/` | One file per floating panel, dialog, menu, flyout, preferences page, timeline view board (boards marked `split` in the manifest are cut into their panels by layer name: `Panel / …`, `Dialog / …`, `Menu / …`, `Flyout / …`, `Window / …`, `Sample / …`, `Canvas — …`) |
| `exports/panels/{dark,light,mid-neutral-gray,dark-graphite}/boards/{png,svg}/` | The multi-panel boards themselves, as laid out in Figma |
| `docs/illustrations/` | Foundations and component shelves, audit, icons, used by the docs |
| `docs/data/figma-dump.json` | Every variable collection (all modes), text style, component (properties, variants, description), page and top-level frame |
| `docs/data/outline/*.txt` | Text outline of every canonical Dark frame: containers, auto layout, texts, instances with property values. Palette variants share the same structure and never overwrite these files. |
| `exports/export-report.json` | Count, failures, duration |

The rows below are the v2.25 archive export. Keep **v2.25 JSON dump + outlines** unticked: it overwrites
`docs/data/figma-dump.json`, which `crates/neoflash-ui/build.rs` reads and which must stay the v2.25 dump.

The list of v2.25 nodes is `exports/manifest.json`. After editing it run `python3 tools/export-plugin/build.py`
to regenerate `code.js` (the plugin cannot read local files, so the list is baked in).

`receiver.py` only accepts paths inside `design-system/` and only listens on 127.0.0.1.

## Icon masters

`exports/icons/{12,16,20}/*.svg` are generated. Never edit them by hand and never re-fit an icon point by point:
that is what bent the arrows and curves of the first optical masters.

```bash
python3 tools/icon-masters/build.py           # write exports/icons/{12,16,20}
python3 tools/icon-masters/build.py --check   # fail if the exports differ from the sources
python3 tools/icon-masters/audit.py exports/icons/16   # geometry audit: lopsided heads, near-miss symmetry, kinks
```

| File | Content |
|---|---|
| `kit.py` | Primitives and the grid rules. A coordinate is fitted as "axis + whole steps", so a shape symmetric at 16 px is symmetric at 12 and 20 px; arcs meet their lines exactly; `chevron`, `corner` and `arrow` have equal arms; `flip` draws an exact mirrored twin; `rotate` turns upright artwork; `native` holds a hand-fitted variant for one size. |
| `arrows.py`, `layout_text.py`, `objects.py`, `tools.py` | One short function per icon, in 16 px coordinates: stroke axes on n + 0.5, fill edges on n. An icon whose axis carries a stroke declares `g.axis(8.5, …)`. |
| `grid20.txt` | The icons that have a 20 px master. |

To add or change an icon: edit its function, run `build.py`, look at the three sizes enlarged over a pixel grid, run
`audit.py` and `cargo test -p neoflash-ui --test icon_masters`, then update the Figma components
`NF2 Icon/{12,16,20}/<name>` and re-pin the design lock. `figma_pack.py <dir> 13000 <name …>` packs the changed
icons with a checksum; its docstring holds the runner that replaces the vectors in place (component IDs and the
`color/text/secondary` binding stay), so every instance on the boards follows.

## Build the PDFs

```bash
tools/build-pdf.sh
```
Renders `docs/source/*.html` to `docs/pdf/*.pdf` with headless Google Chrome.
