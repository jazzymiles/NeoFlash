# NeoFlash UI design system (NF2)

NF2 is the design system of the NeoFlash authoring UI. It replaces the earlier `docs/design-system/tokens.json`
standard. The design system is **locked**: implementation agents use it and never change it (AGENTS.md ›
"Design system (NF2) — locked"; requests go to [NF2-REQUESTS.md](NF2-REQUESTS.md)).

| What | Where |
| --- | --- |
| Design source (Figma export, docs, references) | [`interface/design-system/`](../interface/design-system/README.md) |
| Full specification (every screen, panel, dialog, menu, component) | [`NeoFlash-Design-System.codex.md`](../interface/design-system/docs/NeoFlash-Design-System.codex.md) |
| Code form of the design system (Rust, egui) | [`crates/neoflash-ui`](../crates/neoflash-ui/src/lib.rs) |
| Editor integration (theme roles, window chrome, panel files) | `crates/neoflash-editor/src/{theme.rs, shell.rs, dock_grid.rs, workspace.rs, panels/}` |
| What was changed and how it is structured | [NF2-INTEGRATION.md](NF2-INTEGRATION.md) |
| What every unfinished control must do / how to wire it | [NF2-FUNCTIONAL-SPEC.md](NF2-FUNCTIONAL-SPEC.md), [NF2-WIRING-PLAN.md](NF2-WIRING-PLAN.md) |
| Requests to the design track | [NF2-REQUESTS.md](NF2-REQUESTS.md) |

The Figma file (`BUz3pseopiuOg2Uiy3Fog7`, local copy `interface/figma/Interface test.fig`, not versioned) is the source
of truth for visuals. Code never copies a hex value or a size from a screenshot: it reads tokens and metrics from
`neoflash-ui`, which `build.rs` generates from the exported files. Exact gaps and sizes come from
`interface/design-system/docs/data/outline/*.txt`. If the design lacks a value, it is listed below
([Values NF2 does not define](#values-nf2-does-not-define)) instead of being invented.

## Rules

1. **Tokens only.** Colours come from `neoflash_ui::palette(ctx)` (or the editor roles in `theme::UiPalette`), sizes
   from `neoflash_ui::metrics` and `neoflash_ui::tokens::{space, radius}`, text from `neoflash_ui::typography`
   (`rich` single line, `rich_block` paragraphs, `font`, `format_line`, `format`). No `Color32::from_rgb`, no ad-hoc
   font sizes, no magic paddings in panels.
2. **Components, not styling patches.** Panels are composed of `neoflash_ui::components`. No panel-specific visual
   exceptions. New components are added only by the design track, mirroring a Figma component set.
3. **Component API mirrors Figma.** Variant properties are enums with the same option names
   (`ButtonType::Primary`), booleans are builder flags (`ToolButton::flyout`), text properties are `&str`.
   Hover / focus / disabled / pressed are runtime state; `Selected` / `Checked` / `Active` are props.
4. **Two themes, one set of names.** Dark is the default, Light swaps values only. Components never branch on theme.
5. **Red means error.** `danger_*` only for errors and destructive actions (plus the Flash conventions: playhead,
   frame labels). Warnings amber, breakpoints orange.
6. **Fixed geometry.** Fields / buttons 32, tree items 32, menu items / layer rows / segments 28, panel header 40,
   panel tabs 36, dock tabs 40, timeline controls 44, status bar 24, top bar 48, tools rail 56 (1 column) or 80
   (2 columns, the base layout), inspector 320, bottom dock 420. Inspector body: 16 pt padding, 12 pt between rows;
   Library body: 8 pt padding and gaps; floating panels: 12 / 12. Timeline: 12 pt per frame (Normal), rows 28,
   ruler 32, layer column 280, frame `n` at `(n − 1) × 12`, view modes Tiny 6 · Small 9 · Normal 12 · Medium 16 · Large 22.
7. **Single-line text has no extra leading.** Labels inside controls use `rich` / `format_line` so glyphs centre
   exactly in 28/32/40 pt rows; the style line height is only for paragraphs.
8. **Accessibility.** Text contrast ≥ 4.5:1 (see findings), 2 pt focus ring on keyboard focus only
   (`theme::keyboard_focus` + `theme::focus_ring`), every icon-only control has an accessible name and an NF2 tooltip
   with its shortcut (`components::tooltip`; `components::unavailable` for disabled controls).
9. **No v1.** `NF1` components in Figma are the legacy reconstruction; never implement them.
10. **Engine boundary.** `neoflash-ui` does not depend on `neoflash-core`, `-script`, `-export` or `-import`. Panels
    read and write documents only through editor methods. A control whose feature the engine lacks is disabled with
    the tooltip "Not available yet"; a panel the engine cannot back is a read-only *planned preview*
    (Window ▸ Planned panels). Never a working-looking fake.

## Architecture

```
interface/design-system/        Figma export (tokens, icons, cursors, outlines, docs; PNG/SVG/PDF local only)
        │  build.rs reads tokens/tokens.json, exports/icons, exports/cursors, docs/data/figma-dump.json
        ▼
crates/neoflash-ui              NF2 for egui — no engine dependency
  tokens.rs      Palette (one field per `color/*` variable), DARK, LIGHT, space::S1..S6, radius::*, TextStyle
  metrics.rs     control heights, window regions, timeline geometry, shadows (not Figma variables)
  theme.rs       Theme, install(), apply_visuals(), palette(), focus_ring(), keyboard_focus(); NF2 scrollbars
  typography.rs  bundled Inter 4.1 + Roboto Mono (OFL) + NF2 Symbols fallback (▸ ◂ ▾ ▴, generated by
                 scripts/make-nf2-symbols-font.py), rich(), rich_block(), font(), format_line(), format()
  icons.rs       i16::*, i20::* (304 SVG icons) → tinted textures; paint(), paint_sized(), show()
  cursors.rs     39 cursors with hotspots; show() paints over the canvas, system_fallback()
  components/    see "Component catalogue"
  examples/gallery.rs   every component in Dark/Light; saves a PNG for comparison
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
| `inspector.rs` | DragField (inspector number field with drag-to-scrub and type-to-edit), Type Header |
| `toggle.rs` | Checkbox, Switch, Radio, Slider (`SliderValue::{None, Field, Inline}`) |
| `segmented.rs` | Segmented Control / Segment |
| `headers.rs` | Dock Tab, Panel Tab, Script Tab, Section Header, Panel Header, Floating Panel Header, Dialog Header, Document Tab |
| `tree.rs` | Tree Item, Table Row |
| `menu.rs` | Menu Item (incl. keyboard `highlighted`), Menu Section, Menu Separator, Menu Bar Item, menu bar, popovers, context menus, submenus |
| `breadcrumb.rs` | Breadcrumb Item / Separator / Overflow |
| `color.rs` | Color Well, Fill Stroke Chip, Swatch Cell, Registration Picker, Icon Group |
| `icon_group_items.rs` | Icon Group with per-item disabled state and shortcut tooltips |
| `display.rs` | Badge, Kbd, Tooltip, Swatch, Divider, Logo, Spinner, Progress, Toast, Empty State, Inline Message |
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
| `gradient.rs` | Gradient Bar, Gradient Stop |
| `envelope.rs` | Sound envelope lanes and anchor points (Edit envelope) |
| `canvas.rs` | Stage Ruler, Ruler Corner, Guide, Stage Grid, Selection Box, Transform Box, Anchor Point, Bezier Handle, Marquee, Shape Selection (two-tone hatch), Gradient Transform, Snap Ring, Motion Path, Motion Path Handle, Shape Hint |

### Additions after the v2.2 export

The v2.2 export in `interface/design-system/` (2026-11-02) does not yet contain these. They live in Figma on the shelf
"Timeline & canvas answers (R-006…R-010) · selection contrast" (page `v2 · System & Components`, node `116:797`) and
in `neoflash-ui`. They appear in the generated reference with the next export.

| Component | Rule |
| --- | --- |
| `NF2/Shape Selection` (changed) | Two-tone 45° hatch: 1 pt white at 90 % plus 1 pt black at 75 %, 1.5 pt apart, repeating every 4 pt (both measured horizontally; ≈ 1.06 pt and 2.83 pt perpendicular to the stripes; R-058, was 8 pt). It does not follow the theme. On any fill, the better of the two lines reaches at least 3.8:1 (the v2.2 single grey hatch reached 1.5:1 on blue artwork in Dark and 1.1:1 in Light). |
| Figma file structure v2.2 (T-123, 2026-11-07) | Live pages are `v2.2 · System & Components`, `v2.2 · Panels`, `v2.2 · Interface — Dark` / `— Light` (the former v2 pages renamed, so node IDs and the export manifest stay valid). `v2 · … (archive)` pages are full clones kept for comparison; they reference the live components and styles. New boards `v2.2 · Timeline density, type & selection (T-101)` on `v2.2 · Panels`: Dark `211:28634`, Light `212:29343`. Re-export before relying on `exports/` or `docs/data/` for these changes. |
| Density profiles (new, owner decision 2026-11-07) | Comfortable / Compact / Classic Flash change chrome sizes only: top bar 48/40/32, dock tabs and panel headers 40/32/28, section headers 32/28/24, fields and buttons 32/28/24, rows 28/24/20, tool buttons 32/28/24 (tool icons 20/20/16), Tools rail 56/48/40, Timeline controls 44/40/32, frame ruler and layers header 32/26/22, default layer rows 28/22/16, layer column 372/332/292 (with Mode and Solo), Inspector 320/288/256, status bar 24/22/20, minimum target 32/28/24. `neoflash_ui::metrics` keeps the Comfortable constants and exposes density functions (`control()`, `row()` …). Switch: Preferences ▸ General ▸ Density (live preview) and Window ▸ Density; stored in workspace preferences, default Comfortable (R-051). |
| Type scale B (changed, owner decision 2026-11-07) | Professional-tool scale, one table in `tokens.json` / `typography.css`: Heading 14/20, Dialog Title 14/20, Title 12/16 (600; panel titles such as Properties, Color, Library), Dialog Body 12/16, Body 11/14, Body Strong 11/14, Small 10/13, Small Strong 10/13, new Caption 10/12, Overline 10/12, Mono 11/14, Mono Small 10/12 (was 20/18/16/14/13/13/12/12/–/11/12/11). Row, control and panel heights are unchanged; density profiles are R-051. Authored stage text and the user-set code editor font are not UI type. |
| `NF2/Icon Button` Type=Primary (changed) | Square like every icon button (radius md), `accent/default` fill; the round shape is retired. The Timeline Play is Primary **SM 32 pt** inside the 2 pt transport group (R-052, 2026-11-07). |
| `NF2/TL Layer` heights (changed) | Three presets: Normal 28 pt with `NF2/Body`, Short 22 pt with `NF2/Small`, Compact 16 pt with the new `NF2/Caption`; Short and Compact use 12 pt icons; toggle columns stay 20 pt (R-052). |
| `NF2/TL Layer` Narrow (new) | Below a 280 pt layer column the 64 pt `Mode` cell becomes a 20 pt cell with a 12/16 pt `mixer` glyph in `accent/default`, shown only for non-Normal blend modes (and on hover); the header hides the "Mode" label (R-052). |
| Timeline controls bar (changed) | No clipping or hidden scrolling: controls hide by priority (zoom slider, Duration / Center playhead, Scene, Folder…), the right-end More ⋯ turns Selected and lists them under "Hidden controls" (grouped into Layers / Playback / Frames and tweens / Onion skin / View submenus beyond 8 rows). Add layer, transport and More are always shown; transport-over-frame-1 alignment applies only when everything fits. Frame view adds Fit selection / Fit content (R-052). |
| `NF2/Shape Hint` | 16 pt disc with a letter a–z. Unplaced = `danger/default`, Start = `warning/default`, End = `success/default`, with `text/on-accent` for the letter and outline. Colours are pinned to the Dark token mode (R-009). |
| `NF2/Motion Path Handle` | `color/guide` tangent line and a 7 pt disc. Hover adds a ring; Drag uses `color/guide-selected` (R-008). |
| `NF2/TL Span · Sound — decoded peaks` | One mirrored 1 pt column per point in `success/default`; stereo uses two lanes; while not yet decoded, a 1 pt centre line at 50 % (R-010). |
| Paste motion special dialog | Checkbox grid of properties, two override options, Select none · Cancel · **Paste** (R-006). |
| Properties — Classic tween | Rotate × count + Direction, then Orient to path, Snap to motion guide, Sync graphic symbols and Scale (R-007). |

Shared pieces of the 5 November 2026 answers (R-022, R-032, R-033, R-035, R-036, R-038; page `v2 · Panels`, frames
`124:18100` Dark / `132:19052` Light) and the v2.2 shelf "Brushes & tool flyouts" (`96:793`):

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
| `tools.rs` | Tools (2 columns; 1 column with "…" overflow when narrow) | `panels/*/png/panel-tools-floating.png` |
| `properties.rs` | Properties | `interface/*/png/editor-main.png`, `panels/*/png/panel-properties-*.png` |
| `library.rs` | Library | `panels/*/png/panels-more.png` |
| `actions.rs` | Actions, Output, bottom-panel dispatch | `interface/*/png/editor-actions-tab.png`, `panels/*/png/panel-actions-floating.png`, `panels-more-2.png` |
| `accessibility.rs` | Accessibility | `panels/*/png/panels-more-2.png` |
| `planned.rs`, `planned_panels.rs`, `planned_motion.rs`, `planned_dialogs.rs` | 39 read-only planned previews (panels, dialogs, a Test Movie window, the Symbols & scenes popover), opened from their menu items / buttons and from Window ▸ Planned panels | the remaining `panels/*/png/*.png` |
| `../authoring_modify.rs` | Align, Transform, Color panel bodies | `panel-align.png`, `panel-transform.png`, `panel-color-*.png` |
| `../symbol_dialog.rs`, `../app.rs` (`draw_dialogs`) | Convert to Symbol, Unsaved changes, About | `interface/*/png/editor-*-dialog.png` |

### Workspace templates

Window ▸ Workspace and the top-bar select switch between **Classic** (NF2 default), **Animator** (tall timeline,
Transform beside Properties), **Designer** (Color, Align, Transform open), **Developer** (tall Actions with Output) and
**Essentials** (one-column tools, compact dock). Templates replace the layout and close floating panel windows; theme,
document and history are untouched. "Reset … workspace" re-applies the current template.

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

### Design findings to resolve in Figma

| Finding | Detail |
| --- | --- |
| Light `warning/default` (`#b26a00`) on `bg/raised` is 4.24:1 | Below the 4.5:1 text rule. Warnings are never colour-only (icon + text); the editor accepts ≥ 4.2 until the token is darkened. |
| Idle scrollbar thumb (`text/disabled` on `bg/hover`) ≈ 2:1 | The NF2 Technical shelf shows a quiet 4 pt floating thumb; hover and drag reach ≥ 3:1. |
| Library is 100 pt tall in `editor-main` | Shows only the header and search; users drag the splitter for the list. Stacked groups never shrink below their header + first row. |
| §3.2 says "field rows gap 8", outlines use 12 in the inspector | The editor follows the outlines (inspector 12, Library 8). |

## Verifying

```bash
cargo run -p neoflash-ui --example gallery -- dark target/ui-shots/gallery-dark.png
```

```bash
NEOFLASH_WINDOW=1920x1080 NEOFLASH_SCREENSHOT=target/ui-shots/editor-dark.png NEOFLASH_SCREENSHOT_THEME=dark cargo run -p neoflash-editor
```

| Variable | Effect |
| --- | --- |
| `NEOFLASH_SCREENSHOT=path.png` | Save a PNG of the main window after the window settles, then quit. Saved layouts are ignored. |
| `NEOFLASH_SCREENSHOT_THEME=dark\|light` | Theme for the capture. |
| `NEOFLASH_SCREENSHOT_WORKSPACE=classic\|animator\|designer\|developer\|essentials` | Workspace template for the capture (e.g. `developer` shows Actions). |
| `NEOFLASH_WINDOW=1920x1080` | Fixed window size (leaves a restored full-screen state first); matches the Figma screens. |
| `NF2_GALLERY_SECTION=planned` | Gallery scrolls to the planned-panel components. |
| `NEOFLASH_SCREENSHOT_PLANNED=Ease,Debugger\|all` | Open the listed planned previews side by side for the capture. |

Compare with `interface/design-system/exports/interface/{dark,light}/png/editor-main.png` (3840 × 2160 = 1920 × 1080 @2x;
crop the same region of both, e.g. with `ffmpeg -i ref.png -i cur.png -filter_complex
"[0]crop=W:H:X:Y[a];[1]crop=W:H:X:Y[b];[a][b]vstack" out.png`). A screenshot comparison is not proof of keyboard,
docking or accessibility behaviour; those stay covered by the editor tests. The shared acceptance scenarios
(1024 × 720 and 1920 × 1080, Light and Dark, keyboard focus, dense and empty content, detach → dock → reopen,
Retina scaling) apply to every delivery batch.
