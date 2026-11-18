# NeoFlash UI design system (NF v3)

This is the design system of the NeoFlash authoring UI. **v3** (2026-11-18, owner-requested) is the current
reference: an evolution of NF2 that keeps its four palettes, Inter / Roboto Mono type scale and icon masters and
replaces the metrics, the atoms and the composition rules. NF2 names (`NF2 · Tokens`, `NF2/*` text styles,
`NF2 Icon/*`) stay as the token namespace. The v2.25 Figma pages and exports are the frozen previous reference.
The design system is **locked** outside an explicit owner-authorized design delivery: implementation agents use it
and never improvise changes (AGENTS.md › "Design system (NF2) — locked"; requests go to
[NF2-REQUESTS.md](NF2-REQUESTS.md)).

> **Reading order.** [v3](#v3--density-aware-atoms-and-stable-layouts) below overrides every size, spacing and
> composition statement in the v2.25 sections that follow; colours, typography, icons, contrast, accessibility and
> behaviour contracts of v2.25 still apply. The exported outlines and PNG references under
> `interface/design-system/exports` are v2.25 (Comfortable sizes) until the owner re-exports the v3 pages.

| What | Where |
| --- | --- |
| Design source (Figma export, docs, references) | [`interface/design-system/`](../interface/design-system/README.md) |
| Full specification (every screen, panel, dialog, menu, component) | [`NeoFlash-Design-System.codex.md`](../interface/design-system/docs/NeoFlash-Design-System.codex.md) |
| Code form of the design system (Rust, egui) | [`crates/neoflash-ui`](../crates/neoflash-ui/src/lib.rs) |
| Editor integration (theme roles, window chrome, panel files) | `crates/neoflash-editor/src/{theme.rs, shell.rs, dock_grid.rs, workspace.rs, panels/}` |
| What was changed and how it is structured | [NF2-INTEGRATION.md](NF2-INTEGRATION.md) |
| What every unfinished control must do / how to wire it | [NF2-FUNCTIONAL-SPEC.md](NF2-FUNCTIONAL-SPEC.md), [NF2-WIRING-PLAN.md](NF2-WIRING-PLAN.md) |
| Requests to the design track | [NF2-REQUESTS.md](NF2-REQUESTS.md) |

The cloud Figma file (`BUz3pseopiuOg2Uiy3Fog7`) is the source of truth for visuals; an older local `.fig` copy is not
an implementation reference. Code never copies a hex value or a size from a screenshot: it reads tokens and metrics
from `neoflash-ui`, which `build.rs` generates from the exported files. Exact gaps and sizes come from
`interface/design-system/docs/data/outline/*.txt`. If the design lacks a value, it is listed below
([Values NF2 does not define](#values-nf2-does-not-define)) instead of being invented.

## v3 — density-aware atoms and stable layouts

### Principles

1. **Density is a property of every atom.** Heights, inner paddings, gaps, radii, toggle sizes, label columns and
   default panel and dialog widths come from one table with three paradigms: **Normal** (the `Comfortable` value in
   saved preferences), **Compact** and **Classic Flash**. Type scale and colours never depend on density. A layout is
   written once and is correct in all three.
2. **Normal is already compact.** Controls are 26 pt (were 32), the inspector 264 pt (was 320). Nothing is wide
   "for comfort": numeric fields, selects and sliders have natural widths.
3. **Nothing jumps.** A dialog has one size per density — never per page, target or tab. A dock keeps its width when
   the active tab changes (minimum and maximum are taken over all its tabs). Scroll areas never shrink to their
   content. A control keeps its label and position in every context (`Browse…` is always `Browse…`). A row never
   overflows its panel: secondary actions become icon buttons with tooltips or ⋯ menu items.
4. **One form grid.** Inspector rows are two equal columns on one axis with the label inside the field; full-width
   rows only for text and selects. Dialogs use a label column (`dialog_label()`) and controls at their natural
   width (`field_num()`, `field_s()`, `field_m()`, `field_l()`); only path fields fill the row.
5. **One row per record.** A colour, a shortcut, a publish target, a layer, an API member: one row of `control()`
   or `row()` height with its actions at the end.
6. **Accent discipline.** One Primary button per surface. Accent marks selection, the active tool or tab, a toggle
   that is on, and keyboard focus — nothing else. Form options are checkboxes; a switch is only for an application
   state that applies at once (Autosave, Snapping).
7. **Features are never removed** by a layout change; they move to an icon button or a ⋯ menu.

### Density table

Source of truth: `neoflash_ui::metrics` (`DENSITY_TABLE`) and, on the Figma side, the variable collection
`NF3 · Density` (built from `interface/figma-plugin/v3/src/05-density.js`; `crates/neoflash-ui/tests/density_parity.rs`
keeps the two equal). Values are logical points, Normal / Compact / Classic Flash.

| Metric (function · Figma variable) | Normal | Compact | Classic Flash | v2.25 (C / Co / CF) |
| --- | --- | --- | --- | --- |
| `control()` · `size/control` — fields, selects, buttons, icon buttons | 26 | 22 | 20 | 32 / 28 / 24 |
| `row()` · `size/row` — tree rows, menu items, table rows | 24 | 20 | 18 | 28 / 24 / 20 |
| `panel_tab()`, `panel_header()`, `dock_tab()`, `icon_button_md()` · `size/tab` | 30 | 26 | 22 | 36–40 / 30–32 / 26–28 |
| `section_header()` · `size/section` | 24 | 22 | 20 | 32 / 28 / 24 |
| `top_bar()` · `size/top-bar` | 36 | 32 | 28 | 48 / 40 / 32 |
| `status_bar()`, `menu_section()` · `size/status` | 22 | 20 | 18 | 24 / 22 / 20 |
| `dialog_header()` · `size/dialog-header` | 44 | 38 | 32 | 60 / 52 / 44 |
| `tool_button()` · `size/tool`, tool glyph `icon_tool()` | 28, 20 | 24, 16 | 22, 16 | 32, 20 / 28, 20 / 24, 16 |
| `tools_rail()`, `tools_two_columns()` · `width/tools-1`, `width/tools-2` | 40, 72 | 36, 64 | 32, 57 | 56, 80 / 48, 68 / 40, 56 |
| `inspector()` · `width/inspector` — default and minimum right dock | 264 | 240 | 224 | 320 / 288 / 256 |
| `timeline_controls()` · `size/timeline-controls` | 34 | 30 | 26 | 44 / 40 / 32 |
| `timeline_ruler()`, `layers_header()` · `size/timeline-ruler` | 24 | 22 | 20 | 32 / 26 / 22 |
| default layer row (presets Normal / Short / Compact) · `size/layer-row` | 22 | 18 | 16 | 28 / 22 / 16 |
| `layer_column()` (+ Mode 64 + Solo 20 + 8 = `layer_column_mode_solo()`) | 208 (300) | 184 (276) | 164 (256) | 280 (372) / 240 (332) / 200 (292) |
| `breadcrumb()`, `zoom_control()` · `size/stage-bar` | 28, 32 | 24, 28 | 22, 24 | 34, 42 / 30, 36 / 26, 30 |
| `stage_ruler()` · `size/stage-ruler` | 18 | 16 | 16 | 20 / 18 / 16 |
| `toggle_row()`, `check_box()` · `size/toggle`, `size/check` | 18, 14 | 16, 14 | 16, 12 | 20, 18 (fixed) |
| `switch_width()` × `switch_height()`, `slider_thumb()` | 26 × 14, 12 | 24 × 12, 10 | 22 × 12, 10 | 32 × 18, 14 (fixed) |
| `badge()`, `kbd()`, `swatch()`, `menu_separator()` | 18, 18, 18, 9 | 16, 16, 16, 7 | 16, 16, 14, 5 | 22, 20, 18, 9 (fixed) |
| `space::s1()` … `s6()` · `gap/*`, `pad/*` | 4 · 6 · 8 · 12 · 20 · 28 | 3 · 4 · 6 · 8 · 16 · 22 | 2 · 4 · 5 · 6 · 12 · 16 | 4 · 8 · 12 · 16 · 24 · 32 (fixed) |
| `row_gap()` · `gap/row` — between form rows | 6 | 4 | 3 | 8 (fixed) |
| `radius::sm()` … `xl()` · `radius/*` | 3 · 5 · 8 · 10 | 3 · 4 · 6 · 8 | 2 · 3 · 4 · 6 | 4 · 6 · 8 · 12 (fixed) |
| `label_width()`, `dialog_label()` · `width/label`, `width/dialog-label` | 72, 112 | 64, 100 | 60, 92 | 76, 120 (fixed) |
| `field_num()`, `field_s()`, `field_m()`, `field_l()` · `width/num`, `width/field-*` | 64, 96, 160, 224 | 56, 88, 148, 204 | 52, 80, 136, 188 | 64–160 clamp, 200 |
| `sidebar()`, `sidebar_wide()`, `tree_width()`, `menu_width()` | 160, 208, 240, 208 | 148, 192, 220, 188 | 136, 176, 204, 172 | 160, —, 224, 220 |
| `dialog_s()`, `dialog_m()`, `dialog_l()`, `dialog_xl()` · `width/dialog-*` | 360, 480, 720, 880 | 328, 440, 660, 820 | 304, 408, 620, 760 | per-dialog literals |
| `dialog_l_height()`, `dialog_xl_height()`, `reference_width()` | 576, 640, 960 | 500, 600, 900 | 440, 560, 860 | content-driven |

The spacing scale has a meaning per step: `s1` tiny gap inside a control, `s2` gap between controls, `s3` inner
padding of fields and buttons, `s4` panel padding, `s5` dialog padding, `s6` gap between blocks. The uppercase
constants `space::S1…S6` and `radius::SM…XL` keep the v2.25 values for the few constant contexts that still need
them; layout code calls the functions. The `pub const` metrics are the Normal values.

### Surfaces redesigned in v3

| Surface | v3 composition | Code |
| --- | --- | --- |
| Properties — any selection | One-line type header (icon · kind · name). Rows on the two-column grid; `Name │ Swap…`, then `Behaviour │ Edit symbol (icon) │ Duplicate symbol (icon)`. The constrain toggle sits in the section header so X / Y and W / H share one axis. | `panels/properties*.rs` |
| Used colours (R-106) | Header row carries `Only on stage` and a ⋯ menu (Import palette…). One row per colour: swatch · `#HEX` with alpha · usage · select on stage. Replace is one bar: `Replace ▢ › ▢ … Replace all`. | `panels/swatches.rs` |
| Tools | No title in a narrow header (grip + ⋯ only); the grid has no holes; the options block has a fixed height, so the palette never resizes when another tool is selected. | `panels/tools.rs`, `components/headers.rs` |
| Stage toolbar | One height (`zoom_control()`), fixed 44 pt percent cell, collapsed state expands away from a fixed anchor. | `components/surface.rs`, `shell.rs` |
| Publish settings | `dialog_xl()` × `dialog_l_height()` for every target. Sidebar: Profile select + ⋯ (Duplicate, Import JSON…, Export JSON…), target rows with checkboxes. Page: label-column grid; one shared Output row (`path │ Browse…`). Footer: summary · Cancel · OK · Publish. | `panels/publish_settings.rs` |
| Preferences | `dialog_l()` × `dialog_l_height()` for every category. Category list, label-column grid, checkboxes for options (switch only for Autosave). | `panels/preferences.rs` |
| Scripting Reference | `reference_width()` × `dialog_xl_height()`. Always exactly one language (defaults to the project language). `API` tab: packages and classes · members with support status · member detail (signature, parameters, status in NeoFlash, example, Insert at cursor). `Guides` tab: plain articles with an outline. No tags, no "all languages". | `panels/help_as_reference.rs`, `panels/help_manual.rs` |
| UI Layout Studio (owner tool) | One toolbar row in groups, fixed columns, the selected panel previewed in all three densities, spacing picked from the density system. | `panels/ui_studio/` |

### Figma

- Pages: `v3 · Concept` (principles and the three paradigms side by side), `v3 · Atoms` (component sets `NF3/<Atom>`
  and the icon shelves), one reference page per combination `v3 · <Density> · <Theme>` (12 pages; the master is
  `v3 · Normal · Light`, the others are copies with two explicit variable modes), and `v3 · Builder source`.
- Variables: colours stay in `NF2 · Tokens` (Dark, Light, Mid · Neutral Gray, Dark · Graphite); sizes live in
  `NF3 · Density` (Normal, Compact, Classic Flash). Every atom binds its sizes and colours to them, so any board
  shows any of the twelve combinations by switching two modes.
- The v3 pages are generated: [`interface/figma-plugin/v3`](../interface/figma-plugin/v3/README.md) holds the builder
  (core, atoms, kit, surfaces) and an offline mock harness. Change a surface by editing its builder and re-running
  it; do not hand-edit generated boards.
- The board `Dialog / Help — Flash 8 study (not implemented)` is a design study only.

## Rules

1. **Tokens only.** Colours come from `neoflash_ui::palette(ctx)` (or the editor roles in `theme::UiPalette`), sizes
   from `neoflash_ui::metrics` and `neoflash_ui::tokens::{space, radius}`, text from `neoflash_ui::typography`
   (`rich` single line, `rich_block` paragraphs, `font`, `format_line`, `format`). No `Color32::from_rgb`, no ad-hoc
   font sizes, no magic paddings in panels. Coordinates of a drawing itself (Help illustrations, the UI Layout Studio
   canvas) are artwork, not layout; a depicted interface part still uses its metric (R-098).
2. **Components, not styling patches.** Panels are composed of `neoflash_ui::components`. No panel-specific visual
   exceptions. New components are added only by the design track, mirroring a Figma component set.
3. **Component API mirrors Figma.** Variant properties are enums with the same option names
   (`ButtonType::Primary`), booleans are builder flags (`ToolButton::flyout`), text properties are `&str`.
   Hover / focus / disabled / pressed are runtime state; `Selected` / `Checked` / `Active` are props.
4. **Four themes, one set of names.** Dark is the default; Light, Mid Neutral Gray and Dark Graphite swap semantic
   values only. Mid Neutral Gray uses `color/bg/app = #505050`; Dark Graphite uses `color/bg/app = #171717` and
   `color/timeline/grid = #2E2E2E`. Components never branch on theme. Public screenshot,
   gallery and export slugs are `dark`, `light`, `mid-neutral-gray` and `dark-graphite`.
5. **Red means error.** `danger_*` only for errors and destructive actions (plus the Flash conventions: playhead,
   frame labels). Warnings amber, breakpoints orange.
6. **Fixed geometry.** Fields / buttons 32, tree items 32, menu items / layer rows / segments 28, panel header 40,
   panel tabs 36, dock tabs 40, timeline controls 44, status bar 24, top bar 48, tools rail 56 (1 column) or 80
   (2 columns, the base layout), Properties/inspector minimum 320, bottom dock 420. Inspector body: 16 pt padding,
   12 pt between rows;
   Library body: 8 pt padding and gaps; floating panels: 12 / 12. Timeline: 12 pt per frame (Normal), rows 28,
   ruler 32, layer column 280, frame `n` at `(n − 1) × 12`, view modes Tiny 6 · Small 9 · Normal 12 · Medium 16 · Large 22.
   Major frame labels are centred on their ticks; frame 1 uses the edge-safe rule and seconds labels remain separate.
7. **Single-line text has no extra leading.** Labels inside controls use `rich` / `format_line` so glyphs centre
   exactly in 28/32/40 pt rows; the style line height is only for paragraphs.
8. **Accessibility.** Text contrast follows the contrast contract below (primary and secondary text ≥ 4.5:1), 2 pt focus ring on keyboard focus only
   (`theme::keyboard_focus` + `theme::focus_ring`), every icon-only control has an accessible name and an NF2 tooltip
   with its shortcut (`components::tooltip`; `components::not_available` for disabled controls).
9. **No v1.** `NF1` components in Figma are the legacy reconstruction; never implement them.
10. **Engine boundary.** `neoflash-ui` does not depend on `neoflash-core`, `-script`, `-export` or `-import`. Panels
    read and write documents only through editor methods. A control whose feature the engine lacks is disabled with
    the tooltip "Not available yet"; a panel the engine cannot back is a read-only *planned preview*
    (Window ▸ Panels ▸ Planned panels). Never a working-looking fake.

## Architecture

```
interface/design-system/        Figma export (tokens, icons, cursors, outlines, docs; PNG/SVG/PDF local only)
        │  build.rs reads tokens/tokens.json, exports/icons, exports/cursors, docs/data/figma-dump.json
        ▼
crates/neoflash-ui              NF2 for egui — no engine dependency
  tokens.rs      Palette (one field per `color/*` variable), DARK, LIGHT, MID_NEUTRAL_GRAY, DARK_GRAPHITE,
                 space::S1..S6, radius::*, TextStyle
  metrics.rs     control heights, window regions, timeline geometry, shadows (not Figma variables)
  theme.rs       Theme, install(), apply_visuals(), palette(), focus_ring(), keyboard_focus(); NF2 scrollbars
  typography.rs  bundled Inter 4.1 + Roboto Mono (OFL) + NF2 Symbols fallback (▸ ◂ ▾ ▴, generated by
                 scripts/make-nf2-symbols-font.py), rich(), rich_block(), font(), format_line(), format()
  icons.rs       i12::*, i16::*, i20::* (675 optical masters: 302 glyph names on the 12 / 16 / 20 grids)
                 → tinted textures; paint(), paint_sized() (size-matched master), show()
  cursors.rs     39 cursors with hotspots; show() paints over the canvas, system_fallback()
  components/    see "Component catalogue"
  examples/gallery.rs   every component in all four themes; saves a PNG for comparison
        ▼
crates/neoflash-editor
  theme.rs          editor roles (UiPalette) mapped onto NF2 tokens; editor constants = NF2 metrics
  fonts.rs          installs NF2 fonts for chrome; authored fonts stay separate
  shell.rs          top bar title/actions, workspace menu, status bar, stage breadcrumb / size badge / zoom
                    control, View zoom commands, screenshot mode
  dock_grid.rs      dock areas; NF2 Dock Tab (bottom), Panel Header (single side panel), Panel Tab (side groups),
                    NF2 Splitter, ghost ⋯ menu, per-panel body padding
  workspace.rs      NF2 default layout + workspace templates (prefs key `neoflash.workspace.v2`)
  native_windows.rs macOS: detached panels are auxiliary windows (no full-screen, no tabbing)
  ui_icons.rs       tool icon roles → NF2 SVG icons; NF2 menu rows for the menu bar
  paint_controls.rs shared inspector rows (Field / DragField / TextField / colour row) with edit sessions
  panels/           one file per panel (see below)
```

### Component catalogue (`crates/neoflash-ui/src/components/`)

| File | Figma components |
| --- | --- |
| `button.rs` | Button (Primary / Secondary / Ghost / Danger), Icon Button (Ghost / Selected / Primary, SM / MD), Tool Button (`ToolButton::corner_hit` = flyout corner hit area) |
| `field.rs` | Field (prefix, unit, scrub, ↑/↓, ⇧×10, `stepper()` = Stepper Field, `presets()` = Combo Field), Text Field, Search, Select, Color Field, Line Style Select |
| `inspector.rs` | DragField (inspector number field with drag-to-scrub and type-to-edit), Type Header (48 pt) and Compact Type Header (36 pt row and tile, 20 pt tool icon; Brush and Pencil properties) |
| `toggle.rs` | Checkbox, Switch, Radio, Slider (`SliderValue::{None, Field, Inline}`) |
| `segmented.rs` | Segmented Control / Segment |
| `headers.rs` | Dock Tab, Panel Tab, Script Tab, Section Header, Panel Header, Floating Panel Header, Dialog Header, Document Tab |
| `tree.rs` | Tree Item, Table Row |
| `menu.rs` | Menu Item (incl. keyboard `highlighted`), Menu Section, Menu Separator, Menu Bar Item, menu bar, popovers, context menus, submenus |
| `breadcrumb.rs` | Breadcrumb Item / Separator / Overflow |
| `color.rs` | Color Well, Fill Stroke Chip, Swatch Cell, Registration Picker, Icon Group |
| `icon_group_items.rs` | Icon Group with per-item disabled state and shortcut tooltips |
| `display.rs` | Badge, Kbd, Tooltip (`set_tooltip_shortcuts` hides every shortcut chip for Preferences ▸ Show tooltips with shortcuts), Swatch, Divider, Logo, Spinner, Progress, Toast, Empty State, Inline Message. `platform_shortcut` spells a shortcut for the running platform (`⇧⌘S` on macOS, `Ctrl+Shift+S` on Windows and Linux); Kbd, Menu Item and the shortcut chips apply it, so callers always pass macOS notation |
| `unavailable.rs` | "Not available yet" tooltip for disabled controls |
| `surface.rs` | Surfaces (App, Panel, Floating, Popover, Dialog, Canvas), scrim, dialog footer, status bar, Zoom Control, Dock Zone, Dock Compass, Drop Indicator, Splitter, Drag Ghost |
| `docking.rs` | dock target and drag ghost helpers |
| `shortcuts.rs` | Preferences ▸ Shortcuts (R-046): `ShortcutRow` (title, “Also on …” note, Kbd chips joined by “or”, “—” / “Add shortcut”), `ShortcutRecorder` (“Press keys…”, captured chips, Cancel / Accept), `conflict_strip` (warning + “Go to …”) |
| `timeline.rs` | TL Layer (optional Mode cell + Solo toggle: `LayerColumns`, `ModeCell`, `show_with` / `paint_at_with`, `paint_solo_toggle`, `BLEND_MODE_GROUPS`; R-040 / R-045, 372 pt column `LAYER_COLUMN_MODE_SOLO`), TL Toggle, TL Layers Header (`layers_header_with`), TL Grid, TL Grid Lines, TL Span (incl. Sound decoded peaks, `paint_sound_peaks`), TL Marker, TL Frame Label, TL Ruler, TL Onion Range, TL Playhead |
| `code.rs` | Code Editor: syntax highlighter (NF2 syntax tokens), gutter, markers, current line, script status, compiler messages, log lines, range underline, Error peek (error / warning) |
| `weight.rs` | Publish weight bar, legend and totals |
| `brushes.rs` | Tool Flyout Item, Brush Preset, Brush Shape, Brush Cursor, `NF2 Brush/*` sample strokes |
| `text_area.rs` | multi-line Text Field |
| `ease.rs` | Ease Preset, Ease Graph (0–100 %, Overshoot, Undershoot), preview track |
| `motion.rs` | Motion editor lanes |
| `swatches.rs` | Swatches Popover, gradient swatches |
| `view_settings.rs` | View Settings (rulers, guides & grid) |
| `color_picker.rs` | Color Picker (saturation/brightness area, hue and alpha strips; standard HSB conversion) |
| `gradient.rs` | Gradient Bar, Gradient Stop, Gradient Overflow Preview (Extend / Reflect / Repeat; `paint_gradient_overflow_preview`) |
| `envelope.rs` | Sound envelope lanes and anchor points (Edit envelope) |
| `canvas.rs` | Stage Ruler, Ruler Corner, Guide, Stage Grid, Selection Box, Transform Box, Anchor Point, Bezier Handle, Marquee, Shape Selection (two-tone hatch), Gradient Transform, Snap Ring, Motion Path, Motion Path Handle, Shape Hint |

### v2.25 contract and additions after the original v2.2 baseline

The live Figma file and generated `interface/design-system/` reference are synchronized through the v2.25 migration.
The owner-authorized repair changes layout and behavior contracts without removing implemented functions. The named
shelves/components remain the implementation contract together with the exported outlines.

| Component | Rule |
| --- | --- |
| `NF2/Shape Selection` (changed) | Two-tone 45° hatch: 1 pt white at 90 % plus 1 pt black at 75 %, 1.5 pt apart, repeating every 4 pt (both measured horizontally; ≈ 1.06 pt and 2.83 pt perpendicular to the stripes; R-058, was 8 pt). It does not follow the theme. On any fill, the better of the two lines reaches at least 3.8:1 (the v2.2 single grey hatch reached 1.5:1 on blue artwork in Dark and 1.1:1 in Light). |
| Figma file structure v2.25 (T-159/T-207, owner decisions 2026-11-09/10) | Live pages are `v2.25 · System & Components`, `v2.25 · Panels`, `v2.25 · Interface — Dark`, `— Light`, `— Mid Neutral Gray` and `— Dark Graphite`. Dark is the canonical structural page; the other interface pages and the four panel-board sets keep the same component structure with an explicit NF2 token mode. Exports use `interface/{dark,light,mid-neutral-gray,dark-graphite}` and `panels/{dark,light,mid-neutral-gray,dark-graphite}`. `v2.2 · … (archive 2026-11-09)` pages remain detached frozen references. The two `v2.25 · Interface — Light Neutral Gray … (archive 2026-11-14)` pages are an abandoned fifth-theme sketch archived by owner decision: not exported, not a reference. |
| Theme switching v2.25 | Window ▸ Appearance and Preferences ▸ General expose Dark, Light, Mid Neutral Gray and Dark Graphite. Preferences previews the draft immediately; Save persists it, while Cancel, Escape or closing the dialog restores the previously saved theme. The theme is an application preference and never changes authored document colours or history. |
| Optical icon masters 12 / 16 / 20 | Every icon grid has its own master in `exports/icons/{12,16,20}` and in Figma (`NF2 Icon/{12,16,20}/*`, page `v2.25 · System & Components`). 12 and 16 px use 1 pt hinted strokes; 20 px uses 1.5 pt strokes. `icons::paint_sized` and `Icon::at` select the matching master instead of scaling. v2.25 approves Gradient Transform, ToolModifierGlyph, missing TextPropertyGlyph and Extend / Reflect / Repeat overflow-preview masters; existing super/subscript, alignment and text-border masters are reused. |
| Tools panel v2.25 | One standard `NF2/Panel Header` titled Tools, with the Properties title style and left inset. There are no internal Tools / View / Colors / Options captions; dividers and every control remain. Fill/Stroke and reset/swap are centred on the active grid. Detached Tools snaps 16 pt inside eight Stage anchors with 112 pt capture: middle-left/right force the vertical rail, other anchors keep the two-column palette. Left-side popovers open right, right-side popovers left. Fill/Stroke reveals or focuses the real Color panel. |
| Contextual Properties v2.25 | No Selection / Tool switch. Context follows Stage, selected object/text/symbol/video, Timeline frame/range/tween/sound or active tool. Minimum content/floating width is 320; paired fields share equal columns with 8 pt gap and ≥80 pt cells, stacking below the fit threshold. Transform order: W/H, X/Y, Scale X/Y, Rotation/Skew. Text retains Character, Advanced character, Paragraph, Container and flow, Options and Filters, including every implemented CS5/TLF field. |
| Color panel v2.25 | No Selection / Drawing switch, object count or Mixed/apply gate. Fill is left, Stroke right. None / Solid / Linear / Radial / Bitmap share one contextual panel; Extend / Reflect / Repeat and Linear RGB, gradient stops, RGB/HSB, alpha, stroke width and Add to swatches remain. Floating minimum 248×190. Preferences remains 720 pt wide with a 160 pt category list and 120 pt labels (R-055). |
| Floating panels and docking v2.25 | The single `NF2/Floating Panel Header` covers one/two/three tabs, overflow, active, compact and collapsed states; no native macOS title chrome. Tabs keep reorder, tear-off, individual docking and commands. Width minima apply docked and floating; panel-specific height minima only floating. Visible edge target ≈5.5%, clamped 44–80 / 36–66; invisible hit halo ≈9.5%, clamped 72–132 / 60–112 and capped at 30% of the surface. The dashed halo appears only in explanatory boards. |
| Help and Function Search v2.25 | Help uses a 920×760 nominal clamped shell with fixed 52 pt header/close, 212 pt sidebar, stationary filter/badge/navigation, independent chapter/article scrolling, and Open in browser · Command Search (F1) · Close. The portable browser edition inherits the active application theme when opened; its single header control cycles Dark, Light, Mid Neutral Gray and Dark Graphite without changing the page layout. Function Search opens with input focused; the first character survives menu dismissal and keyboard selection scrolls into view (width ≤560, ≤24 results, results height ≤392). |
| Panel minimums v2.25 | Tools 88×160; Timeline 320×180; Library 248×300; Properties 320×220; Align/Transform/Color/Scene/History/Info 248×190; Swatches 300×360; Project 248×280; Filters 268×260; Code Snippets/Movie Explorer 280×280; Compiler Errors 360×180; Ease 320×420; Motion Editor 640×260; remaining panels 320×220. |
| Side-dock priority and splitter (changed, owner decision 2026-11-07) | Stacked side-dock groups have Inspector priority (Properties 68 %, Library 32 %; the new default, was 83 / 17) or Library priority (the reverse); minimums stay Library 100 pt, others 120 pt. Switch with Window ▸ Inspector / Library priority (⌥⌘F3 / ⌥⌘L) or a double-click on the splitter. `NF2/Splitter` always shows its 24 × 3 grip in `border/strong`, accent while hovered or dragged (R-054). |
| Stage toolbar states (new, owner decision 2026-11-07) | `NF2/Zoom Control` has three states: Floating (bottom centre), Collapsed (a pill with ‹ and the zoom percentage at the bottom-right corner that expands in place on hover and collapses 1 s after the pointer leaves) and Edit bar (a `control + 8` pt strip above the Stage, `bg/panel` with a `border/subtle` bottom line: breadcrumb left; stage size, hand, zoom, fit, rulers / guides / grid / snap and ⚙ right; the canvas shrinks, nothing covers artwork). Floating states hide while a drag that started on the canvas is in progress. Switch: View ▸ Stage toolbar or right-click the toolbar; Comfortable and Compact default to Collapsed, Classic Flash to Edit bar (R-053). |
| Density profiles (v2.25 values; superseded) | Runtime Comfortable / Compact / Classic Flash profiles were introduced here with heights only (controls 32 / 28 / 24, inspector 320 / 288 / 256). v3 renames Comfortable to Normal and makes paddings, gaps, radii, toggles and widths part of the profile: see [v3 › Density table](#density-table). Switch: Preferences ▸ General ▸ Density (live preview) and Window ▸ Density; stored in workspace preferences, default Normal (R-051). |
| Main menu composition (changed, owner decision 2026-11-08; v2.25 search behavior) | Order: File · Edit · View · Insert · Modify · Control · **Document** · Window · Help. Document contains Document settings… (`⌘J`, including Background), the open-document list, Next / Previous and Close / Close other documents / Close all. Every authoring-panel visibility row is under Window ▸ Panels; Workspaces, Hide panels, Inspector / Library priority, detach/dock, Appearance and Density stay at Window top level. The first Help row is Search functions… (`F1`), opening the existing catalogue with its input already focused (R-059, R-064). |
| Type scale B (changed, owner decision 2026-11-07) | Professional-tool scale, one table in `tokens.json` / `typography.css`: Heading 14/20, Dialog Title 14/20, Title 12/16 (600; panel titles such as Properties, Color, Library), Dialog Body 12/16, Body 11/14, Body Strong 11/14, Small 10/13, Small Strong 10/13, new Caption 10/12, Overline 10/12, Mono 11/14, Mono Small 10/12 (was 20/18/16/14/13/13/12/12/–/11/12/11). Row, control and panel heights are unchanged; density profiles are R-051. Authored stage text and the user-set code editor font are not UI type. |
| `NF2/Icon Button` Type=Primary (changed) | Square like every icon button (radius md), `accent/default` fill; the round shape is retired. The Timeline Play is Primary **SM 32 pt** inside the 2 pt transport group (R-052, 2026-11-07). |
| `NF2/TL Layer` heights (changed) | Three presets: Normal 28 pt with `NF2/Body`, Short 22 pt with `NF2/Small`, Compact 16 pt with the new `NF2/Caption`; Short and Compact use 12 pt icons; toggle columns stay 20 pt (R-052). |
| `NF2/TL Layer` Narrow (new) | Below a 280 pt layer column the 64 pt `Mode` cell becomes a 20 pt cell with a 12/16 pt `mixer` glyph in `accent/default`, shown only for non-Normal blend modes (and on hover); the header hides the "Mode" label (R-052). |
| Timeline controls bar (changed) | No clipping or hidden scrolling: controls hide by priority (zoom slider, Duration / Center playhead, Scene, Folder…), the right-end More ⋯ turns Selected and lists them under "Hidden controls" (grouped into Layers / Playback / Frames and tweens / Onion skin / View submenus beyond 8 rows). Add layer, transport and More are always shown; transport-over-frame-1 alignment applies only when everything fits. Frame view adds Fit selection / Fit content (R-052). |
| Timeline ruler labels v2.25 | Major frame labels are optically centred on their tick for mixed digit counts (1, 5, 10, 100). Frame 1 uses an edge-safe rule; the seconds row retains its independent placement. |
| `NF2/Shape Hint` | 16 pt disc with a letter a–z. Unplaced = `danger/default`, Start = `warning/default`, End = `success/default`, with `text/on-accent` for the letter and outline. Colours are pinned to the Dark token mode (R-009). |
| `NF2/Motion Path Handle` | `color/guide` tangent line and a 7 pt disc. Hover adds a ring; Drag uses `color/guide-selected` (R-008). |
| `NF2/TL Span · Sound — decoded peaks` | One mirrored 1 pt column per point in `success/default`; stereo uses two lanes; while not yet decoded, a 1 pt centre line at 50 % (R-010). |
| Paste motion special dialog | Checkbox grid of properties, two override options, Select none · Cancel · **Paste** (R-006). |
| Properties — Classic tween | Rotate × count + Direction, then Orient to path, Snap to motion guide, Sync graphic symbols and Scale (R-007). |

Shared pieces of the 5 November 2026 answers (R-022, R-032, R-033, R-035, R-036, R-038; canonical live-page
examples on `v2.25 · Panels`, frames `124:18100` Dark / `132:19052` Light, with equivalent Mid Neutral Gray and
Dark Graphite export boards) and the live `v2.25 · System & Components` shelf "Brushes & tool flyouts" (`96:793`):

| Component | Rule |
| --- | --- |
| `NF2/Document Tab` (`DocumentTab`) | 32 pt, radius md, 12 / 8 pt padding, file icon, `NF2/Small` label, 16 pt trailing slot; width 96–220 with ending ellipsis. Active = `bg/selected`, Hover = `bg/hover`; dot when dirty, × on hover and on the clean active tab; tooltip = path; equal names add the folder in tertiary text (R-035). |
| Inline message (`InlineMessage`) | 16 pt icon, 8 pt gap, wrapped `NF2/Small`. Info `text/secondary`, Warning `warning/default`, Error `danger/default` (R-032, R-036, R-022). |
| Publish weight (`WeightBar`, `weight_legend`, `weight_totals`) | 8 pt bar, 2 pt gaps, radius sm, on-disk bytes; parts WebAssembly `accent/default`, Runtime scripts `syntax/keyword`, Project scripts `syntax/function`, Content `layer/green`, Resources `warning/default`, Fonts `syntax/type`, Page `text/tertiary`. Wrapped legend (8 pt swatch, label, value) and totals. No per-part gzip, no budget marker (R-032). |
| Range underline (`paint_range_underline`) | 1.2 pt zigzag, 2 pt high, ~3 pt period under the exact range; `syntax/error` or `syntax/warning` (R-033). |
| Error peek (`ErrorPeek`) | `danger/subtle` or `warning/subtle`, 1 pt `syntax/error` / `syntax/warning` border, radius md, 10 × 6 padding, icon, `NF2/Small` message, `NF2/Small Strong` accent action (R-033). |
| Menu Item `highlighted` | Keyboard-chosen row in a ↑↓ menu (quick fixes) drawn like hover (R-033). |
| v2.2 `ToolFlyoutItem`, `BrushPreset`, `BrushShape`, `brush_cursor`, `brush_stroke_sample` | As on the shelf. The brush cursor follows zoom: at least 4 pt, above 400 pt a crosshair and value chip; Resize is dashed with a chip (R-038). |

### Panel files (`crates/neoflash-editor/src/panels/`)

| File | Panel | Figma reference (`interface/design-system/exports/…`, local) |
| --- | --- | --- |
| `menu.rs` | Menu bar in the top bar | `interface/*/png/editor-edit-menu-open.png`, `panels/*/png/menus.png` |
| `timeline.rs`, `timeline_controls.rs` | Timeline (layers, ruler, frames; 44 pt controls bar, onion popover, frame view menu) | `interface/*/png/editor-main.png`, `timeline-states-legend.png`, `panels/*/png/timeline-frame-views.png` |
| `tools.rs` | Tools (contextual 2-column palette; forced 1-column rail at middle side Stage anchors; no body captions) | `panels/*/png/panel-tools-floating.png` |
| `properties.rs` | Contextual Properties (320 pt minimum; no Selection / Tool switch) | `interface/*/png/editor-main.png`, `panels/*/png/panel-properties-*.png` |
| `library.rs` | Library | `panels/*/png/panels-more.png` |
| `actions.rs` | Actions, Output, bottom-panel dispatch | `interface/*/png/editor-actions-tab.png`, `panels/*/png/panel-actions-floating.png`, `panels-more-2.png` |
| `accessibility.rs` | Accessibility | `panels/*/png/panels-more-2.png` |
| `planned.rs`, `planned_panels.rs`, `planned_color.rs`, `planned_dialogs.rs`, `planned_documents.rs`, `planned_drawing.rs`, `live_planned.rs` | 13 read-only planned previews (`PlannedPanel`: Debugger, Export image, Import video, Bitmap fill, Oval / Rectangle primitive, Deco, Spray brush, IK bone, Graphic looping, Stroke style, Font embedding, Test Movie window), opened from their menu items / buttons and from Window ▸ Panels ▸ Planned panels. They stay visible in Beta 1 (owner decision); entries that still show sample data are being bound to live data or marked unavailable | the remaining `panels/*/png/*.png` |
| `../authoring_modify.rs` | Align, Transform, Color panel bodies | `panel-align.png`, `panel-transform.png`, `panel-color-*.png` |
| `../symbol_dialog.rs`, `../app.rs` (`draw_dialogs`) | Convert to Symbol, Unsaved changes, About | `interface/*/png/editor-*-dialog.png` |
| `publish_settings.rs`, `preferences.rs`, `export_*.rs`, `import_video.rs`, `help_*.rs`, `function_search.rs`, `debugger.rs` | Designed dialogs and panels whose current bodies **extend** the reference (see Provisional surfaces) | `dialog-publish-settings.png`, `dialog-preferences-*.png`, `dialog-export-*.png`, `dialog-import-video.png`, `dialog-help-reference*.png`, `popover-function-search.png`, `panel-debugger.png` |
| `appearance.rs`, `properties_video.rs`, `productivity_tools.rs`, `output.rs`, `ui_studio/` | Provisional (no Figma reference): Properties ▸ Appearance, Properties ▸ Video montage, productivity dialogs, virtualized Output, UI Layout Studio | none yet (R-068, R-069; see Provisional surfaces) |

### Workspace templates

Window ▸ Workspaces and the top-bar select switch between **Classic** (NF2 default), **Animator** (tall timeline,
Transform beside Properties), **Designer** (Color, Align, Transform open), **Developer** (tall Actions with Output),
**Video edit** (video montage: Properties · Scene and Library · Project tab groups, 44 pt preview rows; added after
v2.25 without a design board; saved preferences named `premiere` load as Video edit) and **Essentials** (one-column tools, compact dock).
Templates replace the layout and close floating panel windows; theme, document and history are untouched.
"Reset … workspace" re-applies the current template.

### Editor palette roles

`theme::UiPalette` keeps the role names that existing painters use; each role is one NF2 token. New code reads NF2
tokens directly.

| Role | NF2 token | Role | NF2 token |
| --- | --- | --- | --- |
| `panel_fill` | `bg/panel` | `window_fill` | `bg/raised` |
| `divider` | `border/subtle` | `border`, `stage_border` | `border/default` |
| `focus`, `scrollbar_focus` | `border/focus` | `accent`, `timeline_script` | `accent/default` |
| `muted_text` | `text/secondary` | `origin_cross` | `text/tertiary` |
| `disabled_text` | `text/disabled` | `error_text` / `warning_text` | `danger/default` / `warning/default` |
| `scrollbar_track` | `bg/hover` | `scrollbar_thumb / hover / drag` | `text/disabled` / `text/tertiary` / `text/secondary` |
| `pasteboard` | `bg/canvas` | | |

### Values NF2 does not define

Kept in `theme::StageOverlay` (editor-only) until the design covers them; the design track promotes them to
`NF2 · Tokens` and deletes the constants.

| Value | Why it exists |
| --- | --- |
| `overlay`, `overlay_halo`, `handle_fill` | Selection / transform outline drawn over arbitrary artwork: outline + halo pair with ≥ 9:1 contrast. |
| `onion_before`, `onion_after` | Onion-skin tints for previous / next frames. |
| `stage_shadow` | Shadow under the stage page. |

Hard-coded in editor files after v2.25 and **not** yet NF2 values (to be replaced by canvas-chrome tokens that follow
the black/white-pair rule):

| Value | Where |
| --- | --- |
| 3D gizmo axes X `#E64444`, Y `#38C462`, Z `#428AF5`, vanishing point `#F5B234` | `panels/properties_movie_clip.rs`; kept as fixed canvas values with a black underlay (R-091) |
| Puppet / bone pins `#F5B234` with a black ring | `panels/properties_tool.rs`; fixed canvas value, white ring inside a black ring (R-091) |
| Gradient overflow preview greys and a red dashed boundary (red is reserved for errors) | `authoring_modify.rs` (`paint_overflow_preview`); replaced by `paint_gradient_overflow_preview` (R-081) |
| Stage text-preview error in `DARK_RED` monospace 12 | `stage_primitives.rs`; replaced by the error plate of R-085 |

### Contrast contract and open design findings

Text contrast, measured from `tokens.json` (14 November 2026, after the Light and Dark Graphite corrections and the
state-colour correction of R-101):

| Text colour | Dark, Light, Dark Graphite | Mid Neutral Gray |
| --- | --- | --- |
| `text/primary`, `text/secondary` | ≥ 4.5:1 on app, panel, raised, field, hover and selected | `text/primary` ≥ 4.5:1 everywhere; `text/secondary` 4.16 on raised |
| `text/tertiary`, `accent/default`, `danger/default`, `warning/default`, `success/default` | ≥ 4.5:1 on app, panel, raised and field; ≥ 3:1 on hover and selected | not guaranteed (`success/default` 1.9–3.1, tertiary 3.0–4.9) |

These colours never carry meaning alone: an error, warning or success state always has an icon and text.

State colours against the surface they sit on (R-101). A hover or selection fill is as strong as the text contract
on it allows; a rail is a control part and uses a border colour, never `bg/hover`.

| State | Dark | Light | Dark Graphite | Mid Neutral Gray |
| --- | --- | --- | --- | --- |
| `bg/hover` on raised (menus) · panel · app | 1.49 · 1.58 · 1.65 | 1.30 · 1.21 · 1.14 | 1.40 · 1.59 · 1.73 | 1.91 · 1.75 · 1.21 |
| `bg/selected` on raised · panel · app | 1.35 · 1.43 · 1.50 | 1.30 · 1.21 · 1.14 | 1.25 · 1.41 · 1.54 | 2.07 · 1.89 · 1.31 |
| Slider and progress rail (`border/strong`) on raised · panel | 2.04 · 2.16 | 2.17 · 2.03 | 1.59 · 1.80 | 1.95 · 2.13 |

In Mid Neutral Gray the hover and selection fills are darker than every surface, because the surfaces of that theme
lie on both sides of any mid grey. `NF2_GALLERY_SECTION=states` in the gallery shows these states on the three
surfaces; run it after any change to a background, border or text token.

| Finding | State |
| --- | --- |
| Mid Neutral Gray has no red / amber semantics | `danger/default #FFF5F4`, `warning/default #FFF5E0` and `timeline/label #FFF5F4` are near-white and cannot be told apart from `text/primary #F4F4F4`. The owner kept the palette for Beta 1; it is a known limit of this theme. |
| Control boundaries | `border/default` against `bg/field` / `bg/panel` is 1.39–2.02:1 in all four themes (WCAG 1.4.11 asks 3:1). Accepted: a field is identified by its fill, label and border together, and keyboard focus adds the 2 pt ring. |
| Idle scrollbar thumb (`text/disabled` on `bg/hover`) ≈ 1.7–1.8:1 (3.8:1 in Mid Neutral Gray) | Accepted: a quiet 4 pt floating thumb; hover and drag reach ≥ 3:1. |
| Stacked side dock | Decided in R-077: the second group keeps a useful minimum (Library 245 / 213 / 185 pt by density) or collapses to its header. Implementation pending. |
| Minimum window 1024 × 720 | Decided in R-078 (Stage row ≥ 360 pt, pinned Tools block with "…" overflow, workspace select 132–180 pt). Reference screen `NF2 / Editor — Main — 1024 × 720` is drawn in four themes (`editor-main-1024x720` in the export manifest); implementation and the PNG export are pending. |
| `editor-main` export predates v2.25 details | Decided in R-087. The Figma screens now show the label "Classic", the Scene selector, three dock tabs, two-column Tools and the Mode / Solo layer columns; Properties ▸ Publish settings… is added to the product. PNG export pending. |
| §3.2 says "field rows gap 8", outlines use 12 in the inspector | The editor follows the outlines (inspector 12, Library 8). |

### Accessibility acceptance matrix (R-057)

State on 14 November 2026, from the code and the shared test suite; it is not a screen-reader or hardware audit.
**T** = proven by a test, **I** = implemented but not tested, **P** = partly there (the gap is named), **—** = not there.
K = keyboard operation, F = 2 pt keyboard focus ring, N = accessible names and roles, D = a disabled control gives
its reason.

| Surface | K | F | N | D | Gap that matters most |
| --- | --- | --- | --- | --- | --- |
| Menu bar, Search functions (F1) | P | I | T | P | The menu bar cannot be opened from the keyboard; F1 search is the keyboard path. |
| Tools | P | I | T | — | No arrow navigation between tools; Smooth / Straighten give no reason when disabled. |
| Stage | P | — | P | P | Nudge and view keys work; objects, handles and the context menu have no keyboard path, focus mark or name. |
| Timeline | P | P | T | P | Layer rows and frames cannot be walked or range-selected by keyboard; cells have no ring; frames are unnamed. |
| Properties | T | P | T | P | Fields show the focus border on pointer focus too; the panel shortcut does not move focus into the panel. |
| Library | I | I | T | — | No ↑ / ↓ row navigation; about 30 menu rows disable without a reason. |
| Color panel and popover | P | P | P | — | Colour area, hue, alpha and gradient stops are drag-only; hue and alpha are unnamed. |
| Actions | P | I | T | P | Script tabs have no ring. |
| Output, Compiler errors | I | I | T / I | — | Copy and Clear give no reason when empty. |
| Debugger | I | I | T | — | Step keys are untested outside the ignored browser test. |
| Preferences, Shortcuts recorder | T | P | T | P | Shortcut rows and the recorder have no ring. |
| Publish settings | T | I | T | P | A disabled Publish button does not repeat the validation reason. |
| Export video, Export audio | I | I | I | — | Not driven by keys or labels in any test. |
| Document tabs, empty state | I | I | T | — | Tab switching keys are untested. |
| Docking | P | I | T | — | Splitters cannot be resized from the keyboard. |
| Help | I | P | I | — | Only a smoke test on a sample control. |
| Layer properties, Convert to symbol | I | I | P | I | Enter / Esc are local, outside the shared dialog helper, and untested. |
| Toasts | P | I | T | n/a | No dismiss key and no live-region role. |

Shared facts: the ring is `theme::focus_ring` (2 pt, 2 pt outside the control, `border/focus`) and shows only after
Tab (`theme::keyboard_focus`); text fields are the exception and show their focus border on any focus. Dialogs using
`dialog_helpers::DialogKeyboardState` share Enter / Esc handling (nine dialogs; sixteen handle them locally). Disabled
reasons are hover tooltips and are not part of the accessible node. VoiceOver for the editor itself is a documented
Beta 1 limit.

Acceptance bar for Beta 1: every panel and dialog is reachable and its commands run from menus, Search functions or
shortcuts (true today except Stage object selection and Timeline cell navigation, which need a pointer); names are
tested on every panel except Export, Help and the two small dialogs. Order of repair after Beta 1: Stage and
Timeline keyboard paths; disabled reasons in the accessible node; rings on Menu Item, Script Tab, Swatch Cell,
Gradient Stop, shortcut rows and layer cells; keyboard colour picking; one shared dialog helper with a focus-trap
test. One finding needs a runtime check before Beta 1: single-key shortcuts are blocked only by text focus, so Enter
or Space on a Tab-focused button may also trigger a global command.

### Density profiles

Superseded by [v3 › Density table](#density-table). The v2.25 board `Density profiles — Comfortable, Compact,
Classic Flash` stays in the archive pages; in v3 every board exists in all three densities.

## Provisional surfaces after v2.25

Implemented with NF2 components but without a Figma board, export or outline. They are product behaviour, not design
reference; changes to them still need a design-track answer.

| Surface | Code | What is provisional |
| --- | --- | --- |
| Publish settings extensions | `panels/publish_settings.rs` | Drawn on the `Dialog / Publish settings` board in four themes and decided in R-089 (options order, Sequences row, collapsed Network startup simulator, scrolling right column); export and implementation pending. |
| Preferences ▸ Updates | `panels/preferences.rs` | Still shows the design board's sample data (version, extensions); it is being replaced by a real GitHub Releases check and verified download (owner decision 2026-11-14). |
| Video montage | `panels/properties_video.rs`, `timeline_preview.rs`, Video edit workspace | Drawn in four themes: `NF2 / Editor — Video edit workspace` (tab groups, 44 pt Preview rows, linked video and audio spans) and the frame Video section on `Properties and Motion editor — additions after v2.25` (R-093). Razor tool masters `tool-razor` (12 / 16 / 20) delivered. Export and implementation pending. |
| Properties ▸ Appearance | `panels/appearance.rs` | Drawn on the `Properties — Appearance` board in four themes and specified in R-095; the shipped panel still uses text buttons. Export and implementation pending. |
| Stage gizmos | `panels/properties_movie_clip.rs`, `panels/properties_tool.rs` | 3D, puppet / bone and deform handles with hard-coded colours kept as fixed canvas values (R-091); tool masters delivered (R-067b). |
| Motion editor tools | `panels/motion_editor.rs` | Tools row drawn in four themes; shown only with a motion tween in context (R-090). Implementation pending. |
| Live Debugger and Quick-Fixes | `panels/debugger.rs`, `debugger*.rs` | Ratified as built (R-092, states of R-033). |
| Productivity dialogs | `panels/productivity_tools.rs` | Drawn on the `Productivity — dialogs` board in four themes (R-068); export pending. |
| Project and Selection colours | `panels/swatches.rs`, `panels/properties.rs` | Selection colours panel drawn in four themes; usage is Small text, not a Badge (R-094). Implementation pending. |
| AI connections | `ai_connections.rs` (no panel yet) | Designed, not built: Preferences ▸ AI, the AI connection dialog and the Send to AI review dialog are drawn on `AI connections — Preferences, connection and consent`, and the AI answer dialog (waiting, streaming, proposals, failure) on `AI answer — waiting, streaming, proposals and failure`; both in four themes, specified in R-097 and R-099. |
| Project script language | `panels/document_settings.rs`, `panels/properties.rs`, `panels/actions.rs` | Designed, not built: the `Script` select with the "Also compile …" checkbox and the count message in Document settings and Properties ▸ Publish, the language `Badge` and the notice row in Actions are drawn on `Script language — Document settings, Properties and Actions` in four themes and specified in R-102. Implementation pending. |
| Beta feedback | no panel yet | Designed, not built: the `Bug?` chip in the bottom-left corner of the Stage and the Beta feedback dialog in four variants (email, name and text · email and text · name and text · text only, each with image attachments) are drawn on `Beta feedback — Bug? chip and feedback dialog` in four themes and specified in R-103; Beta 1 ships the text-only variant with attachments. Implementation pending. |
| UI Layout Studio | `panels/ui_studio/` (Window ▸ UI Layout Studio…) | Developer tool composition and history contract (R-069). |

## Verifying

```bash
for theme in dark light mid-neutral-gray dark-graphite; do
  cargo run -p neoflash-ui --example gallery -- "$theme" "target/ui-shots/gallery-${theme}.png"
done
```

```bash
for theme in dark light mid-neutral-gray dark-graphite; do
  NEOFLASH_WINDOW=1920x1080 \
  NEOFLASH_SCREENSHOT="target/ui-shots/editor-${theme}.png" \
  NEOFLASH_SCREENSHOT_THEME="$theme" \
  cargo run -p neoflash-editor
done
```

| Variable | Effect |
| --- | --- |
| `NEOFLASH_SCREENSHOT=path.png` | Save a PNG of the main window after the window settles, then quit. Saved layouts are ignored. |
| `NEOFLASH_SCREENSHOT_THEME=dark\|light\|mid-neutral-gray\|dark-graphite` | Stable theme slug for the capture. The underscore spellings of the two added themes remain accepted for compatibility, but documentation and export folders use hyphens. |
| `NEOFLASH_SCREENSHOT_WORKSPACE=classic\|animator\|designer\|developer\|video edit\|essentials` | Workspace template for the capture (e.g. `developer` shows Actions). |
| `NEOFLASH_SCREENSHOT_DENSITY=compact\|classic`, `NEOFLASH_SCREENSHOT_STAGEBAR=collapsed\|hover\|editbar`, `NEOFLASH_SCREENSHOT_PRIORITY=library` | Density profile, Stage toolbar state and side-dock priority. |
| `NEOFLASH_SCREENSHOT_PANELS=history,swatches,…`, `NEOFLASH_SCREENSHOT_DIALOG=publish_settings\|export_video\|…`, `NEOFLASH_SCREENSHOT_PREFERENCES=<page>` | Open real panels, a dialog or a Preferences page for the capture. |
| `NEOFLASH_SCREENSHOT_SELECT=select\|transform\|motion\|…`, `NEOFLASH_SCREENSHOT_FRAME=<layer>,<frame>`, `NEOFLASH_OPEN=path.nflash` | Selection, frame and document for contextual Properties captures. |
| `NEOFLASH_WINDOW=1920x1080` | Fixed window size (leaves a restored full-screen state first); matches the Figma screens. |
| `NF2_GALLERY_SECTION=planned` | Gallery scrolls to the planned-panel components. |
| `NEOFLASH_SCREENSHOT_PLANNED=Ease,Debugger\|all` | Open the listed planned previews side by side for the capture. |

Compare with `interface/design-system/exports/interface/{dark,light,mid-neutral-gray,dark-graphite}/png/editor-main.png`
(3840 × 2160 = 1920 × 1080 @2x;
crop the same region of both, e.g. with `ffmpeg -i ref.png -i cur.png -filter_complex
"[0]crop=W:H:X:Y[a];[1]crop=W:H:X:Y[b];[a][b]vstack" out.png`). A screenshot comparison is not proof of keyboard,
docking or accessibility behaviour; those stay covered by the editor tests. The shared acceptance scenarios
(1024 × 720 and 1920 × 1080, all four themes, keyboard focus, dense and empty content, detach → dock → reopen,
Retina scaling) apply to every delivery batch.
