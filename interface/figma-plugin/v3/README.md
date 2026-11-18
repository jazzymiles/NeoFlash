# NeoFlash v3 Figma builder

Builds the NF3 pages of the NeoFlash Figma file (`BUz3pseopiuOg2Uiy3Fog7`) from code: the density variable
collection, the atoms (component sets `NF3/<Atom>`), and every reference board (screens, panels, dialogs, menus).
The sources are plain Figma Plugin API scripts in `src/`, concatenated in file-name order.

## Design rules the builder encodes

1. **Density is a property of every atom.** Heights, paddings, gaps, radii, toggle sizes, label columns and default
   widths are bound to the `NF3 · Density` collection (modes Normal / Compact / Classic Flash). Colours are bound to
   `NF2 · Tokens` (Dark / Light / Mid · Neutral Gray / Dark · Graphite). Text uses the `NF2/*` styles and does not
   change with density. A board is therefore built once and shown in any of the 12 combinations by setting two
   explicit modes on a frame or page.
2. **Never use literal sizes for chrome.** Use density variables (`'size/control'`, `'pad/panel'`, `'gap/row'`,
   `'width/inspector'` …). Literals are allowed only for artwork (stage content, timeline frame width 12) and for
   a content width that genuinely does not scale.
3. **Nothing jumps.** A dialog has one size (`width/dialog-s|m|l|xl`, `height/dialog-l|xl`) for all its pages; rows
   never overflow their panel; a control keeps its label in every context.
4. **One form grid.** Inspector rows are two equal columns (`K.pair`) with the label inside the field; dialogs use a
   label column (`K.formRow`) and controls at their natural width (`width/num`, `width/field-s|m|l`), never stretched.
5. **One row per record** (colour, shortcut, layer, target): `size/control` or `size/row` high, actions at the end.
6. **Accent discipline.** One Primary button per surface; accent for selection, the active tool / tab, toggles that
   are on, focus. Options are checkboxes; `K.sw` (switch) only for an instantly applied application state.
7. **Never remove a feature.** Every control of the previous design keeps a place: on the surface, as an icon
   button with a tooltip, or in a ⋯ menu.

## Running

- As a Figma development plugin (no rate limits; rebuilds everything, about a minute or two): `./build.sh`, then in
  Figma Desktop `Plugins → Development → Import plugin from manifest…` → `manifest.json` and run **NeoFlash v3
  Builder**. `src/99-main.js` is the entry point and `src/90-pages.js` makes the eleven page copies; `code.js` is a
  build product.
- In this project the scripts are executed through the Figma MCP runner: each source file is stored as a TEXT node
  on the page `v3 · Builder source`, the nodes are concatenated in name order and run as
  `new Function('figma', src + '\nreturn (async () => {' + JOB + '\n})();')(figma)`.
- Offline smoke test (no Figma needed): `node test/run.js --job "<statements>" [extra files]`, see `test/`.
- `src/99-main.js` is never stored on the `v3 · Builder source` page (it would run on every job).
- Source rules: no backticks, no `${`, no backslashes (the files are sent inside `String.raw`); only standard
  Plugin API (no `figma.createAutoLayout`, `node.query`, `node.screenshot`).

## API (see `src/00-core.js` and `src/12-kit.js` for the exact code)

Core: `await nfInit()` first, `await nfFlush()` last (applies text styles). `NF.warn` collects warnings.

| Call | Meaning |
| --- | --- |
| `box(o, kids)`, `row(o, kids)`, `col(o, kids)` | Auto-layout frame. `o`: `name`, `w`, `h` (number, density variable name, `'fill'`, or omitted = hug), `gap`, `pad` (`n`, `[v, h]` or `[t, r, b, l]`; items may be variable names), `fill`, `stroke` (colour token such as `'bg/panel'`, or `'#RRGGBB'` for artwork), `sides: [t, r, b, l]` stroke weights, `radius`, `align` (cross axis `MIN`/`CENTER`/`MAX`; rows default to `CENTER`, columns to `MIN`), `justify` (main axis, also `SPACE_BETWEEN`), `clip`, `wrap`, `shadow: 'popover' \| 'dialog'`, `minH`, `minW`, `opacity`. |
| `txt(s, style, colour, o)` | Text. Styles: Heading, Title, Body, Body Strong, Small, Small Strong, Overline, Mono, Mono Small, Dialog Title, Dialog Body, Caption. `o.grow` fills the row and truncates; add `o.wrap` to wrap; `o.w` fixes the width and wraps. |
| `icon(name, colour, size)` | Instance of `NF2 Icon/<size>/<name>` (sizes 12, 16, 20; default 16). Unknown names give a warning and a placeholder. |
| `rect(w, h, fill, o)`, `dot(size, fill, o)`, `divider(vertical)`, `spacer()` | Primitives. `spacer()` pushes the following items to the far end of a row. |
| `markGrow(node, w, h)` | Makes a node fill its parent (applied when it is added to a parent through `box` / `add`). A filling child needs a parent with a fixed or filling size on that axis. |
| `nfMode(node, theme, density)` | Explicit modes, e.g. `nfMode(frame, 'Dark', 'Compact')`. |

Kit (`K.*`, all return nodes; controls fill their row unless `o.w` is given):
`K.btn(label, {kind: 'Secondary'|'Primary'|'Ghost'|'Danger', state, icon, w})`, `K.ibtn(icon, {kind: 'Ghost'|'Outlined', state: 'Default'|'Hover'|'Selected'|'Disabled', w})`,
`K.field(label, value, {unit, stepper, state, w})`, `K.num(label, value, unit, o)`, `K.select(value, {label, state, w})`, `K.search(placeholder, {w})`,
`K.check(label, on | 'mixed', {state})`, `K.radio(label, on)`, `K.sw(label, on)`, `K.slider({w})`, `K.seg(labels, selectedIndex, {w})`,
`K.tab(label, selected, icon)`, `K.section(title, open, {action: iconName})`, `K.dlgHeader(title)`,
`K.mi(label, {key, check, sub, state})` (menu item; `check: true|false` shows the check column), `K.msep()`,
`K.tree(label, {level: 0..2, chev: true|false|null, icon, meta, state})`, `K.badge(label, tone)`, `K.kbd(label)`, `K.tip(label, key)`,
`K.msg(text, 'Info'|'Warning'|'Error'|'Success')`, `K.swatch('#RRGGBB')`, `K.progress()`, `K.tool(name, state, flyout)`,
`K.t(text, colour, style)`, `K.small(text)`, `K.pair(a, b)` (two equal columns), `K.line([...])` (one row),
`K.body([...])` (padded section body), `K.formRow(label, [...])` (dialog label column + controls),
`K.panel(name, {tabs, sel, w, h}, kids)`, `K.dialog(title, {w, h}, body, footer)`, `K.menu(name, items)`,
`K.board(title, note, kids, {noteW})`.

Surface helpers (`S.*`, files `20`–`23`): `S.typeHeader(icon, kind, name)`, `S.group(caption)`,
`S.navRow(icon, label, selected, checked)`, `S.dialogBody([sidebar, page])`, `S.page([...])`, `S.side([...], wide)`,
`S.code([lines])`, `S.para(text)`, `S.kv(key, text)`, `S.densityTriple(() => [nodes])`, plus the finished surfaces.

Reference pages: `const page = await nfMaster()` (sets the current page, call once per job), then
`nfBoard(page, 'Panel / Name', note, [nodes], {noteW})` for every board and `nfArrange(page)` at the end. Board
titles start with `Screen / `, `Panel / `, `Dialog / `, `Menu / `, `Popover / ` or `Board / `.

## Gotchas

- New nodes are created on `figma.currentPage`; a job sets the target page before building.
- After an instance swap nested icon layers are renamed and keep stale fill overrides: use `K.ibtn`, `K.btn`,
  `K.tool`, which re-tint against the icon master; do not look nested icons up by name.
- A `wrap` row needs about 2 pt of slack to fit the expected number of columns.
- Component properties are addressed by their plain name; `inst()` warns about an unknown property and Figma throws
  on an unknown variant value.
