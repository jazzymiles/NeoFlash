# NeoFlash Design System (NF2): complete reference

<!--IF:claude-->
> Audience: Claude (and any engineer or designer) who has to **extend, edit, implement or audit** the NeoFlash
> interface. This file is exhaustive on purpose. The short human version is the PDF in `pdf/`.
> A Codex-oriented variant of the same content is `NeoFlash-Design-System.codex.md` (+ `../AGENTS.md`).
<!--END-->
<!--IF:codex-->
> Audience: OpenAI Codex (and any coding agent) that has to **implement NeoFlash's UI in code** or keep code in sync
> with the design. Start with `../AGENTS.md` (short brief); this file is the exhaustive reference.
> Use the generated tokens in `../tokens/` instead of copying hex values. The human version is the PDF in `pdf/`.
<!--END-->
> Generated parts (tokens, styles, components, icons, cursors, exports, outlines) are rebuilt from the Figma file
> by `tools/build-docs.py`. Do not hand-edit this file; edit `docs/source/NeoFlash-Design-System.src.md` and rebuild.

Generated: {{GENERATED}} · Figma file: `{{FILE_NAME}}` (key `BUz3pseopiuOg2Uiy3Fog7`).

---

<!--IF:claude-->
## 0. TL;DR for Claude
<!--END-->
<!--IF:codex-->
## 0. TL;DR for Codex

- **Implementing UI?** Read §3 (foundations), §4–§9 (layout, timeline, panels, dialogs), §10 (component API) and
  §13 (implementation guide). Colours, spacing, radii and type come from `../tokens/tokens.css` / `tokens.json`;
  never hard-code values. Pixel references: `../exports/{interface,panels}/{dark,light}/{png,svg}/`.
- **Changing the design itself** happens in Figma, not in this repo. If you have the Figma MCP server, follow §11;
  otherwise describe the change and leave the Figma edit to a designer.
<!--END-->

- **What it is.** NeoFlash is a desktop vector-animation editor in the tradition of Macromedia / Adobe Flash:
  stage + timeline + layers + nested symbols (movie clip / graphic / button) + ActionScript-style scripting,
  publishing to HTML5 (Rust / WebAssembly runtime), video, images and audio.
- **Two designs live in one Figma file.** `v1` is a faithful vector reconstruction of the current app
  (NF1 components, `NF1 · Tokens`). `v2` is the redesign (NF2 components, `NF2 · Tokens`, Dark + Light modes).
  **They never share a variable, style or component.** Editing one must not change the other.
- **Source of truth is the Figma file**, edited through the Figma MCP `use_figma` tool (Plugin API).
  The old generator plugin in `figma-plugin/` is history; never re-run it on this file.
- **Bind every v2 colour/spacing/radius to `NF2 · Tokens`** and every v2 text to an `NF2/*` text style.
  Never hard-code a hex in v2 except translucent tints (see §11 gotchas).
- **Red is reserved for errors** (and the playhead / frame labels, which are Flash conventions). Breakpoints are orange,
  warnings amber, strings green, tags blue.
- **Timeline geometry:** 12 px per frame (Normal view), rows 28 px, ruler 32 px, frame `n` starts at `x = (n − 1) × 12`.
- **Icons:** monochrome, 16 px (UI) and 20 px (tools), components `NF2 Icon/16/*` and `NF2 Icon/20/*`, recoloured by token.
- **Exports and data** live next to this file: `../exports/` (PNG @2x + SVG, Dark and Light), `data/figma-dump.json`,
  `data/outline/*.txt` (text outline of every exported frame). Re-export with `tools/export-plugin` (no MCP limits).

---

## 1. Product and principles

NeoFlash keeps the mental model Flash animators already have, and modernises the chrome around it.

| Principle | What it means in the UI |
|---|---|
| One primary path | Every screen has one obvious primary action (Publish, Play, Save, Apply). Everything else is secondary and never competes in weight. |
| Group by task | Transport, onion skin, zoom, layer actions are grouped controls, not scattered rows. |
| Surfaces, not borders | Depth comes from background levels (`bg/app → panel → raised → field`); borders are separators only. |
| Systematic | Own variables (colour, spacing, radius), text styles and components; two themes on one collection. |
| Accessible | Text contrast ≥ 4.5:1, focus ring 2 px, hit areas ≥ 32 px, every tool tooltip shows its shortcut. |
| Isolated from v1 | No v2 variable, style or component references v1. |
| Flash-literate | Layer kinds, frame states, keyframe glyphs, symbol types, menus and shortcuts follow Flash semantics so that a Flash user is instantly at home. |

### 1.1 Audit v1 → v2 (summary)

The audit board (`Audit · v1 → v2`) lists 14 findings: 5 High, 7 Medium, 2 Low. Key numbers: disabled text contrast
in v1 is 2.16:1 (norm 4.5:1); 20 tool buttons had no labels or tooltips.

| # | Area | Severity | v1 problem | v2 solution |
|---|---|---|---|---|
| 1 | Action hierarchy | High | "Test Movie" is a small link; "Publish HTML5…" hidden at the bottom of Properties. No primary action. | Top bar: Test Movie (secondary) + **Publish** (primary, accent). Project title + unsaved badge centred. |
| 2 | Timeline controls | High | Scene/FPS/duration on top, Play and onion skin at the bottom, zoom on a third row. Transport next to add/delete layer. | One control bar above the grid: transport + timecode + scene / FPS / duration + frame tools + onion + zoom on the right. |
| 3 | Layer column | Med | Three unlabelled header icons; 60 % empty; add/delete detached from the list. | Add/delete in the layer column; 28 px rows with colour dot; lock/eye with tooltips. |
| 4 | Timeline bottom row | Med | Onion skin checkboxes, fields and slider in one ungrouped line. | Onion skin is one toggle + range handles on the ruler; options in a popover and Preferences ▸ Timeline. |
| 5 | Tools palette | High | 20 icons, no labels, no shortcuts, grey group captions. | 56 px rail, groups split by dividers, accent active tool, tooltip "Name  key", flyouts for tool groups. |
| 6 | Typography & contrast | High | 14 px base, 11–12 px secondary, disabled 2.16:1. | Scale 11/12/13/14/16/18/20, ≥ 12 px for running text, all text tokens ≥ 4.5:1 (except disabled). |
| 7 | Surfaces & borders | Med | Every control outlined 1 px #6D7690; no depth. | Surface levels; borders only as separators; radii 4/6/8/12 from tokens. |
| 8 | Property fields | Med | FPS/Frames/Alpha look like buttons; no units, steppers or scrub. | One Field: inline prefix label, units in value, hover/focus/disabled, linked W × H. |
| 9 | Library | Med | Search + Clear + Sort + A–Z take two rows; 500 px empty; no empty state. | Search with clear, sort in header menu, footer actions, preview and empty state. |
| 10 | Bottom dock | Low | Actions/Accessibility/Output as a permanent 186 px panel showing one line. | One dock with tabs: Timeline · Actions · Output · Accessibility. |
| 11 | Unsaved changes dialog | High | Save/Discard/Cancel equal weight; two different dialog title styles. | One Dialog: Save primary, Discard danger-ghost left, Cancel secondary, consequence text, Esc/Enter. |
| 12 | Convert to Symbol dialog | Med | Eight fields in one column, linkage expanded, disabled fields look active. | Name / Type / Folder first, compact registration picker, linkage under Advanced, Convert primary. |
| 13 | Status line | Low | Hints and shortcuts in one 12 px grey line; broken glyph "⌘□". | 24 px status bar: state left, shortcut chips (F5/F6/F7/⌘↵) right. |
| 14 | Accessibility & themes | Med | Selection by colour + 1 px border, weak focus, 24–28 px targets, no dark theme. | 2 px focus ring, ≥ 32 px targets, Dark/Light on one variable collection. |

![Audit board](illustrations/audit-v1-to-v2.png)

### 1.2 Functional coverage

The board `Functional coverage — v2` checks v2 against what a Flash-style editor needs (drawing, nested symbols,
timeline animation, scripting, import / export of graphics, video and sound): **33 of 33 covered**. The table in §9
maps every capability to the frame that shows it.

---

## 2. Figma file map

{{GEN:pages}}

Page roles:

- **v1 · Components / v1 · Interface** — reconstruction of the shipped app. Touch only when asked.
- **Audit · v1 → v2** — findings, annotated v1 map, principles, functional coverage table.
- **v2 · System & Components** — tokens boards (Colors Dark/Light, Type scale, Spacing and radius) and component
  *shelves* (`Shelf / …` frames). Every NF2 component lives here.
- **Icons** — every NF2 / NF1 icon grouped with "where used", the Cursors section and the list of unused icons.
  Regenerate it after adding icons (see §11).
- **v2 · Panels** — every floating panel, dialog, menu, preferences page and timeline view mode, in a Dark board and a
  Light twin board (`Panels — Light`, `Menus — Light`, `Preferences — Light`, `Timeline frame views — Light`).
- **v2 · Interface — Dark / Light** — full 1920 × 1080 editor screens in each state. The Light page holds clones of the
  Dark frames with the NF2 collection set to Light mode on the frame. **After changing a Dark screen, re-clone it to
  Light** (delete the old twin, clone, set explicit mode Light).

---

## 3. Foundations

### 3.1 Colour tokens (`NF2 · Tokens`, modes Dark / Light)

All colours are variables in one collection with two modes. Themes switch by setting the explicit mode of a frame
(`frame.setExplicitVariableModeForCollection(collection, lightModeId)`); nothing else changes.

**Semantic rules**

- Surfaces stack: `bg/app` (window) → `bg/panel` (docked panels, inspector) → `bg/raised` (popovers, cards, menus,
  floating panels) → `bg/field` (inputs). Hover = `bg/hover`, selected row = `bg/selected`.
- `bg/canvas` is the pasteboard around the stage; `bg/stage` is the *document* background (user content, so it is the
  same in both modes).
- Borders: `border/subtle` separators inside panels, `border/default` control outlines where needed,
  `border/strong` emphasised outlines, `border/focus` focus ring (2 px).
- Text: `text/primary` content, `text/secondary` labels, `text/tertiary` hints and metadata, `text/disabled`,
  `text/on-accent` on accent fills, `text/inverse` on the tooltip.
- `accent/*` is the only interactive colour (primary buttons, selection, active tool, toggles on, focus).
- `danger/*` = destructive actions and errors. `warning/*` = warnings, unsaved badge. `success/default` = OK states.
- `brand/orange` = logo, breakpoints family. `layer/*` = layer colour dots (outline colour of a layer).
- `timeline/*` = everything inside the timeline grid (see §5). `syntax/*` = code editor (see §7).
- `guide`, `guide-selected`, `stage-grid` = canvas aids. `overlay` = modal scrim (has alpha). `tooltip/bg`.

{{GEN:colors}}

![Colours — Dark](illustrations/colors-dark.png)
![Colours — Light](illustrations/colors-light.png)

### 3.2 Spacing and radius tokens

{{GEN:dimensions}}

Usage:

- Panel padding 12 (`space/3`) horizontally, 12 between groups; field rows gap 8; dialog padding 24 (`space/5`).
- Control heights: 32 (fields, selects, buttons, icon buttons SM), 28 (tree rows, menu items, layer rows,
  breadcrumb), 40 (dock tabs, primary play button MD), 36 (panel tabs, floating panel header), 24 (status bar).
- Radii: `radius/sm` 4 (chips, swatches, kbd), `radius/md` 6 (fields, buttons, menu items), `radius/lg` 8
  (cards, popovers, panels), `radius/xl` 12 (dialogs, floating panels), `radius/full` pills / switches.

![Spacing and radius](illustrations/spacing-and-radius.png)

### 3.3 Typography

Fonts: **Inter** (UI) and **Roboto Mono** (code, timecode, numbers in rulers, hex values). Minimum running-text size is
12 px; 11 px only for overlines and mono metadata.

{{GEN:type}}

Usage:

- `NF2/Heading` page/board titles, `NF2/Title` panel titles and section titles in big panels,
  `NF2/Dialog Title` + `NF2/Dialog Body` in dialogs.
- `NF2/Body` default UI text (field values, menu items, tree rows). `NF2/Body Strong` labels that need weight
  (selected tab, project title).
- `NF2/Small` secondary text, descriptions, hints; `NF2/Small Strong` field prefixes and badges.
- `NF2/Overline` section headers inside panels (uppercase, +6 % tracking).
- `NF2/Mono` code editor, timecode; `NF2/Mono Small` ruler numbers, hex, IDs, file paths.

![Type scale](illustrations/type-scale.png)

### 3.4 Iconography

- Two grids: **16 px** for UI (panels, menus, timeline, dialogs) and **20 px** for tools (Tools panel, colour
  controls). Stroke icons (≈1.3 px on the 16 grid, 1.5 px on the 20 grid, round caps), no colour of their own:
  the instance is recoloured by token
  (`text/secondary` default, `text/primary` hover, `accent/default` active, `text/on-accent` on primary buttons).
- Names: `NF2 Icon/16/<name>`, `NF2 Icon/20/<name>`; tools are `NF2 Icon/20/tool-<name>`, colour controls
  `NF2 Icon/20/color-<name>`.
- The **Icons** page lists every icon with where it is used, grouped like the table below. Unused icons are listed
  as cleanup candidates (kept on purpose for upcoming features).

{{GEN:icons}}

![Icons — tools section](illustrations/icons-page-tools-section.png)

### 3.5 Cursors

39 cursor components `NF2 Cursor/<name>`, 24 × 24, black glyph with white outline (readable on any stage colour).
The hotspot is stored in each component's description (`Hotspot at x N, y N`) and must be used when implementing.

{{GEN:cursors}}

![Cursors](illustrations/icons-page-cursors-section.png)

### 3.6 Elevation, focus and states

- Floating panels and popovers: `bg/raised`, radius 12, 1 px `border/subtle`, drop shadow **y 12, blur 32,
  black 35 %** (same in both themes). Dialogs: drop shadow **y 16, blur 48, black 45 %**, on the `overlay` scrim.
  (Values read from the SVG exports.)
- Focus: 2 px `border/focus` ring, 2 px outside the control radius (`NF2/Focus Ring`). Shown only for keyboard focus.
- Hover: background `bg/hover` (ghost controls) or `accent/hover` (primary). Pressed / on = the `Selected` variant.
- Disabled: `State=Disabled` variants use `text/disabled` and lose hover; never rely on colour alone.
- Selection: `bg/selected` fill + `accent/default` text/icon; on the timeline `timeline/selected`.
- Tooltips: `NF2/Tooltip`, `tooltip/bg` with `text/inverse`, label + `NF2/Kbd` shortcut (e.g. "Selection  V").

---

## 4. Main window

![Editor — Dark](../exports/interface/dark/png/editor-main.png)

Frame `NF2 / Editor — Main`, 1920 × 1080. Regions (top to bottom, left to right):

| Region | Size | Content |
|---|---|---|
| Top bar | 1920 × 48 | `NF2/Logo` + "NeoFlash"; menu bar File · Edit · View · Modify · Insert · Control · Window (ghost buttons, open as dropdowns, §8); centred project title "Ideas in motion" + `Badge` "Unsaved" (warning); right: workspace `Select` "Classic workspace", **Test Movie** (secondary button with play icon), **Publish** (primary button with upload icon). |
| Tools rail | 56 × 588 | `NF2/Tools Panel` Layout=1 Column. Tool groups split by dividers, flyout corner on grouped tools, fill/stroke chip, swap/default colours, snap toggle at the bottom. |
| Canvas | 1544 × 588 | Pasteboard `bg/canvas`; stage 960 × 600 shown at 75 % (720 × 452) with `NF2/Stage Grid`; horizontal + vertical `NF2/Stage Ruler` (20 px) and `NF2/Ruler Corner`; `NF2/Guide` lines; breadcrumb (floating, top-left, 34 px); stage-size badge (top-right); `NF2/Zoom Control` floating bottom-centre (hand, −, 75 %, +, fit, rulers, guides, grid, snap to guides, view settings ⚙). |
| Inspector | 320 × 588 | **Properties** (`Panel Header`, `Segmented Control` Selection / Tool; sections Stage: Name, W × H with link, Background swatch + hex + α; Playback: FPS, Frames; Publish: target badge "HTML5 · Rust / WebAssembly" + "Publish settings…") and **Library** (search, tree). Content changes with the selection (see §6 Properties variants). |
| Bottom dock | 1920 × 420 | Dock tabs (40): Timeline · Actions · Output · Accessibility, collapse ⌄ and more ⋯. Timeline controls (44). Timeline body (336). |
| Status bar | 1920 × 24 | "Ready" + document summary "Stage 960 × 600 · 24 fps · 120 frames" left; shortcut hints right: `F5` frame, `F6` keyframe, `F7` blank keyframe, `⌘↵` Test Movie. |

**Timeline controls bar** (left → right, gap 8): layer actions (new layer, new folder, add motion guide, spacer, delete)
· transport (first, step back, **play** = primary icon button MD 40 px, step forward, last) · loop toggle (Selected
style when on) · timecode `00:01.21` + `30 / 120 F` (mono) · Scene `Select` · `Field` FPS · `Field` Duration ·
`Icon Group` frame insert (frame F5, keyframe F6, blank keyframe F7) · `Icon Group` tweens (classic, shape, motion) ·
ease · sound · onion group (onion skin on/off, outlines, edit multiple frames, modify markers) · centre playhead ·
frame view ⋯ · zoom − slider +.

### 4.1 Editor states (all exported in Dark and Light)

| Frame | Shows |
|---|---|
| Editor — Main | Default state, guides and grid visible, tooltip on the Selection tool. |
| Editor — Actions tab | Bottom dock switched to the docked Actions editor. |
| Editor — Unsaved changes dialog | Modal on `overlay`: Save (primary), Discard (danger ghost, left), Cancel. |
| Editor — Convert to Symbol dialog | F8 dialog over the stage. |
| Timeline — states legend | Legend of every layer type, column toggle and frame state (§5). |
| Editor — View settings open | Rulers / guides / grid popover opened from the ⚙ in the zoom control. |
| Editor — Swatches open | Quick colour popover opened from the fill chip. |
| Editor — Ease panel open | Floating Ease panel over the canvas for the selected classic tween. |
| Editor — Edit menu open | Main menu dropdown with the Timeline submenu. |
| Editor — Editing symbol in place | Breadcrumb Scene 1 ▸ orbit ▸ planet, faded parent content, symbols & scenes popover for large projects (search, sections Scenes · Matches · Recent · Browse, keyboard hint). |
| Editor — Video on timeline | Video layer `intro.mp4` with filmstrip span, video on stage, Properties — Video floating. |
| Editor — Motion tween path | Motion path on stage with frame dots, property keyframes ◆ on the hand layer. |

{{GEN:interface-gallery}}

---

## 5. Timeline (Flash semantics)

![Timeline legend](../exports/interface/dark/png/timeline-states-legend.png)

### 5.1 Geometry

| Item | Value |
|---|---|
| Frame width | 12 px (Normal). View modes: Tiny 6, Small 9, Normal 12, Medium 16, Large 22 px/frame. |
| Row height | 28 px (Short mode: 18 px). |
| Ruler | `NF2/TL Ruler`, 32 px: seconds row on top (`0s 1s 2s …` at 24 fps), frame numbers row below (1, 5, 10, …) with ticks. |
| Layer column | 280 px (`NF2/TL Layers Header` + `NF2/TL Layer` rows). |
| Grid | `NF2/TL Grid` 1440 px = 120 frames; every 5th frame shaded (`timeline/empty-5th`); `NF2/TL Grid Lines` overlay keeps filled frames countable. |
| Marker position | Frame `n` → `x = (n − 1) × 12`; spans resize to `frames × 12`. |

### 5.2 Layers

`NF2/TL Layer` variants `Type` = Layer · Masked · Folder Open · Folder Closed · Guide · Motion Guide · Mask · Sound ·
Video · Armature, `Selected` = True/False; booleans `Indent 1`, `Indent 2` (nesting under folders, guides, masks),
`Editing` (pencil = current layer); text `Label`.

| Type | Icon | Rule |
|---|---|---|
| Layer | page | Normal layer. |
| Folder Open / Closed | folder (brand orange) + chevron | Children indent 1 level; collapsed folder row shows a flat strip in the grid (`TL Grid` Kind=Folder). |
| Motion Guide | curved path | Guided layers sit under it, indented. |
| Guide | pencil-ruler | Reference only, **not exported**. |
| Mask / Masked | mask / masked | Masked layers indent under the mask. |
| Sound | waveform (green) | Grid shows the waveform span (`TL Span` Type=Sound). |
| Video | film | Grid shows a filmstrip span (`TL Span` Type=Video). |
| Armature | bone | IK poses as property keyframes ◆. |

Columns (`NF2/TL Toggle`): Visibility (dot / red ✕ hidden), Lock (dot / lock), Outline (solid layer-colour square /
outline-only square). Header icons eye, lock, outline toggle all rows.

### 5.3 Frames and markers

{{GEN:timeline-states}}

`NF2/TL Marker` kinds: Keyframe ● (filled), Blank Keyframe ○, Span End ▯, Property Keyframe ◆ (motion tween / armature),
Action Keyframe "a" (script on frame); `Tone=On Selection` inverts colours on selected frames.
`NF2/TL Frame Label` kinds: Label (red flag, `timeline/label`), Comment (green `//`, `timeline/comment`),
Anchor (⚓, named anchor).

`NF2/TL Playhead`: red line through all rows; frame number chip on the frame row; current time as a quiet chip on the
seconds row (e.g. `1.21s`). `NF2/TL Onion Range`: draggable before/after handles on the ruler.

### 5.4 Frame view menu (⋯ in the timeline controls)

Placement ▸ Bottom (docked) · Top · Left · Right · Floating window; Tiny · Small · Normal · Medium · Large ·
Preview (thumbnails per span) · Preview in context (object inside full stage) · Short (18 px rows) ·
Tinted frames (off = spans not colour-coded, only arrows) · Show seconds on ruler · Snap playhead to keyframes ·
Help F1 · Close panel.

![Timeline frame views](../exports/panels/dark/png/timeline-frame-views.png)

### 5.5 Nesting and large projects

Breadcrumb (`NF2/Breadcrumb Item` Type = Scene / Movie clip / Graphic / Button, State = Link / Current, optional ▾ menu;
`Breadcrumb Separator`, `Breadcrumb Overflow` "…" when the path is long). Clicking an item goes up; ▾ lists siblings.
The Symbols & scenes popover handles hundreds of symbols: search, sections *Scenes · N*, *Matches in symbols · 3 of 248*,
*Recent*, *Browse* (folders with counts), footer hint `↑↓ move · ↵ edit · ⌥↵ edit in place · ⌘↵ new window`.

Button symbols get the 4-frame timeline Up · Over · Down · Hit (Hit is never drawn, it defines the clickable area).

![Button symbol timeline](../exports/panels/dark/png/timeline-button-symbol-cta-btn.png)

---

## 6. Panels

All detached panels share `NF2/Floating Panel Header` (drag grip, up to 3 `Panel Tab`s via `Tab 2` / `Tab 3`
booleans, panel menu ⋯, collapse, close), `bg/raised`, radius 12, 1 px border, shadow. Bodies use 12 px padding and
`NF2/Section Header` (collapsible, Overline style) between groups. Docked versions use `NF2/Panel Header`.

{{GEN:panels}}

---

## 7. Code editor and scripting

- Font Roboto Mono 13 px / line 1.6 (Preferences ▸ Code editor). Gutter with line numbers; markers in the gutter:
  error (red circle ✕), warning (amber triangle), breakpoint (orange dot), execution arrow (accent), fold ▸/▾,
  bookmark. Overview ruler on the right mirrors markers.
- Inline error peek under the line ("1120: Access of undefined property orbit_mc." + "Quick fix ⌘.").
- Code hints popover (`Menu Item` with type on the right). Script tabs (`NF2/Script Tab`, pinned state).
- Toolbar: language select (ActionScript 3.0), target, auto-format, code hint, debug, collapse/expand, comment block/line,
  uncomment, toggle sidebar, Script assist, help.

**Syntax scheme "NF2 Night"** (Dark values; the Light column is the same tokens in Light mode)

| Token | Dark | Light | Rule |
|---|---|---|---|
| Keyword | `#c6a0f6` | `#7e3fc2` | violet |
| Type / class | `#8bd5ca` | `#0b7f72` | teal |
| Function | `#8aadf4` | `#2f5bd8` | blue |
| String | `#a6da95` | `#3d7a2a` | green |
| Number | `#f5a97f` | `#b5521b` | orange-peach |
| Comment | `#6e7891` | `#8a93a6` | grey |
| XML tag | `#7dc4e4` | `#0b6e99` | blue (never red) |
| Attribute | `#eed49f` | `#8a6500` | sand |
| Error | `#ff6b6b` | `#d92d2d` | **the only red** |
| Warning | `#f5b94a` | `#b26a00` | amber |
| Breakpoint | `#ff9a5c` | `#e8731f` | orange |
| Selection | `#2a3a63` | `#d5e2ff` | |
| Current line | `#171c27` | `#f2f5fb` | |

![Code editor shelf](illustrations/code-editor.png)

---

## 8. Menus and shortcuts

Main menu items are `NF2/Menu Bar Item` (Default / Open); dropdowns are `bg/raised` lists of `NF2/Menu Item`
(Default / Hover / Checked / Disabled, `Shortcut` text, `Submenu` boolean), `NF2/Menu Section` captions and
`NF2/Menu Separator`. Shortcuts are shown for macOS (⌘ ⌥ ⇧); Windows uses Ctrl / Alt / Shift.

{{GEN:menus}}

![Menus](../exports/panels/dark/png/menus.png)

---

## 9. Dialogs and preferences

Dialog anatomy: `NF2/Dialog Header` (title + close), body 24 px padding, footer right-aligned buttons
(primary last; destructive as danger-ghost on the left), `overlay` scrim, radius 12. Esc = Cancel, Enter = primary.

Preferences: 920 px dialog, left category tree (`Tree Item`) General · Code editor · Timeline · Drawing · Text ·
Clipboard · Shortcuts · Publish · Updates; right column of `Overline` sections with label + control rows;
footer Reset to defaults · Cancel · **Save**.

{{GEN:dialogs}}

### 9.1 Functional coverage map

{{GEN:coverage}}

---

## 10. Components (catalogue)

Every NF2 component, grouped by its shelf on `v2 · System & Components`. `set` = component set (variants);
property types: VARIANT (options), TEXT (default), BOOLEAN (default), INSTANCE_SWAP.

{{GEN:components}}

Shelf illustrations:

{{GEN:shelf-gallery}}

---

## 11. Working on the Figma file (rules for agents)

### 11.1 Access

- Figma MCP tools, `fileKey: BUz3pseopiuOg2Uiy3Fog7`. Writes go through `use_figma` (Plugin API, JS, 50 KB per
  call, no state between calls, `return` a value; `figma.notify` and `fetch` are unavailable).
- **Rate limit:** Organization / Full seat = 200 MCP calls a day, 15 a minute (reads count; `use_figma` counted
  too in practice). Batch work into few large `use_figma` calls; never export assets through MCP — use
  `tools/export-plugin` (see §12).

### 11.2 Standard helpers (repeat in each call)

```js
const coll = (await figma.variables.getLocalVariableCollectionsAsync()).find(c => c.name === 'NF2 · Tokens');
const V = {}; for (const id of coll.variableIds) { const v = await figma.variables.getVariableByIdAsync(id); V[v.name] = v; }
const P = n => figma.variables.setBoundVariableForPaint({ type: 'SOLID', color: { r: 0, g: 0, b: 0 } }, 'color', V[n]);
const styles = {}; for (const s of await figma.getLocalTextStylesAsync()) { styles[s.name] = s; await figma.loadFontAsync(s.fontName); }
async function T(str, style, token) { const t = figma.createText(); t.fontName = styles[style].fontName; t.characters = str;
  await t.setTextStyleIdAsync(styles[style].id); t.fills = [P(token)]; return t; }
const IC = {}; (await figma.getNodeByIdAsync('1:2845')).findAll(n => n.type === 'COMPONENT' && n.name.startsWith('NF2 Icon/')).forEach(c => IC[c.name] = c);
const recolor = (node, token) => node.findAll(() => true).forEach(x => { if ('fills' in x && x.fills.length) x.fills = [P(token)]; if ('strokes' in x && x.strokes.length) x.strokes = [P(token)]; });
const variant = (set, props) => set.children.find(c => Object.entries(props).every(([k, v]) => c.name.split(', ').includes(`${k}=${v}`)));
const setP = (inst, name, val) => { const key = Object.keys(inst.componentProperties).find(k => k === name || k.startsWith(name + '#')); inst.setProperties({ [key]: val }); };
const al = (name, dir, gap) => { const f = figma.createAutoLayout(dir, { name, itemSpacing: gap }); f.fills = []; return f; };
```

### 11.3 Gotchas (all hit in practice)

1. **Bound paint drops opacity.** `setBoundVariableForPaint` ignores paint `opacity`; for translucent tints use node
   opacity or an unbound colour (e.g. breakpoint line tint = unbound colour at 0.12).
2. **Per-side stroke weights** make `strokeWeight` = `figma.mixed`; read per-side values instead.
3. **`resize()` sets both sizing modes to FIXED.** Reset `primaryAxisSizingMode = 'AUTO'` / `counterAxisSizingMode`
   when content must hug, or frames collapse.
4. **Instances are named after the set** (`NF2/Panel Tab`), not the variant.
5. **Nested instances are only found reliably on the current page**: `await figma.setCurrentPageAsync(page)` before
   `findAll` / `findAllWithCriteria`. `createNodeFromSvg` creates on the current page; grouping needs nodes on the
   current page.
6. Page roots may contain TEXT nodes (titles): skip them before calling `findOne` / `findAll` on children.
7. Lines and vectors inside components need constraints STRETCH / SCALE and an instance re-resize to stretch.
8. Cross-page copy: `node.clone()` then `targetPage.appendChild(clone)`.
9. Light twin: `frame.setExplicitVariableModeForCollection(coll, coll.modes[1].modeId)`.
10. Fonts: only Inter and Roboto Mono (Helvetica broke `setProperties` in the past).
11. Descriptions: the API returns straight quotes and `&` HTML-escaped (`&quot;`, `&amp;`). Write typographic
    quotes “…” and the word “and” in component descriptions instead.

### 11.4 Rules

- New v2 work: NF2 components only; colours via `P(token)`; text via NF2 styles; spacing/radius values from tokens.
- New component → put it on a `Shelf / …` frame on `v2 · System & Components`, give it a description (what, when,
  interaction), then use instances in panels/screens. Add Light twins for new panels/screens.
- New icon → `NF2 Icon/16/<name>` (or 20), stroke-only, then regenerate the Icons page.
- Keep the coverage table and this document in sync: run the export plugin, then `tools/build-docs.py`.

---

## 12. Exports and folder layout

```
design-system/
  AGENTS.md                          ← short brief for Codex / coding agents (generated)
  tokens/                            ← tokens.css, typography.css, tokens.json (generated)
  docs/
    NeoFlash-Design-System.md        ← reference for Claude (generated)
    NeoFlash-Design-System.codex.md  ← same reference, Codex framing (generated)
    pdf/NeoFlash-Design-System-EN.pdf
    pdf/NeoFlash-Design-System-RU.pdf
    illustrations/                   ← shelves, tokens, audit, icons (PNG @2x)
    data/figma-dump.json             ← variables (all modes), styles, components, pages
    data/outline/*.txt               ← text outline of every exported frame
    data/*.psv                       ← token / style tables (pipe-separated)
    source/                          ← doc sources (md + html templates)
  exports/
    manifest.json                    ← node id → file paths
    interface/{dark,light}/{png,svg}/
    panels/{dark,light}/{png,svg}/
    icons/{16,20}/*.svg               ← every NF2 icon
    cursors/*.svg                     ← every NF2 cursor
  tools/
    export-plugin/                   ← Figma dev plugin: exports + dump + outlines
    receiver.py                      ← local receiver for the plugin
    build-docs.py                    ← builds both MDs, AGENTS.md, tokens and the PDF HTML
    build-pdf.sh                     ← HTML → PDF with headless Chrome
```

Re-export: `python3 tools/receiver.py`, run **NeoFlash Export** in Figma Desktop, then
`python3 tools/build-docs.py && tools/build-pdf.sh`. PNG are @2x and include drop shadows (frame effect bounds);
SVG text is outlined.

{{GEN:exports}}

---

## 13. Implementing in code

### 13.1 Tokens

`tools/build-docs.py` writes code-ready tokens from the Figma variables:

| File | Content |
|---|---|
| `tokens/tokens.css` | CSS custom properties. `:root` = Dark (default), `[data-theme="light"]` = Light. Names: `color/bg/panel` → `--nf-color-bg-panel`, `space/3` → `--nf-space-3`, `radius/md` → `--nf-radius-md`. |
| `tokens/typography.css` | One class per text style: `.nf-text-body`, `.nf-text-overline`, … (font family, weight, size, line height, tracking, case). |
| `tokens/tokens.json` | W3C Design Tokens format (`$type`, `$value`) with `dark` and `light` groups; text styles under `typography`. |

```css
@import "tokens/tokens.css";
.panel { background: var(--nf-color-bg-raised); border: 1px solid var(--nf-color-border-subtle);
         border-radius: var(--nf-radius-xl); padding: var(--nf-space-3); }
```

Switch theme by setting `data-theme="light"` on the root (or on a subtree, like Figma's per-frame mode).

### 13.2 Mapping components

- One code component per NF2 component **set**; Figma variant properties become props with the same option names
  (`Type="Primary" | "Secondary" | "Ghost" | "Danger"`, `State` is runtime state — hover / focus / disabled are
  pseudo-classes, not props, except `Selected` / `Checked` / `Active` which are real props).
- TEXT properties → string props / children; BOOLEAN properties → boolean props (e.g. `Icon`, `Flyout`, `Submenu`,
  `Indent 1/2`, `Editing`); nested icon instances → an `icon` prop taking an icon name (`"16/align-left"`).
- Sizes in §10 are the default instance size. Heights are fixed (32 / 28 / 40 / 36 / 24 px); widths hug content or
  fill the parent as in the outlines (`[H gap 8]` = horizontal auto layout with 8 px gap → `display:flex; gap:8px`).
- Timeline must be virtualised: 12 px × frame count per row, markers absolutely positioned at `(frame − 1) × 12`,
  spans as single elements of `frames × 12` px width, the grid drawn as a repeating background (every 5th cell
  `--nf-color-timeline-empty-5th`), not one element per frame.

### 13.3 Icons and cursors

- Icons: `exports/icons/16/*.svg`, `exports/icons/20/*.svg` (one file per `NF2 Icon/*` component, exported by the
  plugin). They are monochrome strokes; replace the stroke colour with `currentColor` so the token colour of the
  parent applies.
- Cursors: `exports/cursors/*.svg` (24 × 24) with the hotspot from §3.5 →
  `cursor: url(exports/cursors/pen.svg) 6 21, crosshair;`.

### 13.4 Interaction rules to preserve

- Numeric fields: drag the prefix label to scrub, ↑/↓ steps, ⇧ = ×10, units shown in the value.
- Tool buttons with `Flyout`: long-press (or right-click) opens the tool group; tooltip = name + shortcut.
- Panels: floating panels can dock (dock zones + docking compass), splitters resize docks, tabs drag between panels.
- Keyboard: all shortcuts in §8; Esc = cancel / close popover, Enter = primary action in dialogs; focus ring only on
  keyboard focus.
- Red only for errors; never use `--nf-color-danger-*` for anything decorative.

---

## Appendix A. Panel and screen outlines (condensed)

Condensed from `data/outline/*.txt`: `§` = container, `[Component "texts"]` = instance, quoted = text.
Use the full outlines for exact structure (auto layout direction and gap, sizes, property values).

{{GEN:outlines}}
