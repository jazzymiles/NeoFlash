# NF2 integration — what changed and how it is structured

This page explains the NF2 design-system integration to implementation agents (Codex) and maintainers: what exists,
where it lives, which decisions were made and what is deliberately unfinished. Rules: [DESIGN-SYSTEM.md](DESIGN-SYSTEM.md)
and AGENTS.md › "Design system (NF2) — locked". Work still to do: [NF2-FUNCTIONAL-SPEC.md](NF2-FUNCTIONAL-SPEC.md) and
[NF2-WIRING-PLAN.md](NF2-WIRING-PLAN.md).

## 1. Summary

- The whole editor chrome is built from one new crate, `crates/neoflash-ui` (the NF2 design system in Rust). It is
  generated from the Figma export in `interface/design-system` and has no dependency on the engine crates.
- The window follows the NF2 main screen: top bar (logo, menus, title + Unsaved badge, workspace select, Test Movie,
  Publish), tools rail, inspector (Properties over Library), full-width bottom dock (Timeline · Actions · Output ·
  Accessibility), status bar; breadcrumb, stage size and zoom control float over the canvas.
- Every existing panel was restyled with NF2 components and moved to one file per panel under
  `crates/neoflash-editor/src/panels/`. Behaviour, undo, selection, shortcuts and accessibility names are unchanged.
- NF2 features the engine does not support are visible but honest: disabled controls with "Not available yet", and
  39 read-only planned previews, opened from their natural menu items and buttons as well as Window ▸ Planned panels.
- New: workspace templates, View menu with zoom commands (⌘= ⌘− ⌘1 ⌘0), macOS fix for detached panels opening full
  screen, screenshot mode for design comparison, design lock for implementation agents.
- The engine crates (`neoflash-core`, `-script`, `-export`, `-import`, `-web`) were not modified.

## 2. Design source → code

| Source (`interface/design-system/`) | Generated / used by | Notes |
| --- | --- | --- |
| `tokens/tokens.json` (colour variables Dark/Light, spacing, radii, text styles) | `neoflash-ui/build.rs` → `tokens::Palette`, `DARK`, `LIGHT`, `space`, `radius`, `TextStyle` | One `Palette` field per Figma variable: `color/bg/panel` → `bg_panel`. |
| `exports/icons/{16,20}/*.svg` (304) | `build.rs` → `icons::i16::*`, `icons::i20::*` | Rendered white with resvg and tinted by token; cached per display scale. |
| `exports/cursors/*.svg` (39) + hotspots from `docs/data/figma-dump.json` | `build.rs` → `cursors::*` | Painted over the canvas (`cursors::show`); not yet used by stage tools. |
| `docs/data/outline/*.txt` | read by people/agents | Exact auto-layout gaps, sizes, texts of every exported frame. Use these, not the PNGs, for numbers. |
| `exports/interface|panels/{dark,light}/{png,svg}` | visual comparison only | Local only (ignored by Git, 105 MB); regenerate with `tools/export-plugin`. |
| Fonts | `crates/neoflash-ui/assets/fonts/` | Inter 4.1 Regular/Medium/SemiBold, Roboto Mono Regular, SIL OFL; licences in the folder and `THIRD_PARTY_NOTICES.txt`. |

Versioned from `interface/`: tokens, icons, cursors, manifests, Markdown docs, outlines, figma-dump, illustrations,
tools (≈ 7.6 MB). Ignored: screen/panel PNG/SVG exports, PDFs, the gallery site, `docs/source/build`, the `.fig` file.

## 3. Editor structure after the change

| Area | File(s) | What it does |
| --- | --- | --- |
| Theme | `theme.rs` | `ThemeMode` (Dark default) installs NF2 visuals; `UiPalette` roles map to NF2 tokens; layout constants are NF2 metrics; `StageOverlay` holds the few values NF2 lacks. |
| Fonts | `fonts.rs` | Adds the bundled NF2 fonts first; authored fonts still load from the system. |
| Window chrome | `shell.rs`, `panels/menu.rs` | Top bar, workspace menu, status bar, stage breadcrumb / size badge / zoom control, View zoom commands, screenshot mode. |
| Docking | `dock_grid.rs`, `detached.rs`, `native_windows.rs` | NF2 dock tabs / panel headers / splitters; floating panel chrome; macOS auxiliary windows for detached panels. |
| Layout model | `workspace.rs` | NF2 default layout, `WorkspaceTemplate`, per-panel body padding, stacked minimum heights; prefs key `neoflash.workspace.v2`. |
| Shared rows | `paint_controls.rs`, `ui_icons.rs` | Inspector rows on NF2 fields with property edit sessions; tool icon roles and menu rows. |
| Panels | `panels/*.rs`, `authoring_modify.rs`, `symbol_dialog.rs`, `app.rs::draw_dialogs` | See DESIGN-SYSTEM.md › Panel files. |

### Defaults that changed for users

- Dark theme by default (an explicit saved choice is kept).
- New layout: the old saved layout (`neoflash.workspace.v1`) is not migrated; the NF2 default replaces it once.
- Timeline: 12 pt per frame by default (was 13), rows 28, layer column 280; onion details moved to a popover.
- Tools: two columns by default; one column with a "…" overflow when the panel is narrow.
- Properties starts with a Selection / Tool segmented control; Stage, Playback and Publish sections.

## 4. Decisions and deviations

| Topic | Decision |
| --- | --- |
| Values from outlines vs. §3 prose | Where they differ, the outlines win (e.g. inspector row gap 12, tree item height 32). |
| Text centring | Single-line labels have no extra leading (`typography::rich` / `format_line`); paragraphs use `rich_block`. Without this, egui draws labels ~2 pt above centre. |
| Scrollbars | NF2 thin floating thumb (4 pt at rest, 6 pt on hover) inside the panel gutter; colours from NF2 text tokens. Properties and Accessibility pad inside their scroll areas so the bar never covers fields. |
| Inspector split | Properties 83 % / Library 17 % as in `editor-main`; Library never smaller than its header + search. |
| Inspector width | Right dock = 320 pt panel + 6 pt resize strip so rows keep the Figma 288 pt width. |
| Docked tools header | The rail has no title; a 14 pt grip strip keeps drag/detach/context menu. |
| Publishing | Publishing runs from the top-bar Publish button and File ▸ Publish; Properties › "Publish settings…" opens the Publish settings preview, as designed. |
| Unsupported features | Disabled with "Not available yet" or shown as planned previews; nothing pretends to work. |
| Actions toolbox / Script navigator | Built from the declaration audit through `scripts/editor-api-index.py`; folder rows use `TreeItem::icon()` (package, class, scene, target overrides from Figma). Details: [NF2-ACTIONS-EDITOR.md](NF2-ACTIONS-EDITOR.md). |
| Detached panels on macOS | Automatic window tabbing is disabled and panel windows are `FullScreenAuxiliary`, so undocking keeps the panel's own size even when the main window is full screen. |

## 5. Verification performed

- `cargo test -p neoflash-editor -p neoflash-ui`: 129 + 2 tests pass (UI tests were updated to NF2 geometry; behavioural assertions kept).
- `cargo clippy -p neoflash-ui -p neoflash-editor --all-targets -- -D warnings` and `cargo fmt --check` for both crates: clean.
- Screenshots at 1920 × 1080 in Dark and Light compared region by region with `editor-main.png` (top bar, inspector,
  timeline controls) and with `editor-actions-tab.png` (Actions toolbar); gallery compared with the component shelves
  and `view-settings.png`.
- Distribution: `scripts/package-macos.sh release` builds `dist/NeoFlash.app` (release binary, web runtime,
  `THIRD_PARTY_NOTICES.txt` with the bundled font licences, ad-hoc signature); the packaged binary was launched in
  screenshot mode at 1920 × 1080 and renders the NF2 main window. Not notarized.
- Not verified here: VoiceOver, IME, mixed-DPI monitors; detaching while the main window is in native full screen
  (the macOS fix is in place but was checked by reasoning and build only); every planned preview against its PNG.

## 6. Known gaps (next work)

- Stage rulers, guides, grid overlay and NF2 stage cursors are not wired yet; their components exist in `neoflash-ui/components/canvas.rs` (planned as a separate work item).
- Planned previews and disabled controls: see NF2-FUNCTIONAL-SPEC.md / NF2-WIRING-PLAN.md.
- Native detached windows still use the dock tab header; the NF2 Floating Panel Header is used for embedded floating panels.
- Library at the default size shows only the header and search (as in the design).
