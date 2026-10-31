# NeoFlash UI Builder (Figma plugin)

Builds the whole NeoFlash Figma file from the screenshots in `../screenshots`:
real frames, auto-layout, components + variants, variables and text styles. No bitmaps.

## Run

1. In Figma Desktop create a **new empty design file** (the plugin refuses to run twice in one file).
2. `Plugins → Development → Import plugin from manifest…` and choose `figma-plugin/manifest.json`.
3. `Plugins → Development → NeoFlash UI Builder`. It takes ~20–60 s; the notifications show the stage.

## Pages it creates

| Page | Content |
|---|---|
| `v1 · Components` | NF1 icon set, buttons, fields, tabs, tree, layer rows, timeline cells/rows/ruler/playhead… (variants + text properties) |
| `v1 · Interface` | Main window, Actions panel, Unsaved-changes dialog, Convert-to-Symbol dialog (instances of NF1 components) |
| `Audit · v1 → v2` | 14 findings with severity, annotated v1 map, principles |
| `v2 · System & Components` | NF2 variables (Dark / Light), text styles, icons, components |
| `v2 · Interface` | Dark, Light, Actions tab, both dialogs (instances of NF2 components) |

## Independence of v1 and v2

* Variables: `NF1 · Tokens` and `NF2 · Tokens` are separate collections.
* Text styles: `NF1/*` and `NF2/*`.
* Components and icons: `NF1/*`, `NF1 Icon/*` and `NF2/*`, `NF2 Icon/*`.
* No v2 node is an instance of a v1 component (and vice versa) — editing one version never changes the other.

## Rebuild

Sources are in `src/`; `./build.sh` concatenates them into `code.js` and syntax-checks it.
