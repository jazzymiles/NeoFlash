# NF2 design requests

Implementation contributors must not edit the NF2 design system (`interface/`, `crates/neoflash-ui/`,
`docs/DESIGN-SYSTEM.md`) or change how a panel looks — see AGENTS.md › "Design system (NF2) — locked".
When a task needs something the design does not provide, add a request here and continue with the parts
that do not depend on it. The design track answers each request, updates Figma and `neoflash-ui`, re-pins
the design lock and marks the request as resolved.

## How to write a request

Copy the template, give it the next number, keep it short and concrete.

```markdown
### R-000 · <short title>
- **Status:** open
- **Requested by / for:** implementation track · <work item from docs/NF2-WIRING-PLAN.md>
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

### R-033 · Actions diagnostic peek, fix selection and debugger connection states
- **Status:** open
- **Requested by / for:** implementation track · AC-03/AC-08 (Actions tooling)
- **Where:** `sources.rs::script_workspace`, Actions Debug options, planned Debugger panels
- **Need:** NF2 components/states for the error peek and source-range underline; selection among several quick fixes;
  debugger disconnected/unsupported states and binding of breakpoint/call-stack/locals views to a live session.
- **Why:** structured severity/code/range/fixes and exact handler source maps are available. Existing controls can
  display warnings and apply a unique safe ⌘. fix, but cannot present choices or claim a connected debugger.
- **Checked:** `panels-panel-actions-floating.txt` Error peek / "Quick fix ⌘." / overview ruler; `docs-code-editor.txt`;
  `neoflash-ui/src/components/code.rs`; `docs/NF2-ACTIONS-EDITOR.md`.
- **Proposal (optional):** reuse the approved Menu Item presentation for fix choices; specify the peek/range and
  debugger connection states before implementation. Debug protocol remains engine work, independent of this request.


### R-001 · Ruler pointer-position marker
- **Status:** open
- **Requested by / for:** implementation track · W0.4 rulers (ST-01 "pointer position marker")
- **Where:** `stage_view.rs › stage_rulers`, `NF2/Stage Ruler`
- **Need:** a marker state of `NF2/Stage Ruler` showing the pointer's position on both rulers (colour, length).
- **Why:** spec ST-01 lists it; the ruler shelf has no marker variant.
- **Checked:** `docs-rulers-guides-grid.txt`, `editor-main.png` (no marker shown).
- **Proposal (optional):** 1 pt `color/guide` tick across the ruler.

### R-002 · Exact guide position (Move guide)
- **Status:** open
- **Requested by / for:** implementation track · W1.2 guides (ST-02 "double-click → exact position")
- **Where:** guide double-click on the stage
- **Need:** a small popover or dialog with an axis-labelled position field and Remove / Cancel / OK.
- **Why:** dragging is limited to whole pixels at the current zoom; Flash offers a Move Guide dialog.
- **Checked:** reference §3, §6 "View settings", panels exports — no such dialog.

### R-003 · View ▸ Snapping submenu and object / pixel snapping
- **Status:** open
- **Requested by / for:** implementation track · W1.3 snapping (ST-04, TO-06, MN-03)
- **Where:** `panels/menu.rs › view_menu` (Snapping is a disabled submenu), tools rail snap toggle
- **Need:** the Snapping submenu items (order, labels, shortcuts) and whether Snap to grid / guides repeat there;
  the tools-rail snap toggle placement.
- **Why:** guide and grid snapping work through Grid ▸ / Guides ▸; object and pixel snapping have no entry point.
- **Checked:** `panels-menus.txt` (Snapping row only, no submenu frame), §8.

### R-004 · Lock guides shortcut and View settings colour swatch
- **Status:** open
- **Requested by / for:** implementation track · W1.2 guides
- **Where:** View ▸ Guides ▸ Lock guides; `NF2/View Settings` colour fields
- **Need:** confirm ⌥⌘; (spec ST-02; implemented, not shown on the menu row) and which picker the colour-field
  swatch opens (hex and alpha are editable today; the swatch also shows the alpha checkerboard where the reference
  draws the colour opaque).
- **Why:** the menu row shows no shortcut; clicking the swatch does nothing yet.
- **Checked:** `panels-menus.txt`, `view-settings.png`.

### R-011 · Canvas chrome on artwork: rotated boxes, contrast, Light transformation point, cursor map
- **Status:** open
- **Requested by / for:** implementation track · ST-07, ST-11
- **Where:** `neoflash_ui::components::canvas::{paint_selection_box, paint_transform_box, transformation_point}`,
  `neoflash_ui::cursors`; used by `crates/neoflash-editor/src/stage_selection.rs` and `stage_cursor.rs`
- **Need:** (1) a rotated/skewed (quad) variant of `Selection Box` and `Transform Box`; today rotated objects get the
  axis-aligned box of their bounds. (2) A contrast rule over artwork: the 1 pt `accent/default` line has no halo and
  disappears on blue or busy artwork (the old editor overlay used a halo pair). (3) The transformation point is
  `#FFFFFF` with a `bg/canvas` outline, nearly invisible on light artwork in the Light theme. (4) A per-handle cursor
  table (rotate zone size outside corners, skew vs scale hit areas); the editor follows Flash: handles scale, the edge
  between handles skews, up to 18 pt outside a corner rotates.
- **Why:** the boxes and cursors are now wired; these are the remaining visual decisions the editor must not improvise.
- **Checked:** `canvas-editing-chrome.png` (Dark only), `cursors.png`, design doc §3.5 and the canvas chrome catalogue.
- **Answer:**

### R-012 · Colour effect styles (Brightness / Tint / Advanced) and Properties Swap… symbol picker
- **Status:** open
- **Requested by / for:** implementation track · W0.6 (PR-04), W0.15 (PR-06)
- **Where:** `crates/neoflash-editor/src/panels/properties.rs` (symbol instance inspector) and `stage.rs` object context menu (`Swap symbol…`, `Duplicate symbol…`)
- **Need:** (1) NF2 row composition in the Properties instance inspector for Colour effect `Style` (`None` / `Brightness` / `Tint` / `Advanced` / `Alpha`) and its per-style value rows (Tint colour + %, Advanced 8 multiplier/offset fields). (2) NF2 row/button placement in Properties for `Swap…` plus the Swap symbol picker dialog or popover (Library row/⋯ `Swap symbol` already works when an instance is selected, and `Duplicate symbol` works in the Library).
- **Why:** `Transform.color_transform` and `Editor::swap_instance_symbol` / `duplicate_symbol` exist, but `panels/properties.rs` currently contains only the Opacity row and no Swap… button or symbol-picker dialog preview; adding new rows or a picker dialog without a locked NF2 composition would violate the design lock.
- **Checked:** `panel-properties-movie-clip.png`, `panels/properties.rs`, `panels/planned_*.rs` (no Swap symbol dialog preview exists).
- **Answer:**

### R-013 · Preferences ▸ General ▸ "follow system" control
- **Status:** open
- **Requested by / for:** implementation track · DL-01
- **Where:** `crates/neoflash-editor/src/panels/preferences.rs` (`prefs_general`), reference `dialog-preferences-*.png`
- **Need:** a selectable state for following the macOS appearance. The reference shows `Dark | Light` and the caption
  "or follow system", which is not a control; a third segment ("System") or a checkbox would make it selectable.
- **Why:** the editor can follow the system theme (egui reports it); without a control the caption stays disabled.
- **Checked:** `dialog-preferences-*.png`, `Segmented` component, design doc Preferences section.
- **Answer:**

### R-014 · Preferences ▸ General ▸ Accent colour, Language and Selection highlight
- **Status:** open
- **Requested by / for:** implementation track · DL-01 / W3.6
- **Where:** `crates/neoflash-editor/src/panels/preferences.rs` (`prefs_general`)
- **Need:** (1) Confirmation on whether `Accent` swatches override `accent_default`/`accent_hover` tokens in `neoflash-ui`
  or stay disabled; (2) localisation policy for `Language` (`English` is the only interface language today); (3) token/role
  for `Selection ▸ Highlight` (`Layer colour` vs accent).
- **Why:** palette accent tokens live in locked `neoflash-ui`, only English strings exist, and stage selection chrome
  currently uses `accent_default` (R-011).
- **Checked:** `panels-dialog-preferences-general.txt`, `dialog-preferences-general.png`, `docs/DESIGN-SYSTEM.md`.
- **Answer:**

### R-015 · Preferences ▸ General ▸ Welcome screen (On launch) and Crash recovery dialog
- **Status:** open
- **Requested by / for:** implementation track · DL-01 / W3.6 (`platform-recovery`)
- **Where:** `crates/neoflash-editor/src/panels/preferences.rs` (`prefs_general`, `On launch`) and `crates/neoflash-editor/src/app.rs` (`"Recover unsaved work?"` prompt on launch)
- **Need:** (1) A Welcome / Start screen frame if `"Welcome screen"` should be offered in `On launch` (today `On launch`
  offers `"New document"`, `"Last document"` and `"Example"`); (2) confirmation or a dedicated layout for the launch crash
  recovery dialog (currently reuses the exact 480 pt `Surface::Dialog` + warning badge + `Discard` / `Restore` footer of
  `"Save changes before continuing?"`).
- **Why:** `panels-dialog-preferences-general.txt` shows `"Welcome screen"` as a sample label with no screen frame in
  exports, and `interface-editor-unsaved-changes-dialog.txt` is the only confirmation modal in NF2.
- **Checked:** `panels-dialog-preferences-general.txt`, `interface-editor-unsaved-changes-dialog.txt`, `exports/panels`.
- **Answer:**

### R-016 · Preferences ▸ General ▸ Show tooltips with shortcuts inside locked NF2 components
- **Status:** open
- **Requested by / for:** implementation track · DL-01 / W3.6
- **Where:** `crates/neoflash-ui/src/components/display.rs` (`tooltip`), `headers.rs`, `view_settings.rs`, `swatches.rs`
- **Need:** a `ctx` flag or helper option read by `neoflash_ui::components::display::tooltip` so setting `Show tooltips
  with shortcuts` to off suppresses the shortcut suffix inside the 6 locked `neoflash-ui` call sites (and on `nf::Button`,
  which currently only shows a tooltip when `.shortcut(...)` is non-empty) as well as editor-supplied tooltips.
- **Why:** editor toolbars and panels already gate `.shortcut(...)` via `Editor::tooltip_sc`, but 6 shortcuts (`⌘W`, `Esc`,
  `I`) are passed inside locked `neoflash-ui` components.
- **Checked:** `crates/neoflash-ui/src/components/display.rs`, `button.rs`, `headers.rs`, `view_settings.rs`, `swatches.rs`.
- **Answer:**

### R-017 · Preferences ▸ Code editor ▸ Custom syntax colour schemes
- **Status:** open
- **Requested by / for:** implementation track · DL-01 / W3.6 (`prefs_code_editor`)
- **Where:** `crates/neoflash-editor/src/panels/preferences.rs` (`prefs_code_editor`) and `crates/neoflash-ui/src/components/code.rs`
- **Need:** (1) An entry point in `neoflash_ui::components::code` accepting custom token colour overrides while enforcing
  the NF2 rule that red is reserved for errors; (2) an interaction spec for clicking a `prefs-syntax` swatch row, naming a
  `Duplicate` scheme, and the JSON schema for `Import…`.
- **Why:** `nf::code::highlight` reads fixed `Palette` `syntax_*` tokens in locked `neoflash-ui`, and the reference shows
  read-only swatches without a picker/rename popover.
- **Checked:** `panels-dialog-preferences-code-editor.txt`, `crates/neoflash-ui/src/components/code.rs`.
- **Answer:**

### R-018 · Preferences ▸ Timeline ▸ Custom Onion skin Before / After tints
- **Status:** open
- **Requested by / for:** implementation track · DL-01 / W3.6 (`prefs_timeline`)
- **Where:** `crates/neoflash-editor/src/panels/preferences.rs` (`prefs_timeline`) and `crates/neoflash-editor/src/stage.rs`
- **Need:** interaction for the `Before` and `After` `SwatchCell` controls (colour picker popover vs cycling NF2 layer
  tokens) and how custom tints adapt between Dark and Light themes in place of `StageOverlay::onion_before` / `onion_after`.
- **Why:** each `SwatchCell` in `prefs_timeline` is a single cell with no picker attached in the reference.
- **Checked:** `panels-dialog-preferences-timeline.txt`, `crates/neoflash-editor/src/theme.rs`.
- **Answer:**

### R-019 · Preferences ▸ Drawing ▸ Pen solid anchor points and Snap align guides
- **Status:** open
- **Requested by / for:** implementation track · DL-01 / W3.6 (`prefs_drawing`)
- **Where:** `crates/neoflash-ui/src/components/canvas.rs` (`paint_anchor`, stage alignment overlay)
- **Need:** (1) A solid-unselected state on `NF2/Anchor Point` (`paint_anchor`) for `Show solid points` that remains
  visually distinct from `AnchorState::Selected`; (2) an NF2 canvas painter/token for transient smart-alignment guide lines
  (`Snap align`).
- **Why:** `AnchorState` only has `Unselected` (hollow), `Selected` (filled) and `Hover`, and NF2 forbids hand-styled stage
  overlay strokes.
- **Checked:** `panels-dialog-preferences-drawing.txt`, `crates/neoflash-ui/src/components/canvas.rs`.
- **Answer:**

### R-020 · Preferences ▸ Shortcuts ▸ Rename shortcut set inline or modal prompt
- **Status:** open
- **Requested by / for:** implementation track · DL-01 / W3.7 (`prefs_shortcuts`)
- **Where:** `crates/neoflash-editor/src/panels/preferences.rs` (`prefs_shortcuts`, `Shortcut set` row)
- **Need:** an inline `TextField` state or small rename popover/modal for the `Rename` button next to the `Shortcut set`
  select (`Duplicate` and `Delete` are wired).
- **Why:** `panels-dialog-preferences-shortcuts.txt` shows only the closed `NF2/Select` and the `Rename` ghost button with
  no editable text field state.
- **Checked:** `panels-dialog-preferences-shortcuts.txt`, `dialog-preferences-shortcuts.png`.
- **Answer:**

### R-021 · Object Drawing (`J`) toggle on the Tools rail and Modify ▸ Combine objects / Shape submenus
- **Status:** open
- **Requested by / for:** implementation track · TO-06, MN-05 (Classic Flash CS4 drawing modes & vector boolean operations)
- **Where:** `crates/neoflash-editor/src/panels/tools.rs` (Options section at bottom of Tools rail) and `crates/neoflash-editor/src/authoring_modify.rs` (`modify_menu`)
- **Need:** (1) an NF2 `Object Drawing` (`J`) toggle icon/button state in the Tools panel `Options` area when shape/stroke drawing tools (`Pen`, `Line`, `Rectangle`, `Oval`, `PolyStar`, `Pencil`, `Brush`) are active, distinguishing Object Drawing from planar Merge Drawing. (2) Menu row composition and dialogs for `Modify ▸ Combine objects` (`Union`, `Intersect`, `Punch`, `Crop`) and `Modify ▸ Shape` (`Convert lines to fills`, `Expand fill…`, `Soften fill edges…`, `Smooth`, `Straighten`, `Optimize…`).
- **Why:** classic Flash vector authoring relies on toggling Merge vs Object Drawing (`J`) and boolean/contour shape operations; neither the Tools rail icon slot nor the `Expand fill…` / `Soften fill edges…` / `Optimize…` parameter dialogs exist in NF2 yet.
- **Checked:** `panels/tools.rs`, `authoring_modify.rs`, `panels/planned_drawing.rs`, `panels-menus.txt`.
- **Proposal (optional):** a 1-icon toggle button in the Tools `Options` footer using `NF2/Tool Button` selected state, plus compact `NF2/Dialog` sheets with `Field` steppers and `Preview` checkbox for `Expand fill…`, `Soften fill edges…` and `Optimize…`.
- **Answer:**

### R-022 · Bitmap properties and Modify ▸ Bitmap ▸ Trace bitmap… modal dialogs
- **Status:** open
- **Requested by / for:** implementation track · LB-04, MN-05 (Classic Flash CS4 bitmap asset inspection & vectorization)
- **Where:** `crates/neoflash-editor/src/panels/library.rs` (bitmap item `Properties…` context menu) and `crates/neoflash-editor/src/authoring_modify.rs` (`Modify ▸ Bitmap ▸ Trace bitmap…`)
- **Need:** (1) `Dialog / Bitmap properties` mirroring `Sound properties` (`panels/sound_properties.rs`): thumbnail preview, dimensions/format summary, `Allow smoothing` checkbox, `Compression` select (`Photo (JPEG)` with quality slider vs `Lossless (PNG/GIF)`), ActionScript linkage (`Export for ActionScript`, `Class`), and `Update` / `Import…` / `Test` / `OK` / `Cancel` buttons. (2) `Dialog / Trace bitmap` with `Color threshold`, `Minimum area` (px), `Curve fit` select (`Pixels` / `Very tight` / `Tight` / `Normal` / `Smooth` / `Very smooth`), `Corner threshold` select (`Many corners` / `Normal` / `Few corners`), and `Preview` / `OK` / `Cancel`.
- **Why:** `Swap bitmap` and `Break apart` (bitmap fill) exist in `bitmaps.rs`, while `Sound properties` already has a locked NF2 dialog, but bitmap library items have no `Bitmap properties` dialog and vectorization (`Trace bitmap…`) has no parameter modal.
- **Checked:** `panels/library.rs`, `bitmaps.rs`, `panels/sound_properties.rs` (`dialogs-sound-stroke.png`), `panels/planned_dialogs.rs`.
- **Proposal (optional):** match the two-column header + `NF2/Dialog` layout of `Sound properties` (`dialogs-sound-stroke.png`) for `Bitmap properties`, and a 4-row `NF2/Dialog` form for `Trace bitmap`.
- **Answer:**

### R-023 · 9-slice scaling checkbox in Symbol properties and stage 9-slice guide overlay
- **Status:** open
- **Requested by / for:** implementation track · DL-04, ST-02 (Classic Flash CS4 `scale9Grid` authoring)
- **Where:** `crates/neoflash-editor/src/symbol_dialog.rs` (`Convert to symbol` / `New symbol` / `Symbol properties`) and `crates/neoflash-editor/src/stage_view.rs` (in-place symbol editing overlay)
- **Need:** (1) placement of the `Enable guides for 9-slice scaling` checkbox (enabled for `MovieClip` and `Button` symbols) inside `symbol_dialog.rs`. (2) Visual token and stroke pattern (e.g. dashed line colour, hover/drag state, bounds clamping indicator) for the 4 `scale9Grid` slice guides when editing a 9-slice symbol, distinct from ordinary `Timeline::guides` solid ruler guides (`docs/NF2-RULERS-GUIDES-GRID.md`).
- **Why:** AS3 `DisplayObject.scale9Grid` exists in runtime/API, and `docs/NF2-RULERS-GUIDES-GRID.md` locks ruler-guide rendering; authoring 9-slice symbols requires a checkbox in `Symbol properties` and a visually distinct dashed guide style.
- **Checked:** `symbol_dialog.rs`, `stage_view.rs`, `docs/NF2-RULERS-GUIDES-GRID.md`, `symbols-panels.png`, `canvas-editing-chrome.png`.
- **Proposal (optional):** place `Enable guides for 9-slice scaling` under the `Advanced` / `Linkage` section of `symbol_dialog.rs`, and draw 9-slice guides as 1 pt dashed `color/guide` lines inside symbol edit mode.
- **Answer:**

### R-024 · Decoded waveform backdrop and Time In / Time Out trim handles in Edit envelope
- **Status:** open
- **Requested by / for:** implementation track · DL-09, TL-13 (Classic Flash CS4 sound envelope & trim editing; complements `R-010`)
- **Where:** `crates/neoflash-editor/src/panels/edit_envelope.rs` (`edit_envelope_dialog`) and `neoflash_ui::components::EnvelopeLane`
- **Need:** (1) extend `neoflash_ui::components::EnvelopeLane` to accept optional decoded per-bucket `[min, max]` waveform peak data (`&[[f32; 2]]`) for the `L` and `R` channel strips (`R-010` covers `SpanType::Sound` on the main timeline). (2) Draggable `Time In` and `Time Out` vertical trim handles on the `Edit envelope` time ruler with dimmed out-of-range regions outside `[in_point, out_point]`.
- **Why:** the engine and playback foundation is now ready — `WaveformPeaks` (`sample_waveform_peaks`, `wav_waveform_peaks`), cached `TimelineAudio::waveform_peaks` (MP3/WAV), and `FrameSound::{in_point, out_point}` playback trimming in `neoflash-core`, `neoflash-editor`, and `web/audio.js` all work — so only the locked `EnvelopeLane` waveform slot and ruler trim-handle chrome are needed to expose visual trimming in `Edit envelope`.
- **Checked:** `panels/edit_envelope.rs`, `dialogs-sound-stroke.png`, `R-010`, `crates/neoflash-core/src/timeline_sound.rs`.
- **Proposal (optional):** pass `peaks: Option<&[[f32; 2]]>` and `trim: Option<(f32, f32)>` into `EnvelopeLane` + a paired handle bar on the 18 pt ruler between the `L` and `R` lanes.
- **Answer:**

### R-025 · Pinnable multi-script tabs (`Pin script`, `⌘=`) in the Actions panel bar
- **Status:** open
- **Requested by / for:** implementation track · AC-01, AC-03 (Classic Flash CS4 multi-script workflow)
- **Where:** `crates/neoflash-editor/src/panels/actions.rs` (`actions_contents` top toolbar / target bar)
- **Need:** (1) a `Pin script` icon button and horizontal pinned-tab strip in the Actions panel header (`Current selection` tab + pinned frame/instance/file tabs with close `×` affordance). (2) Shortcut assignment clarification, since classic Flash uses `⌘=` for `Pin script` while NF2 assigns `⌘=` to `View ▸ Zoom in`.
- **Why:** clicking another object or timeline frame immediately switches the Actions editor away from the active script; pinning tabs lets authors cross-reference and edit multiple frame and object scripts side-by-side without losing selection context.
- **Checked:** `panels/actions.rs`, `panels/actions_sidebar.rs`, `panel-actions-floating.png`, `editor-actions-tab.png`, `scripting-panels.png`.
- **Proposal (optional):** reuse NF2 compact tab pills above the code gutter alongside a pin toggle `IconButton` in `CODE_TOOLS`.
- **Answer:**

### R-026 · Bandwidth profiler & Simulate download in Test movie and the Motion presets panel
- **Status:** open
- **Requested by / for:** implementation track · TM-02, PN-12 (Classic Flash CS4 publishing diagnostics & reusable motion templates)
- **Where:** `crates/neoflash-editor/src/panels/planned_documents.rs` (`test_movie`) and `crates/neoflash-editor/src/panels/menu.rs` (`window_menu`)
- **Need:** (1) in the `Test Movie` surface (`publish-misc-dialogs.png`), a collapsible `Bandwidth profiler` drawer (per-frame byte bar graph, red frame-budget threshold line, document size/preload summary) and `Simulate download` speed profile controls (`DSL`, `Cable`, `Mobile 3G/4G`, custom `KB/s`). (2) `Panel / Motion presets` (`Window ▸ Motion presets`): folder tree of default & custom presets, live animated preview square, and `Apply` / `Save selection as preset…` footer actions.
- **Why:** `neoflash_export::format_size_report(&Project)` (`--size-report`) now computes exact per-scene, per-frame, symbol, font, resource, and script byte breakdowns ready to feed a `Bandwidth profiler` and `Publish settings ▸ Generate size report` checkbox, and the motion-tween engine (`timeline_motion.rs` Copy/Paste motion) can already serialize/apply motion tracks once a `Motion presets` panel design exists.
- **Checked:** `panels/planned_documents.rs › test_movie`, `panels/planned_dialogs.rs › publish_settings`, `publish-misc-dialogs.png`, `panels/menu.rs › window_menu`, `panels/planned_panels.rs`.
- **Proposal (optional):** add a toggleable profiler strip above the `Test Movie` transport bar and follow the `Code snippets` tree + preview split (`scripting-panels.png`) for `Motion presets`.
- **Answer:**

### R-027 · Instance Colour effect parameter rows and Motion editor Colour effect tracks
- **Status:** open
- **Requested by / for:** implementation track · PR-04, PP-02 (Group A core-to-editor wiring)
- **Where:** `crates/neoflash-editor/src/panels/properties.rs` (instance Colour effect section) and `crates/neoflash-editor/src/panels/motion_editor.rs` (`Colour effect` section `+` menu)
- **Need:** exact NF2 row layout and field ranges for each `ColorTransform` style (`Brightness` `-100..=100 %`, `Tint` RGB colour swatch + `0..=100 %` tint weight, `Advanced` 2-column `R/G/B/A × %` + `R/G/B/A + offset` `-255..=255` grid), plus the corresponding `MotionRow` sub-tracks under `Colour effect` in `Motion editor` (`TweenProperty::Color`).
- **Why:** `Transform.color_transform`, classic/motion `ColorTransform` interpolation (`evaluate.rs`), and `.fla` colour-effect import are complete in `neoflash-core`, while `properties.rs` exposes only the `Opacity` slider (building on `R-011`).
- **Checked:** `panel-properties-movie-clip.png`, `motion-panels.png`, `panels/properties.rs`, `panels/motion_editor.rs`.
- **Proposal (optional):** `c::Select` style picker above a conditional 1-row (`Brightness` / `Alpha`), 2-control (`Tint`), or 4×2 `c::Field` (`Advanced`) sub-form matching `SectionHeader("Color effect")`.
- **Answer:**

### R-028 · Authoring Strings localization panel (XLIFF) and Text String ID binding
- **Status:** open
- **Requested by / for:** implementation track · Group A (`mx.lang.Locale` authoring UI)
- **Where:** `Window ▸ Strings` (new utility panel) and `crates/neoflash-editor/src/panels/properties.rs` (`TextBox` inspector)
- **Need:** (1) NF2 layout for a `Strings` panel (`Active language` and `Default language` selects, Add/Remove language dialog, `ID | Language…` table, and Import/Export XLIFF 1.0 actions). (2) A `String ID` field row in `Properties` for `Dynamic` / `Static` `TextBox` elements.
- **Why:** the AS2/AS3 `mx.lang.Locale` runtime parser and XLIFF 1.0 loader already work in `neoflash-script`, and `Project.resources` can bundle `.xml`/`.xliff` assets, but there is no NF2 panel or `TextBox` inspector slot to author string tables visually.
- **Checked:** `panels-menus.txt` (§8 `Window`), `panels/planned_*.rs`, `panel-properties-text` outlines — no `Strings` panel preview exists yet.
- **Proposal (optional):** utility panel using `c::Select`, `c::Search`, `c::TableRow`, and an `IconButton` footer (`+`, `Trash`, `Import`, `Export`).
- **Answer:**

### R-029 · Complete TextBox Properties inspector (text kind, line mode, character & paragraph metrics)
- **Status:** open
- **Requested by / for:** implementation track · PR-07 (Group A `TextBox` model-to-UI parity)
- **Where:** `crates/neoflash-editor/src/panels/properties.rs` (`TextBox` selection inspector)
- **Need:** NF2 section/row composition for the remaining `TextBox` / `TextFormat` properties: text kind (`Static` / `Dynamic` / `Input`), line mode (`Single line` / `Multiline` / `Multiline no wrap` / `Password`), toggles (`Selectable`, `Show border`, `Auto-size`, `Auto kern`), fields (`Max chars`, `Variable`, `Letter spacing`, `Line spacing`, `Left margin`, `Right margin`, `Indent`, `URL`, `Target`).
- **Why:** runtime `TextField` / `TextFormat` and legacy `.fla` text import already decode and execute these attributes, whereas `properties.rs` currently exposes only `Content`, `Font`, `Size`, `Style` (B/I), `Color`, and `Align`.
- **Checked:** `panels/properties.rs`, `panels/planned_*.rs`, `docs/NF2-FUNCTIONAL-SPEC.md` PR-07.
- **Proposal (optional):** `Type` + `Behavior` rows at the top of `Character`, `Letter spacing` + `Auto kern` under `Character`, `Margins` / `Indent` / `Line spacing` 2×2 `c::Field` grid under `Paragraph`, and collapsible `Options` (`URL`, `Target`, `Max chars`).
- **Answer:**

### R-030 · Extended per-filter parameter sub-forms and presets in Panel / Filters
- **Status:** open
- **Requested by / for:** implementation track · PR-05 (`panels/filters.rs`)
- **Where:** `crates/neoflash-editor/src/panels/filters.rs` (`Filters` section parameter area)
- **Need:** locked NF2 sub-form compositions for non-Blur filter parameters: colour swatch wells (`Drop shadow`, `Glow`, `Bevel` highlight/shadow), `Angle` (`°`) & `Distance` (`px`) row, boolean checkboxes (`Inner`, `Knockout`, `Hide object`), 4-field `Adjust color` (`Brightness`, `Contrast`, `Saturation`, `Hue`), 15-stop gradient bar for `Gradient glow` / `Gradient bevel`, and a `Presets` menu button.
- **Why:** `PanelId::Filters` (`Window ▸ Filters`) is now live with all 7 Flash filter types (`Drop shadow`, `Blur`, `Glow`, `Bevel`, `Gradient glow`, `Gradient bevel`, `Adjust color`) mapped into the locked 2×2 parameter grid from `panel-properties-movie-clip.png`; exposing the remaining per-filter controls requires approved NF2 rows beyond those 4 slots.
- **Checked:** `panel-properties-movie-clip.png`, `panels/planned_panels.rs` (`properties_filters`), `panels/filters.rs`.
- **Proposal (optional):** adaptive 2-column `c::Field` / `c::ColorSwatch` / `c::Checkbox` rows per selected `FilterSpec` variant inside `SectionHeader("Filters")`.
- **Answer:**

### R-031 · Dynamic focal-point handle and anisotropic radii on Gradient Transform canvas overlay
- **Status:** open
- **Requested by / for:** implementation track · TO-04 (`GradientTransform` tool)
- **Where:** `neoflash_ui::components::canvas::paint_gradient_transform` (`crates/neoflash-ui/src/components/canvas.rs`) and `crates/neoflash-editor/src/stage.rs`
- **Need:** extend `paint_gradient_transform` (or add `paint_radial_gradient_transform`) to accept `focal_point_ratio: Option<f32>` (`-1.0..=1.0`, omitted for linear gradients) and separate `(radius_x, radius_y)` ellipse parameters instead of the hardcoded decorative focal marker at `Vec2::new(radius * 0.35, 0.0)`.
- **Why:** `GradientFill.focal_point` (`-1.0..=1.0`), `GradientMatrix` (`scale_x`, `scale_y`, `rotation`), and HTML5 two-circle `createRadialGradient(fx, fy, 0, 0, 0, r)` already work end-to-end, but `crates/neoflash-ui/` is design-locked so the stage overlay cannot move the focal triangle handle dynamically until `neoflash-ui` exposes the parameter.
- **Checked:** `crates/neoflash-ui/src/components/canvas.rs`, `canvas-editing-chrome.png`, `crates/neoflash-editor/src/stage.rs`.
- **Proposal (optional):** `paint_gradient_transform_with_focal(painter, p, center, rx, ry, angle_rad, focal_point: Option<f32>)`.
- **Answer:**

### R-032 · Publish size scale with weight breakdown in Publish settings
- **Status:** open
- **Requested by / for:** implementation track · DL-03 (Publish settings, HTML5 target)
- **Where:** `crates/neoflash-editor/src/panels/planned_dialogs.rs` (`publish_settings`, `Runtime` group and footer `Estimated size`)
- **Need:** (1) a four-stop size scale in the HTML5 `Runtime` group: `Full` · `Balanced` · `Compact` · `Shared runtime`, with a selected stop, a `Custom` state when an individual option differs, and a runtime URL field shown only for `Shared runtime`. (2) a stacked weight bar under it with segments `WebAssembly`, `Runtime scripts`, `Project scripts`, `Content`, `Resources`, `Fonts`, `Page`, two totals (`on disk` and `transfer, gzip`), an optional budget line (for example 150 KB) and a computing state while the estimate runs. (3) a short trade-off line per stop (for example "Needs HTTP(S) hosting", or "Left out: AIR desktop APIs; …") and a warning style when a left-out family could matter.
- **Why:** `neoflash_export::estimate_publish_sizes` returns each level's files, bytes, gzip bytes, per-part breakdown, trade-offs and left-out modules, and `export_project_with_options` publishes any level. The current dialog has only a static `WASM: optimised (−O3)` select and an `Estimated size` footer string, so the user cannot see or choose the trade-off.
- **Checked:** `panels/planned_dialogs.rs › publish_settings`, `publish-misc-dialogs.png`, docs/decisions/0038-publish-size-levels.md.
- **Proposal (optional):** replace the `WASM: optimised (−O3)` select with an NF2 segmented control, so the footer's `Estimated size` mirrors the selected stop.
- **Answer:**

## Resolved requests

### R-006 · Paste motion special dialog
- **Status:** resolved
- **Requested by / for:** implementation track · MN-02 frame menu (Copy / Paste motion)
- **Where:** frame menu ▸ Paste motion special… (`panels/timeline.rs › frame_context_menu`, disabled)
- **Need:** a dialog choosing which properties to paste (X/Y, scale, rotation and skew, colour, filters, blend,
  override target scale / rotation, ease) with OK / Cancel.
- **Why:** Copy motion and Paste motion work; the "special" variant has no dialog in the design.
- **Checked:** reference §8 (Frame menu), `panels-menus.txt`, `planned_dialogs.rs` — no Paste motion special dialog.
- **Proposal (optional):** `NF2/Dialog` with a column of `NF2/Checkbox` rows like the other planned dialogs.
- **Answer:** Figma node `118:830` (Dark) / `118:987` (Light), shelf "Timeline & canvas answers". It is an `NF2/Dialog`
  440 pt wide, built from `NF2/Dialog Header` "Paste motion special". Body (24 pt side padding, 12 pt gap): overline
  PROPERTIES, then two columns of `NF2/Checkbox` (10 pt row gap). Left column: X position, Horizontal scale, Rotation
  and skew, Filters, Ease. Right column: Y position, Vertical scale, Colour, Blend mode. All are checked by default,
  and the dialog keeps the last choice for the session. Then overline OPTIONS with two unchecked boxes: Override
  target scale properties, Override target rotation and skew properties. With an override on, the target keeps its
  own scale / rotation and the copied change is added on top. Footer: Ghost "Select none" on the left, Secondary
  Cancel, Primary **Paste**. Enter = Paste, Esc = Cancel, and Paste is disabled while no property is checked. Open it
  from the Frame menu and from Edit ▸ Timeline ▸ Paste motion special…. It is composed only of existing components,
  so `neoflash-ui` needs nothing new.

### R-007 · Orient / Snap rows in the classic Tween section
- **Status:** resolved
- **Requested by / for:** implementation track · TL-01, PR-10 (WIRING-PLAN §7.4 row 5)
- **Where:** `panels/properties.rs` frame Tween section for classic tweens on a guided layer
- **Need:** rows for Orient to path and Snap (to motion guide), as Flash's classic tween properties show them.
- **Why:** `Keyframe.orient_to_path` and `Editor::set_orient_to_path` work, but the classic Tween section has no slot.
- **Checked:** `panel-properties-frame` reference and the Motion tween section, which has these rows.
- **Proposal (optional):** reuse the Motion tween section's Orient checkbox row.
- **Answer:** Figma node `118:913` (Dark) / `118:1067` (Light). This is the classic-tween form of the frame Tween section. After
  the Easing select comes a row with `NF2/Stepper Field` "Rotate" (count, "0 ×") and the `NF2/Select` "Direction:
  Auto / CW / CCW / None", each set to Fill, the same pair as the Motion tween ROTATION row. Then one `NF2/Checkbox`
  per row: **Orient to path**, **Snap to motion guide**, Sync graphic symbols, Scale. Labels are identical to the
  Motion tween panel. Orient and Snap are enabled only when the layer has `motion_guide`. Otherwise they use the
  standard disabled state with the tooltip "Needs a motion guide layer". Controls whose model field does not exist
  yet keep "Not available yet".

### R-008 · Bézier handles for motion path editing
- **Status:** resolved
- **Requested by / for:** implementation track · TL-05 (WIRING-PLAN §7.4 row 9)
- **Where:** stage motion path overlay (`NF2/Motion Path`)
- **Need:** a curve-handle style (tangent line + handle dot, hover/drag states) to bend the path between ◆ keys.
- **Why:** paths are edited only at their ◆ points today.
- **Checked:** R-005 answer (◆ handles only), `canvas.rs`.
- **Answer:** New `NF2/Motion Path Handle` (Figma component set `116:821`, States Default / Hover / Drag;
  `neoflash_ui::components::canvas::{paint_motion_handle, motion_handle_hit, MotionHandleState}`). A 1 pt
  `color/guide` tangent line runs from the ◆ to the handle. The handle is a 7 pt `color/guide` disc with a 1 pt
  `bg/canvas` outline. Hover adds a 13 pt ring; Drag fills `color/guide-selected`. Hit radius is 7 pt and the cursor is
  `arrow-reshape-curve`. Show both handles of the selected ◆ and the handle of each neighbour that faces it, and paint
  handles before `paint_motion_key`. Drag keeps the two handles mirrored (smooth), ⌥-drag breaks the tangent
  (corner), and ⌥-click on a ◆ toggles corner / smooth. A segment without handles stays straight, as in R-005.
  Frame dots follow the curve (sample `117:892`). The data model is an engine decision; WIRING-PLAN §7.4 row 9 plans
  `Keyframe.motion_path` with arc-length sampling in `sample_motion`.

### R-009 · Shape hint markers
- **Status:** resolved
- **Requested by / for:** implementation track · TL-04 (WIRING-PLAN §7.4 row 10)
- **Where:** stage overlay on shape-tween keyframes; View ▸ Show shape hints, Modify ▸ Shape ▸ Add shape hint
- **Need:** the lettered shape-hint marker (a–z) with unplaced / start (yellow) / end (green) states.
- **Why:** shape hints pin the morph correspondence; there is no marker component.
- **Checked:** design system reference, `canvas.rs`.
- **Answer:** New `NF2/Shape Hint` (Figma component set `116:807`, property `Letter`, States Unplaced / Start / End;
  `neoflash_ui::components::canvas::{paint_shape_hint, shape_hint_hit, ShapeHintState, SHAPE_HINT_RADIUS}`). It is a
  16 pt disc with a 1 pt outline and a letter a–z in Inter Semi Bold 10. Unplaced = `danger/default` (red),
  Start = `warning/default` (yellow, start keyframe, matched), End = `success/default` (green, end keyframe, matched).
  The letter and outline use `text/on-accent`. Colours stay on the Dark token mode in both themes, because they sit on
  artwork. Hit radius is 8 pt. Drag places a hint; dragging it off the stage removes it. Letters follow creation
  order, with at most 26 hints. Hints show only on the shape-tween keyframes of the current layer while View ▸ Show
  shape hints (⌥⌘H) is on. Add with Modify ▸ Shape ▸ Add shape hint (⇧⌘H); Remove all hints is in the same submenu.
  See sample `117:863`.

### R-010 · Sound span painter with real waveform peaks
- **Status:** resolved
- **Requested by / for:** implementation track · TL-07, TL-13 (WIRING-PLAN §7.4 row 13)
- **Where:** `neoflash_ui::components::timeline::paint_span(SpanType::Sound)`
- **Need:** a variant that takes decoded peak values instead of the decorative waveform.
- **Why:** Sound spans should show the actual sound.
- **Checked:** `timeline.rs` span painter, `timeline-states-legend.png`.
- **Answer:** New `neoflash_ui::components::timeline::paint_sound_peaks(painter, ctx, rect, channels)` (Figma section `117:930`).
  Paint it over the Sound span from `paint_span`. Each channel is a slice of per-point `(min, max)` in −1…1. One
  mirrored 1 pt `success/default` column is drawn per screen point, with height = peak × (lane height / 2 − 2). Stereo
  uses two lanes (L top, R bottom) 1 pt apart. An empty `channels` (not decoded yet) draws a flat 1 pt centre line
  at 50 %. Compute peaks off the UI thread, cache them per resource and zoom, and never decode while scrubbing. The
  decorative waveform in `paint_span` remains only for the gallery and the timeline legend.

### R-005 · Motion path on the stage
- **Status:** resolved
- **Requested by / for:** implementation track · TL-05 Motion tween, PR-10
- **Where:** `crates/neoflash-editor/src/stage.rs` overlay for the selected motion-tweened instance
- **Need:** an NF2 painter for a motion tween path: polyline through the per-frame positions with frame dots, ◆ markers at property keyframes, selected/hover states, and a drag handle style for editing path points.
- **Why:** motion tweens work (independent tracks, ◆ keys) but the path is invisible on the stage; drawing it with hand-styled egui strokes is not allowed.
- **Checked:** design system reference §5 (Timeline) and §9 (Stage overlays), `exports/interface/*/png/editor-main.png`; no motion-path component in `neoflash-ui`.
- **Proposal (optional):** reuse `color/guide` for the line and the `TL Marker` ◆ glyph at keyframes.
- **Answer:** `neoflash_ui::components::canvas` gains `NF2/Motion Path`: `paint_motion_path` (1 pt `color/guide`
  polyline through the per-frame registration points, 1.5 pt `color/guide` dot per frame), `paint_motion_key` (the
  `TL Marker` ◆ glyph, 4.5 pt half-diagonal, `color/guide` fill with a 1 pt `bg/canvas` outline; Hover adds a
  `color/guide` ring 3 pt outside; Selected = dragging, fill `color/guide-selected`) and `motion_key_hit` (7 pt radius).
  The ◆ itself is the drag handle (Grab / Grabbing cursor); no separate handle style.


### R-034 · Paint Bucket gap closing and partial-stroke eraser controls
- **Status:** open
- **Requested by / for:** implementation track · TO-07 drawing follow-ups
- **Where:** `authoring_tools.rs::authoring_tool_options`, Tools options and Properties ▸ Tool.
- **Need:** approved gap choices (None/Small/Medium/Large), a partial-stroke eraser choice, scope/limitation text and their placement in the existing tool options.
- **Why:** tested engines are available, but the current Bucket area only describes closed shapes and Eraser lists Whole objects/fills/strokes. Adding entries or controls changes the locked composition. Masked partial erasing and combined fill/stroke effects are currently refused; touched curves are approximated.
- **Checked:** `panels-panel-tools-floating.txt`, `docs-panels-tools-components.txt`, `panels-brushes-pencil-panels.txt`, `panels-tools-flyout-groups.txt`, TO-07 and the existing options body. The v2.2 Brushes/Pencil work has its own scope.
- **Proposal (optional):** existing NF2 Select rows, retaining the current whole-object modes; the design track chooses labels, order and explanation.

### R-035 · Multiple open documents
- **Status:** open
- **Requested by / for:** implementation track · editor workspace/document follow-ups
- **Where:** top bar, File/Window menus and `app.rs::DocumentSession`.
- **Need:** document switcher/tab placement, active/dirty/close states, overflow, and New/Open/Close/Quit transitions.
- **Why:** the existing title and Unsaved badge provide no document selection or per-document close control. Dormant session helpers omit per-document recovery/selection state and cannot safely be exposed as completed tabs.
- **Checked:** `interface-editor-main.txt`, `panels-menus.txt`, `shell.rs` and planned panel/dialog previews (no document-session preview).
- **Proposal (optional):** design track chooses a tab row or Window menu list; retain the existing unsaved prompt for each affected document.

### R-036 · Named workspace presets
- **Status:** open
- **Requested by / for:** implementation track · editor workspace follow-ups
- **Where:** top-bar workspace selector and `shell.rs::workspace_menu`.
- **Need:** Save/Rename/Delete menu order and labels, name-entry/overwrite/delete states, active user-preset label and reset semantics.
- **Why:** the current menu exposes built-in templates and Reset only. The dormant named-presets map stores only DockLayout; a usable preset must also preserve panel visibility and floating/detached layout state.
- **Checked:** `panels-menus.txt`, `interface-editor-main.txt`, Preferences General outline and planned dialog previews (no workspace naming dialog).
- **Proposal (optional):** existing NF2 Menu Item/Select/Dialog components after their composition is approved; built-in templates remain intact.


### R-037 · Stage-grid contrast and dense-spacing visibility
- **Status:** open
- **Requested by / for:** implementation track · ST-03 / MN-03; user-reported invisible grid on the demo scene.
- **Where:** `neoflash-ui/src/components/canvas.rs::paint_stage_grid`, `view_settings.rs::default_grid_color` and their design tokens.
- **Need:** approved default grid contrast over arbitrary authored backgrounds and a defined dense-spacing state for each axis.
- **Why:** Light's dark 14% grid nearly vanishes on the dark demo; Dark's white grid vanishes on white pages. Show toggles correctly repaint, but the result looks unchanged. Either axis below 3 screen points suppresses both axes, including a still-readable axis. Native Light demo became clearly visible using the existing custom colour field; no theme token or painter was changed.
- **Checked:** `docs-rulers-guides-grid.txt`, `panels-view-settings.txt`, ST-03, `NF2-RULERS-GUIDES-GRID.md`, current grid painter and `color/stage-grid`.
- **Proposal (optional):** design-owned background-independent contrast treatment and per-axis major-line fallback at dense zoom; retain custom colour/opacity and separate Show/Snap settings. The design track chooses the approved rendering.
